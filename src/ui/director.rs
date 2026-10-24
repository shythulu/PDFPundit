//! The cat's timeline (T-19): which pose, glow, hint and file position the cat
//! shows at a given time, from the events the loop feeds it.
//!
//! Two drop reactions, each a `const` table copied from generate.py (lines
//! 800–937 and 1261, the reference implementation; the T-00 goldens prove the
//! copy): the chomp ([`CHOMP`]), played when files land with no warning, and in
//! kitty the drag tracking ([`DRAG`]), which hands over to the chomp's step 4 on
//! release. Between reactions the cat shows the current [`Mood`]'s pose.
//!
//! Everything is a pure function of the event history and the time: there is no
//! clock in here. The loop stamps every event with `at` and asks for a frame at
//! `t`, both on the same monotonic clock (the time since the app started). The
//! mockup cuts hard from one keyframe to the next (its animations are CSS
//! `step-end`), so every step has `cut: true`; a step with `cut: false` would
//! blend linearly into the next over its hold.
//!
//! Positions are in the cells of the layout the director is staged for
//! ([`Stage`]): the full layout's 112 × 38 or the widget's 32 × 16, because the
//! mockup's file paths, effects and gazes differ between the two.
#![cfg_attr(not(test), allow(dead_code))]

use std::time::Duration;

use super::cat::{Pose, gaze_to};
use crate::pdf::model::Ratio;

/// A terminal cell, column then row, from the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPos {
    pub x: u16,
    pub y: u16,
}

/// What the batch is doing, as the cat shows it between reactions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Working { progress: Ratio },
    NeedsYou,
    Done { ok: u32, partial: u32, failed: u32 },
    Failed,
}

/// What the loop tells the director.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatEvent {
    /// kitty only (OSC 72): a file is dragged over the window, its top-left
    /// cell at the position; `None` = the drag left without a drop.
    DragAt(Option<CellPos>),
    /// `n` files landed on the cat.
    Dropped { n: usize },
    /// The batch's state changed. During a reaction it shows once the reaction
    /// ends.
    Mood(Mood),
}

/// Which layout the frames are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stage {
    /// 112 × 38, the cat at (27, 8), scale 0.93.
    #[default]
    Full,
    /// 32 × 16, the cat at (1, 0), scale 0.465.
    Widget,
}

/// Where the dragged or eaten file is drawn: its left cell `x`, its top edge in
/// pixel rows (two per cell) and the first of its pixel rows still showing (the
/// cat slurps it from the top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileAt {
    pub x: i32,
    pub pixel_row: i32,
    pub first_row: i32,
}

/// One frame of the cat, for the layout to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatFrame {
    pub pose: Pose,
    /// The drop ring's strength, 0..1.
    pub glow: f64,
    /// The hint row. `{n}` stands for [`CatFrame::n`]; see [`CatFrame::hint_text`].
    pub hint: &'static str,
    /// The step's name, as the mockup captions it.
    pub caption: &'static str,
    /// The file, while it is there to draw.
    pub file_at: Option<FileAt>,
    /// Text drawn over the scene: `(x, y, text, colour role)`, the role letter
    /// as the mockup's markup uses it (`'M'` = needs-input, …).
    pub fx: &'static [(i32, i32, &'static str, char)],
    /// Draw [`CRUMBS`] round [`CRUMBS_AT`] (full layout only).
    pub crumbs: bool,
    /// How far the side panels step back, 0..1.
    pub panel_dim: f64,
    /// The ‼ mark's blink phase while the cat needs you (2 Hz).
    pub blink_on: bool,
    /// How many files the current chomp is eating; 0 otherwise.
    pub n: usize,
}

impl CatFrame {
    /// The hint with `{n}` filled in.
    pub fn hint_text(&self) -> String {
        self.hint.replace("{n}", &self.n.to_string())
    }
}

/// One step of a reaction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub pose: Pose,
    pub glow: f64,
    /// The side-panel dim.
    pub dim: f64,
    /// The full layout's hint.
    pub hint: &'static str,
    /// The widget's hint.
    pub whint: &'static str,
    pub caption: &'static str,
    /// Hold this pose for the step (`true`), or blend into the next step's.
    pub cut: bool,
}

/// generate.py's `pose()`: the meme behind its plate.
const P: Pose = Pose {
    yaw: 0.0,
    pitch: 0.0,
    eo: 0.0,
    mouth: 0.0,
    gx: 0.0,
    gy: 0.0,
    ears: 0.0,
    happy: false,
    meme: 1.0,
    plate: Some(0.0),
    chew: false,
    puff: 0.0,
};

const HINT_DROP: &str = "·∙· drop a pdf on the cat ·∙·";
const HINT_NOTICE: &str = "·∙· the cat has noticed something ·∙·";
const HINT_FEED: &str = "» release to feed the cat «";
const WHINT_DROP: &str = "·∙ feed me a pdf ∙·";
const WHINT_NOTICE: &str = "·∙ the cat has noticed ∙·";
const WHINT_FEED: &str = "» release to feed «";

const fn key(
    pose: Pose,
    glow: f64,
    dim: f64,
    hint: &'static str,
    whint: &'static str,
    caption: &'static str,
) -> Key {
    Key {
        pose,
        glow,
        dim,
        hint,
        whint,
        caption,
        cut: true,
    }
}

/// The drag, kitty only (generate.py `DRAG`); the widget plays it too.
pub const DRAG: [Key; 8] = [
    key(P, 0.0, 0.12, HINT_DROP, WHINT_DROP, "the meme: file enters"),
    key(
        Pose {
            ears: 0.6,
            eo: 0.3,
            meme: 0.6,
            plate: Some(0.5),
            ..P
        },
        0.0,
        0.22,
        HINT_NOTICE,
        WHINT_NOTICE,
        "ears up",
    ),
    key(
        Pose {
            yaw: 0.25,
            pitch: 0.12,
            ears: 1.0,
            eo: 0.7,
            meme: 0.2,
            plate: Some(2.0),
            ..P
        },
        0.0,
        0.32,
        HINT_NOTICE,
        WHINT_NOTICE,
        "turns",
    ),
    key(
        Pose {
            yaw: 0.42,
            pitch: 0.22,
            ears: 1.0,
            eo: 1.0,
            mouth: 0.1,
            meme: 0.0,
            plate: Some(4.0),
            ..P
        },
        0.0,
        0.42,
        HINT_NOTICE,
        WHINT_NOTICE,
        "looks up",
    ),
    key(
        Pose {
            yaw: 0.36,
            pitch: 0.15,
            ears: 0.8,
            eo: 1.0,
            mouth: 0.3,
            meme: 0.0,
            plate: Some(7.0),
            ..P
        },
        0.3,
        0.48,
        HINT_NOTICE,
        WHINT_NOTICE,
        "tracks it",
    ),
    key(
        Pose {
            yaw: 0.24,
            pitch: 0.06,
            ears: 0.6,
            eo: 1.0,
            mouth: 0.55,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.6,
        0.55,
        HINT_FEED,
        WHINT_FEED,
        "jaw drops",
    ),
    key(
        Pose {
            yaw: 0.12,
            ears: 0.4,
            eo: 1.0,
            mouth: 0.8,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.85,
        0.55,
        HINT_FEED,
        WHINT_FEED,
        "wider",
    ),
    key(
        Pose {
            yaw: 0.05,
            pitch: -0.02,
            ears: 0.3,
            eo: 1.0,
            mouth: 1.0,
            meme: 0.0,
            plate: None,
            ..P
        },
        1.0,
        0.55,
        HINT_FEED,
        WHINT_FEED,
        "ready to eat",
    ),
];

/// Seconds per drag step; the last is held for as long as the drag lasts.
pub const DRAG_DURS: [f64; 8] = [0.9, 0.22, 0.22, 0.3, 0.24, 0.22, 0.22, 1.7];

/// The mockup's scripted drag path, the file's top-left cell per step, full
/// layout. The app follows the real pointer instead; the tests drive the drag
/// along this path to reproduce the goldens.
pub const DRAG_DOC: [(u16, u16); 8] = [
    (100, 7),
    (94, 10),
    (88, 13),
    (82, 16),
    (77, 19),
    (72, 21),
    (68, 23),
    (64, 24),
];

/// [`DRAG_DOC`] for the widget: down the edge, clear of the eyes.
pub const WDRAG_DOC: [(u16, u16); 8] = [
    (25, 0),
    (25, 1),
    (25, 2),
    (25, 3),
    (25, 5),
    (24, 7),
    (21, 8),
    (18, 9),
];

/// The drop reaction everywhere (generate.py `CHOMP`).
pub const CHOMP: [Key; 8] = [
    key(
        Pose {
            eo: 0.5,
            ears: 0.6,
            meme: 0.4,
            ..P
        },
        0.0,
        0.15,
        "·∙· plop · {n} pdfs landed on the cat ·∙·",
        "·∙ plop · {n} pdfs ∙·",
        "the drop: plop",
    ),
    key(
        Pose {
            pitch: 0.18,
            ears: 1.0,
            eo: 1.0,
            mouth: 0.15,
            meme: 0.0,
            plate: Some(2.0),
            ..P
        },
        0.3,
        0.3,
        HINT_NOTICE,
        WHINT_NOTICE,
        "looks up",
    ),
    key(
        Pose {
            pitch: 0.05,
            ears: 0.8,
            eo: 1.0,
            mouth: 0.8,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.6,
        0.45,
        "» the cat is eating your pdfs «",
        "» nom time «",
        "jaw drops",
    ),
    key(
        Pose {
            chew: true,
            puff: 0.6,
            ears: 0.3,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.0,
        0.45,
        "» c h o m p «",
        "» c h o m p «",
        "chomp",
    ),
    key(
        Pose {
            chew: true,
            puff: 1.0,
            ears: 0.2,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.0,
        0.4,
        "·∙· nom ·∙·",
        "·∙ nom ∙·",
        "nom",
    ),
    key(
        Pose {
            chew: true,
            puff: 0.5,
            ears: 0.2,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.0,
        0.35,
        "·∙· nom nom nom ·∙·",
        "·∙ nom nom nom ∙·",
        "nom nom",
    ),
    key(
        Pose {
            eo: 0.5,
            pitch: -0.05,
            puff: 0.15,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.0,
        0.25,
        "·∙· gulp ·∙·",
        "·∙ gulp ∙·",
        "gulp",
    ),
    key(
        Pose {
            happy: true,
            meme: 0.0,
            plate: None,
            ..P
        },
        0.0,
        0.1,
        "·∙· burp. {n} pdfs queued for repair ·∙·",
        "·∙ burp. {n} queued ∙·",
        "burp",
    ),
];

/// Seconds per chomp step; after the last the cat goes back to its mood.
pub const CHOMP_DURS: [f64; 8] = [0.45, 0.3, 0.2, 0.45, 0.28, 0.28, 0.35, 1.6];

/// The file during the chomp, full layout: (x cell, top pixel row, first doc
/// row drawn), or `None`. From "chomp" on it hangs from the mouth and is
/// slurped up; a first row past the file's 16 leaves only the crumbs.
pub const CHOMP_DOC: [Option<(i32, i32, i32)>; 8] = [
    Some((50, 18, 0)),
    Some((50, 18, 0)),
    Some((50, 34, 0)),
    Some((50, 54, 4)),
    Some((50, 54, 9)),
    Some((50, 54, 16)),
    None,
    None,
];

/// [`CHOMP_DOC`] for the widget, whose file is 8 pixel rows; no crumbs at this
/// size.
pub const WCHOMP_DOC: [Option<(i32, i32, i32)>; 8] = [
    Some((12, 3, 0)),
    Some((12, 3, 0)),
    Some((12, 11, 0)),
    Some((12, 19, 2)),
    Some((12, 19, 5)),
    None,
    None,
    None,
];

type Fx = &'static [(i32, i32, &'static str, char)];

/// The chomp's effects, full layout. Step 1 carries the sparkles generate.py's
/// `frame_chomp` draws round the landing file; the rest is its `CHOMP_FX`.
pub const CHOMP_FX: [Fx; 8] = [
    &[
        (48, 9, "*", 'M'),
        (63, 8, "·", 'Y'),
        (47, 12, "∙", 'C'),
        (62, 13, "*", 'm'),
    ],
    &[],
    &[],
    &[(79, 10, "CHOMP!", 'M')],
    &[(28, 12, "nom", 'm')],
    &[(28, 12, "nom", 'm'), (81, 15, "nom", 'M')],
    &[(80, 12, "gulp", 'C')],
    &[],
];

/// The chomp's effects, widget.
pub const WCHOMP_FX: [Fx; 8] = [
    &[],
    &[],
    &[],
    &[],
    &[(28, 1, "nom", 'm')],
    &[(28, 1, "nom", 'M')],
    &[],
    &[],
];

/// Crumb pixels once the file is gone: (cell offset, pixel-row offset, kind)
/// from [`CRUMBS_AT`]. `"paper"` is the theme's `W` mixed 40% towards white,
/// `"ink"` is `#e0445c`.
pub const CRUMBS: [(i32, i32, &str); 5] = [
    (-3, 9, "paper"),
    (2, 11, "ink"),
    (12, 10, "paper"),
    (15, 7, "paper"),
    (-5, 4, "ink"),
];

/// Where the file was last seen (x cell, top pixel row), full layout.
pub const CRUMBS_AT: (i32, i32) = (50, 54);

/// The full idle screen's sparkles (generate.py `frame_main`).
const IDLE_FX: Fx = &[
    (30, 10, "*", 'M'),
    (80, 11, "∙", 'C'),
    (83, 20, "+", 'Y'),
    (29, 25, "∙", 'W'),
    (82, 29, "*", 'm'),
    (28, 18, "·", 'B'),
];

// Each reaction is at most 10 keyframes, as data (UI:202).
const _: () = assert!(DRAG.len() <= 10 && CHOMP.len() <= 10);
const _: () = assert!(DRAG_DURS.len() == DRAG.len() && CHOMP_DURS.len() == CHOMP.len());

/// The chomp's "chomp" step, where a tracked drag carries on.
const CHOMP_AFTER_DRAG: usize = 3;

/// The mood poses (generate.py `POSES` and `WPOSES`).
const HAPPY: Pose = Pose {
    happy: true,
    meme: 0.0,
    plate: None,
    ..P
};
const NEEDS: Pose = Pose {
    eo: 1.0,
    ears: 1.0,
    meme: 0.0,
    plate: None,
    gy: 0.1,
    ..P
};
/// No mockup frame shows a failed batch: eyes half shut, ears folded down.
const FAILED: Pose = Pose {
    eo: 0.5,
    ears: -1.0,
    meme: 0.0,
    plate: None,
    ..P
};

impl Stage {
    /// The cat's top-left cell and scale; the file's size in pixels.
    fn geometry(self) -> ((f64, f64), f64, (f64, f64)) {
        match self {
            Stage::Full => ((27.0, 8.0), 0.93, (12.0, 16.0)),
            Stage::Widget => ((1.0, 0.0), 0.93 / 2.0, (7.0, 8.0)),
        }
    }

    /// How many pixel rows the file is.
    fn doc_rows(self) -> i32 {
        match self {
            Stage::Full => 16,
            Stage::Widget => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Rest,
    Drag {
        since: Duration,
        at: CellPos,
    },
    Chomp {
        since: Duration,
        from: usize,
        n: usize,
    },
}

/// The cat's state machine: rest → (drag →) chomp → rest.
#[derive(Clone, Debug, PartialEq)]
pub struct Director {
    stage: Stage,
    mood: Mood,
    act: Act,
}

impl Default for Director {
    fn default() -> Director {
        Director::new()
    }
}

impl Director {
    /// An idle cat, staged for the full layout.
    pub fn new() -> Director {
        Director {
            stage: Stage::Full,
            mood: Mood::Idle,
            act: Act::Rest,
        }
    }

    /// Which layout the frames are drawn for; the loop sets it whenever the
    /// layout changes.
    pub fn set_stage(&mut self, stage: Stage) {
        self.stage = stage;
    }

    /// Applies `ev`, which happened at `at`.
    pub fn on(&mut self, ev: CatEvent, at: Duration) {
        if let Act::Chomp { since, from, .. } = self.act
            && at >= since + total(&CHOMP_DURS[from..])
        {
            self.act = Act::Rest;
        }
        match ev {
            CatEvent::DragAt(Some(pos)) => match &mut self.act {
                Act::Rest => self.act = Act::Drag { since: at, at: pos },
                Act::Drag { at: p, .. } => *p = pos,
                // The cat finishes eating first.
                Act::Chomp { .. } => {}
            },
            CatEvent::DragAt(None) => {
                if matches!(self.act, Act::Drag { .. }) {
                    self.act = Act::Rest;
                }
            }
            CatEvent::Dropped { n } => {
                let from = match self.act {
                    Act::Drag { .. } => CHOMP_AFTER_DRAG,
                    _ => 0,
                };
                self.act = Act::Chomp { since: at, from, n };
            }
            CatEvent::Mood(m) => self.mood = m,
        }
    }

    /// The cat at `t`.
    pub fn frame(&self, t: Duration) -> CatFrame {
        match self.act {
            Act::Rest => self.rest(t),
            Act::Drag { since, at } => {
                let (i, f) = step_at(&DRAG_DURS, 0, t.saturating_sub(since))
                    .unwrap_or((DRAG.len() - 1, 0.0));
                let file = FileAt {
                    x: i32::from(at.x),
                    pixel_row: 2 * i32::from(at.y),
                    first_row: 0,
                };
                let mut fr = self.keyed(&DRAG, i, f, 0);
                fr.file_at = Some(file);
                self.aim(&mut fr.pose, file);
                fr
            }
            Act::Chomp { since, from, n } => {
                let Some((i, f)) = step_at(&CHOMP_DURS, from, t.saturating_sub(since)) else {
                    return self.rest(t);
                };
                let mut fr = self.keyed(&CHOMP, i, f, n);
                let (doc, fx) = match self.stage {
                    Stage::Full => (CHOMP_DOC[i], CHOMP_FX[i]),
                    Stage::Widget => (WCHOMP_DOC[i], WCHOMP_FX[i]),
                };
                fr.fx = fx;
                if let Some((x, pixel_row, first_row)) = doc {
                    let file = FileAt {
                        x,
                        pixel_row,
                        first_row,
                    };
                    self.aim(&mut fr.pose, file);
                    if first_row < self.stage.doc_rows() {
                        fr.file_at = Some(file);
                    } else {
                        fr.crumbs = self.stage == Stage::Full;
                    }
                }
                fr
            }
        }
    }

    /// Step `i` of `keys`, `f` of the way through its hold.
    fn keyed(&self, keys: &[Key; 8], i: usize, f: f64, n: usize) -> CatFrame {
        let k = &keys[i];
        let (pose, glow, dim) = match keys.get(i + 1) {
            Some(next) if !k.cut => (
                lerp_pose(&k.pose, &next.pose, f),
                lerp(k.glow, next.glow, f),
                lerp(k.dim, next.dim, f),
            ),
            _ => (k.pose, k.glow, k.dim),
        };
        CatFrame {
            pose,
            glow,
            hint: match self.stage {
                Stage::Full => k.hint,
                Stage::Widget => k.whint,
            },
            caption: k.caption,
            file_at: None,
            fx: &[],
            crumbs: false,
            panel_dim: dim,
            blink_on: false,
            n,
        }
    }

    /// generate.py's `gaze_to`: open eyes look at the middle of the file.
    fn aim(&self, pose: &mut Pose, file: FileAt) {
        if pose.eo <= 0.0 {
            return;
        }
        let ((cx, cy), scale, (dw, dh)) = self.stage.geometry();
        let x = f64::from(file.x);
        let y = f64::from(file.pixel_row) / 2.0;
        gaze_to(
            pose,
            (
                (x + dw / 2.0 - cx) / scale,
                ((y - cy) * 2.0 + dh / 2.0) / scale,
            ),
        );
    }

    /// The cat between reactions.
    fn rest(&self, t: Duration) -> CatFrame {
        let (pose, caption) = match self.mood {
            Mood::Idle => (P, "idle"),
            Mood::Working { .. } => (HAPPY, "working"),
            Mood::NeedsYou => (NEEDS, "needs"),
            Mood::Done { .. } => (HAPPY, "done"),
            Mood::Failed => (FAILED, "ears down"),
        };
        let (hint, fx) = match self.stage {
            Stage::Full if self.mood == Mood::Idle => (HINT_DROP, IDLE_FX),
            Stage::Full => (HINT_DROP, &[][..]),
            Stage::Widget => (WHINT_DROP, &[][..]),
        };
        CatFrame {
            pose,
            glow: 0.0,
            hint,
            caption,
            file_at: None,
            fx,
            crumbs: false,
            panel_dim: 0.0,
            blink_on: self.mood == Mood::NeedsYou && (t.as_millis() / 250).is_multiple_of(2),
            n: 0,
        }
    }
}

/// A step's length, in whole milliseconds (every mockup duration is a multiple
/// of 10 ms, so the rounding is exact and no float sum decides a boundary).
fn ms(secs: f64) -> u128 {
    // Small and non-negative, so the cast is exact.
    (secs * 1000.0).round_ties_even() as u128
}

fn total(durs: &[f64]) -> Duration {
    let sum: u128 = durs.iter().map(|&d| ms(d)).sum();
    Duration::from_millis(u64::try_from(sum).unwrap_or(u64::MAX))
}

/// The step `elapsed` falls in, counting from step `from`, and how far through
/// it; `None` once the last step's hold is over.
fn step_at(durs: &[f64; 8], from: usize, elapsed: Duration) -> Option<(usize, f64)> {
    let mut left = elapsed.as_millis();
    for (i, &d) in durs.iter().enumerate().skip(from) {
        let len = ms(d);
        if left < len {
            // Both below a few thousand, so the conversions are exact.
            return Some((i, left as f64 / len as f64));
        }
        left -= len;
    }
    None
}

fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}

/// Blends every number; flags, and a plate that comes or goes, follow `a`.
fn lerp_pose(a: &Pose, b: &Pose, f: f64) -> Pose {
    Pose {
        yaw: lerp(a.yaw, b.yaw, f),
        pitch: lerp(a.pitch, b.pitch, f),
        eo: lerp(a.eo, b.eo, f),
        mouth: lerp(a.mouth, b.mouth, f),
        gx: lerp(a.gx, b.gx, f),
        gy: lerp(a.gy, b.gy, f),
        ears: lerp(a.ears, b.ears, f),
        meme: lerp(a.meme, b.meme, f),
        plate: match (a.plate, b.plate) {
            (Some(p), Some(q)) => Some(lerp(p, q, f)),
            (p, _) => p,
        },
        puff: lerp(a.puff, b.puff, f),
        ..*a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cat::{self, CellGrid};
    use crate::ui::goldens::{self, GoldenCell, GoldenPose};
    use crate::ui::theme::Theme;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// When step `i` of `durs` starts, counting from step `from`.
    fn start(durs: &[f64; 8], from: usize, i: usize) -> Duration {
        total(&durs[from..i])
    }

    fn pose_of(g: &GoldenPose) -> Pose {
        Pose {
            yaw: g.yaw,
            pitch: g.pitch,
            eo: g.eo,
            mouth: g.mouth,
            gx: g.gx,
            gy: g.gy,
            ears: g.ears,
            happy: g.happy,
            meme: g.meme,
            plate: g.plate,
            chew: g.chew,
            puff: g.puff,
        }
    }

    fn cell(grid: &CellGrid, x: u16, y: u16) -> GoldenCell {
        match grid.get(x, y) {
            None => GoldenCell::Empty,
            Some(c) => {
                let (top, bottom) = c.halves();
                GoldenCell::Half {
                    top: top.map(|c| goldens::Rgb(c.0)),
                    bottom: bottom.map(|c| goldens::Rgb(c.0)),
                }
            }
        }
    }

    /// `frame` equals the golden pose `key` bit for bit, glow included, and the
    /// pose renders to that key's golden at full (0.93) and widget (0.465)
    /// scale, cell-exact.
    #[track_caller]
    fn assert_keyframe(frame: &CatFrame, key: &str) {
        let theme = Theme::default_theme();
        for scale in [0.93, 0.465] {
            let g = goldens::load(&goldens::cat_name(scale, key));
            let want = pose_of(&g.pose.expect("pose"));
            assert_eq!(frame.pose, want, "{key}: pose");
            for (a, b) in [(frame.pose.gx, want.gx), (frame.pose.gy, want.gy)] {
                assert_eq!(a.to_bits(), b.to_bits(), "{key}: gaze bits");
            }
            assert_eq!(Some(frame.glow), g.glow, "{key}: glow");
            let grid = cat::render(&frame.pose, scale, theme, frame.glow);
            g.assert_matches(grid.cols, grid.rows, |x, y| cell(&grid, x, y));
        }
    }

    /// Every drag and chomp keyframe, full and widget, reaches its golden: the
    /// drag driven along the mockup's file path, the chomp from a plain drop.
    #[test]
    fn every_keyframe_renders_to_its_golden() {
        for (stage, docs, prefix) in [(Stage::Full, DRAG_DOC, ""), (Stage::Widget, WDRAG_DOC, "w")]
        {
            for (i, &(x, y)) in docs.iter().enumerate() {
                let mut d = Director::new();
                d.set_stage(stage);
                d.on(CatEvent::DragAt(Some(CellPos { x, y })), ms(0));
                let f = d.frame(start(&DRAG_DURS, 0, i));
                assert_eq!(f.caption, DRAG[i].caption);
                assert_keyframe(&f, &format!("{prefix}drag-{}", i + 1));
            }
            let mut d = Director::new();
            d.set_stage(stage);
            d.on(CatEvent::Dropped { n: 3 }, ms(0));
            for (i, k) in CHOMP.iter().enumerate() {
                let f = d.frame(start(&CHOMP_DURS, 0, i));
                assert_eq!(f.caption, k.caption);
                assert_keyframe(&f, &format!("{prefix}chomp-{}", i + 1));
            }
        }
    }

    fn dragged(at: Duration) -> Director {
        let mut d = Director::new();
        for (i, &(x, y)) in DRAG_DOC.iter().enumerate() {
            let when = at + start(&DRAG_DURS, 0, i);
            d.on(CatEvent::DragAt(Some(CellPos { x, y })), when);
        }
        d
    }

    /// The end of the chomp's last hold, for a chomp started at `at` from `from`.
    fn chomp_end(at: Duration, from: usize) -> Duration {
        at + total(&CHOMP_DURS[from..])
    }

    #[test]
    fn a_drop_after_a_drag_skips_to_chomp() {
        let mut d = dragged(ms(0));
        let at = ms(5000);
        assert_eq!(d.frame(at).caption, "ready to eat");
        d.on(CatEvent::Dropped { n: 2 }, at);
        let f = d.frame(at);
        assert_eq!((f.caption, f.hint), ("chomp", "» c h o m p «"));
        assert_eq!(f.n, 2);
        // steps 4..8 only: burp starts after 0.45 + 0.28 + 0.28 + 0.35 s
        assert_eq!(d.frame(at + ms(1359)).caption, "gulp");
        let burp = d.frame(at + ms(1360));
        assert_eq!(burp.caption, "burp");
        assert_eq!(burp.hint_text(), "·∙· burp. 2 pdfs queued for repair ·∙·");
    }

    #[test]
    fn both_reactions_end_on_burp_then_the_mood() {
        for (mut d, at, from) in [
            (Director::new(), ms(0), 0),
            (dragged(ms(0)), ms(3000), CHOMP_AFTER_DRAG),
        ] {
            d.on(CatEvent::Dropped { n: 3 }, at);
            let end = chomp_end(at, from);
            assert_eq!(d.frame(end - ms(1)).caption, "burp");
            assert_eq!(d.frame(end), Director::new().frame(end), "back to idle");
        }
    }

    #[test]
    fn a_drag_that_leaves_returns_to_idle() {
        let mut d = dragged(ms(0));
        assert_eq!(d.frame(ms(1500)).caption, "looks up");
        d.on(CatEvent::DragAt(None), ms(1500));
        assert_eq!(d.frame(ms(1500)), Director::new().frame(ms(1500)));
        // and a later drop is a plain chomp
        d.on(CatEvent::Dropped { n: 1 }, ms(2000));
        assert_eq!(d.frame(ms(2000)).caption, "the drop: plop");
    }

    /// The drag plays through once, then holds "ready to eat" while the file
    /// hovers; the pupils follow it.
    #[test]
    fn the_drag_holds_its_last_step_and_follows_the_file() {
        let mut d = Director::new();
        d.on(CatEvent::DragAt(Some(CellPos { x: 64, y: 24 })), ms(0));
        let held = d.frame(ms(60_000));
        assert_eq!(held.caption, "ready to eat");
        assert_eq!(
            held.file_at,
            Some(FileAt {
                x: 64,
                pixel_row: 48,
                first_row: 0
            })
        );
        d.on(CatEvent::DragAt(Some(CellPos { x: 10, y: 2 })), ms(60_000));
        let moved = d.frame(ms(60_000));
        assert_eq!(moved.caption, "ready to eat", "moving keeps the step");
        assert!(moved.pose.gx < 0.0 && moved.pose.gy < 0.0, "looks up-left");
    }

    #[test]
    fn a_mood_during_a_reaction_shows_when_it_ends() {
        let mut d = Director::new();
        d.on(CatEvent::Dropped { n: 3 }, ms(0));
        d.on(CatEvent::Mood(Mood::NeedsYou), ms(100));
        assert_eq!(d.frame(ms(100)).caption, "the drop: plop");
        let end = chomp_end(ms(0), 0);
        assert_eq!(d.frame(end).caption, "needs");
    }

    /// The cat looks straight out at the viewer, and ‼ blinks at 2 Hz: on for
    /// 250 ms, off for 250 ms.
    #[test]
    fn needs_you_looks_at_the_viewer_and_blinks_at_2_hz() {
        let mut d = Director::new();
        d.on(CatEvent::Mood(Mood::NeedsYou), ms(0));
        let f = d.frame(ms(0));
        assert_eq!((f.pose.gx, f.pose.eo), (0.0, 1.0));
        let blink: Vec<bool> = [0, 249, 250, 499, 500, 749, 750, 1000]
            .iter()
            .map(|&t| d.frame(ms(t)).blink_on)
            .collect();
        assert_eq!(blink, [true, true, false, false, true, true, false, true]);
        d.on(CatEvent::Mood(Mood::Idle), ms(0));
        assert!(!d.frame(ms(0)).blink_on && !d.frame(ms(300)).blink_on);
    }

    /// The mood poses are the mockup's (the widget states and the full idle).
    #[test]
    fn mood_poses_render_to_their_goldens() {
        let progress = Ratio { num: 3, den: 7 };
        for (mood, key) in [
            (Mood::Idle, "closed"),
            (Mood::Idle, "widget-idle"),
            (Mood::Working { progress }, "widget-working"),
            (Mood::NeedsYou, "widget-needs"),
            (
                Mood::Done {
                    ok: 6,
                    partial: 1,
                    failed: 1,
                },
                "widget-done",
            ),
            (
                Mood::Done {
                    ok: 1,
                    partial: 0,
                    failed: 0,
                },
                "happy",
            ),
        ] {
            let mut d = Director::new();
            d.on(CatEvent::Mood(mood), ms(0));
            assert_keyframe(&d.frame(ms(0)), key);
        }
        let mut d = Director::new();
        d.on(CatEvent::Mood(Mood::Failed), ms(0));
        let f = d.frame(ms(0));
        assert!(f.pose.ears < 0.0, "ears down");
        let g = cat::render(&f.pose, 0.465, Theme::default_theme(), f.glow);
        assert!(g.cells.iter().any(Option::is_some));
    }

    /// generate.py's animation totals: 4.02 s for the drag, 3.91 s for the chomp.
    #[test]
    fn durations_sum_to_the_mockup_totals() {
        assert_eq!(total(&DRAG_DURS), ms(4020));
        assert_eq!(total(&CHOMP_DURS), ms(3910));
        assert!(DRAG.iter().chain(&CHOMP).all(|k| k.cut), "the mockup cuts");
    }

    #[test]
    fn file_at_is_none_once_the_file_is_eaten() {
        for stage in [Stage::Full, Stage::Widget] {
            let mut d = Director::new();
            d.set_stage(stage);
            d.on(CatEvent::Dropped { n: 3 }, ms(0));
            let docs = match stage {
                Stage::Full => CHOMP_DOC,
                Stage::Widget => WCHOMP_DOC,
            };
            let mut gone = false;
            for (i, doc) in docs.iter().enumerate() {
                let f = d.frame(start(&CHOMP_DURS, 0, i));
                gone |= doc.is_none_or(|(_, _, first)| first >= stage.doc_rows());
                assert_eq!(f.file_at.is_none(), gone, "{stage:?} step {}", i + 1);
                let crumbs = stage == Stage::Full && i == 5;
                if crumbs {
                    let (x, row, _) = doc.expect("the crumbs mark where the file was");
                    assert_eq!((x, row), CRUMBS_AT);
                    assert_eq!(CRUMBS.len(), 5);
                }
                assert_eq!(f.crumbs, crumbs, "{stage:?} step {}", i + 1);
            }
            assert!(gone);
        }
    }

    /// Two directors fed the same events give equal frames at equal times;
    /// asking one for frames in between changes nothing.
    #[test]
    fn frame_is_pure() {
        let working = Mood::Working {
            progress: Ratio { num: 1, den: 2 },
        };
        let events = [
            (CatEvent::Mood(working), 0),
            (CatEvent::DragAt(Some(CellPos { x: 90, y: 9 })), 300),
            (CatEvent::DragAt(Some(CellPos { x: 70, y: 20 })), 900),
            (CatEvent::Dropped { n: 4 }, 1700),
            (CatEvent::Mood(Mood::NeedsYou), 2000),
            (CatEvent::Dropped { n: 1 }, 6000),
        ];
        let (mut a, mut b) = (Director::new(), Director::new());
        let mut frames = 0;
        for (ev, at) in events {
            a.on(ev, ms(at));
            b.on(ev, ms(at));
            for t in (at..at + 4500).step_by(37) {
                let f = a.frame(ms(t));
                assert_eq!(f, a.frame(ms(t)));
                assert_eq!(f, b.frame(ms(t)), "t = {t} ms");
                frames += 1;
            }
            assert_eq!(a, b);
        }
        assert!(frames > 600);
    }

    #[test]
    fn every_hint_and_caption_is_in_the_string_table() {
        use crate::ui::strings::ALL;
        let mut seen = Vec::new();
        for k in DRAG.iter().chain(&CHOMP) {
            seen.extend([k.hint, k.whint, k.caption]);
        }
        for stage in [Stage::Full, Stage::Widget] {
            for mood in [
                Mood::Idle,
                Mood::Working {
                    progress: Ratio { num: 0, den: 1 },
                },
                Mood::NeedsYou,
                Mood::Done {
                    ok: 0,
                    partial: 0,
                    failed: 0,
                },
                Mood::Failed,
            ] {
                let mut d = Director::new();
                d.set_stage(stage);
                d.on(CatEvent::Mood(mood), ms(0));
                let f = d.frame(ms(0));
                seen.extend([f.hint, f.caption]);
            }
        }
        for fx in CHOMP_FX.iter().chain(&WCHOMP_FX) {
            seen.extend(
                fx.iter()
                    .map(|e| e.2)
                    .filter(|t| t.chars().any(char::is_alphabetic)),
            );
        }
        let missing: Vec<_> = seen.iter().filter(|s| !ALL.contains(s)).collect();
        assert!(missing.is_empty(), "not in strings::ALL: {missing:?}");
    }

    /// A step marked `cut: false` blends into the next over its hold.
    #[test]
    fn an_uncut_step_blends_into_the_next() {
        let mut keys = CHOMP;
        keys[1].cut = false;
        let d = Director::new();
        let mid = d.keyed(&keys, 1, 0.5, 0);
        assert!((mid.pose.mouth - 0.475).abs() < 1e-12);
        assert!((mid.glow - 0.45).abs() < 1e-12 && (mid.panel_dim - 0.375).abs() < 1e-12);
        assert_eq!(mid.pose.plate, Some(2.0), "the plate leaves on the cut");
        assert_eq!(d.keyed(&CHOMP, 1, 0.5, 0).pose, CHOMP[1].pose);
    }

    #[test]
    fn a_drop_with_no_drag_starts_at_plop() {
        let mut d = Director::new();
        d.on(CatEvent::Dropped { n: 3 }, ms(1000));
        let f = d.frame(ms(1000));
        assert_eq!(f.caption, "the drop: plop");
        assert_eq!(f.hint, "·∙· plop · {n} pdfs landed on the cat ·∙·");
        assert_eq!(f.n, 3);
    }
}
