//! The browse picker's directory listing (T-37): one folder's entries,
//! filtered and sorted, read without following a symlink out of the folder.
//!
//! | entry | listed when |
//! |---|---|
//! | a folder | always |
//! | a file | the filter lets it through (`.pdf`, any case) |
//! | a hidden one (a name starting with `.`; on Windows also the hidden attribute) | only when hidden entries are shown |
//! | a symlink (or a Windows junction) | every hop of its chain stays strictly inside the folder being listed; it is then listed as what it ends at |
//! | anything else (a socket, a device, a broken link) | never |
//!
//! The symlink rule keeps the picker inside the tree it is looking at: a link
//! to an ancestor (a loop), to the folder itself or to somewhere else on the
//! disk is left out, so descending only ever goes deeper. Folders come first,
//! then files, each by name without regard to case (the exact name breaks a
//! tie), so the order is the same on every platform.
//!
//! A link is judged from its text, never by opening what it points at. Opening
//! a link to `\\host\share` (Windows) or `/net/host` (macOS automount) would
//! reach out over the network just because the folder was listed, and a link
//! to a dead mount would hang the UI. So [`follow`] reads each link with
//! `read_link`, walks its target one name at a time from the folder down,
//! looking at each name without following it, and gives up at the first step
//! that would leave the folder, a share or device path, or too many hops. Only
//! names already shown to be inside the folder are ever looked at.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::{self, Metadata};
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};

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
    list_with(dir, filter, hidden, &mut Disk)
}

/// [`list`], with `look` doing every look a link's walk makes.
fn list_with(
    dir: &Path,
    filter: Filter,
    hidden: bool,
    look: &mut impl Look,
) -> io::Result<Vec<Entry>> {
    let mut tree = Tree::new(dir);
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else {
            continue;
        };
        let name = item.file_name();
        let path = item.path();
        let Ok(own) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !hidden && is_hidden(&name, &own) {
            continue;
        }
        let node = match node_of(&path, &own) {
            Some(Node::Link(target)) => follow(&mut tree, target, look),
            other => other,
        };
        match node {
            Some(Node::Dir) => out.push(Entry {
                name,
                is_dir: true,
                size: 0,
            }),
            Some(Node::File(size)) if filter.admits(Path::new(&name)) => out.push(Entry {
                name,
                is_dir: false,
                size,
            }),
            _ => {}
        }
    }
    out.sort_by(order);
    Ok(out)
}

/// What one path is, looked at without following a link there.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Dir,
    /// A file and its length in bytes.
    File(u64),
    /// A symlink or a Windows junction, and the target as written in it.
    Link(PathBuf),
    /// A socket, a device, a pipe.
    Other,
}

/// Looks at one path without following a link there (`lstat`, then
/// `read_link` for a link). The walk's only way to touch the disk, so a test
/// can see every path it touches.
trait Look {
    fn look(&mut self, path: &Path) -> Option<Node>;
}

/// The real file system.
struct Disk;

impl Look for Disk {
    fn look(&mut self, path: &Path) -> Option<Node> {
        let meta = fs::symlink_metadata(path).ok()?;
        node_of(path, &meta)
    }
}

/// `meta` (from `symlink_metadata`) as a [`Node`]; for a link, its target
/// from `read_link`, which reads the link and not what it points at.
fn node_of(path: &Path, meta: &Metadata) -> Option<Node> {
    let kind = meta.file_type();
    Some(if kind.is_symlink() {
        Node::Link(fs::read_link(path).ok()?)
    } else if kind.is_dir() {
        Node::Dir
    } else if kind.is_file() {
        Node::File(meta.len())
    } else {
        Node::Other
    })
}

/// Links followed for one entry before it is left out (a loop ends here).
const MAX_LINKS: usize = 8;

/// The folder being listed, and the absolute forms an absolute link target
/// is matched against.
struct Tree<'a> {
    dir: &'a Path,
    /// Made on the first absolute target: `dir` made absolute, and `dir`
    /// with its own links resolved (that touches only `dir` and its
    /// ancestors, which listing it already has).
    roots: Option<Vec<PathBuf>>,
}

impl<'a> Tree<'a> {
    fn new(dir: &'a Path) -> Tree<'a> {
        Tree { dir, roots: None }
    }

    /// The part of the absolute `target` below the folder, if it is below it.
    fn below(&mut self, target: &Path) -> Option<PathBuf> {
        let dir = self.dir;
        let roots = self.roots.get_or_insert_with(|| {
            let mut roots = Vec::new();
            for root in [std::path::absolute(dir), fs::canonicalize(dir)]
                .into_iter()
                .flatten()
            {
                if let Some(root) = local(&root).filter(|r| !roots.contains(r)) {
                    roots.push(root);
                }
            }
            roots
        });
        roots
            .iter()
            .find_map(|root| target.strip_prefix(root).ok())
            .map(Path::to_path_buf)
    }
}

/// One step of a walk below the folder.
enum Step {
    Up,
    Name(OsString),
}

/// What the link entry whose target is `target` ends at, if every hop of
/// its chain stays strictly inside `tree`'s folder. Each hop is decided from
/// the link's text: an absolute target must start with the folder, a share
/// or device target is refused outright, and a relative one is walked from
/// the link's own folder. Only names the walk has reached from the folder
/// are looked at, one at a time and never followed, so nothing outside the
/// folder is touched, not even to see whether it exists.
fn follow(tree: &mut Tree, target: PathBuf, look: &mut impl Look) -> Option<Node> {
    let mut at = tree.dir.to_path_buf();
    // Names pushed onto `at` below the folder.
    let mut depth = 0usize;
    let mut steps = VecDeque::new();
    let mut links = 0;
    let mut pending = Some(target);
    // What `at` is: the folder itself until a name is reached.
    let mut last = Node::Dir;
    loop {
        if let Some(target) = pending.take() {
            links += 1;
            if links > MAX_LINKS {
                return None;
            }
            let rest = if bare(&target) {
                target
            } else {
                // Absolute: it must name a place below the folder.
                let target = local(&target)?;
                if !target.is_absolute() {
                    // `C:x` or `\x` on Windows: relative to something else.
                    return None;
                }
                let rest = tree.below(&target)?;
                at = tree.dir.to_path_buf();
                depth = 0;
                rest
            };
            for step in steps_of(&rest)?.into_iter().rev() {
                steps.push_front(step);
            }
        }
        let Some(step) = steps.pop_front() else {
            break;
        };
        // Only a folder has names under it.
        if last != Node::Dir {
            return None;
        }
        match step {
            Step::Up => {
                if depth == 0 {
                    return None;
                }
                at.pop();
                depth -= 1;
                last = Node::Dir;
            }
            Step::Name(name) => {
                at.push(&name);
                depth += 1;
                match look.look(&at)? {
                    Node::Link(next) => {
                        // Its target is read from the link's folder.
                        at.pop();
                        depth -= 1;
                        last = Node::Dir;
                        pending = Some(next);
                    }
                    node => last = node,
                }
            }
        }
    }
    // The folder itself (`self -> .`) is a loop, not an entry.
    (depth > 0).then_some(last)
}

/// A target with neither a root nor a drive or share prefix.
fn bare(target: &Path) -> bool {
    !matches!(
        target.components().next(),
        Some(Component::Prefix(_) | Component::RootDir)
    )
}

/// The steps of a relative path. A `.` or `..` that a verbatim path keeps as
/// a name is still a step, so the walk's count stays true.
fn steps_of(rest: &Path) -> Option<Vec<Step>> {
    let mut out = Vec::new();
    for part in rest.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => out.push(Step::Up),
            Component::Normal(n) if n == OsStr::new(".") => {}
            Component::Normal(n) if n == OsStr::new("..") => out.push(Step::Up),
            Component::Normal(n) => out.push(Step::Name(n.to_os_string())),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// `path` as a plain local path, or `None` for a share or device path. A
/// share (`\\host\share`, `\\?\UNC\host\share`) would reach out over the
/// network, the drop gate's `is_unc` rule; a device or volume path (`\\.\`,
/// `\\?\Volume{..}`) is not a place in a folder. A verbatim drive path
/// (`\\?\C:\`, what `read_link` gives for a junction and `canonicalize`
/// for any path) becomes the plain one (`C:\`), so the two compare. Paths
/// have prefixes only on Windows.
fn local(path: &Path) -> Option<PathBuf> {
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Some(path.to_path_buf());
    };
    match prefix.kind() {
        Prefix::Disk(_) => Some(path.to_path_buf()),
        Prefix::VerbatimDisk(drive) => {
            let mut plain = PathBuf::from(format!("{}:", char::from(drive.to_ascii_uppercase())));
            plain.extend(parts);
            Some(plain)
        }
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) | Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
            None
        }
    }
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

/// A size as the picker shows it: bytes, then KiB, then MiB up to EiB with
/// one decimal (rounded down, integer arithmetic). Never more than 10
/// characters (`1023.9 GiB`), the picker's size column; `u64::MAX` is
/// `15.9 EiB`.
pub fn size_label(bytes: u64) -> String {
    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    if bytes < KIB {
        return format!("{bytes} B");
    }
    if bytes < MIB {
        return format!("{} KiB", bytes / KIB);
    }
    // The largest unit with at least one whole of it (MiB at least here).
    let (shift, name) = [(60, "EiB"), (50, "PiB"), (40, "TiB"), (30, "GiB")]
        .into_iter()
        .find(|&(shift, _)| bytes >> shift > 0)
        .unwrap_or((20, "MiB"));
    let tenths = (u128::from(bytes) * 10) >> shift;
    format!("{}.{} {name}", tenths / 10, tenths % 10)
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

    /// Records every path the link walk looks at, on the real disk.
    struct Seen(Vec<PathBuf>);

    impl Look for Seen {
        fn look(&mut self, path: &Path) -> Option<Node> {
            self.0.push(path.to_path_buf());
            Disk.look(path)
        }
    }

    /// A made-up disk: the paths it knows, and every path it was asked about.
    #[derive(Default)]
    struct Fake {
        nodes: Vec<(PathBuf, Node)>,
        seen: Vec<PathBuf>,
    }

    impl Look for Fake {
        fn look(&mut self, path: &Path) -> Option<Node> {
            self.seen.push(path.to_path_buf());
            self.nodes
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, n)| n.clone())
        }
    }

    /// Links out of the tree, to an automount-style path or along a chain
    /// that leaves it, are left out without the walk looking at anything
    /// outside the folder: their targets are never touched.
    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_tree_is_refused_without_touching_its_target() {
        use std::os::unix::fs::symlink;
        let outside = ScratchDir::new("fs-untouched");
        fs::write(outside.join("far.pdf"), b"far").unwrap();
        let dir = fixture("fs-untouched-in");
        symlink(
            "/net/pdfpundit-no-such-host/share/x.pdf",
            dir.join("net.pdf"),
        )
        .unwrap();
        symlink(outside.join("far.pdf"), dir.join("far.pdf")).unwrap();
        symlink("hop2.pdf", dir.join("hop1.pdf")).unwrap();
        symlink(outside.join("far.pdf"), dir.join("hop2.pdf")).unwrap();
        symlink("sub/../../x/far.pdf", dir.join("climb.pdf")).unwrap();
        symlink("loop-b", dir.join("loop-a")).unwrap();
        symlink("loop-a", dir.join("loop-b")).unwrap();
        // In the tree through a relative chain: listed.
        symlink("sub/../a.pdf", dir.join("near.pdf")).unwrap();

        let mut seen = Seen(Vec::new());
        let got = list_with(dir.path(), Filter::PdfOnly, false, &mut seen).unwrap();
        assert_eq!(
            names(&got),
            [
                ("sub".into(), true),
                ("a.pdf".into(), false),
                ("b.PDF".into(), false),
                ("near.pdf".into(), false),
            ]
        );
        assert!(!seen.0.is_empty());
        for path in &seen.0 {
            assert!(
                path.starts_with(dir.path()) && path != dir.path(),
                "{} is outside the folder",
                path.display()
            );
        }
    }

    /// The walk on a made-up disk: what it looks at for each kind of target.
    /// Unix paths; the Windows forms are in the test after the next.
    #[cfg(unix)]
    #[test]
    fn the_walk_looks_only_inside_the_folder() {
        let dir = PathBuf::from("/case");
        let mut fake = Fake::default();
        fake.nodes.push((dir.join("sub"), Node::Dir));
        fake.nodes.push((dir.join("sub/a.pdf"), Node::File(7)));
        fake.nodes
            .push((dir.join("sub/hop"), Node::Link("../out".into())));
        fake.nodes
            .push((dir.join("out"), Node::Link("/elsewhere".into())));
        let mut walk = |target: &str| {
            fake.seen.clear();
            let got = follow(&mut Tree::new(&dir), target.into(), &mut fake);
            (got, std::mem::take(&mut fake.seen))
        };
        assert_eq!(
            walk("sub/a.pdf"),
            (
                Some(Node::File(7)),
                vec![dir.join("sub"), dir.join("sub/a.pdf")]
            )
        );
        assert_eq!(
            walk("/case/sub/./a.pdf"),
            (
                Some(Node::File(7)),
                vec![dir.join("sub"), dir.join("sub/a.pdf")]
            )
        );
        // Out of the tree, the folder itself, a file as a folder: nothing or
        // only names inside the folder are looked at.
        for target in [
            "/net/host/x.pdf",
            "/elsewhere",
            "..",
            "../case/sub",
            "/case",
            ".",
        ] {
            assert_eq!(walk(target), (None, vec![]), "{target}");
        }
        assert_eq!(walk("sub/a.pdf/x").0, None);
        // A chain stops at the hop that leaves.
        assert_eq!(
            walk("sub/hop"),
            (
                None,
                vec![dir.join("sub"), dir.join("sub/hop"), dir.join("out")]
            )
        );
    }

    /// A loop ends at the hop limit.
    #[test]
    fn a_link_loop_ends() {
        let dir = PathBuf::from("/case");
        let mut fake = Fake::default();
        fake.nodes.push((dir.join("a"), Node::Link("b".into())));
        fake.nodes.push((dir.join("b"), Node::Link("a".into())));
        assert_eq!(follow(&mut Tree::new(&dir), "a".into(), &mut fake), None);
        assert_eq!(fake.seen.len(), MAX_LINKS);
    }

    /// Windows: a share, device or volume target is refused before anything
    /// is looked at; a verbatim drive path is the plain one.
    #[cfg(windows)]
    #[test]
    fn a_share_or_device_target_is_refused_untouched() {
        let dir = PathBuf::from(r"C:\case");
        let mut fake = Fake::default();
        fake.nodes.push((dir.join("a.pdf"), Node::File(3)));
        for target in [
            r"\\server\share",
            r"\\server\share\a.pdf",
            r"\\?\UNC\server\share\a.pdf",
            r"\\.\pipe\x",
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\a.pdf",
            r"C:a.pdf",
            r"\case\a.pdf",
        ] {
            let got = follow(&mut Tree::new(&dir), target.into(), &mut fake);
            assert_eq!(got, None, "{target}");
            assert!(fake.seen.is_empty(), "{target} was touched");
        }
        let got = follow(&mut Tree::new(&dir), r"\\?\C:\case\a.pdf".into(), &mut fake);
        assert_eq!(got, Some(Node::File(3)));
        assert_eq!(local(Path::new(r"\\?\c:\x")), Some(PathBuf::from(r"C:\x")));
    }

    /// Windows: a real symlink to a share is left out (creating one needs
    /// Developer Mode or the privilege; the test passes over it without).
    #[cfg(windows)]
    #[test]
    fn a_windows_link_to_a_share_is_left_out() {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let dir = fixture("fs-win-unc");
        if symlink_dir(r"\\pdfpundit-no-such-host\share", dir.join("share")).is_err() {
            return;
        }
        symlink_file(r"\\pdfpundit-no-such-host\share\x.pdf", dir.join("x.pdf")).unwrap();
        symlink_file(r"sub\deep.pdf", dir.join("near.pdf")).unwrap();
        fs::write(dir.join(r"sub\deep.pdf"), b"deep!").unwrap();
        let mut seen = Seen(Vec::new());
        let got = list_with(dir.path(), Filter::PdfOnly, false, &mut seen).unwrap();
        assert_eq!(
            names(&got),
            [
                ("sub".into(), true),
                ("a.pdf".into(), false),
                ("b.PDF".into(), false),
                ("near.pdf".into(), false),
            ]
        );
        assert!(seen.0.iter().all(|p| p.starts_with(dir.path())));
    }

    /// Windows: junctions (no privilege needed) follow the symlink rule.
    #[cfg(windows)]
    #[test]
    fn junctions_are_followed_only_inside_the_tree() {
        let outside = ScratchDir::new("fs-win-outside");
        fs::write(outside.join("far.pdf"), b"far").unwrap();
        let dir = fixture("fs-win-junctions");
        fs::write(dir.join(r"sub\deep.pdf"), b"deep!").unwrap();
        let junction = |name: &str, target: &Path| {
            let made = std::process::Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(dir.join(name))
                .arg(target)
                .output()
                .unwrap();
            assert!(made.status.success(), "mklink /J {name}");
        };
        junction("into-sub", &dir.join("sub"));
        junction("away", outside.path());
        junction("self", dir.path());
        let got = list(dir.path(), Filter::PdfOnly, false).unwrap();
        assert_eq!(
            names(&got),
            [
                ("into-sub".into(), true),
                ("sub".into(), true),
                ("a.pdf".into(), false),
                ("b.PDF".into(), false),
            ]
        );
        let via = list(&dir.join("into-sub"), Filter::PdfOnly, false).unwrap();
        assert_eq!(names(&via), [("deep.pdf".into(), false)]);
    }

    /// Windows: the hidden attribute hides an entry as a dot name does.
    #[cfg(windows)]
    #[test]
    fn the_hidden_attribute_hides_on_windows() {
        let dir = fixture("fs-win-hidden");
        fs::write(dir.join("veiled.pdf"), b"x").unwrap();
        let made = std::process::Command::new("attrib")
            .arg("+H")
            .arg(dir.join("veiled.pdf"))
            .output()
            .unwrap();
        assert!(made.status.success());
        let shown = |hidden| names(&list(dir.path(), Filter::PdfOnly, hidden).unwrap());
        assert!(!shown(false).contains(&("veiled.pdf".into(), false)));
        assert!(shown(true).contains(&("veiled.pdf".into(), false)));
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
            ((1 << 40) - 1, "1023.9 GiB"),
            (1 << 40, "1.0 TiB"),
            (3 << 50, "3.0 PiB"),
            (1 << 60, "1.0 EiB"),
            (u64::MAX, "15.9 EiB"),
        ] {
            assert_eq!(size_label(bytes), want, "{bytes}");
        }
        // Every label fits the picker's 10-cell column.
        for shift in 0..64 {
            for bytes in [(1u64 << shift) - 1, 1 << shift] {
                assert!(size_label(bytes).chars().count() <= 10, "{bytes}");
            }
        }
    }
}
