//! Builds the committed font assets (T-29, TD §5.1, D-021): every
//! `assets/fonts/*.ttf` goes through `build_from_ttf`, which gives its
//! `assets/gmaps/<id>.gmap` and its `assets/fontindex.json` entry. Template
//! PDFs are not files: the app builds them in memory at run time.
//!
//! The [`EXTRAS`] (D-010 (b), G-10) are the corpus's other open-licensed
//! faces: google/fonts files whose `.gmap` and index entry are committed but
//! whose programs are not, because an output draws them with a bundled Noto
//! (the entry's `drawn_with`). `fetch-assets.sh` downloads them into
//! `font-sources/` (git-ignored). An extra that is not there keeps its
//! committed `.gmap` and entry, checked against its pin, so a checkout without
//! the downloads still builds and checks everything else.
//!
//! `build-templates` writes the assets; `build-templates --check` writes
//! nothing and exits 1 if any committed file differs from what it would
//! write. A dev-time tool: nothing here reads a PDF, and the app never runs it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdfpundit::pdf::fontdb::build::{IndexEntry, build_from_ttf};
use sha2::{Digest, Sha256};

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

/// The google/fonts commit every [`Extra`] is pinned at.
const GOOGLE_FONTS_COMMIT: &str = "2eb0b48d5f760f62e286216f0859a8c540dbc1bd";

/// The bundled font a sans-serif or display extra is drawn with.
const SANS: &str = "NotoSans-Regular";
/// The bundled font a serif extra is drawn with.
const SERIF: &str = "NotoSerif-Regular";

/// A font the database holds only the `.gmap` of (D-010 (b), G-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Extra {
    /// The family's Regular TTF under google/fonts at
    /// [`GOOGLE_FONTS_COMMIT`] (for a variable family, its one roman file).
    path: &'static str,
    /// Its SHA-256, lowercase hex.
    sha256: &'static str,
    /// The bundled font an output draws its glyphs with: [`SERIF`] for the
    /// families google/fonts files as serif (and Abril Fatface, a Didone
    /// filed as display), else [`SANS`].
    drawn_with: &'static str,
    /// Curated `/BaseFont` names it stands for, as [`ALIASES`].
    aliases: &'static [&'static str],
}

impl Extra {
    /// The file name `fetch-assets.sh` saves it under in `font-sources/`.
    fn file_name(&self) -> &'static str {
        self.path.rsplit('/').next().unwrap_or(self.path)
    }
}

const fn extra(path: &'static str, sha256: &'static str, drawn_with: &'static str) -> Extra {
    Extra {
        path,
        sha256,
        drawn_with,
        aliases: &[],
    }
}

/// The SIL OFL families of the REPDF originals' `/BaseFont` names that
/// google/fonts carries, one file each. Of the originals' 44 distinct names
/// (subset tags stripped, the producer's `CIDFont+Fn` names left out), these
/// 24 families cover 25. Skipped:
/// - not under an open licence: ArialMT, Calibri, Cambria, CambriaMath,
///   CourierNewPSMT, GmarketSansTTFMedium, MS-Mincho, Peignot, SimSun,
///   SymbolMT, TimesNewRomanPSMT;
/// - not in google/fonts: AaGuDianKeBenSong, BousungEG-Light-GB,
///   dinglieciweifont, KawkabMono-Regular, KingnammmMaiyuan2-Regular,
///   MaoKenWangXingYuan-Regular, zihunbiantaoti;
/// - already bundled: NotoSerif-Thin (Noto Serif).
const EXTRAS: [Extra; 24] = [
    extra(
        "ofl/abrilfatface/AbrilFatface-Regular.ttf",
        "5971d4a3758a922a9fedc7f6fb825a96341a2e718c45a4b2c9a6b417c8c4dbe9",
        SERIF,
    ),
    extra(
        "ofl/archivoblack/ArchivoBlack-Regular.ttf",
        "dd9a89a019b4849f66ab75455fe7bdf931311042cbb0f0f97acc061539703180",
        SANS,
    ),
    extra(
        "ofl/batang/Batang-Regular.ttf",
        "0929031e799b2feadda22208c58f503515e6f8fa2eaba75acd2e6847d73fc54b",
        SERIF,
    ),
    extra(
        "ofl/belgrano/Belgrano-Regular.ttf",
        "5bf095dfbc56718bea7d74c0b30c36413714aaf8833d2cb012b604b64fd383d9",
        SERIF,
    ),
    extra(
        "ofl/bigshoulders/BigShoulders[opsz,wght].ttf",
        "4b4b24aa6f799aa73cdcd5b6fa840cbcbbb38b81fa9fa82c25126a4530c1ba44",
        SANS,
    ),
    extra(
        "ofl/biryani/Biryani-Regular.ttf",
        "0b846b4f8600e7943a3a86a2f7ce04c20daa7d2cdad74c951edcc8e07c367116",
        SANS,
    ),
    extra(
        "ofl/bricolagegrotesque/BricolageGrotesque[opsz,wdth,wght].ttf",
        "413e7357809ddd12fd80a96a8a396de0e401638d4acd3cb3e37532f0472ac682",
        SANS,
    ),
    extra(
        "ofl/cormorant/Cormorant[wght].ttf",
        "8f12cb21f05b61649192eaff13eeeb1b5619bc524feeae672fb916974259a076",
        SERIF,
    ),
    extra(
        "ofl/harmattan/Harmattan-Regular.ttf",
        "5fbaafc51ad21663729b168afaff5f28d3e3087f1fc736ada32d7cd99674c0e0",
        SANS,
    ),
    extra(
        "ofl/hind/Hind-Regular.ttf",
        "01de158022f53077b52303e46de3b0ab5fb245222a7ffe25a2a57fdd9e969162",
        SANS,
    ),
    extra(
        "ofl/marhey/Marhey[wght].ttf",
        "19c30686157ec965797fae8c8feb7bb142b082217227562d1a809c067df447e9",
        SANS,
    ),
    extra(
        "ofl/merriweather/Merriweather[opsz,wdth,wght].ttf",
        "d0ed0e359e396af7ad05e73dffd11a3a4c326ea0d0283c56bd9361cb2cc86a96",
        SERIF,
    ),
    extra(
        "ofl/mukta/Mukta-Regular.ttf",
        "2958e4af564507df2a856164df6f9978dacb03f999a4f34a0c269dc8a4de9688",
        SANS,
    ),
    extra(
        "ofl/nobile/Nobile-Regular.ttf",
        "fc2eab24ea3dbe7f5d80324fa5c5d3ea5098175755c3218e3d54cec4355987f2",
        SANS,
    ),
    extra(
        "ofl/notosanskr/NotoSansKR[wght].ttf",
        "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252",
        SANS,
    ),
    Extra {
        path: "ofl/opensans/OpenSans[wdth,wght].ttf",
        sha256: "36643644f318a812aab2d2ed3bb98f8cf0872527f835fe9398d95fe6b9adb878",
        drawn_with: SANS,
        // The variable font's name-ID-25 prefix: what a producer writes for
        // its default (Regular) instance.
        aliases: &["OpenSansRoman"],
    },
    extra(
        "ofl/oswald/Oswald[wght].ttf",
        "5b38c246e255a12f5712d640d56bcced0472466fc68983d2d0410ec0457c2817",
        SANS,
    ),
    extra(
        "ofl/playfairdisplay/PlayfairDisplay[wght].ttf",
        "c40f2293766a503bc70cce9e512ef844a4ccb7cbcde792fe2ea31d191917d8d6",
        SERIF,
    ),
    extra(
        "ofl/poppins/Poppins-Regular.ttf",
        "7e65201e9b79159e2300267cc885e16c8dcef2424cdfa09a29bfb0980a94a7ba",
        SANS,
    ),
    extra(
        "ofl/prompt/Prompt-Regular.ttf",
        "dbd497803cec3caffbc6b7f599ca6fed8beea0ed7e0ad1e098130c5ebbc4fd42",
        SANS,
    ),
    extra(
        "ofl/rubik/Rubik[wght].ttf",
        "1b3a7437ba2af80e465e773ed60c5036d1ba6ace492d89046dbcf18fb31e4e88",
        SANS,
    ),
    extra(
        "ofl/tajawal/Tajawal-Regular.ttf",
        "6882892da3e03527d5db2bbab3b48bde6ef2e878a43f522d1a4eebda90010a19",
        SANS,
    ),
    extra(
        "ofl/teko/Teko[wght].ttf",
        "d1321889f262bbbff632e7976349853399cd097b6f382d4b19790c915c13c1ae",
        SANS,
    ),
    extra(
        "ofl/zcoolxiaowei/ZCOOLXiaoWei-Regular.ttf",
        "a42b620140f493db42f741351dfbf343c0936d58588ee8004b8b2a218d997ff1",
        SANS,
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
    for x in unfetched(&root, &EXTRAS) {
        eprintln!(
            "kept {} @ {GOOGLE_FONTS_COMMIT} (not in {}; fetch-assets.sh downloads it)",
            x.path,
            sources_dir().display()
        );
    }
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

/// Where `fetch-assets.sh` saves the [`EXTRAS`]: git-ignored, never committed.
fn sources_dir() -> PathBuf {
    PathBuf::from("font-sources")
}

/// The extras of `extras` whose file is not under `root`'s [`sources_dir`].
fn unfetched<'x>(root: &Path, extras: &'x [Extra]) -> Vec<&'x Extra> {
    extras
        .iter()
        .filter(|x| !root.join(sources_dir()).join(x.file_name()).is_file())
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Every output, the gmaps by font id and then the index. The bytes depend on
/// the font files alone.
fn generate(root: &Path) -> Result<Vec<Output>, String> {
    generate_with(root, &EXTRAS)
}

/// [`generate`] with `extras` as the `.gmap`-only fonts.
fn generate_with(root: &Path, extras: &[Extra]) -> Result<Vec<Output>, String> {
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
    let committed = committed_entries(root);
    for x in extras {
        if !entries
            .iter()
            .any(|(e, _)| e.id == x.drawn_with && e.drawn_with.is_none())
        {
            return Err(format!(
                "{}: drawn with {}, which is not a bundled font",
                x.path, x.drawn_with
            ));
        }
        entries.push(extra_entry(root, x, &committed)?);
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

/// The committed index under `root`, or none when it is missing or does not
/// parse.
fn committed_entries(root: &Path) -> Vec<IndexEntry> {
    std::fs::read(root.join(index_path()))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Extra `x`'s index entry and `.gmap`: built from its file in
/// [`sources_dir`] when that is there (its hash must be the pin), else the
/// committed ones, which must name the pinned file, match their `.gmap` and
/// carry the curated fields. No coverage is asked of an extra: its glyphs are
/// drawn with a bundled font.
fn extra_entry(
    root: &Path,
    x: &Extra,
    committed: &[IndexEntry],
) -> Result<(IndexEntry, Vec<u8>), String> {
    let source = root.join(sources_dir()).join(x.file_name());
    if let Ok(bytes) = std::fs::read(&source) {
        let got = sha256_hex(&bytes);
        if got != x.sha256 {
            return Err(format!(
                "{}: SHA-256 {got}, pinned {}",
                source.display(),
                x.sha256
            ));
        }
        let (mut entry, gmap) =
            build_from_ttf(&bytes).map_err(|e| format!("{}: {e}", source.display()))?;
        entry.drawn_with = Some(x.drawn_with.to_owned());
        entry.base_font_aliases = x.aliases.iter().map(|a| (*a).to_owned()).collect();
        return Ok((entry, gmap));
    }
    let not_fetched = |why: &str| {
        format!(
            "{}: not in {} and {why}; run fetch-assets.sh",
            x.path,
            sources_dir().display()
        )
    };
    let entry = committed
        .iter()
        .find(|e| e.sha256 == x.sha256)
        .ok_or_else(|| not_fetched("no committed index entry names it"))?;
    let curated = entry.drawn_with.as_deref() == Some(x.drawn_with)
        && entry
            .base_font_aliases
            .iter()
            .map(String::as_str)
            .eq(x.aliases.iter().copied());
    if !curated {
        return Err(not_fetched("its committed entry's curated fields differ"));
    }
    let path = root.join(gmaps_dir()).join(format!("{}.gmap", entry.id));
    let gmap = std::fs::read(&path)
        .ok()
        .filter(|g| sha256_hex(g) == entry.gmap_sha256)
        .ok_or_else(|| not_fetched("its committed .gmap is missing or not the indexed one"))?;
    Ok((entry.clone(), gmap))
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
