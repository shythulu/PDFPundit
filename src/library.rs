//! The history store (TD §6): [`HistoryStore`] and its JSON backend.
//!
//! ```text
//! <data_dir>/history/
//!   index.json            { version: 1, files: Vec<FileSummary> }: a cache,
//!                         rebuilt from files/ when it is unreadable, newer
//!                         or disagrees with the records
//!   files/<sha256>.json   { version: 1, meta: FileEntry,
//!                           runs: [{ version: 1, ..RunRecord }] }
//!   files/<sha256>.json.corrupt, then .corrupt (2), .corrupt (3), ...
//!                         a record that did not parse, or named another hash,
//!                         moved aside so the store can go on without it; an
//!                         earlier one is never overwritten
//! ```
//!
//! Format versions (F-05): `index.json`, each record and each run carry
//! `"version": 1`, and one with no version (as T-16 first wrote them, the
//! index a bare array) reads as version 1. A record or run with a higher
//! version was written by a newer build: it is left exactly as written and
//! skipped with a warning ([`HistoryStore::take_warnings`]), never moved
//! aside, and a save of its file keeps it in place. A newer index is only a
//! cache, so it is rebuilt.
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

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::appdirs::AppDirs;
use crate::engine::{AnalysisStateUse, RepairReport};
use crate::jobs::Placed;
use crate::place::place_no_replace;

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

/// The version of `index.json`, of every record and of every run this build
/// writes (F-05).
const FORMAT_VERSION: u64 = 1;

/// What `files/<sha256>.json` holds, less its `version`.
#[derive(Debug, Serialize)]
struct FileRecord {
    meta: FileEntry,
    runs: Vec<StoredRun>,
}

/// One entry of a record's `runs`, in the order recorded.
#[derive(Debug)]
enum StoredRun {
    Known(Box<RunRecord>),
    /// A run with a higher version: kept as written, never read.
    Newer(Box<RawValue>),
}

impl Serialize for StoredRun {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            StoredRun::Known(run) => Versioned::new(run).serialize(s),
            StoredRun::Newer(raw) => raw.serialize(s),
        }
    }
}

/// `inner`'s fields with `"version"` written first.
#[derive(Serialize)]
struct Versioned<'a, T> {
    version: u64,
    #[serde(flatten)]
    inner: &'a T,
}

impl<'a, T> Versioned<'a, T> {
    fn new(inner: &'a T) -> Versioned<'a, T> {
        Versioned {
            version: FORMAT_VERSION,
            inner,
        }
    }
}

/// `index.json` as this build writes it.
#[derive(Serialize)]
struct IndexOut<'a> {
    version: u64,
    files: &'a [FileSummary],
}

/// `index.json` from a version-1 object.
#[derive(Deserialize)]
struct IndexIn {
    files: Vec<FileSummary>,
}

/// A record from a version-1 object, its runs not yet read.
#[derive(Deserialize)]
struct RecordIn<'a> {
    meta: FileEntry,
    #[serde(borrow)]
    runs: Vec<&'a RawValue>,
}

/// Only the `version` of a JSON object.
#[derive(Deserialize)]
struct VersionOnly {
    version: Option<u64>,
}

/// The format version `json` says it has (none is 1); `None` if it is not
/// JSON or its `version` is not a whole number.
fn version_of(json: &[u8]) -> Option<u64> {
    serde_json::from_slice::<VersionOnly>(json)
        .ok()
        .map(|v| v.version.unwrap_or(FORMAT_VERSION))
}

impl FileRecord {
    fn known_runs(&self) -> impl Iterator<Item = &RunRecord> {
        self.runs.iter().filter_map(|r| match r {
            StoredRun::Known(run) => Some(&**run),
            StoredRun::Newer(_) => None,
        })
    }

    /// The row for `index.json`. Runs with a higher version are not counted:
    /// this build cannot show them.
    fn summary(&self, record_len: u64) -> FileSummary {
        let m = &self.meta;
        let newest_run = self.known_runs().map(|r| r.started_at).max();
        FileSummary {
            sha256: m.sha256,
            name: m.name.clone(),
            path: m.path.clone(),
            size: m.size,
            added_at: m.added_at,
            last_at: newest_run.map_or(m.added_at, |t| t.max(m.added_at)),
            status: m.status,
            runs: self.known_runs().count() as u64,
            record_len,
        }
    }
}

/// The history behind a minimal interface (TD §6).
pub trait HistoryStore {
    /// Adds the file, or replaces its entry and keeps its runs.
    fn upsert_file(&mut self, file: FileEntry) -> io::Result<()>;
    /// Appends a run to the file `run.input_sha256` names and sets that file's
    /// status. `NotFound` if the file was never upserted (or was deleted);
    /// `InvalidData` if its record was damaged, in which case the record has
    /// been set aside and nothing was written: upsert the file again and
    /// retry to keep the run. Here and in `upsert_file`, `runs_for` and
    /// `delete_file`, a record a newer build wrote is left as it is and the
    /// call fails `InvalidData`.
    fn record_run(&mut self, run: RunRecord, status: RecentStatus) -> io::Result<()>;
    /// Files whose name or path contains `filter` (case-insensitive; `""`
    /// matches all), newest first.
    fn list_files(&self, filter: &str) -> Vec<FileSummary>;
    /// The file's runs in the order they were recorded; empty if unknown.
    fn runs_for(&self, sha256: &[u8; 32]) -> io::Result<Vec<RunRecord>>;
    /// Removes the file and its runs, including every damaged copy set
    /// aside; `false` if there was nothing to remove.
    fn delete_file(&mut self, sha256: &[u8; 32]) -> io::Result<bool>;
    /// Totals and the five newest files, for the idle screen.
    fn summary(&self) -> HistorySummary;
    /// What the store has skipped since the last call, for the debug log:
    /// records and runs a newer build wrote, and a newer index.
    fn take_warnings(&mut self) -> Vec<String>;
}

/// The JSON store. It owns its directory; one process uses it at a time.
#[derive(Debug)]
pub struct JsonStore {
    root: PathBuf,
    /// Sorted by `sha256`.
    index: Vec<FileSummary>,
    /// Pending for [`HistoryStore::take_warnings`]; a cell because reads
    /// through `&self` can skip a newer run.
    warnings: RefCell<Vec<String>>,
}

const INDEX: &str = "index.json";
const FILES: &str = "files";
const TMP_SUFFIX: &str = ".tmp";
const CORRUPT_SUFFIX: &str = ".corrupt";
/// How many damaged copies of one record are kept before moving another
/// aside fails.
const MAX_CORRUPT_COPIES: u32 = 10_000;

/// What [`JsonStore::load_record`] found.
enum Loaded {
    Record(FileRecord),
    Missing,
    /// The record was damaged and has just been moved aside.
    SetAside,
    /// A newer build wrote it, at this version; it is left as it is.
    Newer(u64),
}

/// What reading `files/<sha256>.json` found.
enum OnDisk {
    Missing,
    /// It does not parse, or its `meta.sha256` is not the hash in its name.
    Corrupt,
    /// Its version is this one, higher than [`FORMAT_VERSION`].
    Newer(u64),
    Record {
        record: FileRecord,
        len: u64,
    },
}

/// What reading `index.json` found.
enum CachedIndex {
    Rows(Vec<FileSummary>),
    /// Its version is this one, higher than [`FORMAT_VERSION`].
    Newer(u64),
    Unusable,
}

fn parse_index(bytes: &[u8]) -> CachedIndex {
    // Version 1 as T-16 first wrote it: a bare array.
    if let Ok(rows) = serde_json::from_slice::<Vec<FileSummary>>(bytes) {
        return CachedIndex::Rows(rows);
    }
    match version_of(bytes) {
        Some(v) if v > FORMAT_VERSION => CachedIndex::Newer(v),
        Some(_) => serde_json::from_slice::<IndexIn>(bytes)
            .map_or(CachedIndex::Unusable, |index| {
                CachedIndex::Rows(index.files)
            }),
        None => CachedIndex::Unusable,
    }
}

/// The error for a write or read of a record a newer build wrote.
fn newer_error(version: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "the history record for this file was written by a newer version of \
             PDFPundit (format version {version}); it is left as it is"
        ),
    )
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
        let cached =
            fs::read(root.join(INDEX)).map_or(CachedIndex::Unusable, |bytes| parse_index(&bytes));
        let mut store = JsonStore {
            root,
            index: Vec::new(),
            warnings: RefCell::default(),
        };
        match cached {
            CachedIndex::Rows(index) if index_matches(&index, &files, &on_disk) => {
                store.index = index;
            }
            CachedIndex::Newer(v) => {
                store.warn(format!(
                    "{INDEX} was written by a newer version (format version {v}); \
                     it is rebuilt from the records"
                ));
                store.rebuild_index(&on_disk)?;
            }
            CachedIndex::Rows(_) | CachedIndex::Unusable => store.rebuild_index(&on_disk)?,
        }
        Ok(store)
    }

    /// Queues `message` for [`HistoryStore::take_warnings`] unless it is
    /// already pending.
    fn warn(&self, message: String) {
        let mut pending = self.warnings.borrow_mut();
        if !pending.contains(&message) {
            pending.push(message);
        }
    }

    /// Re-reads every record in `files/` and rewrites `index.json`. A corrupt
    /// record is moved aside; one that cannot be read, or a corrupt one that
    /// cannot be moved, is kept out of the index and tried again on the next
    /// open, so one bad file never makes the store unusable. A newer record
    /// is kept out of the index with a warning; since the index then has no
    /// row for it, every open rebuilds and warns again.
    fn rebuild_index(&mut self, hashes: &BTreeSet<[u8; 32]>) -> io::Result<()> {
        let mut index = Vec::with_capacity(hashes.len());
        for sha in hashes {
            match self.read_record(sha) {
                Ok(OnDisk::Record { record, len }) => index.push(record.summary(len)),
                Ok(OnDisk::Corrupt) => {
                    // Left in place it is retried on the next open.
                    let _ = self.move_aside(sha);
                }
                Ok(OnDisk::Newer(v)) => self.warn(format!(
                    "{}.json was written by a newer version (format version {v}); \
                     it is left as it is and skipped",
                    hex(sha)
                )),
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

    /// Where [`Self::move_aside`] puts the `n`th damaged copy of a record:
    /// `<hex>.json.corrupt`, then `<hex>.json.corrupt (2)` and so on.
    fn corrupt_path(&self, sha256: &[u8; 32], n: u32) -> PathBuf {
        let mut path = self.record_path(sha256).into_os_string();
        path.push(CORRUPT_SUFFIX);
        if n > 1 {
            path.push(format!(" ({n})"));
        }
        PathBuf::from(path)
    }

    fn read_record(&self, sha256: &[u8; 32]) -> io::Result<OnDisk> {
        let bytes = match fs::read(self.record_path(sha256)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(OnDisk::Missing),
            Err(e) => return Err(e),
        };
        match version_of(&bytes) {
            None => return Ok(OnDisk::Corrupt),
            Some(v) if v > FORMAT_VERSION => return Ok(OnDisk::Newer(v)),
            Some(_) => {}
        }
        let body = match serde_json::from_slice::<RecordIn>(&bytes) {
            Ok(body) if &body.meta.sha256 == sha256 => body,
            _ => return Ok(OnDisk::Corrupt),
        };
        let mut runs = Vec::with_capacity(body.runs.len());
        for raw in body.runs {
            let run = match version_of(raw.get().as_bytes()) {
                None => return Ok(OnDisk::Corrupt),
                Some(v) if v > FORMAT_VERSION => {
                    self.warn(format!(
                        "{}.json holds a run written by a newer version (format \
                         version {v}); the run is kept as it is and skipped",
                        hex(sha256)
                    ));
                    StoredRun::Newer(raw.to_owned())
                }
                Some(_) => match serde_json::from_str::<RunRecord>(raw.get()) {
                    Ok(run) => StoredRun::Known(Box::new(run)),
                    Err(_) => return Ok(OnDisk::Corrupt),
                },
            };
            runs.push(run);
        }
        Ok(OnDisk::Record {
            record: FileRecord {
                meta: body.meta,
                runs,
            },
            len: bytes.len() as u64,
        })
    }

    /// The record, with a corrupt one moved aside and dropped from the index,
    /// so the next upsert for that hash starts it afresh.
    fn load_record(&mut self, sha256: &[u8; 32]) -> io::Result<Loaded> {
        match self.read_record(sha256)? {
            OnDisk::Record { record, .. } => Ok(Loaded::Record(record)),
            OnDisk::Missing => Ok(Loaded::Missing),
            OnDisk::Newer(v) => Ok(Loaded::Newer(v)),
            OnDisk::Corrupt => {
                self.move_aside(sha256)?;
                if let Ok(i) = self.index.binary_search_by_key(sha256, |f| f.sha256) {
                    self.index.remove(i);
                    self.write_index()?;
                }
                Ok(Loaded::SetAside)
            }
        }
    }

    /// Renames `<hex>.json` to the first free [`Self::corrupt_path`],
    /// outside the record name pattern. An earlier damaged copy is never
    /// replaced (the rename refuses an existing name).
    fn move_aside(&self, sha256: &[u8; 32]) -> io::Result<()> {
        let from = self.record_path(sha256);
        let mut n = 1;
        loop {
            match place_no_replace(&from, &self.corrupt_path(sha256, n)) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < MAX_CORRUPT_COPIES => {
                    n += 1;
                }
                Err(e) => return Err(e),
            }
        }
        sync_dir(&self.root.join(FILES));
        Ok(())
    }

    /// Every damaged copy of the record for `sha256` that
    /// [`Self::move_aside`] made.
    fn corrupt_copies(&self, sha256: &[u8; 32]) -> io::Result<Vec<PathBuf>> {
        let prefix = format!("{}.json{CORRUPT_SUFFIX}", hex(sha256));
        let mut out = Vec::new();
        for entry in fs::read_dir(self.root.join(FILES))? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(&prefix)) else {
                continue;
            };
            let numbered = rest
                .strip_prefix(" (")
                .and_then(|r| r.strip_suffix(')'))
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()));
            if rest.is_empty() || numbered {
                out.push(entry.path());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Removes `index.json`, then writes the record, then the index with the
    /// record's new row.
    fn write_record(&mut self, record: &FileRecord) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&Versioned::new(record))?;
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
        let bytes = serde_json::to_vec_pretty(&IndexOut {
            version: FORMAT_VERSION,
            files: &self.index,
        })?;
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
        let runs = match self.load_record(&file.sha256)? {
            Loaded::Record(record) => record.runs,
            Loaded::Missing | Loaded::SetAside => Vec::new(),
            Loaded::Newer(v) => return Err(newer_error(v)),
        };
        self.write_record(&FileRecord { meta: file, runs })
    }

    fn record_run(&mut self, run: RunRecord, status: RecentStatus) -> io::Result<()> {
        let mut record = match self.load_record(&run.input_sha256)? {
            Loaded::Record(record) => record,
            Loaded::Missing => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no such file in the history",
                ));
            }
            Loaded::SetAside => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the history record for this file was damaged and has been set \
                     aside; add the file again to record this run",
                ));
            }
            Loaded::Newer(v) => return Err(newer_error(v)),
        };
        record.meta.status = status;
        record.runs.push(StoredRun::Known(Box::new(run)));
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
            OnDisk::Record { record, .. } => Ok(record.known_runs().cloned().collect()),
            OnDisk::Missing => Ok(Vec::new()),
            OnDisk::Corrupt => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the history record for this file is damaged",
            )),
            OnDisk::Newer(v) => Err(newer_error(v)),
        }
    }

    fn delete_file(&mut self, sha256: &[u8; 32]) -> io::Result<bool> {
        // A newer build's record is left as it is, as on every other path.
        if let OnDisk::Newer(v) = self.read_record(sha256)? {
            return Err(newer_error(v));
        }
        // The damaged copies hold the same paths and names, so they go too.
        let mut removed = false;
        let mut paths = self.corrupt_copies(sha256)?;
        paths.insert(0, self.record_path(sha256));
        for path in paths {
            match fs::remove_file(path) {
                Ok(()) => removed = true,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
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

    fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(self.warnings.get_mut())
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
        let rewritten = index_rows(&index);
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
        let rows = index_rows(&index);
        // Written back in the bare-array form, which reads as version 1.
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

        // A run for a damaged record says so rather than "not found", and
        // goes through once the file is upserted again.
        fs::write(&record, b"{").unwrap();
        let err = store
            .record_run(run(1, 45, default_report()), RecentStatus::Partial)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("set aside"), "{err}");
        assert_eq!(store.summary().files, 1);
        store.upsert_file(entry(1, "a.pdf", 30)).unwrap();
        store
            .record_run(run(1, 45, default_report()), RecentStatus::Partial)
            .unwrap();

        // On open: the record is left out of the index and moved aside, so
        // later opens trust the index again.
        fs::write(&record, &bytes[..10]).unwrap();
        drop(store);
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary().files, 1);
        assert!(!record.exists());
        assert!(dir.0.join(FILES).join(&corrupt).exists());
        let rows = index_rows(&dir.0.join(INDEX));
        assert!(index_matches(
            &rows,
            &dir.0.join(FILES),
            &record_hashes(&dir.0.join(FILES)).unwrap()
        ));
        store.upsert_file(entry(1, "a.pdf", 50)).unwrap();
        assert_eq!(store.summary().files, 2);

        // Deleting the file removes the damaged copy as well.
        assert!(store.delete_file(&sha(1)).unwrap());
        assert!(!record.exists());
        assert!(!dir.0.join(FILES).join(&corrupt).exists());
        assert_eq!(
            names_in(&dir.0.join(FILES)),
            [format!("{}.json", hex(&sha(2)))]
        );
        // A damaged copy alone still counts as something removed.
        fs::write(store.corrupt_path(&sha(1), 1), b"{").unwrap();
        assert!(store.delete_file(&sha(1)).unwrap());
        assert!(!store.delete_file(&sha(1)).unwrap());
        assert_eq!(
            names_in(&dir.0.join(FILES)),
            [format!("{}.json", hex(&sha(2)))]
        );
    }

    /// A corrupt record that cannot be moved aside is left out of the index
    /// rather than failing the open.
    #[cfg(unix)]
    #[test]
    fn a_corrupt_record_that_cannot_be_moved_does_not_stop_the_open() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("stuck");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store.upsert_file(entry(2, "b.pdf", 11)).unwrap();
        let record = store.record_path(&sha(1));
        drop(store);
        fs::write(&record, b"{").unwrap();
        let files = dir.0.join(FILES);
        fs::set_permissions(&files, fs::Permissions::from_mode(0o555)).unwrap();
        let opened = JsonStore::open_at(&dir.0);
        let moved = !record.exists();
        fs::set_permissions(&files, fs::Permissions::from_mode(0o755)).unwrap();
        let store = opened.unwrap();
        let rows = store.list_files("");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sha256, sha(2));
        // Unless the directory permissions did not bind (a root user), the
        // record is still in place and the next open tries again.
        if !moved {
            drop(store);
            let store = JsonStore::open_at(&dir.0).unwrap();
            assert_eq!(store.list_files("").len(), 1);
            assert!(!record.exists());
        }
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

    fn json_at(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    /// The rows of the `index.json` at `path`, as this build writes it.
    fn index_rows(path: &Path) -> Vec<FileSummary> {
        match parse_index(&fs::read(path).unwrap()) {
            CachedIndex::Rows(rows) => rows,
            CachedIndex::Newer(_) | CachedIndex::Unusable => panic!("unreadable index"),
        }
    }

    #[test]
    fn the_index_every_record_and_every_run_carry_version_1() {
        let dir = TempDir::new("version");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store
            .record_run(run(1, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        store
            .record_run(run(1, 30, populated_report()), RecentStatus::Partial)
            .unwrap();

        let index = json_at(&dir.0.join(INDEX));
        assert_eq!(index["version"], 1);
        assert_eq!(index["files"].as_array().unwrap().len(), 1);
        let record = json_at(&store.record_path(&sha(1)));
        assert_eq!(record["version"], 1);
        let runs = record["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|r| r["version"] == 1), "{runs:?}");

        // The versioned files read back to the same values, and a fresh open
        // trusts the index it wrote.
        let index_bytes = fs::read(dir.0.join(INDEX)).unwrap();
        let reopened = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(reopened.summary(), store.summary());
        assert_eq!(
            reopened.runs_for(&sha(1)).unwrap(),
            store.runs_for(&sha(1)).unwrap()
        );
        assert_eq!(fs::read(dir.0.join(INDEX)).unwrap(), index_bytes);
    }

    #[test]
    fn files_with_no_version_read_as_version_1() {
        let dir = TempDir::new("unversioned");
        let files = dir.0.join(FILES);
        fs::create_dir_all(&files).unwrap();
        let r = run(1, 20, populated_report());
        let legacy = serde_json::json!({ "meta": entry(1, "a.pdf", 10), "runs": [r] });
        let record_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(files.join(format!("{}.json", hex(&sha(1)))), &record_bytes).unwrap();
        let row = FileSummary {
            sha256: sha(1),
            name: "a.pdf".into(),
            path: Some(PathBuf::from("/cases/a.pdf")),
            size: 1001,
            added_at: 10,
            last_at: 20,
            status: RecentStatus::Pending,
            runs: 1,
            record_len: record_bytes.len() as u64,
        };
        let index_bytes = serde_json::to_vec_pretty(&[row]).unwrap();
        fs::write(dir.0.join(INDEX), &index_bytes).unwrap();

        let mut store = JsonStore::open_at(&dir.0).unwrap();
        // The bare-array index is trusted as it stands.
        assert_eq!(fs::read(dir.0.join(INDEX)).unwrap(), index_bytes);
        assert_eq!((store.summary().files, store.summary().runs), (1, 1));
        assert_eq!(store.runs_for(&sha(1)).unwrap(), std::slice::from_ref(&r));
        assert_eq!(store.take_warnings(), Vec::<String>::new());

        // The next write gives both files a version.
        store
            .record_run(run(1, 30, default_report()), RecentStatus::Repaired)
            .unwrap();
        assert_eq!(json_at(&dir.0.join(INDEX))["version"], 1);
        let record = json_at(&store.record_path(&sha(1)));
        assert_eq!(record["version"], 1);
        assert!(
            record["runs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["version"] == 1)
        );
        assert_eq!(store.runs_for(&sha(1)).unwrap()[0], r);
    }

    #[test]
    fn a_future_version_record_survives_a_load_and_a_save_untouched() {
        let dir = TempDir::new("future-record");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let future = store.record_path(&sha(2));
        let bytes: &[u8] = br#"{"version": 2, "meta": {"digest": "new shape"}, "runs": 7}"#;
        fs::write(&future, bytes).unwrap();
        drop(store);

        // Load: the record is skipped with a warning.
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        let rows = store.list_files("");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sha256, sha(1));
        let warnings = store.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(&hex(&sha(2))) && w.contains("version 2")),
            "{warnings:?}"
        );
        assert_eq!(store.take_warnings(), Vec::<String>::new(), "drained");

        // Save: writes to other files, and attempts on this one, leave it be.
        store
            .record_run(run(1, 20, default_report()), RecentStatus::Repaired)
            .unwrap();
        store.upsert_file(entry(3, "c.pdf", 12)).unwrap();
        let err = store.upsert_file(entry(2, "b.pdf", 11)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("newer"), "{err}");
        let err = store
            .record_run(run(2, 21, default_report()), RecentStatus::Repaired)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            store.runs_for(&sha(2)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(store.delete_file(&sha(2)).is_err());
        assert_eq!(store.summary().files, 2);
        assert_eq!(fs::read(&future).unwrap(), bytes);
        // Never moved aside.
        assert_eq!(
            names_in(&dir.0.join(FILES)),
            [
                format!("{}.json", hex(&sha(1))),
                format!("{}.json", hex(&sha(2))),
                format!("{}.json", hex(&sha(3))),
            ]
        );

        // And again on the next open.
        drop(store);
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary().files, 2);
        assert_eq!(store.take_warnings().len(), 1);
        assert_eq!(fs::read(&future).unwrap(), bytes);
    }

    #[test]
    fn a_future_version_run_is_kept_as_written_and_skipped() {
        let dir = TempDir::new("future-run");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let first = run(1, 20, default_report());
        store
            .record_run(first.clone(), RecentStatus::Repaired)
            .unwrap();
        let path = store.record_path(&sha(1));
        drop(store);
        // A newer build appended a run in a shape this one does not know,
        // with its keys out of alphabetical order.
        let future_run = r#"{"version": 3, "zeta": 18446744073709551616, "alpha": [1, 2]}"#;
        let text = fs::read_to_string(&path).unwrap();
        let tail = "\n  ]\n}";
        assert!(text.ends_with(tail));
        let edited = format!(
            "{},\n    {future_run}{tail}",
            &text[..text.len() - tail.len()]
        );
        fs::write(&path, &edited).unwrap();

        let mut store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(
            store.runs_for(&sha(1)).unwrap(),
            std::slice::from_ref(&first)
        );
        assert_eq!(store.summary().runs, 1);
        let warnings = store.take_warnings();
        assert!(
            warnings.iter().any(|w| w.contains("version 3")),
            "{warnings:?}"
        );

        // A save keeps the future run, byte for byte, in its place.
        let second = run(1, 30, default_report());
        store
            .record_run(second.clone(), RecentStatus::Partial)
            .unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains(future_run), "{after}");
        let runs = json_at(&path)["runs"].as_array().unwrap().clone();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1]["version"], 3);
        assert_eq!(store.runs_for(&sha(1)).unwrap(), [first, second]);
        assert_eq!(store.list_files("")[0].runs, 2);
        assert!(
            !dir.0
                .join(FILES)
                .join(format!("{}.json.corrupt", hex(&sha(1))))
                .exists()
        );
    }

    #[test]
    fn a_future_version_index_is_rebuilt_with_a_warning() {
        let dir = TempDir::new("future-index");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        let expected = store.summary();
        drop(store);
        fs::write(dir.0.join(INDEX), br#"{"version": 2, "rows": {}}"#).unwrap();
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        assert_eq!(store.summary(), expected);
        let warnings = store.take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(INDEX) && w.contains("version 2")),
            "{warnings:?}"
        );
        assert_eq!(json_at(&dir.0.join(INDEX))["version"], 1);
    }

    #[test]
    fn two_corrupt_records_are_both_kept() {
        let dir = TempDir::new("two-corrupt");
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        store.upsert_file(entry(1, "a.pdf", 10)).unwrap();
        store.upsert_file(entry(2, "b.pdf", 11)).unwrap();
        let record = store.record_path(&sha(1));
        let first = format!("{}.json.corrupt", hex(&sha(1)));
        let second = format!("{}.json.corrupt (2)", hex(&sha(1)));

        // Once through a write in the open store ...
        fs::write(&record, b"{").unwrap();
        store.upsert_file(entry(1, "a.pdf", 20)).unwrap();
        // ... and once through the next open's rebuild.
        fs::write(&record, b"[").unwrap();
        drop(store);
        let mut store = JsonStore::open_at(&dir.0).unwrap();
        let files = dir.0.join(FILES);
        assert_eq!(fs::read(files.join(&first)).unwrap(), b"{");
        assert_eq!(fs::read(files.join(&second)).unwrap(), b"[");
        assert!(!record.exists());
        assert_eq!(store.summary().files, 1);

        // Deleting the file removes every damaged copy.
        store.upsert_file(entry(1, "a.pdf", 30)).unwrap();
        fs::write(files.join("unrelated.json.corrupt (2)"), b"x").unwrap();
        assert!(store.delete_file(&sha(1)).unwrap());
        assert_eq!(
            names_in(&files),
            [
                format!("{}.json", hex(&sha(2))),
                "unrelated.json.corrupt (2)".to_owned(),
            ]
        );
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
