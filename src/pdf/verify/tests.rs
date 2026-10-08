//! T-12b acceptance: the golden passes everything, a dropped page and an
//! emptied page are caught by V1, the blank-case fixture, an unrepaired C6
//! file fails V2, a C1 input that hayro cannot load gets a carve proxy, the
//! raster tripwire, the committed record hash, and a re-diagnosis that spends
//! nothing (D-074).

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use lopdf::{Dictionary, Document, Object};
use sha2::{Digest, Sha256};

use super::*;
use crate::engine::{NullProgress, PageSize};
use crate::pdf::emit::{EmitCtx, EmitNotes, emit_resave};
use crate::pdf::fixtures::{GOLDEN_TEXT, corrupt, golden_pdf};
use crate::pdf::model::{ByteSpan, Evidence, FindingKind, Repairability, Severity};
use crate::pdf::rebuild::{plan_ids, rebuild_page_tree};
use crate::pdf::text::tests::{BLANK_CASES, blank_cases_pdf};
use crate::pdf::text::{FontKey, GlyphItem};
use crate::pdf::write::Writer;

use CorruptionClass::{
    C1Header, C3TrailerDamaged, C4PageTreeBroken, C6FontMapLost, C9ZlibTampered,
};

fn carved(bytes: &[u8]) -> CarveReport {
    carve(bytes, &|| false).expect("carve without a cancel")
}

/// Carve, salvage under `budget`, renumber, rebuild the page tree, emit:
/// the Resave candidate of `input`.
fn resave_with(input: &[u8], budget: &SalvageBudget) -> Vec<u8> {
    let carve = carved(input);
    let graph = ObjectGraph::from_carve(&carve);
    let salvage = salvage_all(
        &CarveSource::new(&carve, input),
        budget,
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .expect("salvage without a cancel");
    emit_with(input, &carve, &graph, &salvage)
}

fn resave(input: &[u8]) -> Vec<u8> {
    resave_with(input, &SalvageBudget::default())
}

/// The Resave with `salvage` as the C9 outcomes: an empty index is a run
/// whose passes left C9 out (every stream copied raw).
fn emit_with(
    input: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    salvage: &SalvageIndex,
) -> Vec<u8> {
    let remap = plan_ids(carve, graph);
    let tree = rebuild_page_tree(carve, graph, &remap, PageSize::A4);
    let mut sink = NullProgress;
    let mut ctx = EmitCtx {
        bytes: input,
        input_sha256: Sha256::digest(input).into(),
        salvage,
        sink: &mut sink,
        notes: EmitNotes::default(),
    };
    emit_resave(carve, graph, &remap, &tree, &mut ctx).expect("emit")
}

/// `verify` of `output` against `input`, as the planner calls it.
fn check(output: &[u8], input: &[u8], targeted: &[CorruptionClass]) -> Verification {
    let carve = carved(input);
    let base = baseline(input, &carve);
    verify(output, &carve, &base, targeted, &[])
}

fn r(num: u64, den: u64) -> Ratio {
    Ratio { num, den }
}

fn all(value: u64) -> Ratio {
    r(value, value)
}

/// `bytes` loaded by lopdf, edited, and written again through the writer.
fn rewritten(bytes: &[u8], edit: impl FnOnce(&mut BTreeMap<u32, Object>)) -> Vec<u8> {
    let doc = Document::load_mem(bytes).expect("lopdf loads the fixture");
    let mut objects: BTreeMap<u32, Object> = doc
        .objects
        .iter()
        .map(|(&(n, _), o)| (n, o.clone()))
        .collect();
    edit(&mut objects);
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("/Root");
    let mut w = Writer::with_version("1.7");
    for (n, o) in objects {
        w.add(n, o);
    }
    w.trailer(root, [3; 32], None);
    w.finish().expect("rewrite")
}

fn dict_mut(objects: &mut BTreeMap<u32, Object>, n: u32) -> &mut Dictionary {
    objects
        .get_mut(&n)
        .and_then(|o| o.as_dict_mut().ok())
        .expect("a dictionary")
}

/// The number of the `/Pages` node and of each page, in `/Kids` order.
fn page_numbers(objects: &BTreeMap<u32, Object>) -> (u32, Vec<u32>) {
    let (&pages, node) = objects
        .iter()
        .find(|(_, o)| {
            o.as_dict()
                .ok()
                .and_then(|d| d.get(b"Type").ok())
                .is_some_and(|t| t.as_name().ok() == Some(b"Pages".as_slice()))
        })
        .expect("a /Pages node");
    let kids = node
        .as_dict()
        .unwrap()
        .get(b"Kids")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_reference().unwrap().0)
        .collect();
    (pages, kids)
}

// ── the golden ───────────────────────────────────────────────────────────

/// Glyphs and glyph runs on each golden page.
fn golden_counts() -> (u64, u64) {
    let glyphs = GOLDEN_TEXT
        .iter()
        .flat_map(|p| p.iter())
        .map(|l| l.chars().count() as u64)
        .sum();
    let runs = GOLDEN_TEXT.iter().map(|p| p.len() as u64).sum();
    (glyphs, runs)
}

#[test]
fn the_golden_passes_everything() {
    let golden = golden_pdf();
    let v = check(&golden, &golden, &CorruptionClass::ALL);
    let (glyphs, runs) = golden_counts();
    assert_eq!(v.baseline, BaselineKind::Extracted);
    assert!(v.v0.all_pass(), "{:?}", v.v0);
    assert_eq!(
        v.v1,
        Retention {
            text_ops: all(runs),
            path_fills: r(0, 0),
            images: all(1),
            content_bytes: v.v1.content_bytes,
            blank_pages: 0,
            glyph_count: all(glyphs),
        }
    );
    assert_eq!(v.v1.content_bytes.num, v.v1.content_bytes.den);
    assert!(v.v1.content_bytes.den > 0);
    let letters = v.v2.script_consistency;
    assert_eq!(
        v.v2,
        Plausibility {
            unmapped_glyph: r(0, glyphs),
            fffd: r(0, glyphs),
            script_consistency: letters,
            dictionary_hit: None,
        }
    );
    assert!(letters.num > 0 && letters.num == letters.den, "{letters:?}");
}

#[test]
fn the_resaved_golden_passes_everything_too() {
    let golden = golden_pdf();
    let out = resave(&golden);
    assert_ne!(out, golden);
    assert_eq!(
        check(&out, &golden, &CorruptionClass::ALL),
        check(&golden, &golden, &CorruptionClass::ALL)
    );
}

#[test]
fn a_structural_repair_passes_v0_and_keeps_everything() {
    for class in [C3TrailerDamaged, C4PageTreeBroken] {
        let input = corrupt(class, &golden_pdf(), 3);
        let v = check(&resave(&input), &input, &[class]);
        assert!(v.v0.all_pass(), "{class:?}: {:?}", v.v0);
        assert_eq!(v.v1.blank_pages, 0);
        assert_eq!(v.v1.glyph_count, all(golden_counts().0), "{class:?}");
    }
}

#[test]
fn a_repair_can_retain_more_than_hayro_found_in_the_input() {
    // Seed 7 cuts the page tree so that hayro's scan of the input finds
    // page 2 only; the carver finds both pages and the Resave has both.
    let input = corrupt(C4PageTreeBroken, &golden_pdf(), 7);
    let v = check(&resave(&input), &input, &[C4PageTreeBroken]);
    assert!(v.v0.all_pass(), "{:?}", v.v0);
    let page_two = GOLDEN_TEXT[1]
        .iter()
        .map(|l| l.chars().count() as u64)
        .sum();
    assert_eq!(v.v1.glyph_count, r(golden_counts().0, page_two));
    assert!(v.v1.glyph_count > all(1));
    // The input showed no image; the output draws the golden's one.
    assert_eq!(v.v1.images, r(1, 0));
}

// ── V1 ───────────────────────────────────────────────────────────────────

#[test]
fn a_page_dropping_emit_fails_v1() {
    let golden = golden_pdf();
    let dropped = rewritten(&golden, |objects| {
        let (pages, kids) = page_numbers(objects);
        let d = dict_mut(objects, pages);
        d.set("Kids", Object::Array(vec![Object::Reference((kids[0], 0))]));
        d.set("Count", Object::Integer(1));
    });
    let whole = check(&golden, &golden, &[]);
    let v = check(&dropped, &golden, &[]);
    // The structure is sound, but a page is gone.
    assert!(v.v0.lopdf_reload && v.v0.hayro_render_all_pages && v.v0.root_and_pages_present);
    assert!(!v.v0.page_count_ok);
    let (glyphs, runs) = golden_counts();
    let page_one = GOLDEN_TEXT[0]
        .iter()
        .map(|l| l.chars().count() as u64)
        .sum();
    assert_eq!(v.v1.glyph_count, r(page_one, glyphs));
    assert_eq!(v.v1.text_ops, r(GOLDEN_TEXT[0].len() as u64, runs));
    assert!(v.v1.glyph_count < whole.v1.glyph_count);
    assert!(v.v1.text_ops < whole.v1.text_ops);
    assert!(v.v1.content_bytes < whole.v1.content_bytes);
}

#[test]
fn an_emptied_page_raises_blank_pages_by_one() {
    let golden = golden_pdf();
    let emptied = rewritten(&golden, |objects| {
        let (_, kids) = page_numbers(objects);
        let contents = dict_mut(objects, kids[1])
            .get(b"Contents")
            .and_then(Object::as_reference)
            .expect("page 2 /Contents")
            .0;
        objects.insert(
            contents,
            Object::Stream(lopdf::Stream::new(Dictionary::new(), Vec::new())),
        );
    });
    let before = check(&golden, &golden, &[]);
    let v = check(&emptied, &golden, &[]);
    assert_eq!(before.v1.blank_pages, 0);
    assert_eq!(v.v1.blank_pages, 1);
    assert!(v.v0.all_pass(), "an empty page is still a page: {:?}", v.v0);
    assert!(v.v1.content_bytes < before.v1.content_bytes);
}

#[test]
fn a_page_blank_in_the_input_is_not_a_loss() {
    let golden = golden_pdf();
    let emptied = rewritten(&golden, |objects| {
        let (_, kids) = page_numbers(objects);
        let contents = dict_mut(objects, kids[1])
            .get(b"Contents")
            .and_then(Object::as_reference)
            .unwrap()
            .0;
        objects.insert(
            contents,
            Object::Stream(lopdf::Stream::new(Dictionary::new(), Vec::new())),
        );
    });
    // The emptied file is now the input: its blank page was blank before.
    assert_eq!(check(&resave(&emptied), &emptied, &[]).v1.blank_pages, 0);
}

/// The pages of the blank-case fixture that D-059 calls blank: no paint at
/// all (empty, render mode 3, a path painted with `n`) or nothing inked
/// (spaces only).
const BLANK_PAGES: [usize; 4] = [0, 2, 5, 9];

#[test]
fn the_blank_case_fixture_scores_exactly_the_unpainted_pages() {
    let pdf = blank_cases_pdf();
    let pages = extract_text(&pdf, &ExtractOptions::default()).expect("extracts");
    assert_eq!(pages.len(), BLANK_CASES.len());
    let blank: Vec<usize> = (0..pages.len())
        .filter(|&i| is_blank(&pages[i].paint))
        .collect();
    assert_eq!(blank, BLANK_PAGES);
    // The table's `paint_ops() == 0` rows are among them.
    for i in [0, 2, 9] {
        assert_eq!(pages[i].paint.paint_ops(), 0);
    }

    // Against a baseline that knows no blank page, verify counts all four.
    let proxy = Baseline::CarveProxy {
        show_ops: 5,
        string_bytes: 26,
    };
    let carve = carved(&pdf);
    let v = verify(&pdf, &carve, &proxy, &[], &[]);
    assert_eq!(v.baseline, BaselineKind::CarveProxy);
    assert_eq!(v.v1.blank_pages, BLANK_PAGES.len() as u32);
    // Against itself, nothing was lost.
    let own = baseline(&pdf, &carve);
    assert_eq!(verify(&pdf, &carve, &own, &[], &[]).v1.blank_pages, 0);
}

// ── V2 ───────────────────────────────────────────────────────────────────

#[test]
fn an_unrepaired_c6_file_fails_v2() {
    let golden = check(&golden_pdf(), &golden_pdf(), &[]);
    let input = corrupt(C6FontMapLost, &golden_pdf(), 7);
    let v = check(&resave(&input), &input, &[C6FontMapLost]);
    // hayro's Helvetica fallback reads each two-byte code as two glyphs and
    // maps few of them (FR-04 §4).
    assert!(v.v0.lopdf_reload && v.v0.hayro_render_all_pages);
    assert_eq!(golden.v2.unmapped_glyph, r(0, golden_counts().0));
    assert!(v.v2.unmapped_glyph >= r(1, 3), "{:?}", v.v2.unmapped_glyph);
    assert!(v.v2.unmapped_glyph > golden.v2.unmapped_glyph);
    assert!(v.v2.script_consistency <= golden.v2.script_consistency);
    // The baseline is the damaged input's own extraction, which is garbage
    // too: V1 sees no loss, V2 sees the damage.
    assert_eq!(v.v1.glyph_count.num, v.v1.glyph_count.den);
    assert!(v.v1.glyph_count.den > golden_counts().0);
}

#[test]
fn plausibility_counts_unmapped_replacement_and_mixed_script_glyphs() {
    let glyph = |t: Option<&str>| GlyphItem {
        text: t.map(str::to_owned),
        x: 0.0,
        y: 0.0,
        size: 1.0,
        advance: None,
        font: FontKey::UNKEYED,
        invisible: false,
        type3: false,
        mcid: None,
    };
    // "ab" (Latin), " ", "аб" (Cyrillic), " ", "aб" (mixed), " ", "12",
    // an unmapped glyph, and a U+FFFD.
    let text = [
        Some("a"),
        Some("b"),
        Some(" "),
        Some("а"),
        Some("б"),
        Some(" "),
        Some("a"),
        Some("б"),
        Some(" "),
        Some("1"),
        Some("2"),
        None,
        Some("\u{FFFD}"),
    ];
    let page = PageText {
        index: 0,
        width: 1.0,
        height: 1.0,
        glyphs: text.iter().map(|t| glyph(*t)).collect(),
        unmapped: 1,
        paint: PaintCounts::default(),
        warnings: Vec::new(),
    };
    let p = plausibility(&[page]);
    assert_eq!(p.unmapped_glyph, r(1, 13));
    assert_eq!(p.fffd, r(1, 13));
    // Three tokens carry letters; the mixed one is not consistent.
    assert_eq!(p.script_consistency, r(2, 3));
    assert_eq!(p.dictionary_hit, None);
}

// ── baselines ────────────────────────────────────────────────────────────

/// A C1 input hayro cannot load: the header bytes are gone and so is every
/// way to the catalog (no trailer, and its `/Type /Catalog` is renamed), the
/// one hard failure FR-04 §5 observed.
fn c1_unloadable() -> Vec<u8> {
    let mut bytes = corrupt(C1Header, &golden_pdf(), 7);
    let trailer = bytes
        .windows(7)
        .rposition(|w| w == b"trailer")
        .expect("trailer");
    let len = bytes.len();
    bytes[trailer..len].fill(b' ');
    let at = bytes
        .windows(8)
        .position(|w| w == b"/Catalog")
        .expect("/Catalog");
    bytes[at..at + 8].copy_from_slice(b"/Katalog");
    bytes
}

#[test]
fn a_c1_input_hayro_cannot_load_gets_a_carve_proxy() {
    let input = c1_unloadable();
    assert!(extract_text(&input, &ExtractOptions::default()).is_err());
    let carve = carved(&input);
    let base = baseline(&input, &carve);
    let (glyphs, runs) = golden_counts();
    // Every `Tj` of the golden, and two bytes per glyph (Identity-H).
    assert_eq!(
        base,
        Baseline::CarveProxy {
            show_ops: runs,
            string_bytes: 2 * glyphs,
        }
    );
    assert_eq!(base.kind(), BaselineKind::CarveProxy);

    // Verified against the golden itself (what a perfect repair would give):
    // the proxy kind is recorded, the show operators all come back, and the
    // glyph count is glyphs over string bytes.
    let v = verify(&golden_pdf(), &carve, &base, &[C1Header], &[]);
    assert_eq!(v.baseline, BaselineKind::CarveProxy);
    assert_eq!(v.v1.text_ops, all(runs));
    assert_eq!(v.v1.glyph_count, r(glyphs, 2 * glyphs));
    assert_eq!(v.v1.images, all(1));
}

#[test]
fn a_loading_input_gets_an_extracted_baseline() {
    let golden = golden_pdf();
    let base = baseline(&golden, &carved(&golden));
    assert_eq!(base.kind(), BaselineKind::Extracted);
    let Baseline::Extracted(pages) = base else {
        unreachable!()
    };
    assert_eq!(pages.len(), GOLDEN_TEXT.len());
}

// ── V0 ───────────────────────────────────────────────────────────────────

#[test]
fn garbage_fails_every_gate() {
    let golden = golden_pdf();
    let v = check(b"%PDF-1.7\nnot a pdf", &golden, &[]);
    assert!(!v.v0.lopdf_reload);
    assert!(!v.v0.hayro_render_all_pages);
    assert!(!v.v0.page_count_ok);
    assert!(!v.v0.root_and_pages_present);
    assert!(!v.v0.all_pass());
    assert_eq!(v.v1.glyph_count, r(0, golden_counts().0));
}

#[test]
fn an_unrepaired_structural_class_fails_rediagnosis() {
    // C3 written back as it came in is still C3.
    let input = corrupt(C3TrailerDamaged, &golden_pdf(), 7);
    let v = check(&input, &input, &[C3TrailerDamaged]);
    assert!(!v.v0.rediagnose_clean);
    assert!(!v.v0.lopdf_reload);
    // Not targeted: not counted against it.
    assert!(
        check(&input, &input, &[C4PageTreeBroken])
            .v0
            .rediagnose_clean
    );
}

fn finding(class: CorruptionClass, location: Location) -> Finding {
    Finding {
        id: format!("{}-001", class.code()),
        class: FindingKind::Corruption(class),
        severity: Severity::Error,
        location,
        summary: class.label().to_owned(),
        evidence: vec![Evidence::Text("test".into())],
        repair: Repairability::Auto,
    }
}

#[test]
fn clean_means_no_targeted_finding_outside_the_partial_locations() {
    let at = |n| Location::Object {
        id: (n, 0),
        span: Some(ByteSpan { start: 1, end: 2 }),
    };
    let page = |index| Location::Page { index, obj: None };
    let found = [
        finding(C9ZlibTampered, at(4)),
        finding(C6FontMapLost, page(1)),
    ];

    assert!(!clean_for(&found, &[C9ZlibTampered], &[]));
    // A pass reported object 4 Partial: its finding may stay.
    let partial_4 = Location::Object {
        id: (4, 0),
        span: None,
    };
    assert!(clean_for(&found, &[C9ZlibTampered], &[partial_4]));
    assert!(!clean_for(&found, &[C9ZlibTampered], &[at(5)]));
    // Untargeted classes do not count; targeted ones elsewhere do.
    assert!(!clean_for(
        &found,
        &[C9ZlibTampered, C6FontMapLost],
        &[partial_4]
    ));
    assert!(clean_for(
        &found,
        &[C9ZlibTampered, C6FontMapLost],
        &[partial_4, page(1)]
    ));
    assert!(clean_for(&found, &[C4PageTreeBroken], &[]));
    assert!(clean_for(&[], &CorruptionClass::ALL, &[]));
}

// ── re-diagnosis spends nothing (D-074) ──────────────────────────────────

#[test]
fn a_c9_c4_file_repaired_for_c4_only_is_rediagnosed_without_a_search() {
    let c9 = corrupt(C9ZlibTampered, &golden_pdf(), 7);
    let input = corrupt(C4PageTreeBroken, &c9, 7);
    let carve = carved(&input);
    let graph = ObjectGraph::from_carve(&carve);
    // `passes = Some([C4])`: no C9 pass, so every stream goes out as carved.
    let out = emit_with(&input, &carve, &graph, &SalvageIndex::default());

    // The output still has damaged Flate streams that a budget would search.
    let out_carve = carved(&out);
    let searched = salvage_all(
        &CarveSource::new(&out_carve, &out),
        &SalvageBudget::default(),
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .expect("salvage");
    assert!(
        searched.work_total > 0,
        "the fixture has a stream to search"
    );

    let base = baseline(&input, &carve);
    let (v, spent) = verify_spending(&out, &carve, &base, &[C4PageTreeBroken], &[]);
    assert_eq!(spent, 0, "zero candidate inflates");
    assert!(v.v0.rediagnose_clean);
    assert!(v.v0.root_and_pages_present && v.v0.page_count_ok);
}

/// One page whose one content stream is Flate data nothing can inflate.
fn with_unrecoverable_stream() -> Vec<u8> {
    let dead: Vec<u8> = (0u8..64).map(|i| 0xff - (i % 3)).collect();
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
    ];
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    offsets.push(out.len());
    out.extend_from_slice(
        format!(
            "4 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            dead.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&dead);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

#[test]
fn an_unrecoverable_stream_reported_partial_passes_v0() {
    let input = with_unrecoverable_stream();
    let carve = carved(&input);
    let salvage = salvage_all(
        &CarveSource::new(&carve, &input),
        &SalvageBudget::default(),
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .unwrap();
    assert_eq!(
        salvage.by_obj[&(4, 0)].salvage,
        crate::pdf::streams::salvage::Salvage::Unrecoverable
    );
    let out = resave(&input);
    let base = baseline(&input, &carve);
    // The C9 pass reported object 4 `Partial("unrecoverable stream")`.
    let partial = [Location::Object {
        id: (4, 0),
        span: None,
    }];
    let (v, spent) = verify_spending(&out, &carve, &base, &[C9ZlibTampered], &partial);
    assert!(v.v0.all_pass(), "{:?}", v.v0);
    assert_eq!(spent, 0);
    // The dead stream is still in the output, as found (D-074).
    let doc = Document::load_mem(&out).unwrap();
    assert_eq!(
        doc.get_object((4, 0))
            .unwrap()
            .as_stream()
            .unwrap()
            .content
            .len(),
        64
    );
}

// ── no raster input (D-059) ──────────────────────────────────────────────

/// The V0 render gate: the one function of `verify.rs` that may touch
/// rendering.
const GATE: &str = "fn hayro_renders_every_page(";

#[test]
fn nothing_outside_the_render_gate_touches_pixels() {
    let src = include_str!("../verify.rs");
    let start = src.find(GATE).expect("the render gate");
    let end = start + src[start..].find("\n}\n").expect("the gate's end");
    let mut hits = Vec::new();
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let code = line.split("//").next().unwrap_or("").to_ascii_lowercase();
        let raster = ["render(", "::render", "pixmap", "rgba", "rendersettings"]
            .iter()
            .any(|t| code.contains(t));
        if raster && !(start..end).contains(&at) {
            hits.push(line.trim().to_owned());
        }
        at += line.len();
    }
    assert!(hits.is_empty(), "raster use outside the V0 gate: {hits:?}");
    // The tripwire would fire: the gate itself renders.
    assert!(src[start..end].contains("render("));
}

/// `verify`'s signature: the output's bytes, the input's carve, the
/// baseline, classes and locations. No pixel buffer.
type VerifyFn = fn(&[u8], &CarveReport, &Baseline, &[CorruptionClass], &[Location]) -> Verification;

#[test]
fn verify_takes_no_pixel_buffer() {
    let _: VerifyFn = verify;
}

// ── determinism ──────────────────────────────────────────────────────────

/// sha256 of the JSON verification record of the unrepaired C6 fixture's
/// Resave (golden, seed 7), whose V2 counts unmapped glyphs. The `test` job
/// runs this on all three CI OSes.
const C6_RECORD_SHA256: &str = "fcd93c3d2746778f452a87ad6f705a5978b821bdf0230db68dfb2b8efd840882";

#[test]
fn the_verification_record_is_the_committed_one() {
    let input = corrupt(C6FontMapLost, &golden_pdf(), 7);
    let out = resave(&input);
    let a = check(&out, &input, &[C6FontMapLost]);
    let b = check(&out, &input, &[C6FontMapLost]);
    assert_eq!(a, b);
    let json = serde_json::to_string(&a).expect("serialise");
    let hex: String = Sha256::digest(json.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, C6_RECORD_SHA256, "record: {json}");
}
