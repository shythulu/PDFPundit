//! End-to-end tests through the facade on fixtures (T-14). Uses
//! `crate::engine::*` only, with two exceptions: the inputs come from
//! `crate::pdf::fixtures`, and lopdf's strict load checks the outputs.
//!
//! - every class with a pass in this version repairs end to end under
//!   `UseBest`, and the output reloads strictly with that class gone;
//! - cancellation stops analysis inside the carve;
//! - a signed input's report carries the signature line;
//! - the facade reads the input and returns bytes, and writes no file;
//! - two runs give the same bytes and the same report;
//! - repair after analysis reuses its state (no second salvage), and
//!   rebuilds it for other bytes or another budget;
//! - a report from a result reloaded from history equals the one from a
//!   fresh analysis (D-073);
//! - the recorded settings and replies replay to the same output and report
//!   (goal-r2-q12);
//! - analysis fills the file's summary, statistics and font slots.

use std::cell::Cell;
use std::collections::VecDeque;
use std::num::NonZeroUsize;

use lopdf::{Document, LoadOptions};

use super::*;
use crate::pdf::fixtures::{GOLDEN_TEXT, corrupt, golden_pdf, golden_pdf_signed};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped,
    C6FontMapLost, C7FontStreamDeleted, C9ZlibTampered, C10Truncated,
};

// ── helpers ──────────────────────────────────────────────────────────────

/// A sink that keeps the phases it is told, counts findings and polls, and
/// cancels from poll `cancel_after + 1` on when that is set.
#[derive(Default)]
struct Counter {
    phases: Vec<&'static str>,
    findings: usize,
    logs: Vec<(LogLevel, String)>,
    polls: Cell<u32>,
    cancel_after: Option<u32>,
}

impl Counter {
    fn salvages(&self) -> usize {
        self.phases.iter().filter(|&&p| p == "salvage").count()
    }
}

impl Progress for Counter {
    fn phase(&mut self, name: &'static str, _index: u32, _total: u32) {
        self.phases.push(name);
    }
    fn progress(&mut self, _done: u64, _total: Option<u64>) {}
    fn finding(&mut self, _f: &Finding) {
        self.findings += 1;
    }
    fn log(&mut self, level: LogLevel, msg: String) {
        self.logs.push((level, msg));
    }
    fn cancelled(&self) -> bool {
        let n = self.polls.get() + 1;
        self.polls.set(n);
        self.cancel_after.is_some_and(|k| n > k)
    }
}

/// Answers from a script, in order; a question past its end is a failure.
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
    fn ask(&mut self, req: InteractionRequest) -> Result<InteractionReply, Cancelled> {
        self.asked.push(req.clone());
        Ok(self
            .replies
            .pop_front()
            .unwrap_or_else(|| panic!("an unscripted question: {req:?}")))
    }
}

fn analysed(bytes: &[u8]) -> AnalysisResult {
    analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).expect("never cancelled")
}

fn repaired_with(
    bytes: &[u8],
    analysis: &AnalysisResult,
    opts: &RepairOptions,
    ask: &mut dyn Interact,
    sink: &mut dyn Progress,
) -> RepairOutcome {
    let p = plan(analysis, opts);
    repair(bytes, analysis, &p, opts, &FontDb::empty(), ask, sink).expect("never cancelled")
}

fn repaired(bytes: &[u8], analysis: &AnalysisResult) -> RepairOutcome {
    repaired_with(
        bytes,
        analysis,
        &RepairOptions::default(),
        &mut UseBest,
        &mut NullProgress,
    )
}

fn json<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("serialises")
}

fn classes(findings: &[Finding]) -> Vec<CorruptionClass> {
    findings
        .iter()
        .filter_map(|f| match f.class {
            FindingKind::Corruption(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn strict_load(out: &[u8]) -> Result<Document, lopdf::Error> {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(256 << 20),
        ..Default::default()
    };
    Document::load_mem_with_options(out, opts)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The golden with both pages' `/Resources /Font << … >>` blanked to spaces:
/// its Type0 font is then reachable from nothing, the re-link candidate of
/// both pages' `/F1` (C6 with an orphan to re-link).
pub(super) fn golden_c6_with_orphan() -> Vec<u8> {
    let mut buf = golden_pdf();
    for page in [3, 4] {
        let start = find(&buf, format!("\n{page} 0 obj").as_bytes()).expect("the page") + 1;
        let end = start + find(&buf[start..], b"endobj").expect("endobj");
        let key = start + find(&buf[start..end], b"/Font").expect("/Font");
        let open = key + find(&buf[key..], b"<<").expect("<<");
        let close = open + find(&buf[open..], b">>").expect(">>") + 2;
        buf[key..close].fill(b' ');
    }
    buf
}

/// One input per class with a pass in this version: the T-13a/T-13b seeds
/// that leave every page recoverable.
pub(super) fn class_fixtures() -> Vec<(CorruptionClass, Vec<u8>)> {
    let golden = golden_pdf();
    let mut out: Vec<(CorruptionClass, Vec<u8>)> = [
        C1Header,
        C2XrefMissing,
        C3TrailerDamaged,
        C4PageTreeBroken,
        C5ObjectTagStripped,
        C9ZlibTampered,
        C10Truncated,
    ]
    .into_iter()
    .map(|c| (c, corrupt(c, &golden, 0)))
    .collect();
    out.push((C6FontMapLost, golden_c6_with_orphan()));
    out
}

// ── end to end ───────────────────────────────────────────────────────────

#[test]
fn each_class_with_a_pass_repairs_end_to_end_and_reloads_strictly() {
    for (class, bytes) in class_fixtures() {
        let analysis = analysed(&bytes);
        assert!(
            classes(&analysis.findings).contains(&class),
            "{class:?}: {:#?}",
            analysis.findings
        );
        let out = repaired(&bytes, &analysis);
        assert!(
            matches!(out.status, OutcomeStatus::Ok | OutcomeStatus::Partial(_)),
            "{class:?}: {:?}",
            out.status
        );
        let output = out.output.as_deref().expect("an output");
        strict_load(output).unwrap_or_else(|e| panic!("{class:?}: strict reload: {e}"));
        let again = analysed(output);
        assert!(
            !classes(&again.findings).contains(&class),
            "{class:?}: {:#?}",
            again.findings
        );
        assert!(
            !classes(&out.report.findings_after).contains(&class),
            "{class:?}: {:#?}",
            out.report.findings_after
        );
        assert_eq!(out.report.findings_before, analysis.findings, "{class:?}");
        assert_eq!(out.report.chosen, Some(Toolpath::Resave), "{class:?}");
        let pass = (out.report.passes.iter())
            .find(|p| p.class == class)
            .unwrap_or_else(|| panic!("{class:?}: no pass in {:?}", out.report.passes));
        assert!(
            !matches!(pass.outcome, PassOutcome::Skipped(_)),
            "{class:?}: {pass:?}"
        );
        match (&out.status, class) {
            (OutcomeStatus::Partial(reasons), _) => {
                assert_eq!(reasons, &out.report.partial_reasons)
            }
            (OutcomeStatus::Ok, _) => assert!(out.report.partial_reasons.is_empty()),
            (s, _) => panic!("{s:?}"),
        }
    }
}

#[test]
fn a_near_miss_header_outside_every_object_is_c9_and_gone_after_repair() {
    // `3 0 obk`: the carve finds no object 3, and its rule 7 reads the
    // keyword one byte off in the gap (F-07).
    let mut bytes = golden_pdf();
    let at = find(&bytes, b"3 0 obj").expect("page 1") + 6;
    bytes[at] = b'k';
    let analysis = analysed(&bytes);
    let c9: Vec<&Finding> = (analysis.findings.iter())
        .filter(|f| f.class == FindingKind::Corruption(C9ZlibTampered))
        .collect();
    assert_eq!(c9.len(), 1, "{:#?}", analysis.findings);
    assert_eq!(c9[0].severity, Severity::Warning);
    assert!(matches!(c9[0].location, Location::Span(_)), "{:?}", c9[0]);
    let out = repaired(&bytes, &analysis);
    assert!(
        matches!(out.status, OutcomeStatus::Ok | OutcomeStatus::Partial(_)),
        "{:?}",
        out.status
    );
    let output = out.output.as_deref().expect("an output");
    strict_load(output).expect("strict reload");
    assert!(!classes(&analysed(output).findings).contains(&C9ZlibTampered));
    let pass = (out.report.passes.iter())
        .find(|p| p.class == C9ZlibTampered)
        .expect("a C9 pass");
    // No object to act on: the pass names the span instead of claiming a fix.
    let PassOutcome::Partial(why) = &pass.outcome else {
        panic!("{pass:?}");
    };
    let Location::Span(span) = c9[0].location else {
        unreachable!()
    };
    assert!(
        why.contains(&format!("bytes {}..{}", span.start, span.end))
            && why.contains("not re-emitted"),
        "{why}"
    );
    assert!(pass.actions.is_empty(), "{pass:?}");
}

#[test]
fn a_class_left_out_of_passes_is_skipped_and_said_so() {
    // Every class has a pass since T-30, so C7 runs by default...
    let bytes = corrupt(C1Header, &corrupt(C7FontStreamDeleted, &golden_pdf(), 0), 0);
    let analysis = analysed(&bytes);
    assert_eq!(classes(&analysis.findings), [C1Header, C7FontStreamDeleted]);
    let c7 = |out: &RepairOutcome| {
        (out.report.passes.iter())
            .find(|p| p.class == C7FontStreamDeleted)
            .expect("a C7 report")
            .clone()
    };
    let out = repaired(&bytes, &analysis);
    let pass = c7(&out);
    assert!(!matches!(pass.outcome, PassOutcome::Skipped(_)), "{pass:?}");
    // ...and is skipped, with a log line, when the options leave it out.
    let opts = RepairOptions {
        passes: Some(vec![C1Header]),
        ..RepairOptions::default()
    };
    let mut sink = Counter::default();
    let out = repaired_with(&bytes, &analysis, &opts, &mut UseBest, &mut sink);
    let pass = c7(&out);
    assert!(matches!(pass.outcome, PassOutcome::Skipped(_)), "{pass:?}");
    assert!(
        (sink.logs.iter()).any(|(_, m)| m.starts_with("C7") && m.ends_with("skipped")),
        "{:?}",
        sink.logs
    );
}

#[test]
fn cancellation_stops_analysis_inside_the_carve() {
    // Twenty thousand objects: the carve polls once per 256 landmarks, so
    // it polls well over ten times before it ends.
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for n in 1..=20_000 {
        bytes.extend_from_slice(format!("{n} 0 obj\n<< /N {n} >>\nendobj\n").as_bytes());
    }
    // Poll 1 is the "carving" phase; polls 2 to 11 are the carve's own. A
    // carve that never polled would run to the end, and the "salvage"
    // phase and its polls would follow before the eleventh poll.
    let mut sink = Counter {
        cancel_after: Some(10),
        ..Counter::default()
    };
    let r = analyze(&bytes, &AnalyzeOptions::default(), &mut sink);
    assert_eq!(r, Err(Cancelled));
    assert_eq!(sink.phases, ["carving"]);
    assert_eq!(sink.polls.get(), 11);
    assert_eq!(sink.findings, 0);

    // Cancelled before it starts, repair builds nothing.
    let analysis = analysed(&golden_pdf());
    let p = plan(&analysis, &RepairOptions::default());
    let mut sink = Counter {
        cancel_after: Some(0),
        ..Counter::default()
    };
    let r = repair(
        &golden_pdf(),
        &analysis,
        &p,
        &RepairOptions::default(),
        &FontDb::empty(),
        &mut UseBest,
        &mut sink,
    );
    assert_eq!(r, Err(Cancelled));
    assert!(sink.phases.is_empty());
}

#[test]
fn a_signed_input_s_report_carries_the_signature_line() {
    let bytes = golden_pdf_signed();
    let analysis = analysed(&bytes);
    assert!(
        (analysis.findings.iter()).any(|f| matches!(f.class, FindingKind::Signed { .. })),
        "{:#?}",
        analysis.findings
    );
    let out = repaired(&bytes, &analysis);
    assert_eq!(out.report.signed_note.as_deref(), Some(SIGNED_NOTE));
    assert!(out.report.lines().iter().any(|l| l == SIGNED_NOTE));

    // An unsigned input has no such line.
    let golden = golden_pdf();
    let out = repaired(&golden, &analysed(&golden));
    assert_eq!(out.report.signed_note, None);
}

#[test]
fn the_facade_reads_bytes_returns_bytes_and_writes_no_file() {
    let bytes = corrupt(C2XrefMissing, &golden_pdf(), 0);
    let before = bytes.clone();
    let out = repaired(&bytes, &analysed(&bytes));
    assert_eq!(bytes, before, "the input is untouched");
    assert_ne!(out.output.as_deref(), Some(&bytes[..]));
    // File I/O lives in `jobs.rs` only: the facade's sources name none.
    for (name, src) in [
        ("engine.rs", include_str!("../engine.rs")),
        ("pipeline.rs", include_str!("pipeline.rs")),
        ("slots.rs", include_str!("slots.rs")),
        ("report.rs", include_str!("report.rs")),
    ] {
        for io in ["std::fs", "fs::", "File::", "OpenOptions", "std::io"] {
            assert!(!src.contains(io), "{name} uses {io}");
        }
    }
}

#[test]
fn two_runs_give_the_same_output_and_report() {
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 0);
    let a = repaired(&bytes, &analysed(&bytes));
    let b = repaired(&bytes, &analysed(&bytes));
    assert!(a.output.is_some());
    assert_eq!(a.output, b.output);
    assert_eq!(json(&a.report), json(&b.report));
    assert_eq!(a.report.lines(), b.report.lines());
}

// ── state reuse ──────────────────────────────────────────────────────────

#[test]
fn repair_after_analysis_reuses_its_state_and_never_salvages_again() {
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 0);
    let mut sink = Counter::default();
    let analysis = analyze(&bytes, &AnalyzeOptions::default(), &mut sink).unwrap();
    assert_eq!(sink.salvages(), 1);
    assert_eq!(sink.findings, analysis.findings.len());
    let state = analysis.state.0.as_ref().expect("a state");
    assert_eq!(state.input_sha256, analysis.input_sha256);

    let mut sink = Counter::default();
    let out = repaired_with(
        &bytes,
        &analysis,
        &RepairOptions::default(),
        &mut UseBest,
        &mut sink,
    );
    assert_eq!(sink.salvages(), 0, "{:?}", sink.phases);
    assert_eq!(out.analysis_state, AnalysisStateUse::Reused);
    assert!(out.output.is_some());
}

#[test]
fn other_bytes_or_another_budget_rebuild_the_state() {
    let golden = golden_pdf();
    let bytes = corrupt(C9ZlibTampered, &golden, 0);
    let analysis = analysed(&bytes);

    // The analysis is of the C9 file; repair is handed the C2 file.
    let other = corrupt(C2XrefMissing, &golden, 0);
    let mut sink = Counter::default();
    let out = repaired_with(
        &other,
        &analysis,
        &RepairOptions::default(),
        &mut UseBest,
        &mut sink,
    );
    assert_eq!(sink.salvages(), 1);
    assert_eq!(sink.findings, 0, "a rebuild streams no findings again");
    assert!(
        matches!(&out.analysis_state, AnalysisStateUse::Rebuilt { reason } if reason.contains("other bytes")),
        "{:?}",
        out.analysis_state
    );
    // The report is of the bytes repaired, not of the analysis handed in.
    assert_eq!(out.report.input_sha256, analysed(&other).input_sha256);
    assert!(classes(&out.report.findings_before).contains(&C2XrefMissing));
    assert!(out.output.is_some());

    // The same bytes under another salvage budget.
    let mut opts = RepairOptions::default();
    opts.analyze.salvage_budget.work = 10_000_000;
    let mut sink = Counter::default();
    let out = repaired_with(&bytes, &analysis, &opts, &mut UseBest, &mut sink);
    assert_eq!(sink.salvages(), 1);
    assert!(
        matches!(&out.analysis_state, AnalysisStateUse::Rebuilt { reason } if reason.contains("budget")),
        "{:?}",
        out.analysis_state
    );
    assert_eq!(out.report.settings.salvage_work, 10_000_000);
}

// ── report purity and replay ─────────────────────────────────────────────

#[test]
fn a_report_from_history_equals_the_report_from_a_fresh_analysis() {
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 0);
    let analysis = analysed(&bytes);
    let fresh = repaired(&bytes, &analysis);

    let history: AnalysisResult = serde_json::from_slice(&json(&analysis)).expect("deserialises");
    assert_eq!(history.state, StateHandle::default());
    let mut sink = Counter::default();
    let reloaded = repaired_with(
        &bytes,
        &history,
        &RepairOptions::default(),
        &mut UseBest,
        &mut sink,
    );

    assert_eq!(fresh.analysis_state, AnalysisStateUse::Reused);
    assert!(
        matches!(reloaded.analysis_state, AnalysisStateUse::Rebuilt { .. }),
        "{:?}",
        reloaded.analysis_state
    );
    assert_eq!(sink.salvages(), 1);
    assert_eq!(json(&fresh.report), json(&reloaded.report));
    assert_eq!(fresh.output, reloaded.output);
}

/// The options a report's settings name, with other threads and scratch.
fn options_from(s: &SettingsSnapshot) -> RepairOptions {
    RepairOptions {
        passes: s.passes.clone(),
        font_policy: s.font_policy,
        unreproducible: s.unreproducible,
        default_page_size: s.default_page_size,
        extract_unplaceable_images: s.extract_unplaceable_images,
        auto_accept_confidence: s.auto_accept_confidence,
        max_font_candidates: s.max_font_candidates,
        analyze: AnalyzeOptions {
            salvage_budget: SalvageBudget {
                work: s.salvage_work,
                deep_work: s.salvage_deep_work,
                deep_pool: s.salvage_deep_pool,
                max_search_stream: s.max_search_stream,
            },
            threads: NonZeroUsize::new(3).expect("non-zero"),
            scratch_cap: 64 << 20,
            ..AnalyzeOptions::default()
        },
    }
}

#[test]
fn the_recorded_settings_and_replies_replay_to_the_same_output_and_report() {
    let bytes = corrupt(C9ZlibTampered, &golden_pdf(), 0);
    let opts = RepairOptions {
        passes: Some(vec![C9ZlibTampered, C10Truncated]),
        default_page_size: PageSize::Letter,
        max_font_candidates: 3,
        analyze: AnalyzeOptions {
            salvage_budget: SalvageBudget {
                work: 50_000_000,
                ..SalvageBudget::default()
            },
            ..AnalyzeOptions::default()
        },
        ..RepairOptions::default()
    };
    let analysis = analyze(&bytes, &opts.analyze, &mut NullProgress).unwrap();
    let first = repaired_with(&bytes, &analysis, &opts, &mut UseBest, &mut NullProgress);
    assert_eq!(first.report.settings, SettingsSnapshot::of(&opts));
    // This fixture has no damaged font, so nothing is asked; the font
    // questions' replay is tested in `pdf::repair::tests::fonts`.
    assert!(first.report.interactions.is_empty());

    let replay = options_from(&first.report.settings);
    let mut script = Scripted::new(
        (first.report.interactions.iter())
            .filter(|r| r.source != InteractionSource::Policy)
            .map(|r| r.reply.clone()),
    );
    let again = analyze(&bytes, &replay.analyze, &mut NullProgress).unwrap();
    let second = repaired_with(&bytes, &again, &replay, &mut script, &mut NullProgress);
    assert!(
        script.replies.is_empty(),
        "every recorded reply was asked for"
    );
    assert_eq!(first.output, second.output);
    assert_eq!(json(&first.report), json(&second.report));

    // The C9 golden's report is JSON with its survivors in it (eng-r3-q1).
    assert!(!first.report.c9_survivors.is_empty());
    assert!(first.report.c9_summary.streams_damaged > 0);
    let text = serde_json::to_string(&first.report).expect("serialises");
    let back: RepairReport = serde_json::from_str(&text).expect("deserialises");
    assert_eq!(back, first.report);
}

// ── analysis ─────────────────────────────────────────────────────────────

#[test]
fn the_golden_s_analysis_is_clean_and_filled_in() {
    let bytes = golden_pdf();
    let mut sink = Counter::default();
    let a = analyze(&bytes, &AnalyzeOptions::default(), &mut sink).unwrap();
    assert_eq!(
        sink.phases,
        ["carving", "salvage", "diagnosing", "measuring"]
    );
    assert!(classes(&a.findings).is_empty(), "{:#?}", a.findings);
    assert_eq!(a.meta.version.as_deref(), Some("1.7"));
    assert_eq!(a.meta.pages, 2);
    assert_eq!(a.carve.xref_kind, XrefKind::Classic);
    assert_eq!(a.carve.header_offset, Some(0));
    assert_eq!(a.carve.orphans, 0);
    assert_eq!(a.carve.shadows, 0);
    assert!(!a.carve.truncated);
    assert!(a.carve.caps_hit.is_empty());
    assert!(a.carve.objects > 0 && a.carve.streams > 0);
    assert_eq!(a.stats.bytes, bytes.len() as u64);
    assert_eq!(a.stats.pages_discovered, 2);
    assert_eq!(a.stats.salvage_work_total, 0);
    assert_eq!(a.stats.baseline_kind, BaselineKind::Extracted);
    assert_eq!(a.stats.paint_counts.len(), 2);
    let flate: u32 = a.stats.streams_by_salvage.values().sum();
    assert_eq!(a.stats.streams_by_salvage.get("Clean"), Some(&flate));

    // Nothing to repair: no output and no failure.
    let out = repaired(&bytes, &a);
    assert_eq!(out.output, None);
    assert_eq!(out.status, OutcomeStatus::Ok);
}

#[test]
fn the_golden_s_font_slots_are_its_one_embedded_font_per_page() {
    let a = analysed(&golden_pdf());
    assert_eq!(a.font_slots.len(), 2, "{:#?}", a.font_slots);
    for (page, slot) in a.font_slots.iter().enumerate() {
        let glyphs: usize = GOLDEN_TEXT[page].iter().map(|l| l.chars().count()).sum();
        assert_eq!(slot.page, page as u32);
        assert_eq!(slot.slot, "F1");
        assert_eq!(slot.subtype.as_deref(), Some("/Type0"));
        assert!(slot.base_font.is_some());
        assert!(slot.embedded);
        assert_eq!(slot.tounicode, ToUnicodeState::Present);
        assert_eq!(slot.glyph_count as usize, glyphs);
        assert_eq!(slot.resolution, None);
    }
}

#[test]
fn a_lost_font_map_or_program_shows_in_the_font_slots() {
    // C6: the slot maps to nothing.
    let a = analysed(&golden_c6_with_orphan());
    assert_eq!(a.font_slots.len(), 2);
    for slot in &a.font_slots {
        assert_eq!(slot.slot, "F1");
        assert_eq!(
            (slot.base_font.as_deref(), slot.subtype.as_deref()),
            (None, None)
        );
        assert!(!slot.embedded);
        assert_eq!(slot.tounicode, ToUnicodeState::Missing);
        assert!(slot.glyph_count > 0);
    }
    // C7: the program is blank, the /ToUnicode survives.
    let a = analysed(&corrupt(C7FontStreamDeleted, &golden_pdf(), 0));
    assert!(a.font_slots.iter().all(|s| !s.embedded));
    assert!(
        (a.font_slots.iter()).all(|s| s.tounicode == ToUnicodeState::Present),
        "{:#?}",
        a.font_slots
    );
    // C8: both are blank.
    let a = analysed(&corrupt(
        CorruptionClass::C8FontResourcesDeleted,
        &golden_pdf(),
        0,
    ));
    assert!(
        (a.font_slots.iter()).all(|s| !s.embedded && s.tounicode != ToUnicodeState::Present),
        "{:#?}",
        a.font_slots
    );
}

#[test]
fn a_file_over_the_size_warning_is_analysed_with_a_warning() {
    let bytes = golden_pdf();
    let opts = AnalyzeOptions {
        max_file_bytes: 100,
        ..AnalyzeOptions::default()
    };
    let mut sink = Counter::default();
    let a = analyze(&bytes, &opts, &mut sink).unwrap();
    assert_eq!(a.meta.pages, 2);
    assert!(
        (sink.logs.iter()).any(|(l, m)| *l == LogLevel::Warn && m.contains("bytes")),
        "{:?}",
        sink.logs
    );
}

#[test]
fn an_encrypted_file_fails_with_a_reason_and_no_output() {
    // `/Encrypt` in the trailer: diagnose calls the file encrypted.
    let golden = golden_pdf();
    let at = find(&golden, b"trailer").expect("a trailer");
    let open = at + find(&golden[at..], b"<<").expect("<<") + 2;
    let mut bytes = golden[..open].to_vec();
    bytes.extend_from_slice(b" /Encrypt 99 0 R");
    bytes.extend_from_slice(&golden[open..]);
    let a = analysed(&bytes);
    assert!(a.findings.iter().any(|f| f.class == FindingKind::Encrypted));
    let out = repaired(&bytes, &a);
    assert_eq!(out.output, None);
    assert!(
        matches!(&out.status, OutcomeStatus::Failed(why) if why.contains("encrypted")),
        "{:?}",
        out.status
    );
}
