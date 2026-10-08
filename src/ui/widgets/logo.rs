//! The block-pixel logo, ported from generate.py's `FONT` and `logo_grid`:
//! "PDFPUNDIT" in 10-pixel letters two pixels apart, coloured down the
//! theme's logo gradient with a faint white sheen across the middle, and a
//! drop shadow one pixel down and right. Eleven pixel rows, so six cells.

use std::f64::consts::PI;

use crate::ui::canvas::Canvas;
use crate::ui::color::{Rgb, mix, ramp};
use crate::ui::theme::Theme;

/// The word the logo spells.
const WORD: &str = "PDFPUNDIT";
/// Blank pixel columns between letters.
const GAP: usize = 2;
/// A letter's pixel rows.
const LETTER_ROWS: usize = 10;
/// The logo's pixel rows: the letters and the shadow's extra row.
pub const ROWS: usize = LETTER_ROWS + 1;

/// generate.py's `FONT`: `#` is a pixel.
const FONT: [(char, [&str; LETTER_ROWS]); 7] = [
    (
        'P',
        [
            "#######.", "########", "##....##", "##....##", "########", "#######.", "##......",
            "##......", "##......", "##......",
        ],
    ),
    (
        'D',
        [
            "######..", "#######.", "##...###", "##....##", "##....##", "##....##", "##....##",
            "##...###", "#######.", "######..",
        ],
    ),
    (
        'F',
        [
            "########", "########", "##......", "##......", "######..", "######..", "##......",
            "##......", "##......", "##......",
        ],
    ),
    (
        'U',
        [
            "##....##", "##....##", "##....##", "##....##", "##....##", "##....##", "##....##",
            "###..###", "########", ".######.",
        ],
    ),
    (
        'N',
        [
            "##....##", "###...##", "####..##", "####..##", "##.##.##", "##.##.##", "##..####",
            "##..####", "##...###", "##....##",
        ],
    ),
    (
        'I',
        [
            "######", "######", "..##..", "..##..", "..##..", "..##..", "..##..", "..##..",
            "######", "######",
        ],
    ),
    (
        'T',
        [
            "########", "########", "...##...", "...##...", "...##...", "...##...", "...##...",
            "...##...", "...##...", "...##...",
        ],
    ),
];

fn glyph(ch: char) -> &'static [&'static str; LETTER_ROWS] {
    FONT.iter()
        .find(|(c, _)| *c == ch)
        .map(|(_, rows)| rows)
        .expect("every letter of WORD is in FONT")
}

/// The logo's pixels in `theme`: [`ROWS`] rows, `None` where nothing is
/// drawn.
pub fn grid(theme: &Theme) -> Vec<Vec<Option<Rgb>>> {
    let n = WORD.chars().count();
    let mut rows = vec![String::new(); LETTER_ROWS];
    for (i, ch) in WORD.chars().enumerate() {
        for (r, row) in rows.iter_mut().enumerate() {
            row.push_str(glyph(ch)[r]);
            if i + 1 < n {
                row.push_str(&".".repeat(GAP));
            }
        }
    }
    let on: Vec<Vec<bool>> = rows
        .iter()
        .map(|row| row.chars().map(|c| c == '#').collect())
        .collect();
    let wd = on[0].len() + 1;
    let mut px = vec![vec![None; wd]; ROWS];
    for (r, row) in on.iter().enumerate() {
        for (c, &set) in row.iter().enumerate() {
            if set {
                let sheen = 0.12 * libm::sin(c as f64 / wd as f64 * PI);
                let base = ramp(&theme.logo_ramp, r as f64 / (LETTER_ROWS - 1) as f64);
                px[r][c] = Some(mix(base, Rgb([0xff, 0xff, 0xff]), sheen));
            }
        }
    }
    for (r, row) in on.iter().enumerate() {
        for (c, &set) in row.iter().enumerate() {
            if set && px[r + 1][c + 1].is_none() {
                px[r + 1][c + 1] = Some(theme.logo_shadow);
            }
        }
    }
    px
}

/// Draws the logo with its top-left pixel in cell `(x, y)`.
pub fn draw(c: &mut Canvas, x: i32, y: i32, theme: &Theme) {
    c.pix(x, 2 * y, &grid(theme));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_is_eleven_pixel_rows_and_87_wide() {
        let g = grid(Theme::default_theme());
        assert_eq!(g.len(), ROWS);
        // Nine letters (eight 8 wide, the I 6 wide), eight gaps of two, and
        // the shadow's extra column.
        assert!(g.iter().all(|row| row.len() == 8 * 8 + 6 + 8 * GAP + 1));
        // The shadow's row holds only shadow.
        let t = Theme::default_theme();
        assert!(g[ROWS - 1].iter().flatten().all(|&p| p == t.logo_shadow));
        assert!(g[ROWS - 1].iter().any(Option::is_some));
    }
}
