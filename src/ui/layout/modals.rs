//! Modals (T-24): the font pick (MOCK frame 04) and the theme chooser (frame
//! 06), ported from generate.py's `frame_fontpick` and `frame_themes`, with
//! the keyboard model of each as a pure function.
//!
//! A modal is drawn over the full layout's screen (112 × 38): it steps the
//! screen behind it back toward the background, as generate.py's `dimmed`
//! does, and draws its box on top. The status bar row is the screen's, not
//! the modal's: the loop redraws it after the modal with
//! [`strings::RESOLVING_FILE`] or [`strings::CHOOSING_THEME`] as its state.

use super::FULL_SIZE;
use crate::engine::{FontPickRequest, FontSlot, InteractionReply, Ratio, ToUnicodeState};
use crate::jobs::QueueEntry;
use crate::ui::canvas::{BoxStyle, Boxed, Canvas, HALF, plain_len, text_width};
use crate::ui::color::{Rgb, mix, ramp};
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::widgets::r#box::{fit, pad};

/// A key as the modals read it; the loop maps the terminal's keys onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalKey {
    Up,
    Down,
    Enter,
    Esc,
    Char(char),
}

/// generate.py's `dimmed`: every colour behind a modal steps back toward the
/// background by `f`, except a text cell's background, which steps back by
/// 0.85; nothing behind a modal blinks. Only the full layout's 112 × 38 is
/// stepped back: a larger terminal's margin is left as it is.
pub(super) fn step_back(c: &mut Canvas, f: f64, bg: Rgb) {
    let (w, h) = (c.w.min(FULL_SIZE.0), c.h.min(FULL_SIZE.1));
    for y in 0..h {
        let row = usize::from(y) * usize::from(c.w);
        for cell in &mut c.cells[row..row + usize::from(w)] {
            let back = if cell.ch == HALF { f } else { 0.85 };
            cell.fg = mix(cell.fg, bg, f);
            cell.bg = mix(cell.bg, bg, back);
        }
    }
    c.blink.retain(|&(x, y)| x >= w || y >= h);
}

/// The style every modal's box shares (the browse picker's too): the theme's
/// modal gradient, the title on the accent, a drop shadow.
pub(super) fn modal_style<'a>(
    title: &'a str,
    note: Option<&'a str>,
    theme: &'a Theme,
) -> BoxStyle<'a> {
    BoxStyle {
        title: Some(title),
        note,
        grad: Some(&theme.gradients.modal),
        title_colours: ('W', 'm'),
        shadow: true,
        ..BoxStyle::default()
    }
}

/// `n / d` rounded to the nearest integer, ties to even (Python's `round`).
fn round_div(n: u128, d: u128) -> u128 {
    let (q, r) = (n / d, n % d);
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

/// `r`, clamped to [0, 1], as `(num, den)` with a non-zero `den`.
fn unit(r: Ratio) -> (u128, u128) {
    match (u128::from(r.num), u128::from(r.den)) {
        (_, 0) if r.num == 0 => (0, 1),
        (_, 0) => (1, 1),
        (n, d) => (n.min(d), d),
    }
}

/// `r` with two decimals, as generate.py's `f'{fit:.2f}'` (exact integer
/// rounding: no float reaches the text).
fn two_places(r: Ratio) -> String {
    let (n, d) = unit(r);
    let h = round_div(n * 100, d);
    format!("{}.{:02}", h / 100, h % 100)
}

/// generate.py's `vu`: a `w`-cell meter, `round(w · r)` cells of `■` along
/// the theme's VU gradient, then dim dots.
fn vu(c: &mut Canvas, x: i32, y: i32, w: i32, r: Ratio, theme: &Theme) {
    let (n, d) = unit(r);
    let width = u128::try_from(w.max(0)).unwrap_or(0);
    let lit = i32::try_from(round_div(width * n, d)).unwrap_or(0);
    let last = f64::from((w - 1).max(1));
    for i in 0..w {
        if i < lit {
            let col = ramp(&theme.gradients.vu, f64::from(i) / last);
            c.put(x + i, y, '■', Some(col), None);
        } else {
            c.put(x + i, y, '·', Some(theme.roles.dim), None);
        }
    }
}

/// Literal pieces of text, each in its colour, from `(x, y)` on `bg`; text
/// from a file (a slot or font name) is never read as markup. Returns the
/// column after the last piece.
fn pieces(c: &mut Canvas, x: i32, y: i32, parts: &[(Rgb, &str)], bg: Option<Rgb>) -> i32 {
    parts
        .iter()
        .fold(x, |cx, &(fg, text)| c.text(cx, y, text, fg, bg))
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// The font pick's context: which file asks, and what its analysis knows
/// about the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontPickModal<'a> {
    /// The parked file's name.
    pub file: &'a str,
    /// The slot the question is about, as the analysis listed it.
    pub slot: Option<&'a FontSlot>,
    /// Which of the parked font questions this is and how many there are,
    /// from 1; `None` leaves the count out.
    pub question: Option<(usize, usize)>,
    /// More than one question is parked: the keys row offers "best for all".
    pub all: bool,
    /// The question is about a font no bundled font reproduces, for this
    /// reason (the engine's text, drawn literally): its options are
    /// substitutes with no score or preview, `b` takes the generic one, and
    /// Skip leaves the font as found rather than keeping a best guess.
    pub unreproducible: Option<&'a str>,
}

/// What a key does in the font pick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontPickAction {
    /// Move the cursor to this candidate.
    Select(usize),
    /// Answer the question.
    Reply(InteractionReply),
    /// Answer every parked font question with the best guess ("apply best
    /// to all", TD:469-471).
    BestForAll,
    /// Close the modal; the file stays parked and the question stays open.
    Later,
    /// The key does nothing here.
    Ignore,
}

/// The font pick's box: x, y, w, h.
const PICK_BOX: (i32, i32, i32, i32) = (8, 3, 96, 29);
/// The candidate rows: at most five (TD:424), three rows apart from row 10.
const CANDIDATES: usize = 5;
const CANDIDATE_Y: i32 = 10;
/// The columns: the table's heads, the name, the meter, the numbers, the
/// best-guess mark, the preview's label and the preview.
const HEAD_XS: [i32; 4] = [12, 36, 64, 70];
const NAME_X: i32 = 12;
const NAME_W: usize = 22;
const METER: (i32, i32) = (36, 26);
const NUMBERS_X: i32 = 64;
const BEST_X: i32 = 78;
const PREVIEW_LABEL_X: i32 = 14;
const PREVIEW: (i32, i32) = (23, 78);
/// The source row, the buttons and the keys.
const SOURCE_Y: i32 = 26;
const BUTTONS_Y: i32 = 28;
const BUTTON_XS: [i32; 3] = [11, 22, 47];
const KEYS_AT: (i32, i32) = (10, 30);

impl<'a> FontPickModal<'a> {
    /// The context for `req` asked about `entry`. The file's questions are
    /// its slots whose font program was lost (not embedded), in the order
    /// the analysis listed them; `req`'s slot is matched by page and name,
    /// then by name alone.
    pub fn for_entry(entry: &'a QueueEntry, req: &FontPickRequest) -> FontPickModal<'a> {
        let lost: Vec<&FontSlot> = entry
            .font_resolutions
            .iter()
            .filter(|s| !s.embedded)
            .collect();
        let at = lost
            .iter()
            .position(|s| s.slot == req.slot && s.page == req.page)
            .or_else(|| lost.iter().position(|s| s.slot == req.slot));
        FontPickModal {
            file: &entry.name,
            slot: at.map(|i| lost[i]),
            question: at.map(|i| (i + 1, lost.len())),
            all: false,
            unreproducible: None,
        }
    }

    /// Steps the screen in `c` back and draws the question over it, the
    /// cursor on candidate `selected` (clamped to the candidates shown).
    pub fn draw(&self, c: &mut Canvas, req: &FontPickRequest, selected: usize, theme: &Theme) {
        step_back(c, 0.68, theme.roles.bg);
        c.clipped(FULL_SIZE.0, FULL_SIZE.1, |c| {
            self.draw_box(c, req, selected, theme)
        });
    }

    fn draw_box(&self, c: &mut Canvas, req: &FontPickRequest, selected: usize, theme: &Theme) {
        let (x, y, w, h) = PICK_BOX;
        let r = &theme.roles;
        let note = self.question.map(|(i, n)| {
            strings::N_OF_M
                .replacen("{n}", &format!("{{W}}{i}{{D}}"), 1)
                .replacen("{n}", &format!("{{W}}{n}"), 1)
        });
        // The title is drawn with blanks where the file's name goes, then the
        // name over them as literal text, cut to leave the top edge between
        // the title and the note.
        let label = if self.unreproducible.is_some() {
            strings::SUBSTITUTE_A_FONT
        } else {
            strings::PICK_A_FONT
        };
        let name_x = x + 3 + 1 + to_i32(label.chars().count()) + 1;
        let limit = match &note {
            Some(n) => x + w - 4 - (to_i32(plain_len(n)) + 2) - 1,
            None => x + w - 2,
        };
        let room = usize::try_from(limit - 2 - name_x).unwrap_or(0);
        let name = fit(self.file, room);
        let title = format!("{label} {}", " ".repeat(text_width(&name)));
        let b = c.boxed(
            x,
            y,
            w,
            h,
            &modal_style(&title, note.as_deref(), theme),
            theme,
        );
        c.text(name_x, y, &name, r.heading, Some(r.accent));

        let inside = u16::try_from(x + w - 1).unwrap_or(0);
        c.clipped(inside, u16::MAX, |c| {
            self.slot_line(c, &b, req, theme);
            let why = match self.slot.map(|s| s.tounicode) {
                Some(ToUnicodeState::Present) => Some(strings::WHY_TOUNICODE),
                Some(ToUnicodeState::Missing | ToUnicodeState::Unparsable) => {
                    Some(strings::WHY_NO_TOUNICODE)
                }
                None => None,
            };
            match (self.unreproducible, why) {
                (Some(reason), _) => {
                    let at = b.line(c, y + 3, " ", theme);
                    let room = usize::try_from(x + w - 2 - at).unwrap_or(0);
                    c.text(at, y + 3, &fit(reason, room), r.body, None);
                }
                (None, Some(why)) => {
                    b.line(c, y + 3, &format!(" {why}"), theme);
                }
                (None, None) => {}
            }
            let ask = if self.unreproducible.is_some() {
                strings::PICK_A_SUBSTITUTE
            } else {
                strings::PICK_THE_CANDIDATE
            };
            b.line(c, y + 4, &format!(" {ask}"), theme);
        });

        let sep = [r.bg, r.accent, r.needs_input];
        b.sep(c, y + 5, Some(&sep), theme);
        // A substitute has no score, fit, confidence or preview.
        let plain = self.unreproducible.is_some();
        let heads = if plain { 1 } else { HEAD_XS.len() };
        for (hx, head) in HEAD_XS
            .into_iter()
            .zip(strings::CANDIDATE_COLUMNS)
            .take(heads)
        {
            c.text(hx, y + 6, head, r.dim, None);
        }
        let shown = req.candidates.len().min(CANDIDATES);
        let selected = selected.min(shown.saturating_sub(1));
        for (i, cand) in req.candidates.iter().take(CANDIDATES).enumerate() {
            let ry = CANDIDATE_Y + 3 * to_i32(i);
            let sel = i == selected;
            if sel {
                c.fill(x + 1, ry, w - 2, 2, r.body, r.lightbar);
            }
            let (mark, name_fg) = if sel {
                ("{Y}► ", r.heading)
            } else {
                ("  ", r.body)
            };
            c.rich(NAME_X - 2, ry, mark, None, theme);
            let family = pad(&cand.family, NAME_W);
            c.text(NAME_X, ry, &family, name_fg, None);
            if plain {
                continue;
            }
            vu(c, METER.0, ry, METER.1, cand.score, theme);
            let k = if sel { 'W' } else { 'D' };
            let numbers = format!(
                "{{W}}{}  {{{k}}}{}",
                two_places(cand.score),
                two_places(cand.confidence)
            );
            c.rich(NUMBERS_X, ry, &numbers, None, theme);
            if i == 0 {
                c.text(BEST_X, ry, strings::BEST_GUESS, r.hotkey, None);
            }
            c.text(PREVIEW_LABEL_X, ry + 1, strings::PREVIEW, r.dim, None);
            // The top three by rank stay readable; the rest step back.
            let (fg, bg) = match (sel, i < 3) {
                (true, _) => (r.heading, r.lightbar),
                (false, true) => (r.body, r.bg),
                (false, false) => (r.dim, r.bg),
            };
            let (px, pw) = PREVIEW;
            c.fill(px, ry + 1, pw, 1, fg, bg);
            let width = usize::try_from(pw).unwrap_or(0);
            c.text(px, ry + 1, &fit(&cand.preview, width), fg, Some(bg));
        }

        b.sep(c, SOURCE_Y - 1, Some(&sep), theme);
        b.line(c, SOURCE_Y, &format!(" {}", strings::FONT_SOURCE), theme);
        let buttons = if plain {
            [
                strings::PICK_BUTTON,
                strings::USE_GENERIC_BUTTON,
                strings::LEAVE_BUTTON,
            ]
        } else {
            [
                strings::PICK_BUTTON,
                strings::USE_BEST_BUTTON,
                strings::SKIP_BUTTON,
            ]
        };
        for (bx, button) in BUTTON_XS.into_iter().zip(buttons) {
            c.rich(bx, BUTTONS_Y, button, None, theme);
        }
        let keys = match (plain, self.all) {
            (false, false) => strings::FONT_PICK_KEYS,
            (false, true) => strings::FONT_PICK_KEYS_ALL,
            (true, false) => strings::SUBSTITUTE_KEYS,
            (true, true) => strings::SUBSTITUTE_KEYS_ALL,
        };
        c.rich(KEYS_AT.0, KEYS_AT.1, keys, None, theme);
    }

    /// `slot F3 (CIDFont+F1) · first seen p.12 · 418 glyph codes · language
    /// guess arabic`: the base font, the glyph count and the language only
    /// when known. Pages count from 0 in the request and from 1 on screen.
    fn slot_line(&self, c: &mut Canvas, b: &Boxed, req: &FontPickRequest, theme: &Theme) {
        let r = &theme.roles;
        let row = b.y + 2;
        let x = c.rich(b.x + 1, row, " ", Some(b.fill), theme);
        let mut parts = vec![
            (r.dim, format!("{} ", strings::SLOT)),
            (r.heading, req.slot.clone()),
        ];
        let base = self
            .slot
            .and_then(|s| s.base_font.as_deref())
            .map(|f| f.strip_prefix('/').unwrap_or(f));
        if let Some(base) = base {
            // The space before the bracket is the slot's colour, as drawn.
            parts.push((r.heading, " ".into()));
            parts.push((r.dim, "(".into()));
            parts.push((r.body, base.into()));
            parts.push((r.dim, ")".into()));
        }
        parts.push((r.dim, format!(" · {}", strings::FIRST_SEEN_P)));
        parts.push((r.heading, (u64::from(req.page) + 1).to_string()));
        if let Some(s) = self.slot {
            parts.push((r.dim, " · ".into()));
            parts.push((r.heading, s.glyph_count.to_string()));
            parts.push((r.dim, format!(" {}", strings::GLYPH_CODES)));
        }
        let lang = req.candidates.first().map(|c| c.language.as_str());
        if let Some(lang) = lang.filter(|l| !l.is_empty()) {
            let name = strings::LANGUAGES
                .iter()
                .find(|(code, _)| *code == lang)
                .map_or(lang, |(_, name)| name);
            parts.push((r.dim, format!(" · {} ", strings::LANGUAGE_GUESS)));
            parts.push((r.heading, name.into()));
        }
        let parts: Vec<(Rgb, &str)> = parts.iter().map(|(fg, t)| (*fg, t.as_str())).collect();
        pieces(c, x, row, &parts, Some(b.fill));
    }

    /// The keyboard model: `↑`/`↓` move the cursor over the candidates shown
    /// (stopping at either end), Enter picks the one under it, `b` takes the
    /// best guess (the loop applies it to the file's other open question too:
    /// "use best for both"), `s` skips (the best guess, the finding marked
    /// partial, TD §5.3), `a` takes the best guess for every parked question
    /// ("apply best to all"), Esc closes the modal and leaves the file parked.
    /// `tab` would switch the font source the modal draws, but v1 has only the
    /// bundled fonts (system fonts wait on M5, D-035), so the model has no key
    /// for it.
    pub fn key(req: &FontPickRequest, selected: usize, key: ModalKey) -> FontPickAction {
        let last = req.candidates.len().min(CANDIDATES).checked_sub(1);
        let sel = last.map(|l| selected.min(l));
        match (key, sel, last) {
            (ModalKey::Up, Some(s), _) => FontPickAction::Select(s.saturating_sub(1)),
            (ModalKey::Down, Some(s), Some(l)) => FontPickAction::Select((s + 1).min(l)),
            (ModalKey::Enter, Some(s), _) => {
                FontPickAction::Reply(InteractionReply::Pick(req.candidates[s].font_id.clone()))
            }
            (ModalKey::Char('b' | 'B'), _, _) => FontPickAction::Reply(InteractionReply::UseBest),
            (ModalKey::Char('s' | 'S'), _, _) => FontPickAction::Reply(InteractionReply::Skip),
            (ModalKey::Char('a' | 'A'), _, _) => FontPickAction::BestForAll,
            (ModalKey::Esc, _, _) => FontPickAction::Later,
            _ => FontPickAction::Ignore,
        }
    }
}

/// The theme chooser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThemeChooser;

/// What a key does in the theme chooser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeAction {
    /// Move the cursor to this theme and recolour the screen in it.
    Preview(usize),
    /// Keep this theme and close the chooser.
    Apply(usize),
    /// Close the chooser and go back to the theme it opened on.
    Cancel,
    /// The key does nothing here.
    Ignore,
}

/// The theme chooser's box: x, y, w, h; the column between the list and the
/// preview pane.
const THEME_BOX: (i32, i32, i32, i32) = (4, 2, 104, 33);
const DIVIDER_X: i32 = 38;
/// The list: seven themes, three rows apart from row 4.
const LIST_ROWS: usize = 7;
const LIST_Y: i32 = 4;
/// The preview pane's column.
const PANE_X: i32 = 40;

impl ThemeChooser {
    /// Where the cursor starts: on `current`, else on the first theme.
    pub fn open(themes: &[Theme], current: &Theme) -> usize {
        themes
            .iter()
            .position(|t| t.name == current.name)
            .unwrap_or(0)
    }

    /// Steps the screen in `c` back and draws the chooser over it, the cursor
    /// on theme `selected` (clamped). The chooser previews live: it is drawn
    /// in the theme under the cursor, which is the theme the loop draws the
    /// screen behind it in.
    pub fn draw(c: &mut Canvas, themes: &[Theme], selected: usize) {
        let Some(last) = themes.len().checked_sub(1) else {
            return;
        };
        let selected = selected.min(last);
        let th = &themes[selected];
        step_back(c, 0.8, th.roles.bg);
        c.clipped(FULL_SIZE.0, FULL_SIZE.1, |c| {
            let b = Self::frame(c, themes.len(), th);
            Self::list(c, themes, selected, th);
            Self::pane(c, th);
            c.rich(PANE_X, b.y + b.h - 2, strings::THEME_KEYS, None, th);
        });
    }

    /// The box and the divider between the list and the pane.
    fn frame(c: &mut Canvas, n: usize, th: &Theme) -> Boxed {
        let (x, y, w, h) = THEME_BOX;
        let note = format!(
            "{} · {{W}}{}",
            strings::N_THEMES.replace("{n}", &format!("{{W}}{n}{{D}}")),
            strings::LIVE_PREVIEW
        );
        let b = c.boxed(
            x,
            y,
            w,
            h,
            &modal_style(strings::THEME_TITLE, Some(&note), th),
            th,
        );
        let line = mix(th.roles.dim, th.roles.bg, 0.2);
        for j in y + 1..y + h - 1 {
            c.put(DIVIDER_X, j, '│', Some(line), None);
        }
        let dx = DIVIDER_X - x;
        c.put(DIVIDER_X, y, '╤', Some(b.col(dx, 0)), None);
        c.put(DIVIDER_X, y + h - 1, '╧', Some(b.col(dx, h - 1)), None);
        b
    }

    /// Each theme's name, its ANSI pairs as half blocks (bright over normal)
    /// and its logo gradient; the default starred; the cursor on the
    /// lightbar. Scrolls to keep the cursor in view past seven themes.
    fn list(c: &mut Canvas, themes: &[Theme], selected: usize, th: &Theme) {
        let r = &th.roles;
        let first = selected.saturating_sub(LIST_ROWS - 1);
        let default = Theme::default_theme().name;
        for (row, (i, t)) in themes
            .iter()
            .enumerate()
            .skip(first)
            .take(LIST_ROWS)
            .enumerate()
        {
            let ry = LIST_Y + 3 * to_i32(row);
            let sel = i == selected;
            if sel {
                c.fill(5, ry, 33, 2, r.body, r.lightbar);
            }
            let (mark, fg) = if sel {
                ("{Y}► ", r.heading)
            } else {
                ("  ", r.body)
            };
            c.rich(6, ry, mark, None, th);
            let name = format!("{} ", pad(t.name, 22));
            let e = c.text(8, ry, &name, fg, None);
            if t.name == default {
                c.text(e, ry, strings::DEFAULT_MARK, r.hotkey, None);
            } else {
                c.text(e, ry, " ", fg, None);
            }
            for k in 0..8 {
                let pair = [[Some(t.ansi[k + 8])], [Some(t.ansi[k])]];
                c.pix(8 + to_i32(k), 2 * (ry + 1), &pair);
            }
            for k in 0..18 {
                let col = ramp(&t.logo_ramp, f64::from(k) / 17.0);
                c.put(17 + k, ry + 1, '█', Some(col), None);
            }
        }
        for (j, &(lx, line)) in strings::THEME_FILE_LINES.iter().enumerate() {
            // A blank row between the file's lines and the copy key.
            let gap = if j >= 4 { 1 } else { 0 };
            c.rich(lx, 26 + to_i32(j) + gap, line, None, th);
        }
    }

    /// The theme under the cursor: its name and line, what it can be drawn
    /// in, its source, its roles with their colours, its gradients, its ANSI
    /// colours and a sample queue drawn in it.
    fn pane(c: &mut Canvas, th: &Theme) {
        let r = &th.roles;
        let x = PANE_X;
        let desc = strings::THEME_NAMES
            .iter()
            .position(|&n| n == th.name)
            .map(|i| strings::THEME_DESCS[i]);
        let mut head = vec![(r.heading, format!("{} ", th.name))];
        if let Some(desc) = desc {
            head.push((r.dim, "· ".into()));
            head.push((r.body, desc.into()));
        }
        let head: Vec<(Rgb, &str)> = head.iter().map(|(fg, t)| (*fg, t.as_str())).collect();
        pieces(c, x, 4, &head, None);
        c.rich(x, 5, strings::COLOUR_SUPPORT, None, th);
        if let Some(v) = th.palette_version() {
            c.rich(x, 6, &format!("{}{v}", strings::PALETTE_SOURCE), None, th);
        }

        c.text(x, 7, strings::ROLES, r.heading, None);
        for (col_x, items) in [x, x + 33].into_iter().zip(strings::ROLE_LABELS) {
            for (i, (k, label)) in items.into_iter().enumerate() {
                let ry = 8 + to_i32(i);
                let col = th.slot(k).unwrap_or(r.body);
                c.put(col_x, ry, '█', Some(col), None);
                c.put(col_x + 1, ry, '█', Some(col), None);
                let text = if k == 'b' {
                    format!("{{W/b}} {label} {{/K}}")
                } else {
                    format!("{{{k}}}{label}")
                };
                c.rich(col_x + 3, ry, &text, None, th);
                c.text(col_x + 22, ry, &col.to_string(), r.dim, None);
            }
        }

        c.text(x, 13, strings::GRADIENTS, r.heading, None);
        let ramps: [&[Rgb]; 5] = [
            &r.border_stops,
            &th.logo_ramp,
            &th.gradients.bar,
            &th.gradients.modal,
            &th.gradients.bar2,
        ];
        for (i, (label, stops)) in strings::GRADIENT_LABELS.iter().zip(ramps).enumerate() {
            let ry = 14 + to_i32(i);
            c.text(x, ry, &format!("{label:<10}"), r.dim, None);
            for k in 0..54 {
                let col = ramp(stops, f64::from(k) / 53.0);
                c.put(x + 11 + k, ry, '█', Some(col), None);
            }
        }

        c.text(x, 20, strings::ANSI_0_15, r.heading, None);
        for (k, name) in strings::ANSI_NAMES.iter().enumerate() {
            let sx = x + 11 + 7 * to_i32(k);
            for dx in 0..5 {
                c.put(sx + dx, 21, '█', Some(th.ansi[k]), None);
                c.put(sx + dx, 22, '█', Some(th.ansi[k + 8]), None);
            }
            c.text(sx, 23, name, r.dim, None);
        }
        for (j, label) in strings::ANSI_ROWS.iter().enumerate() {
            c.text(x, 21 + to_i32(j), label, r.dim, None);
        }

        c.text(x, 25, strings::SAMPLE, r.heading, None);
        let style = BoxStyle {
            title: Some(strings::SAMPLE_TITLE),
            note: Some(strings::SAMPLE_NOTE),
            ..BoxStyle::default()
        };
        let sb = c.boxed(x, 26, 66, 7, &style, th);
        c.fill(sb.x + 1, sb.y + 1, sb.w - 2, 1, r.body, r.lightbar);
        for (j, row) in strings::SAMPLE_ROWS.iter().enumerate() {
            c.rich(x + 2, sb.y + 1 + to_i32(j), row, None, th);
        }
        c.bar(x + 2, sb.y + 5, 50, 0.71, &th.gradients.bar, th);
        c.rich(x + 54, sb.y + 5, strings::SAMPLE_PCT, None, th);
    }

    /// The keyboard model: `↑`/`↓` move the cursor (stopping at either end)
    /// and preview the theme under it, Enter applies it, Esc cancels back to
    /// the theme the chooser opened on (the loop keeps that one).
    pub fn key(themes: usize, selected: usize, key: ModalKey) -> ThemeAction {
        let Some(last) = themes.checked_sub(1) else {
            return match key {
                ModalKey::Esc => ThemeAction::Cancel,
                _ => ThemeAction::Ignore,
            };
        };
        let sel = selected.min(last);
        match key {
            ModalKey::Up => ThemeAction::Preview(sel.saturating_sub(1)),
            ModalKey::Down => ThemeAction::Preview((sel + 1).min(last)),
            ModalKey::Enter => ThemeAction::Apply(sel),
            ModalKey::Esc => ThemeAction::Cancel,
            _ => ThemeAction::Ignore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{FontCandidate, InteractionRequest, InteractionRequestId};
    use crate::jobs::EntryState;
    use crate::ui::canvas::{CanvasCell, HALF, printable};
    use crate::ui::color::Rgb;
    use crate::ui::goldens::{self, Golden, GoldenCell};
    use crate::ui::layout::FULL_SIZE;
    use crate::ui::state::AppState;
    use crate::ui::strings;
    use crate::ui::widgets::lightbar;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    /// A golden frame as a canvas, to draw a modal over.
    fn canvas_of(g: &Golden) -> Canvas {
        let mut c = Canvas::new(g.w, g.h, theme());
        for y in 0..g.h {
            for x in 0..g.w {
                let cell = match g.cell(x, y) {
                    GoldenCell::Text { ch, fg, bg } => CanvasCell {
                        ch,
                        fg: Rgb(fg.0),
                        bg: Rgb(bg.0),
                    },
                    GoldenCell::Half {
                        top: Some(t),
                        bottom: Some(b),
                    } => CanvasCell {
                        ch: HALF,
                        fg: Rgb(t.0),
                        bg: Rgb(b.0),
                    },
                    other => panic!("({x}, {y}): not a canvas cell: {other}"),
                };
                c.cells[usize::from(y) * usize::from(g.w) + usize::from(x)] = cell;
            }
        }
        c.blink = g.blink.iter().copied().collect();
        c
    }

    fn row_text(c: &Canvas, y: u16) -> String {
        (0..c.w).map(|x| c.get(x, y).expect("cell").ch).collect()
    }

    fn ratio(num: u64) -> Ratio {
        Ratio { num, den: 100 }
    }

    /// Frame 04's question: the five candidates generate.py draws, their
    /// previews the mockup's strings (the golden keeps them beside the
    /// placeholder).
    fn mockup_request() -> FontPickRequest {
        let g = goldens::load("04-font-pick");
        let spans = g.placeholder.expect("placeholder").spans;
        let rows = [
            ("noto-naskh-arabic", "Noto Naskh Arabic", "ar", 64, 31),
            ("noto-kufi-arabic", "Noto Kufi Arabic", "ar", 55, 22),
            ("noto-sans-arabic", "Noto Sans Arabic", "ar", 47, 18),
            ("noto-nastaliq-urdu", "Noto Nastaliq Urdu", "ur", 21, 7),
            ("noto-sans", "Noto Sans (latin)", "en", 4, 1),
        ];
        let candidates: Vec<FontCandidate> = rows
            .iter()
            .zip(&spans)
            .map(|(&(id, family, lang, score, conf), span)| FontCandidate {
                font_id: id.into(),
                family: family.into(),
                language: lang.into(),
                score: ratio(score),
                confidence: ratio(conf),
                preview: span.text.clone(),
            })
            .collect();
        FontPickRequest {
            id: InteractionRequestId(1),
            page: 11,
            slot: "F3".into(),
            sample_codes: Vec::new(),
            preview: candidates[0].preview.clone(),
            candidates,
        }
    }

    /// Frame 03's batch, `thesis_ar.pdf` parked on the question.
    fn mockup_entry() -> crate::jobs::QueueEntry {
        let app = AppState::mockup_batch();
        let e = app.batch.entries[2].clone();
        assert!(matches!(
            e.state,
            EntryState::WaitingOnUser(InteractionRequest::FontPick(_))
        ));
        e
    }

    /// The status bar as the loop draws it over a modal: the mockup's.
    fn status(c: &mut Canvas, parts: &[&str], right: &str) {
        let parts: Vec<String> = parts.iter().map(|p| p.to_string()).collect();
        lightbar::status_bar(c, 37, 112, &parts, right, theme());
    }

    #[test]
    fn theme_chooser_matches_the_golden() {
        let mut c = canvas_of(&goldens::load("01-idle"));
        let selected = ThemeChooser::open(Theme::all(), theme());
        ThemeChooser::draw(&mut c, Theme::all(), selected);
        status(
            &mut c,
            &[
                "node 1",
                &format!("{{M}}{}", strings::CHOOSING_THEME),
                "{G}offline",
            ],
            "{M}DarkBerry Blackwater {c}│{W} 112×38 {c}│{W} 11:38",
        );
        c.assert_matches(&goldens::load("06-theme-chooser"));
    }

    /// The previews are drawn as terminal text, one character a cell from the
    /// span's left edge in its colours (a format character as `�`). The
    /// golden replaces each embedded span's cells with the placeholder text
    /// (its placeholder policy, D-055): the drawn previews are checked,
    /// replaced the same way, and then every cell must equal the golden's.
    #[test]
    fn font_pick_matches_the_golden_with_the_placeholder_policy() {
        let golden = goldens::load("04-font-pick");
        let policy = golden.placeholder.clone().expect("placeholder");
        let entry = mockup_entry();
        let req = mockup_request();
        let mut c = canvas_of(&goldens::load("03-batch"));
        FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 0, theme());
        status(
            &mut c,
            &[
                "batch 4/7",
                &format!(
                    "{{M}}{}",
                    strings::RESOLVING_FILE.replace("{file}", "thesis_ar.pdf")
                ),
                "{R}1 failed",
                "{G}offline",
            ],
            "{M}DarkBerry Blackwater {c}│{W} 11:43",
        );
        let placeholder: Vec<char> = policy.text.chars().collect();
        for span in &policy.spans {
            let text: Vec<char> = span.text.chars().collect();
            for i in 0..span.w {
                let (x, y) = (span.x + i, span.y);
                let cell = c.get(x, y).expect("cell");
                // The mojibake preview's soft hyphen is a format character,
                // drawn as `�` (D-118).
                let drawn = text.get(usize::from(i)).copied().map_or(' ', printable);
                assert_eq!(
                    (cell.ch, cell.fg.0, cell.bg.0),
                    (drawn, span.fg.0, span.bg.0),
                    "span at ({}, {}) cell {i}",
                    span.x,
                    span.y
                );
                let ch = placeholder.get(usize::from(i)).copied().unwrap_or(' ');
                c.put(i32::from(x), i32::from(y), ch, None, None);
            }
        }
        c.assert_matches(&golden);
    }

    /// CJK, emoji and mixed-width text in the font question (the file's
    /// name, the candidates' families and their previews, which carry
    /// decoded PDF text) lines up (D-118): every cell around it is where it
    /// is for text of one-cell characters as wide, cut or not.
    #[test]
    fn wide_text_in_the_font_question_lines_up() {
        for (name, cells) in crate::ui::canvas::WIDE_NAMES {
            let drawn = |n: &str| {
                let mut entry = mockup_entry();
                entry.name = n.to_string();
                let mut req = mockup_request();
                for cand in &mut req.candidates {
                    cand.family = n.to_string();
                    cand.preview = n.repeat(4);
                }
                let mut c = canvas_of(&goldens::load("03-batch"));
                FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 1, theme());
                c
            };
            let c = drawn(name);
            c.assert_lines_up_with(&drawn(&crate::ui::canvas::stand_in(cells)), name);
            let first = name.chars().next().expect("a name");
            assert!(row_text(&c, 14).contains(first), "{:?}", row_text(&c, 14));
        }
    }

    fn one_candidate(id: &str, family: &str, preview: &str) -> FontCandidate {
        FontCandidate {
            font_id: id.into(),
            family: family.into(),
            language: "ar".into(),
            score: ratio(50),
            confidence: ratio(20),
            preview: preview.into(),
        }
    }

    fn blank() -> Canvas {
        Canvas::new(FULL_SIZE.0, FULL_SIZE.1, theme())
    }

    #[test]
    fn font_pick_keys() {
        use FontPickAction::{BestForAll, Ignore, Later, Reply, Select};
        use ModalKey::{Char, Down, Enter, Esc, Up};
        let req = mockup_request();
        let pick = |id: &str| Reply(InteractionReply::Pick(id.into()));
        for (selected, key, want) in [
            (0, Down, Select(1)),
            (3, Down, Select(4)),
            (4, Down, Select(4)),
            (2, Up, Select(1)),
            (0, Up, Select(0)),
            (9, Up, Select(3)),
            (0, Enter, pick("noto-naskh-arabic")),
            (2, Enter, pick("noto-sans-arabic")),
            (9, Enter, pick("noto-sans")),
            (3, Char('b'), Reply(InteractionReply::UseBest)),
            (3, Char('B'), Reply(InteractionReply::UseBest)),
            (1, Char('s'), Reply(InteractionReply::Skip)),
            (1, Char('S'), Reply(InteractionReply::Skip)),
            (1, Esc, Later),
            (1, Char('a'), BestForAll),
            (1, Char('A'), BestForAll),
            (1, Char('q'), Ignore),
            (1, Char('T'), Ignore),
        ] {
            assert_eq!(
                FontPickModal::key(&req, selected, key),
                want,
                "{selected} {key:?}"
            );
        }
        // Only five candidates are shown, so the cursor stops on the fifth.
        let mut six = req.clone();
        six.candidates.push(one_candidate("extra", "Extra", ""));
        assert_eq!(FontPickModal::key(&six, 4, Down), Select(4));
        // With no candidate there is nothing to move to or pick; the best
        // guess, skip and later still answer.
        let none = FontPickRequest {
            candidates: Vec::new(),
            ..req
        };
        for key in [Up, Down, Enter] {
            assert_eq!(FontPickModal::key(&none, 0, key), Ignore, "{key:?}");
        }
        assert_eq!(FontPickModal::key(&none, 0, Esc), Later);
        assert_eq!(FontPickModal::key(&none, 0, Char('a')), BestForAll);
        assert_eq!(
            FontPickModal::key(&none, 0, Char('b')),
            Reply(InteractionReply::UseBest)
        );
    }

    #[test]
    fn theme_chooser_keys() {
        use ModalKey::{Char, Down, Enter, Esc, Up};
        use ThemeAction::{Apply, Cancel, Ignore, Preview};
        for (selected, key, want) in [
            (0, Down, Preview(1)),
            (5, Down, Preview(6)),
            (6, Down, Preview(6)),
            (3, Up, Preview(2)),
            (0, Up, Preview(0)),
            (40, Up, Preview(5)),
            (2, Enter, Apply(2)),
            (40, Enter, Apply(6)),
            (2, Esc, Cancel),
            (2, Char('e'), Ignore),
            (2, Char('q'), Ignore),
        ] {
            assert_eq!(
                ThemeChooser::key(7, selected, key),
                want,
                "{selected} {key:?}"
            );
        }
        assert_eq!(ThemeChooser::key(0, 0, Down), Ignore);
        assert_eq!(ThemeChooser::key(0, 0, Enter), Ignore);
        assert_eq!(ThemeChooser::key(0, 0, Esc), Cancel);
    }

    #[test]
    fn the_chooser_opens_on_the_current_theme() {
        let all = Theme::all();
        assert_eq!(ThemeChooser::open(all, theme()), 0);
        assert_eq!(ThemeChooser::open(all, &all[4]), 4);
        assert_eq!(ThemeChooser::open(&all[..2], &all[4]), 0);
    }

    /// The question's count and the slot come from the file's lost slots: F3
    /// is the first of two (F1 is embedded), F7 the second, and a slot the
    /// analysis did not list has neither.
    #[test]
    fn the_question_is_counted_among_the_files_lost_slots() {
        let entry = mockup_entry();
        let mut req = mockup_request();
        let m = FontPickModal::for_entry(&entry, &req);
        assert_eq!(m.file, "thesis_ar.pdf");
        assert_eq!(m.question, Some((1, 2)));
        assert_eq!(m.slot.map(|s| s.slot.as_str()), Some("F3"));
        req.slot = "F7".into();
        assert_eq!(
            FontPickModal::for_entry(&entry, &req).question,
            Some((2, 2))
        );
        req.page = 40;
        assert_eq!(
            FontPickModal::for_entry(&entry, &req).question,
            Some((2, 2))
        );
        req.slot = "F9".into();
        let m = FontPickModal::for_entry(&entry, &req);
        assert_eq!((m.slot, m.question), (None, None));
        req.slot = "F1".into();
        assert_eq!(FontPickModal::for_entry(&entry, &req).question, None);
    }

    /// Moving the cursor moves the lightbar and the bright preview; the best
    /// guess stays on the top-ranked candidate.
    #[test]
    fn the_cursor_moves_the_lightbar_not_the_best_guess() {
        let entry = mockup_entry();
        let req = mockup_request();
        let r = &theme().roles;
        let mut c = blank();
        FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 2, theme());
        let cell = |x: u16, y: u16| c.get(x, y).expect("cell");
        assert_eq!(cell(10, 16).ch, '►');
        assert_eq!(cell(10, 10).ch, ' ');
        assert_eq!((cell(9, 16).bg, cell(9, 17).bg), (r.lightbar, r.lightbar));
        assert_eq!(cell(9, 10).bg, r.bg);
        assert_eq!((cell(23, 17).fg, cell(23, 17).bg), (r.heading, r.lightbar));
        assert_eq!((cell(23, 11).fg, cell(23, 11).bg), (r.body, r.bg));
        assert!(row_text(&c, 10).contains(strings::BEST_GUESS));
        assert!(!row_text(&c, 16).contains(strings::BEST_GUESS));
        // A cursor past the end is drawn on the last candidate shown.
        let mut past = blank();
        FontPickModal::for_entry(&entry, &req).draw(&mut past, &req, 99, theme());
        assert_eq!(past.get(10, 22).expect("cell").ch, '►');
    }

    /// Text from the file (its name, the slot's and the fonts' names, the
    /// previews) is drawn as it is, never read as markup, with control
    /// characters as `�`; long text is cut with `…` inside the box, and the
    /// count stays whole on the top edge.
    #[test]
    fn text_from_the_file_is_drawn_literally_and_cut_to_fit() {
        let mut entry = mockup_entry();
        entry.name = format!("{{R}}evil\u{1b}[2J{}.pdf", "x".repeat(80));
        let mut req = mockup_request();
        req.candidates[0].family = "{Y}Fake{/b} family name that runs on".into();
        req.candidates[1].preview = format!("a{{W}}b\u{7}c{}", "z".repeat(300));
        let mut c = blank();
        FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 0, theme());
        let top = row_text(&c, 3);
        assert!(
            top.contains("PiCK A FONT » {R}evil\u{fffd}[2Jxxx"),
            "{top:?}"
        );
        assert!(top.contains("…"), "{top:?}");
        assert!(top.contains(" 1 of 2 "), "{top:?}");
        assert_eq!(top.chars().nth(103), Some('╗'));
        let row = row_text(&c, 10);
        assert!(row.contains("► {Y}Fake{/b} family na… "), "{row:?}");
        let preview: String = row_text(&c, 14).chars().skip(23).take(78).collect();
        assert_eq!(preview.chars().count(), 78);
        assert!(preview.starts_with("a{W}b\u{fffd}czz"), "{preview:?}");
        assert!(preview.ends_with("z…"), "{preview:?}");
        assert_eq!(row_text(&c, 14).chars().nth(103), Some('║'));
    }

    /// A slot whose `/ToUnicode` was lost too (C8) is explained as such, and
    /// a slot without a base font or a listed slot leaves those out.
    #[test]
    fn the_slot_line_and_the_reason_follow_the_slot() {
        let mut entry = mockup_entry();
        let req = mockup_request();
        let mut c = blank();
        FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 0, theme());
        assert_eq!(
            row_text(&c, 5).trim_end().trim_end_matches('║').trim(),
            "║ slot F3 (CIDFont+F1) · first seen p.12 · 418 glyph codes · language guess arabic"
        );
        assert!(row_text(&c, 6).contains("surviving /ToUnicode"));

        let f3 = entry
            .font_resolutions
            .iter_mut()
            .find(|s| s.slot == "F3")
            .expect("F3");
        f3.tounicode = ToUnicodeState::Missing;
        f3.base_font = None;
        let mut c = blank();
        FontPickModal::for_entry(&entry, &req).draw(&mut c, &req, 0, theme());
        assert!(
            row_text(&c, 5).contains("║ slot F3 · first seen p.12 · 418 glyph codes"),
            "{:?}",
            row_text(&c, 5)
        );
        assert!(row_text(&c, 6).contains("No /ToUnicode survives"));

        let unknown = FontPickModal {
            file: "a.pdf",
            slot: None,
            question: None,
            all: false,
            unreproducible: None,
        };
        let mut other = req.clone();
        other.candidates[0].language = "ur".into();
        let mut c = blank();
        unknown.draw(&mut c, &other, 0, theme());
        let line = row_text(&c, 5);
        assert!(
            line.contains("║ slot F3 · first seen p.12 · language guess ur "),
            "{line:?}"
        );
        assert!(!row_text(&c, 3).contains(" of "));
        assert_eq!(
            row_text(&c, 6).trim_matches(|ch| ch == ' ' || ch == '║'),
            ""
        );
    }

    /// With more than one question parked, the keys row offers "best for
    /// all" in place of the parked note.
    #[test]
    fn the_keys_row_offers_best_for_all_when_asked() {
        let entry = mockup_entry();
        let req = mockup_request();
        let mut modal = FontPickModal::for_entry(&entry, &req);
        let mut c = blank();
        modal.draw(&mut c, &req, 0, theme());
        assert!(row_text(&c, 30).contains("(file stays parked)"));
        modal.all = true;
        let mut c = blank();
        modal.draw(&mut c, &req, 0, theme());
        let row = row_text(&c, 30);
        assert!(row.contains("· a best for all ·"), "{row:?}");
        assert_eq!(row.chars().nth(103), Some('║'), "inside the box");
    }

    /// A font no bundled font reproduces: its own title, the engine's reason
    /// on the why line, no meters or best-guess mark, and Skip says it
    /// leaves the font as found.
    #[test]
    fn an_unreproducible_font_says_what_skip_does() {
        let entry = mockup_entry();
        let mut req = mockup_request();
        for c in &mut req.candidates {
            c.preview = String::new();
        }
        let mut modal = FontPickModal::for_entry(&entry, &req);
        modal.unreproducible = Some("no {font} maps every character");
        let mut c = blank();
        modal.draw(&mut c, &req, 0, theme());
        let all: Vec<String> = (0..38).map(|y| row_text(&c, y)).collect();
        let text = all.join("\n");
        assert!(text.contains("SUBSTiTUTE A FONT »"), "{text}");
        assert!(!text.contains("PiCK A FONT"), "{text}");
        assert!(row_text(&c, 6).contains("no {font} maps every character"));
        assert!(text.contains("leaves the font as found"), "{text}");
        assert!(text.contains("Generic for both"), "{text}");
        assert!(text.contains("s leave as found"), "{text}");
        for gone in ["best guess", "keeps best guess", "score", "conf", "preview"] {
            assert!(!text.contains(gone), "{gone:?} in {text}");
        }
        modal.all = true;
        let mut c = blank();
        modal.draw(&mut c, &req, 0, theme());
        let row = row_text(&c, 30);
        assert!(row.contains("· a best for all ·"), "{row:?}");
        assert_eq!(row.chars().nth(103), Some('║'), "inside the box");
    }

    /// Live preview: the chooser is drawn in the theme under the cursor, the
    /// star stays on the default, and the palette source shows only for the
    /// DarkBerry flavours.
    #[test]
    fn the_chooser_is_drawn_in_the_theme_under_the_cursor() {
        let all = Theme::all();
        let acid = &all[4];
        let mut c = Canvas::new(112, 38, acid);
        ThemeChooser::draw(&mut c, all, 4);
        let cell = |x: u16, y: u16| c.get(x, y).expect("cell");
        assert_eq!(cell(4, 2).ch, '╔');
        assert_eq!(cell(4, 2).fg, acid.gradients.modal[0]);
        assert_eq!(cell(6, 16).ch, '►');
        assert_eq!(cell(5, 16).bg, acid.roles.lightbar);
        assert!(row_text(&c, 4).contains("DarkBerry Blackwater   ★"));
        assert!(row_text(&c, 4).contains("ACiD Classic · the original 16 · 1994"));
        assert!(!row_text(&c, 6).contains("palette.json"));
        assert_eq!(cell(40, 8).fg, acid.roles.heading);

        let mut c = blank();
        ThemeChooser::draw(&mut c, all, 1);
        assert!(row_text(&c, 6).contains("palette.json v0.3.0"));
        assert!(row_text(&c, 4).contains("DarkBerry Mire · bog-witch berry · dark"));
    }

    /// Past seven themes the list scrolls to keep the cursor in view.
    #[test]
    fn a_long_theme_list_scrolls_to_the_cursor() {
        let mut themes = Theme::all().to_vec();
        themes.extend(Theme::all()[..2].iter().cloned());
        let mut c = blank();
        ThemeChooser::draw(&mut c, &themes, 8);
        assert!(
            row_text(&c, 4).contains("  DarkBerry Fen"),
            "{:?}",
            row_text(&c, 4)
        );
        assert!(row_text(&c, 22).contains("► DarkBerry Mire"));
        // Nothing to choose from draws nothing.
        let mut empty = blank();
        ThemeChooser::draw(&mut empty, &[], 0);
        assert_eq!(empty, blank());
    }

    /// On a larger canvas both modals stay inside 112 × 38 and leave the rest
    /// alone, its blinking cells included.
    #[test]
    fn modals_stay_inside_112_by_38() {
        let entry = mockup_entry();
        let req = mockup_request();
        let mut before = Canvas::new(140, 50, theme());
        before.put(120, 45, 'x', Some(theme().roles.error), None);
        before.set_blink(120, 45);
        before.set_blink(3, 3);
        type Draw = Box<dyn Fn(&mut Canvas)>;
        let draws: [Draw; 2] = [
            Box::new(move |c| FontPickModal::for_entry(&entry, &req).draw(c, &req, 0, theme())),
            Box::new(|c| ThemeChooser::draw(c, Theme::all(), 0)),
        ];
        for draw in draws {
            let mut c = before.clone();
            draw(&mut c);
            for y in 0..50 {
                for x in 0..140 {
                    if x >= 112 || y >= 38 {
                        assert_eq!(c.get(x, y), before.get(x, y), "({x}, {y})");
                    }
                }
            }
            assert_eq!(c.blink.iter().copied().collect::<Vec<_>>(), [(120, 45)]);
        }
    }

    #[test]
    fn two_places_and_the_meter_round_like_the_mockup() {
        let r = |num, den| Ratio { num, den };
        assert_eq!(two_places(r(64, 100)), "0.64");
        assert_eq!(two_places(r(7, 100)), "0.07");
        assert_eq!(two_places(r(1, 3)), "0.33");
        assert_eq!(two_places(r(2, 3)), "0.67");
        assert_eq!(two_places(r(1, 200)), "0.00");
        assert_eq!(two_places(r(3, 200)), "0.02");
        assert_eq!(two_places(r(5, 4)), "1.00");
        assert_eq!(two_places(r(0, 0)), "0.00");
        assert_eq!(two_places(r(3, 0)), "1.00");
        let lit = |ratio: Ratio| {
            let mut c = Canvas::new(26, 1, theme());
            vu(&mut c, 0, 0, 26, ratio, theme());
            row_text(&c, 0).chars().filter(|&ch| ch == '■').count()
        };
        // 26 · 0.64 = 16.64 → 17; 26 · 0.5 = 13; 26 · 0.25 = 6.5 → 6 (even).
        assert_eq!(lit(r(64, 100)), 17);
        assert_eq!(lit(r(1, 2)), 13);
        assert_eq!(lit(r(1, 4)), 6);
        assert_eq!(lit(r(0, 0)), 0);
        assert_eq!(lit(r(9, 0)), 26);
        assert_eq!(lit(r(7, 5)), 26);
    }

    /// Every fixed phrase the modals draw is in the artefact deny-list (its
    /// plain text, read back from a canvas).
    #[test]
    fn the_modals_phrases_are_in_the_deny_list() {
        let plain = |markup: &str| {
            let mut c = Canvas::new(120, 1, theme());
            let end = c.rich(0, 0, markup, None, theme());
            row_text(&c, 0)
                .chars()
                .take(usize::try_from(end).unwrap_or(0))
                .collect::<String>()
        };
        let mut phrases: Vec<String> = [
            strings::WHY_TOUNICODE,
            strings::WHY_NO_TOUNICODE,
            strings::PICK_THE_CANDIDATE,
            strings::FONT_SOURCE,
            strings::PICK_BUTTON,
            strings::USE_BEST_BUTTON,
            strings::SKIP_BUTTON,
            strings::FONT_PICK_KEYS,
            strings::FONT_PICK_KEYS_ALL,
            strings::PICK_A_SUBSTITUTE,
            strings::USE_GENERIC_BUTTON,
            strings::LEAVE_BUTTON,
            strings::SUBSTITUTE_KEYS,
            strings::SUBSTITUTE_KEYS_ALL,
            strings::COLOUR_SUPPORT,
            strings::PALETTE_SOURCE,
            strings::THEME_KEYS,
        ]
        .iter()
        .map(|m| plain(m))
        .collect();
        phrases.extend(strings::THEME_FILE_LINES.iter().map(|(_, m)| plain(m)));
        phrases.extend(strings::THEME_DESCS.iter().map(|d| d.to_string()));
        for p in &phrases {
            assert!(
                strings::ALL
                    .iter()
                    .any(|a| p.trim() == a.trim() || p.contains(a)),
                "{p:?} is not in strings::ALL"
            );
        }
        for s in [
            strings::PICK_A_FONT,
            strings::SUBSTITUTE_A_FONT,
            strings::BEST_GUESS,
            strings::LANGUAGE_GUESS,
            strings::RESOLVING_FILE,
            strings::CHOOSING_THEME,
            strings::ANSI_0_15,
        ] {
            assert!(strings::ALL.contains(&s), "{s:?}");
        }
        assert_eq!(strings::THEME_DESCS.len(), strings::THEME_NAMES.len());
    }
}
