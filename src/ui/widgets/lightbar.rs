//! The light bar and the hotkey labels: generate.py's `hk`, the full layout's
//! hotkeys row and its `statusbar`, the bottom row in the theme's lightbar
//! colour (`b`) with the app's name and state on the left and the theme, the
//! size and the clock on the right.

use std::fmt::Display;

use crate::ui::canvas::{Canvas, plain_len};
use crate::ui::strings;
use crate::ui::theme::Theme;

/// A hotkey label in markup: `[K] label`, the brackets dim, the key in the
/// hotkey colour. The key is a letter or a word (`enter`, `↑↓`).
pub fn hk(key: impl Display, label: &str) -> String {
    format!("{{D}}[{{Y}}{key}{{D}}]{{w}} {label}")
}

/// The hotkeys row: each `(key, label)` two cells apart, from `(x, y)`.
pub fn hotkeys<K: Display>(c: &mut Canvas, x: i32, y: i32, keys: &[(K, &str)], theme: &Theme) {
    let row: Vec<String> = keys.iter().map(|(k, l)| hk(k, l)).collect();
    c.rich(x, y, &format!(" {}", row.join("  ")), None, theme);
}

/// The status bar on row `y`, `w` cells wide: the app's name and version,
/// then each of `parts` (markup) behind a `│`; `right` (markup) against the
/// right edge, drawn last so it stays whole when the two meet. Returns the
/// column each part starts at, so a caller can draw literal text over one.
pub fn status_bar(
    c: &mut Canvas,
    y: i32,
    w: i32,
    parts: &[String],
    right: &str,
    theme: &Theme,
) -> Vec<i32> {
    let bar = theme.roles.lightbar;
    c.fill(0, y, w, 1, theme.roles.body, bar);
    let name = format!(" {{Y}}{}{{W}} {} ", strings::APP_NAME, strings::VERSION);
    let mut x = c.rich(0, y, &name, Some(bar), theme);
    let mut starts = Vec::with_capacity(parts.len());
    for p in parts {
        x = c.rich(x, y, "{c}│{W} ", Some(bar), theme);
        starts.push(x);
        x = c.rich(x, y, &format!("{{W}}{p} "), Some(bar), theme);
    }
    let right = format!("{right} ");
    let x = w - i32::try_from(plain_len(&right)).unwrap_or(w);
    c.rich(x, y, &right, Some(bar), theme);
    starts
}
