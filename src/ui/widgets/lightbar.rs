//! The light bar and the hotkey labels: generate.py's `hk`, the full layout's
//! hotkeys row and its `statusbar`, the bottom row in the theme's lightbar
//! colour (`b`) with the app's name and state on the left and the theme, the
//! size and the clock on the right.

use crate::ui::canvas::{Canvas, plain_len};
use crate::ui::strings;
use crate::ui::theme::Theme;

/// A hotkey label in markup: `[K] label`, the brackets dim, the key in the
/// hotkey colour.
pub fn hk(key: char, label: &str) -> String {
    format!("{{D}}[{{Y}}{key}{{D}}]{{w}} {label}")
}

/// The hotkeys row: each `(key, label)` two cells apart, from `(x, y)`.
pub fn hotkeys(c: &mut Canvas, x: i32, y: i32, keys: &[(char, &str)], theme: &Theme) {
    let row: Vec<String> = keys.iter().map(|&(k, l)| hk(k, l)).collect();
    c.rich(x, y, &format!(" {}", row.join("  ")), None, theme);
}

/// The status bar on row `y`, `w` cells wide: the app's name and version,
/// then each of `parts` (markup) behind a `│`; `right` (markup) against the
/// right edge, drawn last so it stays whole when the two meet.
pub fn status_bar(c: &mut Canvas, y: i32, w: i32, parts: &[String], right: &str, theme: &Theme) {
    let bar = theme.roles.lightbar;
    c.fill(0, y, w, 1, theme.roles.body, bar);
    let mut left = format!(" {{Y}}{}{{W}} {} ", strings::APP_NAME, strings::VERSION);
    for p in parts {
        left.push_str(&format!("{{c}}│{{W}} {p} "));
    }
    c.rich(0, y, &left, Some(bar), theme);
    let right = format!("{right} ");
    let x = w - i32::try_from(plain_len(&right)).unwrap_or(w);
    c.rich(x, y, &right, Some(bar), theme);
}
