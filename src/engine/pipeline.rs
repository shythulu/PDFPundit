//! The bodies behind [`Pdfpundit`](super::Pdfpundit) (T-14): analysis,
//! planning and repair, wired from carve, salvage, graph, diagnose, plan,
//! generate-and-validate, emit and verify.
//!
//! **Analysis** runs four phases, each named to the [`Progress`] sink:
//! `"carving"` (the carve and the object graph), `"diagnosing"` (the cheap
//! probe, every Flate stream inflated once under no search work; the
//! findings over it, each told at once, every damaged stream's C9 finding
//! [`provisional`]; and file metadata), `"salvage"` (every Flate stream's C9
//! outcome under the budget, and the findings over it: each one that
//! differs from what was told, a graded C9 finding above all, is told
//! again under its id) and `"measuring"` (the text baseline, its paint
//! counts and the font slots). So a C9 file shows its findings before any
//! salvage work is charged (F-09). The result's findings are the ones over
//! the budgeted salvage, as if the probe had never run; when every stream
//! inflated clean or is over the size limit, the probe is that salvage and
//! no second search or diagnosis runs. Cancellation is polled inside the
//! carve and both salvages and between phases. What it built is kept in
//! the result's [`StateHandle`], with the salvage budget it ran under.
//!
//! **Repair** reuses that state when it is present, was built from the same
//! bytes (by SHA-256) and under the salvage budget `opts` names; otherwise it
//! analyses again and says why in [`AnalysisStateUse::Rebuilt`]. A repair
//! that rebuilds reports the findings it rebuilt, not the caller's (they may
//! be of other bytes), and streams none of them again.
//!
//! **The report** (D-073) is filled from the analysis, the settings, the
//! chosen candidate and the questions asked, and from nothing else: every
//! [`Interact::ask`] goes through a recorder that numbers the request and
//! appends it and its reply to `interactions`. `findings_after` is the
//! chosen output re-diagnosed without any C9 search (D-074). `shadows` lists
//! every carved copy of an object that lost to a later one (D-031, D-032).
//! `signed_note` is [`SIGNED_NOTE`] when the input carries a signature
//! (D-052).
//!
//! **Status**: `Failed` when there is no output (an encrypted file, no
//! candidate built, or none passed V0), `Partial` with each partial pass's
//! reason, else `Ok`. A file with nothing to repair is `Ok` with no output.

use std::collections::BTreeMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use super::slots::font_slots;
use super::{
    AnalysisResult, AnalysisState, AnalysisStateUse, AnalyzeOptions, AnalyzeStats, Answer,
    Cancelled, CarveSummary, FontDb, Interact, InteractionRequest, InteractionRequestId, LogLevel,
    OutcomeStatus, Progress, RepairOptions, RepairOutcome, RepairPlan, RepairReport, SIGNED_NOTE,
    SalvageBudget, StateHandle, XrefKind,
};
use crate::pdf::carver::{Body, CarveNote, CarveReport, Orphan, carve};
use crate::pdf::diagnose::{diagnose, provisional};
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::meta::file_meta;
use crate::pdf::model::{ByteSpan, CorruptionClass, Finding, FindingKind, ObjId};
use crate::pdf::plan;
use crate::pdf::repair::{Analysed, generate_and_validate};
use crate::pdf::streams::salvage::{CarveSource, Salvage, SalvageIndex, salvage_all};
use crate::pdf::verify::{Baseline, baseline, classify_only};

/// Analysis's phases, in order.
const PHASES: [&str; 4] = ["carving", "diagnosing", "salvage", "measuring"];

fn phase(sink: &mut dyn Progress, index: usize) -> Result<(), Cancelled> {
    if sink.cancelled() {
        return Err(Cancelled);
    }
    let total = PHASES.len() as u32;
    sink.phase(PHASES[index], index as u32, total);
    Ok(())
}

// ── analysis ─────────────────────────────────────────────────────────────

pub(super) fn analyze(
    bytes: &[u8],
    opts: &AnalyzeOptions,
    sink: &mut dyn Progress,
) -> Result<AnalysisResult, Cancelled> {
    if bytes.len() as u64 > opts.max_file_bytes {
        sink.log(
            LogLevel::Warn,
            format!(
                "the file is {} bytes, over the {}-byte size warning; analysis may be slow",
                bytes.len(),
                opts.max_file_bytes
            ),
        );
    }
    let input_sha256: [u8; 32] = Sha256::digest(bytes).into();

    phase(sink, 0)?;
    let carve = {
        let poll: &dyn Progress = &*sink;
        carve(bytes, &|| poll.cancelled())?
    };
    let graph = ObjectGraph::from_carve(&carve);
    let source = CarveSource::new(&carve, bytes);

    phase(sink, 1)?;
    let probe = {
        let poll: &dyn Progress = &*sink;
        let budget = probe_budget(&opts.salvage_budget);
        salvage_under(&source, &budget, opts, &|| poll.cancelled())?
    };
    let probed = diagnose(bytes, &carve, &graph, &probe);
    let told = provisional(probed.clone());
    for f in &told {
        sink.finding(f);
    }
    let meta = file_meta(bytes, &carve, &graph);

    phase(sink, 2)?;
    let (salvage, findings) = if settled(&probe) {
        (probe, probed)
    } else {
        // Its kept bytes are not held beside the search's.
        drop(probe);
        let salvage = {
            let poll: &dyn Progress = &*sink;
            salvage_under(&source, &opts.salvage_budget, opts, &|| poll.cancelled())?
        };
        let findings = diagnose(bytes, &carve, &graph, &salvage);
        (salvage, findings)
    };
    let told: BTreeMap<&str, &Finding> = told.iter().map(|f| (f.id.as_str(), f)).collect();
    for f in &findings {
        if told.get(f.id.as_str()) != Some(&f) {
            sink.finding(f);
        }
    }

    phase(sink, 3)?;
    // Repair computes this baseline again inside `generate_and_validate`, a
    // second text extraction of the input; keeping it in `AnalysisState`
    // would save that (left for a later ticket, no effect on output).
    let base = baseline(bytes, &carve);
    let font_slots = font_slots(bytes, &carve, &graph, &salvage);
    let stats = AnalyzeStats {
        bytes: bytes.len() as u64,
        pages_discovered: meta.pages,
        salvage_work_total: salvage.work_total,
        streams_by_salvage: streams_by_salvage(&salvage),
        baseline_kind: base.kind(),
        paint_counts: match &base {
            Baseline::Extracted(pages) => pages.iter().map(|p| p.paint).collect(),
            Baseline::CarveProxy { .. } => Vec::new(),
        },
    };
    let summary = carve_summary(&carve, &findings);
    if sink.cancelled() {
        return Err(Cancelled);
    }
    let mut state = AnalysisState::new(carve, graph, salvage, input_sha256);
    state.salvage_budget = opts.salvage_budget;
    Ok(AnalysisResult {
        meta,
        findings,
        carve: summary,
        font_slots,
        stats,
        input_sha256,
        state: StateHandle::new(state),
    })
}

/// The cheap probe's budget (F-09): `budget` with no search work, so every
/// Flate stream is inflated once and classified and no candidate is tried
/// (D-074's zero budget). The size limit stays, so a stream over it is
/// `Unsearched` here as it is under `budget`.
fn probe_budget(budget: &SalvageBudget) -> SalvageBudget {
    SalvageBudget {
        work: 0,
        deep_work: 0,
        deep_pool: 0,
        ..*budget
    }
}

/// Every Flate stream of `source` salvaged under `budget`, on the threads
/// and within the scratch `opts` gives.
fn salvage_under(
    source: &CarveSource<'_>,
    budget: &SalvageBudget,
    opts: &AnalyzeOptions,
    cancel: &dyn Fn() -> bool,
) -> Result<SalvageIndex, Cancelled> {
    salvage_all(source, budget, opts.threads, opts.scratch_cap, cancel)
}

/// Whether the probe is already what the budgeted salvage gives: every
/// stream inflated clean or is over the size limit. Both are decided before
/// any search, whatever the work budget.
fn settled(probe: &SalvageIndex) -> bool {
    (probe.by_obj.values()).all(|e| {
        matches!(
            e.salvage,
            Salvage::Clean { .. } | Salvage::Unsearched { .. }
        )
    })
}

/// The stream count per salvage outcome.
fn streams_by_salvage(salvage: &SalvageIndex) -> BTreeMap<String, u32> {
    let mut out: BTreeMap<String, u32> = BTreeMap::new();
    for e in salvage.by_obj.values() {
        let name = match e.salvage {
            Salvage::Clean { .. } => "Clean",
            Salvage::ChecksumMismatch { .. } => "ChecksumMismatch",
            Salvage::Repaired { .. } => "Repaired",
            Salvage::Prefix { .. } => "Prefix",
            Salvage::Unrecoverable => "Unrecoverable",
            Salvage::Unsearched { .. } => "Unsearched",
        };
        *out.entry(name.to_owned()).or_default() += 1;
    }
    out
}

/// The carve in numbers: one object per id (the copy the rebuild keeps),
/// the copies that lost as shadows, the cross-reference kind of the last
/// table or stream in byte order.
fn carve_summary(carve: &CarveReport, findings: &[Finding]) -> CarveSummary {
    let winners = winning_copies(carve);
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let streams = winners
        .values()
        .filter(|&&at| matches!(carve.objects[at].body, Body::Stream { .. }))
        .count()
        + (carve.orphans.iter())
            .filter(|o| matches!(o, Orphan::Stream { .. }))
            .count();
    let classic = carve.xref_spans.last().map(|s| s.start);
    let stream = carve.xref_streams.last().map(|x| x.span.start);
    let xref_kind = match (classic, stream) {
        (None, None) => XrefKind::None,
        (Some(c), Some(s)) if s > c => XrefKind::Stream,
        (None, Some(_)) => XrefKind::Stream,
        _ => XrefKind::Classic,
    };
    let mut caps_hit: Vec<String> = (carve.notes.iter())
        .filter_map(|n| match n {
            CarveNote::CapHit(cap) => Some(format!("{cap:?}")),
            _ => None,
        })
        .collect();
    caps_hit.sort();
    caps_hit.dedup();
    CarveSummary {
        objects: count(winners.len()),
        orphans: count(carve.orphans.len()),
        streams: count(streams),
        shadows: count(carve.objects.len() - winners.len()),
        xref_kind,
        header_offset: carve.header.as_ref().map(|h| h.offset),
        truncated: findings
            .iter()
            .any(|f| f.class == FindingKind::Corruption(CorruptionClass::C10Truncated)),
        caps_hit,
    }
}

// ── plan ─────────────────────────────────────────────────────────────────

pub(super) fn plan(analysis: &AnalysisResult, opts: &RepairOptions) -> RepairPlan {
    // The v1 table reads the findings alone; the carve and graph are passed
    // when the state is at hand.
    match &analysis.state.0 {
        Some(s) => plan::plan(&analysis.findings, &s.carve, &s.graph, opts),
        None => plan::plan(
            &analysis.findings,
            &CarveReport::default(),
            &ObjectGraph::default(),
            opts,
        ),
    }
}

// ── repair ───────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn repair(
    bytes: &[u8],
    analysis: &AnalysisResult,
    plan: &RepairPlan,
    opts: &RepairOptions,
    fonts: &FontDb,
    ask: &mut dyn Interact,
    sink: &mut dyn Progress,
) -> Result<RepairOutcome, Cancelled> {
    if sink.cancelled() {
        return Err(Cancelled);
    }
    let input_sha256: [u8; 32] = Sha256::digest(bytes).into();
    let stale = match &analysis.state.0 {
        None => Some("the analysis carries no state"),
        Some(s) if s.input_sha256 != input_sha256 => Some("the analysis is of other bytes"),
        Some(s) if s.salvage_budget != opts.analyze.salvage_budget => {
            Some("the analysis ran under another salvage budget")
        }
        Some(_) => None,
    };
    let rebuilt;
    let (analysis, analysis_state) = match stale {
        None => (analysis, AnalysisStateUse::Reused),
        Some(reason) => {
            sink.log(LogLevel::Info, format!("analysing again: {reason}"));
            rebuilt = analyze(bytes, &opts.analyze, &mut Quiet(&mut *sink))?;
            let reason = reason.to_owned();
            (&rebuilt, AnalysisStateUse::Rebuilt { reason })
        }
    };
    let state: Arc<AnalysisState> = analysis
        .state
        .0
        .clone()
        .expect("analysis always keeps its state");

    let mut report = RepairReport::default_for(analysis, opts, fonts);
    let mut numberer = Numberer {
        inner: ask,
        asked: 0,
    };
    let input = Analysed {
        bytes,
        carve: &state.carve,
        graph: &state.graph,
        salvage: &state.salvage,
        findings: &analysis.findings,
        input_sha256,
    };
    let generated = generate_and_validate(&input, plan, opts, fonts, &mut numberer, sink)?;
    // The passes keep the one ordered list of questions and answers, policy
    // answers included (T-30); `numberer` only numbers what is asked.
    generated.record(&mut report);
    report.shadows = shadows(&state.carve);
    if (analysis.findings.iter()).any(|f| matches!(f.class, FindingKind::Signed { .. })) {
        report.signed_note = Some(SIGNED_NOTE.to_owned());
    }
    if let Some(out) = &generated.output {
        if sink.cancelled() {
            return Err(Cancelled);
        }
        report.findings_after = rediagnose(out);
    }
    for e in &generated.escalations {
        sink.log(LogLevel::Warn, e.note.clone());
    }

    let encrypted = (analysis.findings.iter()).any(|f| f.class == FindingKind::Encrypted);
    let status = match &generated.output {
        Some(_) if generated.partial_reasons.is_empty() => OutcomeStatus::Ok,
        Some(_) => OutcomeStatus::Partial(generated.partial_reasons.clone()),
        None if encrypted => {
            OutcomeStatus::Failed("the file is encrypted: decrypt it first".to_owned())
        }
        None if plan.candidates.is_empty() => {
            sink.log(LogLevel::Info, "nothing to repair".to_owned());
            OutcomeStatus::Ok
        }
        None if generated.candidates.is_empty() => {
            OutcomeStatus::Failed("no candidate output could be built".to_owned())
        }
        None => OutcomeStatus::Failed(
            "no candidate passed the hard gates (V0), so nothing was written".to_owned(),
        ),
    };
    Ok(RepairOutcome {
        output: generated.output,
        report,
        status,
        images: Vec::new(),
        analysis_state,
    })
}

/// The findings of `output`, carved and diagnosed with no C9 search (D-074).
fn rediagnose(output: &[u8]) -> Vec<Finding> {
    let Ok(carve) = carve(output, &|| false) else {
        return Vec::new();
    };
    let graph = ObjectGraph::from_carve(&carve);
    let salvage = classify_only(&carve, output);
    diagnose(output, &carve, &graph, &salvage)
}

/// Every carved copy of an object that is not the copy the rebuild keeps,
/// by id then position.
fn shadows(carve: &CarveReport) -> Vec<(ObjId, ByteSpan)> {
    let winners = winning_copies(carve);
    let mut out: Vec<(ObjId, ByteSpan)> = (carve.objects.iter().enumerate())
        .filter(|(at, o)| winners.get(&o.declared_id) != Some(at))
        .map(|(_, o)| (o.declared_id, o.span))
        .collect();
    out.sort();
    out
}

/// A sink that passes everything on but findings: a repair that analyses
/// again has already streamed them once.
struct Quiet<'a>(&'a mut dyn Progress);

impl Progress for Quiet<'_> {
    fn phase(&mut self, name: &'static str, index: u32, total: u32) {
        self.0.phase(name, index, total);
    }
    fn progress(&mut self, done: u64, total: Option<u64>) {
        self.0.progress(done, total);
    }
    fn finding(&mut self, _f: &Finding) {}
    fn log(&mut self, level: LogLevel, msg: String) {
        self.0.log(level, msg);
    }
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
}

/// Numbers every question from 1, in the order asked, and passes it on.
/// The asking pass records the question and its answer (T-30), beside the
/// answers a policy gives without asking, so the report keeps one ordered
/// list.
pub(super) struct Numberer<'a> {
    pub(super) inner: &'a mut dyn Interact,
    pub(super) asked: u64,
}

impl Interact for Numberer<'_> {
    fn ask(&mut self, mut req: InteractionRequest) -> Result<Answer, Cancelled> {
        self.asked += 1;
        let id = InteractionRequestId(self.asked);
        match &mut req {
            InteractionRequest::FontPick(r) => r.id = id,
            InteractionRequest::FontUnreproducible(r) => r.id = id,
        }
        self.inner.ask(req)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::engine::{
        FontCandidate, FontPickRequest, InteractionReply, InteractionRequestId, NullProgress,
        Ratio, SubstituteChoice, UnreproducibleRequest,
    };
    use crate::pdf::fixtures::{corrupt, golden_pdf};

    /// Answers from a script, in order, and keeps what it was asked.
    struct Scripted {
        replies: VecDeque<InteractionReply>,
        asked: Vec<InteractionRequest>,
    }

    impl Scripted {
        fn new(replies: impl IntoIterator<Item = InteractionReply>) -> Self {
            Scripted {
                replies: replies.into_iter().collect(),
                asked: Vec::new(),
            }
        }
    }

    impl Interact for Scripted {
        fn ask(&mut self, req: InteractionRequest) -> Result<Answer, Cancelled> {
            self.asked.push(req.clone());
            let reply = (self.replies.pop_front())
                .unwrap_or_else(|| panic!("an unscripted question: {req:?}"));
            Ok(Answer::user(reply))
        }
    }

    /// The findings and salvage index of the pipeline before F-09: the whole
    /// salvage search under the budget, then one diagnosis.
    fn salvage_first(bytes: &[u8], opts: &AnalyzeOptions) -> (Vec<Finding>, SalvageIndex) {
        let carve = carve(bytes, &|| false).expect("never cancelled");
        let graph = ObjectGraph::from_carve(&carve);
        let salvage = salvage_all(
            &CarveSource::new(&carve, bytes),
            &opts.salvage_budget,
            opts.threads,
            opts.scratch_cap,
            &|| false,
        )
        .expect("never cancelled");
        (diagnose(bytes, &carve, &graph, &salvage), salvage)
    }

    fn assert_as_salvage_first(bytes: &[u8], opts: &AnalyzeOptions, what: &str) {
        let (findings, salvage) = salvage_first(bytes, opts);
        let got = analyze(bytes, opts, &mut NullProgress).expect("never cancelled");
        assert_eq!(got.findings, findings, "{what}");
        let state = got.state.0.as_ref().expect("a state");
        assert_eq!(state.salvage, salvage, "{what}");
        assert_eq!(got.stats.salvage_work_total, salvage.work_total, "{what}");
    }

    #[test]
    fn the_analysis_is_what_salvaging_first_gave() {
        let golden = golden_pdf();
        let opts = AnalyzeOptions::default();
        assert_as_salvage_first(&golden, &opts, "golden");
        for class in CorruptionClass::ALL {
            let bytes = corrupt(class, &golden, 0);
            assert_as_salvage_first(&bytes, &opts, &format!("{class:?}"));
        }
        // A second C9 seed damages other streams.
        let bytes = corrupt(CorruptionClass::C9ZlibTampered, &golden, 1);
        assert_as_salvage_first(&bytes, &opts, "C9 seed 1");
        // Every damaged stream over the search limit: the probe is final.
        let mut small = AnalyzeOptions::default();
        small.salvage_budget.max_search_stream = 1;
        let bytes = corrupt(CorruptionClass::C9ZlibTampered, &golden, 0);
        assert_as_salvage_first(&bytes, &small, "C9 unsearched");
    }

    #[test]
    fn the_probe_charges_no_salvage_work() {
        let bytes = corrupt(CorruptionClass::C9ZlibTampered, &golden_pdf(), 0);
        let opts = AnalyzeOptions::default();
        let carve = carve(&bytes, &|| false).expect("never cancelled");
        let source = CarveSource::new(&carve, &bytes);
        let probe = salvage_under(&source, &probe_budget(&opts.salvage_budget), &opts, &|| {
            false
        })
        .expect("never cancelled");
        let damaged = (probe.by_obj.values())
            .filter(|e| !matches!(e.salvage, Salvage::Clean { .. }))
            .count();
        assert!(damaged > 0);
        assert_eq!(probe.work_total, 0);
        assert!(!settled(&probe));
    }

    #[test]
    fn every_question_is_numbered_and_passed_on() {
        let pick = FontPickRequest {
            id: InteractionRequestId(0),
            page: 2,
            slot: "F3".into(),
            sample_codes: vec![1, 2],
            candidates: vec![FontCandidate {
                font_id: "noto-sans".into(),
                family: "Noto Sans".into(),
                language: "en".into(),
                score: Ratio { num: 1, den: 2 },
                confidence: Ratio { num: 1, den: 3 },
                preview: String::new(),
            }],
            preview: String::new(),
        };
        let unrepro = UnreproducibleRequest {
            id: InteractionRequestId(0),
            family: "Garamond".into(),
            slots: vec![(4, "F7".into()), (5, "F7".into())],
            reason: "no font covers it".into(),
            options: vec![SubstituteChoice {
                font_id: "noto-serif".into(),
                label: "Noto Serif".into(),
            }],
        };
        let mut script = Scripted::new([
            InteractionReply::Pick("noto-sans".into()),
            InteractionReply::UseBest,
        ]);
        let mut numberer = Numberer {
            inner: &mut script,
            asked: 0,
        };
        let a = numberer.ask(InteractionRequest::FontPick(pick)).unwrap();
        let b = (numberer.ask(InteractionRequest::FontUnreproducible(unrepro))).unwrap();
        assert_eq!(a, Answer::user(InteractionReply::Pick("noto-sans".into())));
        assert_eq!(b, Answer::user(InteractionReply::UseBest));
        assert_eq!(numberer.asked, 2);
        let ids: Vec<InteractionRequestId> = (script.asked.iter())
            .map(|r| match r {
                InteractionRequest::FontPick(p) => p.id,
                InteractionRequest::FontUnreproducible(u) => u.id,
            })
            .collect();
        assert_eq!(ids, [InteractionRequestId(1), InteractionRequestId(2)]);
    }
}
