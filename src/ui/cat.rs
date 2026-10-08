//! Cat renderer (T-18): the white Smudge cat, drawn from a pose as half-block
//! cells, ported from generate.py's `pose`, `cat_grid` and `plate_grid` so every
//! frame matches the T-00 goldens cell for cell (D-055).
//!
//! The interface is a pose in, a cell grid out. Behind it: the model geometry
//! (`geometry`), the sphere warp that slides features round the head, 2 × 2
//! supersampling averaged in OKLab, half-block packing, the widget rule (below
//! scale 0.5 every line is at least `0.5 / scale` model units wide, so lids,
//! mouth and whiskers keep a pixel), the glow ring, and a small LRU cache.
//!
//! All geometry and colour math is `f64`, mirroring the Python oracle (eng-r2-q8);
//! transcendentals go through the `libm` crate (D-018, D-076).
//!
//! `examples/cat_dump.rs` includes this file, `color.rs` and `theme.rs` by path,
//! so they reach each other through `super::` only.
#![cfg_attr(not(test), allow(dead_code))]

#[path = "cat/geometry.rs"]
mod geometry;

use std::sync::Mutex;

use super::color::{Rgb, avg, mix, ramp};
use super::theme::Theme;

/// What the face is doing. `yaw` turns the head (features slide round a
/// sphere), `pitch` tips it up, `eo` opens the eyes (0 = the judging squint,
/// 1 = wide), `gx`/`gy` aim the pupils (-1..1), `ears` perks the ears, `mouth`
/// opens the jaw (0 = the resting half-open mouth, 1 = ready to eat), `happy`
/// closes the eyes in contented arcs, `meme` blends in the meme's judging brow
/// and lids, `plate` is the dinner plate's slide (`None` once it is gone; the
/// cat itself ignores it, see [`plate`]), `chew` shuts the eyes happily but
/// keeps the mouth, and `puff` fills out the cheeks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub yaw: f64,
    pub pitch: f64,
    pub eo: f64,
    pub mouth: f64,
    pub gx: f64,
    pub gy: f64,
    pub ears: f64,
    pub happy: bool,
    pub meme: f64,
    pub plate: Option<f64>,
    pub chew: bool,
    pub puff: f64,
}

/// generate.py's `pose()`: the meme, eyes narrowed, behind its plate.
impl Default for Pose {
    fn default() -> Pose {
        Pose {
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
        }
    }
}

/// One terminal cell holding two vertically stacked pixels.
///
/// - both pixels: `'▀'` with `fg` = top and `bg = Some(bottom)`, or `'█'` in
///   `fg` (and `bg = Some(fg)`) when the two are the same colour;
/// - top pixel only: `'▀'` in `fg`, `bg = None` (whatever is underneath shows);
/// - bottom pixel only: `'▄'` in `fg`, `bg = None`.
///
/// A cell with neither pixel is `None` in [`CellGrid::cells`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub ch: char,
}

impl Cell {
    fn from_halves(top: Option<Rgb>, bottom: Option<Rgb>) -> Option<Cell> {
        match (top, bottom) {
            (None, None) => None,
            (Some(t), None) => Some(Cell {
                fg: t,
                bg: None,
                ch: '▀',
            }),
            (None, Some(b)) => Some(Cell {
                fg: b,
                bg: None,
                ch: '▄',
            }),
            (Some(t), Some(b)) => Some(Cell {
                fg: t,
                bg: Some(b),
                ch: if t == b { '█' } else { '▀' },
            }),
        }
    }

    /// The `(top, bottom)` pixels; `None` is transparent.
    pub fn halves(&self) -> (Option<Rgb>, Option<Rgb>) {
        match self.ch {
            '▄' => (None, Some(self.fg)),
            _ => (Some(self.fg), self.bg),
        }
    }
}

/// A block of half-block cells, row-major, `cols * rows` entries; `None` is a
/// cell with nothing drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellGrid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Option<Cell>>,
}

impl CellGrid {
    /// The cell at `(x, y)`; `None` outside the grid too.
    pub fn get(&self, x: u16, y: u16) -> Option<Cell> {
        if x >= self.cols || y >= self.rows {
            return None;
        }
        self.cells[usize::from(y) * usize::from(self.cols) + usize::from(x)]
    }

    /// Packs a `w × h` pixel grid (h even) two pixel rows to a cell.
    fn from_pixels(w: usize, h: usize, px: &[Option<Rgb>]) -> CellGrid {
        let rows = h / 2;
        let mut cells = Vec::with_capacity(w * rows);
        for cy in 0..rows {
            for x in 0..w {
                cells.push(Cell::from_halves(
                    px[2 * cy * w + x],
                    px[(2 * cy + 1) * w + x],
                ));
            }
        }
        CellGrid {
            cols: to_u16(w),
            rows: to_u16(rows),
            cells,
        }
    }
}

fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// The supersampling offsets inside one pixel, in generate.py's order.
const SUB: [(f64, f64); 4] = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];

/// The cat for `pose` at `scale` (model units to pixels; the mockup draws at
/// 0.93 full size, 0.68 in the batch screen, 0.52 on the result screen and 0.465
/// in the widget), with the glow ring at strength `glow` (0 = none) in the
/// theme's `needs_input` colour over its background. The grid is
/// `round(62·scale)` cells wide and `round(28·scale)` cells tall. Pure: the
/// same arguments always give the same grid.
pub fn render(pose: &Pose, scale: f64, theme: &Theme, glow: f64) -> CellGrid {
    let key = Key::new(pose, scale, theme, glow);
    if let Some(hit) = cache::get(&key) {
        return hit;
    }
    let grid = render_uncached(pose, scale, theme.roles.bg, theme.roles.needs_input, glow);
    cache::put(key, grid.clone());
    grid
}

/// generate.py's `cat_grid`, packed into cells.
fn render_uncached(pose: &Pose, scale: f64, bg: Rgb, accent: Rgb, glow: f64) -> CellGrid {
    let ctx = geometry::Ctx {
        pose,
        ears: geometry::ears(pose),
        // keep 1-px lines on the widget-size cat
        minw: if scale < 0.5 { 0.5 / scale } else { 0.0 },
        bg,
    };
    // Both are small and non-negative, so the casts are exact.
    let wd = (62.0 * scale).round_ties_even() as usize;
    let hd = (56.0 * scale / 2.0).round_ties_even() as usize * 2;
    let mut px = vec![None; wd * hd];
    for py in 0..hd {
        for pxi in 0..wd {
            let (fx, fy) = (pxi as f64, py as f64);
            let samples: Vec<Rgb> = SUB
                .iter()
                .filter_map(|&(sx, sy)| {
                    geometry::sample((fx + sx) / scale, (fy + sy) / scale, &ctx)
                })
                .collect();
            let out = &mut px[py * wd + pxi];
            if samples.len() >= 2 {
                *out = Some(avg(&samples));
            } else if glow != 0.0 {
                *out = glow_px(fx, fy, scale, bg, accent, glow);
            }
        }
    }
    CellGrid::from_pixels(wd, hd, &px)
}

/// The ring round the cat while a file is over it: dashes on the rim, a faint
/// wash inside.
fn glow_px(px: f64, py: f64, scale: f64, bg: Rgb, accent: Rgb, glow: f64) -> Option<Rgb> {
    let (x, y) = ((px + 0.5) / scale, (py + 0.5) / scale);
    let dg = geometry::ell(x, y, 31.0, 28.8, 30.6, 27.6);
    if dg > 1.0 {
        return None;
    }
    let r = dg.sqrt();
    let ang = libm::atan2((y - 28.8) / 27.6, (x - 31.0) / 30.6);
    let pi = std::f64::consts::PI;
    // ang + pi is in [0, 2pi], so the cast truncates as Python's int() does.
    let dash = ((ang + pi) / (2.0 * pi) * 44.0) as i64 % 2 == 0;
    if (1.0 - r) * 27.6 * scale < 0.9 && dash {
        Some(mix(bg, accent, glow))
    } else {
        Some(mix(bg, accent, 0.2 * glow * libm::pow(r, 8.0)))
    }
}

/// The meme's dinner plate, `width` pixels (= cells) wide, slid down by `off`
/// (the pose's `plate`; `None` gives an empty grid). The mockup draws two: the
/// full layout's, 61 wide and 10 pixels tall, slid two pixels per unit; and the
/// widget's, 30 wide and 6 tall, slid one pixel per unit. A width below 61 gets
/// the widget's proportions.
pub fn plate(width: u16, off: Option<f64>) -> CellGrid {
    let Some(off) = off else {
        return CellGrid {
            cols: width,
            rows: 0,
            cells: Vec::new(),
        };
    };
    let (h, slide) = if width >= PLATE_FULL_W {
        (10, off * 2.0)
    } else {
        (6, off)
    };
    let w = usize::from(width);
    let wf = f64::from(width);
    let (cx, cy, rx, ry) = (wf / 2.0, 9.5 + slide, wf / 2.0 + 1.0, 9.0);
    let mut px = vec![None; w * h];
    for y in 0..h {
        for x in 0..w {
            let samples: Vec<Rgb> = SUB
                .iter()
                .filter_map(|&(a, b)| {
                    geometry::plate_px(x as f64 + a, y as f64 + b, cx, cy, rx, ry, wf)
                })
                .collect();
            if samples.len() >= 2 {
                px[y * w + x] = Some(avg(&samples));
            }
        }
    }
    CellGrid::from_pixels(w, h, &px)
}

/// The full layout's plate width.
const PLATE_FULL_W: u16 = 61;

/// Aims the pupils at `target_px`, a point in the cat's model space: the 62 × 56
/// unit face, i.e. the target's pixel offset from the cat grid's top-left
/// corner divided by the render scale. generate.py aims at the middle of the
/// dragged file: for a file whose top-left cell is `(dx, dy)`, `w × h` pixels,
/// over a cat drawn at cell `(cx, cy)`, the target is
/// `((dx + w / 2 - cx) / scale, ((dy - cy) * 2 + h / 2) / scale)`.
pub fn gaze_to(pose: &mut Pose, target_px: (f64, f64)) {
    let (px, py) = target_px;
    pose.gx = geometry::clamp((px - 31.0) / 22.0, -1.0, 1.0);
    pose.gy = geometry::clamp((py - 28.0) / 18.0, -1.0, 1.0);
}

/// The cache key: every input `render` reads, bit for bit. A coarser
/// quantisation would hand one pose another's grid and break the 100%-cells
/// gate on computed gazes, so poses are keyed exactly. `plate` is not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    pose: [u64; 9],
    flags: (bool, bool),
    scale: u64,
    glow: u64,
    bg: Rgb,
    accent: Rgb,
}

impl Key {
    fn new(p: &Pose, scale: f64, theme: &Theme, glow: f64) -> Key {
        Key {
            pose: [
                p.yaw, p.pitch, p.eo, p.mouth, p.gx, p.gy, p.ears, p.meme, p.puff,
            ]
            .map(f64::to_bits),
            flags: (p.happy, p.chew),
            scale: scale.to_bits(),
            glow: glow.to_bits(),
            bg: theme.roles.bg,
            accent: theme.roles.needs_input,
        }
    }
}

/// A most-recently-used list of rendered grids. Lookups scan it in order; it
/// never iterates a hash map.
mod cache {
    use super::{CellGrid, Key, Mutex};

    pub(super) const CAPACITY: usize = 32;

    static GRIDS: Mutex<Vec<(Key, CellGrid)>> = Mutex::new(Vec::new());

    fn lock() -> std::sync::MutexGuard<'static, Vec<(Key, CellGrid)>> {
        // A grid is written whole or not at all, so a poisoned list is intact.
        GRIDS.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn get(key: &Key) -> Option<CellGrid> {
        let mut grids = lock();
        let i = grids.iter().position(|(k, _)| k == key)?;
        let entry = grids.remove(i);
        let grid = entry.1.clone();
        grids.insert(0, entry);
        Some(grid)
    }

    pub(super) fn put(key: Key, grid: CellGrid) {
        let mut grids = lock();
        grids.retain(|(k, _)| *k != key);
        grids.insert(0, (key, grid));
        grids.truncate(CAPACITY);
    }

    #[cfg(test)]
    pub(super) fn len() -> usize {
        lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::goldens::{self, Golden, GoldenCell, GoldenPose, Kind};

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

    fn golden_rgb(c: Rgb) -> goldens::Rgb {
        goldens::Rgb(c.0)
    }

    fn as_golden(cell: Option<Cell>) -> GoldenCell {
        match cell {
            None => GoldenCell::Empty,
            Some(c) => {
                let (top, bottom) = c.halves();
                GoldenCell::Half {
                    top: top.map(golden_rgb),
                    bottom: bottom.map(golden_rgb),
                }
            }
        }
    }

    fn mismatch_report(g: &Golden, grid: &CellGrid) -> Option<String> {
        let bad = g.mismatches(grid.cols, grid.rows, |x, y| as_golden(grid.get(x, y)));
        if bad.is_empty() {
            return None;
        }
        let mut msg = if (grid.cols, grid.rows) != (g.w, g.h) {
            format!(
                "{}: size {}x{}, golden {}x{}",
                g.name, grid.cols, grid.rows, g.w, g.h
            )
        } else {
            format!(
                "{}: {} of {} cells differ",
                g.name,
                bad.len(),
                g.cells.len()
            )
        };
        for (x, y, want, have) in bad.iter().take(12) {
            msg.push_str(&format!("\n  ({x}, {y}) expected {want}, got {have}"));
        }
        Some(msg)
    }

    fn cat_goldens() -> Vec<Golden> {
        goldens::index()
            .frames
            .iter()
            .filter(|f| f.kind == Kind::Cat)
            .map(|f| goldens::load(&f.name))
            .collect()
    }

    /// Every cat golden (every pose and keyframe at all four scales) is drawn
    /// cell-exact, characters and colours; failures list `(x, y, expected, got)`.
    #[test]
    fn render_matches_every_cat_golden() {
        let theme = Theme::default_theme();
        let all = cat_goldens();
        assert_eq!(all.len(), 4 * goldens::pose_keys().len());
        let mut failures = Vec::new();
        for g in &all {
            let (Some(scale), Some(glow), Some(p)) = (g.scale, g.glow, g.pose) else {
                panic!("{}: cat golden without scale, glow or pose", g.name);
            };
            let grid = render(&pose_of(&p), scale, theme, glow);
            if let Some(m) = mismatch_report(g, &grid) {
                failures.push(m);
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} cat goldens differ:\n{}",
            failures.len(),
            all.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn grid_sizes_follow_the_scale() {
        let theme = Theme::default_theme();
        for (scale, cols, rows) in [
            (0.93, 58, 26),
            (0.68, 42, 19),
            (0.52, 32, 15),
            (0.465, 29, 13),
        ] {
            let g = render(&Pose::default(), scale, theme, 0.0);
            assert_eq!((g.cols, g.rows), (cols, rows), "scale {scale}");
            assert_eq!(g.cells.len(), usize::from(cols) * usize::from(rows));
        }
    }

    /// generate.py's `pose()` and the three named poses' defaults.
    #[test]
    fn default_pose_is_the_meme() {
        let g = goldens::load("cat-0.93-closed").pose.expect("pose");
        assert_eq!(pose_of(&g), Pose::default());
    }

    /// `render` is pure: equal arguments give equal grids, cached or not, and
    /// whatever was rendered in between (including more than the cache holds).
    #[test]
    fn render_is_pure() {
        let theme = Theme::default_theme();
        let p = Pose {
            eo: 0.7,
            yaw: 0.25,
            gx: 0.3,
            ..Pose::default()
        };
        let first = render(&p, 0.465, theme, 0.6);
        assert_eq!(first, render(&p, 0.465, theme, 0.6));
        let accent = theme.roles.needs_input;
        assert_eq!(
            first,
            render_uncached(&p, 0.465, theme.roles.bg, accent, 0.6)
        );
        for i in 0..=cache::CAPACITY {
            let other = Pose {
                yaw: i as f64 / 100.0,
                ..p
            };
            render(&other, 0.465, theme, 0.0);
        }
        assert!(cache::len() <= cache::CAPACITY);
        assert_eq!(first, render(&p, 0.465, theme, 0.6));
        // The plate is not part of the cat.
        assert_eq!(first, render(&Pose { plate: None, ..p }, 0.465, theme, 0.6));
        // Another theme's colours reach the glow ring.
        let acid = Theme::all()
            .iter()
            .find(|t| t.roles.needs_input != accent)
            .expect("a theme with another accent");
        assert_ne!(first, render(&p, 0.465, acid, 0.6));
    }

    fn lightness(c: Rgb) -> f64 {
        crate::ui::color::lab(c)[0]
    }

    /// The OKLab lightness of the darker pixel of the cell holding model point
    /// `(x, y)`, or `None` if nothing is drawn there.
    fn darkest_at(g: &CellGrid, scale: f64, (x, y): (f64, f64)) -> Option<f64> {
        let cell = g.get((x * scale) as u16, (y * scale / 2.0) as u16)?;
        let (t, b) = cell.halves();
        [t, b].into_iter().flatten().map(lightness).reduce(f64::min)
    }

    /// The widget-size cat keeps its lids, mouth and whiskers: each still puts
    /// at least one pixel on screen. Lids and mouth are `LINE` (#3a2d3c, OKLab L
    /// about 0.3) on white fur (L above 0.75), so the cell over each is dark;
    /// a whisker is light on light fur, so it is found by its own hit test.
    #[test]
    fn widget_cat_keeps_lids_mouth_and_whiskers() {
        let theme = Theme::default_theme();
        let scale = 0.465;
        let closed = render(&Pose::default(), scale, theme, 0.0);
        // The meme's lid line over each eye, and the philtrum line under the nose.
        for (what, at) in [
            ("left lid", (20.0, 27.5)),
            ("right lid", (42.0, 28.5)),
            ("mouth", (31.0, 39.4)),
        ] {
            let l = darkest_at(&closed, scale, at).unwrap_or(1.0);
            assert!(l < 0.6, "{what}: darkest pixel L = {l}");
        }
        // The happy arcs (chew, done, working) are lines too.
        let happy = render(
            &Pose {
                happy: true,
                meme: 0.0,
                plate: None,
                ..Pose::default()
            },
            scale,
            theme,
            0.0,
        );
        for (what, at) in [("left arc", (20.0, 26.7)), ("right arc", (42.0, 26.7))] {
            let l = darkest_at(&happy, scale, at).unwrap_or(1.0);
            assert!(l < 0.6, "{what}: darkest pixel L = {l}");
        }
        // Each whisker, on each side, covers at least one drawn pixel: two or
        // more of the pixel's four samples lie on it.
        let p = Pose::default();
        let minw = 0.5 / scale;
        for w in 0..3 {
            for side in [0..15u16, 15..29] {
                let shown = side.clone().any(|px| {
                    (0..26u16).any(|py| {
                        let on = SUB
                            .iter()
                            .filter(|&&(sx, sy)| {
                                let (x, y) =
                                    ((f64::from(px) + sx) / scale, (f64::from(py) + sy) / scale);
                                geometry::whisker_hits(x, y, &p, minw)[w].is_some()
                            })
                            .count();
                        let (top, bottom) =
                            closed.get(px, py / 2).map_or((None, None), |c| c.halves());
                        let drawn = if py % 2 == 0 { top } else { bottom };
                        on >= 2 && drawn.is_some()
                    })
                });
                assert!(shown, "whisker {w} has no pixel in columns {side:?}");
            }
        }
    }

    /// The `_MINW` rule is what keeps them: without it the widget cat loses
    /// lines (the guard above would have nothing to guard).
    #[test]
    fn min_line_width_applies_below_half_scale() {
        let p = Pose::default();
        let bg = Theme::default_theme().roles.bg;
        let ctx = |minw| geometry::Ctx {
            pose: &p,
            ears: geometry::ears(&p),
            minw,
            bg,
        };
        let (with, without) = (ctx(0.5 / 0.465), ctx(0.0));
        let scale = 0.465;
        let mut differ = 0;
        for py in 0..26 {
            for px in 0..29 {
                for (sx, sy) in SUB {
                    let (x, y) = ((px as f64 + sx) / scale, (py as f64 + sy) / scale);
                    if geometry::sample(x, y, &with) != geometry::sample(x, y, &without) {
                        differ += 1;
                    }
                }
            }
        }
        assert!(differ > 0, "the minimum line width changed no sample");
        let full = render(&p, 0.52, Theme::default_theme(), 0.0);
        assert_eq!(
            full,
            render_uncached(&p, 0.52, bg, Theme::default_theme().roles.needs_input, 0.0)
        );
    }

    /// `gaze_to` reproduces the gaze of every keyframe that aims at the file,
    /// bit for bit, from generate.py's file positions (DRAG, CHOMP_DOC,
    /// WDRAG_DOC, WCHOMP_DOC) and cat placements.
    #[test]
    fn gaze_to_matches_the_keyframes() {
        const DRAG_DOC: [(f64, f64); 8] = [
            (100.0, 7.0),
            (94.0, 10.0),
            (88.0, 13.0),
            (82.0, 16.0),
            (77.0, 19.0),
            (72.0, 21.0),
            (68.0, 23.0),
            (64.0, 24.0),
        ];
        const WDRAG_DOC: [(f64, f64); 8] = [
            (25.0, 0.0),
            (25.0, 1.0),
            (25.0, 2.0),
            (25.0, 3.0),
            (25.0, 5.0),
            (24.0, 7.0),
            (21.0, 8.0),
            (18.0, 9.0),
        ];
        // (x cell, top pixel row); None once the file is eaten.
        const CHOMP_DOC: [Option<(f64, f64)>; 8] = [
            Some((50.0, 18.0)),
            Some((50.0, 18.0)),
            Some((50.0, 34.0)),
            Some((50.0, 54.0)),
            Some((50.0, 54.0)),
            Some((50.0, 54.0)),
            None,
            None,
        ];
        const WCHOMP_DOC: [Option<(f64, f64)>; 8] = [
            Some((12.0, 3.0)),
            Some((12.0, 3.0)),
            Some((12.0, 11.0)),
            Some((12.0, 19.0)),
            Some((12.0, 19.0)),
            None,
            None,
            None,
        ];
        // generate.py's gaze_to arguments: the cat's cell and scale, the file's
        // size in pixels.
        let full = |(dx, dy): (f64, f64)| target((dx, dy), (27.0, 8.0), 0.93, (12.0, 16.0));
        let widget = |(dx, dy): (f64, f64)| target((dx, dy), (1.0, 0.0), 0.93 / 2.0, (7.0, 8.0));
        fn target(d: (f64, f64), cat: (f64, f64), scale: f64, size: (f64, f64)) -> (f64, f64) {
            (
                (d.0 + size.0 / 2.0 - cat.0) / scale,
                ((d.1 - cat.1) * 2.0 + size.1 / 2.0) / scale,
            )
        }
        let mut cases = Vec::new();
        for i in 0..8 {
            cases.push((format!("drag-{}", i + 1), Some(full(DRAG_DOC[i]))));
            cases.push((format!("wdrag-{}", i + 1), Some(widget(WDRAG_DOC[i]))));
            // The chomp aims at (x, top pixel row / 2) as a cell position.
            let chomp = CHOMP_DOC[i].map(|(x, py)| full((x, py / 2.0)));
            cases.push((format!("chomp-{}", i + 1), chomp));
            let wchomp = WCHOMP_DOC[i].map(|(x, py)| widget((x, py / 2.0)));
            cases.push((format!("wchomp-{}", i + 1), wchomp));
        }
        let mut aimed = 0;
        for (key, t) in cases {
            let g = goldens::load(&goldens::cat_name(0.93, &key));
            let gp = g.pose.expect("pose");
            let base = pose_of(&gp);
            // generate.py aims only open eyes, and only while the file is there.
            let Some(t) = t.filter(|_| base.eo > 0.0) else {
                assert_eq!((gp.gx, gp.gy), (0.0, 0.0), "{key}: unaimed");
                continue;
            };
            let mut p = Pose {
                gx: 0.0,
                gy: 0.0,
                ..base
            };
            gaze_to(&mut p, t);
            assert_eq!(p.gx.to_bits(), gp.gx.to_bits(), "{key}: gx");
            assert_eq!(p.gy.to_bits(), gp.gy.to_bits(), "{key}: gy");
            assert_eq!(
                Pose {
                    gx: 0.0,
                    gy: 0.0,
                    ..p
                },
                Pose {
                    gx: 0.0,
                    gy: 0.0,
                    ..base
                }
            );
            aimed += 1;
        }
        assert!(aimed >= 20, "only {aimed} keyframes aimed");
        // Far targets clamp to the edge of the eye.
        let mut p = Pose::default();
        gaze_to(&mut p, (1e9, -1e9));
        assert_eq!((p.gx, p.gy), (1.0, -1.0));
    }

    /// The plate composited as generate.py does (`Canvas.pix` at (25, 30) on the
    /// full screen, rows from 35 cleared; (1, 11) in the widget) reproduces the
    /// plate cells of every canvas golden that shows one.
    #[test]
    fn plate_matches_the_canvas_goldens() {
        let full = [
            ("01-idle", "closed"),
            ("02-drag-1-file-enters", "drag-1"),
            ("02-drag-2-ears-up", "drag-2"),
            ("02-drag-3-turns", "drag-3"),
            ("02-drag-4-looks-up", "drag-4"),
            ("02-drag-5-tracks-it", "drag-5"),
            ("02b-chomp-1-plop", "chomp-1"),
            ("02b-chomp-2-looks-up", "chomp-2"),
        ];
        let widget = [
            ("07-widget-idle", "widget-idle"),
            ("08-widget-drop-1-file-enters", "wdrag-1"),
            ("08-widget-drop-2-ears-up", "wdrag-2"),
            ("08-widget-drop-3-turns", "wdrag-3"),
            ("08-widget-drop-4-looks-up", "wdrag-4"),
            ("08-widget-drop-5-tracks-it", "wdrag-5"),
            ("08b-widget-chomp-1-plop", "wchomp-1"),
            ("08b-widget-chomp-2-looks-up", "wchomp-2"),
        ];
        let mut checked = 0;
        for (frames, scale, width, at, clear_from) in [
            (&full, 0.93, 61, (25, 30), 35),
            (&widget, 0.465, 30, (1, 11), u16::MAX),
        ] {
            for (canvas, key) in frames {
                let off = goldens::load(&goldens::cat_name(scale, key))
                    .pose
                    .expect("pose")
                    .plate;
                assert!(off.is_some(), "{canvas}: has a plate");
                let g = goldens::load(canvas);
                let grid = plate(width, off);
                for y in 0..grid.rows {
                    for x in 0..grid.cols {
                        let (cx, cy) = (at.0 + x, at.1 + y);
                        let Some(cell) = grid.get(x, y) else { continue };
                        if cy >= clear_from || cx >= g.w {
                            continue;
                        }
                        let (top, bottom) = cell.halves();
                        let GoldenCell::Half {
                            top: gt,
                            bottom: gb,
                        } = g.cell(cx, cy)
                        else {
                            panic!("{canvas}: ({cx}, {cy}) is not a pixel cell");
                        };
                        if let Some(t) = top {
                            assert_eq!(gt, Some(golden_rgb(t)), "{canvas}: ({cx}, {cy}) top");
                        }
                        if let Some(b) = bottom {
                            assert_eq!(gb, Some(golden_rgb(b)), "{canvas}: ({cx}, {cy}) bottom");
                        }
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 500, "only {checked} plate cells checked");
    }

    #[test]
    fn plate_sizes() {
        let g = plate(61, Some(0.0));
        assert_eq!((g.cols, g.rows, g.cells.len()), (61, 5, 305));
        let w = plate(30, Some(0.0));
        assert_eq!((w.cols, w.rows), (30, 3));
        let gone = plate(61, None);
        assert_eq!((gone.rows, gone.cells.len()), (0, 0));
        // Sliding it far enough takes it off its grid.
        assert!(plate(61, Some(20.0)).cells.iter().all(Option::is_none));
    }

    #[test]
    fn cell_packing_round_trips() {
        let (a, b) = (Rgb::from_u32(0x102030), Rgb::from_u32(0x405060));
        for (t, bt) in [
            (Some(a), Some(b)),
            (Some(a), Some(a)),
            (Some(a), None),
            (None, Some(b)),
        ] {
            let cell = Cell::from_halves(t, bt).expect("cell");
            assert_eq!(cell.halves(), (t, bt));
            assert!(matches!(cell.ch, '▀' | '▄' | '█' | ' '));
        }
        assert_eq!(Cell::from_halves(Some(a), Some(a)).map(|c| c.ch), Some('█'));
        assert_eq!(Cell::from_halves(None, None), None);
    }
}
