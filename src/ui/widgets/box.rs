//! A titled panel: generate.py's `Box` with a title, as the full layout's side
//! panels draw it (double-line border along the theme's border gradient),
//! whose lines never write past its right border.

use crate::ui::canvas::{BoxStyle, Boxed, Canvas, take_width, text_width};
use crate::ui::color::Rgb;
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

    /// Literal `text` (no markup: a file's name) from column `x` on canvas row
    /// `row`, in `fg` on the panel's fill, cut at the right border. Returns
    /// the column after the text (past the border if it was cut).
    pub fn text(&self, c: &mut Canvas, x: i32, row: i32, text: &str, fg: Rgb) -> i32 {
        let right = u16::try_from(self.0.x + self.0.w - 1).unwrap_or(0);
        let fill = Some(self.0.fill);
        c.clipped(right, u16::MAX, |c| c.text(x, row, text, fg, fill))
    }

    /// Markup `text` from column `x` on canvas row `row`, on the panel's
    /// fill, cut at the right border. Returns the column after the text.
    pub fn line_at(&self, c: &mut Canvas, x: i32, row: i32, text: &str, theme: &Theme) -> i32 {
        let right = u16::try_from(self.0.x + self.0.w - 1).unwrap_or(0);
        let fill = Some(self.0.fill);
        c.clipped(right, u16::MAX, |c| c.rich(x, row, text, fill, theme))
    }

    /// [`Panel::text`] that keeps each cell's background (a row on the light
    /// bar).
    pub fn text_over(&self, c: &mut Canvas, x: i32, row: i32, text: &str, fg: Rgb) -> i32 {
        let right = u16::try_from(self.0.x + self.0.w - 1).unwrap_or(0);
        c.clipped(right, u16::MAX, |c| c.text(x, row, text, fg, None))
    }

    /// A separator across the panel on canvas row `row`.
    pub fn sep(&self, c: &mut Canvas, row: i32, theme: &Theme) {
        self.0.sep(c, row, None, theme);
    }
}

/// Draws an untitled `w × h` panel at `(x, y)`.
pub fn panel_plain(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, theme: &Theme) -> Panel {
    Panel(c.boxed(x, y, w, h, &BoxStyle::default(), theme))
}

/// Draws a box styled `style` titled `title`, a space and `name`, the name
/// drawn as it is (a file's name is not markup) and cut to `max` cells;
/// just `title` when there is no name.
pub fn named(
    c: &mut Canvas,
    (x, y, w, h): (i32, i32, i32, i32),
    style: BoxStyle<'_>,
    title: &str,
    name: &str,
    max: usize,
    theme: &Theme,
) -> Panel {
    let name = fit(name, max);
    let n = text_width(&name);
    // The name's cells are held by spaces, then written over.
    let shown = if n == 0 {
        title.to_string()
    } else {
        format!("{title} {}", " ".repeat(n))
    };
    let b = c.boxed(
        x,
        y,
        w,
        h,
        &BoxStyle {
            title: Some(&shown),
            ..style
        },
        theme,
    );
    let (tfg, tbg) = style.title_colours;
    let fg = theme.slot(tfg).unwrap_or(theme.roles.body);
    let nx = x + 4 + i32::try_from(title.chars().count()).unwrap_or(0) + 1;
    c.text(nx, y, &name, fg, theme.slot(tbg));
    Panel(b)
}

impl Panel {
    /// Literal `text` in `fg` set into the bottom edge, centred between `╡ `
    /// and ` ╞` (the mockup's box-note shape), cut to fit inside the corners.
    pub fn bottom_note(&self, c: &mut Canvas, text: &str, fg: Rgb) {
        let b = &self.0;
        let room = usize::try_from(b.w - 6).unwrap_or(0);
        let text = fit(text, room);
        let n = i32::try_from(text_width(&text)).unwrap_or(0);
        let total = n + 4;
        let x0 = b.x + (b.w - total).div_euclid(2);
        let j = b.h - 1;
        let row = b.y + j;
        let fill = Some(b.fill);
        c.put(x0, row, '╡', Some(b.col(x0 - b.x, j)), fill);
        c.text(x0 + 1, row, &format!(" {text} "), fg, fill);
        let close = x0 + n + 3;
        c.put(close, row, '╞', Some(b.col(close - b.x, j)), fill);
    }
}

/// `s`, cut to `max` cells ([`text_width`]) with a trailing `…` when wider.
/// A wide character that would straddle the cut is left out and a space
/// follows the `…` in its place, so a cut text always takes `max` cells,
/// whatever its characters' widths.
pub fn fit(s: &str, max: usize) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    let mut out = take_width(s, max.saturating_sub(1)).to_string();
    if max > 0 {
        out.push('…');
    }
    if text_width(&out) < max {
        out.push(' ');
    }
    out
}

/// [`fit`], then spaces up to `w` cells: a fixed-width column. (`format!`'s
/// `{:<w$}` counts characters, not cells.)
pub fn pad(s: &str, w: usize) -> String {
    let mut out = fit(s, w);
    let n = text_width(&out);
    out.extend(std::iter::repeat_n(' ', w.saturating_sub(n)));
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

    /// Text is drawn as it is: markup tokens and control characters in it are
    /// not read, and it stops at the border.
    #[test]
    fn literal_text_is_drawn_verbatim_and_stops_at_the_border() {
        let t = Theme::default_theme();
        let mut c = Canvas::new(14, 3, t);
        let p = panel(&mut c, 0, 0, 14, 3, "X", t);
        let end = p.text(&mut c, 1, 1, "a{R}{}\u{1b}b{/K}xyz", t.roles.file);
        let row: String = (0..14).map(|x| c.get(x, 1).expect("cell").ch).collect();
        assert_eq!(row, "║a{R}{}\u{fffd}b{/K}║");
        assert_eq!(end, 16);
        for x in 1..13 {
            let cell = c.get(x, 1).expect("cell");
            assert_eq!(cell.fg, t.roles.file, "column {x}");
            assert_eq!(cell.bg, p.0.fill, "column {x}");
        }
    }

    #[test]
    fn fit_ends_long_text_in_an_ellipsis() {
        assert_eq!(fit("contract_signed.pdf", 20), "contract_signed.pdf");
        assert_eq!(fit("a_very_long_file_name_indeed.pdf", 10), "a_very_lo…");
        assert_eq!(fit("abc", 1), "…");
        assert_eq!(fit("abc", 0), "");
    }

    /// Widths are cells: a wide character counts two and is never cut in
    /// half, a zero-width one counts nothing.
    #[test]
    fn fit_and_pad_count_cells() {
        assert_eq!(fit("报告书.pdf", 10), "报告书.pdf");
        assert_eq!(fit("报告书.pdf", 9), "报告书.p…");
        assert_eq!(fit("报告书.pdf", 6), "报告… ");
        assert_eq!(fit("报告书.pdf", 5), "报告…");
        assert_eq!(fit("报告书.pdf", 4), "报… ");
        assert_eq!(fit("📄", 1), "…");
        assert_eq!(fit("cafe\u{301}", 4), "cafe\u{301}");
        assert_eq!(pad("报告", 6), "报告  ");
        assert_eq!(pad("报告书.pdf", 6), "报告… ");
        assert_eq!(pad("ab", 1), "…");
        for (name, _) in crate::ui::canvas::WIDE_NAMES {
            for w in 0..80 {
                let want = text_width(name).min(w);
                assert_eq!(text_width(&fit(name, w)), want, "{name:?} in {w}");
                assert_eq!(text_width(&pad(name, w)), w, "{name:?} in {w}");
            }
        }
    }
}
