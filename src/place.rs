//! No-clobber output placement (D-044, D-060, D-061).
//!
//! An output never replaces anything: it is written to a temp file in the
//! destination directory, synced, then moved onto a name that does not exist
//! yet, picking `<stem> (N).<ext>` while the name is taken. `std` has no
//! no-replace rename on any OS (FR-r1-f), so the move goes through three tiers,
//! strongest first, and the tier that held is returned as [`Placed`] for the
//! run record (D-073):
//!
//! 1. a no-replace rename: `renameat_with(.., NOREPLACE)` on Unix (`renameat2`
//!    on Linux, `renameatx_np(RENAME_EXCL)` on macOS), `MoveFileExW` without
//!    `MOVEFILE_REPLACE_EXISTING` on Windows → [`Placed::Atomic`];
//! 2. Unix only, when tier 1 is unsupported: `hard_link`, then remove the temp
//!    file → [`Placed::Linked`];
//! 3. when tiers 1 and 2 are unsupported (macOS exFAT rejects both): claim the
//!    name with `create_new`, then rename over our own empty placeholder →
//!    [`Placed::ClaimedThenRenamed`]. Its window is two syscalls, and it can only
//!    overwrite a file that replaced our placeholder inside it, never a file that
//!    existed before.
//!
//! `AlreadyExists` at any tier means "try the next `(N)`", never "try the next
//! tier": where a filesystem does not support a tier it still reports a present
//! destination as EEXIST, so "unsupported" means the name was free at that
//! instant.
// The shell (T-23a) wires this module and has not landed.
// TODO(T-23a): remove this allow once the shell uses it.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub use crate::jobs::Placed;

/// The message a Repair job ends with when it cannot write its output where
/// the examiner asked (D-061). There is no fallback directory (D-062).
pub const READ_ONLY_DESTINATION: &str =
    "can't write beside the original (read-only); set [general] output_dir";

/// Where a job's outputs go: `output_dir` when it is set (created if absent),
/// else the input's own directory. An input with no durable path (a file
/// promise read into memory, D-039) has no "beside", so it needs `output_dir`.
///
/// This is the one branch D-062 would change: there is no automatic fallback
/// to a per-user directory, because an output landing somewhere the examiner
/// did not choose is an audit problem.
pub fn destination_for(input: Option<&Path>, output_dir: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(dir) = output_dir {
        fs::create_dir_all(dir)?;
        return Ok(dir.to_path_buf());
    }
    match input.map(Path::parent) {
        Some(Some(parent)) if parent.as_os_str().is_empty() => Ok(PathBuf::from(".")),
        Some(Some(parent)) => Ok(parent.to_path_buf()),
        _ => Err(io::Error::new(
            ErrorKind::NotFound,
            "the input has no durable path and no output_dir is set",
        )),
    }
}

/// Whether a failure to open the destination means "you cannot write there",
/// which ends the job with [`READ_ONLY_DESTINATION`] (EROFS, EACCES, EPERM,
/// ENOENT and their Windows counterparts).
pub fn is_unwritable(e: &io::Error) -> bool {
    #[cfg(windows)]
    if e.raw_os_error() == Some(windows_sys::Win32::Foundation::ERROR_WRITE_PROTECT as i32) {
        return true;
    }
    matches!(
        e.kind(),
        ErrorKind::ReadOnlyFilesystem | ErrorKind::PermissionDenied | ErrorKind::NotFound
    )
}

/// The canonicalised inputs of the current batch. No output may land on one of
/// them, even when the input is no longer on disk (D-044).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchInputs(BTreeSet<PathBuf>);

impl BatchInputs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, input: &Path) {
        self.0.insert(canonical(input));
    }

    /// True when `path` names a batch input.
    pub fn contains(&self, path: &Path) -> bool {
        !self.0.is_empty() && self.0.contains(&canonical(path))
    }
}

/// `path` canonicalised, or its canonical parent joined with its file name when
/// the path itself does not exist (a candidate output name).
fn canonical(path: &Path) -> PathBuf {
    if let Ok(p) = fs::canonicalize(path) {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            fs::canonicalize(parent)
                .map(|p| p.join(name))
                .unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temp file created with `create_new` in the destination directory.
/// Creating it is the Repair job's only write probe, made before any engine
/// work (D-061); dropping it unplaced removes it.
///
/// Only the name is kept: the handle is closed as soon as the probe succeeds
/// and the file is reopened, without `create`, when the output is placed. A
/// parked job can wait a long time, and one open descriptor per parked job
/// would run a large batch into the per-process file limit (256 by default for
/// a macOS terminal), failing later files instead of letting the batch drain.
#[derive(Debug)]
pub struct TempFile {
    dir: PathBuf,
    path: PathBuf,
    placed: bool,
}

impl TempFile {
    /// Opens `.pdfpundit-<pid>-<n>.tmp` in `dir`, moving to the next `n` while
    /// the name is taken.
    pub fn create(dir: &Path) -> io::Result<TempFile> {
        let pid = std::process::id();
        for _ in 0..1_000 {
            let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".pdfpundit-{pid}-{n}.tmp"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    return Ok(TempFile {
                        dir: dir.to_path_buf(),
                        path,
                        placed: false,
                    });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "no free temp file name in the destination",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `bytes`, syncs, and places the file as `<stem>.<ext>` or the
    /// first free `<stem> (N).<ext>` in the temp file's directory, skipping any
    /// name that is a batch input.
    pub fn place(
        self,
        stem: &str,
        ext: &str,
        bytes: &[u8],
        inputs: &BatchInputs,
    ) -> io::Result<(PathBuf, Placed)> {
        self.place_with(&Tiers::SYSTEM, stem, ext, bytes, inputs)
    }

    fn place_with(
        mut self,
        tiers: &Tiers,
        stem: &str,
        ext: &str,
        bytes: &[u8],
        inputs: &BatchInputs,
    ) -> io::Result<(PathBuf, Placed)> {
        check_component(stem)?;
        let mut file = reopen(&self.path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // Closed before the move: Windows cannot rename a file that is open
        // without delete sharing.
        drop(file);
        for n in 1..=MAX_SUFFIX {
            let dst = self.dir.join(suffixed(stem, n, Some(ext)));
            if inputs.contains(&dst) {
                continue;
            }
            match place_no_replace_with(tiers, &self.path, &dst) {
                Ok(placed) => {
                    self.placed = true;
                    return Ok((dst, placed));
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("every name up to {stem} ({MAX_SUFFIX}).{ext} is taken"),
        ))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.placed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Reopens our own temp file for writing. Never creates it: a temp file that
/// vanished while its job waited is an error, not a new file. On Unix a
/// symlink put in its place is refused rather than followed.
fn reopen(path: &Path) -> io::Result<File> {
    let mut open = OpenOptions::new();
    open.write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = open.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "the temp file is no longer a regular file",
        ));
    }
    Ok(file)
}

/// The highest `(N)` tried before giving up.
const MAX_SUFFIX: u32 = 10_000;

/// `stem.ext` for `n == 1`, else `stem (n).ext`; no extension for directories.
fn suffixed(stem: &str, n: u32, ext: Option<&str>) -> String {
    let base = if n == 1 {
        stem.to_owned()
    } else {
        format!("{stem} ({n})")
    };
    match ext {
        Some(ext) => format!("{base}.{ext}"),
        None => base,
    }
}

/// A single path component: no separator, not empty, not `.` or `..`.
fn check_component(name: &str) -> io::Result<()> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0');
    if bad {
        Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("not a file name: {name:?}"),
        ))
    } else {
        Ok(())
    }
}

/// [`TempFile::create`] then [`TempFile::place`] in `dir`, with no batch.
pub fn write_new(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> io::Result<(PathBuf, Placed)> {
    TempFile::create(dir)?.place(stem, ext, bytes, &BatchInputs::new())
}

/// Moves `tmp` onto `dst` without ever replacing an existing `dst`. Both must
/// be in the same directory. `ErrorKind::AlreadyExists` means `dst` is taken.
pub fn place_no_replace(tmp: &Path, dst: &Path) -> io::Result<Placed> {
    place_no_replace_with(&Tiers::SYSTEM, tmp, dst)
}

type Mover = fn(&Path, &Path) -> io::Result<()>;

/// The two system calls the tiers depend on, as function pointers so a test
/// can make tier 1 (and tier 2) report "unsupported" on any filesystem.
#[derive(Clone, Copy)]
struct Tiers {
    rename_no_replace: Mover,
    hard_link: Mover,
}

impl Tiers {
    const SYSTEM: Tiers = Tiers {
        rename_no_replace,
        hard_link: hard_link_file,
    };
}

fn place_no_replace_with(tiers: &Tiers, tmp: &Path, dst: &Path) -> io::Result<Placed> {
    match (tiers.rename_no_replace)(tmp, dst) {
        Ok(()) => return Ok(Placed::Atomic),
        Err(e) if e.kind() == ErrorKind::AlreadyExists || !unsupported(&e) => return Err(e),
        Err(_) => {}
    }
    // Windows skips tier 2: CreateHardLinkW is NTFS-only and files-only, and
    // MoveFileExW already works on FAT, exFAT, ReFS and SMB.
    if cfg!(unix) {
        match (tiers.hard_link)(tmp, dst) {
            Ok(()) => {
                // Both names hold the full content; losing the temp name is
                // all that is left, and a failure there leaves a harmless file.
                let _ = fs::remove_file(tmp);
                return Ok(Placed::Linked);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists || !unsupported(&e) => return Err(e),
            Err(_) => {}
        }
    }
    // Tier 3: claim the name, then replace our own zero-length placeholder.
    OpenOptions::new().write(true).create_new(true).open(dst)?;
    if let Err(e) = fs::rename(tmp, dst) {
        let _ = fs::remove_file(dst);
        return Err(e);
    }
    Ok(Placed::ClaimedThenRenamed)
}

fn hard_link_file(tmp: &Path, dst: &Path) -> io::Result<()> {
    fs::hard_link(tmp, dst)
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn rename_no_replace(tmp: &Path, dst: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, tmp, CWD, dst, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

/// Other Unixes have no flagged rename in rustix; tier 2 takes over.
#[cfg(all(
    unix,
    not(any(target_os = "linux", target_os = "android", target_vendor = "apple"))
))]
fn rename_no_replace(_tmp: &Path, _dst: &Path) -> io::Result<()> {
    Err(rustix::io::Errno::NOSYS.into())
}

#[cfg(windows)]
fn rename_no_replace(tmp: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(tmp), wide(dst));
    // No MOVEFILE_REPLACE_EXISTING: an existing `dst` fails with 80 or 183,
    // which std maps to AlreadyExists. No MOVEFILE_COPY_ALLOWED: both paths are
    // in one directory, and a cross-volume move should fail loudly.
    // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call.
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// "This filesystem or kernel cannot do that", classified by the raw OS error
/// because macOS reports ENOTSUP as `ErrorKind::Uncategorized`.
#[cfg(unix)]
fn unsupported(e: &io::Error) -> bool {
    use rustix::io::Errno;
    let Some(raw) = e.raw_os_error() else {
        return e.kind() == ErrorKind::Unsupported;
    };
    [
        Errno::INVAL,
        Errno::NOSYS,
        Errno::NOTSUP,
        Errno::OPNOTSUPP,
        Errno::PERM,
    ]
    .iter()
    .any(|errno| errno.raw_os_error() == raw)
}

#[cfg(windows)]
fn unsupported(e: &io::Error) -> bool {
    use windows_sys::Win32::Foundation::{
        ERROR_INVALID_FUNCTION, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED,
    };
    let Some(raw) = e.raw_os_error() else {
        return e.kind() == ErrorKind::Unsupported;
    };
    [
        ERROR_INVALID_FUNCTION,
        ERROR_NOT_SUPPORTED,
        ERROR_INVALID_PARAMETER,
    ]
    .iter()
    .any(|&code| code as i32 == raw)
}

// ── image directories (D-060) ────────────────────────────────────────────

/// Where a set of extracted images went, and whether an identical directory
/// already held them (nothing was written).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagesDir {
    pub path: PathBuf,
    pub reused: bool,
}

/// `<stem>.<first 8 hex of sha256(input)>.images`: named from content, never
/// from what is on disk, so the same input gives the same links (D-060).
pub fn images_dir_name(stem: &str, input_sha256: &[u8; 32]) -> String {
    let hash: String = input_sha256[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{stem}.{hash}.images")
}

/// Writes `files` into the content-named images directory in `dir`. An
/// existing directory of that name whose files are exactly `files` is reused
/// and nothing is written; one with anything else in it is left alone and the
/// images go to `<stem>.<hash> (N).images` instead. The directory is created
/// with `create_dir` (atomic) and every child with `create_new`; there is no
/// staging directory.
pub fn write_images(
    dir: &Path,
    stem: &str,
    input_sha256: &[u8; 32],
    files: &[(String, Vec<u8>)],
) -> io::Result<ImagesDir> {
    check_component(stem)?;
    let names: BTreeSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    if names.len() != files.len() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "two images share a name",
        ));
    }
    for name in &names {
        check_component(name)?;
    }
    let base = images_dir_name(stem, input_sha256);
    let base = base.strip_suffix(".images").expect("named above");
    for n in 1..=MAX_SUFFIX {
        let path = dir.join(format!("{}.images", suffixed(base, n, None)));
        match fs::create_dir(&path) {
            Ok(()) => {
                for (name, bytes) in files {
                    let mut f = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path.join(name))?;
                    f.write_all(bytes)?;
                    f.sync_all()?;
                }
                return Ok(ImagesDir {
                    path,
                    reused: false,
                });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                if holds_exactly(&path, files)? {
                    return Ok(ImagesDir { path, reused: true });
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        format!("every images directory name up to ({MAX_SUFFIX}) is taken"),
    ))
}

/// True when `path` is a real directory (not a symlink) whose entries are
/// exactly `files`, each a regular file with the same bytes.
fn holds_exactly(path: &Path, files: &[(String, Vec<u8>)]) -> io::Result<bool> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Ok(false);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        count += 1;
        let name = entry.file_name();
        let Some(expected) = files
            .iter()
            .find(|(n, _)| name.to_str() == Some(n.as_str()))
        else {
            return Ok(false);
        };
        if !entry.file_type()?.is_file() || fs::read(entry.path())? != expected.1 {
            return Ok(false);
        }
    }
    Ok(count == files.len())
}

#[cfg(test)]
pub(crate) use tests::{ScratchDir, names_in};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    /// A fresh directory under the system temp dir, removed on drop.
    pub(crate) struct ScratchDir(PathBuf);

    impl ScratchDir {
        pub(crate) fn new(label: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("pdfpundit-{label}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("scratch dir");
            ScratchDir(fs::canonicalize(&path).expect("canonical scratch dir"))
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }

        pub(crate) fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// Every entry name, sorted.
        pub(crate) fn names(&self) -> Vec<String> {
            names_in(&self.0)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The error a filesystem gives for an unsupported tier.
    fn unsupported_error() -> io::Error {
        #[cfg(unix)]
        return rustix::io::Errno::NOTSUP.into();
        #[cfg(windows)]
        return io::Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED as i32,
        );
    }

    fn tier_unsupported(_: &Path, _: &Path) -> io::Result<()> {
        Err(unsupported_error())
    }

    fn tier_broken(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::other("disk on fire"))
    }

    fn temp_with(dir: &Path, bytes: &[u8]) -> PathBuf {
        // A free temp name, then the file written and closed (Windows cannot
        // move an open file); the test places it by hand.
        let path = TempFile::create(dir).expect("temp").path().to_path_buf();
        fs::write(&path, bytes).expect("write temp");
        path
    }

    // (a)
    #[test]
    fn an_absent_destination_is_placed_atomically() {
        let dir = ScratchDir::new("place-a");
        let (path, placed) = write_new(dir.path(), "a.repaired", "pdf", b"out").expect("placed");
        assert_eq!(path, dir.join("a.repaired.pdf"));
        assert_eq!(placed, Placed::Atomic);
        assert_eq!(fs::read(&path).expect("read"), b"out");
        assert_eq!(dir.names(), ["a.repaired.pdf"], "the temp file is gone");
    }

    // (b)
    #[test]
    fn an_existing_user_file_survives_and_the_output_takes_2() {
        let dir = ScratchDir::new("place-b");
        fs::write(dir.join("a.repaired.pdf"), b"the user's own file").expect("seed");
        let (path, placed) = write_new(dir.path(), "a.repaired", "pdf", b"new").expect("placed");
        assert_eq!(path, dir.join("a.repaired (2).pdf"));
        assert_eq!(placed, Placed::Atomic);
        assert_eq!(
            fs::read(dir.join("a.repaired.pdf")).expect("read"),
            b"the user's own file"
        );
        assert_eq!(fs::read(&path).expect("read"), b"new");
        let (third, _) = write_new(dir.path(), "a.repaired", "pdf", b"newer").expect("placed");
        assert_eq!(third, dir.join("a.repaired (3).pdf"));
        assert_eq!(
            dir.names(),
            ["a.repaired (2).pdf", "a.repaired (3).pdf", "a.repaired.pdf"]
        );
    }

    // (c)
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_counts_as_taken() {
        let dir = ScratchDir::new("place-c");
        let link = dir.join("a.repaired.pdf");
        std::os::unix::fs::symlink(dir.join("nowhere"), &link).expect("symlink");
        let (path, _) = write_new(dir.path(), "a.repaired", "pdf", b"x").expect("placed");
        assert_eq!(path, dir.join("a.repaired (2).pdf"));
        let target = fs::read_link(&link).expect("still a symlink");
        assert_eq!(target, dir.join("nowhere"));
        assert!(
            !dir.join("nowhere").exists(),
            "nothing written through the link"
        );
    }

    // (d)
    #[test]
    fn sixteen_racing_writers_get_one_name_each() {
        let dir = Arc::new(ScratchDir::new("place-d"));
        let barrier = Arc::new(Barrier::new(16));
        let threads: Vec<_> = (0..16u8)
            .map(|i| {
                let (dir, barrier) = (Arc::clone(&dir), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    let tmp = TempFile::create(dir.path()).expect("temp");
                    barrier.wait();
                    tmp.place("race", "pdf", &[i], &BatchInputs::new())
                        .expect("placed")
                })
            })
            .collect();
        let results: Vec<(PathBuf, Placed)> = threads
            .into_iter()
            .map(|t| t.join().expect("join"))
            .collect();
        let base: Vec<_> = results
            .iter()
            .filter(|(p, _)| *p == dir.join("race.pdf"))
            .collect();
        assert_eq!(base.len(), 1, "exactly one writer gets the plain name");
        assert!(results.iter().all(|(_, placed)| *placed == Placed::Atomic));
        let names: BTreeSet<_> = results.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(names.len(), 16, "no two writers share a name");
        for n in 2..=16 {
            assert!(names.contains(&dir.join(&format!("race ({n}).pdf"))));
        }
        // Every file holds the bytes of the writer that got its name.
        for (path, _) in &results {
            assert_eq!(fs::read(path).expect("read").len(), 1);
        }
        assert_eq!(dir.names().len(), 16, "no temp file is left");
    }

    // (e)
    #[test]
    fn unsupported_tiers_fall_through_and_say_which_held() {
        let dir = ScratchDir::new("place-e");
        let no_rename = Tiers {
            rename_no_replace: tier_unsupported,
            hard_link: hard_link_file,
        };
        let tmp = temp_with(dir.path(), b"linked");
        let placed = place_no_replace_with(&no_rename, &tmp, &dir.join("one.pdf")).expect("placed");
        let expected = if cfg!(unix) {
            Placed::Linked
        } else {
            Placed::ClaimedThenRenamed
        };
        assert_eq!(placed, expected);
        assert!(!tmp.exists());
        assert_eq!(fs::read(dir.join("one.pdf")).expect("read"), b"linked");

        let neither = Tiers {
            rename_no_replace: tier_unsupported,
            hard_link: tier_unsupported,
        };
        let tmp = temp_with(dir.path(), b"claimed");
        let placed = place_no_replace_with(&neither, &tmp, &dir.join("two.pdf")).expect("placed");
        assert_eq!(placed, Placed::ClaimedThenRenamed);
        assert!(!tmp.exists());
        assert_eq!(fs::read(dir.join("two.pdf")).expect("read"), b"claimed");

        // A taken name is AlreadyExists at tier 3 too, never a replacement.
        let tmp = temp_with(dir.path(), b"third");
        let e = place_no_replace_with(&neither, &tmp, &dir.join("two.pdf")).expect_err("taken");
        assert_eq!(e.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(dir.join("two.pdf")).expect("read"), b"claimed");
        assert_eq!(
            fs::read(&tmp).expect("kept"),
            b"third",
            "the source is kept"
        );
        fs::remove_file(&tmp).expect("tidy");

        // Through the suffix loop: the tier is reported with the (N) name.
        let tmp = TempFile::create(dir.path()).expect("temp");
        let (path, placed) = tmp
            .place_with(&neither, "two", "pdf", b"fourth", &BatchInputs::new())
            .expect("placed");
        assert_eq!(path, dir.join("two (2).pdf"));
        assert_eq!(placed, Placed::ClaimedThenRenamed);
        assert!(dir.names().iter().all(|n| !n.ends_with(".tmp")));
    }

    #[test]
    fn a_hard_failure_is_returned_and_the_temp_file_removed() {
        let dir = ScratchDir::new("place-fail");
        let broken = Tiers {
            rename_no_replace: tier_broken,
            hard_link: hard_link_file,
        };
        let tmp = TempFile::create(dir.path()).expect("temp");
        let e = tmp
            .place_with(&broken, "a", "pdf", b"x", &BatchInputs::new())
            .expect_err("tier 1 failed for real");
        assert_eq!(e.to_string(), "disk on fire");
        assert!(dir.names().is_empty(), "no temp file and no output");

        let tmp = TempFile::create(dir.path()).expect("temp");
        assert_eq!(dir.names().len(), 1);
        drop(tmp);
        assert!(dir.names().is_empty(), "an unplaced temp file is removed");
    }

    #[test]
    fn a_waiting_temp_file_holds_no_descriptor_and_is_never_recreated() {
        let dir = ScratchDir::new("place-nofd");
        // More than a macOS terminal's default soft limit of 256 descriptors:
        // parked jobs each keep a TempFile for as long as they wait.
        let waiting: Vec<TempFile> = (0..300)
            .map(|_| TempFile::create(dir.path()).expect("temp"))
            .collect();
        assert_eq!(dir.names().len(), 300);
        let mut waiting = waiting.into_iter();
        let first = waiting.next().expect("first");
        let (path, _) = first
            .place("a.repaired", "pdf", b"out", &BatchInputs::new())
            .expect("placed");
        assert_eq!(fs::read(&path).expect("read"), b"out");

        // A temp file that vanished while its job waited is an error.
        let gone = waiting.next().expect("second");
        fs::remove_file(gone.path()).expect("remove");
        let e = gone
            .place("b.repaired", "pdf", b"x", &BatchInputs::new())
            .expect_err("vanished");
        assert_eq!(e.kind(), ErrorKind::NotFound);
        drop(waiting);
        assert_eq!(dir.names(), ["a.repaired.pdf"]);
    }

    #[test]
    fn a_batch_input_name_is_skipped_even_when_absent() {
        let dir = ScratchDir::new("place-inputs");
        let mut inputs = BatchInputs::new();
        // An input already read into memory and gone from disk.
        inputs.insert(&dir.join("a.repaired.pdf"));
        let tmp = TempFile::create(dir.path()).expect("temp");
        let (path, _) = tmp
            .place("a.repaired", "pdf", b"x", &inputs)
            .expect("placed");
        assert_eq!(path, dir.join("a.repaired (2).pdf"));
        assert!(!dir.join("a.repaired.pdf").exists());
    }

    #[test]
    fn batch_inputs_compare_canonical_paths() {
        let dir = ScratchDir::new("place-canon");
        fs::create_dir(dir.join("sub")).expect("sub");
        fs::write(dir.join("a.pdf"), b"x").expect("input");
        let mut inputs = BatchInputs::new();
        inputs.insert(&dir.join("sub/../a.pdf"));
        assert!(inputs.contains(&dir.join("a.pdf")));
        assert!(inputs.contains(&dir.join("sub").join("..").join("a.pdf")));
        assert!(!inputs.contains(&dir.join("b.pdf")));
    }

    #[test]
    fn names_with_separators_are_refused() {
        let dir = ScratchDir::new("place-names");
        for stem in ["", ".", "..", "a/b", "a\\b"] {
            let e = write_new(dir.path(), stem, "pdf", b"x").expect_err(stem);
            assert_eq!(e.kind(), ErrorKind::InvalidInput);
        }
        assert!(dir.names().is_empty());
    }

    #[test]
    fn the_destination_is_output_dir_else_beside_the_input() {
        let dir = ScratchDir::new("place-dest");
        let input = dir.join("evidence/a.pdf");
        assert_eq!(
            destination_for(Some(&input), None).expect("beside"),
            dir.join("evidence")
        );
        assert_eq!(
            destination_for(Some(Path::new("a.pdf")), None).expect("cwd"),
            PathBuf::from(".")
        );
        let out = dir.join("out/nested");
        assert_eq!(
            destination_for(Some(&input), Some(&out)).expect("output_dir"),
            out
        );
        assert!(out.is_dir(), "output_dir is created when absent");
        assert_eq!(destination_for(None, Some(&out)).expect("promise"), out);
        let e = destination_for(None, None).expect_err("no beside");
        assert!(is_unwritable(&e));
    }

    // (g)
    #[test]
    fn an_identical_images_directory_is_reused_and_a_different_one_is_not() {
        let dir = ScratchDir::new("place-g");
        let sha = [0xab; 32];
        let files = vec![
            ("p1-1.jpg".to_string(), b"jpeg one".to_vec()),
            ("p3-1.jpg".to_string(), b"jpeg two".to_vec()),
        ];
        assert_eq!(images_dir_name("scan", &sha), "scan.abababab.images");

        let first = write_images(dir.path(), "scan", &sha, &files).expect("written");
        assert_eq!(first.path, dir.join("scan.abababab.images"));
        assert!(!first.reused);
        assert_eq!(names_in(&first.path), ["p1-1.jpg", "p3-1.jpg"]);

        let modified = fs::metadata(first.path.join("p1-1.jpg"))
            .and_then(|m| m.modified())
            .expect("mtime");
        let again = write_images(dir.path(), "scan", &sha, &files).expect("reused");
        assert_eq!(
            again,
            ImagesDir {
                path: first.path.clone(),
                reused: true
            }
        );
        assert_eq!(
            fs::metadata(first.path.join("p1-1.jpg"))
                .and_then(|m| m.modified())
                .expect("mtime"),
            modified,
            "nothing was rewritten"
        );
        assert_eq!(dir.names(), ["scan.abababab.images"]);

        let other = vec![("p1-1.jpg".to_string(), b"something else".to_vec())];
        let suffixed = write_images(dir.path(), "scan", &sha, &other).expect("written");
        assert_eq!(suffixed.path, dir.join("scan.abababab (2).images"));
        assert!(!suffixed.reused);
        assert_eq!(
            fs::read(first.path.join("p1-1.jpg")).expect("read"),
            b"jpeg one"
        );

        // A directory with an extra file is different too.
        fs::write(first.path.join("notes.txt"), b"mine").expect("user file");
        let third = write_images(dir.path(), "scan", &sha, &files).expect("written");
        assert_eq!(third.path, dir.join("scan.abababab (3).images"));
        assert_eq!(
            fs::read(first.path.join("notes.txt")).expect("read"),
            b"mine"
        );
    }

    #[test]
    fn image_names_must_be_plain_and_distinct() {
        let dir = ScratchDir::new("place-g-names");
        let sha = [0; 32];
        let dup = vec![("a".to_string(), vec![1]), ("a".to_string(), vec![2])];
        assert!(write_images(dir.path(), "s", &sha, &dup).is_err());
        let escape = vec![("../a".to_string(), vec![1])];
        assert!(write_images(dir.path(), "s", &sha, &escape).is_err());
        assert!(dir.names().is_empty());
    }

    // (f) macOS exFAT, where tiers 1 and 2 both report ENOTSUP. Needs hdiutil
    // and a mountable disk image, so it runs only on request:
    // `cargo test exfat -- --ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "creates and mounts an exFAT disk image with hdiutil"]
    fn exfat_falls_through_to_the_claim_tier() {
        use std::process::Command;
        let scratch = ScratchDir::new("place-exfat");
        let image = scratch.join("vol.dmg");
        let mount = scratch.join("mnt");
        fs::create_dir(&mount).expect("mount point");
        let ok = Command::new("hdiutil")
            .args([
                "create", "-size", "16m", "-fs", "ExFAT", "-volname", "PPEXFAT",
            ])
            .arg(&image)
            .status()
            .expect("hdiutil create")
            .success();
        assert!(ok, "hdiutil create");
        let ok = Command::new("hdiutil")
            .args(["attach", "-nobrowse", "-mountpoint"])
            .arg(&mount)
            .arg(&image)
            .status()
            .expect("hdiutil attach")
            .success();
        assert!(ok, "hdiutil attach");
        let result = std::panic::catch_unwind(|| {
            let (path, placed) = write_new(&mount, "a.repaired", "pdf", b"exfat").expect("placed");
            assert_eq!(placed, Placed::ClaimedThenRenamed);
            assert_eq!(fs::read(&path).expect("read"), b"exfat");
            let (second, _) = write_new(&mount, "a.repaired", "pdf", b"2").expect("placed");
            assert_eq!(second, mount.join("a.repaired (2).pdf"));
            assert_eq!(fs::read(&path).expect("read"), b"exfat");
        });
        let _ = Command::new("hdiutil").arg("detach").arg(&mount).status();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}
