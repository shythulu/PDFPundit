use super::*;

use crate::engine::FontDbError;
use crate::pdf::fixtures::TEST_FONT;

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A one-font database of the test font: its index and blobs.
fn one_font() -> (Vec<u8>, Vec<u8>) {
    let (entry, gmap) = build::build_from_ttf(TEST_FONT).unwrap();
    (serde_json::to_vec(&[entry]).unwrap(), gmap)
}

fn load(index: &[u8], ttf: &[u8], gmap: &[u8]) -> Result<FontDb, FontDbError> {
    let blob = |name: &str| match name {
        "NotoSans-Regular.ttf" => Some(ttf),
        "NotoSans-Regular.gmap" => Some(gmap),
        _ => None,
    };
    FontDb::from_bytes(index, &blob)
}

#[test]
fn bundled_db_lists_the_bundled_fonts_and_its_hash_is_stable() {
    let db = FontDb::bundled();
    let ids: Vec<&str> = db.entries().iter().map(|e| e.id.as_str()).collect();
    assert!(ids.is_sorted(), "{ids:?}");
    // The two Noto fonts with programs and the corpus's other open-licensed
    // faces, .gmap only (G-10).
    assert_eq!(ids.len(), 26);
    for id in [
        "NotoSans-Regular",
        "NotoSerif-Regular",
        "AbrilFatface-Regular",
        "OpenSans-Regular",
        "ZCOOLXiaoWei-Regular",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    assert_eq!(db.sha256(), template::bundled_sha256());
    assert_eq!(FontDb::bundled().sha256(), db.sha256());
    assert!(Arc::ptr_eq(&db, &FontDb::bundled()), "loaded once");
    // Loading the same assets again, from a fresh copy, gives the same hash.
    assert_eq!(FontDb::compiled_in().sha256(), db.sha256());
    assert_eq!(FontDb::compiled_in(), *db);
    assert_eq!(db.gmap("NotoSerif-Regular").unwrap().len(), 2965);
    assert!(db.gmap("Helvetica").is_none());
    assert_eq!(db.entry("NotoSans-Regular").unwrap().family, "Noto Sans");
}

#[test]
fn only_the_bundled_db_carries_the_bundled_word_lists() {
    use dict::Dictionary;
    let db = FontDb::bundled();
    assert!(std::ptr::eq(db.word_lists(), dict::bundled()));
    let langs: Vec<_> = db.word_lists().iter().map(|l| l.lang()).collect();
    assert_eq!(
        langs,
        [
            crate::bench::metrics::Lang::En,
            crate::bench::metrics::Lang::Fr,
            crate::bench::metrics::Lang::Es
        ]
    );
    assert!(FontDb::compiled_in().word_lists().is_empty());
    assert!(FontDb::empty().word_lists().is_empty());
    let (index, gmap) = one_font();
    let built = load(&index, TEST_FONT, &gmap).unwrap();
    assert!(built.word_lists().is_empty());
    let with = built.clone().with_word_lists(dict::bundled());
    assert_eq!(with.word_lists().len(), 3);
    assert_eq!(with.sha256(), built.sha256(), "the lists are not hashed");
}

#[test]
fn the_assets_directory_loads_as_the_compiled_in_db() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    let db = FontDb::from_dir(&dir).expect("assets/ loads");
    assert_eq!(db.sha256(), FontDb::bundled().sha256());
    assert!(matches!(
        FontDb::from_dir(&dir.join("no-such-dir")),
        Err(FontDbError::MissingBlob(_))
    ));
}

#[test]
fn bundled_templates_are_built_lazily_and_kept() {
    let db = FontDb::bundled();
    let a = db.template("NotoSans-Regular").unwrap().unwrap();
    let b = db.template("NotoSans-Regular").unwrap().unwrap();
    assert!(std::ptr::eq(a, b));
    assert_eq!(a, template::bundled("NotoSans-Regular").unwrap());
    assert!(db.template("Helvetica").is_none());
}

#[test]
fn from_bytes_checks_every_blob() {
    let (index, gmap) = one_font();
    let db = load(&index, TEST_FONT, &gmap).unwrap();
    assert_eq!(db.entries().len(), 1);
    let mut want = Sha256::new();
    want.update(&index);
    want.update(&gmap);
    assert_eq!(db.sha256(), <[u8; 32]>::from(want.finalize()));

    let mut tampered = gmap.clone();
    tampered[4] ^= 1;
    assert_eq!(
        load(&index, TEST_FONT, &tampered).unwrap_err(),
        FontDbError::HashMismatch {
            name: "NotoSans-Regular.gmap".into()
        }
    );
    let mut ttf = TEST_FONT.to_vec();
    ttf[100] ^= 1;
    assert_eq!(
        load(&index, &ttf, &gmap).unwrap_err(),
        FontDbError::HashMismatch {
            name: "NotoSans-Regular.ttf".into()
        }
    );
    let blob = |name: &str| (name == "NotoSans-Regular.ttf").then_some(TEST_FONT);
    assert_eq!(
        FontDb::from_bytes(&index, &blob).unwrap_err(),
        FontDbError::MissingBlob("NotoSans-Regular.gmap".into())
    );
    assert!(matches!(
        load(b"not json", TEST_FONT, &gmap),
        Err(FontDbError::BadIndex(_))
    ));

    // A .gmap that matches its hash but is no .gmap.
    let (mut entry, _) = build::build_from_ttf(TEST_FONT).unwrap();
    let junk = [1u8; 10];
    entry.gmap_sha256 = hex(&junk);
    let index = serde_json::to_vec(&[entry.clone()]).unwrap();
    assert!(matches!(
        load(&index, TEST_FONT, &junk),
        Err(FontDbError::BadIndex(_))
    ));
    // An id indexed twice.
    let (entry, gmap) = build::build_from_ttf(TEST_FONT).unwrap();
    let twice = serde_json::to_vec(&[entry.clone(), entry]).unwrap();
    assert!(matches!(
        load(&twice, TEST_FONT, &gmap),
        Err(FontDbError::BadIndex(_))
    ));
}

#[test]
fn empty_db_has_no_fonts() {
    let db = FontDb::empty();
    assert!(db.entries().is_empty());
    assert!(db.inference_gmaps().is_empty());
    assert_eq!(db.sha256(), <[u8; 32]>::from(Sha256::digest([])));
    let empty_index = load(b"[]", TEST_FONT, &[]).unwrap();
    assert!(empty_index.entries().is_empty());
    assert_ne!(empty_index, db, "the hash covers the index bytes");
}

/// The test font's `.gmap` with every glyph id moved up by one: a font whose
/// glyph ids are not the test font's.
fn shifted_gmap() -> Vec<u8> {
    let (_, gmap) = build::build_from_ttf(TEST_FONT).unwrap();
    let table = GmapTable::new(&gmap).unwrap();
    gmap::encode(
        table
            .records()
            .iter()
            .map(|r| gmap::GmapRecord::new(r.gid() + 1, r.unicode(), r.width(), r.source()))
            .collect(),
    )
}

/// The test font, then `Other-Regular`, a font with only a `.gmap` (the
/// shifted one), drawn with `drawn_with`.
fn with_gmap_only(drawn_with: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (entry, gmap) = build::build_from_ttf(TEST_FONT).unwrap();
    let other_gmap = shifted_gmap();
    let mut other = entry.clone();
    other.id = "Other-Regular".into();
    other.postscript_name = "Other-Regular".into();
    other.family = "Other".into();
    other.sha256 = hex(b"the program is not in the database");
    other.gmap_sha256 = hex(&other_gmap);
    other.drawn_with = Some(drawn_with.into());
    (
        serde_json::to_vec(&[entry, other]).unwrap(),
        gmap,
        other_gmap,
    )
}

fn load_with_gmap_only(index: &[u8], gmap: &[u8], other: &[u8]) -> Result<FontDb, FontDbError> {
    let blob = |name: &str| match name {
        "NotoSans-Regular.ttf" => Some(TEST_FONT),
        "NotoSans-Regular.gmap" => Some(gmap),
        "Other-Regular.gmap" => Some(other),
        _ => None,
    };
    FontDb::from_bytes(index, &blob)
}

#[test]
fn a_gmap_only_font_loads_without_a_program_and_names_the_font_that_draws_it() {
    let (index, gmap, other) = with_gmap_only("NotoSans-Regular");
    let db = load_with_gmap_only(&index, &gmap, &other).unwrap();
    let ids: Vec<&str> = db.entries().iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["NotoSans-Regular", "Other-Regular"]);
    let inferred: Vec<&str> = db.inference_gmaps().iter().map(|(id, _)| *id).collect();
    assert_eq!(inferred, ["NotoSans-Regular"], "matched by name only");
    let first = |id: &str| db.gmap(id).unwrap().records()[0].gid();
    assert_eq!(first("Other-Regular"), first("NotoSans-Regular") + 1);
    assert_eq!(db.program_of("Other-Regular"), Some("NotoSans-Regular"));
    assert_eq!(db.program_of("NotoSans-Regular"), Some("NotoSans-Regular"));
    assert_eq!(db.program_of("Helvetica"), None);
    assert!(
        db.template("Other-Regular").is_none(),
        "no program, no template"
    );
    assert!(db.template("NotoSans-Regular").unwrap().is_ok());
    // The hash covers the index and both glyph maps, in index order.
    let mut want = Sha256::new();
    want.update(&index);
    want.update(&gmap);
    want.update(&other);
    assert_eq!(db.sha256(), <[u8; 32]>::from(want.finalize()));
}

#[test]
fn a_gmap_only_font_must_be_drawn_with_a_font_that_has_a_program() {
    for bad in ["Helvetica", "Other-Regular"] {
        let (index, gmap, other) = with_gmap_only(bad);
        assert!(
            matches!(
                load_with_gmap_only(&index, &gmap, &other),
                Err(FontDbError::BadIndex(_))
            ),
            "{bad}"
        );
    }
    let (index, gmap, _) = with_gmap_only("NotoSans-Regular");
    let blob = |name: &str| match name {
        "NotoSans-Regular.ttf" => Some(TEST_FONT),
        "NotoSans-Regular.gmap" => Some(gmap.as_slice()),
        _ => None,
    };
    assert_eq!(
        FontDb::from_bytes(&index, &blob).unwrap_err(),
        FontDbError::MissingBlob("Other-Regular.gmap".into())
    );
}

#[test]
fn the_bundled_db_draws_every_gmap_only_font_with_a_bundled_noto() {
    let db = FontDb::bundled();
    let noto = ["NotoSans-Regular", "NotoSerif-Regular"];
    let mut gmap_only = 0;
    for e in db.entries() {
        match &e.drawn_with {
            None => assert!(noto.contains(&e.id.as_str()), "{} has a program", e.id),
            Some(by) => {
                assert!(noto.contains(&by.as_str()), "{} drawn with {by}", e.id);
                assert!(db.template(&e.id).is_none());
                gmap_only += 1;
            }
        }
    }
    assert_eq!(gmap_only, db.entries().len() - noto.len());
    assert!(gmap_only > 0, "the corpus families are indexed (G-10)");
    for id in noto {
        assert!(db.template(id).unwrap().is_ok(), "{id}");
    }
}
