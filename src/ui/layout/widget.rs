//! The widget layout, 32 × 16 (T-21; UI D5), and the one-line fallback below
//! it, ported from generate.py's `frame_widget`.
//!
//! The cat sits at (1, 0) at half the full scale, its plate on rows 11–13, a
//! status line on row 14 and the status bar on row 15. While a reaction plays
//! (a file in view or being eaten) the status line is the director's hint;
//! between reactions it shows the batch's mood: the hint when idle, the
//! progress bar, the file waiting on you ("zoom me": decisions are never shown
//! in the widget), or the tally.
//!
//! Smaller than 32 × 16, [`OneLine`] draws `=^..^= 3/7 ‼` and nothing else. It
//! is status only and accepts no input (D-064).
#![cfg_attr(not(test), allow(dead_code))]

use super::{Layout, WIDGET_SIZE};
use crate::ui::canvas::Canvas;
use crate::ui::cat;
use crate::ui::color::Rgb;
use crate::ui::director::{CHOMP, CatFrame, DRAG, Mood};
use crate::ui::strings;
use crate::ui::theme::Theme;
use crate::ui::view::ViewModel;

/// The widget's cat scale: half the full layout's 0.93 (generate.py `WCAT_S`).
pub const CAT_SCALE: f64 = 0.93 / 2.0;
/// The cat's top-left cell.
const CAT_AT: (i32, i32) = (1, 0);
/// The plate's top-left cell and width; its 6 pixel rows fill rows 11–13.
const PLATE_AT: (i32, i32) = (1, 11);
const PLATE_W: u16 = 30;
/// The status line and the status bar.
const LINE_Y: i32 = 14;
const BAR_Y: i32 = 15;
const W: i32 = WIDGET_SIZE.0 as i32;

/// The batch progress bar's width on the status line.
const BAR_W: i32 = 24;
/// The longest file name the status bar shows while working, and the needs-you
/// line's; longer names end in `…`.
const WORKING_NAME: usize = 11;
const NEEDS_NAME: usize = 15;
/// The needs-you line's cells, which blink.
const NEEDS_SPAN: std::ops::Range<i32> = 2..29;

/// The dragged file at widget scale, 7 × 8 pixels (generate.py
/// `mini_doc_grid`).
const MINI_DOC: [&str; 8] = [
    "ooooo..", "oWWWfo.", "oWlWffo", "oWWWWWo", "oRRRRRo", "oWllWWo", "oWWWWWo", "ooooooo",
];

fn mini_doc() -> Vec<Vec<Option<Rgb>>> {
    MINI_DOC
        .iter()
        .map(|row| {
            row.chars()
                .map(|k| match k {
                    'o' => Some(Rgb::from_u32(0x8f8597)),
                    'W' => Some(Rgb::from_u32(0xf7f3f8)),
                    'f' => Some(Rgb::from_u32(0xc9bfcf)),
                    'l' => Some(Rgb::from_u32(0xb5acbb)),
                    'R' => Some(Rgb::from_u32(0xe0445c)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

/// The 32 × 16 tile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetLayout;

impl Layout for WidgetLayout {
    fn min_size(&self) -> (u16, u16) {
        WIDGET_SIZE
    }

    fn draw(&self, c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
        let (cx, cy) = CAT_AT;
        c.blit_grid(&cat::render(&cat.pose, CAT_SCALE, theme, cat.glow), cx, cy);
        if let Some(off) = cat.pose.plate {
            c.blit_grid(&cat::plate(PLATE_W, Some(off)), PLATE_AT.0, PLATE_AT.1);
        }
        if let Some(f) = cat.file_at {
            let doc = mini_doc();
            let first = usize::try_from(f.first_row).unwrap_or(0).min(doc.len());
            c.pix(f.x, f.pixel_row, &doc[first..]);
        }
        for &(x, y, text, k) in cat.fx {
            let fg = theme.slot(k);
            for (i, ch) in (x..).zip(text.chars()) {
                c.put(i, y, ch, fg, None);
            }
        }
        if reacting(cat) {
            hint(c, cat, theme);
            let right = if cat.hint == CHOMP[CHOMP.len() - 1].whint {
                format!("{{C}}{} ", fmt_n(strings::N_QUEUED, cat.n))
            } else if cat.n > 0 {
                format!("{{M}}{} ", fmt_n(strings::N_PDFS, cat.n))
            } else {
                String::new()
            };
            status(c, &app_name(None), &right, theme);
            return;
        }
        let counts = vm.counts;
        let done_of = format!("{}/{}", counts.done(), counts.total);
        match vm.mood {
            Mood::Idle => {
                hint(c, cat, theme);
                let right = if vm.status_bar.offline {
                    format!("{{G}}{} ", strings::OFFLINE)
                } else {
                    String::new()
                };
                status(c, &app_name(None), &right, theme);
            }
            Mood::Working { .. } => {
                let pct = vm
                    .batch_progress
                    .map_or(0.0, |r| r.num as f64 / r.den.max(1) as f64);
                c.bar(1, LINE_Y, BAR_W, pct, &theme.gradients.bar2, theme);
                let (d, t) = (counts.done(), counts.total);
                c.rich(27, LINE_Y, &format!("{{W}}{d}{{D}}/{{W}}{t}"), None, theme);
                let name = vm.current.as_ref().map(|f| clip(&f.name, WORKING_NAME));
                status(
                    c,
                    &app_name(name.as_deref()),
                    &format!("{{W}}{done_of} "),
                    theme,
                );
            }
            Mood::NeedsYou => {
                c.put(W - 2, 0, '‼', theme.slot('M'), None);
                c.set_blink(W - 2, 0);
                let (n, name) = vm
                    .needs_you
                    .as_ref()
                    .map_or((0, ""), |(n, name)| (*n, name.as_str()));
                let line = format!(
                    "{{M}}‼ {{C}}{} {{D}}· {{W}}{}",
                    clip(name, NEEDS_NAME),
                    strings::ZOOM_ME
                );
                c.rich(NEEDS_SPAN.start, LINE_Y, &line, None, theme);
                for x in NEEDS_SPAN {
                    c.set_blink(x, LINE_Y);
                }
                status(
                    c,
                    &app_name(None),
                    &format!("{{M}}‼ {n} {{W}}{done_of} "),
                    theme,
                );
            }
            Mood::Done { .. } | Mood::Failed => {
                if matches!(vm.mood, Mood::Done { .. }) {
                    c.rich(1, LINE_Y, &format!("{{M}}{}", strings::BURP), None, theme);
                }
                let tally = format!(
                    "{{G}}{}√ {{Y}}{}~ {{R}}{}×",
                    counts.ok, counts.partial, counts.failed
                );
                c.rich(W - 13, LINE_Y, &tally, None, theme);
                let done = fmt_n(strings::N_DONE, counts.done());
                status(c, &app_name(None), &format!("{{G}}{done} "), theme);
            }
        }
    }
}

/// A reaction is playing: a file is in view (the drag, the chomp's first
/// steps) or being eaten (the chomp to its end; the director counts its
/// files).
fn reacting(cat: &CatFrame) -> bool {
    cat.file_at.is_some() || cat.n > 0
}

/// The director's hint, centred on the status line and coloured out from both
/// ends: the "»…«" calls to action along `m`→`M`→`W`, the rest along the
/// theme's tagline gradient. "Release to feed" blinks.
fn hint(c: &mut Canvas, cat: &CatFrame, theme: &Theme) {
    let text = cat.hint_text();
    let n = i32::try_from(text.chars().count()).unwrap_or(W);
    let x = (W - n).div_euclid(2);
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
    c.gtext(x, LINE_Y, &text, stops, true, None);
    if cat.hint == DRAG[DRAG.len() - 1].whint {
        for i in x..x + n {
            c.set_blink(i, LINE_Y);
        }
    }
}

/// The status bar: `left` from the first column, `right` against the right
/// edge, both markup on the light bar.
fn status(c: &mut Canvas, left: &str, right: &str, theme: &Theme) {
    let bar = theme.roles.lightbar;
    c.fill(0, BAR_Y, W, 1, theme.roles.body, bar);
    c.rich(0, BAR_Y, left, Some(bar), theme);
    if !right.is_empty() {
        let n = i32::try_from(crate::ui::canvas::plain_len(right)).unwrap_or(W);
        c.rich(W - n, BAR_Y, right, Some(bar), theme);
    }
}

/// The status bar's left side: the name, and the file being worked on.
fn app_name(file: Option<&str>) -> String {
    match file {
        Some(f) => format!(" {{Y}}{} {{D}}{f}", strings::APP_NAME),
        None => format!(" {{Y}}{}", strings::APP_NAME),
    }
}

/// `name`, cut to `max` characters with a trailing `…` when longer.
fn clip(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut out: String = name.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A `{n}` string with the count filled in.
fn fmt_n(template: &str, n: usize) -> String {
    template.replace("{n}", &n.to_string())
}

/// Below 32 × 16: `=^..^= 3/7 ‼` on the top row, status only (UI:191, D-064).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OneLine;

impl Layout for OneLine {
    fn min_size(&self) -> (u16, u16) {
        (1, 1)
    }

    /// The face, then files done of the batch once there is one, then ‼
    /// (blinking) while a file waits on you; cut at the canvas's right edge.
    fn draw(&self, c: &mut Canvas, vm: &ViewModel, _cat: &CatFrame, theme: &Theme) {
        let mut line = format!("{{W}}{}", strings::FACE);
        if vm.counts.total > 0 {
            let (d, t) = (vm.counts.done(), vm.counts.total);
            line.push_str(&format!(" {{W}}{d}{{D}}/{{W}}{t}"));
        }
        let end = c.rich(0, 0, &line, None, theme);
        if vm.counts.needs_input > 0 {
            c.put(end + 1, 0, '‼', theme.slot('M'), None);
            c.set_blink(end + 1, 0);
        }
    }

    fn accepts_input(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::ui::canvas::CanvasCell;
    use crate::ui::director::{
        CHOMP_DURS, CatEvent, CellPos, DRAG_DURS, Director, Stage, WDRAG_DOC,
    };
    use crate::ui::goldens;
    use crate::ui::layout::{LayoutKind, LayoutPin, choose};
    use crate::ui::state::AppState;
    use crate::ui::view::view;

    fn theme() -> &'static Theme {
        Theme::default_theme()
    }

    /// When step `i` of a reaction starts, in milliseconds.
    fn step_start(durs: &[f64; 8], i: usize) -> Duration {
        let ms: u64 = durs[..i]
            .iter()
            .map(|d| (d * 1000.0).round_ties_even() as u64)
            .sum();
        Duration::from_millis(ms)
    }

    fn widget_director() -> Director {
        let mut d = Director::new();
        d.set_stage(Stage::Widget);
        d
    }

    /// The cat at rest in `vm`'s mood.
    fn resting(vm: &ViewModel) -> CatFrame {
        let mut d = widget_director();
        d.on(CatEvent::Mood(vm.mood), Duration::ZERO);
        d.frame(Duration::ZERO)
    }

    fn draw(layout: &dyn Layout, size: (u16, u16), vm: &ViewModel, cat: &CatFrame) -> Canvas {
        let mut c = Canvas::new(size.0, size.1, theme());
        layout.draw(&mut c, vm, cat, theme());
        c
    }

    fn assert_golden(name: &str, vm: &ViewModel, cat: &CatFrame) {
        let c = draw(&WidgetLayout, WIDGET_SIZE, vm, cat);
        c.assert_matches(&goldens::load(name));
    }

    /// The four mood states, from the view model of fixture app states.
    #[test]
    fn widget_mood_states_match_the_goldens() {
        for (name, app) in [
            ("07-widget-idle", AppState::mockup_idle()),
            ("07-widget-working", AppState::mockup_widget_working()),
            ("07-widget-needs-you", AppState::mockup_widget_needs()),
            ("07-widget-done", AppState::mockup_done()),
        ] {
            let vm = view(&app);
            assert_golden(name, &vm, &resting(&vm));
        }
    }

    /// The kitty drag, each step at its start, the file down the widget's
    /// edge. The mockup drags three files; the director does not know a
    /// drag's count (OSC 72 reports it on drop), so the test supplies it.
    #[test]
    fn widget_drag_steps_match_the_goldens() {
        let names = [
            "1-file-enters",
            "2-ears-up",
            "3-turns",
            "4-looks-up",
            "5-tracks-it",
            "6-jaw-drops",
            "7-wider",
            "8-ready-to-eat",
        ];
        let vm = view(&AppState::mockup_idle());
        for (i, &(x, y)) in WDRAG_DOC.iter().enumerate() {
            let mut d = widget_director();
            d.on(CatEvent::DragAt(Some(CellPos { x, y })), Duration::ZERO);
            let cat = CatFrame {
                n: 3,
                ..d.frame(step_start(&DRAG_DURS, i))
            };
            assert_golden(&format!("08-widget-drop-{}", names[i]), &vm, &cat);
        }
    }

    /// The chomp of three dropped files, each step at its start.
    #[test]
    fn widget_chomp_steps_match_the_goldens() {
        let names = [
            "1-plop",
            "2-looks-up",
            "3-jaw-drops",
            "4-chomp",
            "5-nom",
            "6-nom-nom",
            "7-gulp",
            "8-burp",
        ];
        let vm = view(&AppState::mockup_idle());
        let mut d = widget_director();
        d.on(CatEvent::Dropped { n: 3 }, Duration::ZERO);
        for (i, name) in names.iter().enumerate() {
            let cat = d.frame(step_start(&CHOMP_DURS, i));
            assert_golden(&format!("08b-widget-chomp-{name}"), &vm, &cat);
        }
    }

    /// The widget draws only inside its 32 × 16 cells, whatever the canvas.
    #[test]
    fn the_widget_stays_inside_its_area() {
        let size = (80, 24);
        let blank = Canvas::new(size.0, size.1, theme());
        for app in [
            AppState::mockup_idle(),
            AppState::mockup_widget_working(),
            AppState::mockup_widget_needs(),
            AppState::mockup_done(),
        ] {
            let vm = view(&app);
            let c = draw(&WidgetLayout, size, &vm, &resting(&vm));
            for y in 0..size.1 {
                for x in 0..size.0 {
                    if x >= WIDGET_SIZE.0 || y >= WIDGET_SIZE.1 {
                        assert_eq!(c.get(x, y), blank.get(x, y), "({x}, {y})");
                    }
                }
            }
            assert!(c.blink.iter().all(|&(x, y)| x < 32 && y < 16));
        }
    }

    fn row_text(c: &Canvas, y: u16) -> String {
        (0..c.w)
            .map(|x| c.get(x, y).expect("cell").ch)
            .collect::<String>()
    }

    /// At 20 × 1 the fallback draws the face, the count and ‼ in its one row
    /// and accepts no input.
    #[test]
    fn one_line_fallback_at_20_by_1() {
        let vm = view(&AppState::mockup_batch());
        let cat = resting(&vm);
        let c = draw(&OneLine, (20, 1), &vm, &cat);
        assert_eq!(c.cells.len(), 20);
        assert_eq!(row_text(&c, 0), "=^..^= 3/7 ‼        ");
        assert_eq!(c.blink.iter().copied().collect::<Vec<_>>(), [(11, 0)]);
        let t = theme();
        let at = |x| c.get(x, 0).expect("cell");
        assert_eq!(at(0).fg, t.roles.heading);
        assert_eq!(at(8).fg, t.roles.dim);
        assert_eq!(at(11).fg, t.roles.needs_input);
        assert!(!OneLine.accepts_input());
        assert!(WidgetLayout.accepts_input());
        assert_eq!(choose((20, 1), LayoutPin::Full), LayoutKind::OneLine);
        assert_eq!(OneLine.min_size(), (1, 1));
        assert_eq!(WidgetLayout.min_size(), (32, 16));
    }

    #[test]
    fn one_line_with_no_batch_is_the_face_alone() {
        let vm = view(&AppState::mockup_idle());
        let c = draw(&OneLine, (20, 1), &vm, &resting(&vm));
        assert_eq!(row_text(&c, 0), "=^..^=              ");
        assert!(c.blink.is_empty());
        let done = view(&AppState::mockup_done());
        let c = draw(&OneLine, (20, 1), &done, &resting(&done));
        assert_eq!(row_text(&c, 0), "=^..^= 7/8          ");
    }

    /// Narrower than the line, the fallback is cut at the edge; it never
    /// writes below its row.
    #[test]
    fn one_line_is_cut_at_the_edge_and_keeps_to_its_row() {
        let vm = view(&AppState::mockup_batch());
        let cat = resting(&vm);
        let c = draw(&OneLine, (8, 1), &vm, &cat);
        assert_eq!(row_text(&c, 0), "=^..^= 3");
        assert!(c.blink.is_empty());
        let c = draw(&OneLine, (31, 15), &vm, &cat);
        let blank = CanvasCell {
            ch: ' ',
            fg: theme().roles.body,
            bg: theme().roles.bg,
        };
        for y in 1..15 {
            for x in 0..31 {
                assert_eq!(c.get(x, y), Some(blank), "({x}, {y})");
            }
        }
    }

    #[test]
    fn clip_ends_long_names_in_an_ellipsis() {
        assert_eq!(clip("invoice_scan.pdf", 11), "invoice_sc…");
        assert_eq!(clip("thesis_ar.pdf", 15), "thesis_ar.pdf");
        assert_eq!(clip("ab", 2), "ab");
        assert_eq!(clip("abc", 2), "a…");
    }

    /// Every string the widget and the fallback draw is in the deny-list.
    #[test]
    fn drawn_strings_are_in_the_string_table() {
        for s in [
            strings::FACE,
            "‼",
            strings::APP_NAME,
            strings::OFFLINE,
            strings::ZOOM_ME,
            strings::BURP,
            strings::N_PDFS,
            strings::N_QUEUED,
            strings::N_DONE,
        ] {
            assert!(strings::ALL.contains(&s), "{s:?} not in strings::ALL");
        }
    }
}
