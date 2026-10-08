//! The drop gate (T-23b, D-049 amended): what may go into the cat, from any
//! source (a paste, the Windows collector, an OSC 72 drop, the picker).
//!
//! | check | outcome |
//! |---|---|
//! | the name ends in `.pdf` (any case) | else refused |
//! | not a UNC path (`\\server\share`; Windows only) | else refused: another machine |
//! | a regular file that opens and reads | else refused |
//! | at most 4 GiB | else refused: the writer cannot emit above it (T-12a rule 7) |
//! | above `warn_above` (512 MiB) | accepted, with a warning |
//!
//! The `%PDF-` sniff is not a check. A header overwritten by corruption (C1)
//! is a reason to come in, so the sniff is only recorded, as `drop_sniff`,
//! for the debug log. The file is opened read-only, and only its first KiB is
//! read.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf, Prefix};

use crate::pdf::write::MAX_OUTPUT_BYTES;
use crate::ui::strings;

use super::paste::is_pdf;

/// How far into the file the sniff looks for `%PDF-`: where readers look for
/// the header.
const SNIFF_WINDOW: u64 = 1024;

/// A file the gate let through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admitted {
    /// The path, made absolute against the working directory.
    pub path: PathBuf,
    pub bytes: u64,
    /// `%PDF-` is in the first KiB. Evidence only (D-049).
    pub drop_sniff: bool,
    /// Above the warning size: held in memory while it is worked on.
    pub big: bool,
}

/// Lets `path` in, or says why not as the hint row says it.
pub fn gate(path: &Path, warn_above: u64) -> Result<Admitted, &'static str> {
    if !is_pdf(path) {
        return Err(strings::DROP_NOT_A_PDF);
    }
    if is_unc(path) {
        return Err(strings::DROP_NOT_LOCAL);
    }
    let meta = fs::metadata(path).map_err(|_| strings::DROP_UNREADABLE)?;
    if !meta.is_file() {
        return Err(strings::DROP_UNREADABLE);
    }
    let bytes = meta.len();
    if bytes > MAX_OUTPUT_BYTES {
        return Err(strings::DROP_TOO_BIG);
    }
    let mut head = Vec::new();
    File::open(path)
        .and_then(|f| f.take(SNIFF_WINDOW).read_to_end(&mut head))
        .map_err(|_| strings::DROP_UNREADABLE)?;
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    Ok(Admitted {
        path,
        bytes,
        drop_sniff: memchr::memmem::find(&head, b"%PDF-").is_some(),
        big: bytes > warn_above,
    })
}

/// Whether `path` names a share on another machine. Checked before any file
/// call: opening one reaches out over SMB, which can hand the user's
/// credentials to the server. Paths have prefixes only on Windows, so this is
/// never true elsewhere. The paste parser refuses these first (`paste.rs`);
/// this covers every other source.
fn is_unc(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(p)) => {
            matches!(p.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::engine::{AnalyzeOptions, Engine, NullProgress, Pdfpundit};
    use crate::pdf::carver::carve;
    use crate::pdf::diagnose::diagnose;
    use crate::pdf::fixtures;
    use crate::pdf::graph::ObjectGraph;
    use crate::pdf::model::{CorruptionClass, FindingKind};
    use crate::pdf::streams::salvage::SalvageIndex;
    use crate::place::ScratchDir;

    fn warn_above() -> u64 {
        AnalyzeOptions::default().max_file_bytes
    }

    #[test]
    fn a_pdf_is_let_in_whatever_the_case_of_its_extension() {
        let dir = ScratchDir::new("gate-case");
        for name in ["a.pdf", "b.PDF", "c.Pdf"] {
            let path = dir.join(name);
            fs::write(&path, fixtures::golden_pdf()).unwrap();
            let a = gate(&path, warn_above()).expect(name);
            assert_eq!(a.path, path);
            assert!(a.drop_sniff && !a.big, "{name}");
            assert_eq!(a.bytes, fixtures::golden_pdf().len() as u64);
        }
    }

    #[test]
    fn a_c1_pdf_is_let_in_and_its_analysis_finds_c1() {
        let dir = ScratchDir::new("gate-c1");
        let path = dir.join("header-gone.pdf");
        let c1 = fixtures::corrupt(CorruptionClass::C1Header, &fixtures::golden_pdf(), 7);
        assert!(!c1.starts_with(b"%PDF-"));
        fs::write(&path, &c1).unwrap();

        let a = gate(&path, warn_above()).expect("C1 reaches the engine");
        assert!(!a.drop_sniff, "the sniff is recorded, not a gate");

        // The admitted path goes through the engine facade, as a drop does.
        let bytes = fs::read(&a.path).unwrap();
        let analysis = Pdfpundit
            .analyze(&bytes, &AnalyzeOptions::default(), &mut NullProgress)
            .expect("not cancelled");
        assert_eq!(analysis.stats.bytes, a.bytes);
        // `Pdfpundit::analyze` reports no findings until T-14 wires the
        // pipeline in, so the C1 finding is checked on the same steps it
        // will run: carve, graph, diagnose.
        let carved = carve(&bytes, &|| false).expect("not cancelled");
        let graph = ObjectGraph::from_carve(&carved);
        let found = diagnose(&bytes, &carved, &graph, &SalvageIndex::default());
        assert!(
            found
                .iter()
                .any(|f| f.class == FindingKind::Corruption(CorruptionClass::C1Header)),
            "{found:#?}"
        );
    }

    #[test]
    fn a_txt_with_a_pdf_header_is_refused() {
        let dir = ScratchDir::new("gate-txt");
        let path = dir.join("looks-like.txt");
        fs::write(&path, fixtures::golden_pdf()).unwrap();
        assert_eq!(gate(&path, warn_above()), Err(strings::DROP_NOT_A_PDF));
    }

    #[test]
    fn a_missing_file_or_a_directory_is_refused() {
        let dir = ScratchDir::new("gate-missing");
        assert_eq!(
            gate(&dir.join("gone.pdf"), warn_above()),
            Err(strings::DROP_UNREADABLE)
        );
        let sub = dir.join("folder.pdf");
        fs::create_dir(&sub).unwrap();
        assert_eq!(gate(&sub, warn_above()), Err(strings::DROP_UNREADABLE));
    }

    #[test]
    fn a_unc_path_is_refused_before_it_is_opened_on_windows() {
        let got = gate(Path::new(r"\\server\share\x.pdf"), warn_above());
        if cfg!(windows) {
            assert_eq!(got, Err(strings::DROP_NOT_LOCAL));
            assert_eq!(
                gate(Path::new(r"\\?\UNC\server\share\x.pdf"), warn_above()),
                Err(strings::DROP_NOT_LOCAL)
            );
        } else {
            // A relative name with backslashes in it, which is not there.
            assert_eq!(got, Err(strings::DROP_UNREADABLE));
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_pdf_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = ScratchDir::new("gate-perm");
        let path = dir.join("locked.pdf");
        fs::write(&path, b"%PDF-1.4").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        // root reads anything; the check only means something for a user.
        if File::open(&path).is_err() {
            assert_eq!(gate(&path, warn_above()), Err(strings::DROP_UNREADABLE));
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// A file of `len` bytes that starts with a header: sparse on Unix,
    /// allocated on Windows.
    fn sparse(dir: &ScratchDir, name: &str, len: u64) -> PathBuf {
        let path = dir.join(name);
        let f = File::create(&path).unwrap();
        (&f).write_all(b"%PDF-1.7\n").unwrap();
        f.set_len(len).unwrap();
        path
    }

    /// Runs on every leg, as the ticket asks. On Windows the files are not
    /// sparse (NTFS allocates what `set_len` asks for), so this needs about
    /// 5.6 GiB of free temporary disk there, which a hosted runner has; the
    /// files are gone when the test ends.
    #[test]
    fn five_gib_is_refused_and_six_hundred_mib_is_let_in_with_a_warning() {
        let dir = ScratchDir::new("gate-size");
        let huge = sparse(&dir, "huge.pdf", 5 << 30);
        assert_eq!(gate(&huge, warn_above()), Err(strings::DROP_TOO_BIG));

        let big = sparse(&dir, "big.pdf", 600 << 20);
        let a = gate(&big, warn_above()).expect("600 MiB comes in");
        assert!(a.big && a.drop_sniff);
        assert_eq!(a.bytes, 600 << 20);

        // The warning follows the size it is given.
        let small = sparse(&dir, "small.pdf", 2048);
        assert!(!gate(&small, warn_above()).unwrap().big);
        assert!(gate(&small, 1024).unwrap().big);
    }

    /// Unix only: on Windows these two would be another 8 GiB of allocated
    /// disk on top of the test above, which the ticket does not ask for.
    #[cfg(unix)]
    #[test]
    fn the_ceiling_itself_is_let_in() {
        let dir = ScratchDir::new("gate-edge");
        let edge = sparse(&dir, "edge.pdf", MAX_OUTPUT_BYTES);
        assert!(gate(&edge, warn_above()).is_ok());
        let past = sparse(&dir, "past.pdf", MAX_OUTPUT_BYTES + 1);
        assert_eq!(gate(&past, warn_above()), Err(strings::DROP_TOO_BIG));
    }
}
