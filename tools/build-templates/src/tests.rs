use super::*;

/// FR-05's pins: id, file size, SHA-256 of the unhinted Noto Regular files.
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

/// Each Noto file's `cmap` maps 2,965 code points (FR-05).
const CMAP_COUNT: usize = 2_965;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The ids of the fonts with a program, as PINNED lists them.
fn programs() -> Vec<&'static str> {
    PINNED.iter().map(|&(id, _, _)| id).collect()
}

fn committed_index() -> Vec<IndexEntry> {
    let bytes = std::fs::read(repo_root().join(index_path())).expect("committed fontindex.json");
    serde_json::from_slice(&bytes).expect("fontindex.json parses")
}

#[test]
fn check_passes_on_the_committed_files() {
    let root = repo_root();
    let outputs = generate(&root).expect("assets build");
    assert_eq!(differences(&root, &outputs).unwrap(), Vec::<PathBuf>::new());
}

#[test]
fn two_runs_are_byte_identical() {
    let root = repo_root();
    assert_eq!(generate(&root).unwrap(), generate(&root).unwrap());
}

#[test]
fn outputs_are_every_gmap_by_id_then_the_index() {
    let paths: Vec<PathBuf> = generate(&repo_root())
        .unwrap()
        .into_iter()
        .map(|o| o.path)
        .collect();
    let mut want: Vec<PathBuf> = committed_index()
        .iter()
        .map(|e| gmaps_dir().join(format!("{}.gmap", e.id)))
        .collect();
    assert_eq!(want.len(), PINNED.len() + EXTRAS.len());
    assert!(want.is_sorted());
    want.push(index_path());
    assert_eq!(paths, want);
}

#[test]
fn the_index_lists_every_font_with_the_pinned_hashes() {
    let index = committed_index();
    let got: Vec<(&str, &str)> = index
        .iter()
        .filter(|e| e.drawn_with.is_none())
        .map(|e| (e.id.as_str(), e.sha256.as_str()))
        .collect();
    let want: Vec<(&str, &str)> = PINNED.iter().map(|&(id, _, h)| (id, h)).collect();
    assert_eq!(got, want);
    // Each extra once, by its pin, drawn with the Noto its list names.
    for x in &EXTRAS {
        let found: Vec<&IndexEntry> = index.iter().filter(|e| e.sha256 == x.sha256).collect();
        assert_eq!(found.len(), 1, "{}", x.path);
        assert_eq!(
            found[0].drawn_with.as_deref(),
            Some(x.drawn_with),
            "{}",
            x.path
        );
        assert!(programs().contains(&x.drawn_with), "{}", x.path);
    }
    for &(id, len, sha) in &PINNED {
        let bytes = std::fs::read(repo_root().join(fonts_dir()).join(format!("{id}.ttf"))).unwrap();
        assert_eq!(bytes.len(), len, "{id}");
        assert_eq!(hex(&Sha256::digest(&bytes)), sha, "{id}");
    }
    for entry in &index {
        let gmap = std::fs::read(
            repo_root()
                .join(gmaps_dir())
                .join(format!("{}.gmap", entry.id)),
        )
        .unwrap();
        assert_eq!(
            entry.gmap_sha256,
            hex(&Sha256::digest(&gmap)),
            "{}",
            entry.id
        );
        if entry.drawn_with.is_some() {
            continue;
        }
        assert_eq!(
            entry.family,
            if entry.id.starts_with("NotoSans") {
                "Noto Sans"
            } else {
                "Noto Serif"
            }
        );
        assert!(
            entry
                .languages
                .starts_with(&["en".into(), "fr".into(), "es".into()]),
            "{entry:?}"
        );
    }
}

#[test]
fn the_index_carries_the_curated_aliases() {
    let index = committed_index();
    let aliases = |id: &str| {
        index
            .iter()
            .find(|e| e.id == id)
            .expect("indexed")
            .base_font_aliases
            .clone()
    };
    assert_eq!(
        aliases("NotoSans-Regular"),
        ["Arial", "ArialMT", "Helvetica", "LiberationSans"]
    );
    assert_eq!(aliases("OpenSans-Regular"), ["OpenSansRoman"]);
    assert_eq!(
        aliases("NotoSerif-Regular"),
        [
            "LiberationSerif",
            "Times-Roman",
            "TimesNewRoman",
            "TimesNewRomanPSMT"
        ]
    );
}

#[test]
fn gmap_entry_count_equals_the_cmap_count() {
    for &(id, _, _) in &PINNED {
        let gmap = std::fs::read(repo_root().join(gmaps_dir()).join(format!("{id}.gmap"))).unwrap();
        assert_eq!(gmap.len(), CMAP_COUNT * RECORD_LEN, "{id}");
    }
}

#[test]
fn both_fonts_with_a_program_cover_the_three_latin_blocks() {
    let drawn = |out: &Output| {
        programs()
            .iter()
            .any(|id| out.path == gmaps_dir().join(format!("{id}.gmap")))
    };
    for out in generate(&repo_root()).unwrap() {
        if drawn(&out) {
            assert_eq!(
                uncovered(&out.bytes),
                Vec::<char>::new(),
                "{}",
                out.path.display()
            );
        }
    }
}

#[test]
fn coverage_names_every_missing_character() {
    // One record per code point of the three blocks but `é` and `Ÿ`.
    let gmap: Vec<u8> = COVERAGE
        .iter()
        .flat_map(|&(lo, hi)| lo..=hi)
        .filter(|&c| c != 'é' && c != 'Ÿ')
        .flat_map(|c| {
            let mut r = vec![0u8; RECORD_LEN];
            r[2..6].copy_from_slice(&u32::from(c).to_le_bytes());
            r
        })
        .collect();
    assert_eq!(uncovered(&gmap), ['é', 'Ÿ']);
    assert_eq!(uncovered(&[]).len(), 95 + 96 + 128);
}

/// A scratch copy of the committed assets the tests may write into.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "pdfpundit-build-templates-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in [fonts_dir(), gmaps_dir()] {
            std::fs::create_dir_all(dir.join(&sub)).unwrap();
            for entry in std::fs::read_dir(repo_root().join(&sub)).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), dir.join(&sub).join(entry.file_name())).unwrap();
            }
        }
        std::fs::copy(repo_root().join(index_path()), dir.join(index_path())).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn check_reports_a_changed_gmap_and_a_stale_one_and_write_repairs_both() {
    let scratch = Scratch::new("stale");
    let root = &scratch.0;
    let sans = gmaps_dir().join("NotoSans-Regular.gmap");
    let mut bytes = std::fs::read(root.join(&sans)).unwrap();
    bytes[4] ^= 1;
    std::fs::write(root.join(&sans), bytes).unwrap();
    let stale = gmaps_dir().join("Gone-Regular.gmap");
    std::fs::write(root.join(&stale), b"").unwrap();

    let outputs = generate(root).unwrap();
    assert_eq!(differences(root, &outputs).unwrap(), [sans, stale.clone()]);
    write(root, &outputs).unwrap();
    assert_eq!(differences(root, &outputs).unwrap(), Vec::<PathBuf>::new());
    assert!(!root.join(stale).exists());
}

#[test]
fn check_reports_a_missing_index() {
    // With every source at hand, the index is all that differs.
    let scratch = serif_only("missing");
    let root = &scratch.0;
    with_source(&scratch, "Subset-Regular.ttf", SUBSET);
    let extras = [subset_extra()];
    write(root, &generate_with(root, &extras).unwrap()).unwrap();
    std::fs::remove_file(root.join(index_path())).unwrap();
    let outputs = generate_with(root, &extras).unwrap();
    assert_eq!(differences(root, &outputs).unwrap(), [index_path()]);
    // Without the sources, an extra has nothing to be kept from.
    let scratch = Scratch::new("missing-unfetched");
    std::fs::remove_file(scratch.0.join(index_path())).unwrap();
    let why = generate(&scratch.0).unwrap_err();
    assert!(why.contains("run fetch-assets.sh"), "{why}");
}

// ── the .gmap-only extras (G-10) ─────────────────────────────────────────

/// A scratch whose `font-sources/` holds `name` with `bytes`.
fn with_source(scratch: &Scratch, name: &str, bytes: &[u8]) {
    let dir = scratch.0.join(sources_dir());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), bytes).unwrap();
}

const SUBSET: &[u8] = include_bytes!("../../../tests/data/NotoSans-Regular-subset.ttf");

/// The test subset as an extra: it maps far less than the Latin blocks.
fn subset_extra() -> Extra {
    Extra {
        path: "ofl/subset/Subset-Regular.ttf",
        sha256: "6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383",
        drawn_with: SERIF,
        aliases: &["SubsetAlias"],
    }
}

/// A scratch holding NotoSerif as its only font with a program, so that the
/// subset (whose id is `NotoSans-Regular`) can be an extra.
fn serif_only(label: &str) -> Scratch {
    let scratch = Scratch::new(label);
    std::fs::remove_file(scratch.0.join(fonts_dir()).join("NotoSans-Regular.ttf")).unwrap();
    scratch
}

#[test]
fn an_extra_is_built_from_its_source_without_coverage_and_drawn_with_a_noto() {
    let scratch = serif_only("extra-source");
    with_source(&scratch, "Subset-Regular.ttf", SUBSET);
    let x = subset_extra();
    assert_eq!(hex(&Sha256::digest(SUBSET)), x.sha256);
    let outputs = generate_with(&scratch.0, &[x]).unwrap();
    let index: Vec<IndexEntry> = serde_json::from_slice(&outputs.last().unwrap().bytes).unwrap();
    let ids: Vec<&str> = index.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["NotoSans-Regular", "NotoSerif-Regular"]);
    let extra = &index[0];
    assert_eq!(extra.sha256, x.sha256);
    assert_eq!(extra.drawn_with.as_deref(), Some(SERIF));
    assert_eq!(extra.base_font_aliases, ["SubsetAlias"]);
    assert_eq!(index[1].drawn_with, None);
    let gmap = &outputs[0];
    assert_eq!(gmap.path, gmaps_dir().join("NotoSans-Regular.gmap"));
    assert_eq!(extra.gmap_sha256, hex(&Sha256::digest(&gmap.bytes)));
    assert!(!uncovered(&gmap.bytes).is_empty(), "no coverage is asked");

    // Written once, the extra is kept from the committed files without its
    // source, byte for byte.
    write(&scratch.0, &outputs).unwrap();
    std::fs::remove_dir_all(scratch.0.join(sources_dir())).unwrap();
    assert_eq!(unfetched(&scratch.0, &[x]), [&x]);
    assert_eq!(generate_with(&scratch.0, &[x]).unwrap(), outputs);
}

#[test]
fn an_unfetched_extra_keeps_its_committed_gmap_and_entry() {
    // The scratch has no font-sources/: every extra is kept as committed.
    let scratch = Scratch::new("extra-kept");
    assert_eq!(unfetched(&scratch.0, &EXTRAS).len(), EXTRAS.len());
    let outputs = generate(&scratch.0).unwrap();
    assert_eq!(
        differences(&scratch.0, &outputs).unwrap(),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn an_extra_is_refused_when_its_source_or_committed_files_are_wrong() {
    let x = subset_extra();
    let refused = |scratch: &Scratch, x: Extra| generate_with(&scratch.0, &[x]).unwrap_err();

    // A source that is not the pinned file.
    let scratch = serif_only("extra-bad-source");
    let mut other = SUBSET.to_vec();
    other[100] ^= 1;
    with_source(&scratch, "Subset-Regular.ttf", &other);
    assert!(refused(&scratch, x).contains("pinned"));

    // No source and nothing committed.
    let scratch = serif_only("extra-not-fetched");
    let why = refused(&scratch, x);
    assert!(why.contains("run fetch-assets.sh"), "{why}");

    // No source, and the committed entry's curated fields or .gmap differ.
    let scratch = serif_only("extra-stale");
    with_source(&scratch, "Subset-Regular.ttf", SUBSET);
    write(&scratch.0, &generate_with(&scratch.0, &[x]).unwrap()).unwrap();
    std::fs::remove_dir_all(scratch.0.join(sources_dir())).unwrap();
    let recurated = Extra { aliases: &[], ..x };
    assert!(refused(&scratch, recurated).contains("curated"));
    let gmap = scratch.0.join(gmaps_dir()).join("NotoSans-Regular.gmap");
    let mut bytes = std::fs::read(&gmap).unwrap();
    bytes[4] ^= 1;
    std::fs::write(&gmap, bytes).unwrap();
    assert!(refused(&scratch, x).contains(".gmap"));

    // Drawn with a font that has no program.
    let scratch = serif_only("extra-no-program");
    with_source(&scratch, "Subset-Regular.ttf", SUBSET);
    let unknown = Extra {
        drawn_with: SANS,
        ..x
    };
    assert!(refused(&scratch, unknown).contains("not a bundled font"));
}

/// `fetch-assets.sh` downloads every extra at the pinned commit and checks
/// the pinned hash.
#[test]
fn fetch_assets_pins_every_extra() {
    let script = std::fs::read_to_string(repo_root().join("fetch-assets.sh")).unwrap();
    assert!(script.contains(GOOGLE_FONTS_COMMIT));
    for x in &EXTRAS {
        let line = script
            .lines()
            .find(|l| l.contains(x.sha256))
            .unwrap_or_else(|| panic!("fetch-assets.sh lacks {}", x.sha256));
        assert!(line.contains(x.path), "{line}");
    }
}
