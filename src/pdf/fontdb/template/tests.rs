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
/// when `tools/build-templates` writes different assets (last: G-10's
/// `.gmap`-only fonts).
const BUNDLED_SHA256: &str = "94ec53a6190061fa76f6efa471fbf177879a94d1240a8e21b0e06abd5b2a680a";

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

/// The bundled fonts that carry a program (the two Noto fonts), each with
/// its program.
fn programs() -> Vec<(&'static BundledFont, &'static [u8])> {
    BUNDLED.iter().filter_map(|f| Some((f, f.ttf?))).collect()
}

fn gmap_of(font: &BundledFont) -> GmapTable<'static> {
    GmapTable::new(font.gmap).expect("bundled gmap loads")
}

#[test]
fn bundled_assets_have_the_pinned_hashes() {
    assert_eq!(programs().len(), PINNED.len());
    for ((font, ttf), &(id, len, sha)) in programs().into_iter().zip(&PINNED) {
        assert_eq!(font.id, id);
        assert_eq!(ttf.len(), len, "{id}");
        assert_eq!(hex(&Sha256::digest(ttf)), sha, "{id}");
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
        // A font indexed with `drawn_with` carries no program (G-10).
        assert_eq!(
            font.ttf.is_none(),
            entry.drawn_with.is_some(),
            "{}",
            entry.id
        );
        if let Some(ttf) = font.ttf {
            assert_eq!(entry.sha256, hex(&Sha256::digest(ttf)), "{}", entry.id);
        }
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
        let Some(ttf) = font.ttf else { continue };
        let (mut built, gmap) = build_from_ttf(ttf).expect("bundled font builds");
        built.base_font_aliases = entry.base_font_aliases.clone();
        assert_eq!(built, entry, "{}", font.id);
        assert!(gmap == font.gmap, "{}: committed gmap is stale", font.id);
    }
}

#[test]
fn gmap_entry_count_equals_the_cmap_count() {
    for (font, ttf) in programs() {
        let cmap = FontRef::new(ttf).unwrap().charmap().mappings().count();
        assert_eq!(cmap, 2_965, "{}", font.id);
        assert_eq!(gmap_of(font).len(), cmap, "{}", font.id);
    }
}

#[test]
fn bundled_fonts_map_the_three_latin_blocks() {
    for (font, _) in programs() {
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
    for (font, ttf) in programs() {
        let gmap = gmap_of(font);
        let doc = load_strict(&build(ttf, &gmap).unwrap());
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
        assert_eq!(usize::try_from(length1).unwrap(), ttf.len());
        let plain = program.decompressed_content().unwrap();
        assert!(plain == ttf, "{}: FontFile2 is the full font", font.id);

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
    for (font, ttf) in programs() {
        let gmap = gmap_of(font);
        let doc = load_strict(&build(ttf, &gmap).unwrap());
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
    for (font, ttf) in programs() {
        let (inked, warnings) = render_page(build(ttf, &gmap_of(font)).unwrap());
        assert!(inked, "{}: the page paints nothing", font.id);
        assert_eq!(warnings, 0, "{}: hayro warned", font.id);
    }
}

#[test]
fn template_is_the_same_on_every_build() {
    let (font, ttf) = programs()[0];
    let a = build(ttf, &gmap_of(font)).unwrap();
    assert_eq!(a, build(ttf, &gmap_of(font)).unwrap());
    assert_eq!(a, bundled(font.id).unwrap());
}

#[test]
fn page_draws_the_coverage_lines() {
    let (font, ttf) = programs()[1];
    let gmap = gmap_of(font);
    let doc = load_strict(&build(ttf, &gmap).unwrap());
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
    for font in BUNDLED.iter().filter(|f| f.ttf.is_none()) {
        assert_eq!(
            bundled(font.id),
            Err(BundledError::NoProgram(font.id.into()))
        );
    }
}

#[test]
fn a_font_the_builder_refuses_gives_no_template() {
    let gmap = gmap_of(programs()[0].0);
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

// ── the harvest (T-30) ───────────────────────────────────────────────────

/// The test font's template and `.gmap`, and its glyph for `c`.
fn test_template() -> (Vec<u8>, Vec<u8>) {
    let (_, gmap_bytes) = build_from_ttf(TEST_FONT).unwrap();
    let gmap = GmapTable::new(&gmap_bytes).unwrap();
    (build(TEST_FONT, &gmap).unwrap(), gmap_bytes.clone())
}

fn glyph(gmap: &GmapTable, c: char) -> u16 {
    cmap_gids(gmap)[&c]
}

#[test]
fn a_harvest_is_numbered_where_it_lands() {
    let (template, gmap_bytes) = test_template();
    let gmap = GmapTable::new(&gmap_bytes).unwrap();
    let a = glyph(&gmap, 'A');
    let used = BTreeMap::from([(a, 'A')]);
    let harvest = harvest(&template, &gmap, &used).unwrap();
    // The codes are the font's own glyphs: no map, five objects.
    assert_eq!(harvest.len(), 5);
    let objects = harvest.numbered(100);
    let numbers: Vec<u32> = objects.iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers, [100, 101, 102, 103, 104]);
    let type0 = objects[0].1.as_dict().unwrap();
    assert_eq!(type0.get(b"Subtype").unwrap().as_name().unwrap(), b"Type0");
    assert_eq!(
        type0.get(b"DescendantFonts").unwrap(),
        &Object::Array(vec![Object::Reference((101, 0))])
    );
    assert_eq!(
        type0.get(b"ToUnicode").unwrap(),
        &Object::Reference((104, 0))
    );
    let cid = objects[1].1.as_dict().unwrap();
    assert_eq!(
        cid.get(b"FontDescriptor").unwrap(),
        &Object::Reference((102, 0))
    );
    assert_eq!(
        cid.get(b"CIDToGIDMap").unwrap().as_name().unwrap(),
        b"Identity"
    );
    let descriptor = objects[2].1.as_dict().unwrap();
    assert_eq!(
        descriptor.get(b"FontFile2").unwrap(),
        &Object::Reference((103, 0))
    );
    // The program is the template's, byte for byte.
    let doc = Document::load_mem(&template).unwrap();
    let program = doc.get_object((FONTFILE2, 0)).unwrap().as_stream().unwrap();
    assert_eq!(objects[3].1.as_stream().unwrap().content, program.content);
    // The /ToUnicode maps only the code used.
    let cmap = objects[4]
        .1
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap();
    assert_eq!(cmap, build_tounicode(&used));
}

#[test]
fn foreign_codes_get_a_cid_to_gid_map() {
    let (template, gmap_bytes) = test_template();
    let gmap = GmapTable::new(&gmap_bytes).unwrap();
    let (a, b) = (glyph(&gmap, 'A'), glyph(&gmap, 'b'));
    // Codes 3 and 7 are not this font's glyphs for A and b.
    let used = BTreeMap::from([(3, 'A'), (7, 'b'), (9, '\u{4e2d}')]);
    let harvest = harvest(&template, &gmap, &used).unwrap();
    assert_eq!(harvest.len(), 6);
    let objects = harvest.numbered(40);
    let cid = objects[1].1.as_dict().unwrap();
    assert_eq!(
        cid.get(b"CIDToGIDMap").unwrap(),
        &Object::Reference((45, 0))
    );
    let map = objects[5]
        .1
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap();
    let gid_at = |code: usize| u16::from_be_bytes([map[2 * code], map[2 * code + 1]]);
    assert_eq!(map.len(), 20);
    // A glyph the font lacks draws .notdef.
    assert_eq!((gid_at(3), gid_at(7), gid_at(9), gid_at(0)), (a, b, 0, 0));
    // /W gives each code its glyph's width.
    let Object::Array(w) = cid.get(b"W").unwrap() else {
        panic!("/W")
    };
    let parsed = parsed_widths(w);
    assert_eq!(parsed[&3], gmap.width(a).unwrap());
    assert_eq!(parsed[&7], gmap.width(b).unwrap());
    assert_eq!(parsed[&9], 0);
}

#[test]
fn harvest_from_the_database_matches_its_template() {
    let db = FontDb::bundled();
    let used = BTreeMap::from([(36, 'A')]);
    let font = harvest_from(&db, "NotoSans-Regular", &used).unwrap();
    let gmap = db.gmap("NotoSans-Regular").unwrap();
    let direct = harvest(
        db.template("NotoSans-Regular").unwrap().unwrap(),
        &gmap,
        &used,
    )
    .unwrap();
    assert_eq!(font, direct);
    assert!(matches!(
        harvest_from(&db, "NoSuchFont", &used),
        Err(HarvestError::Unknown(_))
    ));
}

/// A `.gmap`-only font is drawn with the program its entry names (G-10): its
/// codes, read through its own `.gmap`, are mapped to that program's glyphs.
#[test]
fn harvest_from_a_gmap_only_font_draws_with_its_program_font() {
    let db = FontDb::bundled();
    let (entry, program) = db
        .entries()
        .into_iter()
        .find_map(|e| Some((e, e.drawn_with.as_deref()?)))
        .expect("a .gmap-only font is bundled");
    let own = db.gmap(&entry.id).unwrap();
    // Two of the font's own codes, read through its own .gmap.
    let used: BTreeMap<u16, char> = own
        .records()
        .iter()
        .filter(|r| r.unicode().is_ascii_alphabetic())
        .take(2)
        .map(|r| (r.gid(), r.unicode()))
        .collect();
    assert_eq!(used.len(), 2);
    let font = harvest_from(&db, &entry.id, &used).unwrap();
    let direct = harvest(
        db.template(program).unwrap().unwrap(),
        &db.gmap(program).unwrap(),
        &used,
    )
    .unwrap();
    assert_eq!(font, direct);
    let objects = font.numbered(1);
    let type0 = objects[0].1.as_dict().unwrap();
    assert_eq!(
        type0.get(b"BaseFont").unwrap().as_name().unwrap(),
        program.as_bytes(),
        "the output names the font that draws it"
    );
}
