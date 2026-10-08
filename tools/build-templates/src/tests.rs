use super::*;

use sha2::{Digest, Sha256};

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
fn outputs_are_the_two_gmaps_then_the_index() {
    let paths: Vec<PathBuf> = generate(&repo_root())
        .unwrap()
        .into_iter()
        .map(|o| o.path)
        .collect();
    assert_eq!(
        paths,
        [
            gmaps_dir().join("NotoSans-Regular.gmap"),
            gmaps_dir().join("NotoSerif-Regular.gmap"),
            index_path(),
        ]
    );
}

#[test]
fn the_index_lists_both_fonts_with_the_pinned_hashes() {
    let index = committed_index();
    let got: Vec<(&str, &str)> = index
        .iter()
        .map(|e| (e.id.as_str(), e.sha256.as_str()))
        .collect();
    let want: Vec<(&str, &str)> = PINNED.iter().map(|&(id, _, h)| (id, h)).collect();
    assert_eq!(got, want);
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
    assert_eq!(
        index[0].base_font_aliases,
        ["Arial", "ArialMT", "Helvetica", "LiberationSans"]
    );
    assert_eq!(
        index[1].base_font_aliases,
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
fn both_fonts_cover_the_three_latin_blocks() {
    for out in generate(&repo_root()).unwrap() {
        if out.path.extension().is_some_and(|x| x == "gmap") {
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
    let scratch = Scratch::new("missing");
    let root = &scratch.0;
    std::fs::remove_file(root.join(index_path())).unwrap();
    let outputs = generate(root).unwrap();
    assert_eq!(differences(root, &outputs).unwrap(), [index_path()]);
}
