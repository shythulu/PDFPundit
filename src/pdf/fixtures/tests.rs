//! The goldens and the structural corruptors (T-03a acceptance). Spans are
//! re-derived here from the golden's own bytes, not through the corruptors'
//! helpers.

use super::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::hayro_syntax::object::Name;
use hayro::hayro_syntax::object::stream::ImageDecodeParams;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::LoadOptions;
use lopdf::content::Content;
use skrifa::MetadataProvider;
use skrifa::raw::tables::glyf::Glyph;

const SEEDS: std::ops::Range<u64> = 0..16;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn load_strict(bytes: &[u8]) -> Document {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(1 << 20),
        ..LoadOptions::default()
    };
    Document::load_mem_with_options(bytes, opts).expect("strict lopdf load")
}

fn goldens() -> [(&'static str, Vec<u8>); 2] {
    [
        ("golden_pdf", golden_pdf()),
        ("golden_pdf_signed", golden_pdf_signed()),
    ]
}

fn deref<'a>(doc: &'a Document, obj: &Object) -> &'a Object {
    doc.get_object(obj.as_reference().expect("reference"))
        .expect("referenced object")
}

/// The Type0 font every page uses (through page 1's `/F1`).
fn type0(doc: &Document) -> &Dictionary {
    let page = doc.get_pages()[&1];
    let fonts = doc.get_page_fonts(page).expect("page fonts");
    fonts[b"F1".as_slice()]
}

fn cidfont(doc: &Document) -> &Dictionary {
    let descendants = type0(doc).get(b"DescendantFonts").unwrap();
    let first = &descendants.as_array().unwrap()[0];
    deref(doc, first).as_dict().unwrap()
}

/// Object number of the stream a dictionary entry points at.
fn stream_id(dict: &Dictionary, key: &[u8]) -> u32 {
    dict.get(key).unwrap().as_reference().unwrap().0
}

/// Where stream `id`'s raw data sits in `pdf` (it must occur exactly once).
fn data_range(pdf: &[u8], doc: &Document, id: u32) -> Range<usize> {
    let data = &doc
        .get_object((id, 0))
        .unwrap()
        .as_stream()
        .unwrap()
        .content;
    let at = find(pdf, data).expect("stream data in the file");
    assert_eq!(rfind(pdf, data), Some(at), "stream {id} data is not unique");
    at..at + data.len()
}

fn without(pdf: &[u8], span: Range<usize>) -> Vec<u8> {
    [&pdf[..span.start], &pdf[span.end..]].concat()
}

// ---------------------------------------------------------------------------
// The committed assets

#[test]
fn test_font_is_the_committed_subset() {
    assert_eq!(TEST_FONT.len(), 9_672);
    assert_eq!(
        hex(&Sha256::digest(TEST_FONT)),
        "6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383"
    );
    let font = FontRef::new(TEST_FONT).unwrap();
    assert_eq!(font.maxp().unwrap().num_glyphs(), 109);
    assert_eq!(font.charmap().mappings().count(), 104);
    assert_eq!(font.post().unwrap().version().to_major_minor(), (2, 0));
    let cmap = font.cmap().unwrap();
    let loca = font.loca(None).unwrap();
    let glyf = font.glyf().unwrap();
    for c in ['é', 'è', 'ç', 'ñ', 'ü'] {
        let gid = cmap.map_codepoint(c).unwrap();
        let glyph = loca.get_glyf(gid, &glyf).unwrap().unwrap();
        assert!(
            matches!(glyph, Glyph::Composite(_)),
            "{c} is not a composite"
        );
    }
}

#[test]
fn tiny_jpeg_is_the_committed_8x8_baseline() {
    assert_eq!(
        hex(&Sha256::digest(TINY_JPEG)),
        "b138c5b6855e4eb2c36341f8e9ff7d10f00d153d1583bf9327b91f38791cb1ea"
    );
    // SOF0 (baseline), 8 bits, 8 × 8, three components.
    let sof = find(TINY_JPEG, &[0xFF, 0xC0]).unwrap();
    assert_eq!(&TINY_JPEG[sof + 4..sof + 10], &[8, 0, 8, 0, 8, 3]);
}

#[test]
fn hayro_decodes_tiny_jpeg() {
    let pdf = Pdf::new(golden_pdf()).expect("hayro loads the golden");
    let page = &pdf.pages()[0];
    let image = page
        .resources()
        .get_x_object(&Name::new(b"Im1").unwrap())
        .expect("page 1 /Im1");
    assert_eq!(image.raw_data().as_ref(), TINY_JPEG);
    let params = ImageDecodeParams {
        bpc: Some(8),
        num_components: Some(3),
        width: 8,
        height: 8,
        ..ImageDecodeParams::default()
    };
    let decoded = image.decoded_image(&params).expect("DCT decode");
    assert_eq!(decoded.data.len(), 8 * 8 * 3);
}

// ---------------------------------------------------------------------------
// The goldens

#[test]
fn goldens_load_in_strict_lopdf() {
    for (label, bytes) in goldens() {
        let doc = load_strict(&bytes);
        assert_eq!(doc.get_pages().len(), 2, "{label}");
        assert!(bytes.starts_with(b"%PDF-1.7\n"), "{label}");
        assert_eq!(find(&bytes, b"/XRef"), None, "{label}: classic xref only");
        assert!(find(&bytes, b"\nxref\n0 ").is_some(), "{label}");

        let t0 = type0(&doc);
        assert_eq!(t0.get(b"Subtype").unwrap(), &name(b"Type0"));
        assert_eq!(t0.get(b"Encoding").unwrap(), &name(b"Identity-H"));
        let cid = cidfont(&doc);
        assert_eq!(cid.get(b"Subtype").unwrap(), &name(b"CIDFontType2"));
        assert_eq!(cid.get(b"CIDToGIDMap").unwrap(), &name(b"Identity"));
        let descriptor = deref(&doc, cid.get(b"FontDescriptor").unwrap())
            .as_dict()
            .unwrap();
        let file = doc
            .get_object((stream_id(descriptor, b"FontFile2"), 0))
            .unwrap()
            .as_stream()
            .unwrap();
        assert_eq!(file.decompressed_content().unwrap(), TEST_FONT, "{label}");
    }
}

#[test]
fn goldens_render_in_hayro_with_the_embedded_font() {
    for (label, bytes) in goldens() {
        let warnings = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&warnings);
        let settings = InterpreterSettings {
            // No fallback font: page 2 (text only) can only ink through the
            // embedded program.
            font_resolver: Arc::new(|_| None),
            warning_sink: Arc::new(move |_| {
                sink.fetch_add(1, Ordering::Relaxed);
            }),
            ..InterpreterSettings::default()
        };
        let pdf = Pdf::new(bytes).expect("hayro loads");
        assert_eq!(pdf.pages().len(), 2, "{label}");
        let cache = RenderCache::new();
        let scale = PixmapSettings {
            x_scale: 0.5,
            y_scale: 0.5,
            ..PixmapSettings::default()
        };
        for (i, page) in pdf.pages().iter().enumerate() {
            let pixmap = render(page, &cache, &settings, &RenderSettings::default(), &scale);
            let inked = pixmap.data_as_u8_slice().chunks(4).any(|px| px[3] > 0);
            assert!(inked, "{label}: page {} paints nothing", i + 1);
        }
        assert_eq!(warnings.load(Ordering::Relaxed), 0, "{label}: hayro warned");
    }
}

#[test]
fn content_glyph_ids_match_cmap_lookups() {
    let doc = load_strict(&golden_pdf());
    let font = FontRef::new(TEST_FONT).unwrap();
    let cmap = font.cmap().unwrap();
    for (n, page) in doc.get_pages() {
        let content = Content::decode(&doc.get_page_content(page)).unwrap();
        let drawn: Vec<Vec<u32>> = content
            .operations
            .iter()
            .filter(|op| op.operator == "Tj")
            .map(|op| match &op.operands[0] {
                Object::String(b, StringFormat::Hexadecimal) => b
                    .chunks(2)
                    .map(|p| u32::from(u16::from_be_bytes([p[0], p[1]])))
                    .collect(),
                other => panic!("Tj operand {other:?}"),
            })
            .collect();
        let expected: Vec<Vec<u32>> = GOLDEN_TEXT[n as usize - 1]
            .iter()
            .map(|line| {
                line.chars()
                    .map(|c| cmap.map_codepoint(c).unwrap().to_u32())
                    .collect()
            })
            .collect();
        assert_eq!(drawn, expected, "page {n}");
    }
}

#[test]
fn widths_are_the_fonts_advances_for_every_drawn_glyph() {
    let doc = load_strict(&golden_pdf());
    let font = FontRef::new(TEST_FONT).unwrap();
    let upem = u32::from(font.head().unwrap().units_per_em());
    let hmtx = font.hmtx().unwrap();
    let cmap = font.cmap().unwrap();
    let w = cidfont(&doc).get(b"W").unwrap().as_array().unwrap();
    let mut widths = BTreeMap::new();
    for pair in w.chunks(2) {
        let gid = pair[0].as_i64().unwrap() as u32;
        let ws = pair[1].as_array().unwrap();
        for (k, width) in ws.iter().enumerate() {
            widths.insert(gid + k as u32, width.as_i64().unwrap());
        }
    }
    for c in GOLDEN_TEXT
        .iter()
        .flat_map(|p| p.iter())
        .flat_map(|l| l.chars())
    {
        let gid = cmap.map_codepoint(c).unwrap();
        let advance = u32::from(hmtx.advance(gid).unwrap());
        assert_eq!(
            widths.get(&gid.to_u32()).copied(),
            Some(i64::from(advance * 1000 / upem)),
            "width of {c:?}"
        );
    }
}

#[test]
fn tounicode_maps_every_drawn_glyph_back_to_its_char() {
    let doc = load_strict(&golden_pdf());
    let cmap_id = stream_id(type0(&doc), b"ToUnicode");
    let stream = doc.get_object((cmap_id, 0)).unwrap().as_stream().unwrap();
    let text = String::from_utf8(stream.decompressed_content().unwrap()).unwrap();
    let block = &text[text.find("beginbfchar").unwrap()..text.find("endbfchar").unwrap()];
    let mut map = BTreeMap::new();
    for line in block.lines().skip(1) {
        let (src, dst) = line.split_once(' ').unwrap();
        let gid = u32::from_str_radix(src.trim_matches(['<', '>']), 16).unwrap();
        let unit = u32::from_str_radix(dst.trim_matches(['<', '>']), 16).unwrap();
        map.insert(gid, char::from_u32(unit).unwrap());
    }
    let font = FontRef::new(TEST_FONT).unwrap();
    let cmap = font.cmap().unwrap();
    for c in GOLDEN_TEXT
        .iter()
        .flat_map(|p| p.iter())
        .flat_map(|l| l.chars())
    {
        let gid = cmap.map_codepoint(c).unwrap().to_u32();
        assert_eq!(map.get(&gid), Some(&c), "gid {gid}");
    }
}

#[test]
fn two_flate_content_streams_under_512_bytes_and_a_dct_image() {
    let doc = load_strict(&golden_pdf());
    for page in doc.get_pages().values() {
        let page = doc.get_dictionary(*page).unwrap();
        let contents = deref(&doc, page.get(b"Contents").unwrap())
            .as_stream()
            .unwrap();
        assert_eq!(contents.dict.get(b"Filter").unwrap(), &name(b"FlateDecode"));
        assert!(contents.content.len() < 512, "{} B", contents.content.len());
    }
    let page1 = doc.get_dictionary(doc.get_pages()[&1]).unwrap();
    let resources = page1.get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    let image = deref(&doc, xobjects.get(b"Im1").unwrap())
        .as_stream()
        .unwrap();
    assert_eq!(image.dict.get(b"Filter").unwrap(), &name(b"DCTDecode"));
    assert_eq!(image.content, TINY_JPEG);
}

#[test]
fn signed_golden_has_a_sig_whose_byte_range_brackets_its_contents() {
    assert_eq!(find(&golden_pdf(), b"/Sig"), None);
    let bytes = golden_pdf_signed();
    let doc = load_strict(&bytes);
    let catalog = doc.catalog().unwrap();
    let acroform = catalog.get(b"AcroForm").unwrap().as_dict().unwrap();
    let field = deref(
        &doc,
        &acroform.get(b"Fields").unwrap().as_array().unwrap()[0],
    )
    .as_dict()
    .unwrap();
    assert_eq!(field.get(b"FT").unwrap(), &name(b"Sig"));
    let sig = deref(&doc, field.get(b"V").unwrap()).as_dict().unwrap();
    assert_eq!(sig.get(b"Type").unwrap(), &name(b"Sig"));
    assert_eq!(
        sig.get(b"Contents").unwrap(),
        &Object::String(vec![0; 64], StringFormat::Hexadecimal)
    );
    let br: Vec<usize> = sig
        .get(b"ByteRange")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as usize)
        .collect();
    assert_eq!(br[0], 0);
    assert_eq!(
        &bytes[br[1]..br[2]],
        format!("<{}>", "0".repeat(128)).as_bytes()
    );
    assert_eq!(br[2] + br[3], bytes.len());
}

#[test]
fn goldens_are_byte_identical_across_builds() {
    assert_eq!(golden_pdf(), golden_pdf());
    assert_eq!(golden_pdf_signed(), golden_pdf_signed());
}

// ---------------------------------------------------------------------------
// The corruptors

#[test]
fn c1_overwrites_the_first_12_bytes() {
    for (label, g) in goldens() {
        for seed in SEEDS {
            let out = corrupt(CorruptionClass::C1Header, &g, seed).unwrap();
            assert_eq!(out.len(), g.len(), "{label}");
            assert_eq!(out[12..], g[12..], "{label} seed {seed}");
            assert_ne!(out[..12], g[..12], "{label} seed {seed}");
        }
    }
}

#[test]
fn c2_deletes_the_xref_table() {
    for (label, g) in goldens() {
        let start = find(&g, b"\nxref\n").unwrap() + 1;
        let end = find(&g, b"\ntrailer\n").unwrap() + 1;
        let out = corrupt(CorruptionClass::C2XrefMissing, &g, 0).unwrap();
        assert_eq!(out, without(&g, start..end), "{label}");
    }
}

#[test]
fn c3_deletes_trailer_through_eof() {
    for (label, g) in goldens() {
        let start = find(&g, b"\ntrailer\n").unwrap() + 1;
        let end = rfind(&g, b"%%EOF").unwrap() + 5;
        let out = corrupt(CorruptionClass::C3TrailerDamaged, &g, 0).unwrap();
        assert_eq!(out, without(&g, start..end), "{label}");
        assert_eq!(find(&out, b"%%EOF"), None, "{label}");
    }
}

#[test]
fn c4_deletes_45_to_128_bytes_over_pages_through_count() {
    for (label, g) in goldens() {
        let pages_at = find(&g, b"/Type/Pages").unwrap() + 5;
        let count_end = find(&g, b"/Count 2").unwrap() + 8;
        let mut lens = std::collections::BTreeSet::new();
        for seed in SEEDS {
            let out = corrupt(CorruptionClass::C4PageTreeBroken, &g, seed).unwrap();
            let len = g.len() - out.len();
            assert!((45..=128).contains(&len), "{label} seed {seed}: {len} B");
            let covering = (15..=pages_at)
                .filter(|&s| s + len >= count_end)
                .any(|s| without(&g, s..s + len) == out);
            assert!(covering, "{label} seed {seed}: no covering span");
            lens.insert(len);
        }
        assert!(lens.len() > 1, "{label}: the seed never moves the length");
    }
}

#[test]
fn c5_deletes_exactly_one_8_byte_object_header() {
    for (label, g) in goldens() {
        let mut hit = std::collections::BTreeSet::new();
        for seed in SEEDS {
            let out = corrupt(CorruptionClass::C5ObjectTagStripped, &g, seed).unwrap();
            assert_eq!(g.len() - out.len(), 8, "{label} seed {seed}");
            let at: Vec<usize> = (1..g.len() - 8)
                .filter(|&s| g[s - 1] == b'\n' && g[s].is_ascii_digit() && g[s] != b'0')
                .filter(|&s| &g[s + 1..s + 8] == b" 0 obj\n")
                .filter(|&s| without(&g, s..s + 8) == out)
                .collect();
            assert_eq!(at.len(), 1, "{label} seed {seed}: {at:?}");
            hit.insert(at[0]);
        }
        assert!(hit.len() > 1, "{label}: the seed never moves the header");
    }
}

#[test]
fn c6_deletes_one_pages_font_entry() {
    const ENTRY: &[u8] = b"/Font<</F1 5 0 R>>";
    for (label, g) in goldens() {
        let entries: Vec<usize> = (0..g.len())
            .filter(|&s| g[s..].starts_with(ENTRY) && g[..s].ends_with(b"/Resources<<"))
            .collect();
        assert_eq!(entries.len(), 2, "{label}: one per page");
        let mut hit = std::collections::BTreeSet::new();
        for seed in SEEDS {
            let out = corrupt(CorruptionClass::C6FontMapLost, &g, seed).unwrap();
            let at: Vec<usize> = entries
                .iter()
                .copied()
                .filter(|&s| without(&g, s..s + ENTRY.len()) == out)
                .collect();
            assert_eq!(at.len(), 1, "{label} seed {seed}");
            hit.insert(at[0]);
        }
        assert_eq!(hit.len(), 2, "{label}: both pages are reachable");
    }
}

/// `out` differs from `g` exactly on `spans`, which are all 0x20.
fn assert_blanked(label: &str, g: &[u8], out: &[u8], spans: &[Range<usize>]) {
    assert_eq!(out.len(), g.len(), "{label}");
    for (i, (&a, &b)) in g.iter().zip(out).enumerate() {
        if spans.iter().any(|s| s.contains(&i)) {
            assert_eq!(b, b' ', "{label}: byte {i} not blanked");
        } else {
            assert_eq!(a, b, "{label}: byte {i} changed");
        }
    }
}

#[test]
fn c7_blanks_the_font_program_in_place() {
    for (label, g) in goldens() {
        let doc = load_strict(&g);
        let descriptor = deref(&doc, cidfont(&doc).get(b"FontDescriptor").unwrap())
            .as_dict()
            .unwrap();
        let font = data_range(&g, &doc, stream_id(descriptor, b"FontFile2"));
        let out = corrupt(CorruptionClass::C7FontStreamDeleted, &g, 0).unwrap();
        assert_blanked(label, &g, &out, &[font]);
    }
}

#[test]
fn c8_blanks_the_font_program_and_tounicode_in_place() {
    for (label, g) in goldens() {
        let doc = load_strict(&g);
        let descriptor = deref(&doc, cidfont(&doc).get(b"FontDescriptor").unwrap())
            .as_dict()
            .unwrap();
        let font = data_range(&g, &doc, stream_id(descriptor, b"FontFile2"));
        let cmap = data_range(&g, &doc, stream_id(type0(&doc), b"ToUnicode"));
        let out = corrupt(CorruptionClass::C8FontResourcesDeleted, &g, 0).unwrap();
        assert_blanked(label, &g, &out, &[font, cmap]);
    }
}

#[test]
fn c9_is_not_built_yet() {
    let g = golden_pdf();
    assert_eq!(corrupt(CorruptionClass::C9ZlibTampered, &g, 0), Err(NotYet));
}

#[test]
fn c10_keeps_exactly_70_percent() {
    for (label, g) in goldens() {
        let out = corrupt(CorruptionClass::C10Truncated, &g, 0).unwrap();
        assert_eq!(out.len(), g.len() * 7 / 10, "{label}");
        assert_eq!(out, g[..out.len()], "{label}");
    }
}

#[test]
fn corrupt_is_deterministic_per_seed() {
    for (label, g) in goldens() {
        for class in CorruptionClass::ALL {
            for seed in SEEDS {
                assert_eq!(
                    corrupt(class, &g, seed),
                    corrupt(class, &g, seed),
                    "{label} {} seed {seed}",
                    class.code()
                );
            }
        }
    }
}

#[test]
fn the_generator_is_splitmix64() {
    // The reference sequence for seed 0, so a seed names the same corruption
    // on every platform and release.
    let mut rng = SplitMix64(0);
    assert_eq!(rng.next(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(rng.next(), 0x6E78_9E6A_A1B9_65F4);
}
