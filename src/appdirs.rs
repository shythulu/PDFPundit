//! Config, data, cache and Documents directories on std plus windows-sys (D-046):
//! the paths `ProjectDirs::from("dev", "shythulu", "PDFPundit")` gives, without
//! the `directories` crate and its MPL-2.0 dependency (goal-r2-fr1).
//!
//! - macOS: `~/Library/Application Support/dev.shythulu.PDFPundit` (config and
//!   data), `~/Library/Caches/dev.shythulu.PDFPundit`, `~/Documents`.
//! - Linux and other Unix: `$XDG_CONFIG_HOME/pdfpundit`, `$XDG_DATA_HOME/pdfpundit`,
//!   `$XDG_CACHE_HOME/pdfpundit` (absolute values only; a relative or empty one
//!   falls back to `~/.config`, `~/.local/share`, `~/.cache`), and Documents from
//!   `user-dirs.dirs`, read as bytes.
//! - Windows: `FOLDERID_RoamingAppData\shythulu\PDFPundit\{config,data}`,
//!   `FOLDERID_LocalAppData\shythulu\PDFPundit\cache`, `FOLDERID_Documents`.
//!
//! The environment variables read are the ones plan §3.1 names: `HOME` (or
//! `USERPROFILE`) and the three `XDG_*_HOME`, through [`home`] and `xdg_dirs`.
//! They only locate the config and history, never change engine output.

use std::path::PathBuf;
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
use std::{ffi::OsString, path::Path};

/// Where PDFPundit keeps its files. `documents_dir` is the user's Documents
/// folder, `None` when the platform does not name one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub documents_dir: Option<PathBuf>,
}

/// The `ProjectDirs` qualifier, organisation and application.
#[cfg(any(test, windows))]
const ORG: &str = "shythulu";
#[cfg(any(test, windows))]
const APP: &str = "PDFPundit";
/// macOS bundle id: qualifier, organisation, application joined by dots.
#[cfg(any(test, target_os = "macos"))]
const BUNDLE: &str = "dev.shythulu.PDFPundit";
/// The XDG directory name: the application name in lower case.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
const XDG_NAME: &str = "pdfpundit";

impl AppDirs {
    /// This user's directories, or `None` when there is no home directory (or,
    /// on Windows, a known folder cannot be resolved).
    pub fn resolve() -> Option<AppDirs> {
        #[cfg(target_os = "macos")]
        {
            home().map(|h| macos_dirs(&h))
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let home = home()?;
            Some(xdg_dirs(&home, &|k| std::env::var_os(k), &|p| {
                std::fs::read(p).ok()
            }))
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::UI::Shell::{
                FOLDERID_Documents, FOLDERID_LocalAppData, FOLDERID_RoamingAppData,
            };
            let roaming = known_folder(&FOLDERID_RoamingAppData)?;
            let local = known_folder(&FOLDERID_LocalAppData)?;
            Some(windows_dirs(
                roaming,
                local,
                known_folder(&FOLDERID_Documents),
            ))
        }
        #[cfg(not(any(unix, windows)))]
        {
            None
        }
    }
}

/// The home directory (`HOME`, or `USERPROFILE` on Windows), if it is
/// absolute. The one place outside the known-folder calls that reads it.
pub(crate) fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|h| h.is_absolute())
}

#[cfg(any(test, target_os = "macos"))]
fn macos_dirs(home: &std::path::Path) -> AppDirs {
    let support = home.join("Library/Application Support").join(BUNDLE);
    AppDirs {
        config_dir: support.clone(),
        data_dir: support,
        cache_dir: home.join("Library/Caches").join(BUNDLE),
        documents_dir: Some(home.join("Documents")),
    }
}

/// The XDG layout. `env` reads a variable and `read` a file, so the tests can
/// supply both.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
fn xdg_dirs(
    home: &Path,
    env: &dyn Fn(&str) -> Option<OsString>,
    read: &dyn Fn(&Path) -> Option<Vec<u8>>,
) -> AppDirs {
    // A value counts only if it is an absolute path (XDG base-dir spec).
    let base = |var: &str, fallback: &str| {
        env(var)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(fallback))
    };
    let config_home = base("XDG_CONFIG_HOME", ".config");
    let documents_dir = read(&config_home.join("user-dirs.dirs"))
        .and_then(|text| user_dir(home, &text, "DOCUMENTS"));
    AppDirs {
        config_dir: config_home.join(XDG_NAME),
        data_dir: base("XDG_DATA_HOME", ".local/share").join(XDG_NAME),
        cache_dir: base("XDG_CACHE_HOME", ".cache").join(XDG_NAME),
        documents_dir,
    }
}

/// `XDG_<name>_DIR` from the bytes of a `user-dirs.dirs` file.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
fn user_dir(home: &Path, text: &[u8], name: &str) -> Option<PathBuf> {
    let key = format!("XDG_{name}_DIR");
    // The file is sourced by a shell, so the last assignment wins.
    let value = text
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let (k, v) = split_once(line.trim_ascii(), b'=')?;
            (k.trim_ascii() == key.as_bytes()).then(|| v.trim_ascii())
        })
        .next_back()?;
    let quoted = value.strip_prefix(b"\"")?.strip_suffix(b"\"")?;
    // The prefix is checked on the raw bytes: an escaped `\$HOME` is a
    // literal (relative) `$HOME`, not the home directory.
    if let Some(rest) = quoted.strip_prefix(b"$HOME") {
        // `$HOME` or `$HOME/` alone means the directory is disabled.
        let rest = unescape(rest.strip_prefix(b"/")?);
        if rest.is_empty() {
            return None;
        }
        Some(home.join(path_from_bytes(rest)?))
    } else if quoted.starts_with(b"/") {
        path_from_bytes(unescape(quoted))
    } else {
        None
    }
}

#[cfg(all(unix, any(test, not(target_os = "macos"))))]
fn split_once(line: &[u8], at: u8) -> Option<(&[u8], &[u8])> {
    let i = line.iter().position(|&b| b == at)?;
    Some((&line[..i], &line[i + 1..]))
}

/// Shell double-quote escapes: a backslash before `"`, `\\`, `$` or `` ` ``
/// stands for that byte; any other backslash is kept.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
fn unescape(quoted: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(quoted.len());
    let mut bytes = quoted.iter().copied().peekable();
    while let Some(b) = bytes.next() {
        match (b, bytes.peek()) {
            (b'\\', Some(&next)) if matches!(next, b'"' | b'\\' | b'$' | b'`') => {
                out.push(next);
                bytes.next();
            }
            _ => out.push(b),
        }
    }
    out
}

/// The bytes as a path, as they are.
#[cfg(all(unix, any(test, not(target_os = "macos"))))]
fn path_from_bytes(bytes: Vec<u8>) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(any(test, windows))]
fn windows_dirs(roaming: PathBuf, local: PathBuf, documents: Option<PathBuf>) -> AppDirs {
    let project = roaming.join(ORG).join(APP);
    AppDirs {
        config_dir: project.join("config"),
        data_dir: project.join("data"),
        cache_dir: local.join(ORG).join(APP).join("cache"),
        documents_dir: documents,
    }
}

/// One `SHGetKnownFolderPath` call. The returned buffer is NUL-terminated
/// UTF-16, measured by a scan (no `lstrlenW`) and freed with `CoTaskMemFree`,
/// which the API requires even when the call fails.
#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;

    let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: `id` is a valid GUID and `raw` a valid out-pointer; on success
    // `raw` points to a NUL-terminated wide string that we only read up to the
    // NUL before freeing it once, and freeing a null pointer is a no-op.
    unsafe {
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut raw);
        let path = (hr == 0 && !raw.is_null()).then(|| {
            let mut len = 0usize;
            while *raw.add(len) != 0 {
                len += 1;
            }
            let wide = std::slice::from_raw_parts(raw, len);
            PathBuf::from(std::ffi::OsString::from_wide(wide))
        });
        CoTaskMemFree(raw as *const core::ffi::c_void);
        path
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// The XDG layout is only used on Unix outside macOS, and its inputs are
    /// Unix paths (`/cfg` has no drive, so it is not absolute on Windows).
    #[cfg(unix)]
    mod xdg {
        use std::ffi::OsString;
        use std::path::{Path, PathBuf};

        use super::super::{user_dir, xdg_dirs};

        fn env(
            pairs: &'static [(&'static str, &'static str)],
        ) -> impl Fn(&str) -> Option<OsString> {
            move |k| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == k)
                    .map(|(_, v)| OsString::from(*v))
            }
        }

        fn no_file(_: &Path) -> Option<Vec<u8>> {
            None
        }

        #[test]
        fn xdg_defaults_under_home() {
            let d = xdg_dirs(Path::new("/home/ana"), &env(&[]), &no_file);
            assert_eq!(d.config_dir, PathBuf::from("/home/ana/.config/pdfpundit"));
            assert_eq!(
                d.data_dir,
                PathBuf::from("/home/ana/.local/share/pdfpundit")
            );
            assert_eq!(d.cache_dir, PathBuf::from("/home/ana/.cache/pdfpundit"));
            assert_eq!(d.documents_dir, None);
        }

        #[test]
        fn xdg_absolute_config_home_is_honoured() {
            let e = env(&[
                ("XDG_CONFIG_HOME", "/cfg"),
                ("XDG_DATA_HOME", "/data"),
                ("XDG_CACHE_HOME", "/cache"),
            ]);
            let d = xdg_dirs(Path::new("/home/ana"), &e, &no_file);
            assert_eq!(d.config_dir, PathBuf::from("/cfg/pdfpundit"));
            assert_eq!(d.data_dir, PathBuf::from("/data/pdfpundit"));
            assert_eq!(d.cache_dir, PathBuf::from("/cache/pdfpundit"));
        }

        #[test]
        fn xdg_relative_or_empty_cache_home_is_ignored() {
            let d = xdg_dirs(
                Path::new("/home/ana"),
                &env(&[("XDG_CACHE_HOME", "rel/cache"), ("XDG_DATA_HOME", "")]),
                &no_file,
            );
            assert_eq!(d.cache_dir, PathBuf::from("/home/ana/.cache/pdfpundit"));
            assert_eq!(
                d.data_dir,
                PathBuf::from("/home/ana/.local/share/pdfpundit")
            );
        }

        #[test]
        fn xdg_documents_dir_comes_from_the_fixture_user_dirs_file() {
            let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/appdirs");
            let e = move |k: &str| (k == "XDG_CONFIG_HOME").then(|| OsString::from(fixture));
            let read = |p: &Path| std::fs::read(p).ok();
            let d = xdg_dirs(Path::new("/home/ana"), &e, &read);
            assert_eq!(
                d.documents_dir,
                Some(PathBuf::from("/home/ana/Case Papers"))
            );
            assert_eq!(d.config_dir, Path::new(fixture).join("pdfpundit"));
        }

        #[test]
        fn xdg_user_dirs_file_defaults_to_dot_config() {
            let read = |p: &Path| {
                (p == Path::new("/home/ana/.config/user-dirs.dirs"))
                    .then(|| b"XDG_DOCUMENTS_DIR=\"$HOME/Docs\"\n".to_vec())
            };
            let d = xdg_dirs(Path::new("/home/ana"), &env(&[]), &read);
            assert_eq!(d.documents_dir, Some(PathBuf::from("/home/ana/Docs")));
        }

        #[test]
        fn user_dirs_lines() {
            let h = Path::new("/h");
            let text = b"# written by xdg-user-dirs-update\n\
            XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n\
            XDG_DOCUMENTS_DIR=\"$HOME/Docs \\\"x\\\" \\$y \\\\z\"\n";
            assert_eq!(
                user_dir(h, text, "DOCUMENTS"),
                Some(PathBuf::from("/h/Docs \"x\" $y \\z"))
            );
            assert_eq!(user_dir(h, text, "MUSIC"), None);
            let one = |line: &[u8]| user_dir(h, line, "DOCUMENTS");
            assert_eq!(
                one(b"XDG_DOCUMENTS_DIR=\"/abs/d\""),
                Some(PathBuf::from("/abs/d"))
            );
            // `$HOME/` alone means the directory is disabled.
            assert_eq!(one(b"XDG_DOCUMENTS_DIR=\"$HOME/\""), None);
            assert_eq!(one(b"XDG_DOCUMENTS_DIR=\"$HOME\""), None);
            assert_eq!(one(b"XDG_DOCUMENTS_DIR=\"rel/d\""), None);
            // A shell reads `\$HOME/x` as the relative path `$HOME/x`.
            assert_eq!(one(b"XDG_DOCUMENTS_DIR=\"\\$HOME/x\""), None);
            assert_eq!(one(b"XDG_DOCUMENTS_DIR=$HOME/unquoted"), None);
            assert_eq!(one(b"#XDG_DOCUMENTS_DIR=\"$HOME/c\""), None);
            // The file is sourced by a shell, so the last assignment wins.
            assert_eq!(
                one(b"XDG_DOCUMENTS_DIR=\"$HOME/a\"\r\n  XDG_DOCUMENTS_DIR = \"$HOME/b\"  \n"),
                Some(PathBuf::from("/h/b"))
            );
        }

        #[test]
        fn user_dirs_keeps_non_utf8_bytes() {
            use std::os::unix::ffi::OsStrExt;
            let d = user_dir(
                Path::new("/h"),
                b"XDG_DOCUMENTS_DIR=\"$HOME/Pap\xffiers\"\n",
                "DOCUMENTS",
            )
            .expect("a path");
            assert_eq!(d.as_os_str().as_bytes(), b"/h/Pap\xffiers");
        }
    }

    #[test]
    fn macos_paths() {
        let d = macos_dirs(Path::new("/Users/ana"));
        let support = "/Users/ana/Library/Application Support/dev.shythulu.PDFPundit";
        assert_eq!(d.config_dir, PathBuf::from(support));
        assert_eq!(d.data_dir, PathBuf::from(support));
        assert_eq!(
            d.cache_dir,
            PathBuf::from("/Users/ana/Library/Caches/dev.shythulu.PDFPundit")
        );
        assert_eq!(d.documents_dir, Some(PathBuf::from("/Users/ana/Documents")));
    }

    #[test]
    fn windows_paths() {
        let roaming = PathBuf::from("R");
        let local = PathBuf::from("L");
        let docs = Some(PathBuf::from("D"));
        let d = windows_dirs(roaming.clone(), local.clone(), docs.clone());
        let project = |base: &Path, leaf: &str| base.join("shythulu").join("PDFPundit").join(leaf);
        assert_eq!(d.config_dir, project(&roaming, "config"));
        assert_eq!(d.data_dir, project(&roaming, "data"));
        assert_eq!(d.cache_dir, project(&local, "cache"));
        assert_eq!(d.documents_dir, docs);
    }

    /// Runs the host's real branch: on the windows CI leg this is the first
    /// execution of the `SHGetKnownFolderPath` call (D-058).
    #[test]
    fn resolve_gives_this_hosts_project_dirs() {
        let d = AppDirs::resolve().expect("a home directory");
        for dir in [&d.config_dir, &d.data_dir, &d.cache_dir] {
            assert!(dir.is_absolute(), "{dir:?}");
        }
        if cfg!(target_os = "macos") {
            assert!(
                d.config_dir
                    .ends_with("Library/Application Support/dev.shythulu.PDFPundit")
            );
            assert!(
                d.cache_dir
                    .ends_with("Library/Caches/dev.shythulu.PDFPundit")
            );
        } else if cfg!(windows) {
            assert!(d.config_dir.ends_with("shythulu/PDFPundit/config"));
            assert!(d.data_dir.ends_with("shythulu/PDFPundit/data"));
            assert!(d.cache_dir.ends_with("shythulu/PDFPundit/cache"));
            assert!(d.documents_dir.is_some());
        } else {
            for dir in [&d.config_dir, &d.data_dir, &d.cache_dir] {
                assert!(dir.ends_with("pdfpundit"), "{dir:?}");
            }
        }
    }
}
