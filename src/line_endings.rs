//! Text a test reads from a file, with CRLF line endings made LF (CI-01).
//!
//! `.gitattributes` checks the repo's text files out with LF on every OS, so a
//! source scan or a committed snapshot reads the same bytes everywhere. A
//! checkout that ignores it (an archive unpacked on Windows, a working tree made
//! before the rule) has CRLF; tests that compare text or search it for a line
//! break pass it through [`lf`] first, so they pass there too.

use std::borrow::Cow;

/// `text` with every `\r\n` replaced by `\n`. A lone `\r` is kept: it is not a
/// line ending any checkout writes.
pub(crate) fn lf(text: &str) -> Cow<'_, str> {
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// `text` as a CRLF checkout would hold it, for the tests that prove a helper
/// reads one.
pub(crate) fn crlf(text: &str) -> String {
    lf(text).replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_becomes_lf_and_nothing_else_changes() {
        assert_eq!(lf("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(lf("a\nb\r\nc"), "a\nb\nc");
        assert_eq!(lf("a\rb\r\r\n"), "a\rb\r\n");
        assert!(matches!(lf("a\nb\n"), Cow::Borrowed("a\nb\n")));
        assert_eq!(lf(""), "");
    }

    #[test]
    fn crlf_round_trips_through_lf() {
        assert_eq!(crlf("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(crlf("a\r\nb\n"), "a\r\nb\r\n");
        assert_eq!(lf(&crlf("x\ny\n\nz")), "x\ny\n\nz");
    }
}
