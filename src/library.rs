//! The history store (TD §6): [`HistoryStore`] and its JSON backend.
//!
//! ```text
//! <data_dir>/history/
//!   index.json            Vec<FileSummary>: a cache, rebuilt from files/ when
//!                         it is unreadable or disagrees with the records
//!   files/<sha256>.json   FileRecord { meta: FileEntry, runs: Vec<RunRecord> }
//!   files/<sha256>.json.corrupt
//!                         a record that did not parse, or named another hash,
//!                         moved aside so the store can go on without it
//! ```
//!
//! Every write is temp file → fsync → rename (→ fsync of the directory on
//! Unix). A record write first removes `index.json`, so a crash between the
//! record and the index leaves no index and the next open rebuilds it. The
//! store replaces only its own files; D-044's no-clobber rule is for outputs
//! beside user files. Paths are kept losslessly, non-UTF-8 ones included
//! (see [`path_json`]). Times are unix seconds handed in by the
//! caller (`jobs.rs` reads the clock, D-076), so this file carries no lint
//! allow of any kind (a test checks it); the temporary dead-code allow sits on
//! the `mod` line in `lib.rs`.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::appdirs::AppDirs;
use crate::engine::{AnalysisStateUse, RepairReport};
use crate::jobs::Placed;

/// What the idle screen shows of the history (eng-r3-q4): totals and the five
/// newest files. T-16's store builds it; the view model reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HistorySummary {
    pub files: u64,
    pub runs: u64,
    /// At most 5, newest first.
    pub recent: Vec<RecentRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentRow {
    pub name: String,
    pub status: RecentStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecentStatus {
    Repaired,
    Partial,
    Pending,
    Failed,
}

/// One repair run: the report plus the run facts the report may not carry
/// (D-073), because they depend on the machine, the destination or the
/// runner's state rather than on the input, version, settings and replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Unix seconds, read in `jobs.rs` (D-076). Never reaches an artefact.
    pub started_at: u64,
    pub input_sha256: [u8; 32],
    pub input_name: String,
    #[serde(with = "path_json::option")]
    pub output_path: Option<PathBuf>,
    /// Which no-replace guarantee held (D-044, D-073).
    pub placed: Option<Placed>,
    /// Reused or rebuilt, and why (D-073).
    pub analysis_state: AnalysisStateUse,
    /// The config file the run loaded (eng-r2-q10).
    #[serde(with = "path_json")]
    pub config_path: PathBuf,
    pub report: RepairReport,
}

/// A file the user fed the cat, keyed by the SHA-256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub sha256: [u8; 32],
    /// The name as shown.
    pub name: String,
    /// `None` for an input with no durable path (D-039).
    #[serde(with = "path_json::option")]
    pub path: Option<PathBuf>,
    pub size: u64,
    /// Unix seconds, from the caller.
    pub added_at: u64,
    /// Set by the caller from T-20's mapping; the store keeps it.
    pub status: RecentStatus,
}

/// One row of `index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSummary {
    pub sha256: [u8; 32],
    pub name: String,
    #[serde(with = "path_json::option")]
    pub path: Option<PathBuf>,
    pub size: u64,
    pub added_at: u64,
    /// The later of `added_at` and the newest run's `started_at`.
    pub last_at: u64,
    pub status: RecentStatus,
    pub runs: u64,
    /// The byte length of `files/<sha256>.json` as written with this row.
    /// Open compares it with the file, so a row the record has moved past
    /// is caught without parsing the record.
    pub record_len: u64,
}

/// What `files/<sha256>.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub meta: FileEntry,
    pub runs: Vec<RunRecord>,
}

impl FileRecord {
    fn summary(&self, record_len: u64) -> FileSummary {
        let m = &self.meta;
        let newest_run = self.runs.iter().map(|r| r.started_at).max();
        FileSummary {
            sha256: m.sha256,
            name: m.name.clone(),
            path: m.path.clone(),
            size: m.size,
            added_at: m.added_at,
            last_at: newest_run.map_or(m.added_at, |t| t.max(m.added_at)),
            status: m.status,
            runs: self.runs.len() as u64,
            record_len,
        }
    }
}

/// The history behind a minimal interface (TD §6).
pub trait HistoryStore {
    /// Adds the file, or replaces its entry and keeps its runs.
    fn upsert_file(&mut self, file: FileEntry) -> io::Result<()>;
    /// Appends a run to the file `run.input_sha256` names and sets that file's
    /// status. `NotFound` if the file was never upserted.
    fn record_run(&mut self, run: RunRecord, status: RecentStatus) -> io::Result<()>;
    /// Files whose name or path contains `filter` (case-insensitive; `""`
    /// matches all), newest first.
    fn list_files(&self, filter: &str) -> Vec<FileSummary>;
    /// The file's runs in the order they were recorded; empty if unknown.
    fn runs_for(&self, sha256: &[u8; 32]) -> io::Result<Vec<RunRecord>>;
    /// Removes the file and its runs; `false` if it was not there.
    fn delete_file(&mut self, sha256: &[u8; 32]) -> io::Result<bool>;
    /// Totals and the five newest files, for the idle screen.
    fn summary(&self) -> HistorySummary;
}

/// The JSON store. It owns its directory; one process uses it at a time.
#[derive(Debug)]
pub struct JsonStore {
    root: PathBuf,
    /// Sorted by `sha256`.
    index: Vec<FileSummary>,
}

const INDEX: &str = "index.json";
const FILES: &str = "files";
const TMP_SUFFIX: &str = ".tmp";
const CORRUPT_SUFFIX: &str = ".corrupt";

/// What reading `files/<sha256>.json` found.
enum OnDisk {
    Missing,
    /// It does not parse, or its `meta.sha256` is not the hash in its name.
    Corrupt,
    Record {
        record: FileRecord,
        len: u64,
    },
}

impl JsonStore {
    /// The store in `<data_dir>/history`.
    pub fn open(dirs: &AppDirs) -> io::Result<JsonStore> {
        JsonStore::open_at(dirs.data_dir.join("history"))
    }

    /// The store in `root`, created if missing. Leftover temp files from an
    /// interrupted write are removed; the index is rebuilt from `files/` when
    /// it is unreadable or disagrees with the records (see [`index_matches`]).
    pub fn open_at(root: impl Into<PathBuf>) -> io::Result<JsonStore> {
        let root = root.into();
        let files = root.join(FILES);
        fs::create_dir_all(&files)?;
        remove_temp_files(&root)?;
        remove_temp_files(&files)?;
        let on_disk = record_hashes(&files)?;
        let cached = fs::read(root.join(INDEX))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<FileSummary>>(&bytes).ok())
            .filter(|index| index_matches(index, &files, &on_disk));
        let mut store = JsonStore {
            root,
            index: Vec::new(),
        };
        match cached {
            Some(index) => store.index = index,
            None => store.rebuild_index(&on_disk)?,
        }
        Ok(store)
    }

    /// Re-reads every record in `files/` and rewrites `index.json`. A corrupt
    /// record is moved aside; one that cannot be read is kept out of the
    /// index and tried again on the next open.
    fn rebuild_index(&mut self, hashes: &BTreeSet<[u8; 32]>) -> io::Result<()> {
        let mut index = Vec::with_capacity(hashes.len());
        for sha in hashes {
            match self.read_record(sha) {
                Ok(OnDisk::Record { record, len }) => index.push(record.summary(len)),
                Ok(OnDisk::Corrupt) => self.move_aside(sha)?,
                Ok(OnDisk::Missing) | Err(_) => {}
            }
        }
        // `hashes` is sorted and each row's hash is its file's, so `index` is
        // sorted and unique.
        self.index = index;
        self.write_index()
    }

    fn record_path(&self, sha256: &[u8; 32]) -> PathBuf {
        self.root.join(FILES).join(format!("{}.json", hex(sha256)))
    }

    fn read_record(&self, sha256: &[u8; 32]) -> io::Result<OnDisk> {
        let bytes = match fs::read(self.record_path(sha256)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(OnDisk::Missing),
            Err(e) => return Err(e),
        };
        Ok(match serde_json::from_slice::<FileRecord>(&bytes) {
            Ok(record) if &record.meta.sha256 == sha256 => OnDisk::Record {
                record,
                len: bytes.len() as u64,
            },
            _ => OnDisk::Corrupt,
        })
    }

    /// The record, with a corrupt one moved aside and treated as absent, so
    /// the next write for that hash starts it afresh.
    fn load_record(&mut self, sha256: &[u8; 32]) -> io::Result<Option<FileRecord>> {
        match self.read_record(sha256)? {
            OnDisk::Record { record, .. } => Ok(Some(record)),
            OnDisk::Missing => Ok(None),
            OnDisk::Corrupt => {
                self.move_aside(sha256)?;
                if let Ok(i) = self.index.binary_search_by_key(sha256, |f| f.sha256) {
                    self.index.remove(i);
                    self.write_index()?;
                }
                Ok(None)
            }
        }
    }

    /// Renames `<hex>.json` to `<hex>.json.corrupt`, outside the record name
    /// pattern, replacing an earlier one for the same hash.
    fn move_aside(&self, sha256: &[u8; 32]) -> io::Result<()> {
        let from = self.record_path(sha256);
        let mut to = from.as_os_str().to_owned();
        to.push(CORRUPT_SUFFIX);
        fs::rename(&from, PathBuf::from(to))?;
        sync_dir(&self.root.join(FILES));
        Ok(())
    }

    /// Removes `index.json`, then writes the record, then the index with the
    /// record's new row.
    fn write_record(&mut self, record: &FileRecord) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(record)?;
        self.drop_index_file()?;
        write_atomic(&self.record_path(&record.meta.sha256), &bytes)?;
        let row = record.summary(bytes.len() as u64);
        match self.index.binary_search_by_key(&row.sha256, |f| f.sha256) {
            Ok(i) => self.index[i] = row,
            Err(i) => self.index.insert(i, row),
        }
        self.write_index()
    }

    fn write_index(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.index)?;
        write_atomic(&self.root.join(INDEX), &bytes)
    }

    /// With no index on disk, a crash before the next [`Self::write_index`]
    /// makes the next open rebuild rather than trust a stale row.
    fn drop_index_file(&self) -> io::Result<()> {
        match fs::remove_file(self.root.join(INDEX)) {
            Ok(()) => {
                sync_dir(&self.root);
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Whether a cached index can stand for the records: one row per record file,
/// strictly increasing by hash (so `binary_search_by_key` holds), and each
/// row's `record_len` equal to its file's length now.
fn index_matches(index: &[FileSummary], files: &Path, on_disk: &BTreeSet<[u8; 32]>) -> bool {
    index.len() == on_disk.len()
        && index.iter().zip(on_disk).all(|(row, sha)| {
            row.sha256 == *sha
                && fs::metadata(files.join(format!("{}.json", hex(sha))))
                    .is_ok_and(|m| m.len() == row.record_len)
        })
}

impl HistoryStore for JsonStore {
    fn upsert_file(&mut self, file: FileEntry) -> io::Result<()> {
        let runs = self
            .load_record(&file.sha256)?
            .map(|r| r.runs)
            .unwrap_or_default();
        self.write_record(&FileRecord { meta: file, runs })
    }

    fn record_run(&mut self, run: RunRecord, status: RecentStatus) -> io::Result<()> {
        let Some(mut record) = self.load_record(&run.input_sha256)? else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no such file in the history",
            ));
        };
        record.meta.status = status;
        record.runs.push(run);
        self.write_record(&record)
    }

    fn list_files(&self, filter: &str) -> Vec<FileSummary> {
        let needle = filter.to_lowercase();
        let matches = |f: &FileSummary| {
            needle.is_empty()
                || f.name.to_lowercase().contains(&needle)
                || f.path
                    .as_ref()
                    .is_some_and(|p| p.to_string_lossy().to_lowercase().contains(&needle))
        };
        let mut rows: Vec<FileSummary> =
            self.index.iter().filter(|f| matches(f)).cloned().collect();
        rows.sort_by(|a, b| {
            b.last_at
                .cmp(&a.last_at)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.sha256.cmp(&b.sha256))
        });
        rows
    }

    fn runs_for(&self, sha256: &[u8; 32]) -> io::Result<Vec<RunRecord>> {
        match self.read_record(sha256)? {
            OnDisk::Record { record, .. } => Ok(record.runs),
            OnDisk::Missing => Ok(Vec::new()),
            OnDisk::Corrupt => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the history record for this file is damaged",
            )),
        }
    }

    fn delete_file(&mut self, sha256: &[u8; 32]) -> io::Result<bool> {
        let removed = match fs::remove_file(self.record_path(sha256)) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };
        let before = self.index.len();
        self.index.retain(|f| &f.sha256 != sha256);
        if removed || self.index.len() != before {
            self.write_index()?;
        }
        Ok(removed)
    }

    fn summary(&self) -> HistorySummary {
        HistorySummary {
            files: self.index.len() as u64,
            runs: self.index.iter().map(|f| f.runs).sum(),
            recent: self
                .list_files("")
                .into_iter()
                .take(5)
                .map(|f| RecentRow {
                    name: f.name,
                    status: f.status,
                })
                .collect(),
        }
    }
}

/// The hashes named by `files/<64 hex>.json`; other names are ignored.
fn record_hashes(files: &Path) -> io::Result<BTreeSet<[u8; 32]>> {
    let mut out = BTreeSet::new();
    for entry in fs::read_dir(files)? {
        let name = entry?.file_name();
        if let Some(sha) = name
            .to_str()
            .and_then(|n| n.strip_suffix(".json"))
            .and_then(unhex)
        {
            out.insert(sha);
        }
    }
    Ok(out)
}

fn remove_temp_files(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let ours = entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.ends_with(".json.tmp"));
        if ours && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    let digits = s.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; 32];
    for (i, &[hi, lo]) in digits.as_chunks::<2>().0.iter().enumerate() {
        out[i] = (nibble(hi)? << 4) | nibble(lo)?;
    }
    Some(out)
}

/// `<path>.tmp` → write → fsync → rename over `path` → fsync the directory.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(path, bytes, |file, bytes| file.write_all(bytes))
}

/// [`write_atomic`] with the write step supplied, so a test can fail it
/// part-way. On any error the temp file is removed and `path` is untouched.
fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(TMP_SUFFIX);
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut file = File::create(&tmp)?;
        write(&mut file, bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }
    sync_dir(path.parent().unwrap_or(Path::new(".")));
    Ok(())
}

/// Makes the rename durable. Best effort: some filesystems refuse to fsync a
/// directory, and the rename has already happened.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Paths in the store's JSON, kept losslessly. serde's own `Path` impl
/// refuses a path that is not valid UTF-8, which a file name in a legacy
/// encoding is on Unix; this one writes such a path in a raw form.
///
/// ```text
/// "/cases/a.pdf"                          a path that is valid Unicode
/// {"lossy": "/cases/Pap?iers.pdf",        any other: a display form (U+FFFD
///  "unix_bytes": [47, 99, ...]}           for each bad sequence) and the
///                                         bytes on Unix, or the UTF-16 units
///                                         as "windows_units" on Windows
/// ```
///
/// A raw form read on the other OS family falls back to `lossy`.
mod path_json {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Text(String),
        Raw {
            lossy: String,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            unix_bytes: Option<Vec<u8>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            windows_units: Option<Vec<u16>>,
        },
    }

    fn to_repr(path: &Path) -> Repr {
        if let Some(text) = path.to_str() {
            return Repr::Text(text.to_owned());
        }
        let lossy = path.to_string_lossy().into_owned();
        #[cfg(unix)]
        let (unix_bytes, windows_units) = {
            use std::os::unix::ffi::OsStrExt;
            (Some(path.as_os_str().as_bytes().to_vec()), None)
        };
        #[cfg(windows)]
        let (unix_bytes, windows_units) = {
            use std::os::windows::ffi::OsStrExt;
            (None, Some(path.as_os_str().encode_wide().collect()))
        };
        #[cfg(not(any(unix, windows)))]
        let (unix_bytes, windows_units) = (None, None);
        Repr::Raw {
            lossy,
            unix_bytes,
            windows_units,
        }
    }

    fn from_repr(repr: Repr) -> PathBuf {
        let (lossy, unix_bytes, windows_units) = match repr {
            Repr::Text(text) => return PathBuf::from(text),
            Repr::Raw {
                lossy,
                unix_bytes,
                windows_units,
            } => (lossy, unix_bytes, windows_units),
        };
        #[cfg(unix)]
        if let Some(bytes) = unix_bytes {
            use std::os::unix::ffi::OsStringExt;
            return PathBuf::from(OsString::from_vec(bytes));
        }
        #[cfg(windows)]
        if let Some(units) = windows_units {
            use std::os::windows::ffi::OsStringExt;
            return PathBuf::from(OsString::from_wide(&units));
        }
        // The other family's raw form.
        drop((unix_bytes, windows_units));
        PathBuf::from(OsString::from(lossy))
    }

    pub fn serialize<S: Serializer>(path: &Path, s: S) -> Result<S::Ok, S::Error> {
        to_repr(path).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PathBuf, D::Error> {
        Repr::deserialize(d).map(from_repr)
    }

    pub mod option {
        use std::path::PathBuf;

        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        use super::{Repr, from_repr, to_repr};

        pub fn serialize<S: Serializer>(path: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
            path.as_deref().map(to_repr).serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PathBuf>, D::Error> {
            Ok(Option::<Repr>::deserialize(d)?.map(from_repr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AnalyzeOptions, FontDb, NullProgress, RepairOptions, analyze};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir()
                .join(format!("pdfpundit-library-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sha(n: u8) -> [u8; 32] {
        [n; 32]
    }

    fn entry(n: u8, name: &str, added_at: u64) -> FileEntry {
        FileEntry {
            sha256: sha(n),
            name: name.into(),
            path: Some(PathBuf::from(format!("/cases/{name}"))),
            size: 1000 + u64::from(n),
            added_at,
            status: RecentStatus::Pending,
        }
    }

    fn default_report() -> RepairReport {
        let analysis =
            analyze(b"%PDF-1.7\n", &AnalyzeOptions::default(), &mut NullProgress).unwrap();
        RepairReport::default_for(&analysis, &RepairOptions::default(), &FontDb::empty())
    }

    /// T-02b's fully populated report (eng-r3-q1), as committed.
    fn populated_report() -> RepairReport {
        serde_json::from_str(include_str!(
            "../tests/data/contract/repair_report_populated.json"
        ))
        .expect("the T-02b fixture parses")
    }

    fn run(n: u8, started_at: u64, report: RepairReport) -> RunRecord {
        RunRecord {
            started_at,
            input_sha256: sha(n),
            input_name: format!("file{n}.pdf"),
            output_path: Some(PathBuf::from(format!("/cases/file{n}.repaired.pdf"))),
            placed: Some(Placed::ClaimedThenRenamed),
            analysis_state: AnalysisStateUse::Rebuilt {
                reason: "evicted while parked".into(),
            },
            config_path: PathBuf::from("/cfg/config.toml"),
            report,
        }
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn run_records_round_trip_with_the_default_and_the_populated_report() {
        let dir = TempDir::new("round-trip");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let mut sparse = run(1, 20, default_report());
        sparse.output_path = None;
        sparse.placed = None;
        sparse.analysis_state = AnalysisStateUse::Reused;
        let full = run(1, 30, populated_report());
        assert!(
            !full.report.c9_survivors.is_empty(),
            "the populated fixture"
        );
        store
            .record_run(sparse.clone(), RecentStatus::Failed)
            .unwrap();
        store
            .record_run(full.clone(), RecentStatus::Partial)
            .unwrap();
        assert_eq!(
            store.runs_for(&sha(1)).unwrap(),
            [sparse.clone(), full.clone()]
        );

        // And from disk, through a fresh store.
        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.runs_for(&sha(1)).unwrap(), [sparse, full]);
        let row = &store.list_files("")[0];
        assert_eq!(
            (row.runs, row.last_at, row.status),
            (2, 30, RecentStatus::Partial)
        );
    }

    #[test]
    fn summary_counts_files_and_runs_and_lists_the_five_newest() {
        let dir = TempDir::new("summary");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        let names = [
            "a.pdf", "b.pdf", "c.pdf", "d.pdf", "e.pdf", "f.pdf", "g.pdf",
        ];
        for (i, name) in names.iter().enumerate() {
            store
                .upsert_file(entry(i as u8, name, 100 + i as u64))
                .unwrap();
        }
        // Nine runs. "a.pdf", the oldest file, gets the newest run.
        let runs = [
            (1, 200),
            (1, 201),
            (2, 202),
            (3, 203),
            (3, 204),
            (4, 205),
            (5, 206),
            (6, 207),
        ];
        for (n, t) in runs {
            store
                .record_run(run(n, t, default_report()), RecentStatus::Repaired)
                .unwrap();
        }
        store
            .record_run(run(0, 300, default_report()), RecentStatus::Partial)
            .unwrap();

        let summary = store.summary();
        assert_eq!((summary.files, summary.runs), (7, 9));
        let recent: Vec<(&str, RecentStatus)> = summary
            .recent
            .iter()
            .map(|r| (r.name.as_str(), r.status))
            .collect();
        assert_eq!(
            recent,
            [
                ("a.pdf", RecentStatus::Partial),
                ("g.pdf", RecentStatus::Repaired),
                ("f.pdf", RecentStatus::Repaired),
                ("e.pdf", RecentStatus::Repaired),
                ("d.pdf", RecentStatus::Repaired),
            ]
        );
        assert_eq!(JsonStore::open_at(&dir.0).unwrap().summary(), summary);
    }

    #[test]
    fn upsert_keeps_runs_and_delete_removes_the_file() {
        let dir = TempDir::new("upsert");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store
            .record_run(run(1, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        let mut renamed = entry(1, "renamed.pdf", 10);
        renamed.status = RecentStatus::Repaired;
        store.upsert_file(renamed).unwrap();
        assert_eq!(store.runs_for(&sha(1)).unwrap().len(), 1);
        assert_eq!(store.list_files("RENAMED")[0].name, "renamed.pdf");
        assert_eq!(store.list_files("cases/renamed").len(), 1);
        assert!(store.list_files("nothing").is_empty());

        let err = store.record_run(run(9, 1, default_report()), RecentStatus::Failed);
        assert_eq!(err.unwrap_err().kind(), io::ErrorKind::NotFound);

        assert!(store.delete_file(&sha(1)).unwrap());
        assert!(!store.delete_file(&sha(1)).unwrap());
        assert_eq!(store.summary(), HistorySummary::default());
        assert_eq!(store.runs_for(&sha(1)).unwrap(), []);
        assert_eq!(
            JsonStore::open_at(&dir.0).unwrap().summary(),
            HistorySummary::default()
        );
    }

    #[test]
    fn a_corrupt_index_is_rebuilt_from_the_records() {
        let dir = TempDir::new("corrupt");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store.upsert_file(entry(2, "b.pdf", 11)).unwrap();
        store
            .record_run(run(2, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        let expected = store.summary();
        drop(store);

        let index = dir.0.join(INDEX);
        fs::write(&index, b"[{\"sha256\": tru").unwrap();
        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary(), expected);
        let rewritten: Vec<FileSummary> =
            serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
        assert_eq!(rewritten.len(), 2);

        // An index that parses but misses a record is rebuilt too.
        fs::write(&index, b"[]").unwrap();
        assert_eq!(JsonStore::open_at(&dir.0).unwrap().summary(), expected);
        fs::remove_file(&index).unwrap();
        assert_eq!(JsonStore::open_at(&dir.0).unwrap().summary(), expected);
    }

    #[test]
    fn atomic_writes_leave_no_temp_file_on_success() {
        let dir = TempDir::new("no-tmp");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store
            .record_run(run(1, 20, populated_report()), RecentStatus::Partial)
            .unwrap();
        assert_eq!(names_in(&dir.0), ["files", "index.json"]);
        assert_eq!(
            names_in(&dir.0.join(FILES)),
            [format!("{}.json", hex(&sha(1)))]
        );
    }

    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        let dir = TempDir::new("fail");
        let path = dir.0.join("record.json");
        let half_then_fail = |file: &mut File, bytes: &[u8]| {
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(io::Error::other("disk full"))
        };
        // No file before: none after.
        assert!(write_atomic_with(&path, b"{\"new\": true}", half_then_fail).is_err());
        assert_eq!(names_in(&dir.0), Vec::<String>::new());

        // An existing file keeps its bytes.
        write_atomic(&path, b"{\"old\": true}").unwrap();
        assert!(write_atomic_with(&path, b"{\"new\": true}", half_then_fail).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{\"old\": true}");
        assert_eq!(names_in(&dir.0), ["record.json"]);
    }

    #[test]
    fn open_removes_temp_files_left_by_an_interrupted_write() {
        let dir = TempDir::new("leftover");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        drop(store);
        fs::write(dir.0.join("index.json.tmp"), b"[").unwrap();
        let leftover = dir.0.join(FILES).join(format!("{}.json.tmp", hex(&sha(1))));
        fs::write(&leftover, b"{").unwrap();
        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary().files, 1);
        assert_eq!(names_in(&dir.0), ["files", "index.json"]);
        assert!(!leftover.exists());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_round_trip() {
        use std::ffi::OsString;
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let raw = |b: &[u8]| PathBuf::from(OsString::from_vec(b.to_vec()));
        let dir = TempDir::new("non-utf8");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        let mut file = entry(1, "Pap\u{FFFD}iers.pdf", 10);
        file.path = Some(raw(b"/cases/Pap\xe9iers.pdf"));
        store.upsert_file(file.clone()).unwrap();
        let mut r = run(1, 20, default_report());
        r.output_path = Some(raw(b"/cases/Pap\xe9iers.repaired.pdf"));
        r.config_path = raw(b"/cfg/\xe9/config.toml");
        store.record_run(r.clone(), RecentStatus::Repaired).unwrap();

        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.runs_for(&sha(1)).unwrap(), [r]);
        let row = &store.list_files("")[0];
        assert_eq!(
            row.path.as_ref().unwrap().as_os_str().as_bytes(),
            b"/cases/Pap\xe9iers.pdf"
        );
        // A UTF-8 path is still written as a plain string.
        let index = fs::read_to_string(dir.0.join(INDEX)).unwrap();
        assert!(index.contains("\"unix_bytes\""));
        file.path = Some(PathBuf::from("/cases/plain.pdf"));
        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["path"], "/cases/plain.pdf");
        assert_eq!(
            serde_json::to_value(entry(2, "x", 0)).unwrap()["path"],
            "/cases/x"
        );
    }

    #[test]
    fn a_stale_index_row_is_caught_by_the_record_length() {
        let dir = TempDir::new("stale");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let index_before = fs::read(dir.0.join(INDEX)).unwrap();
        store
            .record_run(run(1, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        drop(store);
        // A crash after the record write, with the old index still on disk.
        fs::write(dir.0.join(INDEX), &index_before).unwrap();
        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary().runs, 1);
        assert_eq!(store.list_files("")[0].status, RecentStatus::Repaired);
    }

    #[test]
    fn a_record_write_removes_the_index_first() {
        let dir = TempDir::new("drop-index");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let expected = store.summary();
        // A directory where the temp file goes makes the record write fail
        // after the index is gone, as a crash there would.
        let blocker = dir.0.join(FILES).join(format!("{}.json.tmp", hex(&sha(1))));
        fs::create_dir(&blocker).unwrap();
        assert!(store.upsert_file(entry(1, "renamed.pdf", 10)).is_err());
        assert!(!dir.0.join(INDEX).exists());
        drop(store);
        fs::remove_dir(&blocker).unwrap();
        // The next open rebuilds from the records.
        assert_eq!(JsonStore::open_at(&dir.0).unwrap().summary(), expected);
        assert!(dir.0.join(INDEX).exists());
    }

    #[test]
    fn an_index_with_a_duplicate_row_is_rebuilt() {
        let dir = TempDir::new("dup");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store.upsert_file(entry(2, "b.pdf", 11)).unwrap();
        let expected = store.summary();
        drop(store);
        let index = dir.0.join(INDEX);
        let rows: Vec<FileSummary> = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
        let dup = [rows[0].clone(), rows[0].clone()];
        fs::write(&index, serde_json::to_vec(&dup).unwrap()).unwrap();
        assert_eq!(JsonStore::open_at(&dir.0).unwrap().summary(), expected);
        // Out of order is rebuilt too.
        let swapped = [rows[1].clone(), rows[0].clone()];
        fs::write(&index, serde_json::to_vec(&swapped).unwrap()).unwrap();
        let store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary(), expected);
        assert_eq!(store.runs_for(&sha(2)).unwrap(), []);
    }

    #[test]
    fn a_truncated_record_is_moved_aside_and_replaced() {
        let dir = TempDir::new("truncated");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store.upsert_file(entry(2, "b.pdf", 11)).unwrap();
        store
            .record_run(run(1, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        let record = store.record_path(&sha(1));
        let bytes = fs::read(&record).unwrap();
        fs::write(&record, &bytes[..bytes.len() / 2]).unwrap();
        let corrupt = format!("{}.json.corrupt", hex(&sha(1)));

        // In the open store: the next write for that hash starts afresh.
        assert_eq!(
            store.runs_for(&sha(1)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        store.upsert_file(entry(1, "a.pdf", 30)).unwrap();
        assert_eq!(store.runs_for(&sha(1)).unwrap(), []);
        assert!(dir.0.join(FILES).join(&corrupt).exists());
        store
            .record_run(run(1, 40, default_report()), RecentStatus::Partial)
            .unwrap();
        assert_eq!(store.summary().runs, 1);

        // On open: the record is left out of the index and moved aside, so
        // later opens trust the index again.
        fs::write(&record, &bytes[..10]).unwrap();
        drop(store);
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary().files, 1);
        assert!(!record.exists());
        assert!(dir.0.join(FILES).join(&corrupt).exists());
        let index = fs::read(dir.0.join(INDEX)).unwrap();
        let rows: Vec<FileSummary> = serde_json::from_slice(&index).unwrap();
        assert!(index_matches(
            &rows,
            &dir.0.join(FILES),
            &record_hashes(&dir.0.join(FILES)).unwrap()
        ));
        store.upsert_file(entry(1, "a.pdf", 50)).unwrap();
        assert_eq!(store.summary().files, 2);
    }

    #[test]
    fn a_record_under_another_hash_is_moved_aside() {
        let dir = TempDir::new("misnamed");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let bytes = fs::read(store.record_path(&sha(1))).unwrap();
        fs::write(store.record_path(&sha(3)), &bytes).unwrap();
        drop(store);
        let store = JsonStore::open_at(&dir.0).unwrap();
        let rows = store.list_files("");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sha256, sha(1));
        assert!(!store.record_path(&sha(3)).exists());
    }

    /// D-076: `started_at` arrives as a number, so nothing here needs a
    /// clippy allow, and this file carries none.
    #[test]
    fn this_file_carries_no_lint_allow() {
        let source = include_str!("library.rs");
        for attr in ["#![", "#["] {
            for lint in ["allow", "expect"] {
                let needle = format!("{attr}{lint}(");
                assert!(!source.contains(&needle), "found {needle}");
            }
        }
    }

    #[test]
    fn hex_names_round_trip() {
        let mut sha = [0u8; 32];
        sha[0] = 0xab;
        sha[31] = 0x09;
        assert_eq!(unhex(&hex(&sha)), Some(sha));
        assert_eq!(unhex("AB"), None);
        assert_eq!(unhex(&"G".repeat(64)), None);
    }
}
