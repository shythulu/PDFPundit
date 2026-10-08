//! The browse picker's directory listing (T-37): one folder's entries,
//! filtered and sorted, read without following a symlink out of the folder.
//!
//! | entry | listed when |
//! |---|---|
//! | a folder | always |
//! | a file | the filter lets it through (`.pdf`, any case) |
//! | a hidden one (a name starting with `.`; on Windows also the hidden attribute) | only when hidden entries are shown |
//! | a symlink (or a Windows junction) | its target resolves inside the folder being listed, and is then listed as its target is |
//! | anything else (a socket, a device, a broken link) | never |
//!
//! The symlink rule keeps the picker inside the tree it is looking at: a link
//! to an ancestor (a loop) or to somewhere else on the disk is left out, so
//! descending only ever goes deeper. Folders come first, then files, each by
//! name without regard to case (the exact name breaks a tie), so the order is
//! the same on every platform.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::path::Path;

use super::input::paste::is_pdf;

/// What the listing lets through besides folders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    /// Files whose name ends in `.pdf`, in any case (the drop gate's rule).
    #[default]
    PdfOnly,
}

impl Filter {
    fn admits(self, name: &Path) -> bool {
        match self {
            Filter::PdfOnly => is_pdf(name),
        }
    }
}

/// One listed entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The name inside the folder, as the file system has it.
    pub name: OsString,
    pub is_dir: bool,
    /// The file's length in bytes; 0 for a folder.
    pub size: u64,
}

impl Entry {
    /// The name as text; a name that is not valid Unicode is drawn with `�`.
    pub fn display(&self) -> String {
        self.name.to_string_lossy().into_owned()
    }
}

/// The entries of `dir` that `filter` lets through, hidden ones only when
/// `hidden`, folders first and each group sorted by name. An entry that
/// cannot be read is left out; only a folder that cannot be read is an error.
pub fn list(dir: &Path, filter: Filter, hidden: bool) -> io::Result<Vec<Entry>> {
    // Resolved once, and only when a symlink needs it.
    let mut real_dir = None;
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else {
            continue;
        };
        let name = item.file_name();
        let Ok(own) = fs::symlink_metadata(item.path()) else {
            continue;
        };
        if !hidden && is_hidden(&name, &own) {
            continue;
        }
        let meta = if own.file_type().is_symlink() {
            let inside = real_dir
                .get_or_insert_with(|| fs::canonicalize(dir))
                .as_ref()
                .ok()
                .zip(fs::canonicalize(item.path()).ok())
                .is_some_and(|(root, target)| target != *root && target.starts_with(root));
            if !inside {
                continue;
            }
            match fs::metadata(item.path()) {
                Ok(m) => m,
                Err(_) => continue,
            }
        } else {
            own
        };
        if meta.is_dir() {
            out.push(Entry {
                name,
                is_dir: true,
                size: 0,
            });
        } else if meta.is_file() && filter.admits(Path::new(&name)) {
            out.push(Entry {
                name,
                is_dir: false,
                size: meta.len(),
            });
        }
    }
    out.sort_by(order);
    Ok(out)
}

/// Folders first, then by name ignoring case, then by the exact name.
fn order(a: &Entry, b: &Entry) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.display().to_lowercase().cmp(&b.display().to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}

/// A dot name, or (Windows) an entry with the hidden attribute.
fn is_hidden(name: &OsString, _meta: &Metadata) -> bool {
    if name.to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if _meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
            return true;
        }
    }
    false
}

/// A size as the picker shows it: bytes, then KiB, MiB and GiB, the last two
/// with one decimal (rounded down, integer arithmetic).
pub fn size_label(bytes: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    let tenths = |unit: u64| u128::from(bytes) * 10 / u128::from(unit);
    match bytes {
        b if b < KIB => format!("{b} B"),
        b if b < MIB => format!("{} KiB", b / KIB),
        _ if bytes < GIB => {
            let t = tenths(MIB);
            format!("{}.{} MiB", t / 10, t % 10)
        }
        _ => {
            let t = tenths(GIB);
            format!("{}.{} GiB", t / 10, t % 10)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::place::ScratchDir;

    fn names(entries: &[Entry]) -> Vec<(String, bool)> {
        entries.iter().map(|e| (e.display(), e.is_dir)).collect()
    }

    /// The ticket's fixture: `a.pdf`, `b.PDF`, `c.txt` and `sub/`.
    pub(crate) fn fixture(label: &str) -> ScratchDir {
        let dir = ScratchDir::new(label);
        fs::write(dir.join("a.pdf"), b"%PDF-1.7\n").unwrap();
        fs::write(dir.join("b.PDF"), b"%PDF-1.4\n%%EOF\n").unwrap();
        fs::write(dir.join("c.txt"), b"%PDF- but a text file").unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn the_fixture_shows_two_files_and_one_folder() {
        let dir = fixture("fs-fixture");
        let got = list(dir.path(), Filter::PdfOnly, false).unwrap();
        assert_eq!(
            names(&got),
            [
                ("sub".into(), true),
                ("a.pdf".into(), false),
                ("b.PDF".into(), false)
            ]
        );
        assert_eq!(got[1].size, 9);
        assert_eq!(got[2].size, 15);
        assert_eq!(got[0].size, 0);
    }

    #[test]
    fn folders_come_first_and_names_sort_without_case() {
        let dir = ScratchDir::new("fs-sort");
        for f in ["Zed.pdf", "apple.pdf", "Mango.PDF", "banana.pdf"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        for d in ["zoo", "Alpha", "beta"] {
            fs::create_dir(dir.join(d)).unwrap();
        }
        let got = list(dir.path(), Filter::PdfOnly, false).unwrap();
        let order: Vec<String> = got.iter().map(Entry::display).collect();
        assert_eq!(
            order,
            [
                "Alpha",
                "beta",
                "zoo",
                "apple.pdf",
                "banana.pdf",
                "Mango.PDF",
                "Zed.pdf"
            ]
        );
    }

    #[test]
    fn hidden_entries_are_listed_only_when_asked() {
        let dir = ScratchDir::new("fs-hidden");
        fs::write(dir.join(".secret.pdf"), b"x").unwrap();
        fs::create_dir(dir.join(".cache")).unwrap();
        fs::write(dir.join("seen.pdf"), b"x").unwrap();
        let shown = |hidden| names(&list(dir.path(), Filter::PdfOnly, hidden).unwrap());
        assert_eq!(shown(false), [("seen.pdf".into(), false)]);
        assert_eq!(
            shown(true),
            [
                (".cache".into(), true),
                (".secret.pdf".into(), false),
                ("seen.pdf".into(), false)
            ]
        );
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let dir = ScratchDir::new("fs-missing");
        assert!(list(&dir.join("gone"), Filter::PdfOnly, false).is_err());
        // A file is not a folder either.
        fs::write(dir.join("a.pdf"), b"x").unwrap();
        assert!(list(&dir.join("a.pdf"), Filter::PdfOnly, false).is_err());
    }

    /// Links inside the tree are listed as their targets; links to an
    /// ancestor, to the folder itself or out of the tree, and broken links,
    /// are not.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_followed_only_inside_the_tree() {
        use std::os::unix::fs::symlink;
        let outside = ScratchDir::new("fs-outside");
        fs::write(outside.join("far.pdf"), b"far").unwrap();
        let dir = fixture("fs-links");
        fs::write(dir.join("sub/deep.pdf"), b"deep!").unwrap();
        symlink(dir.join("sub"), dir.join("into-sub")).unwrap();
        symlink(dir.join("sub/deep.pdf"), dir.join("near.pdf")).unwrap();
        symlink(outside.join("far.pdf"), dir.join("far.pdf")).unwrap();
        symlink(outside.path(), dir.join("away")).unwrap();
        symlink(dir.path(), dir.join("self")).unwrap();
        symlink(dir.join("nothing.pdf"), dir.join("broken.pdf")).unwrap();
        symlink(dir.path(), dir.join("sub/up")).unwrap();
        symlink("..", dir.join("sub/dotdot")).unwrap();

        let got = list(dir.path(), Filter::PdfOnly, false).unwrap();
        assert_eq!(
            names(&got),
            [
                ("into-sub".into(), true),
                ("sub".into(), true),
                ("a.pdf".into(), false),
                ("b.PDF".into(), false),
                ("near.pdf".into(), false),
            ]
        );
        assert_eq!(got[4].size, 5, "listed as its target");
        let sub = list(&dir.join("sub"), Filter::PdfOnly, false).unwrap();
        assert_eq!(names(&sub), [("deep.pdf".into(), false)]);
        // Through the in-tree link, the same.
        let via = list(&dir.join("into-sub"), Filter::PdfOnly, false).unwrap();
        assert_eq!(names(&via), [("deep.pdf".into(), false)]);
    }

    #[test]
    fn sizes_read_in_whole_units() {
        for (bytes, want) in [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1 KiB"),
            (1_048_575, "1023 KiB"),
            (1 << 20, "1.0 MiB"),
            (1_100_000, "1.0 MiB"),
            (1_400_000, "1.3 MiB"),
            ((1 << 30) - 1, "1023.9 MiB"),
            (5 << 30, "5.0 GiB"),
            (u64::MAX, "17179869183.9 GiB"),
        ] {
            assert_eq!(size_label(bytes), want, "{bytes}");
        }
    }
}
