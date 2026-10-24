//! Deny-list scan of every artefact for UI copy (T-14, T-32b). An artefact
//! (the repaired PDF, the report, the exported Markdown) must never carry
//! the cat: no face, pose, reaction, theme name or UI line (goal-r2-q11,
//! goal point 3). Uses `crate::engine::*` only, with these exceptions: the
//! deny-list `crate::ui::strings::ALL`, the inputs from
//! `crate::pdf::fixtures`, the fixture list in the sibling `tests` module,
//! lopdf to decode the repaired PDF, and the Markdown export
//! (`crate::pdf::export`, feature `export`), which sits beside the facade
//! rather than behind it.
//!
//! Every artefact the facade produces for the fixtures is scanned: the
//! repaired PDF, the serialised report and its fixed lines, and the Markdown
//! the export writes from the repair. The repaired PDF is scanned twice: as
//! raw bytes, and decoded. The decoded form is every stream's content after
//! its filters (object streams are already expanded by lopdf's load) and
//! every string's bytes and its text-string decoding (UTF-16BE, UTF-8 or
//! PDFDocEncoding), so copy hidden in a Flate stream, a hex string or a
//! UTF-16 `/Title` is caught. A stream whose filters lopdf cannot undo is
//! scanned raw only. The Markdown of the export's own fixtures (outline-only
//! and Type 3 pages, and a layout with every block and note kind) is scanned
//! as well.
//!
//! **Matching.** An entry hits where its text appears whole: not inside a
//! longer word ("nom" does not hit "nominal"; letters, digits and `_` make
//! words). `{n}` in an entry stands for one or more digits and any other
//! `{…}` placeholder for one or more characters on one line. An entry that
//! is only placeholders and punctuation (`{n}%`) names no copy of its own and
//! is not scanned: it would hit every percentage a report states. Matching
//! is case-sensitive, on raw bytes: the golden's own text says "PDFPundit",
//! which is the document's, not the status bar's "PDFPuNDiT".

use lopdf::{Document, Object, decode_text_string};

use super::tests::class_fixtures;
use super::*;
use crate::pdf::fixtures::{golden_pdf, golden_pdf_signed};
use crate::ui::strings::ALL;

/// One piece of a deny-list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Text(Vec<u8>),
    /// `{n}`.
    Digits,
    /// Any other placeholder.
    Any,
}

/// How far an `Any` placeholder may reach.
const ANY_MAX: usize = 256;

/// `entry` as pieces; `None` when it is not scanned (module docs).
fn pattern(entry: &str) -> Option<Vec<Piece>> {
    let mut pieces = Vec::new();
    let mut rest = entry;
    let mut placeholder = false;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        if open > 0 {
            pieces.push(Piece::Text(rest.as_bytes()[..open].to_vec()));
        }
        pieces.push(match &rest[open..open + close + 1] {
            "{n}" => Piece::Digits,
            _ => Piece::Any,
        });
        placeholder = true;
        rest = &rest[open + close + 1..];
    }
    if !rest.is_empty() {
        pieces.push(Piece::Text(rest.as_bytes().to_vec()));
    }
    let wordy = pieces.iter().any(|p| match p {
        Piece::Text(t) => t.iter().any(u8::is_ascii_alphanumeric),
        _ => false,
    });
    (!placeholder || wordy).then_some(pieces)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Where `pieces` matched from `at` ends, every end it can.
fn ends(hay: &[u8], at: usize, pieces: &[Piece], out: &mut Vec<usize>) {
    let Some((first, rest)) = pieces.split_first() else {
        out.push(at);
        return;
    };
    match first {
        Piece::Text(t) => {
            if hay[at..].starts_with(t) {
                ends(hay, at + t.len(), rest, out);
            }
        }
        Piece::Digits => {
            let n = hay[at..].iter().take_while(|b| b.is_ascii_digit()).count();
            if n > 0 {
                ends(hay, at + n, rest, out);
            }
        }
        Piece::Any => {
            let line = hay[at..].iter().take(ANY_MAX).take_while(|&&b| b != b'\n');
            for k in 1..=line.count() {
                ends(hay, at + k, rest, out);
            }
        }
    }
}

/// The first place `entry` hits in `hay`, if any (module docs).
fn hit(hay: &[u8], entry: &str) -> Option<usize> {
    let pieces = pattern(entry)?;
    let starts_word = entry.bytes().next().is_some_and(is_word) || pieces[0] == Piece::Digits;
    let ends_word =
        entry.bytes().last().is_some_and(is_word) || pieces.last() == Some(&Piece::Digits);
    let mut found = Vec::new();
    for at in 0..hay.len() {
        if starts_word && at > 0 && is_word(hay[at - 1]) {
            continue;
        }
        found.clear();
        ends(hay, at, &pieces, &mut found);
        let whole = |&end: &usize| !ends_word || hay.get(end).is_none_or(|&b| !is_word(b));
        if found.iter().any(whole) {
            return Some(at);
        }
    }
    None
}

/// Every deny-list entry `artefact` holds, with where.
fn hits(artefact: &[u8]) -> Vec<(&'static str, usize)> {
    ALL.iter()
        .filter_map(|&e| hit(artefact, e).map(|at| (e, at)))
        .collect()
}

/// Every deny-list entry `text` holds.
fn entries(text: &str) -> Vec<&'static str> {
    hits(text.as_bytes()).into_iter().map(|(e, _)| e).collect()
}

/// Every artefact of `bytes`'s repair, by name.
fn artefacts(bytes: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let analysis = analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).unwrap();
    let opts = RepairOptions::default();
    let p = plan(&analysis, &opts);
    let out = repair(
        bytes,
        &analysis,
        &p,
        &opts,
        &FontDb::empty(),
        &mut UseBest,
        &mut NullProgress,
    )
    .unwrap();
    let mut v = vec![
        (
            "the report",
            serde_json::to_vec_pretty(&out.report).expect("serialises"),
        ),
        (
            "the report's lines",
            out.report.lines().join("\n").into_bytes(),
        ),
    ];
    #[cfg(feature = "export")]
    v.push((
        "the Markdown export",
        export_of(bytes, &analysis, &out).into_bytes(),
    ));
    if let Some(o) = out.output {
        v.push(("the repaired PDF, decoded", decoded(&o)));
        v.push(("the repaired PDF", o));
    }
    v
}

/// The Markdown the export writes from `out`, as the export job builds it:
/// from the repaired PDF, or the input when there is none, with the
/// analysis's findings and the repair's images.
#[cfg(feature = "export")]
fn export_of(input: &[u8], analysis: &AnalysisResult, out: &RepairOutcome) -> String {
    use crate::pdf::export::layouts;
    use crate::pdf::export::markdown::{MarkdownOptions, to_markdown};
    let pdf = out.output.as_deref().unwrap_or(input);
    let pages = layouts(pdf, &analysis.findings, &out.images).expect("the export reads the pages");
    let opts = MarkdownOptions {
        page_separators: true,
        image_dir_name: (!out.images.is_empty()).then(|| "fixture.0123abcd.images".to_owned()),
    };
    to_markdown(&pages, &opts)
}

/// Cap on one stream's decoded size: the fixtures are small.
const DECODE_MAX: usize = 64 << 20;

/// `pdf`'s decoded streams and strings, one per line (module docs).
fn decoded(pdf: &[u8]) -> Vec<u8> {
    fn strings(o: &Object, out: &mut Vec<u8>) {
        match o {
            Object::String(raw, _) => {
                out.extend_from_slice(raw);
                out.push(b'\n');
                if let Ok(text) = decode_text_string(o) {
                    out.extend_from_slice(text.as_bytes());
                    out.push(b'\n');
                }
            }
            Object::Array(items) => items.iter().for_each(|i| strings(i, out)),
            Object::Dictionary(d) => d.iter().for_each(|(_, v)| strings(v, out)),
            Object::Stream(s) => {
                s.dict.iter().for_each(|(_, v)| strings(v, out));
                if let Ok(body) = s.decompressed_content_with_limit(DECODE_MAX) {
                    out.extend_from_slice(&body);
                    out.push(b'\n');
                }
            }
            _ => {}
        }
    }
    let doc = Document::load_mem(pdf).expect("the repaired PDF loads");
    let mut out = Vec::new();
    for o in doc.objects.values() {
        strings(o, &mut out);
    }
    strings(&Object::Dictionary(doc.trailer.clone()), &mut out);
    out
}

#[test]
fn the_deny_list_is_not_empty() {
    assert!(!ALL.is_empty());
    let scanned = ALL.iter().filter(|e| pattern(e).is_some()).count();
    assert!(scanned > 100, "{scanned} entries scanned");
    for must in ["=^..^=", "feed me", "chomp", "‼", "DarkBerry Blackwater"] {
        assert!(ALL.contains(&must), "{must:?}");
        assert!(pattern(must).is_some(), "{must:?}");
    }
}

#[test]
fn the_scan_finds_what_it_must_and_nothing_inside_words() {
    let planted = |s: &str| hits(format!("<< /Title ({s}) >>").as_bytes());
    assert!(planted("=^..^=").iter().any(|(e, _)| *e == "=^..^="));
    assert!(
        planted("feed me a pdf")
            .iter()
            .any(|(e, _)| *e == "feed me")
    );
    assert!(planted("nom").iter().any(|(e, _)| *e == "nom"));
    assert!(planted("3 pdfs").iter().any(|(e, _)| *e == "{n} pdfs"));
    assert!((planted("resolving thesis.pdf").iter()).any(|(e, _)| *e == "resolving {file}"));
    assert!(planted("Mono Ink").iter().any(|(e, _)| *e == "Mono Ink"));
    assert!(planted("‼").iter().any(|(e, _)| *e == "‼"));
    assert_eq!(entries("the cat says chomp."), ["chomp"]);
    assert!(entries("we open the file").contains(&"open"));
    assert!(
        entries("·∙· burp. 3 pdfs queued for repair ·∙·")
            .contains(&"·∙· burp. {n} pdfs queued for repair ·∙·")
    );
    // Case counts: the document's name is not the status bar's.
    assert!(entries("PDFPuNDiT v0.1").contains(&"PDFPuNDiT"));
    assert!(!entries("PDFPundit golden page one.").contains(&"PDFPuNDiT"));
    // Inside a word, or a bare number with no copy: no hit.
    assert_eq!(planted("nominal xpdfs openly"), []);
    assert_eq!(
        entries("nominal values, reopened files"),
        Vec::<&str>::new()
    );
    assert_eq!(planted("88% kept"), []);
}

#[test]
fn the_decoded_scan_finds_copy_in_compressed_streams_and_utf16_strings() {
    use lopdf::{Dictionary, Stream, dictionary, text_string};
    let mut doc = Document::with_version("1.7");
    let body = "BT (feed me) Tj ET\n".repeat(64).into_bytes();
    let mut stream = Stream::new(Dictionary::new(), body);
    stream.compress().expect("compresses");
    assert!(
        stream.dict.get(b"Filter").is_ok(),
        "the stream is compressed"
    );
    let content = doc.add_object(stream);
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    let info = doc.add_object(dictionary! { "Title" => text_string("chomp ‼") });
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info);
    let mut pdf = Vec::new();
    doc.save_to(&mut pdf).expect("saves");

    let raw: Vec<&str> = hits(&pdf).into_iter().map(|(e, _)| e).collect();
    for e in ["feed me", "chomp", "‼"] {
        assert!(!raw.contains(&e), "{e:?} is hidden from the raw scan");
    }
    let seen: Vec<&str> = hits(&decoded(&pdf)).into_iter().map(|(e, _)| e).collect();
    for e in ["feed me", "chomp", "‼"] {
        assert!(seen.contains(&e), "{e:?} in {seen:?}");
    }
}

#[test]
fn no_artefact_carries_ui_copy() {
    let mut inputs: Vec<(String, Vec<u8>)> = vec![
        ("the golden".into(), golden_pdf()),
        ("the signed golden".into(), golden_pdf_signed()),
    ];
    inputs.extend(
        class_fixtures()
            .into_iter()
            .map(|(c, b)| (format!("the {} fixture", c.code()), b)),
    );
    let mut scanned = 0;
    for (input, bytes) in &inputs {
        for (name, artefact) in artefacts(bytes) {
            scanned += 1;
            let found = hits(&artefact);
            assert!(found.is_empty(), "{name} of {input}: {found:?}");
        }
    }
    // Every input has a report, its lines and (with the export) its
    // Markdown; all but the two goldens an output, scanned raw and decoded.
    let per_input = if cfg!(feature = "export") { 5 } else { 4 };
    assert_eq!(scanned, per_input * inputs.len() - 4);
}

/// The export's own fixtures and layouts, which the facade's fixtures do not
/// reach (module docs).
#[cfg(feature = "export")]
mod markdown {
    use super::entries;
    use crate::pdf::export::layout::{Block, LayoutNote, Line, PageLayout, Run};
    use crate::pdf::export::layouts;
    use crate::pdf::export::markdown::{MarkdownOptions, to_markdown};
    use crate::pdf::fixtures::{
        TINY_JPEG, golden_pdf, golden_pdf_signed, outline_only_page, type3_only_page,
    };

    /// The Markdown of every fixture the export reads, plus every note and
    /// block kind the emitter writes.
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
            assert_eq!(entries(md), Vec::<&str>::new(), "{name}:\n{md}");
        }
    }
}
