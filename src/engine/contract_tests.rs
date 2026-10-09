//! The facade and report contract (T-02b): the engine's trait, `UseBest`, the
//! committed JSON snapshots, the populated round-trip and `StateHandle` equality.
//! Uses `crate::engine::*` only, plus `crate::jobs` for the run-record fields.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::engine::*;
use crate::jobs::Placed;

// ── the engine ──────────────────────────────────────────────────────────

#[test]
fn the_engine_implements_engine() {
    fn engine<E: Engine>(_: &E) {}
    engine(&Pdfpundit);
    let _: &dyn Engine = &Pdfpundit;
}

#[test]
fn analyze_reads_the_header_and_hashes_the_input() {
    let bytes = b"junk%PDF-1.7\n1 0 obj\n<<>>\nendobj\n";
    let r = analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).expect("not cancelled");
    assert_eq!(r.meta.version.as_deref(), Some("1.7"));
    assert_eq!(r.meta.pages, 0);
    assert_eq!(r.input_sha256, sha256_of(bytes));
    assert_eq!(r.stats.bytes, bytes.len() as u64);
    assert_ne!(r.state, StateHandle::default(), "analysis keeps its state");

    let none = analyze(
        b"no header here",
        &AnalyzeOptions::default(),
        &mut NullProgress,
    )
    .expect("not cancelled");
    assert_eq!(none.meta.version, None);
}

#[test]
fn analyze_honours_cancellation() {
    struct Cancelling;
    impl Progress for Cancelling {
        fn phase(&mut self, _: &'static str, _: u32, _: u32) {}
        fn progress(&mut self, _: u64, _: Option<u64>) {}
        fn finding(&mut self, _: &Finding) {}
        fn log(&mut self, _: LogLevel, _: String) {}
        fn cancelled(&self) -> bool {
            true
        }
    }
    let r = analyze(b"%PDF-1.4", &AnalyzeOptions::default(), &mut Cancelling);
    assert_eq!(r, Err(Cancelled));
}

#[test]
fn a_file_with_no_page_plans_resave_and_gets_no_output() {
    let bytes = b"%PDF-1.4\n";
    let analysis = analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).unwrap();
    let opts = RepairOptions::default();
    let p = plan(&analysis, &opts);
    assert_eq!(p.prior, Toolpath::Resave);
    assert!(p.escalations.is_empty());
    assert_eq!(p.candidates, [Toolpath::Resave, Toolpath::TemplateAssemble]);

    let fonts = FontDb::empty();
    let out = repair(
        bytes,
        &analysis,
        &p,
        &opts,
        &fonts,
        &mut UseBest,
        &mut NullProgress,
    )
    .expect("not cancelled");
    assert_eq!(out.output, None);
    assert!(
        matches!(&out.status, OutcomeStatus::Failed(why) if why.contains("(V0)")),
        "{:?}",
        out.status
    );
    assert!(out.images.is_empty());
    assert_eq!(out.analysis_state, AnalysisStateUse::Reused);
    assert_eq!(out.report.input_sha256, analysis.input_sha256);
    assert_eq!(out.report.findings_before, analysis.findings);
}

#[test]
fn use_best_answers_every_interaction_request() {
    for req in [
        InteractionRequest::FontPick(font_pick_request()),
        InteractionRequest::FontUnreproducible(unreproducible_request()),
    ] {
        assert_eq!(UseBest.ask(req), Ok(InteractionReply::UseBest));
    }
}

#[test]
fn null_progress_is_never_cancelled() {
    let mut p = NullProgress;
    p.phase("carving", 1, 6);
    p.progress(1, None);
    p.log(LogLevel::Info, "x".into());
    assert!(!p.cancelled());
}

#[test]
fn schema_version_is_1() {
    assert_eq!(RepairReport::SCHEMA_VERSION, 1);
    let r = RepairReport::default_for(
        &analysis_fixture(),
        &RepairOptions::default(),
        &FontDb::empty(),
    );
    assert_eq!(r.schema_version, 1);
}

#[test]
fn default_for_carries_the_inputs_it_is_given() {
    let a = analysis_fixture();
    let opts = RepairOptions::default();
    let fonts = FontDb::empty();
    let r = RepairReport::default_for(&a, &opts, &fonts);
    assert_eq!(r.engine_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(r.font_db_sha256, fonts.sha256());
    assert_eq!(r.input_sha256, a.input_sha256);
    assert_eq!(r.settings, SettingsSnapshot::of(&opts));
    assert_eq!(r.findings_before, a.findings);
    assert_eq!(r.stats, a.stats);
    assert!(r.findings_after.is_empty() && r.passes.is_empty() && r.candidates.is_empty());
    assert_eq!(r.chosen, None);
}

#[test]
fn defaults_match_the_plan() {
    let a = AnalyzeOptions::default();
    assert_eq!(a.salvage_budget.work, 700_000_000);
    assert_eq!(a.salvage_budget.deep_work, 10_000_000_000);
    assert_eq!(a.salvage_budget.deep_pool, 80_000_000_000);
    assert_eq!(a.salvage_budget.max_search_stream, 4 << 20);
    assert_eq!(a.max_file_bytes, 512 << 20);
    assert_eq!(a.scratch_cap, 1 << 30);

    let s = SettingsSnapshot::of(&RepairOptions::default());
    assert_eq!(s.salvage_work, 700_000_000);
    assert_eq!(s.auto_accept_confidence, Ratio { num: 35, den: 100 });
    assert_eq!(s.max_font_candidates, 5);
    assert_eq!(s.passes, None);
    assert_eq!(s.default_page_size, PageSize::A4);
    assert!(s.extract_unplaceable_images);
}

#[test]
fn threads_and_scratch_cap_never_reach_the_settings_snapshot() {
    let base = RepairOptions::default();
    let mut other = base.clone();
    other.analyze.threads = std::num::NonZeroUsize::new(8).unwrap();
    other.analyze.scratch_cap = 7;
    other.analyze.max_file_bytes = 9;
    assert_eq!(SettingsSnapshot::of(&base), SettingsSnapshot::of(&other));
    let json = serde_json::to_string(&SettingsSnapshot::of(&base)).unwrap();
    assert!(!json.contains("threads") && !json.contains("scratch"));
}

#[test]
fn report_lines_state_the_fixed_lines() {
    let mut r = RepairReport::default_for(
        &analysis_fixture(),
        &RepairOptions::default(),
        &FontDb::empty(),
    );
    assert!(r.lines().is_empty());
    r.signed_note = Some(SIGNED_NOTE.to_string());
    r.c9_summary = C9Summary {
        streams_damaged: 9,
        repaired: 7,
        exact: 5,
        accepted: 2,
        ambiguous: 1,
        unrecoverable: 1,
        unsearched: 0,
    };
    r.reals_narrowed = 3;
    assert_eq!(
        r.lines(),
        vec![
            "the original is digitally signed; the repaired file is not".to_string(),
            "7 streams repaired; 5 of them unique within the searched window; \
             2 accepted without a uniqueness check; 1 ambiguous; 1 unrecoverable, written as found"
                .to_string(),
            "3 numbers were narrowed to single precision on re-emission".to_string(),
        ]
    );
}

// ── StateHandle ──────────────────────────────────────────────────────────

#[test]
fn state_handle_equality_is_identity() {
    let a = Arc::new(AnalysisState::new(
        Default::default(),
        Default::default(),
        Default::default(),
        [7; 32],
    ));
    let one = StateHandle(Some(a.clone()));
    let same = StateHandle(Some(a));
    assert_eq!(one, same, "two handles to one Arc are equal");

    let twin = StateHandle::new(AnalysisState::new(
        Default::default(),
        Default::default(),
        Default::default(),
        [7; 32],
    ));
    assert_ne!(one, twin, "identical contents in another Arc are not equal");
    assert_eq!(StateHandle(None), StateHandle::default(), "None == None");
    assert_ne!(one, StateHandle(None));
}

#[test]
fn heap_bytes_of_the_stub_state_is_zero() {
    let s = AnalysisState::new(
        Default::default(),
        Default::default(),
        Default::default(),
        [0; 32],
    );
    assert_eq!(s.heap_bytes(), 0);
}

#[test]
fn analysis_result_round_trips_with_the_state_dropped() {
    let mut a = analysis_fixture();
    a.state = StateHandle::new(AnalysisState::new(
        Default::default(),
        Default::default(),
        Default::default(),
        a.input_sha256,
    ));
    let json = serde_json::to_string(&a).unwrap();
    assert!(!json.contains("\"state\""));
    let back: AnalysisResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back.state, StateHandle(None));
    let mut expected = a.clone();
    expected.state = StateHandle::default();
    assert_eq!(back, expected);
}

// ── type assertions ──────────────────────────────────────────────────────

const fn serialised<T: Serialize + DeserializeOwned + PartialEq>() {}
const fn send<T: Send>() {}

const _: () = {
    serialised::<SalvageBudget>();
    serialised::<AnalyzeOptions>();
    serialised::<RepairOptions>();
    serialised::<AnalysisResult>();
    serialised::<RepairPlan>();
    serialised::<RepairOutcome>();
    serialised::<FontSlot>();
    serialised::<ToUnicodeState>();
    serialised::<FontResolution>();
    serialised::<FontResolutionKind>();
    serialised::<CarveSummary>();
    serialised::<XrefKind>();
    serialised::<AnalyzeStats>();
    serialised::<BaselineKind>();
    serialised::<Toolpath>();
    serialised::<Escalation>();
    serialised::<EscalationKind>();
    serialised::<FontSourcePolicy>();
    serialised::<UnreproduciblePolicy>();
    serialised::<PageSize>();
    serialised::<LogLevel>();
    serialised::<Cancelled>();
    serialised::<FontDbError>();
    serialised::<OutcomeStatus>();
    serialised::<InteractionRequest>();
    serialised::<InteractionReply>();
    serialised::<InteractionRequestId>();
    serialised::<FontPickRequest>();
    serialised::<FontCandidate>();
    serialised::<UnreproducibleRequest>();
    serialised::<SubstituteChoice>();
    serialised::<InteractionRecord>();
    serialised::<InteractionSummary>();
    serialised::<InteractionSource>();
    serialised::<SettingsSnapshot>();
    serialised::<RepairReport>();
    serialised::<PassReport>();
    serialised::<PassOutcome>();
    serialised::<RepairAction>();
    serialised::<C9Summary>();
    serialised::<CandidateReport>();
    serialised::<ExtractedImage>();
    serialised::<Verification>();
    serialised::<Gates>();
    serialised::<Retention>();
    serialised::<Plausibility>();
    serialised::<AnalysisStateUse>();
    serialised::<Placed>();
    serialised::<crate::library::HistorySummary>();
    serialised::<crate::library::RecentRow>();
    serialised::<crate::library::RecentStatus>();

    send::<AnalyzeOptions>();
    send::<RepairOptions>();
    send::<AnalysisResult>();
    send::<RepairPlan>();
    send::<RepairOutcome>();
    send::<RepairReport>();
    send::<InteractionRequest>();
    send::<InteractionReply>();
    send::<StateHandle>();
    send::<AnalysisState>();
    send::<FontDb>();
    send::<Arc<FontDb>>();
    send::<Pdfpundit>();
    send::<UseBest>();
    send::<NullProgress>();
    send::<Cancelled>();
};

// ── snapshots of the serialised shape ────────────────────────────────────

const CONTRACT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/contract");

/// Compares `value`'s pretty JSON with the committed file. A shape change fails
/// here; to accept one, run the test with `PDFPUNDIT_BLESS_CONTRACT=1` and
/// review the diff of `tests/data/contract/`. The engine version is pinned to
/// `"<version>"` so a version bump is not a shape change.
fn assert_snapshot<T: Serialize>(name: &str, value: &T) {
    // Serialised directly, not through `serde_json::Value`, which sorts keys:
    // field order is part of the shape.
    let version = format!("\"engine_version\": \"{}\"", env!("CARGO_PKG_VERSION"));
    let mut text = serde_json::to_string_pretty(value)
        .expect("serialise")
        .replace(&version, "\"engine_version\": \"<version>\"");
    text.push('\n');
    let path = Path::new(CONTRACT_DIR).join(name);
    if std::env::var_os("PDFPUNDIT_BLESS_CONTRACT").is_some() {
        std::fs::create_dir_all(CONTRACT_DIR).expect("create contract dir");
        std::fs::write(&path, &text).expect("write snapshot");
        return;
    }
    let committed =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        committed == text,
        "the serialised shape of {name} changed. If that is intended, re-run with \
         PDFPUNDIT_BLESS_CONTRACT=1 and commit the diff.\n--- now:\n{text}"
    );
}

#[test]
fn snapshot_default_report() {
    // A bare header's hash and size, with no findings and no other counts:
    // the snapshot checks the shape, not what analysis finds in the file.
    let bytes = b"%PDF-1.7\n";
    let mut analysis = analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).unwrap();
    analysis.findings.clear();
    analysis.stats = AnalyzeStats {
        bytes: bytes.len() as u64,
        ..AnalyzeStats::default()
    };
    let r = RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty());
    assert_snapshot("repair_report_default.json", &r);
}

#[test]
fn snapshot_populated_report() {
    assert_snapshot("repair_report_populated.json", &report_fixture());
}

#[test]
fn snapshot_analysis_result() {
    assert_snapshot("analysis_result.json", &analysis_fixture());
}

#[test]
fn a_changed_shape_fails_the_snapshot() {
    if std::env::var_os("PDFPUNDIT_BLESS_CONTRACT").is_some() {
        return;
    }
    // The default report plus one field: what adding a field to RepairReport
    // would serialise as.
    #[derive(Serialize)]
    struct Grown {
        #[serde(flatten)]
        report: RepairReport,
        placement: &'static str,
    }
    let analysis = analyze(b"%PDF-1.7\n", &AnalyzeOptions::default(), &mut NullProgress).unwrap();
    let grown = Grown {
        report: RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty()),
        placement: "Atomic",
    };
    let failed =
        std::panic::catch_unwind(|| assert_snapshot("repair_report_default.json", &grown)).is_err();
    assert!(failed, "an added field must fail the snapshot");
}

// ── the populated round-trip ─────────────────────────────────────────────

/// The fields T-16's `RunRecord` holds (D-073), for the round-trip only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RunRecordShape {
    started_at: u64,
    input_sha256: [u8; 32],
    input_name: String,
    output_path: Option<PathBuf>,
    placed: Option<Placed>,
    analysis_state: AnalysisStateUse,
    config_path: PathBuf,
    report: RepairReport,
}

/// Every serialised facade value not already inside a report or an analysis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Extras {
    plan: RepairPlan,
    outcomes: Vec<RepairOutcome>,
    options: Vec<RepairOptions>,
    requests: Vec<InteractionRequest>,
    levels: Vec<LogLevel>,
    db_errors: Vec<FontDbError>,
    cancelled: Cancelled,
    placed: Vec<Placed>,
    history: crate::library::HistorySummary,
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) {
    let json = serde_json::to_string(value).expect("serialise");
    let back: T = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(&back, value);
    assert_eq!(serde_json::to_string(&back).expect("again"), json);
}

#[test]
fn populated_values_round_trip_through_json() {
    let report = report_fixture();
    assert!(report.c9_survivors.len() == 2);
    assert!(
        report.c9_survivors.windows(2).all(|w| w[0].0 < w[1].0),
        "c9_survivors is sorted by ObjId"
    );
    round_trip(&report);
    round_trip(&analysis_fixture());
    round_trip(&run_records());
    round_trip(&extras());
}

#[test]
fn fixtures_carry_every_variant() {
    let r = report_fixture();
    let a = analysis_fixture();
    let e = extras();
    let runs = run_records();

    // Every Vec in the report and the analysis is non-empty.
    assert!(!r.interactions.is_empty() && !r.findings_before.is_empty());
    assert!(!r.findings_after.is_empty() && !r.passes.is_empty());
    assert!(r.passes.iter().all(|p| !p.actions.is_empty()));
    assert!(!r.candidates.is_empty() && !r.filter_rewrites.is_empty());
    assert!(!r.shadows.is_empty() && !r.extracted_images.is_empty());
    assert!(!r.partial_reasons.is_empty() && r.c9_survivors.iter().all(|s| !s.1.is_empty()));
    assert!(!a.findings.is_empty() && !a.font_slots.is_empty());
    assert!(!a.carve.caps_hit.is_empty() && !a.stats.paint_counts.is_empty());
    assert!(!a.stats.streams_by_salvage.is_empty());
    assert!(a.font_slots.iter().all(|s| {
        s.resolution
            .as_ref()
            .is_none_or(|r| !r.provenance.is_empty())
    }));

    let all_findings = || a.findings.iter().chain(&r.findings_after);
    assert!(covers(
        all_findings().filter_map(|f| match f.class {
            FindingKind::Corruption(c) => Some(class_index(c)),
            _ => None,
        }),
        CorruptionClass::ALL.len()
    ));
    assert!(covers(all_findings().map(|f| kind_index(&f.class)), 6));
    assert!(covers(
        all_findings().map(|f| severity_index(f.severity)),
        3
    ));
    assert!(covers(
        all_findings().map(|f| location_index(&f.location)),
        4
    ));
    assert!(covers(
        all_findings().flat_map(|f| &f.evidence).map(evidence_index),
        4
    ));
    assert!(covers(
        all_findings()
            .flat_map(|f| &f.evidence)
            .filter_map(|e| match e {
                Evidence::Metric { value, .. } => Some(metric_index(value)),
                _ => None,
            }),
        3
    ));
    assert!(covers(all_findings().map(|f| repair_index(&f.repair)), 5));
    assert!(
        r.findings_after
            .iter()
            .any(|f| matches!(f.class, FindingKind::Signed { .. })),
        "the after list carries an info finding"
    );

    assert!(covers(
        a.font_slots.iter().map(|s| tounicode_index(s.tounicode)),
        3
    ));
    assert!(covers(
        a.font_slots
            .iter()
            .filter_map(|s| s.resolution.as_ref())
            .map(|r| resolution_index(&r.kind)),
        5
    ));
    assert!(covers(carves().iter().map(|c| xref_index(c.xref_kind)), 3));
    assert!(covers(
        [&a.stats, &r.stats]
            .into_iter()
            .chain(stats_variants().iter())
            .map(|s| baseline_index(s.baseline_kind)),
        3
    ));
    assert!(covers(
        r.candidates
            .iter()
            .map(|c| baseline_index(c.verification.baseline)),
        3
    ));
    assert!(covers(
        r.candidates.iter().map(|c| toolpath_index(c.toolpath)),
        2
    ));
    assert!(covers(
        e.plan.escalations.iter().map(|x| escalation_index(&x.kind)),
        5
    ));
    assert!(covers(
        e.options.iter().map(|o| font_policy_index(o.font_policy)),
        2
    ));
    assert!(covers(
        e.options
            .iter()
            .map(|o| unreproducible_index(o.unreproducible)),
        3
    ));
    assert!(covers(
        e.options
            .iter()
            .map(|o| page_size_index(o.default_page_size)),
        3
    ));
    assert!(covers(
        e.outcomes.iter().map(|o| status_index(&o.status)),
        3
    ));
    assert!(covers(
        e.outcomes
            .iter()
            .map(|o| state_use_index(&o.analysis_state)),
        2
    ));
    assert!(covers(e.requests.iter().map(request_index), 2));
    assert!(covers(
        r.interactions.iter().map(|i| reply_index(&i.reply)),
        5
    ));
    assert!(covers(
        r.interactions.iter().map(|i| source_index(i.source)),
        3
    ));
    assert!(covers(
        r.passes.iter().map(|p| pass_outcome_index(&p.outcome)),
        3
    ));
    let grades = r
        .passes
        .iter()
        .flat_map(|p| p.actions.iter())
        .map(|x| grade_index(x.grade));
    assert!(covers(grades, 5));
    assert!(covers(e.levels.iter().map(|l| level_index(*l)), 3));
    assert!(covers(e.db_errors.iter().map(db_error_index), 3));
    assert!(covers(e.placed.iter().map(|p| placed_index(*p)), 3));
    assert!(covers(
        runs.iter().map(|x| state_use_index(&x.analysis_state)),
        2
    ));
    assert!(covers(
        e.history.recent.iter().map(|x| recent_index(x.status)),
        4
    ));
}

// Exhaustive matches: a new variant breaks the build here until a fixture
// carries it.

fn covers(indexes: impl IntoIterator<Item = usize>, n: usize) -> bool {
    let mut seen = vec![false; n];
    for i in indexes {
        seen[i] = true;
    }
    seen.into_iter().all(|s| s)
}

fn class_index(c: CorruptionClass) -> usize {
    match c {
        CorruptionClass::C1Header => 0,
        CorruptionClass::C2XrefMissing => 1,
        CorruptionClass::C3TrailerDamaged => 2,
        CorruptionClass::C4PageTreeBroken => 3,
        CorruptionClass::C5ObjectTagStripped => 4,
        CorruptionClass::C6FontMapLost => 5,
        CorruptionClass::C7FontStreamDeleted => 6,
        CorruptionClass::C8FontResourcesDeleted => 7,
        CorruptionClass::C9ZlibTampered => 8,
        CorruptionClass::C10Truncated => 9,
    }
}

fn kind_index(k: &FindingKind) -> usize {
    match k {
        FindingKind::Corruption(_) => 0,
        FindingKind::Encrypted => 1,
        FindingKind::Signed { .. } => 2,
        FindingKind::OutlinedText { .. } => 3,
        FindingKind::Type3Text { .. } => 4,
        FindingKind::FontNotEmbedded { .. } => 5,
    }
}

fn severity_index(s: Severity) -> usize {
    match s {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn location_index(l: &Location) -> usize {
    match l {
        Location::File => 0,
        Location::Span(_) => 1,
        Location::Object { .. } => 2,
        Location::Page { .. } => 3,
    }
}

fn evidence_index(e: &Evidence) -> usize {
    match e {
        Evidence::Text(_) => 0,
        Evidence::HexWindow(_) => 1,
        Evidence::ObjectRef(_) => 2,
        Evidence::Metric { .. } => 3,
    }
}

fn metric_index(m: &MetricValue) -> usize {
    match m {
        MetricValue::Int(_) => 0,
        MetricValue::Ratio(_) => 1,
        MetricValue::Text(_) => 2,
    }
}

fn repair_index(r: &Repairability) -> usize {
    match r {
        Repairability::Auto => 0,
        Repairability::Interactive(_) => 1,
        Repairability::Partial(_) => 2,
        Repairability::Unrepairable(_) => 3,
        Repairability::NotApplicable => 4,
    }
}

fn tounicode_index(t: ToUnicodeState) -> usize {
    match t {
        ToUnicodeState::Present => 0,
        ToUnicodeState::Missing => 1,
        ToUnicodeState::Unparsable => 2,
    }
}

fn resolution_index(k: &FontResolutionKind) -> usize {
    match k {
        FontResolutionKind::Relinked(_) => 0,
        FontResolutionKind::Picked { .. } => 1,
        FontResolutionKind::Substituted(_) => 2,
        FontResolutionKind::TextOnly => 3,
        FontResolutionKind::Skipped => 4,
    }
}

fn xref_index(k: XrefKind) -> usize {
    match k {
        XrefKind::Classic => 0,
        XrefKind::Stream => 1,
        XrefKind::None => 2,
    }
}

fn baseline_index(k: BaselineKind) -> usize {
    match k {
        BaselineKind::Extracted => 0,
        BaselineKind::CarveProxy => 1,
        BaselineKind::None => 2,
    }
}

fn toolpath_index(t: Toolpath) -> usize {
    match t {
        Toolpath::Resave => 0,
        Toolpath::TemplateAssemble => 1,
    }
}

fn escalation_index(k: &EscalationKind) -> usize {
    match k {
        EscalationKind::Interaction(InteractionKind::FontPick) => 0,
        EscalationKind::Interaction(InteractionKind::FontUnreproducible) => 1,
        EscalationKind::CandidatesDisagree => 2,
        EscalationKind::OutOfDistribution => 3,
        EscalationKind::NoCandidatePassed => 4,
    }
}

fn font_policy_index(p: FontSourcePolicy) -> usize {
    match p {
        FontSourcePolicy::Bundled => 0,
        FontSourcePolicy::BundledThenSystem => 1,
    }
}

fn unreproducible_index(p: UnreproduciblePolicy) -> usize {
    match p {
        UnreproduciblePolicy::Ask => 0,
        UnreproduciblePolicy::SubstituteGeneric => 1,
        UnreproduciblePolicy::TextOnly => 2,
    }
}

fn page_size_index(p: PageSize) -> usize {
    match p {
        PageSize::A4 => 0,
        PageSize::Letter => 1,
        PageSize::Custom { .. } => 2,
    }
}

fn status_index(s: &OutcomeStatus) -> usize {
    match s {
        OutcomeStatus::Ok => 0,
        OutcomeStatus::Partial(_) => 1,
        OutcomeStatus::Failed(_) => 2,
    }
}

fn state_use_index(s: &AnalysisStateUse) -> usize {
    match s {
        AnalysisStateUse::Reused => 0,
        AnalysisStateUse::Rebuilt { .. } => 1,
    }
}

fn request_index(r: &InteractionRequest) -> usize {
    match r {
        InteractionRequest::FontPick(_) => 0,
        InteractionRequest::FontUnreproducible(_) => 1,
    }
}

fn reply_index(r: &InteractionReply) -> usize {
    match r {
        InteractionReply::Pick(_) => 0,
        InteractionReply::UseBest => 1,
        InteractionReply::Skip => 2,
        InteractionReply::Substitute(_) => 3,
        InteractionReply::TextOnly => 4,
    }
}

fn source_index(s: InteractionSource) -> usize {
    match s {
        InteractionSource::User => 0,
        InteractionSource::Policy => 1,
        InteractionSource::UseBest => 2,
    }
}

fn pass_outcome_index(o: &PassOutcome) -> usize {
    match o {
        PassOutcome::Fixed => 0,
        PassOutcome::Partial(_) => 1,
        PassOutcome::Skipped(_) => 2,
    }
}

fn grade_index(g: Option<SalvageGrade>) -> usize {
    match g {
        None => 0,
        Some(SalvageGrade::Exact) => 1,
        Some(SalvageGrade::Accepted) => 2,
        Some(SalvageGrade::Ambiguous) => 3,
        Some(SalvageGrade::Unsearched) => 4,
    }
}

fn level_index(l: LogLevel) -> usize {
    match l {
        LogLevel::Info => 0,
        LogLevel::Warn => 1,
        LogLevel::Error => 2,
    }
}

fn db_error_index(e: &FontDbError) -> usize {
    match e {
        FontDbError::BadIndex(_) => 0,
        FontDbError::MissingBlob(_) => 1,
        FontDbError::HashMismatch { .. } => 2,
    }
}

fn placed_index(p: Placed) -> usize {
    match p {
        Placed::Atomic => 0,
        Placed::Linked => 1,
        Placed::ClaimedThenRenamed => 2,
    }
}

fn recent_index(s: crate::library::RecentStatus) -> usize {
    use crate::library::RecentStatus;
    match s {
        RecentStatus::Repaired => 0,
        RecentStatus::Partial => 1,
        RecentStatus::Pending => 2,
        RecentStatus::Failed => 3,
    }
}

// ── fixtures ─────────────────────────────────────────────────────────────

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn r(num: u64, den: u64) -> Ratio {
    Ratio { num, den }
}

/// One finding per corruption class plus the four non-corruption kinds,
/// cycling through every `Severity`, `Location`, `Evidence`, `MetricValue` and
/// `Repairability` variant.
fn model_findings() -> Vec<Finding> {
    let span = ByteSpan {
        start: 100,
        end: 180,
    };
    let locations = [
        Location::File,
        Location::Span(span),
        Location::Object {
            id: (14, 0),
            span: Some(span),
        },
        Location::Page {
            index: 2,
            obj: Some((3, 0)),
        },
        Location::Object {
            id: (15, 0),
            span: None,
        },
        Location::Page {
            index: 0,
            obj: None,
        },
    ];
    let metrics = [
        MetricValue::Int(-12),
        MetricValue::Ratio(r(7, 10)),
        MetricValue::Text("Repaired".into()),
    ];
    let repairs = [
        Repairability::Auto,
        Repairability::Interactive(InteractionKind::FontPick),
        Repairability::Partial("the truncated tail is lost".into()),
        Repairability::Unrepairable("no object survived".into()),
        Repairability::Interactive(InteractionKind::FontUnreproducible),
    ];
    let mut findings: Vec<Finding> = CorruptionClass::ALL
        .iter()
        .enumerate()
        .map(|(i, &class)| Finding {
            id: format!("{}-001", class.code()),
            class: FindingKind::Corruption(class),
            severity: if i % 2 == 0 {
                Severity::Error
            } else {
                Severity::Warning
            },
            location: locations[i % locations.len()],
            summary: class.label().into(),
            evidence: vec![
                Evidence::Text(format!("{} at byte {}", class.code(), 100 * i)),
                Evidence::HexWindow(HexWindow::new(100 * i as u64, b"endobj\nxref")),
                Evidence::ObjectRef((i as u32 + 1, 0)),
                Evidence::Metric {
                    name: "offset delta".into(),
                    value: metrics[i % metrics.len()].clone(),
                },
            ],
            repair: repairs[i % repairs.len()].clone(),
        })
        .collect();
    let info = |id: &str, class, severity, repair, summary: &str| Finding {
        id: id.into(),
        class,
        severity,
        location: Location::File,
        summary: summary.into(),
        evidence: vec![Evidence::Text(summary.into())],
        repair,
    };
    findings.extend([
        info(
            "ENC-001",
            FindingKind::Encrypted,
            Severity::Error,
            Repairability::Unrepairable("decrypt it first".into()),
            "the file is encrypted",
        ),
        info(
            "SIG-001",
            FindingKind::Signed { fields: 2 },
            Severity::Info,
            Repairability::NotApplicable,
            "the file is signed",
        ),
        info(
            "OUT-001",
            FindingKind::OutlinedText {
                glyph_runs: 12,
                contours: 340,
            },
            Severity::Info,
            Repairability::NotApplicable,
            "text drawn as paths",
        ),
        info(
            "T3-001",
            FindingKind::Type3Text { font: (21, 0) },
            Severity::Info,
            Repairability::NotApplicable,
            "text drawn by a Type 3 font",
        ),
        info(
            "NOEMBED-001",
            FindingKind::FontNotEmbedded {
                font: (5, 0),
                base_font: "Arial".into(),
            },
            Severity::Info,
            Repairability::NotApplicable,
            "the font Arial is not embedded",
        ),
    ]);
    findings
}

fn substitute() -> SubstituteChoice {
    SubstituteChoice {
        font_id: "noto-sans-regular".into(),
        label: "Noto Sans Regular".into(),
    }
}

fn font_pick_request() -> FontPickRequest {
    FontPickRequest {
        id: InteractionRequestId(1),
        page: 2,
        slot: "F1".into(),
        sample_codes: vec![3, 17, 42],
        candidates: vec![
            FontCandidate {
                font_id: "noto-sans-regular".into(),
                family: "Noto Sans".into(),
                language: "en".into(),
                score: r(91, 100),
                confidence: r(4, 5),
                preview: "Quarterly figures".into(),
            },
            FontCandidate {
                font_id: "noto-serif-regular".into(),
                family: "Noto Serif".into(),
                language: "fr".into(),
                score: r(40, 100),
                confidence: r(1, 5),
                preview: "Qvarterly fignres".into(),
            },
        ],
        preview: "Quarterly figures".into(),
    }
}

fn unreproducible_request() -> UnreproducibleRequest {
    UnreproducibleRequest {
        id: InteractionRequestId(2),
        family: "Garamond".into(),
        slots: vec![(0, "F2".into()), (3, "F2".into())],
        reason: "no bundled font covers the glyphs".into(),
        options: vec![substitute()],
    }
}

fn font_slots() -> Vec<FontSlot> {
    let slot = |page, name: &str, tounicode, resolution| FontSlot {
        page,
        slot: name.into(),
        base_font: Some("/ABCDEF+NotoSans-Regular".into()),
        subtype: Some("/Type0".into()),
        embedded: true,
        tounicode,
        glyph_count: 104,
        resolution,
    };
    let res = |kind| {
        Some(FontResolution {
            kind,
            provenance: vec!["carved /FontFile2".into()],
        })
    };
    vec![
        slot(
            0,
            "F1",
            ToUnicodeState::Present,
            res(FontResolutionKind::Relinked((9, 0))),
        ),
        slot(
            1,
            "F1",
            ToUnicodeState::Missing,
            res(FontResolutionKind::Picked {
                font_id: "noto-sans-regular".into(),
                confidence: r(7, 10),
            }),
        ),
        slot(
            2,
            "F2",
            ToUnicodeState::Unparsable,
            res(FontResolutionKind::Substituted(substitute())),
        ),
        slot(
            3,
            "F2",
            ToUnicodeState::Missing,
            res(FontResolutionKind::TextOnly),
        ),
        slot(
            4,
            "F3",
            ToUnicodeState::Missing,
            res(FontResolutionKind::Skipped),
        ),
        slot(5, "F4", ToUnicodeState::Present, None),
    ]
}

fn carves() -> Vec<CarveSummary> {
    [XrefKind::Classic, XrefKind::Stream, XrefKind::None]
        .into_iter()
        .map(|xref_kind| CarveSummary {
            objects: 212,
            orphans: 3,
            streams: 40,
            shadows: 1,
            xref_kind,
            header_offset: Some(4),
            truncated: xref_kind == XrefKind::None,
            caps_hit: vec!["container elements".into()],
        })
        .collect()
}

fn stats(baseline_kind: BaselineKind) -> AnalyzeStats {
    AnalyzeStats {
        bytes: 1_153_024,
        pages_discovered: 6,
        salvage_work_total: 10_700_000_000,
        streams_by_salvage: [("Clean".to_string(), 38), ("Repaired".to_string(), 2)]
            .into_iter()
            .collect(),
        baseline_kind,
        paint_counts: vec![
            PaintCounts {
                path_fills: 1,
                path_strokes: 2,
                images: 1,
                glyph_runs_visible: 12,
                glyph_runs_invisible: 0,
                glyphs_inked: 340,
                clips: 1,
            },
            PaintCounts::default(),
        ],
    }
}

fn stats_variants() -> Vec<AnalyzeStats> {
    [
        BaselineKind::Extracted,
        BaselineKind::CarveProxy,
        BaselineKind::None,
    ]
    .into_iter()
    .map(stats)
    .collect()
}

fn analysis_fixture() -> AnalysisResult {
    AnalysisResult {
        meta: FileMeta {
            version: Some("1.4".into()),
            pages: 6,
            title: Some("Quarterly figures".into()),
            page_sizes: vec![(612, 792), (595, 842)],
        },
        findings: model_findings(),
        carve: carves().remove(0),
        font_slots: font_slots(),
        stats: stats(BaselineKind::Extracted),
        input_sha256: [0xab; 32],
        state: StateHandle::default(),
    }
}

fn verification(baseline: BaselineKind, pass: bool) -> Verification {
    Verification {
        baseline,
        v0: Gates {
            lopdf_reload: true,
            hayro_render_all_pages: true,
            rediagnose_clean: pass,
            page_count_ok: true,
            root_and_pages_present: true,
        },
        v1: Retention {
            text_ops: r(98, 100),
            path_fills: r(1, 1),
            images: r(1, 1),
            content_bytes: r(4012, 4100),
            blank_pages: 0,
            glyph_count: r(339, 340),
        },
        v2: Plausibility {
            unmapped_glyph: r(0, 340),
            fffd: r(0, 340),
            script_consistency: r(1, 1),
            dictionary_hit: if pass { Some(r(9, 10)) } else { None },
        },
    }
}

fn report_fixture() -> RepairReport {
    let analysis = analysis_fixture();
    let mut report =
        RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty());
    report.engine_version = "0.0.0-fixture".into();
    report.font_db_sha256 = [0x11; 32];
    report.interactions = vec![
        InteractionRecord {
            request: InteractionSummary {
                kind: InteractionKind::FontPick,
                page: Some(2),
                slot: Some("F1".into()),
                candidates: vec!["noto-sans-regular".into(), "noto-serif-regular".into()],
            },
            reply: InteractionReply::Pick("noto-sans-regular".into()),
            source: InteractionSource::User,
        },
        InteractionRecord {
            request: InteractionSummary {
                kind: InteractionKind::FontPick,
                page: Some(3),
                slot: Some("F1".into()),
                candidates: vec!["noto-sans-regular".into()],
            },
            reply: InteractionReply::UseBest,
            source: InteractionSource::UseBest,
        },
        InteractionRecord {
            request: InteractionSummary {
                kind: InteractionKind::FontUnreproducible,
                page: Some(0),
                slot: Some("F2".into()),
                candidates: vec!["noto-sans-regular".into()],
            },
            reply: InteractionReply::Substitute(substitute()),
            source: InteractionSource::Policy,
        },
        InteractionRecord {
            request: InteractionSummary {
                kind: InteractionKind::FontUnreproducible,
                page: None,
                slot: None,
                candidates: vec![],
            },
            reply: InteractionReply::TextOnly,
            source: InteractionSource::User,
        },
        InteractionRecord {
            request: InteractionSummary {
                kind: InteractionKind::FontPick,
                page: Some(4),
                slot: Some("F3".into()),
                candidates: vec![],
            },
            reply: InteractionReply::Skip,
            source: InteractionSource::User,
        },
    ];
    report.findings_after = model_findings()
        .into_iter()
        .filter(|f| {
            matches!(
                f.class,
                FindingKind::Corruption(CorruptionClass::C9ZlibTampered)
                    | FindingKind::Signed { .. }
            )
        })
        .collect();
    let action = |object, what: &str, grade| RepairAction {
        object,
        what: what.into(),
        grade,
    };
    report.passes = vec![
        PassReport {
            class: CorruptionClass::C9ZlibTampered,
            outcome: PassOutcome::Fixed,
            actions: vec![
                action((14, 0), "replaced 1 byte", Some(SalvageGrade::Exact)),
                action((15, 0), "replaced 1 byte", Some(SalvageGrade::Accepted)),
                action((16, 0), "replaced 1 byte", Some(SalvageGrade::Ambiguous)),
                action((17, 0), "written as found", Some(SalvageGrade::Unsearched)),
            ],
        },
        PassReport {
            class: CorruptionClass::C10Truncated,
            outcome: PassOutcome::Partial("kept 70%".into()),
            actions: vec![action((30, 0), "dropped the truncated tail", None)],
        },
        PassReport {
            class: CorruptionClass::C6FontMapLost,
            outcome: PassOutcome::Skipped("no orphaned font".into()),
            actions: vec![action((9, 0), "nothing to re-link", None)],
        },
    ];
    report.c9_summary = C9Summary {
        streams_damaged: 4,
        repaired: 3,
        exact: 1,
        accepted: 1,
        ambiguous: 1,
        unrecoverable: 0,
        unsearched: 1,
    };
    report.candidates = vec![
        CandidateReport {
            toolpath: Toolpath::Resave,
            verification: verification(BaselineKind::Extracted, true),
            chosen: true,
        },
        CandidateReport {
            toolpath: Toolpath::TemplateAssemble,
            verification: verification(BaselineKind::CarveProxy, false),
            chosen: false,
        },
        CandidateReport {
            toolpath: Toolpath::TemplateAssemble,
            verification: verification(BaselineKind::None, false),
            chosen: false,
        },
    ];
    report.chosen = Some(Toolpath::Resave);
    report.c9_survivors = vec![
        ((14, 0), vec![vec![(2113, 0x41, 0x42)]]),
        (
            (16, 0),
            vec![vec![(10, 1, 2)], vec![(11, 3, 4), (12, 5, 6)]],
        ),
    ];
    report.filter_rewrites = vec![(
        (20, 0),
        "[/ASCII85Decode /FlateDecode]".into(),
        "/FlateDecode".into(),
    )];
    report.reals_narrowed = 2;
    report.shadows = vec![(
        (5, 0),
        ByteSpan {
            start: 900,
            end: 960,
        },
    )];
    report.signed_note = Some(SIGNED_NOTE.into());
    report.extracted_images = vec![ExtractedImage {
        name: "p3-img1.jpg".into(),
        sha256: [0x22; 32],
        len: 512,
    }];
    report.partial_reasons = vec!["ambiguous: 2 candidate repairs".into()];
    report
}

fn run_records() -> Vec<RunRecordShape> {
    let report = report_fixture();
    vec![
        RunRecordShape {
            started_at: 1_790_000_000,
            input_sha256: report.input_sha256,
            input_name: "invoice_scan.pdf".into(),
            output_path: Some(PathBuf::from("cases/invoice_scan.repaired.pdf")),
            placed: Some(Placed::Atomic),
            analysis_state: AnalysisStateUse::Reused,
            config_path: PathBuf::from("config/config.toml"),
            report: report.clone(),
        },
        RunRecordShape {
            started_at: 1_790_000_100,
            input_sha256: report.input_sha256,
            input_name: "invoice_scan.pdf".into(),
            output_path: None,
            placed: None,
            analysis_state: AnalysisStateUse::Rebuilt {
                reason: "evicted while parked".into(),
            },
            config_path: PathBuf::from("config/config.toml"),
            report,
        },
    ]
}

fn extras() -> Extras {
    use crate::library::{HistorySummary, RecentRow, RecentStatus};
    let options = |font_policy, unreproducible, default_page_size| RepairOptions {
        passes: Some(vec![
            CorruptionClass::C9ZlibTampered,
            CorruptionClass::C4PageTreeBroken,
        ]),
        font_policy,
        unreproducible,
        default_page_size,
        ..RepairOptions::default()
    };
    let outcome = |status, analysis_state| RepairOutcome {
        output: Some(b"%PDF-1.7\n%%EOF\n".to_vec()),
        report: report_fixture(),
        status,
        images: vec![("p3-img1.jpg".into(), vec![0xff, 0xd8, 0xff])],
        analysis_state,
    };
    let escalation = |kind| Escalation {
        kind,
        page: Some(2),
        slot: Some("F1".into()),
        note: "low confidence".into(),
    };
    let row = |name: &str, status| RecentRow {
        name: name.into(),
        status,
    };
    Extras {
        plan: RepairPlan {
            candidates: vec![Toolpath::Resave, Toolpath::TemplateAssemble],
            prior: Toolpath::Resave,
            escalations: vec![
                escalation(EscalationKind::Interaction(InteractionKind::FontPick)),
                escalation(EscalationKind::Interaction(
                    InteractionKind::FontUnreproducible,
                )),
                escalation(EscalationKind::CandidatesDisagree),
                escalation(EscalationKind::OutOfDistribution),
                escalation(EscalationKind::NoCandidatePassed),
            ],
        },
        outcomes: vec![
            outcome(OutcomeStatus::Ok, AnalysisStateUse::Reused),
            outcome(
                OutcomeStatus::Partial(vec!["kept 70%".into()]),
                AnalysisStateUse::Rebuilt {
                    reason: "no state".into(),
                },
            ),
            outcome(
                OutcomeStatus::Failed("engine not built".into()),
                AnalysisStateUse::Reused,
            ),
        ],
        options: vec![
            options(
                FontSourcePolicy::Bundled,
                UnreproduciblePolicy::Ask,
                PageSize::A4,
            ),
            options(
                FontSourcePolicy::BundledThenSystem,
                UnreproduciblePolicy::SubstituteGeneric,
                PageSize::Letter,
            ),
            options(
                FontSourcePolicy::Bundled,
                UnreproduciblePolicy::TextOnly,
                PageSize::Custom {
                    w_pt: 420,
                    h_pt: 595,
                },
            ),
        ],
        requests: vec![
            InteractionRequest::FontPick(font_pick_request()),
            InteractionRequest::FontUnreproducible(unreproducible_request()),
        ],
        levels: vec![LogLevel::Info, LogLevel::Warn, LogLevel::Error],
        db_errors: vec![
            FontDbError::BadIndex("not JSON".into()),
            FontDbError::MissingBlob("NotoSans-Regular.gmap".into()),
            FontDbError::HashMismatch {
                name: "NotoSans-Regular.gmap".into(),
            },
        ],
        cancelled: Cancelled,
        placed: vec![Placed::Atomic, Placed::Linked, Placed::ClaimedThenRenamed],
        history: HistorySummary {
            files: 57,
            runs: 91,
            recent: vec![
                row("thesis_ar.pdf", RecentStatus::Repaired),
                row("invoice_scan.pdf", RecentStatus::Partial),
                row("contract_signed.pdf", RecentStatus::Pending),
                row("payroll_locked.pdf", RecentStatus::Failed),
            ],
        },
    }
}
