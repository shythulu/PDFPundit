//! The full layout, 112 × 38 (T-22a; MOCK "Layout (cells)", frame 01),
//! ported from generate.py's `main_panels`, `frame_main` and `frame_chomp`.
//!
//! The frame: the block-pixel logo at (12, 1), the tagline on row 7, the cat
//! at (27, 8) at scale 0.93 behind its plate at (25, 30), LAST CALLERS and MENU
//! on the left, HOW iT WORKS and SYSTEM on the right, the hint on row 35, the
//! hotkeys on row 36 and the status bar on row 37. While the cat reacts to a
//! drop the side panels step back by the director's `panel_dim`, the file is
//! drawn where the director puts it, and the hint and the status bar follow the
//! reaction.
//!
//! The mockup labels the file during a drag and the first two chomp steps
//! (`thesis_ar.pdf +2`) and shows `dragging 3 files` in the status bar while
//! dragging. The director carries no name or count for a drag
//! (`CatEvent::DragAt`), so neither is drawn: the status bar keeps the
//! batch's state. Both wait on that data.
//!
//! With files in the queue the layout shows the batch view (frame 03) or,
//! while the cursor is on a finished file, the result view (frame 05); see
//! [`screen`]. Both draw only what the view model carries: the cells those
//! frames fill with data it lacks (metadata, pipeline stage, log, output
//! paths, recovery figures) stay blank until it does (T-22b, flagged).
//!
//! MENU draws `H history`, `S setup` and `? help` because the golden does, but
//! v1 has no screen behind them (D-048, flagged for the user): [`menu_key`]
//! answers those keys with the "not yet" hint and opens nothing.
#![cfg_attr(not(test), allow(dead_code))]

use super::{FULL_SIZE, Layout};
use crate::ui::canvas::{BoxStyle, Canvas, plain_len};
use crate::ui::cat;
use crate::ui::color::{Rgb, mix, ramp};
use crate::ui::director::{CHOMP, CRUMBS, CRUMBS_AT, CatFrame};
use crate::ui::state::AppState;
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::{QueueRow, Role, RowKind, ViewModel};
use crate::ui::widgets::r#box::{Panel, fit, named, panel};
use crate::ui::widgets::lightbar::{self, hk};
use crate::ui::widgets::queue::QueueBox;
use crate::ui::widgets::{bar, findings, logo};

/// The cat's scale and top-left cell.
pub const CAT_SCALE: f64 = 0.93;
const CAT_AT: (i32, i32) = (27, 8);
/// The plate's top-left cell and width.
const PLATE_AT: (i32, i32) = (25, 30);
const PLATE_W: u16 = 61;
/// The logo's top-left cell and the tagline's row.
const LOGO_AT: (i32, i32) = (12, 1);
const TAGLINE_Y: i32 = 7;
/// The panels: x, y, w, h.
const CALLERS: (i32, i32, i32, i32) = (1, 9, 25, 10);
const MENU: (i32, i32, i32, i32) = (1, 20, 25, 13);
const HOW: (i32, i32, i32, i32) = (86, 9, 25, 12);
const SYSTEM: (i32, i32, i32, i32) = (86, 22, 25, 11);
/// LAST CALLERS' rows.
const RECENT_ROWS: usize = 5;
/// The blocks that step back while the cat reacts: x, y, w, h.
const DIM_LEFT: (i32, i32, i32, i32) = (0, 8, 27, 26);
const DIM_RIGHT: (i32, i32, i32, i32) = (85, 8, 27, 26);
/// The bottom rows.
const HINT_Y: i32 = 35;
const HOTKEYS_Y: i32 = 36;
const BAR_Y: i32 = 37;
const W: i32 = FULL_SIZE.0 as i32;

/// The file at full size, 12 × 16 pixels (generate.py `doc_grid`): a page
/// with a folded corner, two grey text lines above and below a red band.
fn doc() -> Vec<Vec<Option<Rgb>>> {
    (0..16)
        .map(|y: i32| {
            (0..12)
                .map(|x: i32| {
                    if x >= 8 && y <= 3 && x - 8 > y {
                        return None;
                    }
                    let c = if x >= 8 && y <= 3 {
                        0xc9bfcf
                    } else if x == 0 || x == 11 || y == 0 || y == 15 || (x >= 8 && y == 4) {
                        0x8f8597
                    } else if (7..=9).contains(&y) && (1..=10).contains(&x) {
                        if y != 8 || x % 2 == 1 {
                            0xe0445c
                        } else {
                            0xff8a9c
                        }
                    } else if [3, 5, 11, 13].contains(&y)
                        && (2..=if y < 7 { 6 } else { 9 }).contains(&x)
                    {
                        0xb5acbb
                    } else {
                        0xf7f3f8
                    };
                    Some(Rgb::from_u32(c))
                })
                .collect()
        })
        .collect()
}

/// The pointer dragging the file, 7 × 12 pixels (generate.py `CURSOR`).
const CURSOR: [&str; 12] = [
    "o......", "oo.....", "oWo....", "oWWo...", "oWWWo..", "oWWWWo.", "oWWWWWo", "oWWWooo",
    "oWoWo..", "oo.oWo.", "o...oWo", ".....o.",
];

fn cursor() -> Vec<Vec<Option<Rgb>>> {
    CURSOR
        .iter()
        .map(|row| {
            row.chars()
                .map(|k| match k {
                    'o' => Some(Rgb::from_u32(0x1a1020)),
                    'W' => Some(Rgb::from_u32(0xffffff)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// The 112 × 38 main screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FullLayout;

impl Layout for FullLayout {
    fn min_size(&self) -> (u16, u16) {
        FULL_SIZE
    }

    /// The cat and its plate, between the side panels: columns 27–84, rows
    /// 8–33.
    fn drop_zone(&self) -> Option<(u16, u16, u16, u16)> {
        Some((27, 8, 58, 26))
    }

    /// Drawn under a 112 × 38 clip, so nothing lands past the frame on a
    /// larger canvas.
    fn draw(&self, c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
        c.clipped(FULL_SIZE.0, FULL_SIZE.1, |c| match screen(vm, cat) {
            FullScreen::Main => draw_main(c, vm, cat, theme),
            FullScreen::Batch => draw_batch(c, vm, cat, theme),
            FullScreen::Result => draw_result(c, vm, cat, theme),
        });
    }
}

/// Which of the full layout's screens is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullScreen {
    /// Frame 01: no files yet, and every drop reaction (frames 02, 02b),
    /// which the mockup draws only here.
    Main,
    /// Frame 03: files in the queue, the cursor on one not finished.
    Batch,
    /// Frame 05: the cursor on a finished file.
    Result,
}

/// The screen for `vm` while the cat shows `cat`.
pub fn screen(vm: &ViewModel, cat: &CatFrame) -> FullScreen {
    if vm.queue_rows.is_empty() || reacting(cat) {
        return FullScreen::Main;
    }
    match selected(vm).map(|r| r.kind) {
        Some(RowKind::Ok | RowKind::Partial | RowKind::Failed) => FullScreen::Result,
        _ => FullScreen::Batch,
    }
}

fn selected(vm: &ViewModel) -> Option<&QueueRow> {
    vm.queue_rows.iter().find(|r| r.selected)
}

/// The idle screen and the drop reactions over it.
fn draw_main(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
    panels(c, vm, theme);
    if cat.panel_dim > 0.0 {
        for (x, y, w, h) in [DIM_LEFT, DIM_RIGHT] {
            c.dim_rect(x, y, w, h, cat.panel_dim);
        }
    }
    let (cx, cy) = CAT_AT;
    c.blit_grid(&cat::render(&cat.pose, CAT_SCALE, theme, cat.glow), cx, cy);
    if let Some(off) = cat.pose.plate {
        c.blit_grid(&cat::plate(PLATE_W, Some(off)), PLATE_AT.0, PLATE_AT.1);
    }
    if let Some(f) = cat.file_at {
        let page = doc();
        let first = usize::try_from(f.first_row).unwrap_or(0).min(page.len());
        c.pix(f.x, f.pixel_row, &page[first..]);
        if dragging(cat) {
            c.pix(f.x + 5, f.pixel_row + 8, &cursor());
        }
    }
    if cat.crumbs {
        let (x, py) = CRUMBS_AT;
        let paper = mix(theme.roles.heading, Rgb::from_u32(0xffffff), 0.4);
        for (dx, dy, kind) in CRUMBS {
            let col = if kind == "ink" {
                Rgb::from_u32(0xe0445c)
            } else {
                paper
            };
            c.pix(x + dx, py + dy, &[[Some(col)]]);
        }
    }
    for &(x, y, text, k) in cat.fx {
        let fg = theme.slot(k);
        for (i, ch) in (x..).zip(text.chars()) {
            c.put(i, y, ch, fg, None);
        }
    }
    hint(c, vm, cat, theme);
    lightbar::hotkeys(c, 1, HOTKEYS_Y, &strings::HOTKEYS, theme);
    status(c, vm, cat, theme);
}

/// A kitty drag is in view: the director places the file but counts no
/// eaten files (a chomp always eats at least one).
fn dragging(cat: &CatFrame) -> bool {
    cat.file_at.is_some() && cat.n == 0
}

/// The logo, the tagline and the four side panels.
fn panels(c: &mut Canvas, vm: &ViewModel, theme: &Theme) {
    logo::draw(c, LOGO_AT.0, LOGO_AT.1, theme);
    let tag = strings::TAGLINE;
    let n = to_i32(tag.chars().count());
    c.gtext(
        (W - n).div_euclid(2),
        TAGLINE_Y,
        tag,
        &theme.gradients.tag,
        true,
        None,
    );

    let lc = titled(c, CALLERS, strings::LAST_CALLERS, theme);
    // " √ " before the name.
    let name_w = lc.inner_w().saturating_sub(3);
    for (i, r) in vm.recent.iter().take(RECENT_ROWS).enumerate() {
        let k = slot_of(r.status.role());
        let y = CALLERS.1 + 1 + to_i32(i);
        let x = lc.line(c, y, &format!(" {{{k}}}{} ", r.status.glyph()), theme);
        // The name is the file's, not markup: drawn as it is.
        lc.text(c, x, y, &fit(&r.name, name_w), theme.roles.file);
    }
    let (files, runs) = vm.history_totals;
    let totals = strings::N_FILES_N_RUNS
        .replacen("{n}", &files.to_string(), 1)
        .replacen("{n}", &runs.to_string(), 1);
    let footer = fit(&totals, lc.inner_w().saturating_sub(1));
    lc.line(
        c,
        CALLERS.1 + 2 + to_i32(RECENT_ROWS),
        &format!(" {{D}}{footer}"),
        theme,
    );

    let mb = titled(c, MENU, strings::MENU, theme);
    for (i, &(k, label)) in strings::MENU_ITEMS.iter().enumerate() {
        mb.line(
            c,
            MENU.1 + 1 + to_i32(i),
            &format!(" {}", hk(k, label)),
            theme,
        );
    }
    mb.sep(c, MENU.1 + 8, theme);
    let prompt_y = MENU.1 + 10;
    let e = mb.line(c, prompt_y, &prompt_markup(), theme);
    c.put(e, prompt_y, '█', Some(theme.roles.body), None);
    c.set_blink(e, prompt_y);

    let hb = titled(c, HOW, strings::HOW_IT_WORKS, theme);
    for (i, &(step, text)) in strings::HOW_LINES.iter().enumerate() {
        let line = match (step, i) {
            (Some(n), _) => format!(" {{M}}{n} {{w}}{text}"),
            // The last line is the output name, a file.
            (None, i) if i + 1 == strings::HOW_LINES.len() => format!("   {{C}}{text}"),
            (None, _) => format!("   {{D}}{text}"),
        };
        hb.line(c, HOW.1 + 1 + to_i32(i), &line, theme);
    }

    let sb = titled(c, SYSTEM, strings::SYSTEM, theme);
    let short = theme.name.rsplit(' ').next().unwrap_or(theme.name);
    let history = strings::N_FILES.replace("{n}", &files.to_string());
    let pair = |(label, value): (&str, &str), k: char| format!(" {{D}}{label}{{{k}}}{value}");
    let rows = [
        pair(strings::SYS_ENGINE, 'W'),
        pair(strings::SYS_NETWORK, 'G'),
        pair(strings::SYS_FONTS, 'W'),
        pair((strings::SYS_THEME, short), 'M'),
        pair((strings::SYS_HISTORY, &history), 'W'),
        pair(strings::SYS_ORIGINALS, 'G'),
    ];
    for (i, row) in rows.iter().enumerate() {
        sb.line(c, SYSTEM.1 + 2 + to_i32(i), row, theme);
    }
}

fn titled(c: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), title: &str, theme: &Theme) -> Panel {
    panel(c, x, y, w, h, title, theme)
}

/// MENU's prompt, `main »` with the chevron bright.
fn prompt_markup() -> String {
    let (word, chevron) = strings::MAIN_PROMPT
        .split_once(' ')
        .unwrap_or((strings::MAIN_PROMPT, ""));
    format!(" {{c}}{word} {{W}}{chevron} ")
}

/// The markup letter of a colour role.
fn slot_of(role: Role) -> char {
    match role {
        Role::Ok => 'G',
        Role::Warn => 'Y',
        Role::Error => 'R',
        Role::NeedsInput => 'M',
        Role::File => 'C',
        Role::Body => 'w',
        Role::Dim => 'D',
        Role::Info => 'c',
    }
}

/// The hint row: the reaction's hint while the cat reacts; at rest a one-off
/// hint from the app (D-048's "not yet") when there is one, else the cat's.
/// Centred, coloured out from both ends: the "»…«" calls along `m`→`M`→`W`,
/// the rest along the tagline gradient.
fn hint(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
    let text = match vm.hint {
        Some(h) if !reacting(cat) => h.to_string(),
        _ => cat.hint_text(),
    };
    let n = to_i32(text.chars().count());
    let call: Vec<Rgb>;
    let stops: &[Rgb] = if text.starts_with('»') {
        call = ['m', 'M', 'W']
            .iter()
            .filter_map(|&k| theme.slot(k))
            .collect();
        &call
    } else {
        &theme.gradients.tag
    };
    c.gtext((W - n).div_euclid(2), HINT_Y, &text, stops, true, None);
}

/// A reaction is playing: a file is in view or being eaten.
fn reacting(cat: &CatFrame) -> bool {
    cat.file_at.is_some() || cat.n > 0
}

/// The status bar: node, the batch's state (or the chomp's), the queue and
/// "offline" on the left; the theme, the terminal's size and the clock on the
/// right.
fn status(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
    let sb = &vm.status_bar;
    let mut parts = vec![strings::NODE_N.replace("{n}", &sb.node.to_string())];
    if cat.n > 0 {
        let eaten = cat.hint == CHOMP[CHOMP.len() - 1].hint;
        parts.push(if eaten {
            format!(
                "{{C}}{}",
                strings::N_QUEUED.replace("{n}", &cat.n.to_string())
            )
        } else {
            format!(
                "{{M}}{}",
                strings::EATING_N_PDFS.replace("{n}", &cat.n.to_string())
            )
        });
    } else {
        let k = if sb.state == strings::STATE_NEEDS_YOU {
            'M'
        } else {
            'W'
        };
        parts.push(format!("{{{k}}}{}", sb.state));
        parts.push(strings::N_QUEUED.replace("{n}", &sb.queued.to_string()));
    }
    if sb.offline {
        parts.push(format!("{{G}}{}", strings::OFFLINE));
    }
    let mut right = format!(
        "{{M}}{} {{c}}│{{W}} {}×{}",
        theme.name, sb.size.0, sb.size.1
    );
    if let Some((h, m)) = sb.clock {
        right.push_str(&format!(" {{c}}│{{W}} {h:02}:{m:02}"));
    }
    lightbar::status_bar(c, BAR_Y, W, &parts, &right, theme);
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

// ── the batch and result views (T-22b, frames 03 and 05) ─────────────────

/// The batch view's cat: scale and top-left cell; the line under it.
const BATCH_CAT: (f64, i32, i32) = (0.68, 68, 11);
const DROP_MORE_AT: (i32, i32) = (69, 31);
/// The result view's cat, and its line.
const RESULT_CAT: (f64, i32, i32) = (0.52, 8, 17);
const SATISFIED_AT: (i32, i32) = (5, 31);
/// The queue box: x, y, width, name column.
const BATCH_QUEUE: (i32, i32, i32, usize) = (1, 2, 64, 21);
const RESULT_QUEUE: (i32, i32, i32, usize) = (1, 2, 50, 20);
/// The analysis panel, the needs-input box and the result panel: x, y, w, h.
const ANALYSIS_BOX: (i32, i32, i32, i32) = (1, 13, 64, 19);
const NEEDS_BOX: (i32, i32, i32, i32) = (67, 2, 44, 9);
const RESULT_BOX: (i32, i32, i32, i32) = (52, 2, 59, 30);
/// The per-file menu: x, y, w, h.
const MENU_BOX: (i32, i32, i32, i32) = (17, 7, 32, 10);
/// The progress box's top row.
const PROGRESS_Y: i32 = 32;
/// The finding rows each panel has, and their summary columns.
const ANALYSIS_FINDINGS: (i32, usize, usize) = (20, 4, 22);
const RESULT_FINDINGS: (i32, usize, usize) = (9, 4, 24);
/// The result panel's font rows: first row, count, name column.
const RESULT_FONTS: (i32, usize, usize) = (21, 3, 17);
/// The result panel's RECOVERY rows, which hold the C9 count line (D-041).
const RECOVERY_ROWS: usize = 4;

/// Frame 03: the queue, the analysis of the file under the cursor, the box
/// asking for a decision when a file is parked, the cat, the progress box.
///
/// Frame 03 also shows the file's version, page count, size and producer, the
/// repair's pipeline stage and toolpath, each finding's detail past its
/// location, and a live log. The view model carries none of these (T-22b's
/// open question), so those rows are left blank rather than invented. A list
/// longer than its rows ends in a dim `… N more`.
fn draw_batch(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
    let (crumb, mode) = strings::CRUMB_BATCH;
    header(c, &format!("{{w}}{crumb} {{D}}· {{c}}{mode} "), theme);
    let (x, y, w, name_w) = BATCH_QUEUE;
    QueueBox {
        x,
        y,
        w,
        name_w,
        short: false,
    }
    .draw(c, &vm.queue_rows, &queue_note(vm), theme);

    let name = selected(vm).map_or("", |r| r.name.as_str());
    let ab = named(
        c,
        ANALYSIS_BOX,
        BoxStyle::default(),
        strings::ANALYSIS,
        name,
        title_room(ANALYSIS_BOX.2, strings::ANALYSIS, ""),
        theme,
    );
    let (_, ay, _, _) = ANALYSIS_BOX;
    ab.sep(c, ay + 5, theme);
    ab.line(
        c,
        ay + 6,
        &format!(
            " {{W}}{} {{D}}{}",
            strings::FINDINGS,
            strings::STREAMED_AS_FOUND
        ),
        theme,
    );
    let (fy, rows, summary_w) = ANALYSIS_FINDINGS;
    let (n, more) = findings::shown(vm.selected_findings.len(), rows);
    for (i, f) in vm.selected_findings.iter().take(n).enumerate() {
        let y = fy + to_i32(i);
        let x = findings::finding(c, &ab, y, f, summary_w, theme);
        if let Some(at) = findings::location(&f.location) {
            ab.text(c, x, y, &at, theme.roles.dim);
        }
    }
    if more > 0 {
        findings::more(c, &ab, fy + to_i32(n), more, theme);
    }
    ab.sep(c, ay + 11, theme);
    ab.line(c, ay + 12, &format!(" {{W}}{}", strings::LOG), theme);

    if let Some((_, name)) = &vm.needs_you {
        needs_box(c, vm, name, theme);
    }
    let (scale, cx, cy) = BATCH_CAT;
    c.blit_grid(&cat::render(&cat.pose, scale, theme, cat.glow), cx, cy);
    let (dx, dy) = DROP_MORE_AT;
    c.rich(dx, dy, &format!("{{D}}{}", strings::DROP_MORE), None, theme);
    view_footer(c, vm, theme, None);
}

/// Frame 05: the queue (short statuses), the result for the file under the
/// cursor, the cat, the per-file menu when it is open, the progress box.
///
/// Frame 05 also shows the output's name and folder, the toolpath and the
/// repair's time, each finding's after figure, the recovery bars, a picked
/// font's family and a substitution's caveat. The view model carries none of
/// these (T-22b's open question), so those cells are left blank. The C9 count
/// line (D-041), which frame 05 has no row for, fills the RECOVERY rows; a
/// failed file's reason (D-048) takes the progress box's bottom edge.
fn draw_result(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
    header(c, &format!("{{w}}{} ", strings::CRUMB_RESULTS), theme);
    let (x, y, w, name_w) = RESULT_QUEUE;
    QueueBox {
        x,
        y,
        w,
        name_w,
        short: true,
    }
    .draw(c, &vm.queue_rows, &queue_note(vm), theme);

    let row = selected(vm);
    let note = row.map_or_else(String::new, |r| {
        format!(
            "{{{}}}{} {}",
            slot_of(r.kind.role()),
            r.kind.glyph(),
            r.label
        )
    });
    let rb = named(
        c,
        RESULT_BOX,
        BoxStyle {
            note: Some(&note),
            ..BoxStyle::default()
        },
        strings::RESULT,
        row.map_or("", |r| r.name.as_str()),
        title_room(RESULT_BOX.2, strings::RESULT, &note),
        theme,
    );
    let (_, ry, _, _) = RESULT_BOX;
    rb.sep(c, ry + 5, theme);
    rb.line(
        c,
        ry + 6,
        &format!(
            " {{W}}{:<34}{{D}}{}",
            strings::FINDINGS,
            strings::BEFORE_AFTER
        ),
        theme,
    );
    let (fy, rows, summary_w) = RESULT_FINDINGS;
    let (n, more) = findings::shown(vm.selected_findings.len(), rows);
    for (i, f) in vm.selected_findings.iter().take(n).enumerate() {
        let y = fy + to_i32(i);
        let x = findings::finding(c, &rb, y, f, summary_w, theme);
        findings::before_after(c, &rb, x, y, f, theme);
    }
    if more > 0 {
        findings::more(c, &rb, fy + to_i32(n), more, theme);
    }
    rb.sep(c, ry + 11, theme);
    rb.line(c, ry + 12, &format!(" {{W}}{}", strings::RECOVERY), theme);
    if let Some(line) = &vm.c9_line {
        let room = rb.inner_w().saturating_sub(2);
        for (y, part) in (ry + 13..).zip(wrap(line, room, RECOVERY_ROWS)) {
            let x = rb.line(c, y, " ", theme);
            rb.text(c, x, y, &part, theme.roles.body);
        }
    }
    rb.sep(c, ry + 17, theme);
    rb.line(c, ry + 18, &format!(" {{W}}{}", strings::FONTS), theme);
    let (fy, rows, name_w) = RESULT_FONTS;
    let (n, more) = findings::shown(vm.font_resolutions.len(), rows);
    for (i, f) in vm.font_resolutions.iter().take(n).enumerate() {
        findings::font(c, &rb, fy + to_i32(i), f, name_w, theme);
    }
    if more > 0 {
        findings::more(c, &rb, fy + to_i32(n), more, theme);
    }
    rb.sep(c, ry + 22, theme);
    let keys: Vec<String> = strings::RESULT_KEYS
        .iter()
        .map(|&(k, l)| hk(k, l))
        .collect();
    rb.line(c, ry + 23, &format!(" {}", keys.join("  ")), theme);
    let repaired = row.is_some_and(|r| r.kind == RowKind::Ok);
    if repaired {
        let stops: Vec<Rgb> = ['D', 'm', 'M', 'W']
            .iter()
            .filter_map(|&k| theme.slot(k))
            .collect();
        c.gtext(
            rb.0.x + 2,
            ry + 26,
            strings::CASE_CLOSED,
            &stops,
            true,
            None,
        );
    }

    let (scale, cx, cy) = RESULT_CAT;
    c.blit_grid(&cat::render(&cat.pose, scale, theme, cat.glow), cx, cy);
    if repaired {
        let (sx, sy) = SATISFIED_AT;
        let (line, burp) = strings::CAT_SATISFIED;
        c.rich(sx, sy, &format!("{{D}}{line} {{m}}{burp}"), None, theme);
    }
    // Why a failed file failed: frame 05 has no row for it, and the queue's
    // status column cuts it short.
    let reason = row
        .filter(|r| r.kind == RowKind::Failed)
        .and_then(|r| r.detail.as_deref());
    view_footer(c, vm, theme, reason);
}

/// `text` broken at spaces into at most `rows` lines of at most `w`
/// characters; a word longer than a line, or text left over after the last
/// line, is cut with `…`.
fn wrap(text: &str, w: usize, rows: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let n = cur.chars().count();
        if n > 0 && n + 1 + word.chars().count() > w {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    let over = lines.len() > rows;
    let mut lines: Vec<String> = lines.into_iter().take(rows).map(|l| fit(&l, w)).collect();
    if over && let Some(last) = lines.last_mut() {
        // Room for the `…` after the line, or in place of its last character.
        let head: String = last.chars().take(w.saturating_sub(1)).collect();
        *last = format!("{}…", head.trim_end_matches('…'));
    }
    lines
}

/// What both views draw last: the per-file menu when it is open, the progress
/// box, the hotkeys, the status bar and the bottom edge's note: the one-off
/// hint, else `fallback` (the result view's reason a failed file failed,
/// D-048's hint line). These views have no hint row, so the note takes the
/// progress box's bottom edge, cut only at its corners.
fn view_footer(c: &mut Canvas, vm: &ViewModel, theme: &Theme, fallback: Option<&str>) {
    if let Some(sel) = vm.file_menu {
        file_menu(c, sel, theme);
    }
    let pb = bar::progress(
        c,
        PROGRESS_Y,
        vm.current.as_ref(),
        &batch_markup(vm),
        vm.batch_progress,
        theme,
    );
    if let Some(note) = vm.hint.or(fallback) {
        pb.bottom_note(c, note, theme.roles.body);
    }
    if vm.file_menu.is_some() {
        lightbar::hotkeys(c, 1, HOTKEYS_Y, &strings::MENU_KEYS, theme);
    } else {
        lightbar::hotkeys(c, 1, HOTKEYS_Y, &strings::BATCH_KEYS, theme);
    }
    let counts = &vm.counts;
    let mut parts = vec![
        fill_n(strings::BATCH_N_OF_N, &[counts.done(), counts.total]),
        part(need_input(counts.needs_input)),
        part(failed(counts.failed)),
    ];
    if vm.status_bar.offline {
        parts.push(format!("{{G}}{}", strings::OFFLINE));
    }
    let mut right = format!("{{M}}{}", theme.name);
    if let Some((h, m)) = vm.status_bar.clock {
        right.push_str(&format!(" {{c}}│{{W}} {h:02}:{m:02}"));
    }
    lightbar::status_bar(c, BAR_Y, W, &parts, &right, theme);
}

/// The header row: a block, the app's name in three colours, `»` and the
/// breadcrumb (markup), then a rule to the edge along the border gradient
/// reversed and back.
fn header(c: &mut Canvas, crumb: &str, theme: &Theme) {
    for (x, ch, t) in [(1, '▐', 0.1), (2, '█', 0.4), (3, '▌', 0.8)] {
        c.put(x, 0, ch, Some(ramp(&theme.logo_ramp, t)), None);
    }
    let name: Vec<char> = strings::APP_NAME.chars().collect();
    let part = |r: std::ops::Range<usize>| name[r].iter().collect::<String>();
    let e = c.rich(
        5,
        0,
        &format!(
            "{{W}}{}{{M}}{}{{m}}{} {{D}}» {crumb}",
            part(0..3),
            part(3..6),
            part(6..name.len())
        ),
        None,
        theme,
    );
    let stops: Vec<Rgb> = theme
        .roles
        .border_stops
        .iter()
        .rev()
        .chain(theme.roles.border_stops.iter().skip(1))
        .copied()
        .collect();
    let rule = "─".repeat(usize::try_from(W - e - 2).unwrap_or(0));
    c.gtext(e + 1, 0, &rule, &stops, false, None);
}

/// The queue box's note: done of total, then how many wait on you and how
/// many failed, each only when there are any.
fn queue_note(vm: &ViewModel) -> String {
    let n = &vm.counts;
    let mut note = strings::N_OF_N_DONE
        .replacen("{n}", &format!("{{G}}{}{{w}}", n.done()), 1)
        .replacen("{n}", &n.total.to_string(), 1);
    if n.needs_input > 0 {
        note.push_str(&after_dot(need_input(n.needs_input)));
    }
    if n.failed > 0 {
        note.push_str(&after_dot(failed(n.failed)));
    }
    note
}

/// `1 needs input` in the needs-input colour, `2 need input`, or `0 need
/// input` in green: the text and its colour's slot letter.
fn need_input(n: usize) -> (char, String) {
    let (k, s) = match n {
        0 => ('G', strings::N_NEED_INPUT),
        1 => ('M', strings::N_NEEDS_INPUT),
        _ => ('M', strings::N_NEED_INPUT),
    };
    (k, s.replace("{n}", &n.to_string()))
}

/// `1 failed` in red, or `0 failed` in green.
fn failed(n: usize) -> (char, String) {
    let k = if n == 0 { 'G' } else { 'R' };
    (k, strings::N_FAILED.replace("{n}", &n.to_string()))
}

/// A count after a dim `·`, the space before it in the count's colour.
fn after_dot((k, text): (char, String)) -> String {
    format!(" {{D}}·{{{k}}} {text}")
}

/// A count as a status bar part.
fn part((k, text): (char, String)) -> String {
    format!("{{{k}}}{text}")
}

/// The progress box's batch line: `batch 3/7`, then how many are parked, or
/// failing that how many failed.
fn batch_markup(vm: &ViewModel) -> String {
    let n = &vm.counts;
    let word = strings::BATCH_N_OF_N
        .split_once(' ')
        .map_or(strings::BATCH_N_OF_N, |(w, _)| w);
    let mut s = format!("{{w}}{word} {{W}}{}{{D}}/{{W}}{}", n.done(), n.total);
    if n.needs_input > 0 {
        let parked = strings::N_PARKED.replace("{n}", &n.needs_input.to_string());
        s.push_str(&after_dot(('M', parked)));
    } else if n.failed > 0 {
        s.push_str(&after_dot(failed(n.failed)));
    }
    s
}

/// `template` with each `{n}` replaced by the next of `values`.
fn fill_n(template: &str, values: &[usize]) -> String {
    values.iter().fold(template.to_string(), |s, v| {
        s.replacen("{n}", &v.to_string(), 1)
    })
}

/// The box asking for a decision about the first parked file: its name, how
/// many of its fonts cannot be reproduced (its queue row's detail), and the
/// keys. Frame 03 adds the file's page count and language guess, which the
/// view model does not carry, so nothing follows the name yet.
fn needs_box(c: &mut Canvas, vm: &ViewModel, name: &str, theme: &Theme) {
    let (x, y, w, h) = NEEDS_BOX;
    let style = BoxStyle {
        title: Some(strings::NEEDS_INPUT_TITLE),
        grad: Some(&theme.gradients.modal),
        title_colours: ('W', 'm'),
        ..BoxStyle::default()
    };
    let p = Panel(c.boxed(x, y, w, h, &style, theme));
    let room = usize::try_from(w - 4).unwrap_or(0);
    let nx = p.line(c, y + 2, " ", theme);
    p.text(
        c,
        nx,
        y + 2,
        &format!("{} ", fit(name, room)),
        theme.roles.file,
    );
    let fonts = vm
        .queue_rows
        .iter()
        .find(|r| r.kind == RowKind::NeedsInput)
        .and_then(|r| r.detail.as_deref())
        .unwrap_or("");
    let fx = p.line(c, y + 3, " ", theme);
    let fx = p.text(c, fx, y + 3, fonts, theme.roles.body);
    let (can_be, read, but_not, reproduced) = strings::CAN_BE_READ_NOT_REPRODUCED;
    p.line_at(
        c,
        fx,
        y + 3,
        &format!(" {can_be} {{W}}{read}{{w}} {but_not} {{W}}{reproduced}{{w}}."),
        theme,
    );
    p.line(
        c,
        y + 4,
        &format!(" {{D}}{}", strings::PARKED_KEEPS_GOING),
        theme,
    );
    let keys: Vec<String> = strings::NEEDS_INPUT_KEYS
        .iter()
        .map(|&(k, l)| hk(k, l))
        .collect();
    p.line(c, y + 6, &format!("  {}", keys.join("   ")), theme);
}

/// The per-file menu (D-048, frame 05), the cursor on item `sel`.
fn file_menu(c: &mut Canvas, sel: usize, theme: &Theme) {
    let (x, y, w, h) = MENU_BOX;
    let style = BoxStyle {
        title: Some(strings::FILE_MENU),
        grad: Some(&theme.gradients.menu),
        title_colours: ('W', 'y'),
        shadow: true,
        ..BoxStyle::default()
    };
    c.boxed(x, y, w, h, &style, theme);
    let items = &strings::FILE_MENU_ITEMS;
    let sel = menu_item(sel);
    let bar = theme.slot('b').unwrap_or(theme.roles.lightbar);
    let rule: Vec<Rgb> = [Some(theme.roles.bg), theme.slot('y'), theme.slot('Y')]
        .into_iter()
        .flatten()
        .collect();
    for (i, item) in items.iter().enumerate() {
        let ry = y + 1 + to_i32(i);
        match item {
            None => c.gtext(x + 1, ry, &"─".repeat(30), &rule, true, None),
            Some((label, key)) if i == sel => {
                c.fill(x + 1, ry, w - 2, 1, theme.roles.body, bar);
                c.rich(
                    x + 1,
                    ry,
                    &format!("{{Y/b}} ► {{W}}{label:<22}{{Y}}{key:>3} "),
                    None,
                    theme,
                );
            }
            Some((label, key)) => {
                c.rich(
                    x + 1,
                    ry,
                    &format!("   {{w}}{label:<22}{{Y}}{key:>3}"),
                    None,
                    theme,
                );
            }
        }
    }
}

/// The item the menu's cursor `sel` lands on: itself, or for a separator the
/// next item (the input side never sets one; an index past the end is the
/// last item).
fn menu_item(sel: usize) -> usize {
    let items = &strings::FILE_MENU_ITEMS;
    let last = items.len() - 1;
    (sel.min(last)..=last)
        .find(|&i| items[i].is_some())
        .unwrap_or(last)
}

/// How many characters of a file's name fit in a `w`-wide box's top edge
/// after `title`, leaving room for `note` (markup) at its right.
fn title_room(w: i32, title: &str, note: &str) -> usize {
    let used = 4 + title.chars().count() + 1 + 2;
    let note = if note.is_empty() {
        0
    } else {
        plain_len(note) + 2 + 4
    };
    usize::try_from(w)
        .unwrap_or(0)
        .saturating_sub(used + note + 1)
}

/// What a MENU key does on the main screen when it has no screen in v1
/// (D-048, flagged for the user): `H` (history), `S` (setup) and `?` (help)
/// set the hint row to "not yet" and open nothing. Returns whether the key
/// was one of them; any other key is left to the loop.
pub fn menu_key(app: &mut AppState, key: char) -> bool {
    match key {
        'H' | 'h' | 'S' | 's' | '?' => {
            app.hint = Some(strings::NOT_YET);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::engine::{C9Summary, FontResolutionKind, Location};
    use crate::jobs::{EntryState, QueueEntry};
    use crate::library::{HistorySummary, RecentRow, RecentStatus};
    use crate::ui::canvas::HALF;
    use crate::ui::director::{
        CHOMP_DURS, CatEvent, CellPos, DRAG, DRAG_DOC, DRAG_DURS, Director, Mood,
    };
    use crate::ui::goldens::{self, Golden, GoldenCell, Rgb as GoldenRgb};
    use crate::ui::state::Screen;
    use crate::ui::view::view;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    /// When step `i` of a reaction starts.
    fn step_start(durs: &[f64; 8], i: usize) -> Duration {
        let ms: u64 = durs[..i]
            .iter()
            .map(|d| (d * 1000.0).round_ties_even() as u64)
            .sum();
        Duration::from_millis(ms)
    }

    /// The cat at rest in `vm`'s mood.
    fn resting(vm: &ViewModel) -> CatFrame {
        let mut d = Director::new();
        d.on(CatEvent::Mood(vm.mood), Duration::ZERO);
        d.frame(Duration::ZERO)
    }

    /// The chomp of three dropped files at the start of step `i`.
    fn chomp(i: usize) -> CatFrame {
        let mut d = Director::new();
        d.on(CatEvent::Dropped { n: 3 }, Duration::ZERO);
        d.frame(step_start(&CHOMP_DURS, i))
    }

    /// A kitty drag at the start of step `i`, the file where the mockup has it.
    fn drag(i: usize) -> CatFrame {
        let (x, y) = DRAG_DOC[i];
        let mut d = Director::new();
        d.on(CatEvent::DragAt(Some(CellPos { x, y })), Duration::ZERO);
        d.frame(step_start(&DRAG_DURS, i))
    }

    fn draw(size: (u16, u16), vm: &ViewModel, cat: &CatFrame) -> Canvas {
        let mut c = Canvas::new(size.0, size.1, theme());
        FullLayout.draw(&mut c, vm, cat, theme());
        c
    }

    fn row_text(c: &Canvas, y: u16) -> String {
        (0..c.w).map(|x| c.get(x, y).expect("cell").ch).collect()
    }

    #[test]
    fn idle_matches_the_golden() {
        let vm = view(&AppState::mockup_idle());
        draw(FULL_SIZE, &vm, &resting(&vm)).assert_matches(&goldens::load("01-idle"));
    }

    /// The chomp's fourth step, "chomp": the file half eaten, the panels
    /// stepped back, "CHOMP!" beside the cat.
    #[test]
    fn chomp_step_4_matches_the_golden() {
        let vm = view(&AppState::mockup_idle());
        draw(FULL_SIZE, &vm, &chomp(3)).assert_matches(&goldens::load("02b-chomp-4-chomp"));
    }

    /// The chomp's steps from the jaw dropping on: the mockup's first two
    /// steps also label the file with its name ("thesis_ar.pdf +2"), which the
    /// director does not carry, so they are not compared.
    #[test]
    fn later_chomp_steps_match_the_goldens() {
        let names = [
            "3-jaw-drops",
            "4-chomp",
            "5-nom",
            "6-nom-nom",
            "7-gulp",
            "8-burp",
        ];
        let vm = view(&AppState::mockup_idle());
        for (i, name) in (2..).zip(names) {
            let c = draw(FULL_SIZE, &vm, &chomp(i));
            c.assert_matches(&goldens::load(&format!("02b-chomp-{name}")));
        }
    }

    #[test]
    fn an_empty_history_draws_blank_callers_and_zero_totals() {
        let app = AppState {
            term_size: (112, 38),
            ..AppState::default()
        };
        let vm = view(&app);
        assert!(vm.recent.is_empty());
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        for y in 10..15 {
            let row: String = row_text(&c, y).chars().take(26).collect();
            assert_eq!(row, format!(" ║{:23}║", ""), "row {y}");
        }
        let footer = format!(" ║ {:<22}║", "0 files · 0 runs");
        let row: String = row_text(&c, 16).chars().take(26).collect();
        assert_eq!(row, footer);
        let sys: String = row_text(&c, 28).chars().skip(86).take(25).collect();
        assert_eq!(sys, format!("║ {:<22}║", "history  0 files"));
        // No clock yet: the status bar ends at the size.
        assert!(
            row_text(&c, 37).ends_with("│ 112×38 "),
            "{:?}",
            row_text(&c, 37)
        );
    }

    /// Long names, huge totals and counts, and a clock: everything stays inside
    /// the panels and the 112 × 38 frame on a larger canvas.
    fn stress_states() -> Vec<ViewModel> {
        let mut out: Vec<ViewModel> = [
            AppState::mockup_idle(),
            AppState::mockup_batch(),
            AppState::mockup_done(),
            AppState::default(),
        ]
        .iter()
        .map(view)
        .collect();
        let mut app = AppState::mockup_idle();
        app.history = HistorySummary {
            files: u64::MAX,
            runs: u64::MAX,
            recent: (0..9)
                .map(|i| RecentRow {
                    name: format!("a_really_quite_long_evidence_file_name_{i}.pdf"),
                    status: RecentStatus::Partial,
                })
                .collect(),
        };
        app.term_size = (u16::MAX, u16::MAX);
        app.clock = Some((23, 59));
        app.hint = Some(strings::NOT_YET);
        out.push(view(&app));
        out.push(view(&result_with_c9(Some(0), (30, 28, 12))));
        out.push(view(&result_with_c9(None, (1000, 999, 1))));
        out.push(view(&long_batch()));
        out.push(view(&overflowing_result()));
        out.push(view(&overflowing_batch()));
        out.push(view(&failed_with(&"e".repeat(200))));
        let mut encrypted = AppState::mockup_batch();
        encrypted.selected = Some(6);
        out.push(view(&encrypted));
        out
    }

    /// Frame 05's file with six findings and five font slots: more than the
    /// result panel has rows for.
    fn overflowing_result() -> AppState {
        let mut app = AppState::mockup_result();
        let thesis = &mut app.batch.entries[2];
        let extra = thesis.findings[..2].to_vec();
        thesis.findings.extend(extra);
        let extra = thesis.font_resolutions[..2].to_vec();
        thesis.font_resolutions.extend(extra);
        app
    }

    /// Frame 03's file under analysis with six findings, one on page 3.
    fn overflowing_batch() -> AppState {
        let mut app = AppState::mockup_batch();
        let invoice = &mut app.batch.entries[4];
        let mut extra = invoice.findings.clone();
        extra[0].location = Location::Page {
            index: 2,
            obj: None,
        };
        invoice.findings.extend(extra);
        app
    }

    /// Frame 03's batch with the cursor on a file whose job failed with
    /// `error`.
    fn failed_with(error: &str) -> AppState {
        let mut app = AppState::mockup_batch();
        app.batch.entries[5].state = EntryState::Failed {
            error: error.into(),
            panicked: false,
        };
        app.selected = Some(5);
        app
    }

    /// Frame 05's state with a C9 count line: the selected file's repair
    /// found damaged streams, `(repaired, exact, accepted)` of them.
    fn result_with_c9(
        menu: Option<usize>,
        (repaired, exact, accepted): (u32, u32, u32),
    ) -> AppState {
        let mut app = AppState::mockup_result();
        app.file_menu = menu;
        let run = app.batch.entries[2]
            .run
            .as_mut()
            .expect("thesis_ar.pdf ran");
        run.report.c9_summary = C9Summary {
            streams_damaged: repaired,
            repaired,
            exact,
            accepted,
            ..C9Summary::default()
        };
        app
    }

    /// Frame 03's batch twice over, with long names, failures and the
    /// cursor near the end, so the queue scrolls and every field is cut.
    fn long_batch() -> AppState {
        let mut app = AppState::mockup_batch();
        let extra: Vec<QueueEntry> = app.batch.entries.clone();
        app.batch.entries.extend(extra);
        for (i, e) in app.batch.entries.iter_mut().enumerate() {
            e.name = format!("an_extremely_long_evidence_bundle_name_number_{i}.pdf");
            if i == 9 {
                e.state = EntryState::Failed {
                    error: "w".repeat(200),
                    panicked: false,
                };
            }
            for f in &mut e.findings {
                f.summary = "s".repeat(90);
            }
        }
        app.selected = Some(12);
        app.batch.current = Some(11);
        app.batch.entries[11].state = EntryState::Analyzing {
            phase: None,
            done: 1,
            total: None,
        };
        app.file_menu = Some(99);
        app
    }

    #[test]
    fn everything_drawn_stays_inside_112_by_38() {
        let size = (140, 50);
        let blank = Canvas::new(size.0, size.1, theme());
        let mut cats: Vec<CatFrame> = (0..CHOMP.len()).map(chomp).collect();
        cats.extend((0..DRAG.len()).map(drag));
        for vm in stress_states() {
            cats.push(resting(&vm));
            for cat in &cats {
                let c = draw(size, &vm, cat);
                for y in 0..size.1 {
                    for x in 0..size.0 {
                        if x >= FULL_SIZE.0 || y >= FULL_SIZE.1 {
                            assert_eq!(c.get(x, y), blank.get(x, y), "({x}, {y})");
                        }
                    }
                }
                assert!(c.blink.iter().all(|&(x, y)| x < 112 && y < 38));
            }
        }
    }

    /// Long file names and totals end in `…` inside LAST CALLERS, whose right
    /// border stays whole.
    #[test]
    fn long_names_and_totals_stay_inside_last_callers() {
        let vm = &stress_states()[4];
        let c = draw(FULL_SIZE, vm, &resting(vm));
        for y in 10..17 {
            let row: Vec<char> = row_text(&c, y).chars().collect();
            assert_eq!(row[25], '║', "row {y}: {row:?}");
        }
        let first: String = row_text(&c, 10).chars().skip(2).take(23).collect();
        assert_eq!(first, " ~ a_really_quite_long…");
        let footer: String = row_text(&c, 16).chars().skip(2).take(23).collect();
        assert!(footer.ends_with('…'), "{footer:?}");
        assert_eq!(row_text(&c, 15).chars().nth(25), Some('║'));
    }

    /// A file's name is drawn as it is: braces in it are not colour markup and
    /// a control character in it never reaches a cell.
    #[test]
    fn names_with_markup_or_control_characters_are_drawn_verbatim() {
        let mut app = AppState::mockup_idle();
        app.history.recent = [
            "Invoice {A}.pdf",
            "a{}b.pdf",
            "x{R}.pdf",
            "y{/K}z\u{1b}[2J.pdf",
        ]
        .iter()
        .map(|n| RecentRow {
            name: (*n).to_string(),
            status: RecentStatus::Repaired,
        })
        .collect();
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        let want = [
            "Invoice {A}.pdf",
            "a{}b.pdf",
            "x{R}.pdf",
            "y{/K}z\u{fffd}[2J.pdf",
        ];
        for (y, name) in (10..).zip(want) {
            let n = name.chars().count();
            let row: String = row_text(&c, y).chars().skip(5).take(n + 1).collect();
            assert_eq!(row, format!("{name} "), "row {y}");
            for x in 5..5 + to_i32(n) {
                let cell = c.get(x as u16, y).expect("cell");
                assert_eq!(cell.fg, theme().roles.file, "({x}, {y})");
            }
            assert_eq!(row_text(&c, y).chars().nth(25), Some('║'), "row {y}");
        }
    }

    #[test]
    fn h_s_and_help_set_the_not_yet_hint_and_open_nothing() {
        for key in ['H', 'h', 'S', 's', '?'] {
            let before = AppState::mockup_idle();
            let mut app = before.clone();
            assert!(menu_key(&mut app, key), "{key:?}");
            assert_eq!(app.hint, Some(strings::NOT_YET));
            assert_eq!(app.screen, Screen::Main);
            assert_eq!(
                AppState {
                    hint: None,
                    ..app.clone()
                },
                before,
                "{key:?} changed more than the hint"
            );
            let vm = view(&app);
            let c = draw(FULL_SIZE, &vm, &resting(&vm));
            assert_eq!(row_text(&c, 35).trim(), strings::NOT_YET);
        }
        for key in ['B', 'b', 'T', 'Q', 'q', 'x', '\n'] {
            let mut app = AppState::mockup_idle();
            assert!(!menu_key(&mut app, key), "{key:?}");
            assert_eq!(app, AppState::mockup_idle());
        }
    }

    /// While the cat reacts, its hint takes the row back from a one-off hint.
    #[test]
    fn a_reaction_hint_wins_over_the_not_yet_hint() {
        let mut app = AppState::mockup_idle();
        menu_key(&mut app, '?');
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &chomp(3));
        assert_eq!(row_text(&c, 35).trim(), "» c h o m p «");
    }

    /// The side panels step back by the director's dim during a drag, and the
    /// pointer is drawn below the file.
    #[test]
    fn panels_dim_during_drag_steps() {
        let vm = view(&AppState::mockup_idle());
        let idle = draw(FULL_SIZE, &vm, &resting(&vm));
        for (i, &(dx, dy)) in DRAG_DOC.iter().enumerate() {
            let cat = drag(i);
            assert!(cat.panel_dim > 0.0);
            let c = draw(FULL_SIZE, &vm, &cat);
            let bg = theme().roles.bg;
            // A corner of each side panel, clear of the file and the pointer.
            for (x, y) in [(1, 9), (110, 32)] {
                let want = idle.get(x, y).expect("cell");
                let got = c.get(x, y).expect("cell");
                assert_eq!(got.ch, want.ch);
                assert_eq!(got.fg, mix(want.fg, bg, cat.panel_dim), "step {i}");
            }
            let pointer = c.get(dx + 5, dy + 4).expect("cell");
            assert_eq!(pointer.ch, HALF);
            assert_eq!(pointer.fg, Rgb::from_u32(0x1a1020), "step {i}");
        }
    }

    // ── the batch and result views (T-22b) ───────────────────────────────

    /// Cells frames 03 and 05 fill with data the view model does not carry
    /// (T-22b's open question), as `(row, first column, end column)`. The
    /// layout leaves them blank on the panel's fill, so they are compared as
    /// blank: everything else is cell-exact.
    const BATCH_UNFILLED: &[(u16, u16, u16)] = &[
        // the parked file's page count and language guess
        (4, 83, 110),
        // version, pages, size and producer; the pipeline stage; the toolpath
        (14, 2, 64),
        (16, 2, 64),
        (17, 2, 64),
        // each finding's detail past its location (row 20 has `obj 14 0`),
        // and a carve summary row
        (20, 42, 64),
        (21, 34, 64),
        (22, 34, 64),
        (23, 2, 64),
        // the log
        (26, 2, 64),
        (27, 2, 64),
        (28, 2, 64),
        (29, 2, 64),
    ];
    const RESULT_UNFILLED: &[(u16, u16, u16)] = &[
        // the output's name and folder; the toolpath, time and re-diagnosis
        (4, 53, 110),
        (5, 53, 110),
        (6, 53, 110),
        // each finding's detail column
        (9, 96, 110),
        (10, 96, 110),
        (11, 96, 110),
        // the recovery rows and bars
        (15, 53, 110),
        (16, 53, 110),
        (17, 53, 110),
        (18, 53, 110),
        // a picked font's family; a substitution's caveat
        (22, 84, 110),
        (23, 93, 110),
    ];

    /// The golden `name` with the `spans` the view model cannot fill blanked.
    fn unfilled(name: &str, spans: &[(u16, u16, u16)]) -> Golden {
        let mut g = goldens::load(name);
        let t = theme();
        let blank = GoldenCell::Text {
            ch: ' ',
            fg: GoldenRgb(t.roles.body.0),
            bg: GoldenRgb(t.roles.bg.0),
        };
        let masked = |x: u16, y: u16| {
            spans
                .iter()
                .any(|&(sy, x0, x1)| sy == y && (x0..x1).contains(&x))
        };
        let w = usize::from(g.w);
        for &(y, x0, x1) in spans {
            for x in x0..x1 {
                // A span covers only data, never a border or the background.
                assert!(
                    !matches!(g.cell(x, y), GoldenCell::Text { ch: '║', .. }),
                    "({x}, {y}) is a border"
                );
                g.cells[usize::from(y) * w + usize::from(x)] = blank;
            }
        }
        g.blink.retain(|&(x, y)| !masked(x, y));
        g
    }

    /// The cat between reactions in `mood`.
    fn resting_in(mood: Mood) -> CatFrame {
        let mut d = Director::new();
        d.on(CatEvent::Mood(mood), Duration::ZERO);
        d.frame(Duration::ZERO)
    }

    /// Frame 03, cell-exact outside the data the view model lacks. The
    /// mockup draws the meme pose there ("closed", the idle mood's), not the
    /// needs-you pose the director gives this batch; the goldens test the
    /// layout, so the cat frame is the mockup's.
    #[test]
    fn batch_matches_frame_03() {
        let vm = view(&AppState::mockup_batch());
        let cat = resting_in(Mood::Idle);
        assert_eq!(screen(&vm, &cat), FullScreen::Batch);
        draw(FULL_SIZE, &vm, &cat).assert_matches(&unfilled("03-batch", BATCH_UNFILLED));
    }

    /// Frame 05 with its file menu open on the first item, cell-exact
    /// outside the data the view model lacks; the fixture has no C9 line.
    #[test]
    fn result_matches_frame_05() {
        let mut app = AppState::mockup_result();
        app.file_menu = Some(0);
        let vm = view(&app);
        assert_eq!(vm.c9_line, None);
        let cat = resting(&vm);
        assert_eq!(screen(&vm, &cat), FullScreen::Result);
        draw(FULL_SIZE, &vm, &cat).assert_matches(&unfilled("05-result", RESULT_UNFILLED));
    }

    /// D-041's count line has no row in frame 05, so it fills the RECOVERY
    /// rows (the recovery bars are not in the view model): whole, for one-,
    /// two- and three-digit counts (REPDF's C9 files carry 12 to 30 damaged
    /// streams), broken at spaces inside the panel.
    #[test]
    fn a_c9_count_line_fills_the_recovery_rows_whole() {
        for counts in [(3, 2, 1), (30, 28, 12), (100, 100, 100), (1000, 999, 1)] {
            let vm = view(&result_with_c9(None, counts));
            let line = vm
                .c9_line
                .clone()
                .expect("the repair found damaged streams");
            assert!(
                line.starts_with(&format!("{} streams repaired; ", counts.0)),
                "{line:?}"
            );
            let c = draw(FULL_SIZE, &vm, &resting(&vm));
            let mut parts = Vec::new();
            for y in 15..19 {
                let row: Vec<char> = row_text(&c, y).chars().collect();
                assert_eq!((row[52], row[110]), ('║', '║'), "row {y}");
                let inside: String = row[53..110].iter().collect();
                assert!(inside.starts_with(' '), "row {y}: {inside:?}");
                parts.push(inside.trim().to_string());
            }
            let drawn = parts
                .iter()
                .filter(|p| !p.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            assert_eq!(drawn, line, "{counts:?}");
            // The bottom edge stays a plain border.
            let row = row_text(&c, 35);
            assert_eq!(row, format!(" ╚{}╝ ", "═".repeat(108)), "{counts:?}");
        }
    }

    /// A failed file's reason (D-048's hint line) takes the result view's
    /// bottom edge: "decrypt first" for an encrypted file, which the short
    /// queue status drops, and a job's error, cut only at the box's corners.
    #[test]
    fn a_failed_files_reason_takes_the_result_views_bottom_edge() {
        let mut app = AppState::mockup_batch();
        app.selected = Some(6);
        let vm = view(&app);
        let cat = resting(&vm);
        assert_eq!(screen(&vm, &cat), FullScreen::Result);
        let row = row_text(&draw(FULL_SIZE, &vm, &cat), 35);
        assert!(row.contains("╡ decrypt first ╞"), "{row:?}");

        let error = "the output folder is not set: choose one in setup";
        let vm = view(&failed_with(error));
        let row = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 35);
        assert!(row.contains(&format!("╡ {error} ╞")), "{row:?}");

        let vm = view(&failed_with(&"e".repeat(200)));
        let row: Vec<char> = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 35)
            .chars()
            .collect();
        let drawn: String = row[4..108].iter().collect();
        assert_eq!(drawn, format!("{}…", "e".repeat(103)));
        assert_eq!(
            [row[1], row[2], row[3], row[108], row[109], row[110]],
            ['╚', '╡', ' ', ' ', '╞', '╝']
        );

        // A cancelled file has no reason to give.
        let mut app = AppState::mockup_batch();
        app.batch.entries[5].state = EntryState::Cancelled;
        app.selected = Some(5);
        let vm = view(&app);
        let row = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 35);
        assert_eq!(row, format!(" ╚{}╝ ", "═".repeat(108)));
    }

    /// The batch and result views have no hint row: a one-off hint ("not
    /// yet") takes the progress box's bottom edge, and wins over a failed
    /// file's reason while it is up.
    #[test]
    fn a_one_off_hint_takes_the_views_bottom_edge() {
        let mut app = AppState::mockup_batch();
        app.hint = Some(strings::NOT_YET);
        let vm = view(&app);
        let row = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 35);
        assert!(
            row.contains(&format!("╡ {} ╞", strings::NOT_YET)),
            "{row:?}"
        );
        let mut app = AppState::mockup_batch();
        app.selected = Some(6);
        app.hint = Some(strings::NOT_YET);
        let vm = view(&app);
        let row = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 35);
        assert!(row.contains(strings::NOT_YET), "{row:?}");
        assert!(!row.contains("decrypt first"), "{row:?}");
    }

    /// A list longer than its rows ends in a dim `… N more` on its last row:
    /// the analysis and result panels' findings (four rows) and the result
    /// panel's fonts (three rows).
    #[test]
    fn long_findings_and_font_lists_say_how_many_are_hidden() {
        let vm = view(&overflowing_result());
        assert_eq!(
            (vm.selected_findings.len(), vm.font_resolutions.len()),
            (6, 5)
        );
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        let cell_at = |y: u16, x: u16| c.get(x, y).expect("cell");
        let inside = |y: u16| -> String { row_text(&c, y).chars().skip(53).take(57).collect() };
        assert_eq!(inside(12).trim_end(), " … 3 more");
        assert_eq!(cell_at(12, 54).fg, theme().roles.dim);
        assert!(inside(11).contains("[WRN] C6 "), "{:?}", inside(11));
        assert_eq!(inside(23).trim_end(), " … 3 more");
        assert!(inside(22).contains("F3 CIDFont+F1"), "{:?}", inside(22));

        let vm = view(&overflowing_batch());
        assert_eq!(vm.selected_findings.len(), 6);
        let c = draw(FULL_SIZE, &vm, &resting_in(Mood::Idle));
        let inside = |y: u16| -> String { row_text(&c, y).chars().skip(2).take(62).collect() };
        assert_eq!(inside(23).trim_end(), " … 3 more");
        // The fourth finding, on page 3, is replaced by the count.
        assert!(!row_text(&c, 23).contains("p.3"));

        // Exactly as many as there are rows: no count.
        let vm = view(&AppState::mockup_result());
        assert_eq!(vm.selected_findings.len(), 4);
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        assert!(!row_text(&c, 12).contains("more"));
        assert!(row_text(&c, 12).contains("[iNF]    header intact"));
    }

    /// The analysis panel's detail column starts with the finding's location,
    /// dim: `obj 14 0` on frame 03's C9 row (the golden checks its cells),
    /// `p.N` for a page, nothing for the whole file.
    #[test]
    fn a_findings_location_leads_its_detail_column() {
        assert_eq!(
            findings::location(&Location::Object {
                id: (14, 0),
                span: None
            })
            .as_deref(),
            Some("obj 14 0")
        );
        assert_eq!(
            findings::location(&Location::Page {
                index: 2,
                obj: Some((5, 0))
            })
            .as_deref(),
            Some("p.3")
        );
        assert_eq!(findings::location(&Location::File), None);
        let mut app = AppState::mockup_batch();
        app.batch.entries[4].findings[1].location = Location::Page {
            index: u32::MAX,
            obj: None,
        };
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &resting_in(Mood::Idle));
        let row: String = row_text(&c, 21).chars().skip(34).take(30).collect();
        assert_eq!(row, format!("{:<30}", "p.4294967296"));
        assert_eq!(c.get(34, 21).expect("cell").fg, theme().roles.dim);
        let row: String = row_text(&c, 20).chars().skip(34).take(30).collect();
        assert_eq!(row, format!("{:<30}", "obj 14 0"));
    }

    #[test]
    fn wrap_breaks_at_spaces_and_cuts_what_does_not_fit() {
        assert_eq!(wrap("aa bb cc", 5, 3), ["aa bb", "cc"]);
        assert_eq!(wrap("aa bb cc dd", 5, 1), ["aa b…"]);
        assert_eq!(wrap("aa bbbb", 4, 1), ["aa…"]);
        assert_eq!(wrap("abcdefgh", 5, 2), ["abcd…"]);
        assert_eq!(wrap("", 5, 2), Vec::<String>::new());
    }

    /// Which screen is up: the main one with no files and during every
    /// reaction, then the result view on a finished file, else the batch.
    #[test]
    fn the_screen_follows_the_queue_the_cursor_and_the_cat() {
        let idle = view(&AppState::mockup_idle());
        let batch = view(&AppState::mockup_batch());
        let result = view(&AppState::mockup_result());
        let done = view(&AppState::mockup_done());
        let mut failed = AppState::mockup_batch();
        failed.selected = Some(6);
        let failed = view(&failed);
        let rest = resting_in(Mood::Idle);
        for (vm, cat, want) in [
            (&idle, &rest, FullScreen::Main),
            (&batch, &rest, FullScreen::Batch),
            (&result, &rest, FullScreen::Result),
            (&failed, &rest, FullScreen::Result),
            // a finished batch with the cursor on nothing
            (&done, &rest, FullScreen::Batch),
            (&batch, &chomp(3), FullScreen::Main),
            (&result, &drag(2), FullScreen::Main),
        ] {
            assert_eq!(screen(vm, cat), want);
        }
    }

    /// The menu is drawn only while it is open; its hotkeys replace the
    /// batch keys, an out-of-range cursor lands on the last item and one on
    /// the separator on the item after it.
    #[test]
    fn the_file_menu_is_drawn_only_while_open() {
        let vm = view(&AppState::mockup_result());
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        assert!(!row_text(&c, 7).contains(strings::FILE_MENU));
        assert!(row_text(&c, 36).contains("[enter] file menu"));
        let mut app = AppState::mockup_result();
        app.file_menu = Some(42);
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        assert!(row_text(&c, 7).contains(strings::FILE_MENU));
        assert!(row_text(&c, 10).contains(strings::EXPORT_MARKDOWN));
        assert!(row_text(&c, 15).contains("► Remove from queue"));
        assert!(row_text(&c, 36).contains("[esc] close menu"));
        // A cursor on the separator lands on the next item.
        app.file_menu = Some(4);
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        assert!(row_text(&c, 13).contains("► Repair options…"));
        assert_eq!(menu_item(4), 5);
        assert_eq!(menu_item(0), 0);
        assert_eq!(menu_item(99), 7);
    }

    /// A long name is cut with `…` in the panels' top edges, which stay
    /// whole, and the result panel's name stops short of its note.
    #[test]
    fn long_names_stay_inside_the_panel_titles() {
        let app = long_batch();
        let mut closed = app.clone();
        closed.file_menu = None;
        let vm = view(&closed);
        let row: Vec<char> = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 13)
            .chars()
            .collect();
        assert_eq!(&row[61..=64], &['…', ' ', '╞', '╗'], "{row:?}");
        let mut app = AppState::mockup_result();
        app.batch.entries[2].name = "n".repeat(80);
        let vm = view(&app);
        let row: String = row_text(&draw(FULL_SIZE, &vm, &resting(&vm)), 2);
        assert!(row.contains("… ╞═╡ √ repaired ╞═╗"), "{row:?}");
    }

    /// File names, finding summaries and font names are data: braces in them
    /// are drawn, not read as colour markup.
    #[test]
    fn data_in_the_views_is_drawn_verbatim() {
        let mut app = AppState::mockup_result();
        let thesis = &mut app.batch.entries[2];
        thesis.name = "x{R}.pdf".into();
        thesis.findings[0].summary = "a{G}b".into();
        thesis.font_resolutions[0].base_font = Some("/F{Y}nt".into());
        let vm = view(&app);
        let c = draw(FULL_SIZE, &vm, &resting(&vm));
        assert!(row_text(&c, 2).contains("RESULT » x{R}.pdf"));
        assert!(row_text(&c, 6).contains("x{R}.pdf"));
        assert!(row_text(&c, 9).contains("[ERR] C2 a{G}b"));
        assert!(row_text(&c, 21).contains("F1 F{Y}nt"));
    }

    #[test]
    fn the_layout_is_112_by_38_and_takes_input() {
        assert_eq!(FullLayout.min_size(), (112, 38));
        assert!(FullLayout.accepts_input());
    }

    /// The words the view model supplies (file names, statuses, finding
    /// summaries, font names, the C9 line): data, not the layout's strings,
    /// plus the stand-in for a font with no name, which stays out of the
    /// deny-list because the engine's reports say "unknown" too.
    fn data_words(vm: &ViewModel) -> Vec<String> {
        let mut text: Vec<String> = vm.recent.iter().map(|r| r.name.clone()).collect();
        for r in &vm.queue_rows {
            text.push(r.name.clone());
            text.push(r.label.to_string());
            text.extend(r.detail.clone());
            text.extend(r.short_detail.clone());
        }
        for f in &vm.selected_findings {
            text.push(f.summary.clone());
            text.extend(f.code.map(str::to_string));
            // `obj 14 0`, `p.3`: not in the deny-list (see `strings::LOC_OBJ`).
            text.extend(findings::location(&f.location));
        }
        for f in &vm.font_resolutions {
            text.push(f.slot.clone());
            text.extend(f.name.clone());
            if let Some(FontResolutionKind::Substituted(s)) = &f.resolution {
                text.push(s.label.clone());
            }
        }
        if let Some(cur) = &vm.current {
            text.push(cur.name.clone());
            text.push(cur.activity.label().to_string());
        }
        text.extend(vm.c9_line.clone());
        text.push(strings::FONT_UNKNOWN.to_string());
        let mut words = text.clone();
        words.extend(
            text.iter()
                .flat_map(|t| t.split_whitespace().map(str::to_string)),
        );
        words
    }

    /// Every word the layout draws comes from the string table (the T-14
    /// deny-list) or is a file's name, a number, a size or the clock: the drawn
    /// rows are read back, so a literal that skips `strings.rs` fails here.
    #[test]
    fn drawn_words_are_in_the_string_table() {
        let table: Vec<&str> = strings::ALL
            .iter()
            .flat_map(|s| s.split_whitespace())
            .collect();
        let mut frames = Vec::new();
        for vm in stress_states() {
            let names = data_words(&vm);
            let menu = vm.file_menu.is_some() && screen(&vm, &resting(&vm)) != FullScreen::Main;
            frames.push((draw(FULL_SIZE, &vm, &resting(&vm)), names, menu));
        }
        let vm = view(&AppState::mockup_idle());
        let names: Vec<String> = vm.recent.iter().map(|r| r.name.clone()).collect();
        for i in 0..CHOMP.len() {
            frames.push((draw(FULL_SIZE, &vm, &chomp(i)), names.clone(), false));
        }
        for i in 0..DRAG.len() {
            frames.push((draw(FULL_SIZE, &vm, &drag(i)), names.clone(), false));
        }
        // The open file menu's box: its columns and rows.
        let (mx, my, mw, mh) = MENU_BOX;
        let menu_cols = mx..mx + mw;
        let menu_rows = my..my + mh;
        let menu_keys: Vec<String> = strings::FILE_MENU_ITEMS
            .iter()
            .flatten()
            .map(|(_, k)| k.to_string())
            .collect();
        let cut = |ch: char| ch.is_whitespace() || "║╔╗╚╝╡╞╟╢═─│█".contains(ch) || ch == HALF;
        let mut words = 0;
        for (c, names, menu) in &frames {
            for y in 0..c.h {
                let row = row_text(c, y);
                let chars: Vec<char> = row.chars().collect();
                let in_menu_row = *menu && menu_rows.contains(&i32::from(y));
                let mut x = 0;
                while x < chars.len() {
                    if cut(chars[x]) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < chars.len() && !cut(chars[x]) {
                        x += 1;
                    }
                    let word: String = chars[start..x].iter().collect();
                    let word = word.as_str();
                    let (x0, x1) = (to_i32(start), to_i32(x));
                    // A piece of a name the open file menu is drawn over (the
                    // queue's names, the analysis panel's title): it ends at
                    // the menu's left border or starts right after its right
                    // border.
                    let covered = in_menu_row && (x1 == mx || x0 == mx + mw);
                    // A name's start cut at a box's right border (a queue
                    // row's status).
                    let clipped = chars.get(x) == Some(&'║');
                    let head = word.strip_suffix('…').unwrap_or(word);
                    let a_name = |n: &String| {
                        if covered {
                            n.contains(head)
                        } else if word.ends_with('…') || clipped {
                            n.starts_with(head)
                        } else {
                            n == word
                        }
                    };
                    let numeric = |w: &str| {
                        w.chars()
                            .all(|ch| ch.is_ascii_digit() || ":×…".contains(ch))
                    };
                    // A lone letter only in the open menu's key column.
                    let menu_key = in_menu_row
                        && menu_cols.contains(&x0)
                        && menu_keys.iter().any(|k| k == word);
                    if !word.chars().any(char::is_alphabetic)
                        || menu_key
                        || numeric(word)
                        || names.iter().any(a_name)
                    {
                        continue;
                    }
                    words += 1;
                    // A word cut short with `…` at a panel's edge, or a piece
                    // of one the dragged file is drawn over.
                    let under_file = row.contains(&format!("{word}{HALF}"))
                        || row.contains(&format!("{HALF}{word}"));
                    let cut_short = word
                        .strip_suffix('…')
                        .is_some_and(|head| table.iter().any(|t| t.starts_with(head)))
                        || (under_file && table.iter().any(|t| t.contains(word)));
                    let known = table.contains(&word)
                        || cut_short
                        || (word.chars().all(|ch| ch.is_ascii_digit()) && table.contains(&"{n}"));
                    assert!(known, "{word:?} (row {y}: {row:?}) is not in strings::ALL");
                }
            }
        }
        assert!(words > 500, "too few words read back: {words}");
    }
}
