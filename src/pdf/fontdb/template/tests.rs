use super::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use lopdf::{Document, Encoding, LoadOptions};
use skrifa::MetadataProvider;
use skrifa::raw::FontRef;

use crate::engine::FontDb;
use crate::pdf::fixtures::TEST_FONT;
use crate::pdf::fontdb::build::IndexEntry;

/// FR-05's pins: id, file size, SHA-256.
const PINNED: [(&str, usize, &str); 2] = [
    (
        "NotoSans-Regular",
        431_364,
        "f3961a9cde016d41a4879aecda1474d3a36d6bf54fa0e4643de029cc2248b0e8",
    ),
    (
        "NotoSerif-Regular",
        482_540,
        "a15cfbbc1539d707115111d672d590a3d70d4f74b4c0a315956da20ae19a14e1",
    ),
];

const OFL_NOTO: &[u8] = include_bytes!("../../../../assets/licenses/OFL-Noto.txt");
const OFL_NOTO_SHA256: &str = "cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a";

/// The bundled database's hash with the committed assets. It changes only
/// when `tools/build-templates` writes different assets.
const BUNDLED_SHA256: &str = "a33819310fd6c59ce673c411e0bfc4c82040e9ab02d949a1a8de46d5db8aee05";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn index() -> Vec<IndexEntry> {
    serde_json::from_slice(BUNDLED_INDEX).expect("fontindex.json parses")
}

fn load_strict(bytes: &[u8]) -> Document {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(1 << 20),
        ..LoadOptions::default()
    };
    Document::load_mem_with_options(bytes, opts).expect("strict lopdf load")
}

fn gmap_of(font: &BundledFont) -> GmapTable<'static> {
    GmapTable::new(font.gmap).expect("bundled gmap loads")
}

#[test]
fn bundled_assets_have_the_pinned_hashes() {
    for (font, &(id, len, sha)) in BUNDLED.iter().zip(&PINNED) {
        assert_eq!(font.id, id);
        assert_eq!(font.ttf.len(), len, "{id}");
        assert_eq!(hex(&Sha256::digest(font.ttf)), sha, "{id}");
    }
    assert_eq!(hex(&Sha256::digest(OFL_NOTO)), OFL_NOTO_SHA256);
}

#[test]
fn bundled_fonts_follow_the_index() {
    let index = index();
    let ids: Vec<&str> = index.iter().map(|e| e.id.as_str()).collect();
    let bundled: Vec<&str> = BUNDLED.iter().map(|f| f.id).collect();
    assert_eq!(ids, bundled);
    for (entry, font) in index.iter().zip(&BUNDLED) {
        assert_eq!(entry.sha256, hex(&Sha256::digest(font.ttf)), "{}", entry.id);
        assert_eq!(
            entry.gmap_sha256,
            hex(&Sha256::digest(font.gmap)),
            "{}",
            entry.id
        );
    }
}

/// The committed gmaps and index entries are what `build_from_ttf` gives
/// today (the aliases are curated by the tool, so they are taken as committed).
#[test]
fn committed_assets_match_the_builder() {
    for (entry, font) in index().into_iter().zip(&BUNDLED) {
        let (mut built, gmap) = build_from_ttf(font.ttf).expect("bundled font builds");
        built.base_font_aliases = entry.base_font_aliases.clone();
        assert_eq!(built, entry, "{}", font.id);
        assert!(gmap == font.gmap, "{}: committed gmap is stale", font.id);
    }
}

#[test]
fn gmap_entry_count_equals_the_cmap_count() {
    for font in &BUNDLED {
        let cmap = FontRef::new(font.ttf).unwrap().charmap().mappings().count();
        assert_eq!(cmap, 2_965, "{}", font.id);
        assert_eq!(gmap_of(font).len(), cmap, "{}", font.id);
    }
}

#[test]
fn bundled_fonts_map_the_three_latin_blocks() {
    for font in &BUNDLED {
        let gids = cmap_gids(&gmap_of(font));
        let missing: Vec<char> = LATIN_COVERAGE
            .iter()
            .flat_map(|&(lo, hi)| lo..=hi)
            .filter(|c| !gids.contains_key(c))
            .collect();
        assert!(missing.is_empty(), "{}: {missing:?}", font.id);
    }
}

#[test]
fn template_reloads_strictly_with_the_full_font() {
    for font in &BUNDLED {
        let gmap = gmap_of(font);
        let doc = load_strict(&build(font.ttf, &gmap).unwrap());
        assert_eq!(doc.version, "1.7");
        assert_eq!(doc.get_pages().len(), 1);

        let type0 = doc.get_dictionary((TYPE0_FONT, 0)).unwrap();
        let name_of = |d: &Dictionary, k: &[u8]| d.get(k).unwrap().as_name().unwrap().to_vec();
        assert_eq!(name_of(type0, b"Subtype"), b"Type0");
        assert_eq!(name_of(type0, b"Encoding"), b"Identity-H");
        assert_eq!(name_of(type0, b"BaseFont"), font.id.as_bytes());
        let kids = type0.get(b"DescendantFonts").unwrap().as_array().unwrap();
        let cid = doc.get_dictionary(kids[0].as_reference().unwrap()).unwrap();
        assert_eq!(name_of(cid, b"Subtype"), b"CIDFontType2");
        assert_eq!(name_of(cid, b"CIDToGIDMap"), b"Identity");

        let descriptor = doc
            .get_dictionary(cid.get(b"FontDescriptor").unwrap().as_reference().unwrap())
            .unwrap();
        let program = doc
            .get_object(
                descriptor
                    .get(b"FontFile2")
                    .unwrap()
                    .as_reference()
                    .unwrap(),
            )
            .unwrap()
            .as_stream()
            .unwrap();
        let length1 = program.dict.get(b"Length1").unwrap().as_i64().unwrap();
        assert_eq!(usize::try_from(length1).unwrap(), font.ttf.len());
        let plain = program.decompressed_content().unwrap();
        assert!(plain == font.ttf, "{}: FontFile2 is the full font", font.id);

        // The page's font resource is the harvested subtree's root.
        let page = doc.get_dictionary(doc.get_pages()[&1]).unwrap();
        let fonts = page
            .get(b"Resources")
            .and_then(Object::as_dict)
            .and_then(|r| r.get(b"Font"))
            .and_then(Object::as_dict)
            .unwrap();
        assert_eq!(
            fonts.get(FONT_RESOURCE).unwrap(),
            &Object::Reference((TYPE0_FONT, 0))
        );
    }
}

/// `/W` parsed back to gid → width.
fn parsed_widths(w: &[Object]) -> BTreeMap<u16, u16> {
    let mut out = BTreeMap::new();
    for pair in w.chunks(2) {
        let first = u16::try_from(pair[0].as_i64().unwrap()).unwrap();
        for (k, width) in pair[1].as_array().unwrap().iter().enumerate() {
            let gid = first + u16::try_from(k).unwrap();
            out.insert(gid, u16::try_from(width.as_i64().unwrap()).unwrap());
        }
    }
    out
}

#[test]
fn widths_and_tounicode_come_from_the_gmap() {
    for font in &BUNDLED {
        let gmap = gmap_of(font);
        let doc = load_strict(&build(font.ttf, &gmap).unwrap());
        let cid = doc.get_dictionary((CIDFONT, 0)).unwrap();
        let w = parsed_widths(cid.get(b"W").unwrap().as_array().unwrap());
        let want: BTreeMap<u16, u16> = gmap
            .records()
            .iter()
            .map(|r| (r.gid(), gmap.width(r.gid()).unwrap()))
            .collect();
        assert_eq!(w, want, "{}", font.id);

        let type0 = doc.get_dictionary((TYPE0_FONT, 0)).unwrap();
        let Ok(Encoding::UnicodeMapEncoding(map)) = type0.get_font_encoding(&doc) else {
            panic!("{}: lopdf reads the ToUnicode", font.id);
        };
        for &gid in want.keys() {
            let c = gmap.unicode(gid).unwrap();
            let mut units = [0u16; 2];
            assert_eq!(
                map.get(u32::from(gid), 2),
                Some(c.encode_utf16(&mut units).to_vec()),
                "{}: gid {gid}",
                font.id
            );
        }
    }
}

/// Whether hayro inks the page using only the embedded program (no font
/// resolver), and how many warnings it raised.
fn render_page(bytes: Vec<u8>) -> (bool, usize) {
    let warnings = Arc::new(AtomicUsize::new(0));
    let sink = Arc::clone(&warnings);
    let settings = InterpreterSettings {
        font_resolver: Arc::new(|_| None),
        warning_sink: Arc::new(move |_| {
            sink.fetch_add(1, Ordering::Relaxed);
        }),
        ..InterpreterSettings::default()
    };
    let pdf = Pdf::new(bytes).expect("hayro loads");
    let pages = pdf.pages();
    assert_eq!(pages.len(), 1);
    let scale = PixmapSettings {
        x_scale: 0.5,
        y_scale: 0.5,
        ..PixmapSettings::default()
    };
    let pixmap = render(
        &pages[0],
        &RenderCache::new(),
        &settings,
        &RenderSettings::default(),
        &scale,
    );
    let inked = pixmap.data_as_u8_slice().chunks(4).any(|px| px[3] > 0);
    (inked, warnings.load(Ordering::Relaxed))
}

#[test]
fn template_renders_in_hayro() {
    for font in &BUNDLED {
        let (inked, warnings) = render_page(build(font.ttf, &gmap_of(font)).unwrap());
        assert!(inked, "{}: the page paints nothing", font.id);
        assert_eq!(warnings, 0, "{}: hayro warned", font.id);
    }
}

#[test]
fn template_is_the_same_on_every_build() {
    let font = &BUNDLED[0];
    let a = build(font.ttf, &gmap_of(font)).unwrap();
    assert_eq!(a, build(font.ttf, &gmap_of(font)).unwrap());
    assert_eq!(a, bundled(font.id).unwrap());
}

#[test]
fn page_draws_the_coverage_lines() {
    let font = &BUNDLED[1];
    let gmap = gmap_of(font);
    let doc = load_strict(&build(font.ttf, &gmap).unwrap());
    let content = doc
        .get_object((CONTENTS, 0))
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap();
    let text = String::from_utf8(content).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| l.ends_with(" Tj")).collect();
    // 95 + 96 + 128 characters, 32 to a line.
    assert_eq!(lines.len(), 10);
    let gid_a = cmap_gids(&gmap)[&' '];
    assert!(lines[0].starts_with(&format!("<{gid_a:04X}")));
    assert_eq!(lines[0].len(), 1 + 32 * 4 + 4);
}

#[test]
fn bundled_templates_are_built_once() {
    let a = bundled("NotoSans-Regular").unwrap();
    let b = bundled("NotoSans-Regular").unwrap();
    assert!(std::ptr::eq(a, b));
    assert!(!std::ptr::eq(a, bundled("NotoSerif-Regular").unwrap()));
    assert_eq!(
        bundled("Helvetica"),
        Err(BundledError::Unknown("Helvetica".into()))
    );
}

#[test]
fn a_font_the_builder_refuses_gives_no_template() {
    let gmap = gmap_of(&BUNDLED[0]);
    assert!(matches!(
        build(b"not a font", &gmap),
        Err(TemplateError::Font(BuildError::Parse(_)))
    ));
    // The test font builds with any gmap; widths follow the gmap given.
    let (_, small) = build_from_ttf(TEST_FONT).unwrap();
    let small = GmapTable::new(&small).unwrap();
    load_strict(&build(TEST_FONT, &small).unwrap());
}

#[test]
fn bundled_db_hash_is_stable() {
    let mut h = Sha256::new();
    h.update(BUNDLED_INDEX);
    for entry in index() {
        let font = BUNDLED.iter().find(|f| f.id == entry.id).unwrap();
        h.update(font.gmap);
    }
    let want: [u8; 32] = h.finalize().into();
    assert_eq!(bundled_sha256(), want);
    assert_eq!(FontDb::bundled().sha256(), want);
    assert_eq!(FontDb::bundled().sha256(), FontDb::bundled().sha256());
    assert_eq!(hex(&want), BUNDLED_SHA256);
    assert_ne!(FontDb::bundled().sha256(), FontDb::empty().sha256());
}
