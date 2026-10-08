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
//! MENU draws `H history`, `S setup` and `? help` because the golden does, but
//! v1 has no screen behind them (D-048, flagged for the user): [`menu_key`]
//! answers those keys with the "not yet" hint and opens nothing.
#![cfg_attr(not(test), allow(dead_code))]

use super::{FULL_SIZE, Layout};
use crate::ui::canvas::Canvas;
use crate::ui::cat;
use crate::ui::color::{Rgb, mix};
use crate::ui::director::{CHOMP, CRUMBS, CRUMBS_AT, CatFrame};
use crate::ui::state::AppState;
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::{Role, ViewModel};
use crate::ui::widgets::r#box::{Panel, fit, panel};
use crate::ui::widgets::lightbar::{self, hk};
use crate::ui::widgets::logo;

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

    /// Drawn under a 112 × 38 clip, so nothing lands past the frame on a
    /// larger canvas.
    fn draw(&self, c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
        c.clipped(FULL_SIZE.0, FULL_SIZE.1, |c| draw_main(c, vm, cat, theme));
    }
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
        let row = format!(" {{{k}}}{} {{C}}{}", r.status.glyph(), fit(&r.name, name_w));
        lc.line(c, CALLERS.1 + 1 + to_i32(i), &row, theme);
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
    use crate::library::{HistorySummary, RecentRow, RecentStatus};
    use crate::ui::canvas::HALF;
    use crate::ui::director::{CHOMP_DURS, CatEvent, CellPos, DRAG, DRAG_DOC, DRAG_DURS, Director};
    use crate::ui::goldens;
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
        out
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

    #[test]
    fn the_layout_is_112_by_38_and_takes_input() {
        assert_eq!(FullLayout.min_size(), (112, 38));
        assert!(FullLayout.accepts_input());
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
            let names: Vec<String> = vm.recent.iter().map(|r| r.name.clone()).collect();
            frames.push((draw(FULL_SIZE, &vm, &resting(&vm)), names));
        }
        let vm = view(&AppState::mockup_idle());
        let names: Vec<String> = vm.recent.iter().map(|r| r.name.clone()).collect();
        for i in 0..CHOMP.len() {
            frames.push((draw(FULL_SIZE, &vm, &chomp(i)), names.clone()));
        }
        for i in 0..DRAG.len() {
            frames.push((draw(FULL_SIZE, &vm, &drag(i)), names.clone()));
        }
        let mut words = 0;
        for (c, names) in &frames {
            for y in 0..c.h {
                let row = row_text(c, y);
                let cut =
                    |ch: char| ch.is_whitespace() || "║╔╗╚╝╡╞╟╢═─│█".contains(ch) || ch == HALF;
                for word in row.split(cut) {
                    let a_name = |n: &String| match word.strip_suffix('…') {
                        Some(head) => n.starts_with(head),
                        None => n == word,
                    };
                    let numeric = |w: &str| {
                        w.chars()
                            .all(|ch| ch.is_ascii_digit() || ":×…".contains(ch))
                    };
                    if !word.chars().any(char::is_alphabetic)
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
