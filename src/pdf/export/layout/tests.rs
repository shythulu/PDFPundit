use super::*;

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::pdf::fixtures::{GOLDEN_TEXT, golden_pdf};
use crate::pdf::model::{FindingKind, Location, PaintCounts, Repairability, Severity};
use crate::pdf::text::{ExtractOptions, FontInfo, FontKey, GlyphItem, extract_text, font_table};

const REGULAR: FontKey = FontKey(1);
const BOLD: FontKey = FontKey(2);
const BODY: f64 = 11.0;
/// Body baseline pitch: 1.2 × 11 pt.
const PITCH: f64 = 13.2;
/// Every test glyph advances half an em.
const ADVANCE: f32 = 500.0;

fn font(name: &str, weight: u32) -> FontInfo {
    FontInfo {
        base_font: Some(name.to_owned()),
        subtype: "TrueType".to_owned(),
        flags: Some(32),
        postscript_name: Some(name.to_owned()),
        weight: Some(weight),
        italic: false,
        serif: false,
        mono: false,
    }
}

fn fonts() -> FontTable {
    BTreeMap::from([
        (REGULAR, font("Helvetica", 400)),
        (BOLD, font("Helvetica-Bold", 700)),
    ])
}

fn glyph(text: &str, x: f64, y: f64, size: f64, font: FontKey) -> GlyphItem {
    GlyphItem {
        text: Some(text.to_owned()),
        x,
        y,
        size,
        advance: Some(ADVANCE),
        font,
        invisible: false,
        type3: false,
        mcid: None,
    }
}

/// `s` drawn left to right from `x`, one glyph per char.
fn ltr(out: &mut Vec<GlyphItem>, s: &str, x: f64, y: f64, size: f64, font: FontKey) {
    for (i, c) in s.chars().enumerate() {
        out.push(glyph(
            &c.to_string(),
            x + i as f64 * size * 0.5,
            y,
            size,
            font,
        ));
    }
}

/// `s` in logical order, drawn right to left: its first char ends at `right`.
fn rtl(out: &mut Vec<GlyphItem>, s: &str, right: f64, y: f64, size: f64) {
    for (i, c) in s.chars().enumerate() {
        let x = right - (i + 1) as f64 * size * 0.5;
        out.push(glyph(&c.to_string(), x, y, size, REGULAR));
    }
}

fn page_of(glyphs: Vec<GlyphItem>) -> PageText {
    let unmapped = glyphs.iter().filter(|g| g.text.is_none()).count() as u32;
    PageText {
        index: 0,
        width: 612.0,
        height: 792.0,
        glyphs,
        unmapped,
        paint: PaintCounts::default(),
        warnings: Vec::new(),
    }
}

fn layout(page: &PageText) -> PageLayout {
    analyse(page, &fonts(), &[], &[], &[])
}

fn plain(text: &str) -> Run {
    Run {
        text: text.to_owned(),
        bold: false,
        italic: false,
        mono: false,
        link: None,
    }
}

fn para(lines: &[&str]) -> Block {
    Block::Paragraph(lines.iter().map(|l| Line(vec![plain(l)])).collect())
}

/// Lines `lines` from `top` down at the body pitch.
fn body(out: &mut Vec<GlyphItem>, lines: &[&str], x: f64, top: f64) {
    for (i, s) in lines.iter().enumerate() {
        ltr(out, s, x, top - i as f64 * PITCH, BODY, REGULAR);
    }
}

// ── fixtures ────────────────────────────────────────────────────────────

/// The golden's lines rearranged into two columns on shared baselines: page
/// one's on the left, page two's on the right. Drawn left column first.
fn two_column_page() -> PageText {
    let mut g = Vec::new();
    body(&mut g, GOLDEN_TEXT[0], 72.0, 700.0);
    body(&mut g, GOLDEN_TEXT[1], 340.0, 700.0);
    page_of(g)
}

const TITLE: &str = "PDFPundit golden page one.";
const BODY_LINES: [&str; 3] = [
    "The quick brown fox jumps over the lazy dog.",
    "Le garçon a mangé la crème « au cœur ».",
    "El niño y el pingüino, l’été.",
];

fn title_page() -> PageText {
    let mut g = Vec::new();
    ltr(&mut g, TITLE, 72.0, 720.0, 24.0, REGULAR);
    body(&mut g, &BODY_LINES, 72.0, 690.0);
    page_of(g)
}

/// A 3×3 grid drawn row by row; the first row in `head`.
fn grid_page(head: FontKey) -> PageText {
    let mut g = Vec::new();
    for (r, row) in [["A1", "B1", "C1"], ["A2", "B2", "C2"], ["A3", "B3", "C3"]]
        .iter()
        .enumerate()
    {
        let font = if r == 0 { head } else { REGULAR };
        for (c, cell) in row.iter().enumerate() {
            ltr(
                &mut g,
                cell,
                72.0 + c as f64 * 100.0,
                700.0 - r as f64 * 14.0,
                BODY,
                font,
            );
        }
    }
    page_of(g)
}

fn list_page(lines: &[&str]) -> PageText {
    let mut g = Vec::new();
    body(&mut g, lines, 72.0, 700.0);
    page_of(g)
}

const ARABIC_RIGHT: [&str; 2] = ["مرحبا بالعالم", "هذا نص عربي"];
const ARABIC_LEFT: [&str; 2] = ["العمود الثاني", "سطر آخر هنا"];

/// Two Arabic columns on shared baselines, the left column drawn first so the
/// order comes from geometry, not from content order.
fn arabic_page() -> PageText {
    let mut g = Vec::new();
    for (i, s) in ARABIC_LEFT.iter().enumerate() {
        rtl(&mut g, s, 300.0, 700.0 - i as f64 * PITCH, BODY);
    }
    for (i, s) in ARABIC_RIGHT.iter().enumerate() {
        rtl(&mut g, s, 540.0, 700.0 - i as f64 * PITCH, BODY);
    }
    page_of(g)
}

fn finding(class: FindingKind, index: u32) -> Finding {
    Finding {
        id: "OUTLINE-001".to_owned(),
        class,
        severity: Severity::Info,
        location: Location::Page { index, obj: None },
        summary: String::new(),
        evidence: Vec::new(),
        repair: Repairability::NotApplicable,
    }
}

fn image(name: &str) -> ExtractedImage {
    ExtractedImage {
        name: name.to_owned(),
        sha256: [7; 32],
        len: 1234,
    }
}

// ── acceptance ──────────────────────────────────────────────────────────

#[test]
fn two_columns_read_column_by_column() {
    let l = layout(&two_column_page());
    assert_eq!(l.blocks, vec![para(GOLDEN_TEXT[0]), para(GOLDEN_TEXT[1])]);
    assert_eq!((l.index, l.width_pt, l.height_pt), (0, 612, 792));
    assert!(l.notes.is_empty());
}

#[test]
fn a_large_title_over_body_is_one_heading_and_one_paragraph() {
    let l = layout(&title_page());
    assert_eq!(
        l.blocks,
        vec![
            Block::Heading {
                level: 1,
                runs: vec![plain(TITLE)],
            },
            para(&BODY_LINES),
        ]
    );
}

#[test]
fn an_aligned_grid_is_a_table() {
    let cells = |r: &str| ["A", "B", "C"].map(|c| format!("{c}{r}")).to_vec();
    let rows = vec![cells("1"), cells("2"), cells("3")];
    assert_eq!(
        layout(&grid_page(REGULAR)).blocks,
        vec![Block::Table {
            rows: rows.clone(),
            header: false,
        }]
    );
    assert_eq!(
        layout(&grid_page(BOLD)).blocks,
        vec![Block::Table { rows, header: true }]
    );
}

#[test]
fn bullet_lines_are_one_unordered_list() {
    let l = layout(&list_page(&[
        "• First item",
        "• Second item",
        "• Third item",
    ]));
    let items = ["First item", "Second item", "Third item"]
        .map(|t| vec![Line(vec![plain(t)])])
        .to_vec();
    assert_eq!(
        l.blocks,
        vec![Block::List {
            ordered: false,
            items,
        }]
    );
}

#[test]
fn numbered_lines_are_one_ordered_list() {
    let l = layout(&list_page(&["1. One", "2. Two", "3. Three"]));
    let items = ["One", "Two", "Three"]
        .map(|t| vec![Line(vec![plain(t)])])
        .to_vec();
    assert_eq!(
        l.blocks,
        vec![Block::List {
            ordered: true,
            items,
        }]
    );
}

#[test]
fn a_glyph_and_its_shadow_are_one_run() {
    // Fake bold: every glyph drawn twice, the copy 20 milli-em (0.2 pt at
    // 10 pt) right and down.
    let mut g = Vec::new();
    for (i, c) in "Bold".chars().enumerate() {
        let x = 72.0 + i as f64 * 5.0;
        g.push(glyph(&c.to_string(), x, 700.0, 10.0, REGULAR));
        g.push(glyph(&c.to_string(), x + 0.2, 699.8, 10.0, REGULAR));
    }
    let l = layout(&page_of(g));
    assert_eq!(l.blocks, vec![para(&["Bold"])]);
}

#[test]
fn invisible_and_tiny_glyphs_never_enter_a_run() {
    let mut g = Vec::new();
    body(&mut g, &["Seen"], 72.0, 700.0);
    let hidden = g.len();
    ltr(&mut g, "Hidden", 72.0, 650.0, BODY, REGULAR);
    for item in &mut g[hidden..] {
        item.invisible = true;
    }
    ltr(&mut g, "tiny", 72.0, 600.0, 1.5, REGULAR);
    let l = layout(&page_of(g));
    assert_eq!(l.blocks, vec![para(&["Seen"])]);
    assert_eq!(l.notes, vec![LayoutNote::Unmapped { glyphs: 10 }]);
}

#[test]
fn a_uri_annot_over_a_run_attaches_its_link() {
    let mut g = Vec::new();
    body(&mut g, &["Read the docs"], 72.0, 700.0);
    // "docs" starts at char 9: x = 72 + 9 × 5.5 = 121.5 pt.
    let annots = [UriAnnot {
        rect: [121_000, 698_000, 150_000, 710_000],
        uri: "https://example.org/".to_owned(),
    }];
    let l = analyse(&page_of(g), &fonts(), &[], &[], &annots);
    let link = Run {
        link: Some("https://example.org/".to_owned()),
        ..plain("docs")
    };
    assert_eq!(
        l.blocks,
        vec![Block::Paragraph(vec![Line(vec![plain("Read the "), link])])]
    );
}

#[test]
fn page_findings_become_notes() {
    let findings = [
        finding(
            FindingKind::OutlinedText {
                glyph_runs: 9,
                contours: 412,
            },
            0,
        ),
        finding(FindingKind::Type3Text { font: (12, 0) }, 0),
        // Another page's finding is not this page's note.
        finding(
            FindingKind::OutlinedText {
                glyph_runs: 1,
                contours: 1,
            },
            1,
        ),
    ];
    let l = analyse(&list_page(&["Text"]), &fonts(), &findings, &[], &[]);
    assert_eq!(
        l.notes,
        vec![
            LayoutNote::OutlinedText {
                paths: 9,
                contours: 412,
            },
            LayoutNote::Type3Text,
        ]
    );
}

#[test]
fn an_image_only_page_says_so() {
    let mut page = page_of(Vec::new());
    page.paint.images = 1;
    let l = analyse(&page, &fonts(), &[], &[image("p1-1.jpg")], &[]);
    assert_eq!(
        l.blocks,
        vec![Block::Image {
            name: "p1-1.jpg".to_owned(),
            sha256: [7; 32],
            len: 1234,
        }]
    );
    assert_eq!(l.notes, vec![LayoutNote::ImageOnly]);
}

#[test]
fn unmapped_glyphs_are_a_note() {
    let mut g = Vec::new();
    body(&mut g, &["Text"], 72.0, 700.0);
    g[1].text = None;
    let l = layout(&page_of(g));
    assert_eq!(l.notes, vec![LayoutNote::Unmapped { glyphs: 1 }]);
}

#[test]
fn an_arabic_majority_page_reverses_the_column_order() {
    let l = layout(&arabic_page());
    assert_eq!(l.blocks, vec![para(&ARABIC_RIGHT), para(&ARABIC_LEFT)]);
}

#[test]
fn the_golden_reads_its_known_strings() {
    for (layout, lines) in golden_layouts().iter().zip(GOLDEN_TEXT) {
        let text: Vec<String> = layout.blocks.iter().flat_map(block_lines).collect();
        assert_eq!(text, lines.to_vec(), "{layout:?}");
    }
}

fn block_lines(b: &Block) -> Vec<String> {
    let line = |l: &Line| l.0.iter().map(|r| r.text.as_str()).collect::<String>();
    match b {
        Block::Heading { runs, .. } => vec![line(&Line(runs.clone()))],
        Block::Paragraph(lines) => lines.iter().map(line).collect(),
        Block::List { items, .. } => items.iter().flatten().map(line).collect(),
        Block::Table { rows, .. } => rows.iter().map(|r| r.join(" | ")).collect(),
        Block::Image { name, .. } => vec![name.clone()],
        Block::Rule => vec!["---".to_owned()],
    }
}

fn golden_layouts() -> Vec<PageLayout> {
    let pdf = golden_pdf();
    let pages = extract_text(&pdf, &ExtractOptions::default()).expect("extracts");
    let fonts = font_table(&pdf).expect("font table");
    pages
        .iter()
        .map(|p| analyse(p, &fonts, &[], &[], &[]))
        .collect()
}

/// The golden's pages and every synthetic page above.
fn fixture_layouts() -> Vec<PageLayout> {
    let mut out = golden_layouts();
    out.extend(
        [
            two_column_page(),
            title_page(),
            grid_page(BOLD),
            list_page(&["• First item", "• Second item", "• Third item"]),
            list_page(&["1. One", "2. Two", "3. Three"]),
            arabic_page(),
        ]
        .iter()
        .map(layout),
    );
    out
}

#[test]
fn analyse_is_deterministic() {
    assert_eq!(fixture_layouts(), fixture_layouts());
}

/// sha256 of the fixture layouts' JSON. The `test` job runs this on all three
/// OSes. A change here is a change in what export writes: re-baseline only
/// with a reason (a threshold tuning is a re-baseline, not a design change).
const FIXTURE_DUMP_SHA256: &str =
    "da897c3ce36b5c3b4730aaca90546b5399e9762ac664f4b1fb7b173fd984b728";

#[test]
fn the_fixture_dump_hash_is_the_committed_one() {
    let dump = serde_json::to_string(&fixture_layouts()).expect("serialises");
    let hex: String = Sha256::digest(dump.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, FIXTURE_DUMP_SHA256, "dump:\n{dump}");
}

/// xorshift64*: a fixed, platform-independent source for the fuzz test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A coordinate: mostly on the page, sometimes absurd.
    fn coord(&mut self) -> f64 {
        match self.below(40) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => -1.0e300,
            3 => 1.0e300,
            _ => self.below(900_000) as f64 / 1000.0 - 50.0,
        }
    }
}

#[test]
fn ten_thousand_fuzzed_pages_terminate_without_panic() {
    const TEXTS: [&str; 14] = [
        "a", "B", " ", "•", "-", "1", ".", ")", "(", "ا", "ש", "—", "\u{7}", "ﬁ",
    ];
    let fonts = fonts();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..10_000 {
        let n = rng.below(60);
        let mut glyphs = Vec::new();
        for _ in 0..n {
            let text = match rng.below(12) {
                0 => None,
                _ => Some(TEXTS[rng.below(TEXTS.len() as u64) as usize].to_owned()),
            };
            glyphs.push(GlyphItem {
                text,
                x: rng.coord(),
                y: rng.coord(),
                size: match rng.below(10) {
                    0 => rng.coord(),
                    _ => rng.below(40_000) as f64 / 1000.0,
                },
                advance: match rng.below(10) {
                    0 => None,
                    1 => Some(f32::NAN),
                    2 => Some(-5.0e30),
                    _ => Some(rng.below(1200) as f32),
                },
                font: FontKey(u128::from(rng.below(4) as u8)),
                invisible: rng.below(8) == 0,
                type3: false,
                mcid: None,
            });
        }
        let mut page = page_of(glyphs);
        page.width = if rng.below(10) == 0 { f32::NAN } else { 612.0 };
        page.paint.images = (rng.below(3) == 0).into();
        let annots = [UriAnnot {
            rect: [
                rng.next() as i32,
                rng.next() as i32,
                rng.next() as i32,
                rng.next() as i32,
            ],
            uri: "u".to_owned(),
        }];
        let images = [image("p1-1.jpg")];
        let l = analyse(
            &page,
            &fonts,
            &[],
            &images[..rng.below(2) as usize],
            &annots,
        );
        assert!(l.blocks.iter().all(|b| match b {
            Block::Heading { level, .. } => (1..=3).contains(level),
            _ => true,
        }));
    }
}

/// The comments that fence the entry conversion in `layout.rs`.
const ENTRY_FENCE_START: &str = "// ── entry conversion";
const ENTRY_FENCE_END: &str = "// ── end of entry conversion";

/// The libm ban is crate-wide (clippy.toml). This tripwire keeps floating
/// point inside the entry conversion: outside its fence the module names no
/// float type, rounds nothing and never reads a `GlyphItem`.
#[test]
fn floats_stay_inside_the_entry_conversion() {
    let source = include_str!("../layout.rs");
    let start = source
        .find(ENTRY_FENCE_START)
        .expect("the entry conversion is fenced");
    let end = source.find(ENTRY_FENCE_END).expect("the fence is closed");
    assert!(start < end);
    // Code only: comments may cite section numbers and say "rounded".
    let outside: String = format!("{}{}", &source[..start], &source[end..])
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| format!("{l}\n"))
        .collect();
    for token in ["f32", "f64", "round", "sqrt", "GlyphItem", "as_f", "1000.0"] {
        assert!(
            !outside.contains(token),
            "layout.rs names {token} outside the fence"
        );
    }
    // No float literal outside the fence.
    let bytes = outside.as_bytes();
    for w in bytes.windows(3) {
        assert!(
            !(w[0].is_ascii_digit() && w[1] == b'.' && w[2].is_ascii_digit()),
            "a float literal outside the fence: {}",
            String::from_utf8_lossy(w)
        );
    }
}

// ── the passes, one by one ──────────────────────────────────────────────

/// Glyphs from `(text, x, y, size)` in milli-points, one font, half-em
/// advances, in content order.
fn glyphs(items: &[(&str, i64, i64, i64)]) -> Vec<Glyph> {
    items
        .iter()
        .enumerate()
        .map(|(seq, &(text, x, y, size))| Glyph {
            seq,
            text: Some(text.to_owned()),
            font: REGULAR,
            x,
            y,
            size,
            advance: 500,
            invisible: false,
        })
        .collect()
}

fn texts(spans: &[Span]) -> Vec<&str> {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn runs_of(g: &[Glyph]) -> Vec<Span> {
    merge_runs(g, &fonts(), &[], false)
}

#[test]
fn pass1_drops_a_copy_within_fifty_milli_em_only() {
    // 10 pt: 50 milli-em is 0.5 pt.
    let g = glyphs(&[
        ("a", 0, 0, 10_000),
        ("a", 400, -400, 10_000),
        ("a", 600, 0, 10_000),
        ("b", 0, 0, 10_000),
    ]);
    let kept: Vec<usize> = drop_shadows(g).iter().map(|g| g.seq).collect();
    assert_eq!(kept, [0, 2, 3]);
    let mut other_font = glyphs(&[("a", 0, 0, 10_000), ("a", 0, 0, 10_000)]);
    other_font[1].font = BOLD;
    assert_eq!(drop_shadows(other_font).len(), 2);
}

#[test]
fn pass4_counts_hidden_text_but_not_hidden_spaces() {
    let mut g = glyphs(&[
        ("a", 0, 0, 10_000),
        ("b", 0, 0, 1_999),
        (" ", 0, 0, 1_000),
        ("c", 0, 0, 10_000),
        ("d", 0, 0, 2_000),
    ]);
    g[3].invisible = true;
    g.push(Glyph {
        text: None,
        ..g[0].clone()
    });
    let (kept, hidden) = drop_unprintable(g);
    assert_eq!(kept.iter().map(|g| g.seq).collect::<Vec<_>>(), [0, 4]);
    // "b" (tiny) and "c" (invisible); the tiny space and the unmapped glyph
    // are not counted here.
    assert_eq!(hidden, 2);
}

#[test]
fn pass2_spaces_and_breaks_by_gap() {
    // 10 pt glyphs, 5 pt advances: a glyph at +5 pt touches the last.
    let g = glyphs(&[
        ("a", 0, 0, 10_000),
        ("b", 7_400, 0, 10_000),  // gap 2.4 pt = 240 milli-em: joined
        ("c", 14_900, 0, 10_000), // gap 2.5 pt = 250 milli-em: one space
        ("d", 29_800, 0, 10_000), // gap 9.9 pt: still one space
        ("e", 44_800, 0, 10_000), // gap 10 pt = one em: a new run
    ]);
    assert_eq!(texts(&runs_of(&g)), ["ab c d", "e"]);
}

#[test]
fn pass2_breaks_on_font_baseline_and_link() {
    let mut g = glyphs(&[
        ("a", 0, 0, 10_000),
        ("b", 5_000, 0, 10_000),
        ("c", 10_000, 0, 10_000),
        ("d", 15_000, 2_000, 10_000),
        ("e", 20_000, 2_000, 10_000),
    ]);
    g[1].font = BOLD;
    let r = runs_of(&g);
    assert_eq!(texts(&r), ["a", "b", "c", "de"]);
    assert!(r[1].style.bold && !r[0].style.bold);
    let annots = [UriAnnot {
        rect: [19_000, 1_000, 30_000, 3_000],
        uri: "u".to_owned(),
    }];
    let linked = merge_runs(&g, &fonts(), &annots, false);
    assert_eq!(texts(&linked), ["a", "b", "c", "d", "e"]);
    assert_eq!(linked[4].link, Some(0));
}

#[test]
fn pass2_collapses_whitespace_and_drops_controls() {
    let g = glyphs(&[
        ("a", 0, 0, 10_000),
        (" ", 5_000, 0, 10_000),
        ("\t", 10_000, 0, 10_000),
        ("\u{7}", 15_000, 0, 10_000),
        ("b", 20_000, 0, 10_000),
    ]);
    assert_eq!(texts(&runs_of(&g)), ["a b"]);
}

#[test]
fn pass2_right_to_left_runs_keep_logical_order() {
    let g = glyphs(&[("א", 20_000, 0, 10_000), ("ב", 15_000, 0, 10_000)]);
    assert_eq!(texts(&merge_runs(&g, &fonts(), &[], true)), ["אב"]);
    // Hebrew steps left on a left-to-right page too.
    assert_eq!(texts(&runs_of(&g)), ["אב"]);
    // Latin stepping back a full em is not a run, on either page.
    let g = glyphs(&[("a", 20_000, 0, 10_000), ("b", 15_000, 0, 10_000)]);
    assert_eq!(texts(&runs_of(&g)), ["a", "b"]);
    assert_eq!(texts(&merge_runs(&g, &fonts(), &[], true)), ["a", "b"]);
    // Digits at Helvetica's 556 milli-em step right on a right-to-left page:
    // one run, not four runs a reversed sort could scramble.
    let mut g = glyphs(&[
        ("2", 0, 0, 10_000),
        ("0", 5_560, 0, 10_000),
        ("2", 11_120, 0, 10_000),
        ("4", 16_680, 0, 10_000),
    ]);
    for glyph in &mut g {
        glyph.advance = 556;
    }
    assert_eq!(texts(&merge_runs(&g, &fonts(), &[], true)), ["2024"]);
}

#[test]
fn pass3_joins_baselines_within_four_tenths_of_the_size() {
    let lines = |items: &[(&str, i64, i64, i64)]| -> Vec<Vec<String>> {
        let spans = runs_of(&glyphs(items));
        let all: Vec<usize> = (0..spans.len()).collect();
        cluster_lines(&spans, &all, false)
            .iter()
            .map(|l| l.runs.iter().map(|&i| spans[i].text.clone()).collect())
            .collect()
    };
    // A superscript 0.39 em up joins its line; the line above is apart.
    assert_eq!(
        lines(&[
            ("a", 0, 100_000, 10_000),
            ("2", 30_000, 103_900, 10_000),
            ("c", 0, 88_000, 10_000),
            ("b", 60_000, 112_000, 10_000),
        ]),
        [vec!["b"], vec!["a", "2"], vec!["c"]]
    );
    // 0.4 em apart: two lines.
    assert_eq!(
        lines(&[("a", 0, 100_000, 10_000), ("b", 30_000, 104_000, 10_000)]),
        [vec!["b"], vec!["a"]]
    );
}

/// Spans for whole words: `(text, x, baseline)` at 10 pt.
fn word_spans(words: &[(&str, i64, i64)]) -> Vec<Span> {
    let items: Vec<(&str, i64, i64, i64)> =
        words.iter().map(|&(t, x, y)| (t, x, y, 10_000)).collect();
    // Each word is one glyph wide enough for its text.
    let mut g = glyphs(&items);
    for (glyph, (t, ..)) in g.iter_mut().zip(words) {
        glyph.advance = 500 * t.chars().count() as i64;
    }
    runs_of(&g)
}

fn leaves(spans: &[Span], rtl: bool) -> Vec<Vec<&str>> {
    reading_order(spans, 10_000, rtl)
        .iter()
        .map(|lines| {
            lines
                .iter()
                .flat_map(|l| l.runs.iter().map(|&i| spans[i].text.as_str()))
                .collect()
        })
        .collect()
}

#[test]
fn pass5_needs_two_lines_a_side_for_a_column() {
    // One line with a wide gap: not a gutter.
    let one = word_spans(&[("left", 0, 100_000), ("right", 200_000, 100_000)]);
    assert_eq!(leaves(&one, false), [vec!["left", "right"]]);
    // Two lines a side, drawn column by column: two columns.
    let two = word_spans(&[
        ("l1", 0, 100_000),
        ("l2", 0, 88_000),
        ("r1", 200_000, 100_000),
        ("r2", 200_000, 88_000),
    ]);
    assert_eq!(leaves(&two, false), [vec!["l1", "l2"], vec!["r1", "r2"]]);
    assert_eq!(leaves(&two, true), [vec!["r1", "r2"], vec!["l1", "l2"]]);
}

#[test]
fn pass5_does_not_cut_rows_drawn_across_the_gap() {
    let rows = word_spans(&[
        ("l1", 0, 100_000),
        ("r1", 200_000, 100_000),
        ("l2", 0, 88_000),
        ("r2", 200_000, 88_000),
        ("l3", 0, 76_000),
        ("r3", 200_000, 76_000),
    ]);
    assert_eq!(leaves(&rows, false).len(), 1);
}

#[test]
fn pass5_bands_split_at_a_blank_line() {
    // Pitch 12 pt; the gap after b is two pitches.
    let spans = word_spans(&[
        ("a", 0, 100_000),
        ("b", 0, 88_000),
        ("c", 0, 64_000),
        ("d", 0, 52_000),
    ]);
    assert_eq!(leaves(&spans, false), [vec!["a", "b"], vec!["c", "d"]]);
}

#[test]
fn pass5_recursion_stops_at_the_depth_cap() {
    let spans = word_spans(&[("a", 0, 0), ("b", 0, -100_000)]);
    let cut = Cut {
        spans: &spans,
        rtl: false,
        median_em: 10_000,
        column_gap: 15_000,
        band_gap: 1,
    };
    let mut out = Vec::new();
    cut.split(vec![0, 1], MAX_CUT_DEPTH, &mut out);
    assert_eq!(out, [vec![0, 1]]);
    out.clear();
    cut.split(vec![0, 1], MAX_CUT_DEPTH - 1, &mut out);
    assert_eq!(out, [vec![0], vec![1]]);
}

#[test]
fn pass6_direction_is_the_majority_of_mapped_ink() {
    let mixed = |rtl: usize, ltr: usize| {
        let mut items = vec![("ا", 0, 0, 10_000); rtl];
        items.extend(vec![("a", 0, 0, 10_000); ltr]);
        items.extend([(" ", 0, 0, 10_000); 9]);
        is_rtl(&glyphs(&items))
    };
    assert!(mixed(3, 2));
    assert!(!mixed(2, 2));
    assert!(mixed(1, 0));
    assert!(!mixed(0, 0));
    // Digits have no direction here, Arabic-Indic ones included: six Arabic
    // letters and seven digits are an Arabic page, and two Latin letters
    // outvote one Arabic letter however many Arabic-Indic digits follow.
    let mut phone = vec![("ر", 0, 0, 10_000); 6];
    phone.extend([("5", 0, 0, 10_000); 7]);
    assert!(is_rtl(&glyphs(&phone)));
    let mut indic = vec![
        ("a", 0, 0, 10_000),
        ("b", 0, 0, 10_000),
        ("ر", 0, 0, 10_000),
    ];
    indic.extend([("٥", 0, 0, 10_000); 5]);
    assert!(!is_rtl(&glyphs(&indic)));
}

#[test]
fn pass7_headings_need_size_or_weight_and_levels_go_by_size() {
    assert!(is_heading(12_700, true, 11_000)); // 1.155 and bold
    assert!(!is_heading(12_600, true, 11_000)); // 1.145
    assert!(!is_heading(15_300, false, 11_000)); // 1.39, not bold
    assert!(is_heading(15_400, false, 11_000)); // 1.4
    let levels = heading_levels([30_000, 24_000, 18_000, 14_000].into_iter().collect());
    let got: Vec<(i64, u8)> = levels.into_iter().collect();
    assert_eq!(got, [(14_000, 3), (18_000, 3), (24_000, 2), (30_000, 1)]);
}

#[test]
fn pass7_a_single_line_page_is_never_a_heading() {
    let mut g = Vec::new();
    ltr(&mut g, "Alone", 72.0, 700.0, 24.0, BOLD);
    let bold = Run {
        bold: true,
        ..plain("Alone")
    };
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![Block::Paragraph(vec![Line(vec![bold])])]
    );
}

#[test]
fn pass7_a_bold_line_slightly_larger_is_a_heading_and_two_lines_merge() {
    let mut g = Vec::new();
    ltr(&mut g, "Bold head", 72.0, 720.0, 13.0, BOLD);
    ltr(&mut g, "continued", 72.0, 704.4, 13.0, BOLD);
    body(&mut g, &["Body text here.", "More body text."], 72.0, 680.0);
    let bold = |t: &str| Run {
        bold: true,
        ..plain(t)
    };
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![
            Block::Heading {
                level: 1,
                runs: vec![bold("Bold head continued")],
            },
            para(&["Body text here.", "More body text."]),
        ]
    );
}

#[test]
fn pass8_markers() {
    for (text, want) in [
        ("• a", Some((2, false))),
        ("◦ a", Some((2, false))),
        ("- a", Some((2, false))),
        ("– a", Some((2, false))),
        ("* a", Some((2, false))),
        ("1. a", Some((3, true))),
        ("12) a", Some((4, true))),
        ("b) a", Some((3, true))),
        ("(c) a", Some((4, true))),
        ("-a", None),
        ("1.5 kg", None),
        ("a)b", None),
        ("•", None),
        ("- ", None),
        ("word", None),
        ("1234567890. too long", None),
    ] {
        assert_eq!(list_marker(text), want, "{text:?}");
    }
}

#[test]
fn pass8_lists_split_on_kind_and_take_indented_continuations() {
    let mut g = Vec::new();
    body(&mut g, &["- one"], 72.0, 700.0);
    body(&mut g, &["wraps here"], 83.0, 700.0 - PITCH);
    body(
        &mut g,
        &["1. first", "2. second"],
        72.0,
        700.0 - 2.0 * PITCH,
    );
    let line = |t: &str| Line(vec![plain(t)]);
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![
            Block::List {
                ordered: false,
                items: vec![vec![line("one"), line("wraps here")]],
            },
            Block::List {
                ordered: true,
                items: vec![vec![line("first")], vec![line("second")]],
            },
        ]
    );
}

#[test]
fn pass9_needs_three_rows_and_two_channels() {
    let grid = |rows: usize, cols: usize| {
        let mut words = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                words.push(("x", c as i64 * 100_000, 100_000 - r as i64 * 14_000));
            }
        }
        let spans = word_spans(&words);
        let all: Vec<usize> = (0..spans.len()).collect();
        let lines = cluster_lines(&spans, &all, false);
        find_tables(&spans, &lines, 10_000)
    };
    assert!(grid(2, 3).is_empty());
    assert!(grid(3, 2).is_empty());
    let t = grid(4, 3);
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].start, t[0].end, t[0].cuts.len()), (0, 4, 2));
}

#[test]
fn pass9_ragged_cells_share_a_channel_within_half_an_em() {
    // Left-aligned columns at 0, 100 and 200 pt; cell widths vary.
    let spans = word_spans(&[
        ("short", 0, 100_000),
        ("b", 100_000, 100_000),
        ("c", 200_000, 100_000),
        ("a much longer cell", 0, 86_000),
        ("b", 100_000, 86_000),
        ("c", 204_000, 86_000), // 0.4 em right of the others
        ("a", 0, 72_000),
        ("b", 100_000, 72_000),
        ("c", 200_000, 72_000),
    ]);
    let all: Vec<usize> = (0..spans.len()).collect();
    let lines = cluster_lines(&spans, &all, false);
    let t = find_tables(&spans, &lines, 10_000);
    assert_eq!(t.len(), 1);
    let rows: Vec<Vec<String>> = lines
        .iter()
        .map(|l| table_row(&spans, l, &t[0].cuts, false))
        .collect();
    assert_eq!(rows[1], ["a much longer cell", "b", "c"]);
}

#[test]
fn pass10_rect_corners_in_either_order() {
    let annot = |rect| UriAnnot {
        rect,
        uri: "u".to_owned(),
    };
    let annots = [annot([10, 10, 0, 0]), annot([0, 0, 50, 50])];
    assert_eq!(link_at(&annots, 5, 5), Some(0));
    assert_eq!(link_at(&annots, 10, 0), Some(0));
    assert_eq!(link_at(&annots, 20, 20), Some(1));
    assert_eq!(link_at(&annots, 51, 20), None);
}

#[test]
fn pass11_images_follow_the_text_in_the_order_given() {
    let l = analyse(
        &list_page(&["Caption"]),
        &fonts(),
        &[],
        &[image("p1-1.jpg"), image("p1-2.jpg")],
        &[],
    );
    let names: Vec<&str> = l
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(l.blocks[0], para(&["Caption"]));
    assert_eq!(names, ["p1-1.jpg", "p1-2.jpg"]);
    // A page with text and images is not image-only.
    assert!(l.notes.is_empty());
}

#[test]
fn pass12_outlined_findings_on_one_page_add_up() {
    let outlined = |p, c| {
        finding(
            FindingKind::OutlinedText {
                glyph_runs: p,
                contours: c,
            },
            0,
        )
    };
    let page = page_of(Vec::new());
    assert_eq!(
        notes(&page, &[outlined(2, 3), outlined(4, 5)], &[], 0),
        [LayoutNote::OutlinedText {
            paths: 6,
            contours: 8,
        }]
    );
    // No glyphs and no images: a blank page, not an image-only one.
    assert!(notes(&page, &[], &[], 0).is_empty());
}

#[test]
fn a_rule_line_is_a_rule() {
    assert!(is_rule("-----"));
    assert!(is_rule("— — —"));
    assert!(!is_rule("--"));
    assert!(!is_rule("-=-"));
    assert!(!is_rule("---a"));
    let mut g = Vec::new();
    body(&mut g, &["Above", "_____", "Below"], 72.0, 700.0);
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![para(&["Above"]), Block::Rule, para(&["Below"])]
    );
}

#[test]
fn styled_runs_keep_their_spaces_outside_the_style() {
    let mut g = Vec::new();
    ltr(&mut g, "a ", 72.0, 700.0, BODY, REGULAR);
    ltr(&mut g, "bold", 83.0, 700.0, BODY, BOLD);
    ltr(&mut g, " c", 105.0, 700.0, BODY, REGULAR);
    body(&mut g, &["next line"], 72.0, 700.0 - PITCH);
    let l = layout(&page_of(g));
    let Block::Paragraph(lines) = &l.blocks[0] else {
        panic!("{l:?}");
    };
    let bold = Run {
        bold: true,
        ..plain("bold")
    };
    assert_eq!(lines[0], Line(vec![plain("a "), bold, plain(" c")]));
}

// ── review fixes: mixed direction, hostile sizes, guards ────────────────

/// One piece of a line, in logical order: its text, its font and whether
/// it is a left-to-right unit (a number, a Latin word).
type Piece<'a> = (&'a str, FontKey, bool);

/// A line laid out the way the Unicode bidi algorithm lays out a
/// right-to-left paragraph: pieces in logical order from `right` leftwards,
/// each left-to-right piece a unit of its own whose glyphs run left to right,
/// every other glyph stepping left. Glyphs are emitted in logical order at
/// `advance` milli-em. The caller splits the pieces as the algorithm does: a
/// number and a Latin word with a space between are two units, the space
/// right to left.
fn rtl_line(out: &mut Vec<GlyphItem>, pieces: &[Piece], right: f64, y: f64, advance: f32) {
    let w = BODY * f64::from(advance) / 1000.0;
    let mut pen = right;
    for &(text, font, ltr) in pieces {
        let n = text.chars().count() as f64;
        for (i, c) in text.chars().enumerate() {
            let x = if ltr {
                pen - n * w + i as f64 * w
            } else {
                pen - (i + 1) as f64 * w
            };
            out.push(GlyphItem {
                advance: Some(advance),
                ..glyph(&c.to_string(), x, y, BODY, font)
            });
        }
        pen -= n * w;
    }
}

fn line_strings(l: &PageLayout) -> Vec<String> {
    l.blocks.iter().flat_map(block_lines).collect()
}

/// Helvetica digits are 556 milli-em; a full em is the extreme.
const ADVANCES: [f32; 5] = [300.0, 500.0, 556.0, 600.0, 1000.0];

#[test]
fn numbers_and_latin_words_on_an_arabic_line_keep_their_order() {
    // "مرحبا بالعالم 2024 PDF نص": 2024 is drawn right of PDF.
    for advance in ADVANCES {
        for pdf_font in [REGULAR, BOLD] {
            let mut g = Vec::new();
            rtl_line(
                &mut g,
                &[
                    ("مرحبا بالعالم ", REGULAR, false),
                    ("2024", REGULAR, true),
                    (" ", REGULAR, false),
                    ("PDF", pdf_font, true),
                    (" نص", REGULAR, false),
                ],
                540.0,
                700.0,
                advance,
            );
            let l = layout(&page_of(g));
            assert_eq!(
                line_strings(&l),
                ["مرحبا بالعالم 2024 PDF نص"],
                "advance {advance}: {l:?}"
            );
        }
    }
}

#[test]
fn a_phone_number_on_an_arabic_line_keeps_its_digit_groups_in_order() {
    // "رقم الهاتف 555 1234 للتواصل": 555 is drawn right of 1234.
    for advance in ADVANCES {
        let mut g = Vec::new();
        rtl_line(
            &mut g,
            &[
                ("رقم الهاتف ", REGULAR, false),
                ("555", REGULAR, true),
                (" ", REGULAR, false),
                ("1234", REGULAR, true),
                (" للتواصل", REGULAR, false),
            ],
            540.0,
            700.0,
            advance,
        );
        let l = layout(&page_of(g));
        assert_eq!(
            line_strings(&l),
            ["رقم الهاتف 555 1234 للتواصل"],
            "advance {advance}: {l:?}"
        );
    }
}

/// An English line from `start` holding a right-to-left stretch: `before`,
/// then `stretch` (logical order, each piece a unit as in [`rtl_line`],
/// placed right to left), then `after`. Glyphs in logical order.
fn ltr_line_with_rtl(
    out: &mut Vec<GlyphItem>,
    (before, stretch, after): (&str, &[Piece], &str),
    start: f64,
    advance: f32,
) {
    let w = BODY * f64::from(advance) / 1000.0;
    let at = |out: &mut Vec<GlyphItem>, c: char, x: f64, font: FontKey| {
        out.push(GlyphItem {
            advance: Some(advance),
            ..glyph(&c.to_string(), x, 700.0, BODY, font)
        });
    };
    for (i, c) in before.chars().enumerate() {
        at(out, c, start + i as f64 * w, REGULAR);
    }
    let left = start + before.chars().count() as f64 * w;
    let width: usize = stretch.iter().map(|p| p.0.chars().count()).sum();
    let right = left + width as f64 * w;
    rtl_line(out, stretch, right, 700.0, advance);
    for (i, c) in after.chars().enumerate() {
        at(out, c, right + i as f64 * w, REGULAR);
    }
}

#[test]
fn a_hebrew_name_on_an_english_line_keeps_its_order() {
    for advance in ADVANCES {
        let mut g = Vec::new();
        ltr_line_with_rtl(
            &mut g,
            (
                "Dear ",
                &[("שלום", BOLD, false)],
                " friend, and many more words",
            ),
            72.0,
            advance,
        );
        assert_eq!(
            line_strings(&layout(&page_of(g))),
            ["Dear שלום friend, and many more words"],
            "advance {advance}"
        );
    }
}

#[test]
fn a_number_after_a_hebrew_word_on_an_english_line_keeps_its_place() {
    // "Dear שלום 2024 friend": the number joins the Hebrew stretch and is
    // drawn left of the name ("Dear 2024 שלום friend" on the page).
    for advance in ADVANCES {
        let mut g = Vec::new();
        ltr_line_with_rtl(
            &mut g,
            (
                "Dear ",
                &[
                    ("שלום", REGULAR, false),
                    (" ", REGULAR, false),
                    ("2024", REGULAR, true),
                ],
                " friend, and many more words",
            ),
            72.0,
            advance,
        );
        assert_eq!(
            line_strings(&layout(&page_of(g))),
            ["Dear שלום 2024 friend, and many more words"],
            "advance {advance}"
        );
    }
}

#[test]
fn pass6_a_mixed_stretch_reads_in_content_order_and_a_wide_gap_parts_it() {
    // Content order is logical; x order is the bidi layout's.
    let spans = word_spans(&[
        // Runs a full em or more apart in content order, so each is its
        // own; 10 pt, half-em glyphs.
        ("عربي", 305_000, 0),
        ("one", 260_000, 0),
        ("two", 287_000, 0),
        ("نص", 245_000, 0),
        // 1.2 em past the stretch: still part of it.
        ("ثم", 223_000, 0),
        // 1.8 em past it: a second stretch, in the page's order.
        ("و", 180_000, 0),
        ("لا", 195_000, 0),
    ]);
    let all: Vec<usize> = (0..spans.len()).collect();
    let lines = cluster_lines(&spans, &all, true);
    let order: Vec<&str> = lines[0]
        .runs
        .iter()
        .map(|&i| spans[i].text.as_str())
        .collect();
    assert_eq!(order, ["عربي", "one", "two", "نص", "ثم", "لا", "و"]);
    assert_eq!(dir_of("٢٠٢٤"), Dir::Ltr);
    assert_eq!(dir_of(" (x"), Dir::Ltr);
    assert_eq!(dir_of("…"), Dir::Neutral);
}

#[test]
fn pass1_is_near_linear_on_a_hundred_thousand_identical_glyphs() {
    // 100k copies of "a" on a 2 pt grid, none a shadow of another: the
    // earlier all-pairs scan took seconds here in a release build.
    let items: Vec<(&str, i64, i64, i64)> = (0..100_000)
        .map(|k| ("a", (k % 400) * 2_000, (k / 400) * 2_000, 10_000))
        .collect();
    let mut g = glyphs(&items);
    let n = g.len();
    // Every glyph also gets a fake-bold copy 0.3 pt away.
    let copies: Vec<Glyph> = g
        .iter()
        .map(|o| Glyph {
            seq: o.seq + n,
            x: o.x + 300,
            y: o.y - 300,
            ..o.clone()
        })
        .collect();
    g.extend(copies);
    assert_eq!(drop_shadows(g).len(), n);
}

#[test]
fn a_hundred_thousand_glyph_page_finishes() {
    let mut g = Vec::new();
    for row in 0..250 {
        for col in 0..400 {
            g.push(glyph(
                "a",
                f64::from(col) * 20.0,
                5_000.0 - f64::from(row) * 13.2,
                BODY,
                REGULAR,
            ));
        }
    }
    let l = layout(&page_of(g));
    let ink: usize = line_strings(&l)
        .iter()
        .map(|s| s.chars().filter(|c| *c == 'a').count())
        .sum();
    assert_eq!(ink, 100_000);
}

#[test]
fn pass5_a_scrambled_staircase_of_ten_thousand_spans_finishes() {
    // Every span is its own line and every gap is a candidate gutter; the
    // content order is scrambled so every candidate is refused. Trying each
    // gap took seconds in a release build; only the widest few are tried.
    let mut rng = Rng(7);
    let mut words: Vec<(&str, i64, i64)> = (0..10_000)
        .map(|k| ("x", k * 20_000, -k * 12_000))
        .collect();
    for k in (1..words.len()).rev() {
        words.swap(k, rng.below(k as u64 + 1) as usize);
    }
    let spans = word_spans(&words);
    let total: usize = leaves(&spans, false).iter().map(Vec::len).sum();
    assert_eq!(total, 10_000);
}

#[test]
fn pass5_two_columns_drawn_line_by_line_across_the_gutter_read_across() {
    // The documented cost of the content-order guard (Cut::interleaved):
    // with nothing but geometry this page is two columns, and so is a
    // two-column key/value table; the guard reads both row by row.
    let left = ["Left one.", "Left two.", "Left three.", "Left four."];
    let right = ["Right one.", "Right two.", "Right three.", "Right four."];
    let mut g = Vec::new();
    for (i, (l, r)) in left.iter().zip(right).enumerate() {
        let y = 700.0 - i as f64 * PITCH;
        ltr(&mut g, l, 72.0, y, BODY, REGULAR);
        ltr(&mut g, r, 340.0, y, BODY, REGULAR);
    }
    let l = layout(&page_of(g));
    assert_eq!(l.blocks.len(), 1, "{l:?}");
    let want: Vec<String> = left
        .iter()
        .zip(right)
        .map(|(l, r)| format!("{l} {r}"))
        .collect();
    assert_eq!(line_strings(&l), want);
}

#[test]
fn pass5_a_grid_drawn_column_by_column_is_still_a_table() {
    // The content-order guard lets this cut through; the grid guard does not.
    let mut g = Vec::new();
    for (c, col) in ["A", "B", "C"].iter().enumerate() {
        for r in 0..3 {
            ltr(
                &mut g,
                &format!("{col}{}", r + 1),
                72.0 + c as f64 * 100.0,
                700.0 - r as f64 * 14.0,
                BODY,
                REGULAR,
            );
        }
    }
    let cells = |r: &str| ["A", "B", "C"].map(|c| format!("{c}{r}")).to_vec();
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![Block::Table {
            rows: vec![cells("1"), cells("2"), cells("3")],
            header: false,
        }]
    );
}

#[test]
fn pass5_three_columns_of_prose_on_shared_baselines_read_column_by_column() {
    // Every merged line has the two gutters as cell gaps, so the region
    // lines up as a table; its cells are whole lines of prose, not a grid.
    let col = |name: &str| ["one", "two", "three", "four"].map(|n| format!("{name} {n} is here."));
    let (alpha, beta, gamma) = (col("Alpha"), col("Beta"), col("Gamma"));
    let mut g = Vec::new();
    for (x, lines) in [(50.0, &alpha), (230.0, &beta), (410.0, &gamma)] {
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        body(&mut g, &lines, x, 700.0);
    }
    let para_of = |lines: &[String; 4]| para(&lines.each_ref().map(String::as_str));
    assert_eq!(
        layout(&page_of(g)).blocks,
        vec![para_of(&alpha), para_of(&beta), para_of(&gamma)]
    );
}

#[test]
fn pass5_late_footnote_markers_do_not_merge_two_columns() {
    // Two columns drawn one after the other, then four superscript markers
    // drawn last, alternating sides: five crossings, well short of the seven
    // a page drawn row by row has.
    let left = ["Left a.", "Left b.", "Left c.", "Left d."];
    let right = ["Right a.", "Right b.", "Right c.", "Right d."];
    let mut g = Vec::new();
    body(&mut g, &left, 72.0, 700.0);
    body(&mut g, &right, 340.0, 700.0);
    let end = |x: f64, s: &str| x + s.chars().count() as f64 * BODY * 0.5;
    for (n, (x, row)) in [
        (end(72.0, left[0]), 0.0),
        (end(340.0, right[0]), 0.0),
        (end(72.0, left[1]), 1.0),
        (end(340.0, right[1]), 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 700.0 - row * PITCH + 3.0;
        ltr(&mut g, &(n + 1).to_string(), x, y, 7.0, REGULAR);
    }
    let l = layout(&page_of(g));
    assert_eq!(
        line_strings(&l),
        [
            "Left a.1",
            "Left b.3",
            "Left c.",
            "Left d.",
            "Right a.2",
            "Right b.4",
            "Right c.",
            "Right d."
        ],
        "{l:?}"
    );
    assert_eq!(l.blocks.len(), 2, "{l:?}");
}

#[test]
fn pass5_a_small_table_inside_a_column_does_not_block_the_gutter() {
    let mut g = Vec::new();
    let left = [
        "Left column prose line one.",
        "Left column prose line two.",
        "Left column prose line three.",
        "Left column prose line four.",
    ];
    let right = [
        "Right column prose one.",
        "Right column prose two.",
        "Right column prose three.",
        "Right column prose four.",
        "Right column prose five.",
        "Right column prose six.",
        "Right column prose seven.",
    ];
    body(&mut g, &left, 72.0, 700.0);
    // A 3×3 table under the left prose, inside the left column.
    for r in 0..3 {
        for c in 0..3 {
            ltr(
                &mut g,
                &format!("t{r}{c}"),
                72.0 + c as f64 * 60.0,
                700.0 - (4 + r) as f64 * PITCH,
                BODY,
                REGULAR,
            );
        }
    }
    body(&mut g, &right, 340.0, 700.0);
    let l = layout(&page_of(g));
    assert!(
        matches!(l.blocks.last(), Some(Block::Paragraph(lines)) if lines.len() == right.len()),
        "{l:?}"
    );
    assert!(l.blocks.iter().any(|b| matches!(b, Block::Table { .. })));
}

#[test]
fn pass7_distinct_headings_back_to_back_stay_apart() {
    let mut g = Vec::new();
    ltr(&mut g, "Chapter 2", 72.0, 720.0, 18.0, BOLD);
    // Two sizes lower: a second heading, not a wrapped line.
    ltr(&mut g, "Background", 72.0, 684.0, 18.0, BOLD);
    // A centred title wrapping onto a shorter line is one heading.
    ltr(&mut g, "A centred title that", 200.0, 640.0, 18.0, BOLD);
    ltr(&mut g, "wraps", 267.5, 619.0, 18.0, BOLD);
    body(
        &mut g,
        &[
            "Body text here, long enough to outweigh the headings.",
            "More body text, so that eleven points is the body size.",
        ],
        72.0,
        590.0,
    );
    let heads: Vec<String> = layout(&page_of(g))
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Heading { .. }))
        .flat_map(block_lines)
        .collect();
    assert_eq!(
        heads,
        ["Chapter 2", "Background", "A centred title that wraps"]
    );
}

#[test]
fn pass4_a_glyph_mapped_only_to_controls_is_counted() {
    let mut g = glyphs(&[
        ("a", 0, 0, 10_000),
        ("\u{7}", 5_000, 0, 10_000),
        ("\u{0}\u{1b}", 10_000, 0, 10_000),
        ("\t", 15_000, 0, 10_000),
        ("", 20_000, 0, 10_000),
    ]);
    g.push(Glyph {
        text: Some("b".to_owned()),
        ..g[0].clone()
    });
    let (kept, hidden) = drop_unprintable(g);
    // The tab is a space and the empty text carries nothing.
    assert_eq!(kept.iter().map(|g| g.seq).collect::<Vec<_>>(), [0, 3, 4, 0]);
    assert_eq!(hidden, 2);
    let mut page = Vec::new();
    body(&mut page, &["Seen"], 72.0, 700.0);
    page.push(glyph("\u{3}", 100.0, 700.0, BODY, REGULAR));
    assert_eq!(
        layout(&page_of(page)).notes,
        vec![LayoutNote::Unmapped { glyphs: 1 }]
    );
}
