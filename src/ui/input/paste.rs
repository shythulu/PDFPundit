//! Path candidates from pasted text (T-23b): a bracketed paste on Unix, a
//! burst of typed characters on Windows (`collector.rs`), and a dropped
//! file's paste from a terminal that quotes it.
//!
//! Items are separated by whitespace (spaces, tabs, LF, CRLF). An item may be
//! quoted or escaped, and may be a `file://` URI:
//!
//! | text | Unix | Windows |
//! |---|---|---|
//! | `'a b.pdf'` | quoted, nothing escaped inside | `'` is a name character |
//! | `"a b.pdf"` | quoted; `\` escapes `"`, `\`, `$` and `` ` `` | quoted |
//! | `a\ b.pdf` | `\` escapes the next character | `\` is the path separator |
//! | `file:///x/a%20b.pdf` | percent-decoded; host empty or `localhost` | the same, `/C:/` read as `C:/` |
//! | `\\server\share\a.pdf` | `\` escapes, as above | refused: another machine |
//!
//! Only this machine's files go in. A `file:` URI with another host is
//! refused, and so, on Windows, is a UNC path (`\\server\share`, or
//! `file:////server/share`): opening one reaches out over SMB, which an
//! offline tool must not do, and which can hand the user's credentials to
//! the server. `\\?\C:\…` and `\\.\C:\…` name a local drive and come in.
//!
//! The text is untrusted. A candidate whose path holds U+FFFD is refused: the
//! terminal's bytes were not UTF-8 (crossterm decodes a paste lossily), so the
//! path on disk cannot be recovered from it.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

use crate::ui::strings;

/// One whitespace-separated item of a paste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathCandidate {
    /// The item as pasted, quotes and escapes included.
    pub raw: String,
    /// The path it names; `None` when it is refused.
    pub path: Option<PathBuf>,
    /// Why it is refused, as the hint row says it; `None` when `path` is set.
    pub reason: Option<&'static str>,
}

/// Every item of `text`, in order, each with its path or the reason it has
/// none. Items that come out empty (`''`) are skipped.
pub fn paths_from_paste(text: &str) -> Vec<PathCandidate> {
    parse(text, Style::NATIVE)
}

/// Whose quoting rules a paste follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Style {
    /// POSIX shell quoting: `'…'`, `"…"` and `\`.
    Posix,
    /// `"…"` only; `\` and `'` are path characters. Built elsewhere for
    /// its tests.
    #[cfg_attr(not(windows), allow(dead_code))]
    Windows,
}

impl Style {
    #[cfg(not(windows))]
    pub(crate) const NATIVE: Style = Style::Posix;
    #[cfg(windows)]
    pub(crate) const NATIVE: Style = Style::Windows;
}

/// [`paths_from_paste`] under `style`'s quoting.
pub(crate) fn parse(text: &str, style: Style) -> Vec<PathCandidate> {
    split(text, style)
        .into_iter()
        .map(|(raw, word)| candidate(raw, &word, style))
        .collect()
}

/// The items of `text`: each one's raw text and its unquoted, unescaped
/// value.
fn split(text: &str, style: Style) -> Vec<(String, String)> {
    let mut items = Vec::new();
    let mut raw = String::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if !raw.is_empty() {
                    items.push((std::mem::take(&mut raw), std::mem::take(&mut word)));
                }
                continue;
            }
            '\'' if style == Style::Posix => {
                raw.push(c);
                for q in chars.by_ref() {
                    raw.push(q);
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                raw.push(c);
                while let Some(q) = chars.next() {
                    raw.push(q);
                    match q {
                        '"' => break,
                        '\\' if style == Style::Posix => match chars.peek().copied() {
                            Some(e @ ('"' | '\\' | '$' | '`')) => {
                                raw.push(e);
                                word.push(e);
                                chars.next();
                            }
                            // A quoted line continuation is removed.
                            _ if continuation(&mut chars, &mut raw) => {}
                            _ => word.push(q),
                        },
                        _ => word.push(q),
                    }
                }
            }
            '\\' if style == Style::Posix => {
                raw.push(c);
                if continuation(&mut chars, &mut raw) {
                    // A line continuation joins the lines.
                    continue;
                }
                if let Some(e) = chars.next() {
                    raw.push(e);
                    word.push(e);
                }
            }
            _ => {
                raw.push(c);
                word.push(c);
            }
        }
    }
    if !raw.is_empty() {
        items.push((raw, word));
    }
    items.retain(|(_, word)| !word.is_empty());
    items
}

/// After a `\`, consumes a line break, LF or CRLF, into `raw` and says
/// whether there was one. A lone CR is not a line break.
fn continuation(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, raw: &mut String) -> bool {
    let mut ahead = chars.clone();
    let taken = match (ahead.next(), ahead.next()) {
        (Some('\n'), _) => "\n",
        (Some('\r'), Some('\n')) => "\r\n",
        _ => return false,
    };
    raw.push_str(taken);
    for _ in taken.chars() {
        chars.next();
    }
    true
}

/// The candidate for one item.
fn candidate(raw: String, word: &str, style: Style) -> PathCandidate {
    let path = if has_scheme(word, "file:") {
        from_file_uri(word, style)
    } else if word.contains('\u{FFFD}') {
        Err(strings::DROP_GARBLED)
    } else if style == Style::Windows && is_unc(word) {
        Err(strings::DROP_NOT_LOCAL)
    } else {
        Ok(PathBuf::from(word))
    };
    let path = path.and_then(|p| {
        if is_pdf(&p) {
            Ok(p)
        } else {
            Err(strings::DROP_NOT_A_PDF)
        }
    });
    match path {
        Ok(p) => PathCandidate {
            raw,
            path: Some(p),
            reason: None,
        },
        Err(why) => PathCandidate {
            raw,
            path: None,
            reason: Some(why),
        },
    }
}

/// Whether `path` ends in `.pdf`, in any case (D-049).
pub(crate) fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

fn has_scheme(word: &str, scheme: &str) -> bool {
    word.get(..scheme.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
}

/// The local path a `file:` URI names (RFC 8089): `file:///p`,
/// `file://localhost/p` or `file:/p`. Any other host is refused: only this
/// machine's files go in.
fn from_file_uri(uri: &str, style: Style) -> Result<PathBuf, &'static str> {
    let rest = &uri["file:".len()..];
    let path = match rest.strip_prefix("//") {
        Some(authority) => {
            let slash = authority.find('/').unwrap_or(authority.len());
            let host = &authority[..slash];
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return Err(strings::DROP_NOT_LOCAL);
            }
            &authority[slash..]
        }
        None => rest,
    };
    if !path.starts_with('/') {
        return Err(strings::DROP_GARBLED);
    }
    let bytes: Vec<u8> = percent_decode_str(path).collect();
    if bytes.contains(&0) {
        return Err(strings::DROP_GARBLED);
    }
    // U+FFFD, encoded or decoded: the URI's producer lost the name's bytes.
    if path.contains('\u{FFFD}') || bytes.windows(3).any(|w| w == "\u{FFFD}".as_bytes()) {
        return Err(strings::DROP_GARBLED);
    }
    match style {
        Style::Posix => Ok(posix_path(bytes)),
        Style::Windows => {
            let s = String::from_utf8(bytes).map_err(|_| strings::DROP_GARBLED)?;
            // `file:////server/share`: a UNC path in a local URI.
            if is_unc(&s) {
                return Err(strings::DROP_NOT_LOCAL);
            }
            // `/C:/x` and `/C|/x` are the drive path `C:/x`.
            let b = s.as_bytes();
            let drive = b.len() >= 3
                && b[1].is_ascii_alphabetic()
                && (b[2] == b':' || b[2] == b'|')
                && b.get(3).is_none_or(|&c| c == b'/');
            Ok(PathBuf::from(if drive {
                format!("{}:{}", &s[1..2], &s[3..])
            } else {
                s
            }))
        }
    }
}

/// Whether `path`, read as a Windows path, names another machine: it opens
/// with two separators (`\\` or `/`, in any mix) and is not a local drive's
/// device path (`\\?\C:` or `\\.\C:`).
fn is_unc(path: &str) -> bool {
    let b = path.as_bytes();
    let sep = |i: usize| matches!(b.get(i), Some(b'\\' | b'/'));
    if !(sep(0) && sep(1)) {
        return false;
    }
    let drive = matches!(b.get(2), Some(b'?' | b'.'))
        && sep(3)
        && b.get(4).is_some_and(u8::is_ascii_alphabetic)
        && b.get(5) == Some(&b':')
        && (b.len() == 6 || sep(6));
    !drive
}

/// A decoded URI path as a Unix path: any bytes but NUL are a name.
#[cfg(unix)]
fn posix_path(bytes: Vec<u8>) -> PathBuf {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(bytes))
}

/// Elsewhere the POSIX style runs in tests only; a name that is not UTF-8 is
/// read lossily, which the U+FFFD rule then catches in no case that matters.
#[cfg(not(unix))]
fn posix_path(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(text: &str, style: Style) -> Vec<Option<PathBuf>> {
        parse(text, style).into_iter().map(|c| c.path).collect()
    }

    fn some(p: &str) -> Option<PathBuf> {
        Some(PathBuf::from(p))
    }

    #[test]
    fn spaces_tabs_and_newlines_separate_items() {
        assert_eq!(
            paths("  /a/x.pdf  /b/y.PDF\t/c/z.Pdf  ", Style::Posix),
            [some("/a/x.pdf"), some("/b/y.PDF"), some("/c/z.Pdf")]
        );
        assert_eq!(
            paths("/a/x.pdf\n/b/y.pdf\n\n/c/z.pdf\n", Style::Posix),
            [some("/a/x.pdf"), some("/b/y.pdf"), some("/c/z.pdf")]
        );
        assert!(paths(" \r\n\t ", Style::Posix).is_empty());
        assert!(paths("", Style::Posix).is_empty());
    }

    #[test]
    fn crlf_lines_leave_no_carriage_return() {
        assert_eq!(
            paths("/a/x.pdf\r\n/b/y.pdf\r\n", Style::Posix),
            [some("/a/x.pdf"), some("/b/y.pdf")]
        );
        assert_eq!(
            paths("C:\\a\\x.pdf\r\nC:\\b\\y.pdf", Style::Windows),
            [some("C:\\a\\x.pdf"), some("C:\\b\\y.pdf")]
        );
    }

    #[test]
    fn quotes_keep_spaces() {
        assert_eq!(
            paths("'/a b/x y.pdf' \"/c d/it's.pdf\"", Style::Posix),
            [some("/a b/x y.pdf"), some("/c d/it's.pdf")]
        );
        // Quoting may cover part of an item, as a shell's does.
        assert_eq!(paths("/a' b'/x.pdf", Style::Posix), [some("/a b/x.pdf")]);
        // Inside double quotes, `\` escapes `"` and `\` only.
        assert_eq!(
            paths(r#""/a/say \"hi\" \n.pdf""#, Style::Posix),
            [some(r#"/a/say "hi" \n.pdf"#)]
        );
        // An unclosed quote runs to the end of the paste.
        assert_eq!(paths("'/a b/x.pdf", Style::Posix), [some("/a b/x.pdf")]);
        // An empty item is no item.
        assert_eq!(paths("'' \"\" /x.pdf", Style::Posix), [some("/x.pdf")]);
    }

    #[test]
    fn backslash_escapes_the_next_character_on_unix() {
        assert_eq!(
            paths(r"/a\ b/x\ y.pdf /c/it\'s.pdf", Style::Posix),
            [some("/a b/x y.pdf"), some("/c/it's.pdf")]
        );
        assert_eq!(
            paths("/a/x\\\ny.pdf", Style::Posix),
            [some("/a/xy.pdf")],
            "a line continuation joins"
        );
        assert_eq!(
            paths("/a/x\\\r\ny.pdf \"/b/u\\\r\nv.pdf\"", Style::Posix),
            [some("/a/xy.pdf"), some("/b/uv.pdf")],
            "so does one before CRLF, quoted or not"
        );
    }

    #[test]
    fn windows_quotes_with_double_quotes_and_keeps_backslashes() {
        assert_eq!(
            paths(
                r#""C:\Users\ana\my file.pdf" C:\tmp\it's.pdf"#,
                Style::Windows
            ),
            [some(r"C:\Users\ana\my file.pdf"), some(r"C:\tmp\it's.pdf")]
        );
    }

    #[test]
    fn file_uris_are_percent_decoded() {
        assert_eq!(
            paths(
                "file:///home/ana/my%20file.pdf\r\nfile://localhost/tmp/a%25b.pdf",
                Style::Posix
            ),
            [some("/home/ana/my file.pdf"), some("/tmp/a%b.pdf")]
        );
        assert_eq!(
            paths("FILE://LOCALHOST/x.pdf file:/y.pdf", Style::Posix),
            [some("/x.pdf"), some("/y.pdf")]
        );
        assert_eq!(
            paths("file:///C:/Users/ana/my%20file.pdf", Style::Windows),
            [some("C:/Users/ana/my file.pdf")]
        );
        assert_eq!(
            paths("file:///C|/a.pdf", Style::Windows),
            [some("C:/a.pdf")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_uri_keeps_bytes_that_are_not_utf8_on_unix() {
        use std::os::unix::ffi::OsStrExt;
        let got = paths("file:///tmp/caf%E9.pdf", Style::Posix);
        let path = got[0].as_ref().expect("a path");
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/caf\xE9.pdf");
    }

    #[test]
    fn a_file_uri_on_another_host_or_with_nul_is_refused() {
        let c = &parse("file://evil.example/x.pdf", Style::Posix)[0];
        assert_eq!(c.path, None);
        assert_eq!(c.reason, Some(strings::DROP_NOT_LOCAL));
        let c = &parse("file:///tmp/a%00b.pdf", Style::Posix)[0];
        assert_eq!(
            (c.path.clone(), c.reason),
            (None, Some(strings::DROP_GARBLED))
        );
        let c = &parse("file:relative.pdf", Style::Posix)[0];
        assert_eq!(
            (c.path.clone(), c.reason),
            (None, Some(strings::DROP_GARBLED))
        );
    }

    #[test]
    fn a_unc_path_is_refused_on_windows() {
        for text in [
            r"\\server\share\x.pdf",
            "//server/share/x.pdf",
            r"\/server\share\x.pdf",
            r"\\?\UNC\server\share\x.pdf",
            r#""\\server\my share\x.pdf""#,
            "file:////server/share/x.pdf",
            "file://localhost//server/share/x.pdf",
            "file:///%5C%5Cserver/share/x.pdf",
        ] {
            let got = parse(text, Style::Windows);
            assert_eq!(got.len(), 1, "{text:?}");
            assert_eq!(
                (got[0].path.clone(), got[0].reason),
                (None, Some(strings::DROP_NOT_LOCAL)),
                "{text:?}"
            );
        }
        // A local drive's device path comes in.
        assert_eq!(
            paths(r"\\?\C:\a\x.pdf \\.\D:\y.pdf", Style::Windows),
            [some(r"\\?\C:\a\x.pdf"), some(r"\\.\D:\y.pdf")]
        );
        // On Unix `//x` is a local path.
        assert_eq!(paths("//tmp/x.pdf", Style::Posix), [some("//tmp/x.pdf")]);
    }

    #[test]
    fn non_pdf_items_are_refused() {
        let got = parse(
            "/a/notes.txt /a/x.pdf /a/pdf /a/x.pdf.bak hello",
            Style::Posix,
        );
        let reasons: Vec<_> = got.iter().map(|c| c.reason).collect();
        assert_eq!(
            reasons,
            [
                Some(strings::DROP_NOT_A_PDF),
                None,
                Some(strings::DROP_NOT_A_PDF),
                Some(strings::DROP_NOT_A_PDF),
                Some(strings::DROP_NOT_A_PDF),
            ]
        );
        assert_eq!(got[0].raw, "/a/notes.txt");
        assert_eq!(got[0].path, None);
        assert_eq!(got[1].path, some("/a/x.pdf"));
    }

    #[test]
    fn a_path_with_u_fffd_is_refused_with_a_hint() {
        for text in [
            "/tmp/caf\u{FFFD}.pdf",
            "file:///tmp/caf%EF%BF%BD.pdf",
            "file:///tmp/caf\u{FFFD}.pdf",
        ] {
            let got = parse(text, Style::Posix);
            assert_eq!(got.len(), 1, "{text:?}");
            assert_eq!(got[0].path, None, "{text:?}");
            assert_eq!(got[0].reason, Some(strings::DROP_GARBLED), "{text:?}");
            assert_eq!(got[0].raw, text);
        }
    }

    #[test]
    fn raw_keeps_the_quotes_and_escapes() {
        let got = parse(r"'/a b.pdf' /c\ d.pdf", Style::Posix);
        let raw: Vec<&str> = got.iter().map(|c| c.raw.as_str()).collect();
        assert_eq!(raw, ["'/a b.pdf'", r"/c\ d.pdf"]);
    }

    #[test]
    fn the_native_style_is_the_platforms() {
        assert_eq!(paths_from_paste("/x.pdf"), parse("/x.pdf", Style::NATIVE));
        assert_eq!(Style::NATIVE == Style::Windows, cfg!(windows));
    }
}
