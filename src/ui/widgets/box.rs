//! A titled panel: generate.py's `Box` with a title, as the full layout's side
//! panels draw it (double-line border along the theme's border gradient),
//! whose lines never write past its right border.

use crate::ui::canvas::{BoxStyle, Boxed, Canvas};
use crate::ui::theme::Theme;

/// A panel drawn by [`panel`], for writing lines inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel(pub Boxed);

/// Draws a `w × h` panel at `(x, y)` titled `title`.
pub fn panel(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, title: &str, theme: &Theme) -> Panel {
    let style = BoxStyle {
        title: Some(title),
        ..BoxStyle::default()
    };
    Panel(c.boxed(x, y, w, h, &style, theme))
}

impl Panel {
    /// The cells between the borders.
    pub fn inner_w(&self) -> usize {
        usize::try_from(self.0.w - 2).unwrap_or(0)
    }

    /// Markup `text` from the panel's second column on canvas row `row`, on
    /// its fill, cut at the right border. Returns the column after the text
    /// (past the border if it was cut).
    pub fn line(&self, c: &mut Canvas, row: i32, text: &str, theme: &Theme) -> i32 {
        let right = u16::try_from(self.0.x + self.0.w - 1).unwrap_or(0);
        c.clipped(right, u16::MAX, |c| self.0.line(c, row, text, theme))
    }

    /// A separator across the panel on canvas row `row`.
    pub fn sep(&self, c: &mut Canvas, row: i32, theme: &Theme) {
        self.0.sep(c, row, None, theme);
    }
}

/// `s`, cut to `max` characters with a trailing `…` when longer.
pub fn fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    if max > 0 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_stops_at_the_border() {
        let t = Theme::default_theme();
        let mut c = Canvas::new(12, 3, t);
        let p = panel(&mut c, 0, 0, 8, 3, "X", t);
        assert_eq!(p.inner_w(), 6);
        p.line(&mut c, 1, "abcdefghijkl", t);
        let row: String = (0..12).map(|x| c.get(x, 1).expect("cell").ch).collect();
        assert_eq!(row, "║abcdef║    ");
    }

    #[test]
    fn fit_ends_long_text_in_an_ellipsis() {
        assert_eq!(fit("contract_signed.pdf", 20), "contract_signed.pdf");
        assert_eq!(fit("a_very_long_file_name_indeed.pdf", 10), "a_very_lo…");
        assert_eq!(fit("abc", 1), "…");
        assert_eq!(fit("abc", 0), "");
    }
}
