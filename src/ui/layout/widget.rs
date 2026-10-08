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
use crate::ui::canvas::{Canvas, plain_len};
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
/// Where a kitty drag is told its drop is wanted (T-31): x, y, w, h. The
/// plate's columns, from the cat's top down to the hint row (columns 1–30,
/// rows 0–13).
const DROP_ZONE: (u16, u16, u16, u16) = (
    PLATE_AT.0 as u16,
    CAT_AT.1 as u16,
    PLATE_W,
    (LINE_Y - CAT_AT.1) as u16,
);
const W: i32 = WIDGET_SIZE.0 as i32;

/// The batch progress bar's width on the status line; it gives way to a
/// count too long for [`COUNT_X`].
const BAR_W: i32 = 24;
/// The mockup's columns for the working count (`3/7`) and the done tally
/// (`6√ 1~ 1×`); longer ones are moved left to stay inside the tile.
const COUNT_X: i32 = 27;
const TALLY_X: i32 = W - 13;
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

    fn drop_zone(&self) -> Option<(u16, u16, u16, u16)> {
        Some(DROP_ZONE)
    }

    /// Drawn under a 32 × 16 clip, so long counts or names cannot write past
    /// the tile on a larger canvas.
    fn draw(&self, c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
        c.clipped(WIDGET_SIZE.0, WIDGET_SIZE.1, |c| {
            draw_tile(c, vm, cat, theme)
        });
    }
}

/// The widget's cells; [`WidgetLayout::draw`] clips them to the tile.
fn draw_tile(c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme) {
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
            let (d, t) = (counts.done(), counts.total);
            let count = format!("{{W}}{d}{{D}}/{{W}}{t}");
            let x = right_anchored(COUNT_X, &count).max(2);
            let bar_w = BAR_W.min(x - 2);
            c.bar(1, LINE_Y, bar_w, pct, &theme.gradients.bar2, theme);
            c.rich(x, LINE_Y, &count, None, theme);
            let right = format!("{{W}}{done_of} ");
            // The name gives way to a long count, keeping a cell between them.
            let room = W - to_i32(plain_len(&app_name(Some("")))) - to_i32(plain_len(&right)) - 1;
            let max = usize::try_from(room).unwrap_or(0).min(WORKING_NAME);
            let name = vm
                .current
                .as_ref()
                .filter(|_| max >= 2)
                .map(|f| clip(&f.name, max));
            status(c, &app_name(name.as_deref()), &right, theme);
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
            let tally = format!(
                "{{G}}{}√ {{Y}}{}~ {{R}}{}×",
                counts.ok, counts.partial, counts.failed
            );
            let x = right_anchored(TALLY_X, &tally).max(1);
            let burp = strings::BURP.chars().count();
            // "burp." gives way to a tally too long to sit beside it.
            if matches!(vm.mood, Mood::Done { .. }) && x > 1 + to_i32(burp) {
                c.rich(1, LINE_Y, &format!("{{M}}{}", strings::BURP), None, theme);
            }
            c.rich(x, LINE_Y, &tally, None, theme);
            let done = fmt_n(strings::N_DONE, counts.done());
            status(c, &app_name(None), &format!("{{G}}{done} "), theme);
        }
    }
}

/// Where markup `text` starts on the status line: at `x` (the mockup's
/// column) while it fits, else moved left so it ends one cell short of the
/// tile's right edge.
fn right_anchored(x: i32, text: &str) -> i32 {
    x.min(W - 1 - to_i32(plain_len(text)))
}

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
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
/// edge, both markup on the light bar. `left` is cut a cell short of `right`.
fn status(c: &mut Canvas, left: &str, right: &str, theme: &Theme) {
    let bar = theme.roles.lightbar;
    c.fill(0, BAR_Y, W, 1, theme.roles.body, bar);
    let right_x = W - to_i32(plain_len(right));
    let left_w = if right.is_empty() { W } else { right_x - 1 };
    let left_w = u16::try_from(left_w.max(0)).unwrap_or(0);
    c.clipped(left_w, WIDGET_SIZE.1, |c| {
        c.rich(0, BAR_Y, left, Some(bar), theme)
    });
    if !right.is_empty() {
        c.rich(right_x, BAR_Y, right, Some(bar), theme);
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

    /// The drop zone (T-31) holds the drawn cat and its plate, above the
    /// hint row.
    #[test]
    fn the_drop_zone_holds_the_cat_and_its_plate() {
        let (zx, zy, zw, zh) = DROP_ZONE;
        let (zx, zy, zw, zh) = (i32::from(zx), i32::from(zy), i32::from(zw), i32::from(zh));
        let inside = |(x, y): (i32, i32), g: &cat::CellGrid| {
            let (w, h) = (i32::from(g.cols), i32::from(g.rows));
            x >= zx && y >= zy && x + w <= zx + zw && y + h <= zy + zh
        };
        let g = cat::render(&cat::Pose::default(), CAT_SCALE, theme(), 0.0);
        assert!(inside(CAT_AT, &g), "the cat");
        assert!(
            inside(PLATE_AT, &cat::plate(PLATE_W, Some(0.0))),
            "the plate"
        );
        assert_eq!(zy + zh, LINE_Y, "up to the hint row");
        assert_eq!(WidgetLayout.drop_zone(), Some(DROP_ZONE));
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
    /// Without it the status bar's right side is blank: see
    /// `a_drag_with_no_count_leaves_the_status_bar_right_blank`.
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

    /// What the shipped widget shows during a kitty drag today: the
    /// director's frame has no count (`n` is 0), so the right side of the
    /// status bar, `3 pdfs` in the mockup, stays blank until the drag source
    /// supplies one.
    #[test]
    fn a_drag_with_no_count_leaves_the_status_bar_right_blank() {
        let vm = view(&AppState::mockup_idle());
        for (i, &(x, y)) in WDRAG_DOC.iter().enumerate() {
            let mut d = widget_director();
            d.on(CatEvent::DragAt(Some(CellPos { x, y })), Duration::ZERO);
            let cat = d.frame(step_start(&DRAG_DURS, i));
            assert_eq!(cat.n, 0);
            let c = draw(&WidgetLayout, WIDGET_SIZE, &vm, &cat);
            let bar = row_text(&c, 15);
            assert_eq!(bar, format!(" {:<31}", strings::APP_NAME), "step {i}");
        }
    }

    /// The four moods from the fixtures, then the counts of a large batch:
    /// three- and four-digit counts that the mockup's columns cannot hold.
    fn widget_states() -> Vec<ViewModel> {
        let mut out: Vec<ViewModel> = [
            AppState::mockup_idle(),
            AppState::mockup_widget_working(),
            AppState::mockup_widget_needs(),
            AppState::mockup_done(),
        ]
        .iter()
        .map(view)
        .collect();
        for (ok, partial, failed, total) in [
            (14, 1, 0, 120),
            (100, 20, 3, 1000),
            (1000, 100, 10, 1110),
            (12345, 6789, 1011, 123_456),
        ] {
            for base in [
                AppState::mockup_widget_working(),
                AppState::mockup_widget_needs(),
                AppState::mockup_done(),
            ] {
                let mut vm = view(&base);
                vm.counts.ok = ok;
                vm.counts.partial = partial;
                vm.counts.failed = failed;
                vm.counts.total = total;
                if let Some((n, _)) = vm.needs_you.as_mut() {
                    *n = 1234;
                }
                out.push(vm);
            }
            let mut failed_vm = view(&AppState::mockup_done());
            failed_vm.mood = Mood::Failed;
            failed_vm.counts.failed = total;
            failed_vm.counts.total = total;
            out.push(failed_vm);
        }
        out
    }

    /// Large counts move left to end inside the tile: the working count
    /// shortens the bar, and the done tally stays clear of "burp." (or
    /// replaces it when the two cannot share the line).
    #[test]
    fn long_counts_stay_on_the_status_line() {
        let mut vm = view(&AppState::mockup_widget_working());
        vm.counts.ok = 15;
        vm.counts.partial = 0;
        vm.counts.total = 120;
        let c = draw(&WidgetLayout, WIDGET_SIZE, &vm, &resting(&vm));
        let line = row_text(&c, 14);
        assert!(line.ends_with(" 15/120 "), "{line:?}");
        // The file's name gives way to a long count on the status bar.
        vm.counts.ok = 19_134;
        vm.counts.total = 123_456;
        let c = draw(&WidgetLayout, WIDGET_SIZE, &vm, &resting(&vm));
        assert_eq!(
            row_text(&c, 15),
            format!(" {} invoic… 19134/123456 ", strings::APP_NAME)
        );
        let at = |i| line.chars().nth(i);
        assert_eq!(
            (at(23), at(24)),
            (Some('·'), Some(' ')),
            "the bar gives way: {line:?}"
        );

        let mut vm = view(&AppState::mockup_done());
        let mut tally_line = |ok, partial, failed| {
            vm.counts.ok = ok;
            vm.counts.partial = partial;
            vm.counts.failed = failed;
            let c = draw(&WidgetLayout, WIDGET_SIZE, &vm, &resting(&vm));
            (row_text(&c, 14), format!("{ok}√ {partial}~ {failed}×"))
        };
        // Moved left from column 19, "burp." kept.
        let (line, tally) = tally_line(1000, 100, 10);
        assert_eq!(line, format!(" {}{tally:>25} ", strings::BURP));
        // Too long to share the line: "burp." gives way.
        let (line, tally) = tally_line(123_456_789, 12_345_678, 1_234_567);
        assert_eq!(line, format!("{tally:>31} "));
    }

    /// The widget draws only inside its 32 × 16 cells, whatever the canvas
    /// and however long its counts.
    #[test]
    fn the_widget_stays_inside_its_area() {
        let size = (80, 24);
        let blank = Canvas::new(size.0, size.1, theme());
        for vm in widget_states() {
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

    /// Every word the widget and the fallback draw, in every state, comes
    /// from the string table (the deny-list) or is a file's name: the drawn
    /// rows are read back, so a literal that skips `strings.rs` fails here.
    #[test]
    fn drawn_words_are_in_the_string_table() {
        let table: Vec<&str> = strings::ALL
            .iter()
            .flat_map(|s| s.split_whitespace())
            .collect();
        let mut drawn: Vec<(Canvas, Vec<String>)> = Vec::new();
        let names = |vm: &ViewModel| {
            let mut n = Vec::new();
            if let Some(f) = &vm.current {
                n.push(f.name.clone());
            }
            if let Some((_, name)) = &vm.needs_you {
                n.push(name.clone());
            }
            n
        };
        for vm in widget_states() {
            let cat = resting(&vm);
            drawn.push((draw(&WidgetLayout, WIDGET_SIZE, &vm, &cat), names(&vm)));
            drawn.push((draw(&OneLine, (20, 1), &vm, &cat), Vec::new()));
        }
        let idle = view(&AppState::mockup_idle());
        for (i, &(x, y)) in WDRAG_DOC.iter().enumerate() {
            let mut d = widget_director();
            d.on(CatEvent::DragAt(Some(CellPos { x, y })), Duration::ZERO);
            let cat = d.frame(step_start(&DRAG_DURS, i));
            drawn.push((draw(&WidgetLayout, WIDGET_SIZE, &idle, &cat), Vec::new()));
            let cat = CatFrame { n: 3, ..cat };
            drawn.push((draw(&WidgetLayout, WIDGET_SIZE, &idle, &cat), Vec::new()));
        }
        let mut d = widget_director();
        d.on(CatEvent::Dropped { n: 3 }, Duration::ZERO);
        for i in 0..CHOMP.len() {
            let cat = d.frame(step_start(&CHOMP_DURS, i));
            drawn.push((draw(&WidgetLayout, WIDGET_SIZE, &idle, &cat), Vec::new()));
        }
        let mut words = 0;
        for (c, names) in &drawn {
            for y in 0..c.h {
                let row = row_text(c, y);
                for word in
                    row.split(|ch: char| ch.is_whitespace() || ch == crate::ui::canvas::HALF)
                {
                    // A file's name, whole or cut short with `…`.
                    let a_name = |n: &String| match word.strip_suffix('…') {
                        Some(head) => n.starts_with(head),
                        None => n == word,
                    };
                    if !word.chars().any(char::is_alphabetic) || names.iter().any(a_name) {
                        continue;
                    }
                    words += 1;
                    // `{n}` in a table entry stands for a count.
                    let known = table.contains(&word)
                        || (word.chars().all(|ch| ch.is_ascii_digit()) && table.contains(&"{n}"));
                    assert!(known, "{word:?} (row {y}: {row:?}) is not in strings::ALL");
                }
            }
        }
        assert!(words > 50, "too few words read back: {words}");
    }
}
