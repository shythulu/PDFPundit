//! The two-line progress box under the batch and result views (generate.py's
//! `progress`): what the current job is doing to which file, then the batch,
//! each with a bar and a percentage.

use crate::engine::Ratio;
use crate::ui::canvas::Canvas;
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::{Current, percent};
use crate::ui::widgets::r#box::{Panel, fit, panel_plain};

/// The bars' column and width, and the percentages' column.
const BAR_X: i32 = 40;
const BAR_W: i32 = 56;
const PCT_X: i32 = 98;
/// The box: x, width, height.
const BOX: (i32, i32, i32) = (1, 110, 4);

/// Draws the box with its top edge on row `y`: the current job's line when
/// there is one, and the batch line (`batch` is its markup) with the batch's
/// progress. Returns the box, whose bottom edge can carry a note.
pub fn progress(
    c: &mut Canvas,
    y: i32,
    current: Option<&Current>,
    batch: &str,
    batch_progress: Option<Ratio>,
    theme: &Theme,
) -> Panel {
    let (x, w, h) = BOX;
    let p = panel_plain(c, x, y, w, h, theme);
    if let Some(cur) = current {
        let label = cur.activity.label();
        let x = p.line(c, y + 1, &format!(" {{w}}{label} "), theme);
        // The name is the file's, drawn as it is, and stops short of the bar.
        let room = usize::try_from(BAR_X - 1 - x).unwrap_or(0);
        p.text(c, x, y + 1, &fit(&cur.name, room), theme.roles.file);
        bar(c, y + 1, cur.progress, &theme.gradients.bar, theme);
    }
    p.line(c, y + 2, &format!(" {batch}"), theme);
    bar(c, y + 2, batch_progress, &theme.gradients.bar2, theme);
    p
}

/// A bar along `stops` and its percentage, on row `y`. With no measure yet
/// the bar is empty and no percentage is shown.
fn bar(c: &mut Canvas, y: i32, r: Option<Ratio>, stops: &[crate::ui::color::Rgb], theme: &Theme) {
    let frac = r.map_or(0.0, |r| {
        if r.den == 0 {
            0.0
        } else {
            r.num as f64 / r.den as f64
        }
    });
    c.bar(BAR_X, y, BAR_W, frac.min(1.0), stops, theme);
    if let Some(r) = r {
        let pct = strings::N_PERCENT.replace("{n}", &percent(r).to_string());
        c.rich(PCT_X, y, &format!("{{W}}{pct:>4}"), None, theme);
    }
}
