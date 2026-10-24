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

/// The golden's Markdown, its page-one image in `0123abcd.images`. A
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
            image_dir_name: Some("0123abcd.images".to_owned()),
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
        image_dir_name: Some("0123abcd (2).images".to_owned()),
    };
    assert_eq!(
        to_markdown(&[page(vec![image("p3-1.jpg")])], &opts),
        "![](0123abcd%20%282%29.images/p3-1.jpg)\n"
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

// ── nothing in the text can embed or fetch (F-02) ───────────────────────

fn linked(text: &str, uri: &str) -> Run {
    Run {
        link: Some(uri.to_owned()),
        ..plain(text)
    }
}

/// What a CommonMark reader sees outside code spans, as `(char, escaped)`:
/// a backslash before ASCII punctuation escapes it, and a backtick string
/// opens a code span that the next backtick string of the same length
/// closes (its contents are literal, so they are left out).
fn outside_code(md: &str) -> Vec<(char, bool)> {
    let chars: Vec<char> = md.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(char::is_ascii_punctuation) {
            out.push((chars[i + 1], true));
            i += 2;
        } else if c == '`' {
            let n = chars[i..].iter().take_while(|&&c| c == '`').count();
            let mut j = i + n;
            let mut close = None;
            while j < chars.len() {
                let m = chars[j..].iter().take_while(|&&c| c == '`').count();
                if m == n {
                    close = Some(j + m);
                    break;
                }
                j += m.max(1);
            }
            match close {
                Some(end) => i = end,
                None => {
                    out.extend(std::iter::repeat_n(('`', false), n));
                    i += n;
                }
            }
        } else {
            out.push((c, false));
            i += 1;
        }
    }
    out
}

/// Every place an unescaped `![` would open an image.
fn images_in(md: &str) -> usize {
    outside_code(md)
        .windows(2)
        .filter(|w| w[0] == ('!', false) && w[1] == ('[', false))
        .count()
}

/// Whether an unescaped `<` (raw HTML, an autolink) is anywhere in `md`.
fn raw_angle(md: &str) -> bool {
    outside_code(md).contains(&('<', false))
}

/// The destination of every `[…](…)` link a reader would see.
fn link_targets(md: &str) -> Vec<String> {
    let seen = outside_code(md);
    let mut out = Vec::new();
    for (i, w) in seen.windows(2).enumerate() {
        if w == [(']', false), ('(', false)] {
            let dest: String = seen[i + 2..]
                .iter()
                .take_while(|&&(c, esc)| (c, esc) != (')', false))
                .map(|&(c, _)| c)
                .collect();
            out.push(dest);
        }
    }
    out
}

#[test]
fn a_bang_before_a_link_never_makes_an_image() {
    let out = md(vec![Block::Paragraph(vec![Line(vec![
        plain("Look!"),
        linked("here", "https://example.org/x.png"),
    ])])]);
    assert_eq!(out, "Look\\![here](https://example.org/x.png)\n");
    assert_eq!(images_in(&out), 0, "{out}");
    assert_eq!(link_targets(&out), ["https://example.org/x.png"]);
}

#[test]
fn html_in_pdf_text_is_literal_text() {
    let out = md(vec![
        Block::Paragraph(vec![line("<img src=x>")]),
        Block::Heading {
            level: 1,
            runs: vec![plain("<script>")],
        },
        Block::Table {
            rows: vec![vec!["<b>".to_owned()]],
            header: true,
        },
    ]);
    assert_eq!(
        out,
        "\\<img src=x\\>\n\n# \\<script\\>\n\n| \\<b\\> |\n| --- |\n"
    );
    assert!(!raw_angle(&out), "{out}");
}

#[test]
fn code_spans_never_touch() {
    // A refused link counts as none, and an empty run between two code
    // spans is dropped: either way the spans merge instead of their fences
    // fusing into one unclosed run that leaves `<img …>` bare.
    let mono = |text: &str, link: Option<&str>| Run {
        mono: true,
        link: link.map(str::to_owned),
        ..plain(text)
    };
    let out = md(vec![
        Block::Paragraph(vec![Line(vec![
            mono("<img src=x>", Some("file:///etc/passwd")),
            mono("``b", None),
        ])]),
        Block::Paragraph(vec![Line(vec![
            mono("<i>", None),
            Run {
                bold: true,
                ..plain("")
            },
            mono("<b>", None),
        ])]),
    ]);
    assert_eq!(out, "```<img src=x>``b```\n\n`<i><b>`\n");
    assert!(!raw_angle(&out), "{out}");
}

/// Characters that mean something to Markdown or HTML, weighted so random
/// runs hit every construct often.
const ALPHABET: &[&str] = &[
    "!",
    "[",
    "]",
    "(",
    ")",
    "<",
    ">",
    "`",
    "``",
    "\\",
    "*",
    "_",
    "~",
    "&",
    "#",
    ";",
    ":",
    "/",
    "|",
    "+",
    "-",
    "=",
    ".",
    "1",
    "9",
    "\"",
    "'",
    " ",
    "  ",
    "a",
    "Z",
    "é",
    "\t",
    "\n",
    "\u{1b}",
    "\0",
    "&amp;",
    "&#33;",
    "www.",
    "http://",
    "https://e.org",
    "<img src=x>",
    "![",
    "](",
    "-->",
];

/// Link targets an annotation could carry: allowed and refused schemes,
/// and characters that would end or confuse a destination.
const URIS: &[&str] = &[
    "https://example.org/",
    "http://e.org/a b(c)<d>",
    "mailto:a@b.c",
    "ftp://f.org/x\\y`z[1]",
    "javascript:alert(1)",
    "file:///etc/passwd",
    "data:image/png;base64,AAAA",
];

struct Rng(u64);

impl Rng {
    /// xorshift64*: a fixed, platform-independent source for the fuzz test.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn text(&mut self) -> String {
        (0..self.below(6))
            .map(|_| ALPHABET[self.below(ALPHABET.len())])
            .collect()
    }

    fn run(&mut self) -> Run {
        Run {
            text: self.text(),
            bold: self.below(4) == 0,
            italic: self.below(4) == 0,
            mono: self.below(5) == 0,
            link: (self.below(3) == 0).then(|| URIS[self.below(URIS.len())].to_owned()),
        }
    }

    fn line(&mut self) -> Line {
        Line((0..1 + self.below(5)).map(|_| self.run()).collect())
    }

    fn block(&mut self) -> Block {
        match self.below(5) {
            0 => Block::Heading {
                level: 1 + self.below(3) as u8,
                runs: self.line().0,
            },
            1 => Block::List {
                ordered: self.below(2) == 0,
                items: (0..1 + self.below(3))
                    .map(|_| (0..1 + self.below(2)).map(|_| self.line()).collect())
                    .collect(),
            },
            2 => Block::Table {
                rows: (0..1 + self.below(3))
                    .map(|_| (0..1 + self.below(3)).map(|_| self.text()).collect())
                    .collect(),
                header: self.below(2) == 0,
            },
            _ => Block::Paragraph((0..1 + self.below(3)).map(|_| self.line()).collect()),
        }
    }
}

#[test]
fn random_runs_never_embed_fetch_or_autolink() {
    let allowed_dests: Vec<String> = URIS
        .iter()
        .filter(|u| allowed(u))
        .map(|u| destination(u))
        .collect();
    let mut rng = Rng(0x00f0_2f02_9e37_79b9);
    for case in 0..4000 {
        let blocks: Vec<Block> = (0..1 + rng.below(4)).map(|_| rng.block()).collect();
        let out = md(blocks.clone());
        let ctx = || format!("case {case}: {blocks:?}\n---\n{out}");
        assert_eq!(images_in(&out), 0, "{}", ctx());
        assert!(!raw_angle(&out), "{}", ctx());
        for dest in link_targets(&out) {
            assert!(allowed_dests.contains(&dest), "{dest:?} in {}", ctx());
        }
    }
}
