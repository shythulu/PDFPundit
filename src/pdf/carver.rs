//! Object carver (T-05, T-07; TD §14): every object in a damaged PDF, found
//! by its `num gen obj` header in byte order rather than through the xref
//! table, which C2/C3/C10 damage removes.
//!
//! T-05's rules, each with tests in `tests`:
//! 1. **Header landmark** (`landmarks`): `num gen obj` with one whitespace
//!    byte between the parts, at the start of the buffer or after whitespace,
//!    assembled by walking back at most 24 bytes from `obj`. `endobj` tails
//!    are not headers. Numbers past `u32`, or a generation past 65,535, are
//!    ignored with [`CarveNote::HeaderOutOfRange`].
//! 2. **Byte-order assembly**: headers inside the data of a stream already
//!    resolved are dead, so keyword bytes in stream data never start an
//!    object. A value is parsed only up to the next header.
//! 3. **Extent ladder** for stream data: (a) a direct `/Length` that lands
//!    exactly on EOL + `endstream`; (b) the one `endstream` between the data
//!    and the next header that is framed as EOL + `endstream` + optional
//!    whitespace + `endobj` (two or more: nothing, carry on); (c) an indirect
//!    `/Length` (T-07 rule 2), which is tried right after (a) and before (b),
//!    as TD §14.3 orders it: a resolved length is declared evidence, like
//!    (a), and an exact fit must win over a framed `endstream` that the
//!    data itself holds; (d) the inflate probe (T-07 rule 1); (e) the
//!    last `endstream` before the next header; (f) the end of the file, or
//!    the next header when there is one ([`LengthSource::TruncatedAtEof`]
//!    either way; [`CarveNote::NoEndstream`] marks the second). The scanned
//!    rungs and the probe never look past the next header, so their extent
//!    holds no foreign header (qpdf's check). Rungs (a) and (c) are exempt
//!    from that check: an exact fit is how a stream whose data holds
//!    `\n1 0 obj\n` at a line start keeps its data (the keyword-in-stream
//!    acceptance criterion), and the price is that a `/Length` landing
//!    exactly on a later object's `endstream` swallows the objects between
//!    (pinned by `an_exact_declared_length_is_trusted_over_a_header`).
//! 4. **EOL after `stream`**: LF, CRLF, a bare CR (with
//!    [`CarveNote::BareCrAfterStream`], D-030) or a space and then one of those.
//! 5. A missing `endobj` ends the object at the next header or EOF.
//! 6. Caps: [`MAX_LANDMARKS`] and [`MAX_OBJECTS`], each recorded with
//!    [`CarveNote::CapHit`] when reached.
//!
//! T-07's rules (TD §14.3–14.5), with tests in `tests::carver_b`:
//! 1. **Inflate probe** (rung d), for data whose filter chain starts with
//!    `/FlateDecode` (or that has no filter and starts with `0x78`): the
//!    input a complete inflate consumes is the extent, trusted only when
//!    `endstream`, or the end of the file, follows within two bytes of
//!    whitespace. An untrusted probe leaves
//!    [`CarveNote::InflateProbeUntrusted`] and the ladder goes on to (e).
//! 2. **Deferred indirect `/Length`** (rung c): after a first assembly pass,
//!    each `/Length N G R` is looked up among the objects carved (the last
//!    top-level copy of `N G`, when it is a non-negative integer) and, when
//!    any resolves, the whole assembly runs once more with those values
//!    (exactly two passes; values packed in object streams are not used). A
//!    value that lands exactly on `endstream` settles the extent
//!    ([`LengthSource::DeclaredIndirect`]), whatever the other rungs said in
//!    the first pass, and the phantom objects the first pass found inside
//!    that data go.
//! 3. **Object-stream expansion** (`containers`): each top-level
//!    `/Type /ObjStm` stream is decoded (a damaged Flate stage keeps what
//!    inflates, [`CarveNote::ObjStmPartial`]) and its `N` pairs read. More
//!    than [`MAX_OBJSTM_ENTRIES`] entries, or a bad `/First`, rejects the
//!    container; an entry that is out of range, out of order, names its own
//!    container or does not parse is skipped ([`CarveNote::ObjStmEntryBad`]).
//!    Packed values are never streams and `/Extends` is not followed, so
//!    expansion never recurses (a visited set keyed by the container's offset
//!    is only a guard). The offsets kept strictly increase.
//! 4. **Container-offset precedence** (D-031, defaulted): `objects` is kept
//!    in container-offset order, each packed object right after its
//!    container, so among the copies of one id ([`CarveReport::copies`]) the
//!    last is the one whose top-level holder starts latest
//!    ([`CarveReport::last_by_container_offset`]). That is D-031's ordering
//!    only: which copy wins is T-10's call, which also applies D-032's gate
//!    (a copy that is `Unparsed` or `TruncatedAtEof` does not win). lopdf and
//!    hayro order the top-level copy last instead;
//!    `with_objstm_and_plain_copy` pins which order is applied.
//! 5. **Xref streams** are decoded for diagnosis only ([`XrefStream`]): their
//!    rows, and their `/Root`, `/Info`, `/ID` and `/Encrypt` entries as
//!    trailer candidates.
//! 6. **Gap sweep** (`gaps`): every run of the file outside objects, xref
//!    tables, trailers and `startxref` values that holds at least 16 bytes
//!    other than whitespace and comments is read: a dictionary there is an
//!    [`Orphan::Dict`] (an [`Orphan::Stream`] when `stream` follows it), a
//!    `stream` keyword whose data ends at `endstream` or a complete inflate
//!    is a dictionary-less [`Orphan::Stream`], and anything else is a
//!    [`CarveNote::UnexplainedSpan`]. Orphans count toward [`MAX_OBJECTS`].
//!    A dictionary there is read no further than the next `endstream` or
//!    `endobj`, and rung (b) looks its framed `endstream`s up in a list made
//!    once, so the sweep stays linear however many orphans a gap holds.
//!
//! F-07's rule (D-130):
//! 7. **Near-miss lex** (`gaps`): once the carve is done, every run outside
//!    the objects, orphans, xref tables, trailers and `startxref` values is
//!    lexed in near-miss mode ([`Lexer::with_near_miss`]), and each keyword
//!    read one byte off (`2 0 obk`, `endob]`) becomes a file-level
//!    [`CarveNote::Lex`] of a [`LexNote::NearMissKeyword`], at most
//!    [`MAX_NEAR_MISSES`] of them. Carved objects and orphans are never lexed
//!    this way, and nothing the earlier rules found changes.
// T-09 is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

mod containers;
mod gaps;
mod landmarks;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::ops::Range;

use lopdf::{Dictionary, Object};
use memchr::memmem;

use crate::engine::Cancelled;
use crate::pdf::lexer::{self, LexNote, Lexer, Tok};
use crate::pdf::model::{ByteSpan, LengthSource, ObjId, ObjectKind};
use crate::pdf::streams::{self, Filter, InflateStatus, StreamClass};
use landmarks::Landmarks;

/// Most landmarks the scan keeps; the rest of the file is not looked at.
pub(crate) const MAX_LANDMARKS: usize = 1_000_000;
/// Most objects a carve holds (lopdf's cap).
pub(crate) const MAX_OBJECTS: usize = 1_000_000;
/// `cancel` is polled once per this many landmarks.
const POLL_EVERY: u64 = 256;
/// Bytes of a typeless stream classified by content: its decoded prefix.
/// The content grammar settles well within this.
const CLASSIFY_CAP: usize = 64 << 10;
/// Classification decodes at most this many bytes per byte of input over the
/// whole carve (and never less than [`CLASSIFY_CAP`]), so a file of small
/// deflate bombs cannot make the carve's work outgrow the file.
const CLASSIFY_BUDGET_PER_BYTE: usize = 16;
/// Most near-miss keywords rule 7 notes; the rest are not looked for.
pub(crate) const MAX_NEAR_MISSES: usize = 1_000;
/// Most entries an object stream may declare (our own bound; lopdf caps a
/// file at the same million objects).
pub(crate) const MAX_OBJSTM_ENTRIES: i64 = 1_000_000;
/// Most xref-stream rows a carve decodes, over every xref stream.
pub(crate) const MAX_XREF_ROWS: usize = 1_000_000;
/// The inflate probe and the object- and xref-stream decoders together
/// decode at most this many bytes per byte of input (and never less than
/// [`DECODE_FLOOR`]), so a file of deflate bombs cannot make the carve's
/// work outgrow the file.
const DECODE_BUDGET_PER_BYTE: usize = 64;
const DECODE_FLOOR: usize = 16 << 20;
/// Deflate's greatest expansion: a 258-byte match in two bits, so no byte
/// of Flate input decodes to more than 1,032 bytes.
const MAX_INFLATE_RATIO: usize = 1032;
/// A probe's end is trusted when `endstream` or EOF follows within this many
/// bytes of whitespace (T-07 rule 1).
const PROBE_SLACK: usize = 2;

/// The `%PDF-` header, wherever it is (junk may come before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderInfo {
    pub(crate) offset: u64,
    /// The digits and dots after `%PDF-`, e.g. `"1.7"`; `None` when there are none.
    pub(crate) version: Option<String>,
}

/// Everything the carve found, in byte order.
#[derive(Debug, Default)]
pub(crate) struct CarveReport {
    pub(crate) header: Option<HeaderInfo>,
    /// In container-offset order: top-level objects by where their header
    /// starts, each object stream's packed objects right after it in index
    /// order (rule 4 rests on this order).
    pub(crate) objects: Vec<CarvedObject>,
    /// What the gap sweep found, in byte order.
    pub(crate) orphans: Vec<Orphan>,
    /// From each classic `xref` keyword to the next landmark (`trailer`,
    /// `startxref`, `%%EOF`, a header) or EOF.
    pub(crate) xref_spans: Vec<ByteSpan>,
    /// Each `trailer` keyword and the dictionary after it.
    pub(crate) trailer_spans: Vec<ByteSpan>,
    /// Each `startxref` keyword's offset and the number after it, if any.
    pub(crate) startxref: Vec<(u64, Option<u64>)>,
    /// Where each `%%EOF` starts.
    pub(crate) eof_markers: Vec<u64>,
    /// Every top-level `/Type /XRef` stream, in byte order (rule 5).
    pub(crate) xref_streams: Vec<XrefStream>,
    /// Notes about the file rather than one object, among them rule 7's
    /// near-miss keywords outside every object.
    pub(crate) notes: Vec<CarveNote>,
    pub(crate) stats: CarveStats,
}

/// One object, as its header declared it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CarvedObject {
    pub(crate) declared_id: ObjId,
    /// From the header's first digit to just past `endobj` (or to the next
    /// header or EOF when `endobj` is missing). A packed object has its
    /// container's span: that is where it is in the file.
    pub(crate) span: ByteSpan,
    pub(crate) body: Body,
    pub(crate) kind: ObjectKind,
    pub(crate) origin: Origin,
    pub(crate) notes: Vec<CarveNote>,
}

/// What an object holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    Dict(Dictionary),
    /// `data` is the stream's raw (still filtered) bytes in the file.
    Stream {
        dict: Dictionary,
        data: ByteSpan,
        length_source: LengthSource,
    },
    /// Any other value: a number, name, string, array, reference or null.
    Primitive(Object),
    /// No value could be read after the header.
    Unparsed,
}

/// Where an object was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    TopLevel,
    /// Packed in this object stream (T-07). Its generation is 0.
    Compressed(ObjId),
}

/// A dictionary or stream with no header in front of it, found by the gap
/// sweep (rule 6). It has no id; rebuild assigns one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Orphan {
    /// `span` runs from `<<` to just past `endobj`, or past `>>` when there
    /// is no `endobj`.
    Dict {
        span: ByteSpan,
        dict: Dictionary,
        kind: ObjectKind,
    },
    /// `dict` is empty when the gap held no dictionary before `stream`.
    /// `span` runs to just past `endobj`, or past the data's end.
    Stream {
        span: ByteSpan,
        dict: Dictionary,
        data: ByteSpan,
        length_source: LengthSource,
        kind: ObjectKind,
    },
}

impl Orphan {
    pub(crate) fn span(&self) -> ByteSpan {
        match self {
            Orphan::Dict { span, .. } | Orphan::Stream { span, .. } => *span,
        }
    }

    pub(crate) fn kind(&self) -> &ObjectKind {
        match self {
            Orphan::Dict { kind, .. } | Orphan::Stream { kind, .. } => kind,
        }
    }

    fn heap_bytes(&self) -> u64 {
        let (dict, kind) = match self {
            Orphan::Dict { dict, kind, .. } | Orphan::Stream { dict, kind, .. } => (dict, kind),
        };
        dict_heap(dict) + kind_heap(kind)
    }
}

/// One cross-reference stream, decoded for diagnosis (rule 5); its rows are
/// never taken as the truth.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct XrefStream {
    pub(crate) id: ObjId,
    pub(crate) span: ByteSpan,
    /// Every row the data holds, in `/Index` order; empty when the data does
    /// not decode or `/W` is unusable.
    pub(crate) rows: Vec<XrefRow>,
    pub(crate) trailer: TrailerKeys,
}

/// One xref-stream row: the object number `/Index` gives it and its three
/// fields (ISO 32000-1 7.5.8.3). A zero-width type field reads as type 1;
/// other zero-width fields read as 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct XrefRow {
    pub(crate) num: u64,
    pub(crate) fields: [u64; 3],
}

/// The trailer entries an xref stream's dictionary carries, as written.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TrailerKeys {
    pub(crate) root: Option<Object>,
    pub(crate) info: Option<Object>,
    pub(crate) id: Option<Object>,
    pub(crate) encrypt: Option<Object>,
}

/// Something the carve tolerated or gave up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarveNote {
    /// A header at `at` whose number overflows `u32` or whose generation
    /// exceeds 65,535; it was ignored.
    HeaderOutOfRange { at: u64 },
    /// `stream` was followed by a bare CR at `at`. qpdf, pdf.js, hayro and
    /// lopdf accept it; ISO 32000-1 7.3.8.1 does not (D-030).
    BareCrAfterStream { at: u64 },
    /// The `/Length` (direct, or indirect and resolved) did not land on
    /// `endstream`.
    LengthMismatch { declared: i64 },
    /// The indirect `/Length` names no object carved as a non-negative
    /// integer.
    LengthUnresolved(ObjId),
    /// A complete inflate ended at `end`, but neither `endstream` nor EOF
    /// follows within two bytes: evidence only, the extent was not taken.
    InflateProbeUntrusted { end: u64 },
    /// Rung (b) found this many framed `endstream`s, so it chose none.
    EndstreamAmbiguous { candidates: u32 },
    /// No `endstream` before the next header: the data runs up to it.
    NoEndstream,
    /// No `endobj` before the next header: the object ends there (or at EOF).
    NoEndobj,
    /// A cap was reached and the rest was not carved.
    CapHit(Cap),
    /// The lexer tolerated something in the object's value. For a packed
    /// object, `at` counts in its container's decoded data. Among the
    /// report's own notes it is a rule 7 near-miss keyword, at a file offset.
    Lex(LexNote),
    /// On an object stream: none of it was expanded.
    ObjStmRejected(ObjStmFault),
    /// On an object stream: its data decoded only in part, and the entries
    /// were read from what decoded.
    ObjStmPartial,
    /// On an object stream: entry `index` (from 0) was skipped.
    ObjStmEntryBad { index: u32, fault: EntryFault },
    /// On an xref stream: its data did not decode into rows.
    XrefStreamUndecodable,
    /// A gap between objects with 16 or more bytes that are neither
    /// whitespace nor comments, and no orphan in it; `span` starts at its
    /// first such byte.
    UnexplainedSpan { span: ByteSpan },
}

/// The carve's caps (rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cap {
    Landmarks,
    Objects,
    /// [`MAX_XREF_ROWS`].
    XrefRows,
    /// [`MAX_NEAR_MISSES`].
    NearMisses,
}

/// Why an object stream was not expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjStmFault {
    /// `/N` is missing, negative or above [`MAX_OBJSTM_ENTRIES`].
    BadCount,
    /// `/First` is missing, negative or past the decoded data.
    BadFirst,
    /// The filter chain did not decode.
    Undecodable,
}

/// Why an object-stream entry was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryFault {
    /// The index holds fewer pairs than `/N`; noted once, at the first
    /// missing index.
    Missing,
    /// The object number does not fit `u32`, or the offset is negative or
    /// past the data.
    OutOfRange,
    /// The offset is not above the previous in-range one or not below the
    /// next, or not above an earlier entry that was kept.
    NonMonotonic,
    /// The entry names its own container.
    SelfReference,
    /// No value could be read at the offset.
    Unparsed,
}

/// The carve in numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CarveStats {
    /// Landmarks the scan kept.
    pub(crate) landmarks: u64,
    pub(crate) rungs: RungCounts,
}

/// Streams whose extent each rung of the ladder settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RungCounts {
    /// (a) the direct `/Length`.
    pub(crate) declared: u32,
    /// (b) the one framed `endstream`.
    pub(crate) unique_endstream: u32,
    /// (c) an indirect `/Length`.
    pub(crate) deferred_length: u32,
    /// (d) the inflate probe.
    pub(crate) inflate_probe: u32,
    /// (e) the last `endstream` before the next header.
    pub(crate) last_endstream: u32,
    /// (f) no `endstream`: EOF or the next header.
    pub(crate) eof: u32,
}

impl RungCounts {
    fn add(&mut self, o: RungCounts) {
        self.declared += o.declared;
        self.unique_endstream += o.unique_endstream;
        self.deferred_length += o.deferred_length;
        self.inflate_probe += o.inflate_probe;
        self.last_endstream += o.last_endstream;
        self.eof += o.eof;
    }
}

impl CarveReport {
    /// Every copy of `id` in container-offset order (rule 4, D-031): by the
    /// offset of the top-level object that holds each copy, the object's own
    /// for a top-level copy and its object stream's for a packed one.
    pub(crate) fn copies(&self, id: ObjId) -> impl Iterator<Item = &CarvedObject> {
        self.objects.iter().filter(move |o| o.declared_id == id)
    }

    /// The last of [`CarveReport::copies`]: the copy whose top-level holder
    /// starts latest. This applies D-031's ordering only, not which copy
    /// wins: T-10 decides that and applies D-032's gate on top (a copy that
    /// is `Unparsed` or `TruncatedAtEof` does not win), so this may be a
    /// copy that loses.
    pub(crate) fn last_by_container_offset(&self, id: ObjId) -> Option<&CarvedObject> {
        self.copies(id).last()
    }

    /// Capacity of every owned `Vec` and `String`, in bytes; a dictionary
    /// counts its map's entry and index slots.
    pub(crate) fn heap_bytes(&self) -> u64 {
        let header = self
            .header
            .as_ref()
            .and_then(|h| h.version.as_ref())
            .map_or(0, |v| v.capacity() as u64);
        let objects: u64 = self.objects.iter().map(CarvedObject::heap_bytes).sum();
        let orphans: u64 = self.orphans.iter().map(Orphan::heap_bytes).sum();
        let xref: u64 = self.xref_streams.iter().map(XrefStream::heap_bytes).sum();
        header
            + vec_bytes(&self.objects)
            + objects
            + vec_bytes(&self.orphans)
            + orphans
            + vec_bytes(&self.xref_streams)
            + xref
            + vec_bytes(&self.xref_spans)
            + vec_bytes(&self.trailer_spans)
            + vec_bytes(&self.startxref)
            + vec_bytes(&self.eof_markers)
            + vec_bytes(&self.notes)
    }
}

impl CarvedObject {
    /// Heap bytes this object owns (not its own slot in the list).
    fn heap_bytes(&self) -> u64 {
        let body = match &self.body {
            Body::Dict(d) | Body::Stream { dict: d, .. } => dict_heap(d),
            Body::Primitive(o) => object_heap(o),
            Body::Unparsed => 0,
        };
        body + kind_heap(&self.kind) + vec_bytes(&self.notes)
    }
}

impl XrefStream {
    fn heap_bytes(&self) -> u64 {
        let t = &self.trailer;
        let keys: u64 = [&t.root, &t.info, &t.id, &t.encrypt]
            .into_iter()
            .flatten()
            .map(object_heap)
            .sum();
        vec_bytes(&self.rows) + keys
    }
}

fn kind_heap(kind: &ObjectKind) -> u64 {
    match kind {
        ObjectKind::Other(s) => s.capacity() as u64,
        _ => 0,
    }
}

fn vec_bytes<T>(v: &Vec<T>) -> u64 {
    (v.capacity() * size_of::<T>()) as u64
}

fn object_heap(o: &Object) -> u64 {
    match o {
        Object::Name(v) | Object::String(v, _) => v.capacity() as u64,
        Object::Array(a) => vec_bytes(a) + a.iter().map(object_heap).sum::<u64>(),
        Object::Dictionary(d) => dict_heap(d),
        Object::Stream(s) => dict_heap(&s.dict) + s.content.capacity() as u64,
        _ => 0,
    }
}

/// An `IndexMap`'s entries (hash, key, value) and its index table, plus what
/// each key and value owns.
fn dict_heap(d: &Dictionary) -> u64 {
    let map = d.as_hashmap();
    let slot = size_of::<(u64, Vec<u8>, Object)>() + size_of::<usize>() + 1;
    let entries: u64 = map
        .iter()
        .map(|(k, v)| k.capacity() as u64 + object_heap(v))
        .sum();
    (map.capacity() * slot) as u64 + entries
}

/// Counts landmarks and polls `cancel` once every [`POLL_EVERY`].
struct Poll<'a> {
    cancel: &'a dyn Fn() -> bool,
    steps: u64,
}

impl Poll<'_> {
    fn tick(&mut self) -> Result<(), Cancelled> {
        let due = self.steps.is_multiple_of(POLL_EVERY);
        self.steps += 1;
        if due && (self.cancel)() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The caps, apart so tests can reach them on small inputs.
#[derive(Debug, Clone, Copy)]
struct Caps {
    landmarks: usize,
    objects: usize,
}

const CAPS: Caps = Caps {
    landmarks: MAX_LANDMARKS,
    objects: MAX_OBJECTS,
};

/// Carves `buf`. Never fails but by cancellation: damage shows up as notes,
/// `Unparsed` bodies and scanned extents. `cancel` is polled every 256
/// landmarks.
pub(crate) fn carve(buf: &[u8], cancel: &dyn Fn() -> bool) -> Result<CarveReport, Cancelled> {
    carve_with(buf, cancel, CAPS)
}

fn carve_with(buf: &[u8], cancel: &dyn Fn() -> bool, caps: Caps) -> Result<CarveReport, Cancelled> {
    let mut poll = Poll { cancel, steps: 0 };
    let lm = landmarks::scan(buf, caps.landmarks, &mut poll)?;
    let mut report = CarveReport {
        header: header_info(buf),
        stats: CarveStats {
            landmarks: lm.count as u64,
            rungs: RungCounts::default(),
        },
        ..CarveReport::default()
    };
    if lm.capped {
        report.notes.push(CarveNote::CapHit(Cap::Landmarks));
    }
    let mut decoding = Decoding::new(buf.len());
    // Rule 2: a first pass finds the number objects indirect lengths name,
    // and a second pass, when any resolves, carves with their values.
    let mut lengths = BTreeMap::new();
    let mut pass = assemble(buf, &lm, &lengths, caps.objects, &mut poll, &mut decoding)?;
    let resolved = indirect_lengths(&pass.objects);
    if !resolved.is_empty() {
        lengths = resolved;
        pass = assemble(buf, &lm, &lengths, caps.objects, &mut poll, &mut decoding)?;
    }
    report.objects = pass.objects;
    report.stats.rungs = pass.rungs;
    if pass.capped {
        report.notes.push(CarveNote::CapHit(Cap::Objects));
    }
    let mut classify = Classifier::new(buf.len());
    for o in &mut report.objects {
        if let Body::Stream { dict, data, .. } = &o.body {
            o.kind = classify.kind(dict, &buf[range(*data)]);
        }
    }
    structure(buf, &lm, &pass.dead, &mut poll, &mut report)?;
    containers::expand_objstms(buf, caps.objects, &mut decoding, &mut poll, &mut report)?;
    containers::xref_streams(buf, &mut decoding, &mut report);
    let mut ladder = Ladder {
        buf,
        lm: &lm,
        lengths: &lengths,
        decoding: &mut decoding,
        rungs: RungCounts::default(),
    };
    gaps::sweep(
        &mut ladder,
        &mut classify,
        caps.objects,
        &mut poll,
        &mut report,
    )?;
    report.stats.rungs.add(ladder.rungs);
    gaps::near_misses(buf, &lm, &mut poll, &mut report)?;
    Ok(report)
}

fn header_info(buf: &[u8]) -> Option<HeaderInfo> {
    let offset = memmem::find(buf, b"%PDF-")?;
    let rest = &buf[offset + 5..];
    let len = rest
        .iter()
        .take(16)
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    let version = (len > 0).then(|| String::from_utf8_lossy(&rest[..len]).into_owned());
    Some(HeaderInfo {
        offset: offset as u64,
        version,
    })
}

/// One assembly pass: the objects, the data ranges of the streams carved
/// (sorted: the dead ranges), what each rung settled, and whether the object
/// cap stopped it.
struct Assembly {
    objects: Vec<CarvedObject>,
    dead: Vec<Range<usize>>,
    rungs: RungCounts,
    capped: bool,
}

/// Phase B (TD §14.2): every live header in byte order. `lengths` holds the
/// indirect `/Length` values known so far (rule 2). A stream's kind is left
/// empty: it is classified once the last pass is done.
fn assemble(
    buf: &[u8],
    lm: &Landmarks,
    lengths: &BTreeMap<ObjId, i64>,
    max_objects: usize,
    poll: &mut Poll<'_>,
    decoding: &mut Decoding,
) -> Result<Assembly, Cancelled> {
    let mut ladder = Ladder {
        buf,
        lm,
        lengths,
        decoding,
        rungs: RungCounts::default(),
    };
    let mut out = Assembly {
        objects: Vec::new(),
        dead: Vec::new(),
        rungs: RungCounts::default(),
        capped: false,
    };
    let mut cursor = 0;
    for (i, h) in lm.headers.iter().enumerate() {
        poll.tick()?;
        if h.at < cursor {
            continue;
        }
        if out.objects.len() == max_objects {
            out.capped = true;
            break;
        }
        let next = lm
            .headers
            .get(i + 1)
            .map_or(buf.len(), |n| n.at.max(h.obj_end));
        let obj = carve_object(&mut ladder, h, next);
        if let Body::Stream { data, .. } = &obj.body {
            out.dead.push(range(*data));
        }
        cursor = obj.span.end as usize;
        out.objects.push(obj);
    }
    out.rungs = ladder.rungs;
    Ok(out)
}

/// Rule 2: the value of each indirect `/Length` among `objects`: the last
/// top-level copy of the object it names, when that is a non-negative
/// integer.
fn indirect_lengths(objects: &[CarvedObject]) -> BTreeMap<ObjId, i64> {
    let wanted: std::collections::BTreeSet<ObjId> = objects
        .iter()
        .filter_map(|o| match &o.body {
            Body::Stream { dict, .. } => match dict.get(b"Length") {
                Ok(Object::Reference(id)) => Some(*id),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut out = BTreeMap::new();
    for o in objects.iter().filter(|o| wanted.contains(&o.declared_id)) {
        match o.body {
            Body::Primitive(Object::Integer(n)) if n >= 0 => {
                out.insert(o.declared_id, n);
            }
            _ => {
                out.remove(&o.declared_id);
            }
        }
    }
    out
}

/// The object whose header is `h`; `next` is where the next header starts
/// (or EOF), the end of the window its value is read in.
fn carve_object(ladder: &mut Ladder<'_>, h: &landmarks::Header, next: usize) -> CarvedObject {
    let buf = ladder.buf;
    let window = &buf[..next];
    let mut notes = Vec::new();
    let (value, after) = match lexer::parse_value(window, h.obj_end, 0) {
        Ok(p) => {
            notes.extend(p.notes.into_iter().map(CarveNote::Lex));
            (Some(p.value), p.end)
        }
        Err(_) => (None, h.obj_end),
    };
    let mut lx = Lexer::new(window, after);
    let stream_follows =
        matches!(value, None | Some(Object::Dictionary(_))) && lx.next() == Tok::Kw(&b"stream"[..]);
    let (body, kind, end) = if stream_follows {
        let dict = match value {
            Some(Object::Dictionary(d)) => d,
            _ => Dictionary::new(),
        };
        let ext = ladder.extent(&dict, lx.pos, buf.len(), &mut notes);
        let end = close(buf, ladder.lm, ext.resume, &mut notes);
        let body = Body::Stream {
            dict,
            data: span(ext.data),
            length_source: ext.source,
        };
        (body, ObjectKind::Other(String::new()), end)
    } else {
        let end = close(buf, ladder.lm, after, &mut notes);
        let (body, kind) = match value {
            Some(Object::Dictionary(d)) => {
                let kind = dict_kind(&d);
                (Body::Dict(d), kind)
            }
            Some(o) => (Body::Primitive(o), ObjectKind::Other(String::new())),
            None => (Body::Unparsed, ObjectKind::Other(String::new())),
        };
        (body, kind, end)
    };
    CarvedObject {
        declared_id: (h.num, h.gen_nr),
        span: span(h.at..end),
        body,
        kind,
        origin: Origin::TopLevel,
        notes,
    }
}

fn span(r: Range<usize>) -> ByteSpan {
    ByteSpan {
        start: r.start as u64,
        end: r.end as u64,
    }
}

fn range(s: ByteSpan) -> Range<usize> {
    s.start as usize..s.end as usize
}

/// Where the next header at or after `from` starts.
fn next_header(lm: &Landmarks, from: usize) -> Option<usize> {
    let i = lm.headers.partition_point(|h| h.at < from);
    lm.headers.get(i).map(|h| h.at)
}

/// The landmarks in `list` within `range`.
fn within(list: &[usize], range: Range<usize>) -> &[usize] {
    let lo = list.partition_point(|&at| at < range.start);
    let hi = list.partition_point(|&at| at < range.end);
    &list[lo..hi.max(lo)]
}

/// Rule 5: just past the first `endobj` from `from` before the next header;
/// without one, the next header or EOF.
fn close(buf: &[u8], lm: &Landmarks, from: usize, notes: &mut Vec<CarveNote>) -> usize {
    let next = next_header(lm, from).unwrap_or(buf.len());
    match within(&lm.endobjs, from..next).first() {
        Some(&at) => at + b"endobj".len(),
        None => {
            notes.push(CarveNote::NoEndobj);
            next
        }
    }
}

/// A stream's data and how its end was settled. `resume` is where `endobj`
/// is looked for: past `endstream` when there is one.
struct Extent {
    data: Range<usize>,
    source: LengthSource,
    resume: usize,
}

/// Bytes the carve may still decode, and the probes already run.
struct Decoding {
    left: usize,
    /// `(data start, region end)` → the input a `Done` probe consumed.
    probes: BTreeMap<(usize, usize), Option<usize>>,
}

impl Decoding {
    fn new(file_len: usize) -> Self {
        Self {
            left: file_len
                .saturating_mul(DECODE_BUDGET_PER_BYTE)
                .max(DECODE_FLOOR),
            probes: BTreeMap::new(),
        }
    }

    /// The most one decode may produce now.
    fn cap(&self) -> usize {
        self.left.min(streams::DEFAULT_CAP)
    }

    fn spend(&mut self, n: usize) {
        self.left -= n.min(self.left);
    }

    /// `raw` through `chain` within [`Decoding::cap`]. What decodes is
    /// charged. A decode that fails may have produced output before failing
    /// (an Adler mismatch is found after a full inflate), so it is charged
    /// the most its input could have produced ([`output_bound`]), and the
    /// whole cap when it reached the cap or its chain has no such bound. So
    /// a file of failing decodes cannot outgrow the budget either, and one
    /// small damaged stream does not use up what the others need.
    fn decode(&mut self, raw: &[u8], chain: &[(Filter, Option<Dictionary>)]) -> Option<Vec<u8>> {
        let cap = self.cap();
        match streams::decode_chain(raw, chain, cap) {
            Ok(out) => {
                self.spend(out.len());
                Some(out)
            }
            Err(e) => {
                let bound = match e {
                    streams::DecodeError::CapHit => None,
                    _ => output_bound(raw.len(), chain),
                };
                self.spend(bound.map_or(cap, |b| b.min(cap)));
                None
            }
        }
    }

    /// Rule 1's probe over `buf[start..end]`: the input a complete inflate
    /// consumes, if it completes. Each region is probed once per carve.
    fn probe(&mut self, buf: &[u8], start: usize, end: usize) -> Option<usize> {
        if let Some(&known) = self.probes.get(&(start, end)) {
            return known;
        }
        let cap = self.cap();
        let done = if cap == 0 {
            None
        } else {
            let p = streams::probe(&buf[start..end], cap);
            self.spend(p.produced);
            (p.status == InflateStatus::Done).then_some(p.consumed)
        };
        self.probes.insert((start, end), done);
        done
    }
}

/// The most every stage of `chain` together can produce from `len` bytes,
/// stage by stage; `None` when a stage's output is not bounded by a
/// multiple of its input (LZW, an unknown filter).
fn output_bound(len: usize, chain: &[(Filter, Option<Dictionary>)]) -> Option<usize> {
    chain.iter().try_fold(len, |n, (f, _)| {
        let ratio = match f {
            Filter::Flate => MAX_INFLATE_RATIO,
            // `z` is one byte for four zeros.
            Filter::Ascii85 => 4,
            // A length byte and one byte for up to 128 copies.
            Filter::RunLength => 64,
            // Two digits a byte; an image codec's data passes through.
            Filter::AsciiHex | Filter::Dct | Filter::Jpx | Filter::Ccitt | Filter::Jbig2 => 1,
            Filter::Lzw | Filter::Unknown(_) => return None,
        };
        Some(n.saturating_mul(ratio))
    })
}

/// What the extent ladder works with: the file, its landmarks, the indirect
/// lengths known (rule 2), the decode budget and what each rung settled.
struct Ladder<'a> {
    buf: &'a [u8],
    lm: &'a Landmarks,
    lengths: &'a BTreeMap<ObjId, i64>,
    decoding: &'a mut Decoding,
    rungs: RungCounts,
}

impl Ladder<'_> {
    /// Rule 3, the extent ladder. `kw_end` is just past the `stream`
    /// keyword; no rung looks at or past `limit` (EOF for an object, the
    /// end of its gap for an orphan).
    fn extent(
        &mut self,
        dict: &Dictionary,
        kw_end: usize,
        limit: usize,
        notes: &mut Vec<CarveNote>,
    ) -> Extent {
        let buf = self.buf;
        let start = data_start(buf, kw_end, notes).min(limit);
        let region_end = next_header(self.lm, start).unwrap_or(buf.len()).min(limit);
        let scanned = |end: usize, at: usize| Extent {
            data: start..end,
            source: LengthSource::ScannedEndstream,
            resume: at + b"endstream".len(),
        };
        let fits = |declared: i64| declared_fit(buf, start, declared).filter(|f| f.1 <= limit);

        // (a) A direct /Length that lands exactly on EOL + `endstream`, and
        // (c) an indirect one, once pass 1 has found its value.
        match dict.get(b"Length") {
            Ok(&Object::Integer(declared)) => {
                if let Some((end, resume)) = fits(declared) {
                    self.rungs.declared += 1;
                    return Extent {
                        data: start..end,
                        source: LengthSource::Declared,
                        resume,
                    };
                }
                notes.push(CarveNote::LengthMismatch { declared });
            }
            Ok(&Object::Reference(id)) => match self.lengths.get(&id) {
                Some(&declared) => {
                    if let Some((end, resume)) = fits(declared) {
                        self.rungs.deferred_length += 1;
                        return Extent {
                            data: start..end,
                            source: LengthSource::DeclaredIndirect(id),
                            resume,
                        };
                    }
                    notes.push(CarveNote::LengthMismatch { declared });
                }
                None => notes.push(CarveNote::LengthUnresolved(id)),
            },
            _ => {}
        }

        // (b) The unique-endstream rule (lopdf's definition of the data's end).
        let candidates = within(&self.lm.endstreams, start..region_end);
        match within(&self.lm.framed_endstreams, start..region_end) {
            [at] => {
                self.rungs.unique_endstream += 1;
                return scanned(strip_eol(buf, start, *at), *at);
            }
            [] => {}
            many => notes.push(CarveNote::EndstreamAmbiguous {
                candidates: u32::try_from(many.len()).unwrap_or(u32::MAX),
            }),
        }

        // (d) The inflate probe, with the probe-end guard.
        if looks_deflated(dict, &buf[start..region_end])
            && let Some(consumed) = self.decoding.probe(buf, start, region_end)
        {
            let end = start + consumed;
            match probe_end(buf, end) {
                Some(resume) => {
                    self.rungs.inflate_probe += 1;
                    return Extent {
                        data: start..end,
                        source: LengthSource::InflateProbe,
                        resume,
                    };
                }
                None => notes.push(CarveNote::InflateProbeUntrusted { end: end as u64 }),
            }
        }

        // (e) The last `endstream` before the next header.
        if let Some(&at) = candidates.last() {
            self.rungs.last_endstream += 1;
            return scanned(strip_eol(buf, start, at), at);
        }

        // (f) No `endstream` at all: the data runs to EOF, or up to the next
        // header (or the gap's end), never past it. Both are
        // `TruncatedAtEof`; only the `NoEndstream` note tells a cut at the
        // next header from a file that ended inside the stream.
        self.rungs.eof += 1;
        let end = if region_end == buf.len() {
            region_end
        } else {
            notes.push(CarveNote::NoEndstream);
            strip_eol(buf, start, region_end)
        };
        Extent {
            data: start..end,
            source: LengthSource::TruncatedAtEof,
            resume: region_end,
        }
    }
}

/// Rule 1: data is probed when its chain starts with Flate, or when it has
/// no filter and starts like a zlib stream (`0x78`).
fn looks_deflated(dict: &Dictionary, data: &[u8]) -> bool {
    match streams::filters_of(dict).first() {
        Some((f, _)) => *f == Filter::Flate,
        None => data.first() == Some(&0x78),
    }
}

/// The probe-end guard: past at most [`PROBE_SLACK`] whitespace bytes after
/// `end` comes `endstream` (returns the offset past it) or EOF (returns it).
fn probe_end(buf: &[u8], end: usize) -> Option<usize> {
    (0..=PROBE_SLACK)
        .take_while(|&k| k == 0 || buf.get(end + k - 1).is_some_and(|&b| lexer::is_ws(b)))
        .find_map(|k| {
            let at = end + k;
            if at == buf.len() {
                Some(at)
            } else {
                buf.get(at..)
                    .filter(|rest| rest.starts_with(b"endstream"))
                    .map(|_| at + b"endstream".len())
            }
        })
}

/// Rule 4: where the data starts after `stream` (ending at `kw_end`): past
/// CRLF, LF or a bare CR, each optionally after one space. Anything else:
/// right after the keyword.
fn data_start(buf: &[u8], kw_end: usize, notes: &mut Vec<CarveNote>) -> usize {
    let eol_at = |i: usize| matches!(buf.get(i), Some(b'\r' | b'\n'));
    let i = if buf.get(kw_end) == Some(&b' ') && eol_at(kw_end + 1) {
        kw_end + 1
    } else {
        kw_end
    };
    match (buf.get(i), buf.get(i + 1)) {
        (Some(b'\r'), Some(b'\n')) => i + 2,
        (Some(b'\n'), _) => i + 1,
        (Some(b'\r'), _) => {
            notes.push(CarveNote::BareCrAfterStream { at: i as u64 });
            i + 1
        }
        _ => kw_end,
    }
}

/// The length of the EOL at `at` (CRLF, LF or CR), 0 when there is none.
fn eol_len(buf: &[u8], at: usize) -> usize {
    match (buf.get(at), buf.get(at + 1)) {
        (Some(b'\r'), Some(b'\n')) => 2,
        (Some(b'\r' | b'\n'), _) => 1,
        _ => 0,
    }
}

/// Rung (a): `start + declared` followed by an optional EOL and `endstream`.
/// Returns the data's end and the offset just past `endstream`.
fn declared_fit(buf: &[u8], start: usize, declared: i64) -> Option<(usize, usize)> {
    let end = start.checked_add(usize::try_from(declared).ok()?)?;
    if end > buf.len() {
        return None;
    }
    let kw = end + eol_len(buf, end);
    buf[kw..]
        .starts_with(b"endstream")
        .then_some((end, kw + b"endstream".len()))
}

/// Rung (b)'s framing: an EOL right before `endstream` at `at`, and after it
/// optional whitespace and `endobj`.
fn framed_endstream(buf: &[u8], at: usize) -> bool {
    let eol_before = at > 0 && matches!(buf[at - 1], b'\r' | b'\n');
    let after = at + b"endstream".len();
    let ws = buf[after..]
        .iter()
        .take_while(|&&b| lexer::is_ws(b))
        .count();
    eol_before && buf[after + ws..].starts_with(b"endobj")
}

/// `at` less the EOL right before it, never before `start`.
fn strip_eol(buf: &[u8], start: usize, at: usize) -> usize {
    let end = if at >= 2 && &buf[at - 2..at] == b"\r\n" {
        at - 2
    } else if at >= 1 && matches!(buf[at - 1], b'\r' | b'\n') {
        at - 1
    } else {
        at
    };
    end.max(start).min(at)
}

/// A dictionary's kind from its `/Type`.
fn dict_kind(d: &Dictionary) -> ObjectKind {
    match name(d, b"Type") {
        Some(b"Catalog") => ObjectKind::Catalog,
        Some(b"Pages") => ObjectKind::Pages,
        Some(b"Page") => ObjectKind::Page,
        Some(b"Font") => ObjectKind::Font,
        Some(b"FontDescriptor") => ObjectKind::FontDescriptor,
        Some(b"Sig") => ObjectKind::Sig,
        _ => named(d),
    }
}

/// Classifies streams, holding what is left of the carve's decode budget.
struct Classifier {
    left: usize,
}

impl Classifier {
    fn new(file_len: usize) -> Self {
        Self {
            left: file_len
                .saturating_mul(CLASSIFY_BUDGET_PER_BYTE)
                .max(CLASSIFY_CAP),
        }
    }

    /// A stream's kind: from its dictionary when that decides it (T-06's
    /// [`streams::classify`]), else from the first [`CLASSIFY_CAP`] bytes of
    /// its decoded data (TD §4.2 step 5: a typeless stream is classified by
    /// content). Once the budget is spent, typeless streams are classified
    /// on an empty prefix, that is by their dictionary alone.
    fn kind(&mut self, dict: &Dictionary, raw: &[u8]) -> ObjectKind {
        let class = match streams::classify(dict, &[]) {
            StreamClass::Other => streams::classify(dict, &self.prefix(dict, raw)),
            class => class,
        };
        match class {
            StreamClass::Content => ObjectKind::ContentStream,
            StreamClass::Image { .. } => ObjectKind::Image,
            StreamClass::Form => ObjectKind::Form,
            StreamClass::FontFile => ObjectKind::FontFile,
            StreamClass::CMap if name(dict, b"Type") != Some(b"CMap") => ObjectKind::ToUnicode,
            StreamClass::ObjStm => ObjectKind::ObjStm,
            StreamClass::XRef => ObjectKind::XRefStream,
            StreamClass::CMap | StreamClass::Metadata | StreamClass::Other => named(dict),
        }
    }

    /// What classification reads: at most [`CLASSIFY_CAP`] bytes, the raw
    /// bytes when unfiltered. A chain is decoded a stage at a time; a final
    /// Flate stage keeps whatever inflates before the cap or the first error
    /// (so a C9 stream, or a long one, still classifies), and any other stage
    /// that fails leaves nothing. A stage before the last may grow to the
    /// size of its input. Every stage's output, or its cap when it fails, is
    /// charged to the budget.
    fn prefix<'a>(&mut self, dict: &Dictionary, raw: &'a [u8]) -> Cow<'a, [u8]> {
        let chain = streams::filters_of(dict);
        if chain.is_empty() {
            return Cow::Borrowed(&raw[..raw.len().min(CLASSIFY_CAP)]);
        }
        let mut data = Cow::Borrowed(raw);
        for (i, stage) in chain.iter().enumerate() {
            let last = i + 1 == chain.len();
            let wanted = if last {
                CLASSIFY_CAP
            } else {
                CLASSIFY_CAP.max(data.len())
            };
            let cap = wanted.min(self.left);
            if cap == 0 {
                return Cow::Owned(Vec::new());
            }
            let out = if last && stage.0 == Filter::Flate {
                streams::inflate(&data, cap).out
            } else {
                match streams::decode_chain(&data, std::slice::from_ref(stage), cap) {
                    Ok(out) => out,
                    Err(_) => {
                        self.left -= cap;
                        return Cow::Owned(Vec::new());
                    }
                }
            };
            self.left -= out.len().min(self.left);
            data = Cow::Owned(out);
        }
        data
    }
}

/// `ObjectKind::Other` named by `/Type`, else `/Subtype`, else empty.
fn named(d: &Dictionary) -> ObjectKind {
    let label = name(d, b"Type")
        .or_else(|| name(d, b"Subtype"))
        .map_or_else(String::new, |n| format!("/{}", String::from_utf8_lossy(n)));
    ObjectKind::Other(label)
}

fn name<'d>(d: &'d Dictionary, key: &[u8]) -> Option<&'d [u8]> {
    d.get(key).ok().and_then(|o| o.as_name().ok())
}

/// The file-structure landmarks outside stream data (`dead`), for diagnosis:
/// xref and trailer spans, `startxref` values, `%%EOF` markers, and a note
/// for each out-of-range header. Each value is read only up to the next
/// structural landmark ([`region_end`]), so the windows are disjoint and the
/// pass is linear however the keywords are packed; `poll` ticks once per
/// landmark visited.
fn structure(
    buf: &[u8],
    lm: &Landmarks,
    dead: &[Range<usize>],
    poll: &mut Poll<'_>,
    report: &mut CarveReport,
) -> Result<(), Cancelled> {
    let live = |at: usize| {
        let i = dead.partition_point(|r| r.start <= at);
        i == 0 || at >= dead[i - 1].end
    };
    for &at in &lm.bad_headers {
        poll.tick()?;
        if live(at) {
            report
                .notes
                .push(CarveNote::HeaderOutOfRange { at: at as u64 });
        }
    }
    for &at in &lm.eofs {
        poll.tick()?;
        if live(at) {
            report.eof_markers.push(at as u64);
        }
    }
    for &at in &lm.startxrefs {
        poll.tick()?;
        if live(at) {
            let window = &buf[..region_end(buf, lm, at)];
            let value = match Lexer::new(window, at + b"startxref".len()).next() {
                Tok::Int(n) => u64::try_from(n).ok(),
                _ => None,
            };
            report.startxref.push((at as u64, value));
        }
    }
    for &at in &lm.trailers {
        poll.tick()?;
        if live(at) {
            let from = at + b"trailer".len();
            let window = &buf[..region_end(buf, lm, at)];
            let end = match lexer::parse_value(window, from, 0) {
                Ok(p) if matches!(p.value, Object::Dictionary(_)) => p.end,
                _ => from,
            };
            report.trailer_spans.push(span(at..end));
        }
    }
    for &at in &lm.xrefs {
        poll.tick()?;
        if live(at) {
            report.xref_spans.push(span(at..region_end(buf, lm, at)));
        }
    }
    Ok(())
}

/// Where the next structural landmark after the one at `at` starts: a
/// `trailer`, `startxref`, `xref`, `%%EOF` or header; EOF when there is none.
fn region_end(buf: &[u8], lm: &Landmarks, at: usize) -> usize {
    let first_after = |list: &[usize]| {
        let i = list.partition_point(|&x| x <= at);
        list.get(i).copied()
    };
    [
        first_after(&lm.trailers),
        first_after(&lm.startxrefs),
        first_after(&lm.eofs),
        first_after(&lm.xrefs),
        next_header(lm, at + 1),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(buf.len())
}
