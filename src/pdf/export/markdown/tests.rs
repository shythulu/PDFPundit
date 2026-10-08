use super::*;

use std::collections::BTreeMap;

use crate::pdf::export::layout::{UriAnnot, analyse};
use crate::pdf::export::layouts;
use crate::pdf::export::links::with_link;
use crate::pdf::fixtures::{GOLDEN_TEXT, TINY_JPEG, golden_pdf};
use crate::pdf::model::{Finding, FindingKind, Location, PaintCounts, Repairability, Severity};
use crate::pdf::text::{FontInfo, FontKey, FontTable, GlyphItem, PageText};

// ── synthetic pages ─────────────────────────────────────────────────────

const REGULAR: FontKey = FontKey(1);
const BODY: f64 = 11.0;
const PITCH: f64 = 13.2;

fn fonts() -> FontTable {
    BTreeMap::from([(
        REGULAR,
        FontInfo {
            base_font: Some("Helvetica".to_owned()),
            subtype: "TrueType".to_owned(),
            flags: Some(32),
            postscript_name: Some("Helvetica".to_owned()),
            weight: Some(400),
            italic: false,
            serif: false,
            mono: false,
        },
    )])
}

/// `lines` drawn left to right from `x`, top line at `top`, one glyph per
/// char, half an em apart.
fn body(out: &mut Vec<GlyphItem>, lines: &[&str], x: f64, top: f64) {
    for (row, s) in lines.iter().enumerate() {
        for (i, c) in s.chars().enumerate() {
            out.push(GlyphItem {
                text: Some(c.to_string()),
                x: x + i as f64 * BODY * 0.5,
                y: top - row as f64 * PITCH,
                size: BODY,
                advance: Some(500.0),
                font: REGULAR,
                invisible: false,
                type3: false,
                mcid: None,
            });
        }
    }
}

fn page_of(index: u32, glyphs: Vec<GlyphItem>) -> PageText {
    PageText {
        index,
        width: 612.0,
        height: 792.0,
        glyphs,
        unmapped: 0,
        paint: PaintCounts::default(),
        warnings: Vec::new(),
    }
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

fn line(text: &str) -> Line {
    Line(vec![plain(text)])
}

fn page(blocks: Vec<Block>) -> PageLayout {
    PageLayout {
        index: 0,
        width_pt: 612,
        height_pt: 792,
        blocks,
        notes: Vec::new(),
    }
}

fn md(blocks: Vec<Block>) -> String {
    to_markdown(&[page(blocks)], &MarkdownOptions::default())
}

fn outlined(index: u32) -> Finding {
    Finding {
        id: "OUTLINE-001".to_owned(),
        class: FindingKind::OutlinedText {
            glyph_runs: 9,
            contours: 412,
        },
        severity: Severity::Info,
        location: Location::Page { index, obj: None },
        summary: String::new(),
        evidence: Vec::new(),
        repair: Repairability::NotApplicable,
    }
}

// ── acceptance ──────────────────────────────────────────────────────────

/// The golden's Markdown, its page-one image in `golden.0123abcd.images`. A
/// change here is a change in what export writes: re-baseline only with a
/// reason (a T-32a threshold tuning is a re-baseline, not a design change).
const GOLDEN_MD: &str = include_str!("../../../../tests/data/export/golden.md");

fn golden_markdown() -> String {
    let images = vec![("p1-1.jpg".to_owned(), TINY_JPEG.to_vec())];
    let pages = layouts(&golden_pdf(), &[], &images).expect("the golden loads");
    to_markdown(
        &pages,
        &MarkdownOptions {
            page_separators: true,
            image_dir_name: Some("golden.0123abcd.images".to_owned()),
        },
    )
}

#[test]
fn the_golden_markdown_is_the_committed_one() {
    let out = golden_markdown();
    assert_eq!(out, GOLDEN_MD, "got:\n{out}");
    for lines in GOLDEN_TEXT {
        for l in lines {
            assert!(out.contains(l), "{l:?} is in the export");
        }
    }
}

#[test]
fn two_columns_read_column_by_column() {
    let mut g = Vec::new();
    body(&mut g, GOLDEN_TEXT[0], 72.0, 700.0);
    body(&mut g, GOLDEN_TEXT[1], 340.0, 700.0);
    let layout = analyse(&page_of(0, g), &fonts(), &[], &[], &[]);
    let out = to_markdown(&[layout], &MarkdownOptions::default());
    let expected = format!(
        "{}\n\n{}\n",
        GOLDEN_TEXT[0].join("\n"),
        GOLDEN_TEXT[1].join("\n")
    );
    assert_eq!(out, expected);
}

#[test]
fn a_table_is_a_pipe_table() {
    let cells = |r: &str| ["A", "B", "C"].map(|c| format!("{c}{r}")).to_vec();
    let rows = vec![cells("1"), cells("2"), cells("3")];
    assert_eq!(
        md(vec![Block::Table {
            rows: rows.clone(),
            header: true,
        }]),
        "| A1 | B1 | C1 |\n| --- | --- | --- |\n| A2 | B2 | C2 |\n| A3 | B3 | C3 |\n"
    );
    assert_eq!(
        md(vec![Block::Table {
            rows,
            header: false
        }]),
        "|  |  |  |\n| --- | --- | --- |\n\
         | A1 | B1 | C1 |\n| A2 | B2 | C2 |\n| A3 | B3 | C3 |\n"
    );
}

#[test]
fn table_cells_escape_pipes_and_ragged_rows_are_padded() {
    let rows = vec![
        vec!["a|b".to_owned(), "*x*".to_owned()],
        vec!["only".to_owned()],
    ];
    assert_eq!(
        md(vec![Block::Table { rows, header: true }]),
        "| a\\|b | \\*x\\* |\n| --- | --- |\n| only |  |\n"
    );
}

#[test]
fn an_outlined_page_says_its_text_was_not_extracted() {
    let mut g = Vec::new();
    body(&mut g, &["Some text."], 72.0, 700.0);
    let layout = analyse(&page_of(0, g), &fonts(), &[outlined(0)], &[], &[]);
    assert_eq!(
        to_markdown(&[layout], &MarkdownOptions::default()),
        "<!-- text drawn as outlines on this page (9 paths, 412 contours) \
         was not extracted -->\n\nSome text.\n"
    );
}

#[test]
fn every_note_is_a_visible_comment() {
    let mut p = page(Vec::new());
    p.notes = vec![
        LayoutNote::OutlinedText {
            paths: 1,
            contours: 1,
        },
        LayoutNote::Type3Text,
        LayoutNote::ImageOnly,
        LayoutNote::Unmapped { glyphs: 3 },
        LayoutNote::Unmapped { glyphs: 1 },
    ];
    let out = to_markdown(&[p], &MarkdownOptions::default());
    let lines: Vec<&str> = out.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines,
        [
            "<!-- text drawn as outlines on this page (1 path, 1 contour) was not extracted -->",
            "<!-- this page draws text with Type 3 glyph procedures; \
             text they do not map to Unicode was not extracted -->",
            "<!-- this page is images only; any text in them was not extracted -->",
            "<!-- 3 glyphs on this page were not extracted \
             (no Unicode mapping, invisible, or under 2 pt) -->",
            "<!-- 1 glyph on this page was not extracted \
             (no Unicode mapping, invisible, or under 2 pt) -->",
        ]
    );
}

#[test]
fn a_link_annotation_becomes_a_markdown_link() {
    // Page two's first line, "Le garçon a mangé …", from x = 72 pt.
    let pdf = with_link(
        &golden_pdf(),
        1,
        [60.0, 600.0, 560.0, 760.0],
        "https://example.org/a b",
    );
    let pages = layouts(&pdf, &[], &[]).expect("loads");
    let out = to_markdown(&pages[1..], &MarkdownOptions::default());
    let first = out.lines().next().expect("a line");
    assert_eq!(
        first,
        format!("[{}](https://example.org/a%20b)", GOLDEN_TEXT[1][0])
    );
}

#[test]
fn headings_lists_and_rules() {
    let out = md(vec![
        Block::Heading {
            level: 2,
            runs: vec![plain("Issue #5 ")],
        },
        Block::List {
            ordered: false,
            items: vec![vec![line("one"), line("wraps")], vec![line("two")]],
        },
        Block::List {
            ordered: true,
            items: (1..=10).map(|n| vec![line(&format!("item {n}"))]).collect(),
        },
        Block::Rule,
    ]);
    let numbered: Vec<String> = (1..=10).map(|n| format!("{n}. item {n}")).collect();
    assert_eq!(
        out,
        format!(
            "## Issue \\#5\n\n- one\n  wraps\n- two\n\n{}\n\n---\n",
            numbered.join("\n")
        )
    );
}

#[test]
fn styles_keep_their_spaces_outside_the_markers() {
    let runs = vec![
        plain("a "),
        Run {
            bold: true,
            ..plain("bold ")
        },
        Run {
            italic: true,
            ..plain("it")
        },
        plain(" "),
        Run {
            bold: true,
            italic: true,
            ..plain("both")
        },
        plain(" and "),
        Run {
            mono: true,
            ..plain("a`b")
        },
        plain(" "),
        Run {
            mono: true,
            ..plain("`x")
        },
    ];
    assert_eq!(
        md(vec![Block::Paragraph(vec![Line(runs)])]),
        "a **bold** *it* ***both*** and ``a`b`` `` `x ``\n"
    );
}

#[test]
fn adjacent_runs_of_one_style_and_one_link_merge() {
    let link = Some("https://example.org/".to_owned());
    let runs = vec![
        Run {
            bold: true,
            ..plain("ab")
        },
        Run {
            bold: true,
            ..plain("cd")
        },
        plain(" "),
        Run {
            link: link.clone(),
            ..plain("go ")
        },
        Run {
            link: link.clone(),
            bold: true,
            ..plain("here")
        },
    ];
    assert_eq!(
        md(vec![Block::Paragraph(vec![Line(runs)])]),
        "**abcd** [go **here**](https://example.org/)\n"
    );
}

#[test]
fn only_web_and_mail_links_are_kept() {
    let linked = |uri: &str| Run {
        link: Some(uri.to_owned()),
        ..plain("x")
    };
    let out = md(vec![Block::Paragraph(vec![
        Line(vec![linked("javascript:alert(1)")]),
        Line(vec![linked("MAILTO:a@b.c")]),
        Line(vec![linked("https://e.org/(x)")]),
        Line(vec![linked("file:///etc/passwd")]),
    ])]);
    assert_eq!(out, "x\n[x](MAILTO:a@b.c)\n[x](https://e.org/%28x%29)\nx\n");
}

#[test]
fn pdf_text_never_becomes_markdown_syntax() {
    let lines = [
        "# not a heading",
        "- not a list",
        "+ nor this",
        "1. nor this",
        "2) nor this",
        "=====",
        "    not code",
        "*stars* _under_ [link](x) <b>tag</b> `tick` ~strike~ \\ back",
        "AT&T &amp; &#169; 3.5 million 2024 was",
    ];
    let out = md(vec![Block::Paragraph(
        lines.iter().map(|l| line(l)).collect(),
    )]);
    assert_eq!(
        out,
        "\\# not a heading\n\
         \\- not a list\n\
         \\+ nor this\n\
         1\\. nor this\n\
         2\\) nor this\n\
         \\=====\n\
         not code\n\
         \\*stars\\* \\_under\\_ \\[link\\](x) \\<b\\>tag\\</b\\> \\`tick\\` \\~strike\\~ \\\\ back\n\
         AT&T \\&amp; \\&#169; 3.5 million 2024 was\n"
    );
}

#[test]
fn image_links_point_into_the_images_directory() {
    let image = |name: &str| Block::Image {
        name: name.to_owned(),
        sha256: [0; 32],
        len: 1,
    };
    let opts = MarkdownOptions {
        page_separators: false,
        image_dir_name: Some("my scan.0123abcd (2).images".to_owned()),
    };
    assert_eq!(
        to_markdown(&[page(vec![image("p3-1.jpg")])], &opts),
        "![](my%20scan.0123abcd%20%282%29.images/p3-1.jpg)\n"
    );
    assert_eq!(
        md(vec![image("p3-1.jpg")]),
        "<!-- image p3-1.jpg was not written -->\n"
    );
}

#[test]
fn page_separators_count_from_one_and_empty_blocks_vanish() {
    let mut second = page(vec![Block::Paragraph(vec![line("two")])]);
    second.index = 1;
    let pages = [
        page(vec![
            Block::Paragraph(vec![line("  ")]),
            Block::Heading {
                level: 1,
                runs: Vec::new(),
            },
        ]),
        second,
    ];
    let opts = MarkdownOptions {
        page_separators: true,
        image_dir_name: None,
    };
    assert_eq!(
        to_markdown(&pages, &opts),
        "<!-- page 1 -->\n\n<!-- page 2 -->\n\ntwo\n"
    );
    assert_eq!(to_markdown(&[], &opts), "");
    assert_eq!(to_markdown(&pages[..1], &MarkdownOptions::default()), "");
}

#[test]
fn a_comment_cannot_be_closed_early() {
    let image = Block::Image {
        name: "a-->b".to_owned(),
        sha256: [0; 32],
        len: 1,
    };
    assert_eq!(md(vec![image]), "<!-- image a- ->b was not written -->\n");
}

#[test]
fn the_export_is_deterministic_and_links_follow_the_layout() {
    assert_eq!(golden_markdown(), golden_markdown());
    // A rect in the glyphs' space, so a synthetic page can carry a link.
    let mut g = Vec::new();
    body(&mut g, &["see here"], 72.0, 700.0);
    let annots = [UriAnnot {
        rect: [93_000, 690_000, 115_000, 710_000],
        uri: "https://example.org/".to_owned(),
    }];
    let layout = analyse(&page_of(0, g), &fonts(), &[], &[], &annots);
    assert_eq!(
        to_markdown(&[layout], &MarkdownOptions::default()),
        "see [here](https://example.org/)\n"
    );
}
