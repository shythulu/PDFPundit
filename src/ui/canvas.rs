//! Framework-free cell grid (D-045), ported from generate.py's `Canvas` and
//! `Box`: every layout, modal and the cat draw into a [`Canvas`], the T-00
//! goldens are compared against it cell for cell, and `ui/term.rs` holds the
//! only blit to the terminal.
//!
//! A cell is a character in two colours. Half-block pixel pairs are cells
//! holding `'▀'`, the top pixel in `fg` and the bottom one in `bg` (what the
//! mockup calls an `HB` cell): drawing pixels onto a cell keeps the other half
//! of a pixel pair, and writing text over one first resets its background, as
//! the mockup does. No layout writes `'▀'` as text.
//!
//! A wide character (CJK, most emoji; [`width`] 2) takes its cell and the one
//! after it, which holds a space: the terminal draws the glyph over both.
//! Writing over either half removes the whole character, and one that would
//! cross the edge or a clip is drawn as a space instead (D-118).
//!
//! Colours come from the [`Theme`] by the mockup's slot letters (`'K'` the
//! background, `'M'` needs-input, …; [`Theme::slot`]) or as plain [`Rgb`]s.
//! Every gradient position is `f64`, as in the mockup (eng-r2-q8). Writes
//! outside the canvas are dropped, so a layout can never draw past its area.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;
use unicode_width::UnicodeWidthChar;

use super::cat::CellGrid;
use super::color::{Rgb, mix, ramp};
use super::theme::Theme;

/// The character a pixel-pair cell holds.
pub const HALF: char = '▀';

/// One terminal cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasCell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
}

/// A `w × h` grid of cells, row-major, and the cells that blink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub w: u16,
    pub h: u16,
    pub cells: Vec<CanvasCell>,
    /// `(x, y)` of every blinking cell; the blit shows or hides them by the
    /// blink phase.
    pub blink: BTreeSet<(u16, u16)>,
    /// The theme's background, which text written over a pixel pair gets.
    bg: Rgb,
    /// Writes land only left of and above this cell (see [`Canvas::clipped`]);
    /// the canvas's own size outside a clip.
    lim: (u16, u16),
}

impl Canvas {
    /// A blank canvas: spaces in the theme's body colour on its background.
    pub fn new(w: u16, h: u16, theme: &Theme) -> Canvas {
        let blank = CanvasCell {
            ch: ' ',
            fg: theme.roles.body,
            bg: theme.roles.bg,
        };
        Canvas {
            w,
            h,
            cells: vec![blank; usize::from(w) * usize::from(h)],
            blink: BTreeSet::new(),
            bg: theme.roles.bg,
            lim: (w, h),
        }
    }

    /// Runs `f` with every write limited to the top-left `w × h` cells, so a
    /// layout drawn on a larger canvas cannot spill past its own area.
    pub fn clipped<R>(&mut self, w: u16, h: u16, f: impl FnOnce(&mut Canvas) -> R) -> R {
        let outer = self.lim;
        self.lim = (outer.0.min(w), outer.1.min(h));
        let r = f(self);
        self.lim = outer;
        r
    }

    /// The cell at `(x, y)`; `None` outside the canvas.
    pub fn get(&self, x: u16, y: u16) -> Option<CanvasCell> {
        (x < self.w && y < self.h).then(|| self.cells[self.index(x, y)])
    }

    fn index(&self, x: u16, y: u16) -> usize {
        usize::from(y) * usize::from(self.w) + usize::from(x)
    }

    /// `(x, y)` as a cell position, if it is on the canvas and inside the
    /// current clip.
    fn at(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let (x, y) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
        (x < self.lim.0 && y < self.lim.1).then_some((x, y))
    }

    fn cell_mut(&mut self, x: i32, y: i32) -> Option<&mut CanvasCell> {
        let (x, y) = self.at(x, y)?;
        let i = self.index(x, y);
        Some(&mut self.cells[i])
    }

    /// [`Canvas::cell_mut`] for a cell about to get a new character: if it is
    /// the second half of a wide character, that character gives way to a
    /// space, so its glyph does not cover the new one.
    fn claim(&mut self, x: i32, y: i32) -> Option<&mut CanvasCell> {
        let (cx, cy) = self.at(x, y)?;
        if cx > 0 {
            let left = self.index(cx - 1, cy);
            if width(self.cells[left].ch) == 2 {
                self.cells[left].ch = ' ';
            }
        }
        self.cell_mut(x, y)
    }

    /// Marks `(x, y)` as blinking, if it is on the canvas.
    pub fn set_blink(&mut self, x: i32, y: i32) {
        if let Some(pos) = self.at(x, y) {
            self.blink.insert(pos);
        }
    }

    /// Writes `ch` at `(x, y)`, recolouring the foreground and background
    /// that are given and keeping the others. Text over a pixel pair gets the
    /// canvas background first. A wide `ch` also takes the next cell (a space
    /// in the same colours), or is written as a space when that cell is past
    /// the edge or the clip.
    pub fn put(&mut self, x: i32, y: i32, ch: char, fg: Option<Rgb>, bg: Option<Rgb>) {
        let wide = width(ch) == 2;
        let room = self.at(x.saturating_add(1), y).is_some();
        let ch = if wide && !room { ' ' } else { ch };
        self.put_one(x, y, ch, fg, bg, true);
        if wide && room {
            // The second half: not `claim`ed, its left neighbour is `ch`.
            self.put_one(x + 1, y, ' ', fg, bg, false);
        }
    }

    /// One cell of [`Canvas::put`]; `claim` blanks a wide character whose
    /// second half this cell is.
    fn put_one(&mut self, x: i32, y: i32, ch: char, fg: Option<Rgb>, bg: Option<Rgb>, claim: bool) {
        let canvas_bg = self.bg;
        let cell = if claim {
            self.claim(x, y)
        } else {
            self.cell_mut(x, y)
        };
        let Some(cell) = cell else {
            return;
        };
        if cell.ch == HALF {
            cell.bg = canvas_bg;
        }
        cell.ch = ch;
        if let Some(fg) = fg {
            cell.fg = fg;
        }
        if let Some(bg) = bg {
            cell.bg = bg;
        }
    }

    /// Fills a `w × h` block with spaces in `fg` on `bg`.
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, fg: Rgb, bg: Rgb) {
        for j in y..y + h {
            for i in x..x + w {
                self.put(i, j, ' ', Some(fg), Some(bg));
            }
        }
    }

    /// Writes `text` from `(x, y)` in the mockup's inline colour markup:
    /// `{F}` sets the foreground to slot `F`, `{F/B}` both, `{/B}` the
    /// background, and `{/_}` goes back to keeping each cell's background.
    /// Text starts in the body colour (`w`) on `bg` (`None` keeps each cell's
    /// background). Returns the column after the last character.
    pub fn rich(&mut self, x: i32, y: i32, text: &str, bg: Option<Rgb>, theme: &Theme) -> i32 {
        let mut fg = theme.roles.body;
        let mut bg = bg;
        let mut cx = x;
        for piece in markup(text) {
            match piece {
                Piece::Colour { fg: f, bg: b } => {
                    if let Some(c) = f.and_then(|k| theme.slot(k)) {
                        fg = c;
                    }
                    match b {
                        Some('_') => bg = None,
                        Some(k) => bg = theme.slot(k).or(bg),
                        None => {}
                    }
                }
                Piece::Char(ch) => {
                    self.put(cx, y, ch, Some(fg), bg);
                    cx += 1;
                }
            }
        }
        cx
    }

    /// Writes `text` from `(x, y)` as it is, in `fg` on `bg` (`None` keeps
    /// each cell's background): no markup, so a `{` in untrusted text such as
    /// a file's name is drawn, not read as a colour. Every string from a file
    /// name, a path, a PDF, a font name or a finding is drawn with this. A
    /// control, bidi control or other format character is drawn as `�`
    /// ([`printable`]), so none reaches a cell. Cells are counted as the
    /// terminal counts them ([`width`]): the text is composed (NFC) first, so
    /// a decomposed accent joins its letter, then a wide character takes two
    /// cells and a zero-width one is dropped. Returns the column after the
    /// last character.
    pub fn text(&mut self, x: i32, y: i32, text: &str, fg: Rgb, bg: Option<Rgb>) -> i32 {
        let mut cx = x;
        for ch in text.nfc().map(printable) {
            let n = width(ch);
            if n > 0 {
                self.put(cx, y, ch, Some(fg), bg);
                cx = cx.saturating_add(to_i32(n));
            }
        }
        cx
    }

    /// Writes `text` from `(x, y)` with each character coloured along
    /// `stops`, left to right, or out from both ends to the middle when `sym`.
    /// Spaces are skipped, so whatever is under them shows.
    pub fn gtext(&mut self, x: i32, y: i32, text: &str, stops: &[Rgb], sym: bool, bg: Option<Rgb>) {
        let n = text.chars().count();
        let last = n.saturating_sub(1) as f64;
        for (i, ch) in text.chars().enumerate() {
            let p = if sym {
                i.min(n - 1 - i) as f64 / (last / 2.0).max(1.0)
            } else {
                i as f64 / last.max(1.0)
            };
            if ch != ' ' {
                self.put(x + to_i32(i), y, ch, Some(ramp(stops, p)), bg);
            }
        }
    }

    /// A `w`-cell progress bar at `pct` (0..1): filled cells along `stops`,
    /// then `▓▒` in the last stop, then dim dots.
    pub fn bar(&mut self, x: i32, y: i32, w: i32, pct: f64, stops: &[Rgb], theme: &Theme) {
        // Python's int(round(w * pct)): ties to even; small, so the cast is exact.
        let n = (f64::from(w) * pct).round_ties_even() as i32;
        let end = stops.last().copied();
        let last = f64::from((w - 1).max(1));
        for i in 0..w {
            let (ch, fg) = if i < n {
                ('█', Some(ramp(stops, f64::from(i) / last)))
            } else if i == n {
                ('▓', end)
            } else if i == n + 1 {
                ('▒', end)
            } else {
                ('·', Some(theme.roles.dim))
            };
            self.put(x + i, y, ch, fg, None);
        }
    }

    /// Draws pixel rows from `(x, py)`, `py` in pixel rows (two to a cell), so a
    /// picture can start on the lower half of a cell. `None` pixels are
    /// transparent: the other half of the cell keeps what was there (a pixel
    /// pair's own pixel, or a text cell's background).
    pub fn pix<R: AsRef<[Option<Rgb>]>>(&mut self, x: i32, py: i32, rows: &[R]) {
        for (r, row) in rows.iter().enumerate() {
            let p = py + to_i32(r);
            let (y, lower) = (p.div_euclid(2), p.rem_euclid(2) == 1);
            for (c, &px) in row.as_ref().iter().enumerate() {
                if let Some(px) = px {
                    let (top, bottom) = if lower {
                        (None, Some(px))
                    } else {
                        (Some(px), None)
                    };
                    self.pixel_pair(x + to_i32(c), y, top, bottom);
                }
            }
        }
    }

    /// Draws a half-block grid (the cat, the plate) with its top-left cell at
    /// `(x, y)`; empty cells and pixels are transparent.
    pub fn blit_grid(&mut self, grid: &CellGrid, x: i32, y: i32) {
        for gy in 0..grid.rows {
            for gx in 0..grid.cols {
                if let Some(cell) = grid.get(gx, gy) {
                    let (top, bottom) = cell.halves();
                    self.pixel_pair(x + i32::from(gx), y + i32::from(gy), top, bottom);
                }
            }
        }
    }

    /// Copies every cell of `src`, and its blinking cells, with `src`'s
    /// top-left cell at `(x, y)` (a layout centred in a larger terminal).
    /// What falls past this canvas's edge is dropped.
    pub fn paste(&mut self, src: &Canvas, x: u16, y: u16) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let (Some(tx), Some(ty)) = (x.checked_add(sx), y.checked_add(sy)) else {
                    continue;
                };
                if tx >= self.w || ty >= self.h {
                    continue;
                }
                let i = self.index(tx, ty);
                self.cells[i] = src.cells[src.index(sx, sy)];
                if src.blink.contains(&(sx, sy)) {
                    self.blink.insert((tx, ty));
                }
            }
        }
    }

    /// generate.py's `pix` for one cell: the given pixels over what is there.
    fn pixel_pair(&mut self, x: i32, y: i32, top: Option<Rgb>, bottom: Option<Rgb>) {
        if top.is_none() && bottom.is_none() {
            return;
        }
        let Some(cell) = self.claim(x, y) else {
            return;
        };
        let (under_top, under_bottom) = if cell.ch == HALF {
            (cell.fg, cell.bg)
        } else {
            (cell.bg, cell.bg)
        };
        *cell = CanvasCell {
            ch: HALF,
            fg: top.unwrap_or(under_top),
            bg: bottom.unwrap_or(under_bottom),
        };
    }

    /// Mixes both colours of every cell in a `w × h` block toward the
    /// background by `f` (generate.py's `dim_rect`): the side panels stepping
    /// back while the cat reacts.
    pub fn dim_rect(&mut self, x: i32, y: i32, w: i32, h: i32, f: f64) {
        for j in y..y + h {
            for i in x..x + w {
                self.darken(i, j, f);
            }
        }
    }

    /// Mixes the colours of `(x, y)` toward the background by `f`.
    fn darken(&mut self, x: i32, y: i32, f: f64) {
        let bg = self.bg;
        if let Some(cell) = self.cell_mut(x, y) {
            cell.fg = mix(cell.fg, bg, f);
            cell.bg = mix(cell.bg, bg, f);
        }
    }

    /// generate.py's `Box`: a `w × h` box filled with `style.fill`, its border
    /// double- or single-line and coloured along the gradient diagonally from
    /// the top-left corner, with an optional title, a note on the right of
    /// the top edge and a drop shadow. The returned [`Boxed`] draws lines and
    /// separators inside it.
    pub fn boxed(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        style: &BoxStyle<'_>,
        theme: &Theme,
    ) -> Boxed {
        let b = Boxed {
            x,
            y,
            w,
            h,
            grad: style
                .grad
                .map_or_else(|| theme.roles.border_stops.clone(), <[Rgb]>::to_vec),
            fill: style.fill.unwrap_or(theme.roles.bg),
            double: style.double,
        };
        if style.shadow {
            for j in y + 1..=y + h {
                for i in [x + w, x + w + 1] {
                    self.darken(i, j, 0.75);
                }
            }
            for i in x + 2..x + w {
                self.darken(i, y + h, 0.75);
            }
        }
        self.fill(x + 1, y + 1, w - 2, h - 2, theme.roles.body, b.fill);
        let [hz, vt, tl, tr, bl, br] = if b.double {
            ['═', '║', '╔', '╗', '╚', '╝']
        } else {
            ['─', '│', '┌', '┐', '└', '┘']
        };
        let fill = Some(b.fill);
        for i in 0..w {
            for j in [0, h - 1] {
                let ch = match (i == 0, i == w - 1, j == 0) {
                    (true, _, true) => tl,
                    (true, _, false) => bl,
                    (false, true, true) => tr,
                    (false, true, false) => br,
                    _ => hz,
                };
                self.put(x + i, y + j, ch, Some(b.col(i, j)), fill);
            }
        }
        for j in 1..h - 1 {
            self.put(x, y + j, vt, Some(b.col(0, j)), fill);
            self.put(x + w - 1, y + j, vt, Some(b.col(w - 1, j)), fill);
        }
        let (open, close) = if b.double {
            ('╡', '╞')
        } else {
            ('┤', '├')
        };
        if let Some(title) = style.title {
            self.put(x + 2, y, open, Some(b.col(2, 0)), None);
            let (tfg, tbg) = style.title_colours;
            let e = self.rich(x + 3, y, &format!("{{{tfg}/{tbg}}} {title} "), None, theme);
            self.put(e, y, close, Some(b.col(e - x, 0)), fill);
        }
        if let Some(note) = style.note {
            let n = to_i32(plain_len(note));
            let nx = x + w - 4 - (n + 2);
            self.put(nx, y, open, Some(b.col(nx - x, 0)), fill);
            self.rich(nx + 1, y, &format!(" {note} "), fill, theme);
            self.put(nx + n + 3, y, close, Some(b.col(nx + n + 3 - x, 0)), fill);
        }
        b
    }
}

/// How [`Canvas::boxed`] draws a box. The default is the mockup's: a double
/// border along the theme's border gradient, filled with the background, the
/// title in `W` on `b`, no shadow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxStyle<'a> {
    /// Drawn in the top edge from column 2.
    pub title: Option<&'a str>,
    /// Markup, drawn at the right of the top edge.
    pub note: Option<&'a str>,
    /// The border gradient; `None` is the theme's `border_stops`.
    pub grad: Option<&'a [Rgb]>,
    /// The inside; `None` is the theme's background.
    pub fill: Option<Rgb>,
    /// The title's foreground and background slot letters.
    pub title_colours: (char, char),
    pub shadow: bool,
    /// Double lines (`═║`) or single (`─│`).
    pub double: bool,
}

impl Default for BoxStyle<'_> {
    fn default() -> Self {
        BoxStyle {
            title: None,
            note: None,
            grad: None,
            fill: None,
            title_colours: ('W', 'b'),
            shadow: false,
            double: true,
        }
    }
}

/// A box drawn by [`Canvas::boxed`], for drawing inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Boxed {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub grad: Vec<Rgb>,
    pub fill: Rgb,
    pub double: bool,
}

impl Boxed {
    /// The border colour at `(i, j)` inside the box: the gradient runs
    /// diagonally from the top-left corner to the bottom-right one.
    pub fn col(&self, i: i32, j: i32) -> Rgb {
        let t = (f64::from(i) / f64::from((self.w - 1).max(1))
            + f64::from(j) / f64::from((self.h - 1).max(1)))
            / 2.0;
        ramp(&self.grad, t)
    }

    /// A separator across the box at canvas row `row`, its line coloured out
    /// from both ends along `stops` (`None` is the theme's `sep` gradient).
    pub fn sep(&self, c: &mut Canvas, row: i32, stops: Option<&[Rgb]>, theme: &Theme) {
        let j = row - self.y;
        let (l, r) = if self.double {
            ('╟', '╢')
        } else {
            ('├', '┤')
        };
        let fill = Some(self.fill);
        c.put(self.x, row, l, Some(self.col(0, j)), fill);
        c.put(
            self.x + self.w - 1,
            row,
            r,
            Some(self.col(self.w - 1, j)),
            fill,
        );
        let line: String = "─".repeat(usize::try_from(self.w - 2).unwrap_or(0));
        let stops = stops.unwrap_or(&theme.gradients.sep);
        c.gtext(self.x + 1, row, &line, stops, true, fill);
    }

    /// Markup text from the box's second column on canvas row `row`, on the
    /// box's fill. Returns the column after the text.
    pub fn line(&self, c: &mut Canvas, row: i32, text: &str, theme: &Theme) -> i32 {
        c.rich(self.x + 1, row, text, Some(self.fill), theme)
    }
}

/// One piece of markup text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Piece {
    /// `{F/B}`: either letter may be absent; `_` as `B` means "no background".
    Colour {
        fg: Option<char>,
        bg: Option<char>,
    },
    Char(char),
}

/// Splits `text` the way generate.py's `TOK` does: a token is `{`, an optional
/// letter or `_`, optionally `/` and a letter or `_`, then `}`. Any other `{`
/// is text.
fn markup(text: &str) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let slot = |c: Option<&char>| c.copied().filter(|c| c.is_ascii_alphabetic() || *c == '_');
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            let mut j = i + 1;
            let fg = slot(chars.get(j));
            if fg.is_some() {
                j += 1;
            }
            let mut bg = None;
            if chars.get(j) == Some(&'/')
                && let Some(b) = slot(chars.get(j + 1))
            {
                bg = Some(b);
                j += 2;
            }
            if chars.get(j) == Some(&'}') {
                out.push(Piece::Colour { fg, bg });
                i = j + 1;
                continue;
            }
        }
        out.push(Piece::Char(chars[i]));
        i += 1;
    }
    out
}

/// `ch`, or `�` in place of a character a terminal would act on rather than
/// draw, or would draw as nothing: a C0 or C1 control or DEL (an escape
/// sequence starts with one); a format character (general category Cf),
/// among them the bidi controls (U+200E, U+200F, U+202A–U+202E,
/// U+2066–U+2069), which reorder what follows them
/// (`invoice\u{202E}fdp.exe.pdf`), the soft hyphen, the zero-width space and
/// joiners and the BOM; or the line and paragraph separators (D-118).
pub fn printable(ch: char) -> char {
    if ch.is_control() || is_format(ch) || matches!(ch, '\u{2028}' | '\u{2029}') {
        '\u{fffd}'
    } else {
        ch
    }
}

/// Whether `ch` is a format character: Unicode 16's general category Cf,
/// listed here because no crate in the graph exposes general categories.
fn is_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{890}'..='\u{891}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

/// How many cells `ch` takes in a terminal once [`printable`] has replaced
/// it: 2 for a wide character (CJK, most emoji), 0 for a zero-width one (a
/// combining mark, a variation selector), 1 otherwise. The blit's diff counts
/// cells with the same `unicode-width` tables, so the two always agree.
pub fn width(ch: char) -> usize {
    printable(ch).width().unwrap_or(1)
}

/// How many cells [`Canvas::text`] takes to draw `text`.
pub fn text_width(text: &str) -> usize {
    text.nfc().map(width).sum()
}

/// The longest start of `text` that takes at most `max` cells, never ending
/// in half a wide character.
pub fn take_width(text: &str, max: usize) -> &str {
    let mut used = 0;
    for (i, ch) in text.char_indices() {
        used += width(ch);
        if used > max {
            return &text[..i];
        }
    }
    text
}

/// The longest end of `text` that takes at most `max` cells, never starting
/// with half a wide character.
pub fn take_width_back(text: &str, max: usize) -> &str {
    let mut used = 0;
    for (i, ch) in text.char_indices().rev() {
        used += width(ch);
        if used > max {
            return &text[i + ch.len_utf8()..];
        }
    }
    text
}

/// How many cells `text` takes once its markup is removed (generate.py's
/// `len(plain(text))`).
pub fn plain_len(text: &str) -> usize {
    markup(text)
        .iter()
        .filter(|p| matches!(p, Piece::Char(_)))
        .count()
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

#[cfg(test)]
impl Canvas {
    /// The cell at `(x, y)` as the goldens write it.
    pub(crate) fn golden_cell(&self, x: u16, y: u16) -> super::goldens::GoldenCell {
        use super::goldens::{GoldenCell, Rgb as G};
        let cell = self.get(x, y).expect("on the canvas");
        if cell.ch == HALF {
            GoldenCell::Half {
                top: Some(G(cell.fg.0)),
                bottom: Some(G(cell.bg.0)),
            }
        } else {
            GoldenCell::Text {
                ch: cell.ch,
                fg: G(cell.fg.0),
                bg: G(cell.bg.0),
            }
        }
    }

    /// Panics with the mismatch list unless every cell and the blink set
    /// equal the golden's.
    #[track_caller]
    pub(crate) fn assert_matches(&self, g: &super::goldens::Golden) {
        g.assert_matches(self.w, self.h, |x, y| self.golden_cell(x, y));
        let want: BTreeSet<(u16, u16)> = g.blink.iter().copied().collect();
        assert_eq!(self.blink, want, "{}: blink", g.name);
    }

    /// Panics if any cell holds a character the terminal would act on
    /// rather than draw (see [`printable`]).
    #[track_caller]
    pub(crate) fn assert_printable(&self) {
        for (i, cell) in self.cells.iter().enumerate() {
            let (x, y) = (i % usize::from(self.w), i / usize::from(self.w));
            assert_eq!(
                printable(cell.ch),
                cell.ch,
                "({x}, {y}) holds U+{:04X}",
                u32::from(cell.ch)
            );
        }
    }

    /// Panics unless every cell's character takes exactly the cells the
    /// canvas gives it ([`width`]): no zero-width character, and every wide
    /// one followed, on its own row, by the space that holds its second cell.
    /// A canvas that passes lines up in a terminal as it does here.
    #[track_caller]
    pub(crate) fn assert_aligned(&self) {
        for y in 0..self.h {
            let mut x = 0;
            while x < self.w {
                let ch = self.get(x, y).expect("cell").ch;
                match width(ch) {
                    1 => x += 1,
                    2 => {
                        let tail = self.get(x + 1, y).map(|c| c.ch);
                        assert_eq!(tail, Some(' '), "({x}, {y}): {ch:?} has no second cell");
                        x += 2;
                    }
                    n => panic!("({x}, {y}): {ch:?} takes {n} cells"),
                }
            }
        }
    }

    /// Panics unless `self`, drawn with wide or mixed-width names, equals
    /// `base`, the same view drawn with names as wide ([`stand_in`]), in
    /// every cell but the names' own: a cell may differ only where `base`
    /// holds [`STAND_IN`], `…` or a space and `self` a character of `chars`,
    /// `…` or a space. The layout around a name is then where it would be for
    /// a name of one-cell characters, and the canvas lines up
    /// ([`Canvas::assert_aligned`]).
    #[track_caller]
    pub(crate) fn assert_lines_up_with(&self, base: &Canvas, chars: &str) {
        assert_eq!((self.w, self.h), (base.w, base.h));
        self.assert_aligned();
        let mut bad = Vec::new();
        for y in 0..self.h {
            for x in 0..self.w {
                let (got, want) = (self.get(x, y).expect("cell"), base.get(x, y).expect("cell"));
                if got == want {
                    continue;
                }
                let name_cell = matches!(want.ch, '…' | ' ') || want.ch == STAND_IN;
                let wide_cell = matches!(got.ch, '…' | ' ') || chars.contains(got.ch);
                if !(name_cell && wide_cell) {
                    bad.push(format!("({x}, {y}) {:?} where {:?}", got.ch, want.ch));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{} cells moved:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }
}

/// File names of wide and mixed-width characters, each with the cells a
/// terminal gives it: CJK, emoji, a mix that puts a wide character on odd and
/// even cut positions, and long versions that every layout must cut.
/// [`stand_in`] gives each a name of the same width to compare it with.
#[cfg(test)]
pub(crate) const WIDE_NAMES: [(&str, usize); 6] = [
    ("报告书.pdf", 10),
    ("📄📎.pdf", 8),
    ("a证据b😀c.pdf", 13),
    (
        "调查报告调查报告调查报告调查报告调查报告调查报告调查报告调查报告.pdf",
        68,
    ),
    (
        "x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄x📄.pdf",
        58,
    ),
    ("证x", 3),
];

/// The one-cell character [`stand_in`] names are made of; no layout draws it.
#[cfg(test)]
pub(crate) const STAND_IN: char = '¤';

/// A name `cells` wide, all [`STAND_IN`].
#[cfg(test)]
pub(crate) fn stand_in(cells: usize) -> String {
    STAND_IN.to_string().repeat(cells)
}

/// Names a hostile file can carry (markup, ESC, a CSI sequence, BEL, the C1
/// CSI, the right-to-left override), each with how it is drawn.
#[cfg(test)]
pub(crate) const HOSTILE_NAMES: [(&str, &str); 6] = [
    ("{R}x.pdf", "{R}x.pdf"),
    ("a\u{1b}b.pdf", "a\u{fffd}b.pdf"),
    ("c\u{1b}[2Jd.pdf", "c\u{fffd}[2Jd.pdf"),
    ("e\u{7}f.pdf", "e\u{fffd}f.pdf"),
    ("g\u{9b}31m.pdf", "g\u{fffd}31m.pdf"),
    ("h\u{202e}fdp.exe", "h\u{fffd}fdp.exe"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::goldens;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    /// Every cell of the block `x0..x1 × y0..y1` that differs from the golden.
    fn region_diffs(
        c: &Canvas,
        g: &goldens::Golden,
        (x0, x1): (u16, u16),
        (y0, y1): (u16, u16),
    ) -> Vec<String> {
        let mut bad = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                let (want, got) = (g.cell(x, y), c.golden_cell(x, y));
                if want != got {
                    bad.push(format!("({x}, {y}) expected {want}, got {got}"));
                }
            }
        }
        bad
    }

    fn slot(k: char) -> Rgb {
        theme().slot(k).expect("slot")
    }

    /// Under a clip nothing lands past it, and the clip ends with `f`.
    #[test]
    fn clipped_writes_stay_inside_the_clip() {
        let t = theme();
        let blank = Canvas::new(6, 3, t);
        let mut c = blank.clone();
        c.clipped(3, 2, |c| {
            c.rich(1, 1, "abcdef", None, t);
            c.fill(0, 0, 9, 9, t.roles.dim, t.roles.dim);
            c.set_blink(4, 0);
            c.set_blink(2, 1);
        });
        for y in 0..3 {
            for x in 0..6 {
                let inside = x < 3 && y < 2;
                assert_eq!(c.get(x, y) != blank.get(x, y), inside, "({x}, {y})");
            }
        }
        assert_eq!(c.blink.iter().copied().collect::<Vec<_>>(), [(2, 1)]);
        c.put(5, 2, 'z', None, None);
        assert_eq!(c.get(5, 2).map(|k| k.ch), Some('z'));
    }

    #[test]
    fn a_new_canvas_is_blank_body_on_background() {
        let c = Canvas::new(3, 2, theme());
        assert_eq!(c.cells.len(), 6);
        for cell in &c.cells {
            assert_eq!(
                *cell,
                CanvasCell {
                    ch: ' ',
                    fg: slot('w'),
                    bg: slot('K'),
                }
            );
        }
        assert!(c.blink.is_empty());
    }

    #[test]
    fn a_pasted_canvas_lands_at_its_offset_blink_and_all() {
        let red = Some(slot('R'));
        let mut src = Canvas::new(3, 2, theme());
        src.put(0, 0, 'a', red, None);
        src.put(2, 1, 'b', None, red);
        src.set_blink(2, 1);
        let mut c = Canvas::new(5, 3, theme());
        c.paste(&src, 1, 1);
        let text: Vec<String> = (0..3)
            .map(|y| (0..5).map(|x| c.get(x, y).expect("cell").ch).collect())
            .collect();
        assert_eq!(text, ["     ", " a   ", "   b "]);
        assert_eq!(c.get(1, 1), src.get(0, 0));
        assert_eq!(c.get(3, 2), src.get(2, 1));
        assert_eq!(c.blink.iter().copied().collect::<Vec<_>>(), [(3, 2)]);

        // Past the edge: dropped.
        let mut small = Canvas::new(2, 2, theme());
        small.paste(&src, 1, 1);
        assert_eq!(small.get(1, 1).map(|k| k.ch), Some('a'));
        assert!(small.blink.is_empty());
        small.paste(&src, u16::MAX, u16::MAX);
        assert_eq!(small.cells.len(), 4);
    }

    #[test]
    fn writes_outside_the_canvas_are_dropped() {
        let mut c = Canvas::new(4, 2, theme());
        let before = c.clone();
        let red = Some(slot('R'));
        for (x, y) in [(-1, 0), (4, 0), (0, -1), (0, 2), (i32::MIN, i32::MAX)] {
            c.put(x, y, 'x', red, red);
            c.set_blink(x, y);
        }
        c.rich(2, 1, "{R}abcdef", None, theme());
        c.rich(-2, 0, "{R}abc", None, theme());
        c.gtext(-2, 1, "xyz", &[slot('R')], false, None);
        c.pix(3, 3, &[[Some(slot('R')), Some(slot('R'))]]);
        c.pix(-1, -1, &[[Some(slot('R'))]]);
        c.fill(-5, -5, 3, 3, slot('R'), slot('R'));
        c.boxed(-10, -10, 5, 5, &BoxStyle::default(), theme());
        assert_eq!(c.cells.len(), 8);
        assert!(c.blink.is_empty());
        // Only the in-range parts landed: "ab" at (2, 1) and (3, 1), "c" at
        // (0, 0), "z" at (0, 1).
        let changed: Vec<(u16, u16)> = (0..2)
            .flat_map(|y| (0..4).map(move |x| (x, y)))
            .filter(|&(x, y)| c.get(x, y) != before.get(x, y))
            .collect();
        assert_eq!(changed, vec![(0, 0), (0, 1), (2, 1), (3, 1)]);
        assert_eq!(c.get(0, 0).map(|c| c.ch), Some('c'));
        assert_eq!(c.get(0, 1).map(|c| c.ch), Some('z'));
    }

    #[test]
    fn markup_follows_the_mockups_tokens() {
        let mut c = Canvas::new(12, 1, theme());
        let e = c.rich(0, 0, "a{M}b{W/m}c{/_}d{}e{x/}f{", Some(slot('b')), theme());
        // "{x/}" is not a token (a slash needs a letter), so it is text.
        let text: String = c.cells.iter().map(|c| c.ch).collect();
        assert_eq!(text, "abcde{x/}f{ ");
        assert_eq!(e, 11);
        let cell = |x| c.get(x, 0).expect("cell");
        assert_eq!((cell(0).fg, cell(0).bg), (slot('w'), slot('b')));
        assert_eq!((cell(1).fg, cell(1).bg), (slot('M'), slot('b')));
        assert_eq!((cell(2).fg, cell(2).bg), (slot('W'), slot('m')));
        // {/_} keeps each cell's own background from here on.
        assert_eq!((cell(3).fg, cell(3).bg), (slot('W'), slot('K')));
        assert_eq!(plain_len("{W/m} thesis_ar.pdf +2 {/_}"), 18);
        assert_eq!(plain_len("{G}6√ {Y}1~ {R}1×"), 8);
    }

    #[test]
    fn text_over_a_pixel_pair_resets_its_background() {
        let mut c = Canvas::new(2, 1, theme());
        c.pix(0, 0, &[[Some(slot('R'))], [Some(slot('G'))]]);
        assert_eq!(
            c.get(0, 0),
            Some(CanvasCell {
                ch: HALF,
                fg: slot('R'),
                bg: slot('G'),
            })
        );
        c.put(0, 0, 'x', None, None);
        assert_eq!(
            c.get(0, 0),
            Some(CanvasCell {
                ch: 'x',
                fg: slot('R'),
                bg: slot('K'),
            })
        );
    }

    #[test]
    fn a_single_pixel_keeps_the_other_half() {
        let mut c = Canvas::new(1, 2, theme());
        c.put(0, 0, ' ', None, Some(slot('b')));
        // Over text, the missing half is the text cell's background.
        c.pix(0, 1, &[[Some(slot('R'))]]);
        let cell = c.get(0, 0).expect("cell");
        assert_eq!((cell.ch, cell.fg, cell.bg), (HALF, slot('b'), slot('R')));
        // Over a pixel pair, the other pixel stays.
        c.pix(0, 0, &[[Some(slot('G'))]]);
        let cell = c.get(0, 0).expect("cell");
        assert_eq!((cell.fg, cell.bg), (slot('G'), slot('R')));
        // A transparent pixel changes nothing; an odd start lands lower.
        let before = c.clone();
        c.pix(0, 0, &[[None]]);
        assert_eq!(c, before);
        c.pix(0, 3, &[[Some(slot('Y'))]]);
        let cell = c.get(0, 1).expect("cell");
        assert_eq!((cell.ch, cell.fg, cell.bg), (HALF, slot('K'), slot('Y')));
    }

    #[test]
    fn single_line_box_with_a_shadow() {
        let mut c = Canvas::new(8, 5, theme());
        c.put(5, 1, 'x', Some(slot('W')), Some(slot('b')));
        let style = BoxStyle {
            double: false,
            shadow: true,
            ..BoxStyle::default()
        };
        let b = c.boxed(0, 0, 5, 3, &style, theme());
        let row = |c: &Canvas, y: u16| -> String {
            (0..5).map(|x| c.get(x, y).expect("cell").ch).collect()
        };
        assert_eq!(row(&c, 0), "┌───┐");
        assert_eq!(row(&c, 1), "│   │");
        assert_eq!(row(&c, 2), "└───┘");
        assert_eq!(c.get(4, 0).map(|c| c.fg), Some(b.col(4, 0)));
        // The shadow mixes what is beside and under the box toward the
        // background: two columns on the right, one row below.
        let x = c.get(5, 1).expect("cell");
        assert_eq!(x.ch, 'x');
        assert_eq!(x.fg, mix(slot('W'), slot('K'), 0.75));
        assert_eq!(x.bg, mix(slot('b'), slot('K'), 0.75));
        let sep_stops = [slot('R'), slot('G')];
        b.sep(&mut c, 1, Some(&sep_stops), theme());
        assert_eq!(row(&c, 1), "├───┤");
        assert_eq!(c.get(2, 1).map(|c| c.fg), Some(slot('G')));
    }

    /// generate.py's `main_panels` LAST CALLERS and MENU boxes, drawn through
    /// `boxed`, `line`, `sep` and `rich`, equal the idle golden's cells; the
    /// tagline row tests `gtext`.
    #[test]
    fn idle_golden_boxes_and_tagline() {
        let g = goldens::load("01-idle");
        let t = theme();
        let mut c = Canvas::new(112, 38, t);
        let tag = "·∙· f u r e n s i c   p d f   r e p a i r ·∙·";
        let n = to_i32(tag.chars().count());
        c.gtext((112 - n) / 2, 7, tag, &t.gradients.tag, true, None);
        let lc = c.boxed(
            1,
            9,
            25,
            10,
            &BoxStyle {
                title: Some("LAST CALLERS"),
                ..BoxStyle::default()
            },
            t,
        );
        for (i, (ic, k, f)) in [
            ('√', 'G', "thesis_ar.pdf"),
            ('√', 'G', "minutes_q3.pdf"),
            ('~', 'Y', "invoice_scan.pdf"),
            ('·', 'w', "contract_signed.pdf"),
            ('×', 'R', "payroll_locked.pdf"),
        ]
        .into_iter()
        .enumerate()
        {
            lc.line(&mut c, 10 + to_i32(i), &format!(" {{{k}}}{ic} {{C}}{f}"), t);
        }
        lc.line(&mut c, 16, " {D}57 files · 91 runs", t);
        let mb = c.boxed(
            1,
            20,
            25,
            13,
            &BoxStyle {
                title: Some("MENU"),
                ..BoxStyle::default()
            },
            t,
        );
        for (i, (k, label)) in [
            ('B', "browse for pdfs"),
            ('H', "history"),
            ('S', "setup"),
            ('T', "theme"),
            ('?', "help"),
            ('Q', "quit"),
        ]
        .into_iter()
        .enumerate()
        {
            let hk = format!(" {{D}}[{{Y}}{k}{{D}}]{{w}} {label}");
            mb.line(&mut c, 21 + to_i32(i), &hk, t);
        }
        mb.sep(&mut c, 28, None, t);
        let e = mb.line(&mut c, 30, " {c}main {W}» ", t);
        c.put(e, 30, '█', Some(slot('w')), None);
        c.set_blink(e, 30);

        let mut bad = region_diffs(&c, &g, (0, 112), (7, 8));
        bad.extend(region_diffs(&c, &g, (1, 26), (9, 33)));
        assert!(
            bad.is_empty(),
            "{} cells differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
        assert!(c.blink.iter().all(|p| g.blink.contains(p)));
    }

    /// The result view's box carries a note at the right of its top edge.
    #[test]
    fn result_golden_box_note() {
        let g = goldens::load("05-result");
        let t = theme();
        let mut c = Canvas::new(112, 38, t);
        c.boxed(
            52,
            2,
            59,
            30,
            &BoxStyle {
                title: Some("RESULT » thesis_ar.pdf"),
                note: Some("{G}√ repaired"),
                ..BoxStyle::default()
            },
            t,
        );
        let bad = region_diffs(&c, &g, (52, 111), (2, 3));
        assert!(
            bad.is_empty(),
            "{} cells differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// Text drawn with [`Canvas::text`] never puts a C0 or C1 control, DEL
    /// or a bidi control in a cell; everything else is drawn as it is.
    #[test]
    fn text_replaces_controls_and_bidi_controls() {
        let t = theme();
        let mut replaced: Vec<char> = (0u32..0x20)
            .chain(0x7f..0xa0)
            .filter_map(char::from_u32)
            .collect();
        replaced.extend(['\u{200e}', '\u{200f}']);
        replaced.extend('\u{202a}'..='\u{202e}');
        replaced.extend('\u{2066}'..='\u{2069}');
        let kept = "a{R}é€\u{a0}\u{2010}\u{202f}";
        for ch in replaced {
            let mut c = Canvas::new(3, 1, t);
            let end = c.text(0, 0, &format!("a{ch}b"), t.roles.file, None);
            let row: String = c.cells.iter().map(|c| c.ch).collect();
            assert_eq!(row, "a\u{fffd}b", "U+{:04X}", u32::from(ch));
            assert_eq!(end, 3);
            c.assert_printable();
        }
        let n = kept.chars().count();
        let mut c = Canvas::new(u16::try_from(n).expect("short"), 1, t);
        c.text(0, 0, kept, t.roles.file, None);
        let row: String = c.cells.iter().map(|c| c.ch).collect();
        assert_eq!(row, kept);
    }

    /// The characters of row `y`.
    fn row(c: &Canvas, y: u16) -> String {
        (0..c.w).map(|x| c.get(x, y).expect("cell").ch).collect()
    }

    /// Every format character (Cf), the line and paragraph separators and
    /// the bidi controls are drawn as `�` in one cell (D-118): the soft
    /// hyphen, U+061C, the zero-width space and joiners, the BOM, the tag
    /// characters.
    #[test]
    fn text_replaces_format_characters_and_separators() {
        let t = theme();
        let hidden = [
            '\u{ad}',
            '\u{600}',
            '\u{61c}',
            '\u{6dd}',
            '\u{70f}',
            '\u{890}',
            '\u{8e2}',
            '\u{180e}',
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{2028}',
            '\u{2029}',
            '\u{2060}',
            '\u{2064}',
            '\u{206a}',
            '\u{206f}',
            '\u{feff}',
            '\u{fff9}',
            '\u{fffb}',
            '\u{110bd}',
            '\u{110cd}',
            '\u{13430}',
            '\u{1343f}',
            '\u{1bca0}',
            '\u{1d173}',
            '\u{e0001}',
            '\u{e0020}',
            '\u{e007f}',
        ];
        for ch in hidden {
            assert_eq!(printable(ch), '\u{fffd}', "U+{:04X}", u32::from(ch));
            let mut c = Canvas::new(3, 1, t);
            let end = c.text(0, 0, &format!("a{ch}b"), t.roles.file, None);
            assert_eq!(row(&c, 0), "a\u{fffd}b", "U+{:04X}", u32::from(ch));
            assert_eq!(end, 3);
        }
        // Their neighbours are drawn as they are.
        for ch in ['\u{ac}', '\u{ae}', '\u{2027}', '\u{202f}', '\u{fffc}'] {
            assert_eq!(printable(ch), ch, "U+{:04X}", u32::from(ch));
        }
    }

    /// A wide character (CJK, most emoji) takes its cell and the one after
    /// it, which holds a space in the same colours; the next character starts
    /// after both, and the returned column counts cells, not characters.
    #[test]
    fn a_wide_character_takes_two_cells() {
        let t = theme();
        let red = slot('R');
        let mut c = Canvas::new(8, 1, t);
        let end = c.text(0, 0, "a中📄b", red, Some(slot('b')));
        assert_eq!(row(&c, 0), "a中 📄 b  ");
        assert_eq!(end, 6);
        for x in 0..6 {
            let cell = c.get(x, 0).expect("cell");
            assert_eq!((cell.fg, cell.bg), (red, slot('b')), "column {x}");
        }
        c.assert_aligned();
        assert_eq!(text_width("a中📄b"), 6);
        for (name, cells) in WIDE_NAMES {
            assert_eq!(text_width(name), cells, "{name:?}");
        }
        assert_eq!((width('中'), width('a'), width('\u{301}')), (2, 1, 0));
    }

    /// Zero-width characters (combining marks with no precomposed form,
    /// variation selectors) take no cell and are dropped; a decomposed
    /// accent is composed first (`e` + U+0301 is `é`), so a name the file
    /// system stores decomposed keeps its accent.
    #[test]
    fn zero_width_characters_are_dropped_after_composing() {
        let t = theme();
        let mut c = Canvas::new(6, 1, t);
        let end = c.text(0, 0, "q\u{301}\u{fe0f}x", t.roles.file, None);
        assert_eq!(row(&c, 0), "qx    ");
        assert_eq!(end, 2);
        let mut c = Canvas::new(6, 1, t);
        let end = c.text(0, 0, "cafe\u{301}!", t.roles.file, None);
        assert_eq!(row(&c, 0), "café! ");
        assert_eq!(end, 5);
        assert_eq!(text_width("cafe\u{301}!"), 5);
        assert_eq!(text_width("q\u{301}\u{fe0f}x"), 2);
        c.assert_aligned();
    }

    /// A wide character with one cell left before the canvas edge or a clip
    /// is never split: that cell gets a space and nothing lands past it.
    #[test]
    fn a_wide_character_at_the_edge_is_not_split() {
        let t = theme();
        let mut c = Canvas::new(3, 1, t);
        let end = c.text(0, 0, "ab中", t.roles.file, Some(slot('b')));
        assert_eq!(row(&c, 0), "ab ");
        assert_eq!(c.get(2, 0).map(|k| k.bg), Some(slot('b')));
        assert_eq!(end, 4);
        c.assert_aligned();
        let mut c = Canvas::new(5, 1, t);
        c.clipped(3, 1, |c| c.text(0, 0, "ab中中", t.roles.file, None));
        assert_eq!(row(&c, 0), "ab   ");
        c.assert_aligned();
        // A raw `put` keeps to the same rule.
        let mut c = Canvas::new(2, 1, t);
        c.put(1, 0, '中', None, None);
        assert_eq!(row(&c, 0), "  ");
    }

    /// Writing over either half of a wide character removes all of it: a
    /// write over its second cell blanks the first, so the terminal never
    /// draws a wide glyph over the new cell.
    #[test]
    fn writing_over_half_a_wide_character_removes_it() {
        let t = theme();
        let mut c = Canvas::new(4, 1, t);
        c.text(0, 0, "中中", t.roles.file, None);
        c.put(1, 0, 'x', None, None);
        assert_eq!(row(&c, 0), " x中 ");
        c.assert_aligned();
        c.pix(3, 0, &[[Some(slot('R'))]]);
        assert_eq!(row(&c, 0), format!(" x {HALF}"));
        c.assert_aligned();
        c.text(0, 0, "中", t.roles.file, None);
        c.put(0, 0, 'y', None, None);
        assert_eq!(row(&c, 0), format!("y  {HALF}"));
        c.assert_aligned();
    }

    #[test]
    fn bar_rounds_half_to_even_and_trails_off() {
        let t = theme();
        let mut c = Canvas::new(6, 1, t);
        let stops = [slot('R'), slot('G')];
        // 4 × 0.625 = 2.5 rounds to 2.
        c.bar(0, 0, 4, 0.625, &stops, t);
        let text: String = c.cells.iter().map(|c| c.ch).collect();
        assert_eq!(text, "██▓▒  ");
        assert_eq!(c.get(0, 0).map(|c| c.fg), Some(ramp(&stops, 0.0)));
        assert_eq!(c.get(2, 0).map(|c| c.fg), Some(slot('G')));
        c.bar(0, 0, 6, 0.0, &stops, t);
        let text: String = c.cells.iter().map(|c| c.ch).collect();
        assert_eq!(text, "▓▒····");
        assert_eq!(c.get(5, 0).map(|c| c.fg), Some(t.roles.dim));
    }
}
