//! T-13a acceptance for generate-and-validate and the structural passes:
//! the C1–C5 and C10 fixtures re-diagnose clean for their class and pass
//! V0/V1, the passes override, TemplateAssemble is skipped with a log, the
//! C10 pass drops an unreachable cut object, a file with no page gets no
//! output, the runs are deterministic, and the selection tuple is a pure
//! function with a table test.

use std::cmp::Ordering;
use std::num::NonZeroUsize;

use lopdf::{Dictionary, Document, Object, Stream};
use sha2::{Digest, Sha256};

use super::*;
use crate::engine::{
    Cancelled, EscalationKind, FontDb, LogLevel, NullProgress, PassOutcome, Progress,
    RepairOptions, SalvageBudget, UseBest,
};
use crate::pdf::carver::carve;
use crate::pdf::diagnose::diagnose;
use crate::pdf::fixtures::{GOLDEN_TEXT, corrupt, golden_pdf, golden_pdf_objstm};
use crate::pdf::model::{FindingKind, Ratio};
use crate::pdf::plan::plan;
use crate::pdf::streams::salvage::{CarveSource, salvage_all};
use crate::pdf::verify::{Plausibility, Retention};
use crate::pdf::write::Writer;

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped,
    C6FontMapLost, C9ZlibTampered, C10Truncated,
};

// ── helpers ──────────────────────────────────────────────────────────────

/// What analysis keeps for one input (the `AnalysisState` contents).
struct Input {
    bytes: Vec<u8>,
    carve: CarveReport,
    graph: ObjectGraph,
    salvage: SalvageIndex,
    findings: Vec<Finding>,
}

fn analysed(bytes: Vec<u8>) -> Input {
    let carve = carve(&bytes, &|| false).expect("never cancelled");
    let graph = ObjectGraph::from_carve(&carve);
    let salvage = salvage_all(
        &CarveSource::new(&carve, &bytes),
        &SalvageBudget::default(),
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .expect("never cancelled");
    let findings = diagnose(&bytes, &carve, &graph, &salvage);
    Input {
        bytes,
        carve,
        graph,
        salvage,
        findings,
    }
}

impl Input {
    fn view(&self) -> Analysed<'_> {
        Analysed {
            bytes: &self.bytes,
            carve: &self.carve,
            graph: &self.graph,
            salvage: &self.salvage,
            findings: &self.findings,
            input_sha256: Sha256::digest(&self.bytes).into(),
        }
    }

    fn classes(&self) -> Vec<CorruptionClass> {
        corruption_classes(&self.findings)
    }
}

fn corruption_classes(findings: &[Finding]) -> Vec<CorruptionClass> {
    let mut v: Vec<CorruptionClass> = findings
        .iter()
        .filter_map(|f| match f.class {
            FindingKind::Corruption(c) => Some(c),
            _ => None,
        })
        .collect();
    v.dedup();
    v
}

/// A sink that keeps every log line.
#[derive(Default)]
struct Recorder {
    logs: Vec<(LogLevel, String)>,
    cancel: bool,
}

impl Progress for Recorder {
    fn phase(&mut self, _name: &'static str, _index: u32, _total: u32) {}
    fn progress(&mut self, _done: u64, _total: Option<u64>) {}
    fn finding(&mut self, _f: &Finding) {}
    fn log(&mut self, level: LogLevel, msg: String) {
        self.logs.push((level, msg));
    }
    fn cancelled(&self) -> bool {
        self.cancel
    }
}

fn run_with(input: &Input, opts: &RepairOptions) -> (Generated, Recorder) {
    let plan = plan(&input.findings, &input.carve, &input.graph, opts);
    let mut sink = Recorder::default();
    let generated = generate_and_validate(
        &input.view(),
        &plan,
        opts,
        &FontDb::empty(),
        &mut UseBest,
        &mut sink,
    )
    .expect("never cancelled");
    (generated, sink)
}

fn run(input: &Input) -> (Generated, Recorder) {
    run_with(input, &RepairOptions::default())
}

/// Re-diagnosis of `output` with the full salvage budget: the classes found.
fn classes_in(output: &[u8]) -> Vec<CorruptionClass> {
    corruption_classes(&analysed(output.to_vec()).findings)
}

/// V1 passes: no page went blank and no count fell below the baseline's.
/// `0/0` (the baseline had none of a thing) passes.
fn passes_v1(v1: &Retention) -> bool {
    let kept = |r: Ratio| r.num >= r.den;
    v1.blank_pages == 0
        && kept(v1.text_ops)
        && kept(v1.path_fills)
        && kept(v1.images)
        && kept(v1.content_bytes)
        && kept(v1.glyph_count)
}

fn chosen(g: &Generated) -> &CandidateReport {
    let chosen: Vec<&CandidateReport> = g.candidates.iter().filter(|c| c.chosen).collect();
    assert_eq!(chosen.len(), 1, "{:?}", g.candidates);
    chosen[0]
}

fn pass(g: &Generated, class: CorruptionClass) -> &PassReport {
    g.passes
        .iter()
        .find(|p| p.class == class)
        .unwrap_or_else(|| panic!("no {class:?} pass in {:?}", g.passes))
}

fn golden_glyphs() -> usize {
    GOLDEN_TEXT
        .iter()
        .flat_map(|p| p.iter())
        .map(|l| l.chars().count())
        .sum()
}

// ── the structural fixtures ──────────────────────────────────────────────

/// The seeds whose corruption leaves every page recoverable and whose input
/// hayro reads without a fallback font. C4 seeds 2 and 4 cut into page 1's
/// own dictionary, so that page is gone from the input. C5 seeds 2 to 4 strip
/// the font's header: hayro draws the input with a fallback font and counts
/// each two-byte code as two glyphs, so the baseline holds twice the glyphs
/// and V1's glyph fraction is 1/2 for a perfect output (the next test checks
/// those outputs against the golden's text instead).
const SEEDS: [(CorruptionClass, &[u64]); 6] = [
    (C1Header, &[0, 1, 2]),
    (C2XrefMissing, &[0]),
    (C3TrailerDamaged, &[0]),
    (C4PageTreeBroken, &[0, 1, 3, 5]),
    (C5ObjectTagStripped, &[0, 1, 5]),
    (C10Truncated, &[0]),
];

#[test]
fn each_structural_fixture_repairs_clean_and_passes_v0_and_v1() {
    for (class, seeds) in SEEDS {
        for &seed in seeds {
            let input = analysed(corrupt(class, &golden_pdf(), seed));
            assert!(
                input.classes().contains(&class),
                "{class:?}/{seed}: {:?}",
                input.findings
            );
            let (g, _) = run(&input);
            let out = g.output.as_deref().expect("an output");
            assert_eq!(g.chosen, Some(Toolpath::Resave));
            let c = chosen(&g);
            assert!(
                c.verification.v0.all_pass(),
                "{class:?}/{seed}: {:?}",
                c.verification.v0
            );
            assert!(
                passes_v1(&c.verification.v1),
                "{class:?}/{seed}: {:?}",
                c.verification.v1
            );
            assert!(
                !classes_in(out).contains(&class),
                "{class:?}/{seed}: the output still has it"
            );
            let expected = match class {
                C10Truncated => PassOutcome::Partial(String::new()),
                _ => PassOutcome::Fixed,
            };
            assert_eq!(
                std::mem::discriminant(&pass(&g, class).outcome),
                std::mem::discriminant(&expected),
                "{class:?}/{seed}"
            );
        }
    }
}

#[test]
fn a_repaired_page_tree_or_orphan_keeps_the_golden_text() {
    for (class, seed) in [
        (C4PageTreeBroken, 3),
        (C5ObjectTagStripped, 2),
        (C5ObjectTagStripped, 3),
        (C5ObjectTagStripped, 4),
    ] {
        let (g, _) = run(&analysed(corrupt(class, &golden_pdf(), seed)));
        assert!(chosen(&g).verification.v0.all_pass(), "{class:?}/{seed}");
        let out = g.output.expect("an output");
        assert!(!classes_in(&out).contains(&class), "{class:?}/{seed}");
        let pages = crate::pdf::text::extract_text(&out, &Default::default()).expect("loads");
        let glyphs: usize = pages.iter().map(|p| p.glyphs.len()).sum();
        assert_eq!(
            (pages.len(), glyphs),
            (2, golden_glyphs()),
            "{class:?}/{seed}"
        );
    }
}

#[test]
fn c1_to_c3_are_resolved_by_re_emission() {
    for class in [C1Header, C2XrefMissing, C3TrailerDamaged] {
        let input = analysed(corrupt(class, &golden_pdf(), 0));
        let (g, _) = run(&input);
        let report = pass(&g, class);
        assert_eq!(report.outcome, PassOutcome::Fixed);
        assert_eq!(report.actions.len(), 1, "{class:?}");
        assert!(
            report.actions[0].what.ends_with("resolved by re-emission"),
            "{:?}",
            report.actions[0]
        );
    }
}

#[test]
fn the_c4_pass_records_the_flat_tree_and_the_c5_pass_the_placed_orphan() {
    let (g, _) = run(&analysed(corrupt(C4PageTreeBroken, &golden_pdf(), 3)));
    let c4 = pass(&g, C4PageTreeBroken);
    let pages = c4
        .actions
        .iter()
        .filter(|a| a.what.starts_with("page "))
        .count();
    assert_eq!(pages, 2, "{:?}", c4.actions);

    let input = analysed(corrupt(C5ObjectTagStripped, &golden_pdf(), 2));
    let (g, _) = run(&input);
    let c5 = pass(&g, C5ObjectTagStripped);
    assert_eq!(c5.outcome, PassOutcome::Fixed);
    assert_eq!(c5.actions.len(), 1);
    assert!(
        c5.actions[0]
            .what
            .starts_with("the headerless dictionary at byte "),
        "{:?}",
        c5.actions[0]
    );
}

#[test]
fn template_assemble_is_skipped_with_a_log() {
    let input = analysed(corrupt(C4PageTreeBroken, &golden_pdf(), 3));
    let plan = plan(
        &input.findings,
        &input.carve,
        &input.graph,
        &RepairOptions::default(),
    );
    assert_eq!(
        plan.candidates,
        [Toolpath::Resave, Toolpath::TemplateAssemble]
    );
    let (g, sink) = run(&input);
    assert_eq!(g.candidates.len(), 1);
    assert_eq!(g.candidates[0].toolpath, Toolpath::Resave);
    assert!(
        sink.logs
            .iter()
            .any(|(_, m)| m.contains("TemplateAssemble")),
        "{:?}",
        sink.logs
    );
}

#[test]
fn passes_override_the_planner() {
    // C5 first, then C1 over the first 12 bytes: both classes found.
    let both = corrupt(C1Header, &corrupt(C5ObjectTagStripped, &golden_pdf(), 2), 0);
    let input = analysed(both);
    assert_eq!(input.classes(), [C1Header, C5ObjectTagStripped]);

    let (all, _) = run(&input);
    assert_eq!(pass(&all, C5ObjectTagStripped).outcome, PassOutcome::Fixed);

    let opts = RepairOptions {
        passes: Some(vec![C1Header]),
        ..RepairOptions::default()
    };
    let (only_c1, sink) = run_with(&input, &opts);
    assert_eq!(pass(&only_c1, C1Header).outcome, PassOutcome::Fixed);
    assert!(matches!(
        pass(&only_c1, C5ObjectTagStripped).outcome,
        PassOutcome::Skipped(_)
    ));
    assert!(
        sink.logs.iter().any(|(_, m)| m.contains("C5")),
        "{:?}",
        sink.logs
    );
    // Only Resave was planned, and it still passes its gates.
    assert_eq!(only_c1.candidates.len(), 1);
    assert!(chosen(&only_c1).verification.v0.all_pass());
}

#[test]
fn classes_without_a_pass_are_skipped_with_a_log() {
    // The golden cut at 70% ends inside the font program: C9 and C10.
    let input = analysed(corrupt(C10Truncated, &golden_pdf(), 0));
    assert_eq!(input.classes(), [C9ZlibTampered, C10Truncated]);
    let (g, sink) = run(&input);
    assert!(matches!(
        pass(&g, C9ZlibTampered).outcome,
        PassOutcome::Skipped(_)
    ));
    assert!(
        sink.logs.iter().any(|(_, m)| m.contains("C9")),
        "{:?}",
        sink.logs
    );
    // The cut font program is reachable, so it is kept and clamped.
    let c10 = pass(&g, C10Truncated);
    assert!(
        c10.actions
            .iter()
            .any(|a| a.object == (8, 0) && a.what.contains("clamped")),
        "{:?}",
        c10.actions
    );
    match &c10.outcome {
        PassOutcome::Partial(why) => assert!(why.contains("5923 of 6847"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(g.partial_reasons.iter().any(|r| r.contains("5923 of 6847")));
}

/// The golden with an unreferenced stream object written last, cut inside
/// that stream's data.
fn golden_with_cut_unreferenced_tail() -> (Vec<u8>, u32) {
    let golden = golden_pdf();
    let doc = Document::load_mem(&golden).expect("loads");
    let tail = doc.objects.keys().map(|k| k.0).max().expect("objects") + 1;
    let mut w = Writer::with_version("1.7");
    for (&(n, _), o) in &doc.objects {
        w.add(n, o.clone());
    }
    w.add(
        tail,
        Object::Stream(Stream::new(Dictionary::new(), vec![b'A'; 2000])),
    );
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    w.trailer(root, [7; 32], None);
    let full = w.finish().expect("writes");
    let header = format!("{tail} 0 obj");
    let at = full
        .windows(header.len())
        .position(|win| win == header.as_bytes())
        .expect("the tail object");
    (full[..at + 500].to_vec(), tail)
}

#[test]
fn the_c10_pass_drops_an_unreachable_cut_object() {
    let (bytes, tail) = golden_with_cut_unreferenced_tail();
    let input = analysed(bytes);
    assert_eq!(input.classes(), [C10Truncated]);
    let (g, _) = run(&input);
    let c10 = pass(&g, C10Truncated);
    assert!(
        c10.actions
            .iter()
            .any(|a| a.object == (tail, 0) && a.what.contains("dropped")),
        "{:?}",
        c10.actions
    );
    assert!(matches!(c10.outcome, PassOutcome::Partial(_)));
    let out = g.output.expect("an output");
    let out_carve = carve(&out, &|| false).unwrap();
    assert!(out_carve.objects.iter().all(|o| o.declared_id.0 != tail));
    assert!(chosen_passes(&out));
    let pages = crate::pdf::text::extract_text(&out, &Default::default()).expect("loads");
    let glyphs: usize = pages.iter().map(|p| p.glyphs.len()).sum();
    assert_eq!(glyphs, golden_glyphs());
}

/// Strict lopdf reload, the first V0 gate, as a cheap stand-in check.
fn chosen_passes(out: &[u8]) -> bool {
    Document::load_mem(out).is_ok()
}

#[test]
fn no_output_when_no_candidate_passes_v0() {
    // The object-stream golden cut at 70% keeps only its font program: no
    // catalog and no page survive.
    let input = analysed(corrupt(C10Truncated, &golden_pdf_objstm(), 0));
    let (g, _) = run(&input);
    assert_eq!(g.output, None);
    assert_eq!(g.chosen, None);
    assert_eq!(g.candidates.len(), 1);
    assert!(!g.candidates[0].chosen);
    assert!(!g.candidates[0].verification.v0.all_pass());
    assert_eq!(
        g.escalations.iter().map(|e| e.kind).collect::<Vec<_>>(),
        [EscalationKind::NoCandidatePassed]
    );
}

#[test]
fn nothing_is_built_without_a_candidate() {
    let input = analysed(golden_pdf());
    assert!(input.findings.is_empty());
    let (g, _) = run(&input);
    assert_eq!(g.output, None);
    assert!(g.candidates.is_empty() && g.passes.is_empty() && g.escalations.is_empty());
}

#[test]
fn a_run_is_deterministic() {
    for class in [C4PageTreeBroken, C10Truncated] {
        let input = analysed(corrupt(class, &golden_pdf(), 3));
        let (a, _) = run(&input);
        let (b, _) = run(&input);
        assert_eq!(a, b, "{class:?}");
    }
}

#[test]
fn cancellation_stops_the_run() {
    let input = analysed(corrupt(C4PageTreeBroken, &golden_pdf(), 3));
    let opts = RepairOptions::default();
    let plan = plan(&input.findings, &input.carve, &input.graph, &opts);
    let mut sink = Recorder {
        cancel: true,
        ..Recorder::default()
    };
    let got = generate_and_validate(
        &input.view(),
        &plan,
        &opts,
        &FontDb::empty(),
        &mut UseBest,
        &mut sink,
    );
    assert_eq!(got.err(), Some(Cancelled));
}

#[test]
fn the_run_is_recorded_in_the_report() {
    let input = analysed(corrupt(C10Truncated, &golden_pdf(), 0));
    let (g, _) = run(&input);
    let mut report = crate::engine::RepairReport {
        candidates: Vec::new(),
        ..test_report()
    };
    g.record(&mut report);
    assert_eq!(report.chosen, Some(Toolpath::Resave));
    assert_eq!(report.candidates, g.candidates);
    assert_eq!(report.passes, g.passes);
    assert_eq!(report.partial_reasons, g.partial_reasons);
    assert_eq!(report.reals_narrowed, g.notes.reals_narrowed);
}

fn test_report() -> crate::engine::RepairReport {
    let analysis = crate::engine::analyze(&golden_pdf(), &Default::default(), &mut NullProgress)
        .expect("stub analysis");
    crate::engine::RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty())
}

// ── the selection tuple ──────────────────────────────────────────────────

fn r(num: u64, den: u64) -> Ratio {
    Ratio { num, den }
}

fn key() -> SelectionKey {
    SelectionKey {
        v0: true,
        v1: Retention {
            text_ops: r(4, 4),
            path_fills: r(0, 0),
            images: r(1, 1),
            content_bytes: r(100, 100),
            blank_pages: 0,
            glyph_count: r(10, 10),
        },
        v2: Plausibility {
            unmapped_glyph: r(0, 10),
            fffd: r(0, 10),
            script_consistency: r(5, 5),
            dictionary_hit: None,
        },
        v3: Preservation {
            features: 1,
            annotations: 0,
        },
        prior: true,
        size: 1000,
    }
}

#[test]
fn the_tuple_comparison_table() {
    type Edit = fn(&mut SelectionKey);
    // (what differs, edit to `a`, edit to `b`, how `a` compares with `b`).
    let rows: [(&str, Edit, Edit, Ordering); 17] = [
        ("equal", |_| {}, |_| {}, Ordering::Equal),
        // V0 first: passing the gates beats everything after it.
        (
            "v0",
            |_| {},
            |b| {
                b.v0 = false;
                b.v1.glyph_count = r(20, 10);
                b.v3.features = 9;
                b.size = 1;
            },
            Ordering::Greater,
        ),
        // V1: blank pages first, then glyphs, text ops, images, fills, bytes.
        (
            "blank pages",
            |a| a.v1.blank_pages = 1,
            |b| b.v1.glyph_count = r(1, 10),
            Ordering::Less,
        ),
        (
            "glyphs",
            |a| a.v1.glyph_count = r(9, 10),
            |_| {},
            Ordering::Less,
        ),
        (
            "glyphs over the baseline count as all kept",
            |a| a.v1.glyph_count = r(20, 10),
            |_| {},
            Ordering::Equal,
        ),
        (
            "text ops",
            |_| {},
            |b| b.v1.text_ops = r(3, 4),
            Ordering::Greater,
        ),
        ("images", |a| a.v1.images = r(0, 1), |_| {}, Ordering::Less),
        (
            "path fills",
            |a| a.v1.path_fills = r(1, 0),
            |_| {},
            Ordering::Greater,
        ),
        (
            "content bytes",
            |a| a.v1.content_bytes = r(50, 100),
            |_| {},
            Ordering::Less,
        ),
        (
            "v1 before v2",
            |a| a.v2.fffd = r(5, 10),
            |b| b.v1.images = r(0, 1),
            Ordering::Greater,
        ),
        // V2: fewer U+FFFD, fewer unmapped, more consistent script.
        ("fffd", |a| a.v2.fffd = r(1, 10), |_| {}, Ordering::Less),
        (
            "unmapped",
            |_| {},
            |b| b.v2.unmapped_glyph = r(2, 10),
            Ordering::Greater,
        ),
        (
            "script",
            |a| a.v2.script_consistency = r(4, 5),
            |_| {},
            Ordering::Less,
        ),
        // V3: more preserved features, then more annotations.
        (
            "preservation",
            |a| a.v3.features = 2,
            |b| b.prior = false,
            Ordering::Greater,
        ),
        // V4: the prior, then the smaller file.
        (
            "prior",
            |a| a.prior = false,
            |b| b.size = 5000,
            Ordering::Less,
        ),
        ("smaller", |a| a.size = 999, |_| {}, Ordering::Greater),
        (
            "dictionary",
            |a| a.v2.dictionary_hit = Some(r(1, 2)),
            |_| {},
            Ordering::Greater,
        ),
    ];
    for (what, edit_a, edit_b, expected) in rows {
        let (mut a, mut b) = (key(), key());
        edit_a(&mut a);
        edit_b(&mut b);
        assert_eq!(compare(&a, &b), expected, "{what}");
        assert_eq!(compare(&b, &a), expected.reverse(), "{what}, reversed");
    }
}

#[test]
fn preservation_counts_what_the_catalog_and_pages_keep() {
    let golden = golden_pdf();
    let plain = preservation(&golden);
    let doc = Document::load_mem(&golden).unwrap();
    let mut w = Writer::with_version("1.7");
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    for (&(n, _), o) in &doc.objects {
        let mut o = o.clone();
        if n == root.0 {
            let d = o.as_dict_mut().unwrap();
            d.set("Outlines", Object::Dictionary(Dictionary::new()));
            d.set("Metadata", Object::Null);
        }
        w.add(n, o);
    }
    w.trailer(root, [1; 32], None);
    let richer = preservation(&w.finish().unwrap());
    assert_eq!(richer.features, plain.features + 1, "{plain:?} {richer:?}");
    assert!(richer > plain);
}

#[test]
fn the_candidate_set_follows_the_plan_and_never_the_c6_pass() {
    // A C6 file plans both toolpaths; with no C6 pass yet, Resave is built
    // and C6 is skipped, not dropped from the report.
    let input = analysed(corrupt(C6FontMapLost, &golden_pdf(), 0));
    assert!(input.classes().contains(&C6FontMapLost));
    let (g, _) = run(&input);
    assert!(matches!(
        pass(&g, C6FontMapLost).outcome,
        PassOutcome::Skipped(_)
    ));
    assert_eq!(g.candidates.len(), 1);
}
