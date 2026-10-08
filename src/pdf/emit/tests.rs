//! T-12a acceptance: Resave of the structural fixtures reloads strictly, the
//! trailer whitelist, the filter rewrites of C9 streams read back through
//! hayro, determinism with a committed golden hash, the narrowed-reals count
//! and the raw copy of an unrecoverable stream.

use std::num::NonZeroUsize;
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::Name;
use hayro::hayro_syntax::object::stream::ImageDecodeParams;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, LoadOptions, Object, StringFormat};
use sha2::{Digest, Sha256};

use super::*;
use crate::engine::{
    AnalyzeOptions, Cancelled, FontDb, NullProgress, PageSize, RepairOptions, SalvageBudget,
};
use crate::pdf::carver::carve;
use crate::pdf::diagnose::diagnose;
use crate::pdf::fixtures::{
    self, corrupt, golden_pdf, golden_pdf_objstm, with_flate_then_dct, with_multi_filter,
    with_predictor_image,
};
use crate::pdf::model::{CorruptionClass, FindingKind};
use crate::pdf::rebuild::{plan_ids, rebuild_page_tree};
use crate::pdf::streams::salvage::salvage_all;
use crate::pdf::streams::salvage::{CarveSource, Grade};
use crate::pdf::streams::{DEFAULT_CAP, InflateStatus, decode_chain, inflate};

/// The golden image and page 1's content stream.
const IMAGE: u32 = 10;
const CONTENT_1: u32 = 11;

/// One Resave and what it reported.
struct Run {
    out: Vec<u8>,
    notes: EmitNotes,
    salvage: SalvageIndex,
}

fn resave(bytes: &[u8]) -> Run {
    resave_with(bytes, &SalvageBudget::default())
}

/// Carve, salvage under `budget`, renumber, rebuild the page tree, emit.
fn resave_with(bytes: &[u8], budget: &SalvageBudget) -> Run {
    let never = || false;
    let carve = match carve(bytes, &never) {
        Ok(c) => c,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    };
    let graph = ObjectGraph::from_carve(&carve);
    let salvage = salvage_all(
        &CarveSource::new(&carve, bytes),
        budget,
        NonZeroUsize::MIN,
        1 << 30,
        &never,
    )
    .expect("salvage without a cancel");
    let remap = plan_ids(&carve, &graph);
    let tree = rebuild_page_tree(&carve, &graph, &remap, PageSize::A4);
    let mut sink = NullProgress;
    let mut ctx = EmitCtx {
        bytes,
        input_sha256: Sha256::digest(bytes).into(),
        salvage: &salvage,
        sink: &mut sink,
        notes: EmitNotes::default(),
    };
    let out = emit_resave(&carve, &graph, &remap, &tree, &mut ctx).expect("emit");
    let notes = ctx.notes;
    Run {
        out,
        notes,
        salvage,
    }
}

fn load_strict(bytes: &[u8]) -> Document {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(1 << 24),
        ..LoadOptions::default()
    };
    Document::load_mem_with_options(bytes, opts).expect("strict load")
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    memchr::memmem::find(hay, needle)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    memchr::memmem::find_iter(hay, needle).count()
}

fn trailer_keys(doc: &Document) -> Vec<String> {
    doc.trailer
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn stream(doc: &Document, id: u32) -> &lopdf::Stream {
    doc.get_object((id, 0)).unwrap().as_stream().unwrap()
}

/// `%PDF-1.7`, then each `(number, body)` as `N 0 obj`, then `tail`.
fn hand_made(objects: &[(u32, String)], tail: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    for (n, body) in objects {
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    out.extend_from_slice(tail.as_bytes());
    out.extend_from_slice(b"%%EOF\n");
    out
}

fn stream_body(dict: &str, data: &[u8]) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{}\nendstream",
        data.len(),
        String::from_utf8_lossy(data)
    )
}

/// One page drawing a form XObject whose `/Matrix` is `matrix`.
fn with_form_matrix(matrix: &str) -> Vec<u8> {
    hand_made(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
                 /Resources << /XObject << /Fm1 5 0 R >> >> /Contents 4 0 R >>"
                    .into(),
            ),
            (4, stream_body("", b"/Fm1 Do")),
            (
                5,
                stream_body(
                    &format!("/Type /XObject /Subtype /Form /BBox [0 0 100 100] /Matrix {matrix}"),
                    b"0 0 50 50 re f",
                ),
            ),
        ],
        "",
    )
}

// ── structural fixtures ──────────────────────────────────────────────────

#[test]
fn resave_of_c2_c3_c5_c10_reloads_in_strict_lopdf() {
    use CorruptionClass::{C2XrefMissing, C3TrailerDamaged, C5ObjectTagStripped, C10Truncated};
    for class in [
        C2XrefMissing,
        C3TrailerDamaged,
        C5ObjectTagStripped,
        C10Truncated,
    ] {
        for seed in 0..3 {
            let input = corrupt(class, &golden_pdf(), seed);
            let out = resave(&input).out;
            let label = format!("{} seed {seed}", class.code());
            let doc = load_strict(&out);
            assert!(out.starts_with(b"%PDF-1.7\n%"), "{label}");
            assert_eq!(count(&out, b"\nxref\n"), 1, "{label}");
            assert_eq!(find(&out, b"/XRef"), None, "{label}");
            assert_eq!(find(&out, b"/ObjStm"), None, "{label}");
            let pages = doc.get_pages().len();
            if class == C10Truncated {
                assert!(pages >= 1, "{label}: {pages} pages");
                continue;
            }
            assert_eq!(pages, 2, "{label}");
            // The re-emitted file is clean of the class it was cut with.
            let carve = carve(&out, &|| false).unwrap();
            let graph = ObjectGraph::from_carve(&carve);
            let findings = diagnose(&out, &carve, &graph, &SalvageIndex::default());
            let structural: Vec<_> = findings
                .iter()
                .filter(|f| matches!(f.class, FindingKind::Corruption(_)))
                .collect();
            assert!(structural.is_empty(), "{label}: {structural:?}");
        }
    }
}

#[test]
fn the_golden_resaves_to_its_own_pages_and_streams() {
    let input = golden_pdf();
    let out = resave(&input).out;
    let before = load_strict(&input);
    let after = load_strict(&out);
    assert_eq!(after.get_pages().len(), 2);
    for id in [8, 9, IMAGE, CONTENT_1, 12] {
        assert_eq!(
            stream(&after, id).content,
            stream(&before, id).content,
            "{id}"
        );
        assert_eq!(
            stream(&after, id).dict.get(b"Filter").ok(),
            stream(&before, id).dict.get(b"Filter").ok(),
            "{id}"
        );
    }
}

#[test]
fn gapped_numbers_keep_separate_xref_subsections() {
    let input = hand_made(
        &[
            (5, "<< /Type /Catalog /Pages 9 0 R >>".into()),
            (9, "<< /Type /Pages /Kids [20 0 R] /Count 1 >>".into()),
            (
                20,
                "<< /Type /Page /Parent 9 0 R /MediaBox [0 0 100 100] /Contents 30 0 R >>".into(),
            ),
            (30, stream_body("", b"0 0 m 10 10 l S")),
        ],
        "",
    );
    let out = resave(&input).out;
    // The flat page tree's node takes the first free number, 31.
    let doc = load_strict(&out);
    assert_eq!(doc.trailer.get(b"Size").unwrap(), &Object::Integer(32));
    for sub in ["\n0 1\n", "\n5 1\n", "\n9 1\n", "\n20 1\n", "\n30 2\n"] {
        assert_eq!(count(&out, sub.as_bytes()), 1, "subsection {sub:?}");
    }
    assert_eq!(doc.get_pages().into_values().collect::<Vec<_>>(), [(20, 0)]);
    let page = doc.get_dictionary((20, 0)).unwrap();
    assert_eq!(page.get(b"Parent").unwrap(), &Object::Reference((31, 0)));
}

// ── the trailer ──────────────────────────────────────────────────────────

#[test]
fn the_trailer_is_root_size_and_the_input_hash_id() {
    for input in [golden_pdf(), golden_pdf_objstm()] {
        let out = resave(&input).out;
        let doc = load_strict(&out);
        assert_eq!(trailer_keys(&doc), ["Root", "ID", "Size"]);
        assert_eq!(
            doc.trailer.get(b"Root").unwrap(),
            &Object::Reference((1, 0))
        );
        let digest = Sha256::digest(&input);
        let id = Object::String(digest[..16].to_vec(), StringFormat::Hexadecimal);
        assert_eq!(
            doc.trailer.get(b"ID").unwrap(),
            &Object::Array(vec![id.clone(), id])
        );
        let at = out.windows(7).rposition(|w| w == b"trailer").unwrap();
        let trailer = &out[at..];
        assert_eq!(find(trailer, b"/Type"), None);
        assert_eq!(find(&out, b"/XRef"), None);
        for absent in [
            b"/Info".as_slice(),
            b"/CreationDate",
            b"/ModDate",
            b"/Producer",
        ] {
            assert_eq!(find(&out, absent), None);
        }
    }
}

#[test]
fn info_is_kept_only_when_the_input_had_one() {
    let objects = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned(),
        ),
        (
            4,
            "<< /Title (Minutes) /CreationDate (D:20240101000000Z) >>".to_owned(),
        ),
    ];
    let with = hand_made(&objects, "trailer\n<< /Root 1 0 R /Info 4 0 R /Size 5 >>\n");
    let doc = load_strict(&resave(&with).out);
    assert_eq!(trailer_keys(&doc), ["Root", "ID", "Info", "Size"]);
    let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let info = doc.get_dictionary(info).unwrap();
    assert_eq!(
        info.get(b"Title").unwrap(),
        &Object::string_literal("Minutes")
    );
    assert!(info.has(b"CreationDate"));

    // The same dictionary referenced by nothing and without a /Title is not
    // an information dictionary.
    let mut without = objects.clone();
    without[3].1 = "<< /Author (x) >>".to_owned();
    let doc = load_strict(&resave(&hand_made(&without, "")).out);
    assert_eq!(trailer_keys(&doc), ["Root", "ID", "Size"]);
}

// ── pages ────────────────────────────────────────────────────────────────

#[test]
fn inherited_attributes_are_pinned_on_each_page_of_the_flat_tree() {
    let input = hand_made(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] \
                 /Resources << /Font << /F1 6 0 R >> >> /Rotate 90 >>"
                    .into(),
            ),
            (
                3,
                "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>".into(),
            ),
            (4, "<< /Type /Page /Parent 3 0 R /Contents 5 0 R >>".into()),
            (5, stream_body("", b"BT /F1 12 Tf (x) Tj ET")),
            (
                6,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
            ),
        ],
        "",
    );
    let doc = load_strict(&resave(&input).out);
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 1);
    let page = doc.get_dictionary(pages[&1]).unwrap();
    assert_eq!(
        page.get(b"MediaBox").unwrap(),
        &Object::Array(vec![0.into(), 0.into(), 300.into(), 400.into()])
    );
    assert_eq!(page.get(b"Rotate").unwrap(), &Object::Integer(90));
    let font = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"F1")
        .unwrap();
    assert_eq!(font, &Object::Reference((6, 0)));
    let parent = page.get(b"Parent").unwrap().as_reference().unwrap();
    let catalog = doc.catalog().unwrap();
    assert_eq!(
        catalog.get(b"Pages").unwrap().as_reference().unwrap(),
        parent
    );
    assert_eq!(
        doc.get_dictionary(parent).unwrap().get(b"Kids").unwrap(),
        &Object::Array(vec![Object::Reference(pages[&1])])
    );
}

#[test]
fn a_file_without_a_catalog_gets_one() {
    let input = hand_made(
        &[
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into(),
            ),
        ],
        "",
    );
    let doc = load_strict(&resave(&input).out);
    assert_eq!(doc.get_pages().len(), 1);
    let catalog = doc.catalog().unwrap();
    assert_eq!(
        catalog.get(b"Type").unwrap(),
        &Object::Name(b"Catalog".to_vec())
    );
}

// ── filter rewrites ──────────────────────────────────────────────────────

fn ascii85_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut g = [0u8; 4];
        g[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(g);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = b'!' + (v % 85) as u8;
            v /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// `pdf` with one byte of stream `id`'s Flate-stage input replaced so its
/// deflate data is invalid (a C9 hit in the Flate domain, never one the
/// Adler-32 alone sees, so a reader cannot decode the damaged file to the
/// original's samples), the earlier filters re-encoded around it, in place.
fn c9_hit(pdf: &[u8], id: u32) -> Vec<u8> {
    let doc = Document::load_mem(pdf).unwrap();
    let s = stream(&doc, id);
    let raw = &s.content;
    let at = find(pdf, raw).unwrap();
    let chain = filters_of(&s.dict);
    let flate_at = chain.iter().position(|(f, _)| *f == Filter::Flate).unwrap();
    let input = decode_chain(raw, &chain[..flate_at], DEFAULT_CAP).unwrap();
    for pos in (input.len() / 2..input.len() - 4).chain(2..input.len() / 2) {
        let mut damaged = input.clone();
        damaged[pos] ^= 0x5a;
        if !matches!(
            inflate(&damaged, DEFAULT_CAP).status,
            InflateStatus::Failed { .. }
        ) {
            continue;
        }
        let damaged_raw = if flate_at == 0 {
            damaged
        } else {
            ascii85_encode(&damaged)
        };
        if damaged_raw.len() != raw.len() {
            continue;
        }
        let mut out = pdf.to_vec();
        out[at..at + raw.len()].copy_from_slice(&damaged_raw);
        return out;
    }
    panic!("no damaging byte in stream {id}");
}

/// Page 1's `/Im1`, decoded by hayro as 8×8 RGB samples.
fn hayro_image_on_page_1(bytes: &[u8]) -> Vec<u8> {
    let pdf = Pdf::new(bytes.to_vec()).expect("hayro loads");
    let page = &pdf.pages()[0];
    let image = page
        .resources()
        .get_x_object(&Name::new(b"Im1").unwrap())
        .expect("page 1 /Im1");
    let params = ImageDecodeParams {
        bpc: Some(8),
        num_components: Some(3),
        width: 8,
        height: 8,
        ..ImageDecodeParams::default()
    };
    image
        .decoded_image(&params)
        .expect("image decodes")
        .data
        .into_owned()
}

/// Page 1's content, decoded by hayro.
fn hayro_content_of_page_1(bytes: &[u8]) -> Vec<u8> {
    let pdf = Pdf::new(bytes.to_vec()).expect("hayro loads");
    pdf.pages()[0]
        .page_stream()
        .expect("page 1 content")
        .to_vec()
}

/// Page 1 rendered by hayro with no fallback font: its RGBA bytes.
fn render_page_1(bytes: &[u8]) -> Vec<u8> {
    let settings = InterpreterSettings {
        font_resolver: Arc::new(|_| None),
        ..InterpreterSettings::default()
    };
    let pdf = Pdf::new(bytes.to_vec()).expect("hayro loads");
    let scale = PixmapSettings {
        x_scale: 0.5,
        y_scale: 0.5,
        ..PixmapSettings::default()
    };
    let pixmap = render(
        &pdf.pages()[0],
        &RenderCache::new(),
        &settings,
        &RenderSettings::default(),
        &scale,
    );
    pixmap.data_as_u8_slice().to_vec()
}

#[test]
fn a_repaired_predictor_image_keeps_flate_and_its_parms() {
    let original = with_predictor_image();
    let run = resave(&c9_hit(&original, IMAGE));
    assert!(matches!(
        run.salvage.by_obj[&(IMAGE, 0)].salvage,
        Salvage::Repaired { .. }
    ));
    let doc = load_strict(&run.out);
    let image = stream(&doc, IMAGE);
    assert_eq!(
        image.dict.get(b"Filter").unwrap(),
        &Object::Name(b"FlateDecode".to_vec())
    );
    let parms = image.dict.get(b"DecodeParms").unwrap().as_dict().unwrap();
    assert_eq!(parms.get(b"Predictor").unwrap(), &Object::Integer(15));
    assert_eq!(
        hayro_image_on_page_1(&run.out),
        hayro_image_on_page_1(&original)
    );
    assert!(run.notes.filter_rewrites.is_empty());
}

#[test]
fn a_repaired_ascii85_flate_stream_drops_the_ascii85_stage() {
    let original = with_multi_filter();
    let run = resave(&c9_hit(&original, CONTENT_1));
    assert!(matches!(
        run.salvage.by_obj[&(CONTENT_1, 0)].salvage,
        Salvage::Repaired { .. }
    ));
    let doc = load_strict(&run.out);
    assert_eq!(
        stream(&doc, CONTENT_1).dict.get(b"Filter").unwrap(),
        &Object::Name(b"FlateDecode".to_vec())
    );
    assert_eq!(
        hayro_content_of_page_1(&run.out),
        hayro_content_of_page_1(&original)
    );
    assert_eq!(
        run.notes.filter_rewrites,
        [(
            (CONTENT_1, 0),
            "[/ASCII85Decode /FlateDecode]".to_owned(),
            "/FlateDecode".to_owned()
        )]
    );
}

#[test]
fn a_repaired_flate_dct_image_keeps_both_stages() {
    let original = with_flate_then_dct();
    let run = resave(&c9_hit(&original, IMAGE));
    assert!(matches!(
        run.salvage.by_obj[&(IMAGE, 0)].salvage,
        Salvage::Repaired { .. }
    ));
    let doc = load_strict(&run.out);
    assert_eq!(
        stream(&doc, IMAGE).dict.get(b"Filter").unwrap(),
        &Object::Array(vec![
            Object::Name(b"FlateDecode".to_vec()),
            Object::Name(b"DCTDecode".to_vec())
        ])
    );
    assert_eq!(
        hayro_image_on_page_1(&run.out),
        hayro_image_on_page_1(&original)
    );
    assert!(run.notes.filter_rewrites.is_empty());
}

#[test]
fn a_checksum_mismatch_predictor_image_is_written_uncompressed_and_renders_the_same() {
    let original = with_predictor_image();
    let doc = Document::load_mem(&original).unwrap();
    let raw = stream(&doc, IMAGE).content.clone();
    let at = find(&original, &raw).unwrap() + raw.len();
    let mut damaged = original.clone();
    // The Adler-32 trailer: every sample still decodes.
    damaged[at - 1] ^= 0x01;
    damaged[at - 2] ^= 0x01;
    let no_search = SalvageBudget {
        work: 0,
        deep_work: 0,
        deep_pool: 0,
        ..SalvageBudget::default()
    };
    let run = resave_with(&damaged, &no_search);
    let Salvage::ChecksumMismatch { data } = &run.salvage.by_obj[&(IMAGE, 0)].salvage else {
        panic!("{:?}", run.salvage.by_obj[&(IMAGE, 0)].salvage);
    };
    let out = load_strict(&run.out);
    let image = stream(&out, IMAGE);
    assert_eq!(&image.content, data);
    assert!(!image.dict.has(b"Filter"));
    assert!(!image.dict.has(b"DecodeParms"));
    assert_eq!(render_page_1(&run.out), render_page_1(&original));
    assert_eq!(
        hayro_image_on_page_1(&run.out),
        hayro_image_on_page_1(&original)
    );
    assert_eq!(
        run.notes.filter_rewrites,
        [((IMAGE, 0), "/FlateDecode".to_owned(), "[]".to_owned())]
    );
}

#[test]
fn the_salvage_of_a_later_copy_is_not_applied_to_an_earlier_winner() {
    // Object 12's later copy is cut off by EOF, so the earlier one wins
    // (D-032) while the salvage index describes the later one.
    let input = fixtures::with_truncated_later_duplicate();
    let run = resave(&input);
    let golden = load_strict(&golden_pdf());
    let doc = load_strict(&run.out);
    assert_eq!(stream(&doc, 12).content, stream(&golden, 12).content);
    assert!(run.notes.filter_rewrites.is_empty());
}

// ── unrecoverable ────────────────────────────────────────────────────────

#[test]
fn an_unrecoverable_stream_is_written_as_carved_with_its_filter() {
    let dead: Vec<u8> = (0u8..64).map(|i| 0xff - (i % 3)).collect();
    let mut input = hand_made(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".into(),
            ),
        ],
        "",
    );
    // Object 4 by hand: its data is binary.
    let tail = input.split_off(input.len() - b"%%EOF\n".len());
    input.extend_from_slice(
        format!(
            "4 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            dead.len()
        )
        .as_bytes(),
    );
    input.extend_from_slice(&dead);
    input.extend_from_slice(b"\nendstream\nendobj\n");
    input.extend_from_slice(&tail);

    let run = resave(&input);
    assert_eq!(
        run.salvage.by_obj[&(4, 0)].salvage,
        Salvage::Unrecoverable,
        "the fixture's one Flate stream is unrecoverable"
    );
    let doc = load_strict(&run.out);
    let s = stream(&doc, 4);
    assert_eq!(s.content, dead);
    assert_eq!(
        s.dict.get(b"Filter").unwrap(),
        &Object::Name(b"FlateDecode".to_vec())
    );
    assert!(run.notes.filter_rewrites.is_empty());
}

// ── determinism ──────────────────────────────────────────────────────────

/// The Resave of the C3 fixture (golden, seed 7). The same bytes on every
/// CI OS: the hash is committed.
const C3_SEED_7_RESAVE_SHA256: &str =
    "b6bf65e1d957219097f9d645375a222a81a4330b741921b88f77c7c452b2d95a";

#[test]
fn the_same_input_gives_the_same_bytes_and_the_committed_hash() {
    let input = corrupt(CorruptionClass::C3TrailerDamaged, &golden_pdf(), 7);
    let a = resave(&input).out;
    let b = resave(&input).out;
    assert_eq!(a, b);
    assert_eq!(hex(&Sha256::digest(&a)), C3_SEED_7_RESAVE_SHA256);
}

#[test]
fn a_repaired_stream_resaves_to_the_same_bytes_twice() {
    let input = c9_hit(&with_multi_filter(), CONTENT_1);
    assert_eq!(resave(&input).out, resave(&input).out);
}

// ── narrowing (D-075) ────────────────────────────────────────────────────

fn report_for(bytes: &[u8]) -> RepairReport {
    let opts = RepairOptions::default();
    let analysis =
        crate::engine::analyze(bytes, &AnalyzeOptions::default(), &mut NullProgress).unwrap();
    RepairReport::default_for(&analysis, &opts, &FontDb::empty())
}

const NARROWED_LINE: &str = "2 numbers were narrowed to single precision on re-emission";

#[test]
fn a_long_matrix_is_counted_as_narrowed_and_reported() {
    let input = with_form_matrix("[0.70710678 0 0 0.70710678 0 0]");
    let run = resave(&input);
    assert_eq!(run.notes.reals_narrowed, 2);
    let mut report = report_for(&input);
    run.notes.record(&mut report);
    assert_eq!(report.reals_narrowed, 2);
    assert!(report.lines().iter().any(|l| l == NARROWED_LINE));
    // What the output carries is the f32 value.
    let doc = load_strict(&run.out);
    let form = stream(&doc, 5);
    assert_eq!(
        form.dict.get(b"Matrix").unwrap().as_array().unwrap()[0],
        Object::Real(0.707_106_77)
    );
}

#[test]
fn short_reals_report_nothing() {
    let input = with_form_matrix("[0.5 0 0 0.25 10.75 -3.5]");
    let run = resave(&input);
    assert_eq!(run.notes.reals_narrowed, 0);
    let mut report = report_for(&input);
    run.notes.record(&mut report);
    assert!(!report.lines().iter().any(|l| l.contains("narrowed")));
}

#[test]
fn an_orphans_long_reals_are_counted() {
    let input = hand_made(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into(),
            ),
        ],
        "<< /Type /Annot /Subtype /Square /Rect [0.123456789 1 2 3] >>\nendobj\n",
    );
    let run = resave(&input);
    assert_eq!(run.notes.reals_narrowed, 1);
}

// ── the writer's rules through emit ──────────────────────────────────────

#[test]
fn no_object_or_xref_streams_come_out_of_an_objstm_input() {
    let run = resave(&golden_pdf_objstm());
    let doc = load_strict(&run.out);
    assert_eq!(doc.get_pages().len(), 2);
    assert_eq!(find(&run.out, b"/ObjStm"), None);
    assert_eq!(find(&run.out, b"/XRef"), None);
    assert!(matches!(
        doc.reference_table.cross_reference_type,
        lopdf::xref::XrefType::CrossReferenceTable
    ));
}

#[test]
fn a_repaired_flate_content_stream_decodes_to_the_golden() {
    let run = resave(&c9_hit(&golden_pdf(), CONTENT_1));
    match &run.salvage.by_obj[&(CONTENT_1, 0)].salvage {
        Salvage::Repaired { grade, .. } => assert_eq!(*grade, Grade::Exact),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        hayro_content_of_page_1(&run.out),
        hayro_content_of_page_1(&golden_pdf())
    );
}
