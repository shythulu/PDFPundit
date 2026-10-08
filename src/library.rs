//! The history store (TD §6): [`HistoryStore`] and its JSON backend.
//!
//! ```text
//! <data_dir>/history/
//!   index.json            Vec<FileSummary>: a cache, rebuilt from files/ when
//!                         it is unreadable or names a different set of files
//!   files/<sha256>.json   FileRecord { meta: FileEntry, runs: Vec<RunRecord> }
//! ```
//!
//! Every write is temp file → fsync → rename (→ fsync of the directory on
//! Unix). The store replaces only its own files; D-044's no-clobber rule is
//! for outputs beside user files. Times are unix seconds handed in by the
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
    pub output_path: Option<PathBuf>,
    /// Which no-replace guarantee held (D-044, D-073).
    pub placed: Option<Placed>,
    /// Reused or rebuilt, and why (D-073).
    pub analysis_state: AnalysisStateUse,
    /// The config file the run loaded (eng-r2-q10).
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
    pub path: Option<PathBuf>,
    pub size: u64,
    pub added_at: u64,
    /// The later of `added_at` and the newest run's `started_at`.
    pub last_at: u64,
    pub status: RecentStatus,
    pub runs: u64,
}

/// What `files/<sha256>.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub meta: FileEntry,
    pub runs: Vec<RunRecord>,
}

impl FileRecord {
    fn summary(&self) -> FileSummary {
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

impl JsonStore {
    /// The store in `<data_dir>/history`.
    pub fn open(dirs: &AppDirs) -> io::Result<JsonStore> {
        JsonStore::open_at(dirs.data_dir.join("history"))
    }

    /// The store in `root`, created if missing. Leftover temp files from an
    /// interrupted write are removed; the index is rebuilt from `files/` when
    /// it is unreadable or names a different set of files.
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
            .filter(|index| index.iter().map(|f| f.sha256).collect::<BTreeSet<_>>() == on_disk);
        let mut store = JsonStore {
            root,
            index: Vec::new(),
        };
        match cached {
            Some(mut index) => {
                index.sort_by_key(|f| f.sha256);
                store.index = index;
            }
            None => store.rebuild_index(&on_disk)?,
        }
        Ok(store)
    }

    /// Re-reads every record in `files/` and rewrites `index.json`. A record
    /// that does not parse is left in place and kept out of the index.
    fn rebuild_index(&mut self, hashes: &BTreeSet<[u8; 32]>) -> io::Result<()> {
        self.index = hashes
            .iter()
            .filter_map(|sha| self.read_record(sha).ok().flatten())
            .map(|record| record.summary())
            .collect();
        self.write_index()
    }

    fn record_path(&self, sha256: &[u8; 32]) -> PathBuf {
        self.root.join(FILES).join(format!("{}.json", hex(sha256)))
    }

    fn read_record(&self, sha256: &[u8; 32]) -> io::Result<Option<FileRecord>> {
        match fs::read(self.record_path(sha256)) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Writes the record, then the index row for it.
    fn write_record(&mut self, record: &FileRecord) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(record)?;
        write_atomic(&self.record_path(&record.meta.sha256), &bytes)?;
        let row = record.summary();
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
}

impl HistoryStore for JsonStore {
    fn upsert_file(&mut self, file: FileEntry) -> io::Result<()> {
        let runs = self
            .read_record(&file.sha256)?
            .map(|r| r.runs)
            .unwrap_or_default();
        self.write_record(&FileRecord { meta: file, runs })
    }

    fn record_run(&mut self, run: RunRecord, status: RecentStatus) -> io::Result<()> {
        let Some(mut record) = self.read_record(&run.input_sha256)? else {
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
        Ok(self
            .read_record(sha256)?
            .map(|r| r.runs)
            .unwrap_or_default())
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
