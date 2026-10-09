//! T-13a acceptance for generate-and-validate and the structural passes:
//! the C1–C5 and C10 fixtures re-diagnose clean for their class and pass
//! V0/V1, the passes override, a TemplateAssemble with no font to
//! substitute is skipped with a log, the
//! C10 pass drops an unreachable cut object but keeps a cut catalog or
//! `/Info` written last, a file with no page gets no
//! output, the runs are deterministic, and the selection tuple is a pure
//! function with a table test.
//!
//! The C9 swap and the C6 re-link (T-13b) are tested in `content`, the C7
//! and C8 passes and template assembly (T-30) in `fonts`.

mod content;
mod fonts;

/// Test seams of generate-and-validate.
pub(super) mod seam {
    use std::cell::Cell;

    use crate::engine::Toolpath;

    thread_local! {
        static BREAK_ASSEMBLY: Cell<bool> = const { Cell::new(false) };
    }

    /// While `f` runs, every `TemplateAssemble` output is cut in half, so it
    /// fails V0.
    pub(crate) fn with_broken_assembly<T>(f: impl FnOnce() -> T) -> T {
        BREAK_ASSEMBLY.with(|b| b.set(true));
        let out = f();
        BREAK_ASSEMBLY.with(|b| b.set(false));
        out
    }

    /// `output`, cut in half when it is a `TemplateAssemble` output and the
    /// seam is set.
    pub(crate) fn broken(toolpath: Toolpath, output: Vec<u8>) -> Vec<u8> {
        if toolpath == Toolpath::TemplateAssemble && BREAK_ASSEMBLY.with(Cell::get) {
            output[..output.len() / 2].to_vec()
        } else {
            output
        }
    }
}

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
use crate::pdf::rebuild::MatchedBy;
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
    let placed: Vec<&RepairAction> = (c5.actions.iter())
        .filter(|a| a.what.starts_with("the headerless "))
        .collect();
    assert_eq!(placed.len(), 1, "{:?}", c5.actions);
    assert!(
        placed[0]
            .what
            .starts_with("the headerless dictionary at byte "),
        "{:?}",
        placed[0]
    );
}

// ── references the rebuild repaired (F-08) ───────────────────────────────

/// The golden with `edit` applied to it and its first page's id, written
/// afresh with its header damaged (C1) so it is repaired. Returns the
/// bytes and the page's id.
fn golden_edited(edit: impl FnOnce(&mut Document, ObjId)) -> (Vec<u8>, ObjId) {
    let mut doc = Document::load_mem(&golden_pdf()).expect("loads");
    let page = *doc.get_pages().values().next().expect("a page");
    assert!(!doc.objects.keys().any(|k| k.0 == 99));
    edit(&mut doc, page);
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    let mut w = Writer::with_version("1.7");
    for (&(n, _), o) in &doc.objects {
        w.add(n, o.clone());
    }
    w.trailer(root, [7; 32], None);
    let full = w.finish().expect("writes");
    (corrupt(C1Header, &full, 0), page)
}

fn dict_mut(doc: &mut Document, id: ObjId) -> &mut Dictionary {
    doc.get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .expect("a dictionary")
}

/// The golden with `/Thumb 99 0 R` on its first page, a reference to an
/// object the file never had (see [`golden_edited`]).
fn golden_with_dangling_thumb() -> (Vec<u8>, ObjId) {
    golden_edited(|doc, page| dict_mut(doc, page).set("Thumb", Object::Reference((99, 0))))
}

/// The first page of `out`, and its dictionary.
fn first_out_page(out: &[u8]) -> (Document, ObjId) {
    let doc = Document::load_mem(out).expect("loads");
    let page = *doc.get_pages().values().next().expect("a page");
    (doc, page)
}

/// The one action, across every pass, that mentions `99 0 R`.
fn the_99_action(g: &Generated) -> &RepairAction {
    let found: Vec<&RepairAction> = (g.passes.iter())
        .flat_map(|p| &p.actions)
        .filter(|a| a.what.contains("99 0 R"))
        .collect();
    assert_eq!(found.len(), 1, "{:?}", g.passes);
    found[0]
}

#[test]
fn a_dangling_reference_written_as_null_is_listed_in_the_report() {
    let (bytes, page) = golden_with_dangling_thumb();
    let input = analysed(bytes);
    assert_eq!(input.classes(), [C1Header]);
    let (g, _) = run(&input);
    let out = g.output.as_deref().expect("an output");

    // No C5 or C10 pass ran, so the first pass that ran carries it.
    let c1 = pass(&g, C1Header);
    let nulled: Vec<&RepairAction> = (c1.actions.iter())
        .filter(|a| a.what.contains("99 0 R"))
        .collect();
    assert_eq!(nulled.len(), 1, "{:?}", c1.actions);
    assert_eq!(nulled[0].object, page);
    assert_eq!(nulled[0].grade, None);
    assert_eq!(
        nulled[0].what,
        "/Thumb names 99 0 R, which no carved object matches: written as null"
    );
    assert_eq!(
        (g.passes.iter())
            .flat_map(|p| &p.actions)
            .filter(|a| a.what.contains("99 0 R"))
            .count(),
        1,
        "listed once"
    );

    // As before: the output's reference is null.
    let doc = Document::load_mem(out).expect("loads");
    let out_page = *doc.get_pages().values().next().expect("a page");
    let thumb = doc
        .get_dictionary(out_page)
        .expect("the page")
        .get(b"Thumb")
        .expect("kept");
    assert_eq!(thumb, &Object::Null);
}

#[test]
fn a_dangling_page_resources_taken_from_the_ancestor_is_reported_so() {
    // The page's own /Resources moves to its parent and the page names a
    // missing object instead: the output page carries the parent's.
    let (bytes, page) = golden_edited(|doc, page| {
        let own = dict_mut(doc, page)
            .get(b"Resources")
            .expect("the page has resources")
            .clone();
        let parent = (dict_mut(doc, page).get(b"Parent"))
            .and_then(Object::as_reference)
            .expect("a parent");
        dict_mut(doc, parent).set("Resources", own);
        dict_mut(doc, page).set("Resources", Object::Reference((99, 0)));
    });
    let input = analysed(bytes);
    assert_eq!(input.classes(), [C1Header]);
    let (g, _) = run(&input);
    let action = the_99_action(&g);
    assert_eq!(action.object, page);
    assert_eq!(
        action.what,
        "/Resources names 99 0 R, which no carved object matches: the page takes its \
         nearest ancestor's /Resources instead"
    );

    let (doc, out_page) = first_out_page(g.output.as_deref().expect("an output"));
    let resources = (doc.get_dictionary(out_page).expect("the page"))
        .get(b"Resources")
        .expect("pinned on the page");
    assert_ne!(resources, &Object::Null, "not written as null");
    let resources = match resources {
        Object::Reference(id) => doc.get_dictionary(*id).expect("a dictionary"),
        other => other.as_dict().expect("a dictionary"),
    };
    assert!(
        resources.has(b"Font"),
        "the parent's dictionary: {resources:?}"
    );
}

#[test]
fn a_dangling_page_attribute_no_ancestor_has_is_reported_removed() {
    let (bytes, page) = golden_edited(|doc, page| {
        dict_mut(doc, page).set("CropBox", Object::Reference((99, 0)));
    });
    let input = analysed(bytes);
    assert_eq!(input.classes(), [C1Header]);
    let (g, _) = run(&input);
    let action = the_99_action(&g);
    assert_eq!(action.object, page);
    assert_eq!(
        action.what,
        "/CropBox names 99 0 R, which no carved object matches: /CropBox is removed from \
         the page, as no ancestor has one"
    );

    let (doc, out_page) = first_out_page(g.output.as_deref().expect("an output"));
    let d = doc.get_dictionary(out_page).expect("the page");
    assert!(!d.has(b"CropBox"), "removed, not null: {d:?}");
}

#[test]
fn a_dangling_page_resources_no_ancestor_has_is_reported_removed() {
    let (bytes, page) = golden_edited(|doc, page| {
        dict_mut(doc, page).set("Resources", Object::Reference((99, 0)));
    });
    let input = analysed(bytes);
    assert_eq!(input.classes(), [C1Header]);
    let (g, _) = run(&input);
    let action = the_99_action(&g);
    assert_eq!(action.object, page);
    assert_eq!(
        action.what,
        "/Resources names 99 0 R, which no carved object matches: /Resources is removed \
         from the page, as no ancestor has one"
    );

    let (doc, out_page) = first_out_page(g.output.as_deref().expect("an output"));
    let d = doc.get_dictionary(out_page).expect("the page");
    assert!(!d.has(b"Resources"), "removed, not null: {d:?}");
}

#[test]
fn a_dangling_page_mediabox_is_reported_with_the_box_the_chain_gave() {
    let (bytes, _) = golden_edited(|doc, page| {
        dict_mut(doc, page).set("MediaBox", Object::Reference((99, 0)));
    });
    let input = analysed(bytes);
    let (g, _) = run(&input);
    let what = &the_99_action(&g).what;
    assert!(what.starts_with("/MediaBox names 99 0 R"), "{what}");
    assert!(!what.contains("null"), "{what}");

    let (doc, out_page) = first_out_page(g.output.as_deref().expect("an output"));
    let d = doc.get_dictionary(out_page).expect("the page");
    assert!(
        matches!(d.get(b"MediaBox"), Ok(Object::Array(a)) if a.len() == 4),
        "{d:?}"
    );
}

#[test]
fn every_kept_referrer_of_a_match_or_of_a_dropped_object_is_listed() {
    // Pages 1 and 2 both name 5 1 R, which the generation fallback matches
    // to 5 0; page 2 also names 4 0 R. A pass then drops page 1 (the
    // referrer the match was made for) and object 4.
    let bytes = b"%PDF-1.7\n\
        1 0 obj\n<< /Type /Page /Contents 5 1 R >>\nendobj\n\
        2 0 obj\n<< /Type /Page /Contents 5 1 R /Thumb 4 0 R >>\nendobj\n\
        4 0 obj\n<< /Kind /Thumb >>\nendobj\n\
        5 0 obj\n<< /Length 2 >>\nstream\nBT\nendstream\nendobj\n%%EOF\n";
    let input = analysed(bytes.to_vec());
    let plan = plan_ids(&input.carve, &input.graph);
    let rec = plan.reconciled();
    assert_eq!(rec.len(), 1, "{rec:?}");
    assert_eq!(rec[0].from, (1, 0));
    let tree = rebuild_page_tree(
        &input.carve,
        &input.graph,
        &plan,
        RepairOptions::default().default_page_size,
    );
    let mut doc = RebuildDoc::new(plan.clone());
    doc.forget(plan.target((1, 0)).expect("page 1"));
    doc.forget(plan.target((4, 0)).expect("object 4"));

    let actions = reference_actions(&input.carve, &input.graph, &plan, &tree, &doc);
    let lines: Vec<(ObjId, &str)> = (actions.iter())
        .map(|a| (a.object, a.what.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (
                (2, 0),
                "/Contents names 5 1 R, which no carved object carries: re-linked to output \
                 object 5, matched by generation: 5 0 obj, the same number"
            ),
            (
                (2, 0),
                "/Thumb names 4 0 R, which a repair pass dropped: written as null"
            ),
        ]
    );
}

#[test]
fn a_reference_matched_by_position_is_listed_with_how_it_was_matched() {
    let input = analysed(corrupt(C5ObjectTagStripped, &golden_pdf(), 2));
    let rec = plan_ids(&input.carve, &input.graph).reconciled().to_vec();
    assert_eq!(rec.len(), 1, "{rec:?}");
    let MatchedBy::Position { delta } = rec[0].by else {
        panic!("{rec:?}");
    };
    let (g, _) = run(&input);
    let c5 = pass(&g, C5ObjectTagStripped);
    let (n, gen_) = rec[0].missing;
    let relinked: Vec<&RepairAction> = (c5.actions.iter())
        .filter(|a| a.what.contains(&format!("names {n} {gen_} R")))
        .collect();
    // One line per reference to it: both pages name the font.
    let referrers: Vec<ObjId> = rec[0].refs.iter().map(|(from, _)| *from).collect();
    assert_eq!(referrers.len(), 2, "{rec:?}");
    assert_eq!(referrers[0], rec[0].from);
    let objects: Vec<ObjId> = relinked.iter().map(|a| a.object).collect();
    assert_eq!(objects, referrers, "{:?}", c5.actions);
    let (fnum, fgen) = rec[0].from;
    for a in relinked {
        let what = &a.what;
        assert!(what.contains("re-linked to output object "), "{what}");
        assert!(what.contains("matched by position"), "{what}");
        assert!(
            what.contains(&format!("{delta} bytes from {fnum} {fgen} obj")),
            "{what}"
        );
    }
}

#[test]
fn a_partial_location_carries_its_pass_class() {
    let page = Location::Page {
        index: 0,
        obj: None,
    };
    let finding = |class: CorruptionClass| Finding {
        id: format!("{}-001", class.code()),
        class: FindingKind::Corruption(class),
        severity: crate::pdf::model::Severity::Error,
        location: page,
        summary: class.label().to_owned(),
        evidence: Vec::new(),
        repair: Repairability::Auto,
    };
    let schedule = vec![
        (
            C9ZlibTampered,
            Scheduled::Run(
                pass_for(C9ZlibTampered).unwrap(),
                vec![finding(C9ZlibTampered)],
            ),
        ),
        (
            C6FontMapLost,
            Scheduled::Run(
                pass_for(C6FontMapLost).unwrap(),
                vec![finding(C6FontMapLost)],
            ),
        ),
    ];
    let reports = [
        PassReport {
            class: C9ZlibTampered,
            outcome: PassOutcome::Partial("unrecoverable stream".into()),
            actions: Vec::new(),
        },
        PassReport {
            class: C6FontMapLost,
            outcome: PassOutcome::Fixed,
            actions: Vec::new(),
        },
    ];
    let (targeted, partial) = targets(
        &reports,
        &schedule,
        &PassNotes::default(),
        &IdRemap::default(),
    );
    assert_eq!(targeted, [C9ZlibTampered, C6FontMapLost]);
    assert_eq!(partial, [(C9ZlibTampered, page)]);
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
        sink.logs
            .iter()
            .any(|(_, m)| m.contains("C5") && m.contains("re-emits the file regardless")),
        "{:?}",
        sink.logs
    );
    // Only Resave was planned, and it still passes its gates.
    assert_eq!(only_c1.candidates.len(), 1);
    assert!(chosen(&only_c1).verification.v0.all_pass());
}

#[test]
fn the_c10_pass_clamps_a_reachable_cut_stream() {
    // The golden cut at 70% ends inside the font program: C9 and C10.
    let input = analysed(corrupt(C10Truncated, &golden_pdf(), 0));
    assert_eq!(input.classes(), [C9ZlibTampered, C10Truncated]);
    let (g, _) = run(&input);
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

/// The golden's objects with one more written last, as `tail`: `last` makes
/// it from the root id and `tail`, and `trailer` gives the trailer's
/// `(root, info)`. Cut at `cut` bytes into the tail object.
fn golden_with_tail(
    last: impl FnOnce(ObjId, u32) -> Object,
    trailer: impl FnOnce(ObjId, u32) -> (ObjId, Option<ObjId>),
    skip_root: bool,
    cut: impl FnOnce(&[u8]) -> usize,
) -> (Vec<u8>, u32) {
    let doc = Document::load_mem(&golden_pdf()).expect("loads");
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    let tail = doc.objects.keys().map(|k| k.0).max().expect("objects") + 1;
    let mut w = Writer::with_version("1.7");
    for (&(n, _), o) in &doc.objects {
        if !(skip_root && n == root.0) {
            w.add(n, o.clone());
        }
    }
    w.add(tail, last(root, tail));
    let (r, info) = trailer(root, tail);
    w.trailer(r, [7; 32], info);
    let full = w.finish().expect("writes");
    let header = format!("{tail} 0 obj");
    let at = full
        .windows(header.len())
        .position(|win| win == header.as_bytes())
        .expect("the tail object");
    let end = at + cut(&full[at..]);
    (full[..end].to_vec(), tail)
}

#[test]
fn the_c10_pass_keeps_a_cut_catalog_written_last() {
    // The catalog moved to the last object, cut right after `/Catalog`: its
    // `/Pages` is gone, so only the rebuild's fallback finds it.
    let golden = Document::load_mem(&golden_pdf()).expect("loads");
    let (bytes, tail) = golden_with_tail(
        |root, _| {
            let old = golden.get_dictionary(root).expect("the catalog");
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"Catalog".to_vec()));
            for (k, v) in old.iter().filter(|(k, _)| k.as_slice() != b"Type") {
                d.set(k.clone(), v.clone());
            }
            Object::Dictionary(d)
        },
        |_, tail| ((tail, 0), None),
        true,
        |obj| {
            let name = b"/Catalog";
            obj.windows(name.len()).position(|w| w == name).unwrap() + name.len()
        },
    );
    let input = analysed(bytes);
    assert!(
        input.classes().contains(&C10Truncated),
        "{:?}",
        input.findings
    );
    let (g, _) = run(&input);
    let c10 = pass(&g, C10Truncated);
    assert!(
        !c10.actions
            .iter()
            .any(|a| a.object == (tail, 0) && a.what.contains("dropped")),
        "{:?}",
        c10.actions
    );
    let out = g.output.as_deref().expect("an output");
    assert!(chosen(&g).verification.v0.all_pass());
    let pages = crate::pdf::text::extract_text(out, &Default::default()).expect("loads");
    assert_eq!(pages.len(), 2);
}

#[test]
fn the_c10_pass_keeps_a_cut_info_dictionary_written_last() {
    // A trailer `/Info` written last, cut after its `>>`: only `endobj` is
    // missing.
    let (bytes, tail) = golden_with_tail(
        |_, _| {
            let mut d = Dictionary::new();
            d.set("Title", Object::string_literal("Golden"));
            Object::Dictionary(d)
        },
        |root, tail| (root, Some((tail, 0))),
        false,
        |obj| obj.windows(2).position(|w| w == b">>").unwrap() + 2,
    );
    let input = analysed(bytes);
    assert!(
        input.classes().contains(&C10Truncated),
        "{:?}",
        input.findings
    );
    let (g, _) = run(&input);
    let c10 = pass(&g, C10Truncated);
    assert!(
        !c10.actions
            .iter()
            .any(|a| a.object == (tail, 0) && a.what.contains("dropped")),
        "{:?}",
        c10.actions
    );
    let out = g.output.as_deref().expect("an output");
    assert!(chosen(&g).verification.v0.all_pass());
    assert_eq!(preservation(out).features, 1, "{:?}", preservation(out));
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
    // The passes tried are reported; no partial repair is, as nothing was
    // written.
    assert!(matches!(
        pass(&g, C10Truncated).outcome,
        PassOutcome::Partial(_)
    ));
    assert!(g.partial_reasons.is_empty());
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
    // A C6 file plans Resave only (T-30's row), and the C6 pass runs inside
    // it.
    let input = analysed(corrupt(C6FontMapLost, &golden_pdf(), 0));
    assert!(input.classes().contains(&C6FontMapLost));
    let opts = RepairOptions::default();
    let planned = plan(&input.findings, &input.carve, &input.graph, &opts);
    assert_eq!(planned.candidates, [Toolpath::Resave]);
    let (g, _) = run(&input);
    assert!(!matches!(
        pass(&g, C6FontMapLost).outcome,
        PassOutcome::Skipped(_)
    ));
    assert_eq!(g.candidates.len(), 1);
    assert_eq!(g.candidates[0].toolpath, Toolpath::Resave);
}
