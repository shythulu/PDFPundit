//! Deny-list scan of every artefact for UI copy (T-14). Uses only `crate::engine::*`,
//! plus the two things the scan is about: the UI string table it reads its
//! deny-list from, and the Markdown export (T-32b), which sits beside the
//! facade rather than behind it.
//!
//! An artefact (the repaired PDF, the report, the exported Markdown) must
//! never carry the cat: no face, pose, reaction, theme name or UI line
//! (goal point 3). The deny-list is `ui::strings::ALL`. Several of its entries
//! are ordinary words ("open", "done", "nom"), so a hit is a whole-word match,
//! and a `{n}`-style placeholder matches any text on the same line. Case
//! counts: the golden's own text says "PDFPundit", which is the document's,
//! not the status bar's "PDFPuNDiT".
//!
//! T-32b adds the `.md` of the fixtures; T-14 adds the repaired PDFs and the
//! reports when the engine's bodies land.

use crate::pdf::export::layout::{Block, LayoutNote, Line, PageLayout, Run};
use crate::pdf::export::layouts;
use crate::pdf::export::markdown::{MarkdownOptions, to_markdown};
use crate::pdf::fixtures::{
    TINY_JPEG, golden_pdf, golden_pdf_signed, outline_only_page, type3_only_page,
};
use crate::ui::strings::ALL;

/// The deny-list entries `text` contains.
fn hits(text: &str) -> Vec<&'static str> {
    ALL.iter()
        .copied()
        .filter(|entry| text.lines().any(|line| matches(line, entry)))
        .collect()
}

/// Whether `entry`, its `{…}` placeholders matching any text, occurs in
/// `line` as whole words.
fn matches(line: &str, entry: &str) -> bool {
    let pieces = pieces(entry);
    let Some(first) = pieces.first() else {
        return false;
    };
    let starts_word = entry.chars().next().is_some_and(char::is_alphanumeric);
    let ends_word = entry.chars().last().is_some_and(char::is_alphanumeric);
    line.match_indices(first.as_str()).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        if starts_word && before.is_some_and(char::is_alphanumeric) {
            return false;
        }
        rest_matches(&line[at + first.len()..], &pieces[1..], ends_word)
    })
}

/// The rest of the pieces in order after a placeholder each; the last one
/// ends a word when the entry does.
fn rest_matches(line: &str, pieces: &[String], ends_word: bool) -> bool {
    let Some((piece, rest)) = pieces.split_first() else {
        return !ends_word || !line.chars().next().is_some_and(char::is_alphanumeric);
    };
    if piece.is_empty() {
        return rest_matches(line, rest, ends_word);
    }
    line.match_indices(piece.as_str())
        .any(|(at, _)| rest_matches(&line[at + piece.len()..], rest, ends_word))
}

/// `entry` split at its `{…}` placeholders: the literal text before, between
/// and after them (an empty piece after a trailing placeholder).
fn pieces(entry: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut in_placeholder = false;
    for c in entry.chars() {
        match (c, in_placeholder) {
            ('{', false) => in_placeholder = true,
            ('}', true) => {
                in_placeholder = false;
                out.push(String::new());
            }
            (_, true) => {}
            (c, false) => out.last_mut().expect("never empty").push(c),
        }
    }
    if out.first().is_some_and(String::is_empty) && out.len() > 1 {
        out.remove(0);
    }
    out
}

#[test]
fn the_deny_list_is_not_empty_and_the_matcher_finds_hits() {
    assert!(ALL.len() > 50, "the scan cannot pass vacuously");
    assert_eq!(hits("the cat says chomp."), ["chomp"]);
    assert!(
        hits("nominal values, reopened files").is_empty(),
        "whole words"
    );
    assert!(hits("we open the file").contains(&"open"));
    assert!(hits("PDFPuNDiT v0.1").contains(&"PDFPuNDiT"));
    assert!(!hits("PDFPundit golden page one.").contains(&"PDFPuNDiT"));
    assert!(
        hits("·∙· burp. 3 pdfs queued for repair ·∙·")
            .contains(&"·∙· burp. {n} pdfs queued for repair ·∙·")
    );
    assert!(hits("=^..^=").contains(&"=^..^="));
}

/// The Markdown of every fixture the export reads, plus every note and block
/// kind the emitter writes.
fn fixture_exports() -> Vec<(String, String)> {
    let opts = MarkdownOptions {
        page_separators: true,
        image_dir_name: Some("fixture.0123abcd.images".to_owned()),
    };
    let images = vec![("p1-1.jpg".to_owned(), TINY_JPEG.to_vec())];
    let mut out: Vec<(String, String)> = [
        ("golden", golden_pdf()),
        ("golden signed", golden_pdf_signed()),
        ("outline only", outline_only_page()),
        ("type3 only", type3_only_page()),
    ]
    .into_iter()
    .map(|(name, pdf)| {
        let pages = layouts(&pdf, &[], &images).expect("the fixture loads");
        (name.to_owned(), to_markdown(&pages, &opts))
    })
    .collect();
    let plain = |text: &str| Run {
        text: text.to_owned(),
        bold: false,
        italic: false,
        mono: false,
        link: None,
    };
    let every_kind = PageLayout {
        index: 0,
        width_pt: 612,
        height_pt: 792,
        blocks: vec![
            Block::Heading {
                level: 1,
                runs: vec![plain("Title")],
            },
            Block::Paragraph(vec![Line(vec![plain("Body.")])]),
            Block::List {
                ordered: true,
                items: vec![vec![Line(vec![plain("item")])]],
            },
            Block::Table {
                rows: vec![vec!["a".into(), "b".into(), "c".into()]],
                header: false,
            },
            Block::Image {
                name: "p1-1.jpg".into(),
                sha256: [0; 32],
                len: 1,
            },
            Block::Rule,
        ],
        notes: vec![
            LayoutNote::OutlinedText {
                paths: 9,
                contours: 412,
            },
            LayoutNote::Type3Text,
            LayoutNote::ImageOnly,
            LayoutNote::Unmapped { glyphs: 1 },
            LayoutNote::Unmapped { glyphs: 2 },
        ],
    };
    out.push(("every kind".to_owned(), to_markdown(&[every_kind], &opts)));
    out.push((
        "no images".to_owned(),
        to_markdown(
            &[PageLayout {
                index: 0,
                width_pt: 1,
                height_pt: 1,
                blocks: vec![Block::Image {
                    name: "p1-1.jpg".into(),
                    sha256: [0; 32],
                    len: 1,
                }],
                notes: Vec::new(),
            }],
            &MarkdownOptions::default(),
        ),
    ));
    out
}

#[test]
fn the_fixtures_markdown_carries_no_ui_copy() {
    let exports = fixture_exports();
    assert!(exports.iter().all(|(_, md)| !md.is_empty()));
    for (name, md) in &exports {
        assert_eq!(hits(md), Vec::<&str>::new(), "{name}:\n{md}");
    }
}
