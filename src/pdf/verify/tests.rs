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
use crate::pdf::fixtures::{BLANK_CASES, GOLDEN_TEXT, blank_cases_pdf, corrupt, golden_pdf};
use crate::pdf::model::{ByteSpan, Evidence, FindingKind, Repairability, Severity};
use crate::pdf::rebuild::{plan_ids, rebuild_page_tree};
use crate::pdf::text::{FontKey, GlyphItem};
use crate::pdf::write::Writer;

use CorruptionClass::{
    C1Header, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped, C6FontMapLost,
    C9ZlibTampered,
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
        codes: 26,
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
    // Both sides draw the page through the fallback font, so both count the
    // codes the content shows, once each (D-088): V1 sees no loss, V2 sees
    // the damage.
    assert_eq!(v.v1.glyph_count, all(golden_counts().0));
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
    // Every `Tj` of the golden, and one code per glyph: two bytes each under
    // its Identity-H font (D-088).
    assert_eq!(
        base,
        Baseline::CarveProxy {
            show_ops: runs,
            codes: glyphs,
        }
    );
    assert_eq!(base.kind(), BaselineKind::CarveProxy);

    // Verified against the golden itself (what a perfect repair would give):
    // the proxy kind is recorded, and the show operators and the glyphs all
    // come back.
    let v = verify(&golden_pdf(), &carve, &base, &[C1Header], &[]);
    assert_eq!(v.baseline, BaselineKind::CarveProxy);
    assert_eq!(v.v1.text_ops, all(runs));
    assert_eq!(v.v1.glyph_count, all(glyphs));
    assert_eq!(v.v1.images, all(1));
}

#[test]
fn a_loading_input_gets_an_extracted_baseline() {
    let golden = golden_pdf();
    let base = baseline(&golden, &carved(&golden));
    assert_eq!(base.kind(), BaselineKind::Extracted);
    let Baseline::Extracted { pages, codes } = base else {
        unreachable!()
    };
    assert_eq!(pages.len(), GOLDEN_TEXT.len());
    // Every glyph had its own font: hayro's count stands.
    assert_eq!(codes, None);
}

// ── glyphs drawn through hayro's fallback font (D-088) ───────────────────

/// C5 seeds 2 and 4 strip the header of the golden's Type 0 font: hayro
/// draws the input with its fallback Helvetica, one glyph per byte, and the
/// two-byte codes would count twice.
#[test]
fn a_baseline_drawn_with_the_fallback_font_counts_each_code_once() {
    let (glyphs, _) = golden_counts();
    for seed in [2, 4] {
        let input = corrupt(C5ObjectTagStripped, &golden_pdf(), seed);
        let carve = carved(&input);
        let base = baseline(&input, &carve);
        let Baseline::Extracted { pages, codes } = &base else {
            panic!("seed {seed}: {:?}", base.kind())
        };
        let drawn: usize = pages.iter().map(|p| p.glyphs.len()).sum();
        assert_eq!(drawn as u64, 2 * glyphs, "seed {seed}: one glyph per byte");
        // The font reference names the headerless object; the rebuild's
        // match resolves it, so the codes are two bytes each.
        assert_eq!(*codes, Some(glyphs), "seed {seed}");
        // A perfect repair keeps every glyph.
        let v = verify(&golden_pdf(), &carve, &base, &[C5ObjectTagStripped], &[]);
        assert_eq!(v.v1.glyph_count, all(glyphs), "seed {seed}");
    }
}

/// A two-page file whose pages bind font names by `fonts` (one `/Font`
/// dictionary per page) and show `content` (one stream per page). Objects
/// 5, 6, ... are fonts of the `subtypes`, in order.
fn fonts_pdf(subtypes: &[&[u8]], fonts: [&[(&str, u32)]; 2], content: [&[u8]; 2]) -> Vec<u8> {
    let mut w = Writer::with_version("1.7");
    let font_dict = |entries: &[(&str, u32)]| {
        let mut d = Dictionary::new();
        for &(name, id) in entries {
            d.set(name, Object::Reference((id, 0)));
        }
        d
    };
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set(
        "Kids",
        Object::Array(vec![Object::Reference((3, 0)), Object::Reference((4, 0))]),
    );
    pages.set("Count", Object::Integer(2));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference((2, 0)));
    w.add(1, Object::Dictionary(catalog));
    w.add(2, Object::Dictionary(pages));
    let first_stream = 5 + subtypes.len() as u32;
    for (i, n) in [3u32, 4].into_iter().enumerate() {
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(font_dict(fonts[i])));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference((2, 0)));
        page.set(
            "MediaBox",
            Object::Array([0, 0, 612, 792].map(Object::Integer).to_vec()),
        );
        page.set("Resources", Object::Dictionary(resources));
        let stream = first_stream + i as u32;
        page.set("Contents", Object::Reference((stream, 0)));
        w.add(n, Object::Dictionary(page));
        w.add_stream_uncompressed(stream, Dictionary::new(), content[i].to_vec());
    }
    for (n, subtype) in (5u32..).zip(subtypes) {
        let mut font = Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(subtype.to_vec()));
        font.set("BaseFont", Object::Name(b"Example".to_vec()));
        w.add(n, Object::Dictionary(font));
    }
    w.trailer((1, 0), [5; 32], None);
    w.finish().expect("write")
}

/// Object 5 a Type 0 font, object 6 a TrueType one.
const MIXED: &[&[u8]] = &[b"Type0", b"TrueType"];

/// `show_counts` of `pdf`.
fn shown(pdf: &[u8]) -> Shown {
    let carve = carved(pdf);
    let graph = ObjectGraph::from_carve(&carve);
    show_counts(pdf, &carve, &graph, &classify_only(&carve, pdf))
}

fn shown_as(ops: u64, codes: u64, exact: bool, wide: bool) -> Shown {
    Shown {
        ops,
        codes,
        exact,
        wide,
    }
}

#[test]
fn the_carve_count_reads_each_code_at_its_fonts_width() {
    // `/F1` (Type 0): two bytes a code; `/F2` (TrueType): one.
    let page: &[u8] = b"BT /F1 12 Tf <00410042> Tj /F2 12 Tf (ABC) Tj [(AB) -20 <0043>] TJ ET";
    let pdf = fonts_pdf(
        MIXED,
        [&[("F1", 5), ("F2", 6)], &[("F2", 6)]],
        [page, b"BT /F2 12 Tf (D) Tj ET"],
    );
    // 2 codes under /F1; 3, then 2 + 2, then 1 under /F2.
    assert_eq!(shown(&pdf), shown_as(4, 2 + 3 + 4 + 1, true, true));
}

#[test]
fn a_font_name_bound_to_two_widths_counts_one_byte_a_code_and_is_not_exact() {
    // `/F1` is the Type 0 font on page 1 and the TrueType font on page 2:
    // which one a stream means is not known.
    let pdf = fonts_pdf(
        MIXED,
        [&[("F1", 5)], &[("F1", 6)]],
        [b"BT /F1 12 Tf <00410042> Tj ET", b"BT /F1 12 Tf (AB) Tj ET"],
    );
    assert_eq!(shown(&pdf), shown_as(2, 4 + 2, false, false));
}

#[test]
fn an_unbound_name_takes_the_width_the_files_fonts_agree_on() {
    // `/F9` is bound nowhere (as after C6), and before the first `Tf` no
    // font is selected: every font of the file is Type 0, so two bytes a
    // code.
    let page: &[u8] = b"BT <0041> Tj /F9 12 Tf <00410042> Tj ET";
    let pdf = fonts_pdf(&[b"Type0", b"Type0"], [&[("F1", 5)], &[]], [page, b""]);
    assert_eq!(shown(&pdf), shown_as(2, 1 + 2, true, true));
    // With fonts of both widths in the file the width is not known.
    let pdf = fonts_pdf(MIXED, [&[("F1", 5)], &[("F2", 6)]], [page, b""]);
    assert_eq!(shown(&pdf), shown_as(2, 2 + 4, false, false));
}

#[test]
fn a_fallback_baseline_whose_widths_are_not_known_keeps_hayros_count() {
    // hayro resolves none of these skeletal fonts and draws every string
    // with its fallback font, one glyph per byte (all of them ASCII).
    let page: &[u8] = b"BT /F9 12 Tf <00410042> Tj ET";
    let unknown = fonts_pdf(MIXED, [&[("F1", 5)], &[("F2", 6)]], [page, b""]);
    let Baseline::Extracted { pages, codes } = baseline(&unknown, &carved(&unknown)) else {
        panic!("hayro loads it")
    };
    assert_eq!(pages.iter().map(|p| p.glyphs.len()).sum::<usize>(), 4);
    assert_eq!(codes, None);
    // When the file's fonts agree on two bytes, the codes are known.
    let known = fonts_pdf(&[b"Type0"], [&[("F1", 5)], &[]], [page, b""]);
    let Baseline::Extracted { codes, .. } = baseline(&known, &carved(&known)) else {
        panic!("hayro loads it")
    };
    assert_eq!(codes, Some(2));
}

#[test]
fn q_and_big_q_restore_the_font_a_string_is_read_at() {
    // `/F1` (Type 0) is selected inside `q` .. `Q`; after `Q` the font in
    // force is `/F2` (TrueType) again. The stray `Q` first is ignored.
    let page: &[u8] = b"Q BT /F2 12 Tf ET q BT /F1 12 Tf <00410042> Tj ET Q BT (AB) Tj ET";
    let pdf = fonts_pdf(MIXED, [&[("F1", 5), ("F2", 6)], &[]], [page, b""]);
    assert_eq!(shown(&pdf), shown_as(2, 2 + 2, true, true));
    // Nested deeper than the saved levels: the outer levels still restore.
    let deep = [
        &b"BT /F2 12 Tf ET "[..],
        &b"q ".repeat(SAVED_FONTS + 1),
        b"BT /F1 12 Tf ET ",
        &b"Q ".repeat(SAVED_FONTS + 1),
        b"BT (AB) Tj ET",
    ]
    .concat();
    let pdf = fonts_pdf(MIXED, [&[("F1", 5), ("F2", 6)], &[]], [&deep, b""]);
    assert_eq!(shown(&pdf), shown_as(1, 2, true, false));
}

/// A one-page file whose page binds `/F1` to a font of `subtype` with base
/// font Helvetica (object 5) and shows `page`. A FreeText annotation's
/// normal appearance, a form, shows `appearance` through `/F1`. With
/// `font_file`, object 8 is a `FontFile3` stream of subtype `/OpenType`.
fn annotated_pdf(subtype: &[u8], page: &[u8], appearance: &[u8], font_file: bool) -> Vec<u8> {
    let name = |n: &[u8]| Object::Name(n.to_vec());
    let rect = || Object::Array([0, 0, 612, 792].map(Object::Integer).to_vec());
    let fonts = || {
        let mut fonts = Dictionary::new();
        fonts.set("F1", Object::Reference((5, 0)));
        let mut resources = Dictionary::new();
        resources.set("Font", Object::Dictionary(fonts));
        Object::Dictionary(resources)
    };
    let mut w = Writer::with_version("1.7");
    let mut catalog = Dictionary::new();
    catalog.set("Type", name(b"Catalog"));
    catalog.set("Pages", Object::Reference((2, 0)));
    w.add(1, Object::Dictionary(catalog));
    let mut pages = Dictionary::new();
    pages.set("Type", name(b"Pages"));
    pages.set("Kids", Object::Array(vec![Object::Reference((3, 0))]));
    pages.set("Count", Object::Integer(1));
    w.add(2, Object::Dictionary(pages));
    let mut leaf = Dictionary::new();
    leaf.set("Type", name(b"Page"));
    leaf.set("Parent", Object::Reference((2, 0)));
    leaf.set("MediaBox", rect());
    leaf.set("Resources", fonts());
    leaf.set("Contents", Object::Reference((4, 0)));
    leaf.set("Annots", Object::Array(vec![Object::Reference((6, 0))]));
    w.add(3, Object::Dictionary(leaf));
    let text = |show: &[u8]| [&b"BT 72 700 Td "[..], show, b" ET"].concat();
    w.add_stream_uncompressed(4, Dictionary::new(), text(page));
    let mut font = Dictionary::new();
    font.set("Type", name(b"Font"));
    font.set("Subtype", name(subtype));
    font.set("BaseFont", name(b"Helvetica"));
    w.add(5, Object::Dictionary(font));
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference((7, 0)));
    let mut annot = Dictionary::new();
    annot.set("Type", name(b"Annot"));
    annot.set("Subtype", name(b"FreeText"));
    annot.set("Rect", rect());
    annot.set("DA", Object::string_literal("/F1 12 Tf 0 g"));
    annot.set("AP", Object::Dictionary(ap));
    w.add(6, Object::Dictionary(annot));
    let mut form = Dictionary::new();
    form.set("Type", name(b"XObject"));
    form.set("Subtype", name(b"Form"));
    form.set("BBox", rect());
    form.set("Resources", fonts());
    w.add_stream_uncompressed(7, form, text(appearance));
    if font_file {
        let mut file = Dictionary::new();
        file.set("Subtype", name(b"OpenType"));
        w.add_stream_uncompressed(8, file, b"OTTO".to_vec());
    }
    w.trailer((1, 0), [8; 32], None);
    w.finish().expect("write")
}

/// The glyphs hayro drew on `pages`, and whether any through its fallback.
fn drawn(pages: &[PageText]) -> (u64, bool) {
    let n = pages.iter().map(|p| p.glyphs.len() as u64).sum();
    (n, drew_with_fallback(pages))
}

#[test]
fn a_one_byte_fallback_keeps_hayros_count_and_leaves_annotations_out() {
    // The page shows `(ABC)` through `/F9`, bound nowhere, so hayro draws it
    // with its fallback font. The annotation shows `(WXYZ)` through `/F1`,
    // and T-36 does not draw annotations.
    let input = annotated_pdf(
        b"Type1",
        b"/F9 12 Tf (ABC) Tj",
        b"/F1 12 Tf (WXYZ) Tj",
        false,
    );
    let carve = carved(&input);
    let base = baseline(&input, &carve);
    let Baseline::Extracted { pages, codes } = &base else {
        panic!("hayro loads it")
    };
    assert_eq!(drawn(pages), (3, true));
    // One byte a code everywhere: hayro's count is already right.
    assert_eq!(*codes, None);
    // The repaired page, whose `/F1` hayro draws, keeps every glyph; the
    // unrepaired input does not score higher.
    let fixed = annotated_pdf(
        b"Type1",
        b"/F1 12 Tf (ABC) Tj",
        b"/F1 12 Tf (WXYZ) Tj",
        false,
    );
    let fixed_text = extract_text(&fixed, &ExtractOptions::default()).expect("loads");
    assert_eq!(drawn(&fixed_text), (3, false));
    let perfect = verify(&fixed, &carve, &base, &[], &[]).v1.glyph_count;
    let unrepaired = verify(&input, &carve, &base, &[], &[]).v1.glyph_count;
    assert_eq!(perfect, all(3));
    assert!(unrepaired <= perfect, "{unrepaired:?} > {perfect:?}");
}

#[test]
fn a_two_byte_fallback_counts_only_the_codes_the_pages_show() {
    // hayro does not resolve the skeletal Type 0 `/F1` and draws the page's
    // two codes as four fallback glyphs. The annotation's code is in no
    // page's content.
    let input = annotated_pdf(
        b"Type0",
        b"/F1 12 Tf <00410042> Tj",
        b"/F1 12 Tf <0043> Tj",
        false,
    );
    let carve = carved(&input);
    let base = baseline(&input, &carve);
    let Baseline::Extracted { pages, codes } = &base else {
        panic!("hayro loads it")
    };
    assert_eq!(drawn(pages), (4, true));
    assert_eq!(*codes, Some(2));
    // Every stream, the appearance included, holds three codes.
    assert_eq!(shown(&input), shown_as(2, 3, true, true));
    // The input as its own output counts the same way on both sides.
    let v = verify(&input, &carve, &base, &[], &[]);
    assert_eq!(v.v1.glyph_count, all(2));
}

#[test]
fn an_opentype_font_file_is_not_taken_for_a_one_byte_font() {
    // The Type 0 font is the file's only font; its `FontFile3` stream's
    // `/Subtype /OpenType` is not a font of width 1. So `/F9`, bound
    // nowhere, takes the file's two bytes a code.
    let pdf = annotated_pdf(b"Type0", b"/F9 12 Tf <00410042> Tj", b"", true);
    assert_eq!(shown(&pdf), shown_as(1, 2, true, true));
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

/// The golden with each page's `/MediaBox` set to `boxes[i]`.
fn with_media_boxes(boxes: [[f32; 4]; 2]) -> Vec<u8> {
    rewritten(&golden_pdf(), |objects| {
        let (_, kids) = page_numbers(objects);
        for (page, b) in kids.into_iter().zip(boxes) {
            let rect = b.iter().map(|&v| Object::Real(v)).collect();
            dict_mut(objects, page).set("MediaBox", Object::Array(rect));
        }
    })
}

#[test]
fn a_huge_media_box_renders_small_and_passes() {
    // At 1/8 scale the first page alone would be a 65535 × 65535 pixmap,
    // about 17 GiB; the second a 125000 × 0 one.
    let out = with_media_boxes([[0.0, 0.0, 524_280.0, 524_280.0], [0.0, 0.0, 1.0e6, 1.0]]);
    let v = check(&out, &golden_pdf(), &[]);
    assert!(v.v0.hayro_render_all_pages, "{:?}", v.v0);
    assert!(v.v0.all_pass(), "{:?}", v.v0);
}

#[test]
fn a_media_box_beyond_f32_renders_at_scale_zero() {
    // lopdf's `Real` is `f32` and PDF numbers have no exponent, so the
    // 41-digit width (1e40, infinite as `f32`) goes in as bytes.
    let out = with_media_boxes([[0.0, 0.0, 612.0, 792.0], [0.0, 0.0, 777_777.0, 792.0]]);
    let at = out
        .windows(6)
        .position(|w| w == b"777777")
        .expect("the marker width");
    let mut huge = out[..at].to_vec();
    huge.extend_from_slice(format!("1{}", "0".repeat(40)).as_bytes());
    huge.extend_from_slice(&out[at + 6..]);
    // The xref offsets are off now, which hayro repairs. The page draws at
    // scale 0 (`gate_scale`) and the gate passes rather than aborting.
    assert!(hayro_renders_every_page(&huge, 2));
}

#[test]
fn the_gate_scale_is_an_eighth_bounded_by_the_longest_side() {
    assert_eq!(gate_scale(612.0, 792.0), 0.125);
    assert_eq!(gate_scale(2048.0, 100.0), 0.125);
    assert_eq!(gate_scale(4096.0, 100.0), 0.0625);
    assert_eq!(gate_scale(100.0, 524_288.0), 256.0 / 524_288.0);
    assert_eq!(gate_scale(f32::INFINITY, 792.0), 0.0);
    assert_eq!(gate_scale(f32::NAN, f32::NAN), 0.0);
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
    let partial_4 = (
        C9ZlibTampered,
        Location::Object {
            id: (4, 0),
            span: None,
        },
    );
    assert!(clean_for(&found, &[C9ZlibTampered], &[partial_4]));
    assert!(!clean_for(
        &found,
        &[C9ZlibTampered],
        &[(C9ZlibTampered, at(5))]
    ));
    // Untargeted classes do not count; targeted ones elsewhere do.
    assert!(!clean_for(
        &found,
        &[C9ZlibTampered, C6FontMapLost],
        &[partial_4]
    ));
    assert!(clean_for(
        &found,
        &[C9ZlibTampered, C6FontMapLost],
        &[partial_4, (C6FontMapLost, page(1))]
    ));
    assert!(clean_for(&found, &[C4PageTreeBroken], &[]));
    assert!(clean_for(&[], &CorruptionClass::ALL, &[]));
}

#[test]
fn a_partial_excuses_only_a_finding_of_its_own_class() {
    let page = |index, obj: Option<u32>| Location::Page {
        index,
        obj: obj.map(|n| (n, 0)),
    };
    let targeted = [C6FontMapLost, C9ZlibTampered];
    let c6_on_page_1 = [finding(C6FontMapLost, page(1, None))];
    // A C9 Partial on page 1 no longer excuses the C6 finding there.
    assert!(!clean_for(
        &c6_on_page_1,
        &targeted,
        &[(C9ZlibTampered, page(1, None))]
    ));
    assert!(!clean_for(
        &c6_on_page_1,
        &targeted,
        &[(C9ZlibTampered, page(1, Some(7)))]
    ));
    // A C6 Partial at the same site does.
    assert!(clean_for(
        &c6_on_page_1,
        &targeted,
        &[(C6FontMapLost, page(1, Some(7)))]
    ));
    // The same holds for an object location.
    let obj = |n| Location::Object {
        id: (n, 0),
        span: None,
    };
    let c9_at_7 = [finding(C9ZlibTampered, obj(7))];
    assert!(!clean_for(&c9_at_7, &targeted, &[(C6FontMapLost, obj(7))]));
    assert!(clean_for(&c9_at_7, &targeted, &[(C9ZlibTampered, obj(7))]));
}

#[test]
fn a_site_matches_per_page_and_across_object_and_page_forms() {
    let obj = |n| Location::Object {
        id: (n, 0),
        span: None,
    };
    let page = |index, obj: Option<u32>| Location::Page {
        index,
        obj: obj.map(|n| (n, 0)),
    };
    // A page-wide location covers every object on that page, both ways.
    assert!(same_site(&page(1, None), &page(1, Some(7))));
    assert!(same_site(&page(1, Some(7)), &page(1, None)));
    assert!(!same_site(&page(1, Some(7)), &page(1, Some(8))));
    assert!(!same_site(&page(1, None), &page(2, None)));
    // An object and a page location naming it, both ways.
    assert!(same_site(&obj(7), &page(1, Some(7))));
    assert!(same_site(&page(1, Some(7)), &obj(7)));
    assert!(!same_site(&obj(7), &page(1, Some(8))));
    assert!(!same_site(&obj(7), &page(1, None)));
    // Spans and the file match only themselves.
    let span = Location::Span(ByteSpan { start: 1, end: 2 });
    assert!(same_site(&span, &span));
    assert!(!same_site(&span, &Location::File));
    assert!(same_site(&Location::File, &Location::File));
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
    let partial = [(
        C9ZlibTampered,
        Location::Object {
            id: (4, 0),
            span: None,
        },
    )];
    let (v, spent) = verify_spending(&out, &carve, &base, &[C9ZlibTampered], &partial);
    assert!(v.v0.all_pass(), "{:?}", v.v0);
    assert_eq!(spent, 0);
    // T-11b: diagnose's C6-C9 classes are still a stub, so today the output
    // carries no C9 finding and `partial` changes nothing. Once T-11b reports
    // the dead stream, `partial` is what keeps V0 passing.
    let out_carve = carved(&out);
    let out_graph = ObjectGraph::from_carve(&out_carve);
    let findings = diagnose(
        &out,
        &out_carve,
        &out_graph,
        &classify_only(&out_carve, &out),
    );
    let c9_at_4 = findings.iter().any(|f| {
        f.class == FindingKind::Corruption(C9ZlibTampered) && same_site(&f.location, &partial[0].1)
    });
    if c9_at_4 {
        let (bare, _) = verify_spending(&out, &carve, &base, &[C9ZlibTampered], &[]);
        assert!(
            !bare.v0.rediagnose_clean,
            "the finding counts without `partial`"
        );
    }
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

/// Identifiers that render or hold pixels (lowercased): any outside the
/// gate trips the wire. `hayro_render_all_pages` and `RENDER_MAX_SIDE` are
/// whole identifiers of their own and do not.
const RASTER_IDENTS: [&str; 7] = [
    "render",
    "render_into",
    "rendercache",
    "rendersettings",
    "rendercontext",
    "pixmap",
    "pixmapsettings",
];

/// Whether `line`'s code (comments cut) names a raster identifier.
fn raster(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("").to_ascii_lowercase();
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|ident| {
            RASTER_IDENTS.contains(&ident) || ident.contains("pixmap") || ident.contains("rgba")
        })
}

/// The lines of `src` outside the render gate that touch pixels, and the
/// gate's own text, with LF line endings. Panics if `src` has no gate, or no
/// line closing it.
fn raster_outside_gate(src: &str) -> (Vec<String>, String) {
    let src = crate::line_endings::lf(src);
    let start = src.find(GATE).expect("the render gate");
    let end = start + src[start..].find("\n}\n").expect("the gate's end");
    let mut hits = Vec::new();
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        if raster(line) && !(start..end).contains(&at) {
            hits.push(line.trim().to_owned());
        }
        at += line.len();
    }
    (hits, src[start..end].to_owned())
}

#[test]
fn nothing_outside_the_render_gate_touches_pixels() {
    let (hits, gate) = raster_outside_gate(include_str!("../verify.rs"));
    assert!(hits.is_empty(), "raster use outside the V0 gate: {hits:?}");
    // The tripwire would fire: the gate itself renders, and each spelling
    // the review named is caught.
    assert!(raster(&gate));
    for spelling in [
        "use hayro::{render, RenderCache};",
        "render (page, &cache)",
        "let p: Pixmap = x;",
        "data.rgba8()",
    ] {
        assert!(raster(spelling), "{spelling}");
    }
    assert!(!raster("hayro_render_all_pages: RENDER_MAX_SIDE"));
}

/// A CRLF checkout of `verify.rs` scans the same (CI-01): the gate's end is
/// found and a raster use outside it is still caught.
#[test]
fn the_render_gate_scan_reads_a_crlf_checkout() {
    use crate::line_endings::crlf;
    let src = include_str!("../verify.rs");
    let (hits, gate) = raster_outside_gate(&crlf(src));
    assert!(hits.is_empty(), "{hits:?}");
    assert_eq!(gate, raster_outside_gate(src).1);
    let planted = crlf(&format!("{src}\nfn leak() {{ let _ = Pixmap::new(); }}\n"));
    let (hits, _) = raster_outside_gate(&planted);
    assert_eq!(hits, ["fn leak() { let _ = Pixmap::new(); }"]);
}

/// `verify`'s signature: the output's bytes, the input's carve, the
/// baseline, classes and class-qualified locations. No pixel buffer.
type VerifyFn = fn(
    &[u8],
    &CarveReport,
    &Baseline,
    &[CorruptionClass],
    &[(CorruptionClass, Location)],
) -> Verification;

#[test]
fn verify_takes_no_pixel_buffer() {
    let _: VerifyFn = verify;
}

// ── determinism ──────────────────────────────────────────────────────────

/// sha256 of the JSON verification record of the unrepaired C6 fixture's
/// Resave (golden, seed 7), whose V2 counts unmapped glyphs. Its V0 fails
/// `rediagnose_clean`: Resave repairs nothing, so the output still carries
/// the targeted C6 finding. Its V1 glyph count is the codes the content
/// shows, as hayro draws the font-less page with its fallback font (D-088).
/// The `test` job runs this on all three CI OSes.
const C6_RECORD_SHA256: &str = "1665c82b8672feb502f21a2b1b4a29f7935de73d5960dc9aa8d9485ffe0b0f32";

#[test]
fn the_verification_record_is_the_committed_one() {
    let input = corrupt(C6FontMapLost, &golden_pdf(), 7);
    let out = resave(&input);
    let a = check(&out, &input, &[C6FontMapLost]);
    let b = check(&out, &input, &[C6FontMapLost]);
    assert_eq!(a, b);
    assert!(!a.v0.rediagnose_clean, "an unrepaired C6 is not clean");
    let json = serde_json::to_string(&a).expect("serialise");
    let hex: String = Sha256::digest(json.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, C6_RECORD_SHA256, "record: {json}");
}
