//! The queue box (generate.py's `queue_box`, frames 03 and 05): one row per
//! dropped file, its glyph in the state's colour (blinking while the file is
//! worked on or waits on you), its name and its status. The cursor's row sits
//! on the light bar in bright text. A long queue scrolls to keep the cursor
//! in view.

use crate::ui::canvas::BoxStyle;
use crate::ui::canvas::Canvas;
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::{QueueRow, RowKind};
use crate::ui::widgets::r#box::{Panel, pad};

/// The most rows the box shows at once (frame 03's seven files).
pub const MAX_ROWS: usize = 7;

/// How the box is drawn: where, how wide the name column is, and whether
/// the statuses take their short form (the result view's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    /// The name column, without the space after it.
    pub name_w: usize,
    pub short: bool,
}

impl QueueBox {
    /// Draws the box with `note` (markup) in its top edge and the visible
    /// window of `rows`. Its height follows the rows shown, as the mockup's
    /// does: a heading row, the rows and a blank row inside the border.
    pub fn draw(&self, c: &mut Canvas, rows: &[QueueRow], note: &str, theme: &Theme) -> Panel {
        let (_, shown) = window(rows);
        let h = to_i32(shown.len()) + 4;
        let style = BoxStyle {
            title: Some(strings::QUEUE),
            note: Some(note),
            ..BoxStyle::default()
        };
        let p = Panel(c.boxed(self.x, self.y, self.w, h, &style, theme));
        p.line(
            c,
            self.y + 1,
            &format!(" {{D}}{}", strings::QUEUE_HEADINGS),
            theme,
        );
        for (i, r) in shown.iter().enumerate() {
            self.row(c, &p, self.y + 2 + to_i32(i), r, theme);
        }
        p
    }

    fn row(&self, c: &mut Canvas, p: &Panel, y: i32, r: &QueueRow, theme: &Theme) {
        let bar = theme.slot('b').unwrap_or(theme.roles.lightbar);
        if r.selected {
            c.fill(self.x + 1, y, self.w - 2, 1, theme.roles.body, bar);
        }
        c.put(
            self.x + 2,
            y,
            r.kind.glyph(),
            Some(r.kind.role().rgb(&theme.roles)),
            None,
        );
        if r.kind.blinks() {
            c.set_blink(self.x + 2, y);
        }
        let bright = theme.slot('W').unwrap_or(theme.roles.heading);
        let (name_fg, status_fg) = if r.selected {
            (bright, bright)
        } else {
            (theme.roles.file, status_colour(r, theme))
        };
        // The name and the status are data, drawn as they are.
        let name = format!("{} ", pad(&r.name, self.name_w));
        let x = p.text_over(c, self.x + 5, y, &name, name_fg);
        let detail = if self.short {
            &r.short_detail
        } else {
            &r.detail
        };
        let status = match detail {
            Some(d) => format!("{} · {d}", r.label),
            None => r.label.to_string(),
        };
        p.text_over(c, x, y, &status, status_fg);
    }
}

/// The rows shown: all of them up to [`MAX_ROWS`], else the window that ends
/// at the cursor's row once it is past the first screenful. Returns the first
/// shown row's index too.
fn window(rows: &[QueueRow]) -> (usize, &[QueueRow]) {
    let sel = rows.iter().position(|r| r.selected).unwrap_or(0);
    let first = (sel + 1).saturating_sub(MAX_ROWS);
    let end = (first + MAX_ROWS).min(rows.len());
    (first, &rows[first..end])
}

/// A status in the mockup's colours: repaired in body text, a clean file and
/// a queued one dim, the rest in their state's colour. A clean file is the
/// one finished-fine row whose short form drops its detail
/// (`QueueRow::short_detail`).
fn status_colour(r: &QueueRow, theme: &Theme) -> crate::ui::color::Rgb {
    match r.kind {
        RowKind::Ok if r.detail.is_some() && r.short_detail.is_none() => theme.roles.dim,
        RowKind::Ok => theme.roles.body,
        kind => kind.role().rgb(&theme.roles),
    }
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(i: usize, selected: bool) -> QueueRow {
        QueueRow {
            name: format!("f{i}.pdf"),
            kind: RowKind::Queued,
            label: "queued",
            detail: None,
            short_detail: None,
            selected,
        }
    }

    #[test]
    fn a_long_queue_scrolls_to_keep_the_cursor_in_view() {
        let rows: Vec<QueueRow> = (0..12).map(|i| row(i, i == 9)).collect();
        let (first, shown) = window(&rows);
        assert_eq!((first, shown.len()), (3, MAX_ROWS));
        assert!(shown.last().is_some_and(|r| r.selected));
        let rows: Vec<QueueRow> = (0..12).map(|i| row(i, i == 2)).collect();
        assert_eq!(window(&rows).0, 0);
        let rows: Vec<QueueRow> = (0..3).map(|i| row(i, false)).collect();
        assert_eq!(window(&rows).1.len(), 3);
        assert_eq!(window(&[]).1.len(), 0);
    }

    #[test]
    fn names_and_statuses_stop_at_the_border() {
        let t = Theme::default_theme();
        let mut c = Canvas::new(40, 8, t);
        let mut r = row(0, false);
        r.name = "a_really_quite_long_evidence_file_name.pdf".into();
        r.label = "failed";
        r.kind = RowKind::Failed;
        r.detail = Some("x".repeat(60));
        let q = QueueBox {
            x: 0,
            y: 0,
            w: 30,
            name_w: 10,
            short: false,
        };
        q.draw(&mut c, &[r], "", t);
        let line: String = (0..40).map(|x| c.get(x, 2).expect("cell").ch).collect();
        assert_eq!(line, format!("║ ×  a_really_… failed · xxxx║{:10}", ""));
    }
}
