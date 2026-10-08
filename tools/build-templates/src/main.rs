//! Builds the committed font assets (T-29, TD §5.1, D-021): every
//! `assets/fonts/*.ttf` goes through `build_from_ttf`, which gives its
//! `assets/gmaps/<id>.gmap` and its `assets/fontindex.json` entry. Template
//! PDFs are not files: the app builds them in memory at run time.
//!
//! `build-templates` writes the assets; `build-templates --check` writes
//! nothing and exits 1 if any committed file differs from what it would
//! write. A dev-time tool: nothing here reads a PDF, and the app never runs it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdfpundit::pdf::fontdb::build::{IndexEntry, build_from_ttf};

/// The `/BaseFont` names each bundled font stands in for: curated here, not
/// read from the font (TD §5.1).
const ALIASES: [(&str, &[&str]); 2] = [
    (
        "NotoSans-Regular",
        &["Arial", "ArialMT", "Helvetica", "LiberationSans"],
    ),
    (
        "NotoSerif-Regular",
        &[
            "LiberationSerif",
            "Times-Roman",
            "TimesNewRoman",
            "TimesNewRomanPSMT",
        ],
    ),
];

/// The blocks every bundled font must map in full: Basic Latin, the Latin-1
/// Supplement and Latin Extended-A, without their control characters.
const COVERAGE: [(char, char); 3] = [
    ('\u{20}', '\u{7e}'),
    ('\u{a0}', '\u{ff}'),
    ('\u{100}', '\u{17f}'),
];

/// One `.gmap` record's length; its code point is the little-endian `u32` at
/// bytes 2..6 (`src/pdf/fontdb/gmap.rs`).
const RECORD_LEN: usize = 9;

/// One file the tool writes: its path under the repository root and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Output {
    path: PathBuf,
    bytes: Vec<u8>,
}

fn main() -> ExitCode {
    let mut check = false;
    for arg in std::env::args().skip(1) {
        if arg == "--check" {
            check = true;
        } else {
            eprintln!("usage: build-templates [--check]");
            return ExitCode::from(2);
        }
    }
    let root = repo_root();
    let outputs = match generate(&root) {
        Ok(outputs) => outputs,
        Err(e) => {
            eprintln!("build-templates: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = if check {
        differences(&root, &outputs).map(|diff| {
            for path in &diff {
                eprintln!("differs: {}", path.display());
            }
            diff.is_empty()
        })
    } else {
        write(&root, &outputs).map(|()| true)
    };
    match result {
        Ok(true) => {
            for out in &outputs {
                println!("ok   {}", out.path.display());
            }
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("build-templates: committed assets are stale; run tools/build-templates");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("build-templates: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root: two levels above this tool's manifest.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fonts_dir() -> PathBuf {
    Path::new("assets").join("fonts")
}

fn gmaps_dir() -> PathBuf {
    Path::new("assets").join("gmaps")
}

fn index_path() -> PathBuf {
    Path::new("assets").join("fontindex.json")
}

/// Every output, the gmaps by font id and then the index. The bytes depend on
/// the font files alone.
fn generate(root: &Path) -> Result<Vec<Output>, String> {
    let dir = root.join(fonts_dir());
    let mut fonts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ttf"))
        .collect();
    fonts.sort();
    if fonts.is_empty() {
        return Err(format!("no .ttf under {}", dir.display()));
    }

    let mut entries: Vec<(IndexEntry, Vec<u8>)> = Vec::new();
    for path in &fonts {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (mut entry, gmap) =
            build_from_ttf(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let missing = uncovered(&gmap);
        if !missing.is_empty() {
            let list: Vec<String> = missing
                .iter()
                .map(|&c| format!("U+{:04X}", u32::from(c)))
                .collect();
            return Err(format!(
                "{} does not map {}",
                path.display(),
                list.join(" ")
            ));
        }
        entry.base_font_aliases = ALIASES
            .iter()
            .find(|(id, _)| *id == entry.id)
            .map(|(_, aliases)| aliases.iter().map(|a| (*a).to_owned()).collect())
            .unwrap_or_default();
        entries.push((entry, gmap));
    }
    entries.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    if let Some(pair) = entries.windows(2).find(|w| w[0].0.id == w[1].0.id) {
        return Err(format!("two fonts have the id {}", pair[0].0.id));
    }

    let mut outputs: Vec<Output> = entries
        .iter()
        .map(|(entry, gmap)| Output {
            path: gmaps_dir().join(format!("{}.gmap", entry.id)),
            bytes: gmap.clone(),
        })
        .collect();
    let index: Vec<&IndexEntry> = entries.iter().map(|(entry, _)| entry).collect();
    let mut json = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    json.push(b'\n');
    outputs.push(Output {
        path: index_path(),
        bytes: json,
    });
    Ok(outputs)
}

/// The [`COVERAGE`] characters `gmap` gives no glyph.
fn uncovered(gmap: &[u8]) -> Vec<char> {
    let mapped: BTreeSet<u32> = gmap
        .as_chunks::<RECORD_LEN>()
        .0
        .iter()
        .map(|r| u32::from_le_bytes([r[2], r[3], r[4], r[5]]))
        .collect();
    COVERAGE
        .iter()
        .flat_map(|&(lo, hi)| lo..=hi)
        .filter(|&c| !mapped.contains(&u32::from(c)))
        .collect()
}

/// The paths whose committed bytes differ from `outputs`, plus any committed
/// `.gmap` the tool would not write.
fn differences(root: &Path, outputs: &[Output]) -> Result<Vec<PathBuf>, String> {
    let mut diff = Vec::new();
    for out in outputs {
        if std::fs::read(root.join(&out.path)).ok().as_ref() != Some(&out.bytes) {
            diff.push(out.path.clone());
        }
    }
    diff.extend(stale_gmaps(root, outputs)?);
    Ok(diff)
}

/// The `.gmap` files under `assets/gmaps` that `outputs` does not name.
fn stale_gmaps(root: &Path, outputs: &[Output]) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(gmaps_dir());
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut stale = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = gmaps_dir().join(entry.file_name());
        if path.extension().is_some_and(|x| x == "gmap") && !outputs.iter().any(|o| o.path == path)
        {
            stale.push(path);
        }
    }
    stale.sort();
    Ok(stale)
}

/// Writes every output and removes stale `.gmap` files.
fn write(root: &Path, outputs: &[Output]) -> Result<(), String> {
    let gmaps = root.join(gmaps_dir());
    std::fs::create_dir_all(&gmaps).map_err(|e| format!("{}: {e}", gmaps.display()))?;
    for path in stale_gmaps(root, outputs)? {
        let path = root.join(path);
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    for out in outputs {
        let path = root.join(&out.path);
        std::fs::write(&path, &out.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
