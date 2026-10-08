//! Finding and font rows (frames 03 and 05): a finding's level tag, its class
//! code and its summary, with the result view's before and after marks; a
//! font slot's name, where its font came from and what became of it.
//!
//! The summary and the font's name come from the file and the engine, so they
//! are drawn as they are, never read as colour markup.

use crate::engine::FontResolutionKind;
use crate::ui::canvas::Canvas;
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::{FindingRow, FontRow, Level};
use crate::ui::widgets::r#box::{Panel, fit};

/// A finding's tag (`[ERR]`) and its colour's slot letter.
pub fn tag(level: Level) -> (&'static str, char) {
    match level {
        Level::Error => (strings::TAG_ERR, 'R'),
        Level::Warning => (strings::TAG_WRN, 'Y'),
        Level::Info => (strings::TAG_INF, 'c'),
    }
}

/// One finding on canvas row `y` of `p`: a space, the tag, the class code
/// (or blanks on an info row) and the summary padded to `summary_w`. Returns
/// the column after the padded summary.
pub fn finding(
    c: &mut Canvas,
    p: &Panel,
    y: i32,
    f: &FindingRow,
    summary_w: usize,
    theme: &Theme,
) -> i32 {
    let (tag, k) = tag(f.level);
    let x = match f.code {
        Some(code) => {
            let x = p.line(c, y, &format!(" {{{k}}}{tag} {{W}}"), theme);
            p.text(c, x, y, &format!("{code} "), slot(theme, 'W'))
        }
        None => p.line(c, y, &format!(" {{{k}}}{tag}    "), theme),
    };
    let summary = format!(
        "{:<w$}",
        fit(&f.summary, summary_w.saturating_sub(1)),
        w = summary_w
    );
    p.text(c, x, y, &summary, theme.roles.body)
}

/// The result view's before and after marks from column `x`: `×` before for
/// a problem, then `√` once the re-diagnosis no longer finds it (or `×`
/// while it does); dots on an info row. Returns the column after.
pub fn before_after(
    c: &mut Canvas,
    p: &Panel,
    x: i32,
    y: i32,
    f: &FindingRow,
    theme: &Theme,
) -> i32 {
    let (_, k) = tag(f.level);
    if f.level == Level::Info {
        return p.text(c, x, y, "  ·    ·", theme.roles.dim);
    }
    let x = p.text(c, x, y, "  ×    ", slot(theme, k));
    match f.fixed {
        Some(true) => p.text(c, x, y, "√ ", theme.roles.ok),
        Some(false) => p.text(c, x, y, "× ", slot(theme, k)),
        None => x,
    }
}

/// One font slot on canvas row `y` of `p`: the slot, its name padded to
/// `name_w` (or "unknown"), then where the font came from and what became of
/// it. A picked font's family is not in the view model (T-22b's open
/// question), so nothing follows "bundled" for it yet; an unresolved slot
/// that lost its font shows nothing either, since the needs-input box asks
/// about it.
pub fn font(c: &mut Canvas, p: &Panel, y: i32, f: &FontRow, name_w: usize, theme: &Theme) {
    let x = p.line(c, y, " ", theme);
    let x = p.text(c, x, y, &format!("{:<2} ", f.slot), slot(theme, 'W'));
    let name = f.name.as_deref().unwrap_or(strings::FONT_UNKNOWN);
    let name = format!("{:<w$} ", fit(name, name_w), w = name_w);
    let x = p.text(c, x, y, &name, theme.roles.body);
    let source = |s: &str| format!("{{D}}{s:<9}");
    let markup = match &f.resolution {
        None if f.embedded => format!(
            "{}{{G}}{}",
            source(strings::FONT_EMBEDDED),
            strings::FONT_INTACT
        ),
        None => String::new(),
        Some(FontResolutionKind::Relinked(_)) => format!(
            "{}{{G}}{}",
            source(strings::FONT_EMBEDDED),
            strings::FONT_RELINKED
        ),
        Some(FontResolutionKind::Picked { .. } | FontResolutionKind::Substituted(_)) => {
            source(strings::FONT_BUNDLED)
        }
        Some(FontResolutionKind::TextOnly) => format!("{{Y}}{}", strings::FONT_TEXT_ONLY),
        Some(FontResolutionKind::Skipped) => format!("{{Y}}{}", strings::FONT_LEFT_AS_FOUND),
    };
    let x = p.line_at(c, x, y, &markup, theme);
    if let Some(FontResolutionKind::Substituted(choice)) = &f.resolution {
        p.text(c, x, y, &choice.label, theme.roles.file);
    }
}

fn slot(theme: &Theme, k: char) -> crate::ui::color::Rgb {
    theme.slot(k).unwrap_or(theme.roles.body)
}
