//! The C9 salvage ladder with a work budget (T-08; D-007, D-041, D-056,
//! D-066, D-072).
//!
//! Every Flate stream is inflated once. A stream that ends `Done` with its
//! Adler-32 verified is `Clean` and keeps no bytes. A damaged one is searched:
//! one input byte at a time is replaced by each of the 255 other values,
//! resuming from a decoder checkpoint taken every 256 input bytes, and a
//! candidate is accepted only when miniz_oxide ends it `Done`, which means its
//! Adler-32 matched. The search never stops at the first accept: it runs until
//! its window is exhausted or its budget is spent, and survivors are told
//! apart by their output bytes. A stream whose zlib header is rejected is
//! searched as zlib first (the header's window is cheap) and read as raw
//! deflate only when that finds no repair.
//!
//! Budgets count work, never time: W is the input consumed by candidate
//! inflates plus 244 per candidate (FR-g5), so the same input and budget give
//! the same result on any machine and at any thread count. `salvage_all` runs
//! in two phases: every stream under `work`, then the per-file deep pool is
//! allocated in a pinned order before any deep search starts.
//!
//! Every edit offset is in the Flate stage's input domain: filters before
//! `/FlateDecode` are decoded first, filters after it are recorded in the
//! [`FilterStage`] and applied by [`SalvageIndex::decoded`].
// T-11b, T-12b and T-14 are the first callers outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};

use lopdf::Dictionary;
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_HAS_MORE_INPUT, TINFL_FLAG_PARSE_ZLIB_HEADER,
    TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{DecompressorOxide, decompress_with_limit};
use sha2::{Digest, Sha256};

use super::{
    DEFAULT_CAP, DecodeError, Filter, InflateStatus, RAW_END_SLACK, StreamClass, classify,
    decode_chain, filters_of, inflate, unpredict_flate, unpredict_flate_lenient,
};
use crate::engine::{Cancelled, SalvageBudget};
use crate::pdf::carver::{Body, CarveReport};
use crate::pdf::model::{ObjId, SalvageGrade};

#[cfg(test)]
mod tests;
mod ttf;

pub(crate) use ttf::TtfLocalizer;

/// A checkpoint is taken every this many input bytes (FR-08).
const CHECKPOINT_EVERY: usize = 256;
/// The fixed W charged per candidate inflate (FR-g5).
const CALL_W: u64 = 244;
/// The error window reaches this far back from the failure offset `k` ...
const ERROR_WINDOW: usize = 4_096;
/// ... and this far under `deep_work` (eng-r1-fr1: the three misses sat
/// 4.6–6.6 KB before `k`).
const DEEP_ERROR_WINDOW: usize = 16_384;
/// ... and this far forward of `k`.
const ERROR_SLACK: usize = 8;
/// An Adler-only search starts after the two zlib header bytes.
const ADLER_FROM: usize = 2;
/// Candidates between two cancellation polls.
const POLL_EVERY: u64 = 4_096;
/// Output room past the expected length in the search's buffers.
const OUT_SLACK: usize = 64 << 10;
/// Adler-32's modulus.
const ADLER_MOD: u32 = 65_521;

/// One byte edit in the Flate-input domain: (offset, old byte, new byte).
pub(crate) type Edit = (usize, u8, u8);

/// How sure a repair is (D-041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grade {
    /// The window was exhausted and exactly one distinct output survived.
    Exact,
    /// An accept was found but the budget ran out first: `searched` of the
    /// `window` candidates were tried and no second distinct output was seen.
    Accepted { searched: u64, window: u64 },
    /// `outputs` (at least two) distinct outputs survived. A raw-deflate
    /// stream has no Adler-32, so this is its best grade whatever `outputs`
    /// is.
    Ambiguous { outputs: u32 },
}

impl Grade {
    /// The grade as the report states it (T-02's `SalvageGrade`).
    pub(crate) fn report(&self) -> SalvageGrade {
        match self {
            Grade::Exact => SalvageGrade::Exact,
            Grade::Accepted { .. } => SalvageGrade::Accepted,
            Grade::Ambiguous { .. } => SalvageGrade::Ambiguous,
        }
    }
}

/// Why a damaged stream was not searched: its raw length is over
/// `max_search_stream` (D-072).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverMaxSearchStream {
    pub raw_len: u64,
    pub limit: u64,
}

/// One Flate stream's salvage outcome. Every `data` is the Flate stage's
/// output with its predictor undone (later filters not applied). Unverified
/// output can hold PNG rows whose filter type is invalid: the predictor is
/// undone up to the first such row and `data` ends there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Salvage {
    /// Inflated `Done` with its Adler-32 verified. Keeps no bytes (D-072):
    /// [`SalvageIndex::decoded`] re-inflates on demand. `decoded_len`,
    /// `class` and `sha256` describe the whole chain's output.
    Clean {
        decoded_len: u64,
        class: StreamClass,
        sha256: [u8; 32],
    },
    /// Decoded to its end but the Adler-32 disagrees and no repair was
    /// found: every byte decoded (up to an invalid predictor row), unverified.
    /// Also a repair whose verified output holds an invalid predictor row:
    /// the rows before it.
    ChecksumMismatch { data: Vec<u8> },
    /// A search found at least one Adler-verified repair. `data` and `edits`
    /// are the survivor first in the pinned order (body edits before trailer
    /// edits, then lowest offset, then lowest replacement byte); `survivors`
    /// holds one edit list per distinct output, in the same order, `edits`
    /// first. `work` is the W the stream spent. `trailer_edit` says the chosen
    /// repair rewrites a byte of the Adler-32 trailer (the body already
    /// decoded to the original); it is T-08's addition to the interface, so
    /// the report can name trailer evidence without re-deriving it.
    /// `adler_rerun` says the localizer's early check refused every candidate
    /// of its window and the repair was found when the window was searched
    /// again under the Adler-32 alone (for a font: the original's own table
    /// checksum was wrong); T-08b's addition, so the report can say so.
    Repaired {
        data: Vec<u8>,
        edits: Vec<Edit>,
        grade: Grade,
        survivors: Vec<Vec<Edit>>,
        work: u64,
        trailer_edit: bool,
        adler_rerun: bool,
    },
    /// Invalid or truncated data and no repair: the bytes decoded before
    /// input byte `in_used` of `in_total`.
    Prefix {
        data: Vec<u8>,
        in_used: usize,
        in_total: usize,
    },
    /// Damaged, no repair, and nothing kept: nothing decoded, nothing left
    /// once the predictor stopped at an invalid row, or a decode past T-06's
    /// 256 MiB inflate cap (which `decode_chain` refuses too, and which the
    /// memory bound will not keep).
    Unrecoverable,
    /// Damaged and over the size limit, so never searched. It keeps no
    /// bytes; [`SalvageIndex::decoded`] gives what a `ChecksumMismatch` or
    /// `Prefix` would hold.
    Unsearched { reason: OverMaxSearchStream },
}

impl Salvage {
    /// The grade the report carries: a repair's, or `Unsearched`.
    pub(crate) fn grade(&self) -> Option<SalvageGrade> {
        match self {
            Salvage::Repaired { grade, .. } => Some(grade.report()),
            Salvage::Unsearched { .. } => Some(SalvageGrade::Unsearched),
            _ => None,
        }
    }

    /// Capacity of every retained vector, in bytes.
    fn heap_bytes(&self) -> u64 {
        let edits = |v: &Vec<Edit>| (v.capacity() * size_of::<Edit>()) as u64;
        match self {
            Salvage::ChecksumMismatch { data } | Salvage::Prefix { data, .. } => {
                data.capacity() as u64
            }
            Salvage::Repaired {
                data,
                edits: chosen,
                survivors,
                ..
            } => {
                data.capacity() as u64
                    + edits(chosen)
                    + (survivors.capacity() * size_of::<Vec<Edit>>()) as u64
                    + survivors.iter().map(edits).sum::<u64>()
            }
            _ => 0,
        }
    }
}

/// Where the Flate stage sits in a stream's chain, so T-12a can rewrite the
/// chain around a salvaged stage.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FilterStage {
    /// The filters before `/FlateDecode`, decoded before the salvage runs.
    pub earlier: Vec<Filter>,
    /// The Flate stage's `/DecodeParms` (`/Predictor` and its kin).
    pub flate_parms: Option<Dictionary>,
    /// The filters after it, with their parameters (an image codec stays
    /// encoded).
    pub later: Vec<(Filter, Option<Dictionary>)>,
}

/// One stream's entry in the index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SalvageEntry {
    pub salvage: Salvage,
    pub stage: FilterStage,
}

/// Every Flate stream's salvage outcome, keyed by object id. In memory only,
/// never serialised. Streams without a Flate stage, and streams whose chain
/// fails for a reason other than the Flate stage's own data (an earlier
/// filter's corrupt data, parameters it cannot apply, the output cap), have
/// no entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SalvageIndex {
    pub by_obj: BTreeMap<ObjId, SalvageEntry>,
    /// The W every search together spent.
    pub work_total: u64,
}

impl SalvageIndex {
    /// Capacity of every retained `data`, `edits` and `survivors` vector, in
    /// bytes. `Clean` entries count nothing.
    pub(crate) fn heap_bytes(&self) -> u64 {
        self.by_obj.values().map(|e| e.salvage.heap_bytes()).sum()
    }

    /// The stream's decoded bytes, as `decode_chain` would give them for an
    /// undamaged stream: borrowed for an entry that keeps its data (when no
    /// filter follows the Flate stage), re-decoded through T-06 otherwise. A
    /// stream with no entry is decoded through its whole chain. Output over
    /// `cap` is `CapHit`.
    pub(crate) fn decoded<'s, S: StreamSource + ?Sized>(
        &'s self,
        carve: &S,
        id: ObjId,
        cap: usize,
    ) -> Result<Cow<'s, [u8]>, DecodeError> {
        let entry = self.by_obj.get(&id);
        if let Some(SalvageEntry {
            salvage:
                Salvage::ChecksumMismatch { data }
                | Salvage::Repaired { data, .. }
                | Salvage::Prefix { data, .. },
            stage,
        }) = entry
        {
            return if !stage.later.is_empty() {
                decode_chain(data, &stage.later, cap).map(Cow::Owned)
            } else if data.len() > cap {
                Err(DecodeError::CapHit)
            } else {
                Ok(Cow::Borrowed(data))
            };
        }
        let (dict, raw) = carve.stream(id).ok_or_else(|| missing(id))?;
        let chain = filters_of(dict);
        if let Some(SalvageEntry {
            salvage: Salvage::Unsearched { .. },
            stage,
        }) = entry
        {
            // What a ChecksumMismatch or Prefix would have kept: the damaged
            // decode in the mode the ladder chose for it.
            let flate_at = chain.iter().position(|(f, _)| *f == Filter::Flate);
            let earlier = &chain[..flate_at.unwrap_or(0)];
            let input = decode_chain(raw, earlier, DEFAULT_CAP)?;
            let out = match read(&input) {
                Read::Clean(out) | Read::Truncated { out, .. } | Read::Damaged { out, .. } => out,
                Read::Header { .. } => raw_fallback(&input).map(|(_, out)| out).unwrap_or_default(),
                Read::CapHit => return Err(DecodeError::CapHit),
            };
            if out.len() > cap {
                return Err(DecodeError::CapHit);
            }
            let (data, _) = unpredict_flate_lenient(out, stage.flate_parms.as_ref())?;
            return decode_chain(&data, &stage.later, cap).map(Cow::Owned);
        }
        decode_chain(raw, &chain, cap).map(Cow::Owned)
    }
}

fn missing(id: ObjId) -> DecodeError {
    DecodeError::Unsupported(Filter::Unknown(format!("<no stream {} {}>", id.0, id.1)))
}

/// Where the salvage reads streams from: each stream's id, dictionary and
/// raw (undecoded) data. [`CarveSource`] is the carve's (T-05); the tests
/// use their own.
pub(crate) trait StreamSource: Sync {
    /// Every stream, in any order. For an id given twice the last one wins.
    fn streams(&self) -> Vec<(ObjId, &Dictionary, &[u8])>;

    /// The stream `id`: the last one [`Self::streams`] gives. This default
    /// rebuilds and scans the whole list on every call, and
    /// [`SalvageIndex::decoded`] calls it once per stream, so a real source
    /// ([`CarveSource`]) overrides it with a keyed lookup.
    fn stream(&self, id: ObjId) -> Option<(&Dictionary, &[u8])> {
        self.streams()
            .into_iter()
            .rev()
            .find(|(i, _, _)| *i == id)
            .map(|(_, d, r)| (d, r))
    }
}

/// The carve (T-05) with the input its spans point into: the
/// [`StreamSource`] analysis hands to [`salvage_all`] and
/// [`SalvageIndex::decoded`]. `CarveReport` holds byte spans, not bytes, so
/// it cannot be a source alone. A stream whose span lies outside `buf` is
/// left out.
pub(crate) struct CarveSource<'a> {
    carve: &'a CarveReport,
    buf: &'a [u8],
    /// Each id's last stream object, by its index in `carve.objects`.
    last: BTreeMap<ObjId, usize>,
}

impl<'a> CarveSource<'a> {
    pub(crate) fn new(carve: &'a CarveReport, buf: &'a [u8]) -> CarveSource<'a> {
        let mut last = BTreeMap::new();
        for (at, obj) in carve.objects.iter().enumerate() {
            if let Body::Stream { .. } = obj.body {
                last.insert(obj.declared_id, at);
            }
        }
        CarveSource { carve, buf, last }
    }

    fn at(&self, at: usize) -> Option<(ObjId, &'a Dictionary, &'a [u8])> {
        let obj = &self.carve.objects[at];
        let Body::Stream { dict, data, .. } = &obj.body else {
            return None;
        };
        let start = usize::try_from(data.start).ok()?;
        let end = usize::try_from(data.end).ok()?;
        Some((obj.declared_id, dict, self.buf.get(start..end)?))
    }
}

impl StreamSource for CarveSource<'_> {
    fn streams(&self) -> Vec<(ObjId, &Dictionary, &[u8])> {
        (0..self.carve.objects.len())
            .filter_map(|at| self.at(at))
            .collect()
    }

    fn stream(&self, id: ObjId) -> Option<(&Dictionary, &[u8])> {
        let (_, dict, raw) = self.at(*self.last.get(&id)?)?;
        Some((dict, raw))
    }
}

/// Narrows a search to where the damage can be (T-08b's TrueType-checksum
/// localizer, T-08c's grammar one). The first registered localizer whose
/// [`Self::window`] answers sets the window.
pub(crate) trait Localizer: Sync {
    /// The input positions to search, or `None` when this localizer does not
    /// apply. `baseline_out` is the damaged stream's own output.
    fn window(&self, baseline_out: &[u8], trace: &InputTrace) -> Option<Window>;
    /// Asked once per candidate when the window carries a [`Check`], with
    /// the candidate's first `at_out` output bytes: `Some(true)` rejects the
    /// candidate; anything else lets it decode on to its end.
    fn early_reject(&self, candidate_out_prefix: &[u8]) -> Option<bool>;
}

/// The localizers the ladder consults, in order.
const LOCALIZERS: &[&dyn Localizer] = &[&TtfLocalizer];

/// Input positions to search: each range from its top down, in the order
/// given. Ranges are clamped to the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Window {
    pub ranges: Vec<Range<usize>>,
    /// The early verdict on candidates, if the localizer has one.
    pub check: Option<Check>,
}

/// An early verdict on candidates (T-08b's verifier B). A candidate at an
/// input position at or past `from` is decoded to `at_out` output bytes,
/// [`Localizer::early_reject`] is asked once with them, and only a candidate
/// it does not reject decodes on to its end; a position before `from` is
/// judged by its end alone. The damaged stream's own first `at_out` bytes
/// must fail the check: a candidate resumed from a checkpoint at or past
/// `at_out` reproduces them, so it is rejected without an inflate. Each
/// decode stage is charged as a candidate inflate.
///
/// The check assumes the original passed it. When the window is exhausted
/// with no accept, it is searched again with no check, so an original that
/// failed its own check costs more but is never refused; a repair found then
/// carries `adler_rerun`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Check {
    pub at_out: usize,
    pub from: usize,
}

/// The damaged stream's decode, input byte by input byte: `out_after()[i]`
/// is the output length once input byte `i` has been consumed. It maps
/// output offsets to the input positions that produced them, and is built on
/// first use (a localizer that does not apply never pays for it).
/// `decoded_to_end` says the decode ran to the stream's end and only the
/// Adler-32 shows the damage.
#[derive(Debug)]
pub(crate) struct InputTrace<'a> {
    input: &'a [u8],
    mode: Mode,
    pub decoded_to_end: bool,
    out_after: OnceCell<Vec<usize>>,
}

impl<'a> InputTrace<'a> {
    fn of(input: &'a [u8], mode: Mode, decoded_to_end: bool) -> InputTrace<'a> {
        InputTrace {
            input,
            mode,
            decoded_to_end,
            out_after: OnceCell::new(),
        }
    }

    pub(crate) fn out_after(&self) -> &[usize] {
        self.out_after.get_or_init(|| trace(self.input, self.mode))
    }
}

fn trace(input: &[u8], mode: Mode) -> Vec<usize> {
    let mut st = Box::<DecompressorOxide>::default();
    let mut out = Vec::new();
    let mut out_after = Vec::with_capacity(input.len());
    let mut out_pos = 0;
    for i in 0..input.len() {
        let more = if i + 1 < input.len() {
            TINFL_FLAG_HAS_MORE_INPUT
        } else {
            0
        };
        let r = drive(
            &mut st,
            &input[i..=i],
            &mut out,
            out_pos,
            mode.flags() | more,
            DEFAULT_CAP,
            None,
        );
        out_pos = r.out_end;
        if r.used == 0 {
            break;
        }
        out_after.push(out_pos);
        if r.end != End::Starved {
            break;
        }
    }
    out_after
}

/// Salvages one Flate stage's input on its own: the ladder with `work`, then
/// `deep_work` when no accept was found and `deep_pool` covers one draw (the
/// pool is this stream's alone). `max_search_stream` applies to the input's
/// length. A `Clean` result is classified by its bytes alone.
pub(crate) fn salvage_inflate(flate_input: &[u8], budget: &SalvageBudget) -> Salvage {
    salvage_with(flate_input, budget, LOCALIZERS)
}

fn salvage_with(
    flate_input: &[u8],
    budget: &SalvageBudget,
    localizers: &[&dyn Localizer],
) -> Salvage {
    let ctx = Ctx::alone();
    let first = begin(
        flate_input,
        flate_input.len() as u64,
        budget,
        localizers,
        &ctx,
    );
    let outcome = match first {
        Ok(Step::Clean(out)) => {
            return Salvage::Clean {
                decoded_len: out.len() as u64,
                class: classify(&Dictionary::new(), &out),
                sha256: Sha256::digest(&out).into(),
            };
        }
        Ok(Step::Final(o)) => o,
        Ok(Step::Deep(p)) if !deep_allocation(&[(0, (0, 0))], budget).is_empty() => {
            match deepen(flate_input, *p, budget, localizers, &ctx) {
                Ok(o) => o,
                Err(Cancelled) => unreachable!("a lone salvage is never cancelled"),
            }
        }
        Ok(Step::Deep(p)) => p.settle(),
        Err(Cancelled) => unreachable!("a lone salvage is never cancelled"),
    };
    outcome.salvage
}

/// Salvages every Flate stream `carve` gives, in two phases (D-066): every
/// stream under `work` on `threads` workers, then the deep pool is allocated
/// to the streams that found no accept and still have budget or window
/// headroom (smallest raw stream first, then lowest id; a fixed `deep_work`
/// per draw, no refunds, D-056) and the granted streams resume their search.
/// A search starts only while the estimated scratch of the searches running
/// stays within `scratch_cap`, or when none is running (D-072). Neither the
/// thread count nor the cap changes the result. `cancel` is polled on the
/// calling thread between streams and while searches run.
pub(crate) fn salvage_all<S: StreamSource + ?Sized>(
    carve: &S,
    budget: &SalvageBudget,
    threads: NonZeroUsize,
    scratch_cap: u64,
    cancel: &dyn Fn() -> bool,
) -> Result<SalvageIndex, Cancelled> {
    salvage_streams(carve, budget, threads, scratch_cap, cancel, LOCALIZERS)
}

fn salvage_streams<S: StreamSource + ?Sized>(
    carve: &S,
    budget: &SalvageBudget,
    threads: NonZeroUsize,
    scratch_cap: u64,
    cancel: &dyn Fn() -> bool,
    localizers: &[&dyn Localizer],
) -> Result<SalvageIndex, Cancelled> {
    let mut by_id: BTreeMap<ObjId, (&Dictionary, &[u8])> = BTreeMap::new();
    for (id, dict, raw) in carve.streams() {
        by_id.insert(id, (dict, raw));
    }
    let streams: Vec<(ObjId, &Dictionary, &[u8])> =
        by_id.into_iter().map(|(id, (d, r))| (id, d, r)).collect();
    let sched = Sched::new(scratch_cap);

    // Phase 1: every stream under `work`.
    let firsts = sched.run(&streams, threads, cancel, |&(_, dict, raw), sched| {
        first(dict, raw, budget, localizers, sched)
    })?;

    let mut index = SalvageIndex::default();
    let mut pending = Vec::new();
    for (at, f) in firsts.into_iter().enumerate() {
        let (id, _, raw) = streams[at];
        match f {
            First::Skip => {}
            First::Final(entry, work) => {
                index.work_total += work;
                index.by_obj.insert(id, entry);
            }
            First::Pending(stage, p) => pending.push(Waiting {
                at,
                raw_len: raw.len() as u64,
                stage,
                pending: Mutex::new(Some(p)),
            }),
        }
    }

    // Phase 2: the deep pool, allocated before any deep search runs.
    let granted = deep_allocation(
        &pending
            .iter()
            .map(|w| (w.raw_len, streams[w.at].0))
            .collect::<Vec<_>>(),
        budget,
    );
    let (deep, settled): (Vec<Waiting>, Vec<Waiting>) = pending
        .into_iter()
        .partition(|w| granted.contains(&streams[w.at].0));
    for w in settled {
        if let Some(p) = w.take() {
            let outcome = p.settle();
            index.work_total += outcome.work;
            index
                .by_obj
                .insert(streams[w.at].0, entry_of(outcome, w.stage));
        }
    }
    let deepened = sched.run(&deep, threads, cancel, |w, sched| {
        let (_, dict, raw) = streams[w.at];
        let ctx = Ctx {
            sched: Some(sched),
            raw_len: w.raw_len,
        };
        let p = w.take().expect("each granted stream is deepened once");
        // The saved windows point into this input: phase 1 decoded the same
        // earlier filters over the same bytes, so they decode now.
        let input = flate_input(dict, raw).expect("phase 1 decoded this stream's earlier filters");
        deepen(&input, *p, budget, localizers, &ctx)
    })?;
    for (w, outcome) in deep.into_iter().zip(deepened) {
        index.work_total += outcome.work;
        index
            .by_obj
            .insert(streams[w.at].0, entry_of(outcome, w.stage));
    }
    Ok(index)
}

/// The streams granted `deep_work`, in the order drawn: smallest raw length
/// first, then lowest id, one fixed draw each while the pool covers it.
fn deep_allocation(pending: &[(u64, ObjId)], budget: &SalvageBudget) -> Vec<ObjId> {
    let mut order = pending.to_vec();
    order.sort();
    let mut pool = budget.deep_pool;
    let mut granted = Vec::new();
    for (_, id) in order {
        if budget.deep_work == 0 || pool < budget.deep_work {
            break;
        }
        pool -= budget.deep_work;
        granted.push(id);
    }
    granted
}

/// The estimated search scratch of a stream of `raw` bytes whose damaged
/// decode is `out` bytes long (D-072): checkpoints and input, plus the output
/// buffers. A search holds four output-sized buffers (the fallback, the
/// baseline it resumes from, the candidate scratch and the best survivor);
/// the plan's `66 × raw + max(12 × raw, 1 MiB)` covers them up to 3:1, so a
/// more compressible stream (a flat image at 1000:1) is charged `4 × out`
/// instead. A candidate whose output runs
/// past the damaged decode still grows its scratch beyond the estimate (up
/// to T-06's 256 MiB inflate cap, measured at about 7 × out on a flat
/// stream), which no estimate taken before the search can foresee.
/// Admission never changes a result, only when a search starts.
fn scratch_estimate(raw: u64, out: u64) -> u64 {
    66 * raw + (12 * raw).max(1 << 20).max(out.saturating_mul(4))
}

/// A stream that ended phase 1 waiting for a deep draw; `at` indexes the
/// streams in id order.
struct Waiting {
    at: usize,
    raw_len: u64,
    stage: FilterStage,
    pending: Mutex<Option<Box<Pending>>>,
}

impl Waiting {
    fn take(&self) -> Option<Box<Pending>> {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
    }
}

enum First {
    /// Not a Flate stream, or one the chain fails on for another reason.
    Skip,
    Final(SalvageEntry, u64),
    Pending(FilterStage, Box<Pending>),
}

/// Phase 1 for one stream of `salvage_all`.
fn first(
    dict: &Dictionary,
    raw: &[u8],
    budget: &SalvageBudget,
    localizers: &[&dyn Localizer],
    sched: &Sched,
) -> Result<First, Cancelled> {
    let chain = filters_of(dict);
    let Some(flate_at) = chain.iter().position(|(f, _)| *f == Filter::Flate) else {
        return Ok(First::Skip);
    };
    let earlier_full = &chain[..flate_at];
    let stage = FilterStage {
        earlier: earlier_full.iter().map(|(f, _)| f.clone()).collect(),
        flate_parms: chain[flate_at].1.clone(),
        later: chain[flate_at + 1..].to_vec(),
    };
    if unpredict_flate(Vec::new(), stage.flate_parms.as_ref()).is_err() {
        return Ok(First::Skip);
    }
    match decode_chain(raw, &chain, DEFAULT_CAP) {
        Ok(bytes) => {
            let salvage = Salvage::Clean {
                decoded_len: bytes.len() as u64,
                class: classify(dict, &bytes),
                sha256: Sha256::digest(&bytes).into(),
            };
            return Ok(First::Final(SalvageEntry { salvage, stage }, 0));
        }
        Err(DecodeError::Inflate(_)) => {}
        Err(_) => return Ok(First::Skip),
    }
    let Some(input) = flate_input(dict, raw) else {
        return Ok(First::Skip);
    };
    let ctx = Ctx {
        sched: Some(sched),
        raw_len: raw.len() as u64,
    };
    Ok(
        match begin(&input, raw.len() as u64, budget, localizers, &ctx)? {
            // The Flate stage is sound; a later stage failed.
            Step::Clean(_) => First::Skip,
            Step::Final(outcome) => {
                let work = outcome.work;
                First::Final(entry_of(outcome, stage), work)
            }
            Step::Deep(p) => First::Pending(stage, p),
        },
    )
}

/// The Flate stage's input: `raw` through the filters before the first
/// `/FlateDecode`, with their parameters.
fn flate_input(dict: &Dictionary, raw: &[u8]) -> Option<Vec<u8>> {
    let chain = filters_of(dict);
    let flate_at = chain.iter().position(|(f, _)| *f == Filter::Flate)?;
    decode_chain(raw, &chain[..flate_at], DEFAULT_CAP).ok()
}

/// The index entry: the outcome's bytes with the Flate predictor undone, as
/// far as the first invalid PNG row. Only an outcome with nothing left is
/// `Unrecoverable`; a `Prefix` keeps the inflate's `in_used`. A repair whose
/// verified output holds an invalid row is no longer a whole repair: it
/// becomes a `ChecksumMismatch` of the rows before that row, so no grade
/// vouches for truncated bytes.
fn entry_of(outcome: Outcome, stage: FilterStage) -> SalvageEntry {
    let parms = stage.flate_parms.as_ref();
    // `first` refused parameters the predictor cannot apply.
    let unpredict = |data: Vec<u8>| unpredict_flate_lenient(data, parms).ok();
    let kept = |mut d: Vec<u8>| {
        d.shrink_to_fit();
        d
    };
    let salvage = match outcome.salvage {
        Salvage::ChecksumMismatch { data } => match unpredict(data) {
            Some((d, stopped)) if stopped.is_none() || !d.is_empty() => {
                Salvage::ChecksumMismatch { data: kept(d) }
            }
            _ => Salvage::Unrecoverable,
        },
        Salvage::Prefix {
            data,
            in_used,
            in_total,
        } => match unpredict(data) {
            Some((d, stopped)) if stopped.is_none() || !d.is_empty() => Salvage::Prefix {
                data: kept(d),
                in_used,
                in_total,
            },
            _ => Salvage::Unrecoverable,
        },
        Salvage::Repaired {
            data,
            edits,
            grade,
            survivors,
            work,
            trailer_edit,
            adler_rerun,
        } => match unpredict(data) {
            Some((d, None)) => Salvage::Repaired {
                data: kept(d),
                edits,
                grade,
                survivors,
                work,
                trailer_edit,
                adler_rerun,
            },
            Some((d, Some(_))) if !d.is_empty() => Salvage::ChecksumMismatch { data: kept(d) },
            _ => Salvage::Unrecoverable,
        },
        other => other,
    };
    SalvageEntry { salvage, stage }
}

// ── the ladder ───────────────────────────────────────────────────────────

/// Zlib, or raw deflate (no header, no Adler-32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Zlib,
    Raw,
}

impl Mode {
    fn flags(self) -> u32 {
        let header = match self {
            Mode::Zlib => TINFL_FLAG_PARSE_ZLIB_HEADER,
            Mode::Raw => 0,
        };
        header | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
    }
}

/// What the damage looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Damage {
    /// Decoded to its end; the Adler-32 trailer starts at `trailer`.
    Adler { trailer: usize },
    /// Invalid data after `k` input bytes.
    Error { k: usize },
}

/// A survivor's place in the pinned order: trailer edits after body edits,
/// then offset, then the replacement byte.
type Key = (bool, usize, u8);

/// A search's progress, kept between the phases. Checkpoints are not kept:
/// they are scratch, rebuilt (identically) when the search resumes.
#[derive(Debug, Clone)]
struct Pending {
    mode: Mode,
    damage: Damage,
    /// The damaged stream's own output (raw inflate domain): the fallback.
    fallback: Vec<u8>,
    in_total: usize,
    /// The windows of phase 1 and of the deep phase; the second extends the
    /// first.
    windows: [Vec<Range<usize>>; 2],
    /// The localizer that set the window, if any.
    localizer: Option<usize>,
    /// The localizer's early check, until the window is searched again
    /// without it.
    check: Option<Check>,
    /// The window is being (or was) searched again without the check.
    rerun: bool,
    /// The zlib header was rejected: when the zlib window is exhausted with
    /// no survivor, the search moves on to raw deflate.
    raw_next: bool,
    hunt: Hunt,
}

#[derive(Debug, Clone, Default)]
struct Hunt {
    cursor: Cursor,
    /// W spent so far.
    spent: u64,
    /// Candidates tried so far.
    tried: u64,
    /// Candidates accepted (one output may be accepted more than once).
    accepted: u64,
    /// Candidates the check rejected after decoding to its `at_out`.
    checked_out: u64,
    /// Candidates the check rejected with no inflate.
    no_inflate: u64,
    /// Every distinct output's hash and its best key and edit.
    found: BTreeMap<[u8; 32], (Key, Edit)>,
    /// The output of the survivor first in the pinned order.
    best: Option<(Key, Vec<u8>)>,
}

/// The next candidate: range `range`, position `pos`, value `value`.
#[derive(Debug, Clone, Copy, Default)]
struct Cursor {
    range: usize,
    /// One past the position to try; `None` before the range is entered.
    top: Option<usize>,
    value: u16,
}

struct Outcome {
    /// In the raw inflate domain; `Clean` never appears here.
    salvage: Salvage,
    work: u64,
}

enum Step {
    /// The Flate stage decodes cleanly: its bytes.
    Clean(Vec<u8>),
    Final(Outcome),
    /// No accept yet, and budget or window left for a deep draw.
    Deep(Box<Pending>),
}

/// The damaged stream's own decode, as the ladder reads it.
enum Read {
    /// `Done`, with the Adler-32 verified.
    Clean(Vec<u8>),
    /// The output reached T-06's inflate cap.
    CapHit,
    /// The input ran out after `consumed` bytes.
    Truncated { out: Vec<u8>, consumed: usize },
    /// The zlib header was rejected after `at` bytes, with nothing decoded.
    /// The search tries zlib first; only when that finds no repair is the
    /// stream read as raw deflate ([`raw_fallback`]).
    Header { at: usize },
    /// Searchable damage.
    Damaged {
        mode: Mode,
        damage: Damage,
        out: Vec<u8>,
    },
}

/// Inflates `input` and says what the damage looks like.
fn read(input: &[u8]) -> Read {
    let first = inflate(input, DEFAULT_CAP);
    match first.status {
        InflateStatus::Done => Read::Clean(first.out),
        InflateStatus::CapHit => Read::CapHit,
        InflateStatus::NeedsMoreInput => Read::Truncated {
            out: first.out,
            consumed: first.consumed,
        },
        InflateStatus::AdlerMismatch => Read::Damaged {
            mode: Mode::Zlib,
            damage: Damage::Adler {
                trailer: first.consumed.saturating_sub(4),
            },
            out: first.out,
        },
        // T-06's own test for a rejected header.
        InflateStatus::Failed { at } if at <= 2 && first.out.is_empty() => Read::Header { at },
        InflateStatus::Failed { at } => Read::Damaged {
            mode: Mode::Zlib,
            damage: Damage::Error { k: at },
            out: first.out,
        },
    }
}

/// A stream whose zlib header was rejected, read as raw deflate: searchable
/// only when the raw decode fails on invalid data after decoding something.
/// A raw decode that runs out of input or reaches the cap is dropped, as
/// T-06's inflate drops it: bytes that are not raw deflate often decode a
/// little by chance, and a salvage must not keep such a prefix. (A raw
/// `Done` never gets here: T-06's inflate already returned it.)
fn raw_fallback(input: &[u8]) -> Option<(Damage, Vec<u8>)> {
    let b = baseline(input, Mode::Raw, false);
    (b.end == End::Failed && !b.out.is_empty()).then_some((Damage::Error { k: b.consumed }, b.out))
}

/// The inflater's buffer is sized for growth; the search keeps the bytes.
fn fallback_of(mut out: Vec<u8>) -> Vec<u8> {
    out.shrink_to_fit();
    out
}

/// Phase 1: inflate, then the search under `work`.
fn begin(
    input: &[u8],
    raw_len: u64,
    budget: &SalvageBudget,
    localizers: &[&dyn Localizer],
    ctx: &Ctx,
) -> Result<Step, Cancelled> {
    let final_ = |salvage| Ok(Step::Final(Outcome { salvage, work: 0 }));
    let (mode, damage, fallback, raw_next) = match read(input) {
        Read::Clean(out) => return Ok(Step::Clean(out)),
        Read::CapHit => return final_(Salvage::Unrecoverable),
        Read::Truncated { out, consumed } => {
            return final_(prefix(out, consumed, input.len()));
        }
        // Zlib first: the error window `[0, at + 8)` covers the header, and
        // almost every candidate fails at its check bits.
        Read::Header { at } => (Mode::Zlib, Damage::Error { k: at }, Vec::new(), true),
        Read::Damaged { mode, damage, out } => (mode, damage, fallback_of(out), false),
    };
    if raw_len > budget.max_search_stream {
        return final_(Salvage::Unsearched {
            reason: OverMaxSearchStream {
                raw_len,
                limit: budget.max_search_stream,
            },
        });
    }
    let (windows, localizer, check) = windows_for(input, mode, damage, &fallback, localizers);
    let mut p = Pending {
        mode,
        damage,
        fallback,
        in_total: input.len(),
        windows,
        localizer,
        check,
        rerun: false,
        raw_next,
        hunt: Hunt::default(),
    };
    let exhausted = search(input, &mut p, 0, budget.work, localizers, ctx)?;
    let headroom = !exhausted || total(&p.windows[1]) > total(&p.windows[0]);
    Ok(if p.hunt.found.is_empty() && headroom {
        Step::Deep(Box::new(p))
    } else {
        Step::Final(p.conclude(0, exhausted))
    })
}

/// Phase 2 for a stream granted `deep_work`: the search resumes over the deep
/// window until the stream has spent `work + deep_work` in all.
fn deepen(
    input: &[u8],
    mut p: Pending,
    budget: &SalvageBudget,
    localizers: &[&dyn Localizer],
    ctx: &Ctx,
) -> Result<Outcome, Cancelled> {
    let limit = budget.work.saturating_add(budget.deep_work);
    let exhausted = search(input, &mut p, 1, limit, localizers, ctx)?;
    Ok(p.conclude(1, exhausted))
}

impl Pending {
    /// The result for a stream that got no deep draw.
    fn settle(self) -> Outcome {
        self.conclude(0, false)
    }

    /// The result once the search stopped in window `phase`.
    fn conclude(self, phase: usize, exhausted: bool) -> Outcome {
        let work = self.hunt.spent;
        let Some((best_key, data)) = self.hunt.best else {
            let salvage = match self.damage {
                Damage::Adler { .. } => Salvage::ChecksumMismatch {
                    data: self.fallback,
                },
                Damage::Error { k } => prefix(self.fallback, k, self.in_total),
            };
            return Outcome { salvage, work };
        };
        let mut survivors: Vec<(Key, Edit)> = self.hunt.found.into_values().collect();
        survivors.sort();
        let outputs = survivors.len() as u32;
        let grade = if self.mode == Mode::Raw || outputs >= 2 {
            Grade::Ambiguous { outputs }
        } else if exhausted {
            Grade::Exact
        } else {
            Grade::Accepted {
                searched: self.hunt.tried,
                window: total(&self.windows[phase]),
            }
        };
        let survivors: Vec<Vec<Edit>> = survivors.into_iter().map(|(_, e)| vec![e]).collect();
        let salvage = Salvage::Repaired {
            data,
            edits: survivors[0].clone(),
            grade,
            survivors,
            work,
            trailer_edit: best_key.0,
            adler_rerun: self.rerun,
        };
        Outcome { salvage, work }
    }

    /// The window is exhausted. With no survivor, a window searched under a
    /// [`Check`] is searched again without it; then an Adler-only zlib stream
    /// gets the trailer-recompute path, and a stream whose zlib header was
    /// rejected moves on to raw deflate when [`raw_fallback`] allows it:
    /// `true` means the search goes on, from the top of its own windows (the
    /// W spent so far stays spent).
    fn exhausted(&mut self, input: &[u8], localizers: &[&dyn Localizer]) -> bool {
        if !self.hunt.found.is_empty() {
            return false;
        }
        if self.check.take().is_some() {
            self.rerun = true;
            self.hunt.cursor = Cursor::default();
            return true;
        }
        match (self.mode, self.damage) {
            (Mode::Zlib, Damage::Adler { trailer }) => {
                trailer_survivor(input, trailer, self);
                false
            }
            (Mode::Zlib, Damage::Error { .. }) if self.raw_next => {
                self.raw_next = false;
                let Some((damage, out)) = raw_fallback(input) else {
                    return false;
                };
                let fallback = fallback_of(out);
                let (windows, localizer, check) =
                    windows_for(input, Mode::Raw, damage, &fallback, localizers);
                self.mode = Mode::Raw;
                self.damage = damage;
                self.fallback = fallback;
                self.windows = windows;
                self.localizer = localizer;
                self.check = check;
                self.hunt.cursor = Cursor::default();
                true
            }
            _ => false,
        }
    }

    /// Records a survivor whose output is `out`.
    fn accept(&mut self, key: Key, edit: Edit, out: &[u8]) {
        self.hunt.accepted += 1;
        let hash: [u8; 32] = Sha256::digest(out).into();
        let rep = self.hunt.found.entry(hash).or_insert((key, edit));
        if key < rep.0 {
            *rep = (key, edit);
        }
        if self.hunt.best.as_ref().is_none_or(|(k, _)| key < *k) {
            self.hunt.best = Some((key, out.to_vec()));
        }
    }
}

fn prefix(mut data: Vec<u8>, in_used: usize, in_total: usize) -> Salvage {
    if data.is_empty() {
        return Salvage::Unrecoverable;
    }
    data.shrink_to_fit();
    Salvage::Prefix {
        data,
        in_used,
        in_total,
    }
}

/// The phase-1 and deep windows, and the localizer that set them with its
/// check.
fn windows_for(
    input: &[u8],
    mode: Mode,
    damage: Damage,
    baseline_out: &[u8],
    localizers: &[&dyn Localizer],
) -> ([Vec<Range<usize>>; 2], Option<usize>, Option<Check>) {
    if !localizers.is_empty() {
        let to_end = matches!(damage, Damage::Adler { .. });
        let trace = InputTrace::of(input, mode, to_end);
        for (i, l) in localizers.iter().enumerate() {
            if let Some(w) = l.window(baseline_out, &trace) {
                let ranges: Vec<Range<usize>> = w
                    .ranges
                    .into_iter()
                    .map(|r| r.start.min(input.len())..r.end.min(input.len()))
                    .collect();
                return ([ranges.clone(), ranges], Some(i), w.check);
            }
        }
    }
    match damage {
        Damage::Adler { trailer } => {
            let w = std::iter::once(ADLER_FROM.min(trailer)..trailer).collect::<Vec<_>>();
            ([w.clone(), w], None, None)
        }
        Damage::Error { k } => {
            let hi = (k + ERROR_SLACK).min(input.len());
            let lo = k.saturating_sub(ERROR_WINDOW).min(hi);
            let deep_lo = k.saturating_sub(DEEP_ERROR_WINDOW).min(lo);
            (
                [std::iter::once(lo..hi).collect(), vec![lo..hi, deep_lo..lo]],
                None,
                None,
            )
        }
    }
}

/// Candidates in a window.
fn total(window: &[Range<usize>]) -> u64 {
    window.iter().map(|r| r.len() as u64 * 255).sum()
}

/// The trailer-recompute path (eng-r1-fr1 fact 7): when the body decoded and
/// the stored Adler-32 differs from the output's in exactly one byte, that
/// byte's rewrite is a survivor. It costs no inflate. It runs only once the
/// body window is exhausted with no body survivor: a rewrite makes any
/// output whose Adler-32 is one byte off verify, and body damage that keeps
/// the byte sum (a few output bytes permuted) often is, so a trailer rewrite
/// is evidence only when no body edit explains the mismatch. One damaged
/// byte is in the body or in the trailer, never both, so this is also the
/// pinned order's "body edits before trailer edits".
///
/// The cost of that gate: exhausting the body window takes about 127 × n²
/// W for an n-byte stream, so trailer-only damage is found under `work`
/// only up to about 2.3 KB and under `deep_work` up to about 9 KB. A larger
/// stream with only a trailer byte damaged spends its `work`, draws a
/// `deep_work`, and still ends `ChecksumMismatch` (its data is the original
/// either way). A false `Accepted` on permuted body bytes is the worse
/// failure for a forensic report, so the gate stays.
fn trailer_survivor(input: &[u8], trailer: usize, p: &mut Pending) {
    let Some(stored) = input.get(trailer..trailer + 4) else {
        return;
    };
    let want = adler32(&p.fallback).to_be_bytes();
    let differ: Vec<usize> = (0..4).filter(|&i| stored[i] != want[i]).collect();
    if let [i] = differ[..] {
        let at = trailer + i;
        let fallback = std::mem::take(&mut p.fallback);
        p.accept((true, at, want[i]), (at, stored[i], want[i]), &fallback);
        p.fallback = fallback;
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 bytes is the most that cannot overflow `b` before the reduction.
    for chunk in data.chunks(5_552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= ADLER_MOD;
        b %= ADLER_MOD;
    }
    (b << 16) | a
}

/// Searches window `phase` of `p` until it is exhausted (`true`) or the
/// stream has spent `limit` (`false`), moving on to raw deflate when
/// [`Pending::exhausted`] says so.
fn search(
    input: &[u8],
    p: &mut Pending,
    phase: usize,
    limit: u64,
    localizers: &[&dyn Localizer],
    ctx: &Ctx,
) -> Result<bool, Cancelled> {
    loop {
        if !search_window(input, p, phase, limit, localizers, ctx)? {
            return Ok(false);
        }
        if !p.exhausted(input, localizers) {
            return Ok(true);
        }
    }
}

/// One mode's pass over window `phase`: `true` once it is exhausted. Positions
/// run from the top of each range down, values from 0 up, skipping the byte
/// already there. A raw-deflate candidate is accepted only when it ends
/// within [`RAW_END_SLACK`] bytes of the input's end: with no Adler-32, a
/// stream cut short is otherwise an accept too. A [`Check`] judges the
/// candidates at or past its `from` first.
fn search_window(
    input: &[u8],
    p: &mut Pending,
    phase: usize,
    limit: u64,
    localizers: &[&dyn Localizer],
    ctx: &Ctx,
) -> Result<bool, Cancelled> {
    let window = p.windows[phase].clone();
    if next_candidate(&window, p.hunt.cursor, input).is_none() {
        return Ok(true);
    }
    if p.hunt.spent >= limit {
        return Ok(false);
    }
    let _admitted = ctx.admit(p.fallback.len() as u64)?;
    let base = baseline_sized(input, p.mode, true, p.fallback.len());
    let early = p.localizer.map(|i| localizers[i]);
    let mut buf = input.to_vec();
    // Room past the damaged output, so a candidate of about its length does
    // not double the buffer.
    let mut scratch = Vec::with_capacity(base.out.len().saturating_add(OUT_SLACK));
    scratch.extend_from_slice(&base.out);
    scratch.resize(scratch.capacity(), 0);
    // `scratch[..valid]` still equals the damaged stream's own output.
    let mut valid = base.out.len();
    let mut st = Box::<DecompressorOxide>::default();
    loop {
        let Some((cursor, pos, value)) = next_candidate(&window, p.hunt.cursor, input) else {
            return Ok(true);
        };
        if p.hunt.spent >= limit {
            return Ok(false);
        }
        if p.hunt.tried.is_multiple_of(POLL_EVERY) {
            ctx.poll()?;
        }
        let cp = &base.checkpoints[base.checkpoints.partition_point(|c| c.in_pos <= pos) - 1];
        let checked = p.check.filter(|c| pos >= c.from);
        if checked.is_some_and(|c| cp.out_pos >= c.at_out) {
            // The candidate's first `at_out` bytes are the damaged stream's.
            p.hunt.tried += 1;
            p.hunt.no_inflate += 1;
            p.hunt.cursor = cursor;
            continue;
        }
        if valid < cp.out_pos {
            scratch[valid..cp.out_pos].copy_from_slice(&base.out[valid..cp.out_pos]);
        }
        valid = cp.out_pos;
        st.clone_from(&cp.state);
        let old = buf[pos];
        buf[pos] = value;
        let mut ask = early.map(|l| move |out: &[u8]| l.early_reject(out));
        let pause = checked
            .zip(ask.as_mut())
            .map(|(c, f)| (c.at_out, f as Ask<'_>));
        let r = drive(
            &mut st,
            &buf[cp.in_pos..],
            &mut scratch,
            cp.out_pos,
            p.mode.flags(),
            DEFAULT_CAP,
            pause,
        );
        buf[pos] = old;
        p.hunt.spent += r.used as u64 + CALL_W * r.stages;
        p.hunt.tried += 1;
        p.hunt.cursor = cursor;
        if r.end == End::Rejected {
            p.hunt.checked_out += 1;
        }
        let to_the_end = cp.in_pos + r.used + RAW_END_SLACK >= input.len();
        if r.end == End::Done && (p.mode == Mode::Zlib || to_the_end) {
            p.accept(
                (false, pos, value),
                (pos, old, value),
                &scratch[..r.out_end],
            );
        }
    }
}

/// The candidate at `cursor` (moving into the next range or position as
/// needed) and the cursor after it, or `None` when the window is exhausted.
fn next_candidate(
    window: &[Range<usize>],
    mut c: Cursor,
    input: &[u8],
) -> Option<(Cursor, usize, u8)> {
    loop {
        let range = window.get(c.range)?;
        let top = c.top.unwrap_or(range.end);
        if top <= range.start {
            c = Cursor {
                range: c.range + 1,
                top: None,
                value: 0,
            };
            continue;
        }
        let pos = top - 1;
        let mut value = c.value;
        if value < 256 && value as u8 == input[pos] {
            value += 1;
        }
        if value >= 256 {
            c = Cursor {
                range: c.range,
                top: Some(pos),
                value: 0,
            };
            continue;
        }
        let after = Cursor {
            range: c.range,
            top: Some(top),
            value: value + 1,
        };
        return Some((after, pos, value as u8));
    }
}

// ── the decoder ──────────────────────────────────────────────────────────

/// How a decode ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    /// Done; for zlib, with the Adler-32 verified.
    Done,
    AdlerMismatch,
    Failed,
    /// The input ran out (with more promised, or truncated).
    Starved,
    CapHit,
    /// A localizer's `early_reject` refused it.
    Rejected,
}

struct Driven {
    end: End,
    /// Input consumed, summed over every `decompress` call.
    used: usize,
    out_end: usize,
    /// Decode stages: 2 when a pause's check let the decode go on.
    stages: u64,
}

/// A localizer's `early_reject` for one candidate.
type Ask<'a> = &'a mut dyn FnMut(&[u8]) -> Option<bool>;

/// Runs `st` over `input`, writing from `out[out_pos]` and growing `out` (up
/// to `cap`) as needed. With a `pause` `(at, ask)`, the decode stops once
/// `at` output bytes exist, `ask` judges them, and only a decode it does not
/// reject goes on (a second stage). A decode that ends before `at` is not
/// asked.
fn drive(
    st: &mut DecompressorOxide,
    input: &[u8],
    out: &mut Vec<u8>,
    mut out_pos: usize,
    flags: u32,
    cap: usize,
    mut pause: Option<(usize, Ask<'_>)>,
) -> Driven {
    let mut used = 0usize;
    let mut stages = 1;
    let mut called = false;
    let end = loop {
        if let Some((at, ask)) = pause.as_mut()
            && out_pos >= *at
        {
            if ask(&out[..*at]) == Some(true) {
                break End::Rejected;
            }
            pause = None;
            if called {
                stages += 1;
            }
        }
        let limit = pause.as_ref().map_or(usize::MAX, |(at, _)| at - out_pos);
        let (status, i, o) = decompress_with_limit(st, &input[used..], out, out_pos, limit, flags);
        called = true;
        used += i;
        out_pos += o;
        break match status {
            TINFLStatus::HasMoreOutput => {
                let paused = pause.as_ref().is_some_and(|(at, _)| out_pos >= *at);
                if out_pos < out.len() || paused {
                    continue;
                }
                if out.len() >= cap {
                    End::CapHit
                } else {
                    let grown = out.len().saturating_mul(2).max(64 << 10).min(cap);
                    out.resize(grown, 0);
                    continue;
                }
            }
            TINFLStatus::Done => End::Done,
            TINFLStatus::Adler32Mismatch => End::AdlerMismatch,
            TINFLStatus::NeedsMoreInput | TINFLStatus::FailedCannotMakeProgress => End::Starved,
            // `Failed`, `BadParam`, or anything newer (non-exhaustive).
            _ => End::Failed,
        };
    };
    Driven {
        end,
        used,
        out_end: out_pos,
        stages,
    }
}

/// A decoder state after `in_pos` input bytes and `out_pos` output bytes.
struct Checkpoint {
    in_pos: usize,
    out_pos: usize,
    state: Box<DecompressorOxide>,
}

/// The damaged stream's own decode, fed 256 input bytes at a time, with a
/// checkpoint at every boundary when asked.
struct Baseline {
    end: End,
    consumed: usize,
    out: Vec<u8>,
    checkpoints: Vec<Checkpoint>,
}

fn baseline(input: &[u8], mode: Mode, checkpoints: bool) -> Baseline {
    baseline_sized(input, mode, checkpoints, 0)
}

/// [`baseline`] with the output length expected (the damaged decode's), so
/// the buffer is allocated once instead of doubling past it.
fn baseline_sized(input: &[u8], mode: Mode, checkpoints: bool, expect_out: usize) -> Baseline {
    let mut st = Box::<DecompressorOxide>::default();
    let first_size = input
        .len()
        .saturating_mul(4)
        .max(expect_out.saturating_add(OUT_SLACK))
        .clamp(64 << 10, DEFAULT_CAP);
    let mut out = vec![0u8; first_size];
    let mut cps: Vec<Checkpoint> = Vec::new();
    let (mut in_pos, mut out_pos, mut stop) = (0usize, 0usize, 0usize);
    loop {
        if checkpoints && cps.last().is_none_or(|c| c.in_pos < in_pos) {
            cps.push(Checkpoint {
                in_pos,
                out_pos,
                state: st.clone(),
            });
        }
        stop = (stop + CHECKPOINT_EVERY).min(input.len());
        let more = stop < input.len();
        let flags = mode.flags() | if more { TINFL_FLAG_HAS_MORE_INPUT } else { 0 };
        let r = drive(
            &mut st,
            &input[in_pos..stop],
            &mut out,
            out_pos,
            flags,
            DEFAULT_CAP,
            None,
        );
        in_pos += r.used;
        out_pos = r.out_end;
        if r.end == End::Starved && more {
            continue;
        }
        out.truncate(out_pos);
        out.shrink_to_fit();
        return Baseline {
            end: r.end,
            consumed: in_pos,
            out,
            checkpoints: cps,
        };
    }
}

// ── the scheduler ────────────────────────────────────────────────────────

/// Work queue, scratch admission and cancellation for `salvage_all`'s
/// workers. Only the calling thread polls `cancel`; workers wake it.
struct Sched {
    state: Mutex<Queue>,
    wake: Condvar,
    stop: AtomicBool,
    cap: u64,
}

struct Queue {
    next: usize,
    workers: usize,
    in_use: u64,
}

impl Sched {
    fn new(cap: u64) -> Sched {
        Sched {
            state: Mutex::new(Queue {
                next: 0,
                workers: 0,
                in_use: 0,
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            cap,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Maps `items` through `f` on up to `threads` workers; the results come
    /// back in item order.
    fn run<T: Sync, R: Send>(
        &self,
        items: &[T],
        threads: NonZeroUsize,
        cancel: &dyn Fn() -> bool,
        f: impl Fn(&T, &Sched) -> Result<R, Cancelled> + Sync,
    ) -> Result<Vec<R>, Cancelled> {
        if self.stopped() || cancel() {
            return Err(Cancelled);
        }
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let workers = threads.get().min(items.len());
        {
            let mut q = self.lock();
            q.next = 0;
            q.workers = workers;
        }
        #[cfg(test)]
        let tracked = tests::track::current();
        let results = std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    s.spawn(|| {
                        #[cfg(test)]
                        tests::track::set(tracked);
                        let _leave = Leave(self);
                        let mut done = Vec::new();
                        while !self.stopped() {
                            let i = {
                                let mut q = self.lock();
                                let i = q.next;
                                q.next += 1;
                                i
                            };
                            let Some(item) = items.get(i) else { break };
                            match f(item, self) {
                                Ok(r) => done.push((i, r)),
                                Err(Cancelled) => break,
                            }
                            self.wake.notify_all();
                        }
                        done
                    })
                })
                .collect();
            let mut q = self.lock();
            while q.workers > 0 {
                if !self.stopped() && cancel() {
                    self.stop.store(true, Ordering::Relaxed);
                    self.wake.notify_all();
                }
                q = self.wake.wait(q).unwrap_or_else(|p| p.into_inner());
            }
            drop(q);
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
                .collect::<Vec<_>>()
        });
        if self.stopped() {
            return Err(Cancelled);
        }
        let mut all: Vec<(usize, R)> = results.into_iter().flatten().collect();
        all.sort_by_key(|(i, _)| *i);
        Ok(all.into_iter().map(|(_, r)| r).collect())
    }
}

/// Marks a worker gone (also when it unwinds) and wakes the caller.
struct Leave<'a>(&'a Sched);

impl Drop for Leave<'_> {
    fn drop(&mut self) {
        self.0.lock().workers -= 1;
        self.0.wake.notify_all();
    }
}

/// A search's link to the scheduler: its scratch estimate, admission and
/// cancellation. A lone salvage has none.
struct Ctx<'a> {
    sched: Option<&'a Sched>,
    /// The stream's raw length, for [`scratch_estimate`].
    raw_len: u64,
}

impl Ctx<'_> {
    fn alone() -> Ctx<'static> {
        Ctx {
            sched: None,
            raw_len: 0,
        }
    }

    /// Waits until the search's scratch (for a damaged decode of `out_len`
    /// bytes) fits beside the running ones, or nothing runs.
    fn admit(&self, out_len: u64) -> Result<Admitted<'_>, Cancelled> {
        let Some(s) = self.sched else {
            return Ok(Admitted(None, 0));
        };
        let est = scratch_estimate(self.raw_len, out_len);
        let mut q = s.lock();
        while q.in_use != 0 && q.in_use.saturating_add(est) > s.cap {
            if s.stopped() {
                return Err(Cancelled);
            }
            q = s.wake.wait(q).unwrap_or_else(|p| p.into_inner());
        }
        if s.stopped() {
            return Err(Cancelled);
        }
        q.in_use += est;
        Ok(Admitted(Some(s), est))
    }

    /// Wakes the calling thread so it polls `cancel`; fails once cancelled.
    fn poll(&self) -> Result<(), Cancelled> {
        match self.sched {
            Some(s) if s.stopped() => Err(Cancelled),
            Some(s) => {
                s.wake.notify_all();
                Ok(())
            }
            None => Ok(()),
        }
    }
}

/// Admitted scratch, released on drop.
struct Admitted<'a>(Option<&'a Sched>, u64);

impl Drop for Admitted<'_> {
    fn drop(&mut self) {
        if let Some(s) = self.0 {
            s.lock().in_use -= self.1;
            s.wake.notify_all();
        }
    }
}
