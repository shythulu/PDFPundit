//! The cat's model geometry, ported from generate.py's `ell`, `in_tri`, `seg`,
//! `hsh`, `fluffy`, `ear_pts`, `cat_sample`, `whiskers` and `plate_px`.
//!
//! The model is a 62 × 56 unit face; `sample` answers "what colour is the cat at
//! this model point" (`None` where it draws nothing). Every operation follows the
//! Python expression it was ported from, operand for operand and in the same
//! evaluation order, because the T-00 goldens are matched on 100% of cells. All
//! arithmetic is `f64`; every `**` is `libm::pow`, and `sin`, `asin`, `atan2` and
//! `hypot` come from the `libm` crate (D-076). `sqrt`, `abs` and `floor` are exact
//! IEEE operations and stay on `f64`. Python's `int()` on a float truncates toward
//! zero, as `as i64` does.

use super::{Pose, Rgb, mix, ramp};

const fn c(v: u32) -> Rgb {
    Rgb::from_u32(v)
}

const FUR: [Rgb; 5] = [
    c(0xffffff),
    c(0xf5eff5),
    c(0xe2d8e4),
    c(0xbcaec2),
    c(0x8a7c91),
];
const LINE: Rgb = c(0x3a2d3c);
const WHITE: Rgb = c(0xffffff);

type Pt = (f64, f64);
type Tri = [Pt; 3];

const EAR_L: Tri = [(6.0, 27.0), (8.5, 1.0), (27.5, 12.5)];
const EAR_L_IN: Tri = [(10.2, 22.0), (10.4, 5.8), (22.5, 13.8)];

/// Python's `x ** y` on floats.
fn pw(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

/// Python's `min(a, b)`: `a` unless `b` is smaller.
fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Python's `max(a, b)`: `a` unless `b` is larger.
fn max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Python's `max(lo, min(hi, v))`.
pub(super) fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    max(lo, min(hi, v))
}

pub(super) fn ell(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> f64 {
    pw((x - cx) / rx, 2.0) + pw((y - cy) / ry, 2.0)
}

fn in_tri(x: f64, y: f64, a: Pt, b: Pt, c: Pt) -> bool {
    let s = |p: Pt, q: Pt, r: Pt| (p.0 - r.0) * (q.1 - r.1) - (q.0 - r.0) * (p.1 - r.1);
    let (d1, d2, d3) = (s((x, y), a, b), s((x, y), b, c), s((x, y), c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Distance from `(x, y)` to the segment `a`–`b`.
fn seg(x: f64, y: f64, a: Pt, b: Pt) -> f64 {
    let (ax, ay) = a;
    let (bx, by) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let t = max(
        0.0,
        min(1.0, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)),
    );
    libm::hypot(x - ax - t * dx, y - ay - t * dy)
}

/// The mockup's hash: integer arguments only (eng-r1-fr5).
pub(super) fn hsh(a: i64, b: i64) -> f64 {
    let v = libm::sin(a as f64 * 12.9898 + b as f64 * 78.233) * 43758.5453;
    v - v.floor()
}

/// An ellipse with a wavy edge: whether `(x, y)` is inside, and the plain
/// ellipse distance.
#[allow(clippy::too_many_arguments)]
fn fluffy(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64, amp: f64, seed: f64) -> (bool, f64) {
    let e = ell(x, y, cx, cy, rx, ry);
    if e > 1.5 {
        return (false, e);
    }
    let ang = libm::atan2((y - cy) / ry, (x - cx) / rx);
    let thr =
        1.0 + amp * libm::sin(ang * 19.0 + seed) + amp * 0.6 * libm::sin(ang * 33.0 + seed * 2.3);
    (e <= thr, e)
}

/// One ear, outer and inner triangle; side -1 is the viewer's left.
fn ear_pts(tri: &Tri, side: f64, p: &Pose) -> Tri {
    let pts = tri.map(|(x, y)| (if side > 0.0 { 62.0 - x } else { x }, y));
    let base_x = (pts[0].0 + pts[2].0) / 2.0;
    let squash = if p.yaw * side > 0.0 {
        1.0 - 0.3 * p.yaw * side
    } else {
        1.0 + 0.08 * p.yaw.abs()
    };
    let mut out = pts;
    for (i, (x, y)) in pts.into_iter().enumerate() {
        let mut x = base_x + (x - base_x) * squash + p.yaw * 8.0;
        let mut y = y + p.pitch * 6.0;
        if i == 1 {
            // the tip perks up and forward
            y -= p.ears * 2.5;
            x -= side * p.ears * 1.2;
        }
        out[i] = (x, y);
    }
    out
}

/// Both ears placed for a pose, viewer's left first: `(outer, inner)`.
pub(super) type Ears = [(Tri, Tri); 2];

pub(super) fn ears(p: &Pose) -> Ears {
    [-1.0, 1.0].map(|s| (ear_pts(&EAR_L, s, p), ear_pts(&EAR_L_IN, s, p)))
}

/// What one sample of the cat needs besides the point.
pub(super) struct Ctx<'a> {
    pub(super) pose: &'a Pose,
    pub(super) ears: Ears,
    /// The minimum line half-width in model units (generate.py's `_MINW`).
    pub(super) minw: f64,
    /// The theme background, under the whiskers where nothing else is drawn.
    pub(super) bg: Rgb,
}

/// The cat's colour at model point `(x, y)`, or `None` (generate.py `cat_sample`).
pub(super) fn sample(x: f64, y: f64, cx: &Ctx) -> Option<Rgb> {
    let p = cx.pose;
    let minw = cx.minw;
    let op = p.mouth;
    let happy = p.happy;
    let mut col: Option<Rgb> = None;
    let (hx, hy) = (31.0 + p.yaw * 2.5, 31.0 - p.pitch * 1.5);
    let (h1, e1) = fluffy(x, y, hx, hy, 24.5, 18.5, 0.035, 0.7);
    let jy = 38.5 - p.pitch * 4.0 + p.puff;
    let (jrx, jry) = (13.0 + 2.0 * p.puff, 10.5 + p.puff);
    let (h2, _) = fluffy(x, y, 15.0 + p.yaw * 3.5 - p.puff, jy, jrx, jry, 0.07, 2.1);
    let (h2b, _) = fluffy(
        62.0 - x,
        y,
        15.0 - p.yaw * 3.5 - p.puff,
        jy,
        jrx,
        jry,
        0.07,
        2.1,
    );
    let h3 = op > 0.25
        && ell(
            x,
            y,
            31.0 + p.yaw * 4.0,
            44.0 + 3.5 * op - p.pitch * 3.0,
            11.0,
            8.0 * op,
        ) <= 1.0;
    let head = h1 || h2 || h2b || h3;

    for (outer, inner) in &cx.ears {
        if !head && in_tri(x, y, outer[0], outer[1], outer[2]) {
            col = Some(ramp(
                &[c(0xcfc4d3), c(0xf3edf4), c(0xffffff)],
                (y - 2.0) / 22.0,
            ));
            if in_tri(x, y, inner[0], inner[1], inner[2]) {
                let mut k = ramp(&[c(0xf7d9e3), c(0xefb8ca), c(0xd995ad)], (y - 6.0) / 16.0);
                if hsh((x * 1.3) as i64, (y * 0.8) as i64) < 0.2 {
                    k = mix(k, WHITE, 0.6);
                }
                col = Some(k);
            }
        }
    }
    if !head {
        return whiskers(x, y, p, col, cx);
    }

    // map the screen point back onto the face (features ride a sphere)
    let (r_, ry_) = (26.0, 22.0);
    let sx = clamp((x - 31.0) / r_, -0.999, 0.999);
    let sy = clamp((y - 31.0) / ry_, -0.999, 0.999);
    let fx = 31.0 + r_ * libm::sin(libm::asin(sx) - p.yaw);
    let fy = 31.0 + ry_ * libm::sin(libm::asin(sy) + p.pitch);
    let xm = 31.0 - (fx - 31.0).abs();
    let left = fx < 31.0;

    let (ecx, ecy) = (if left { 20.0 } else { 42.0 }, 28.5);
    let d = min(e1, 2.5).sqrt();
    let mut t = 0.04 + 0.3 * pw(min(1.3, d), 3.0) + 0.06 * (y - 14.0) / 40.0;
    if y > 40.0 {
        t += 0.12 * (y - 40.0) / 15.0;
    }
    t += 0.05 * (hsh((fx * 1.4) as i64, (fy * 0.6) as i64) - 0.5); // fur streaks
    if ell(fx, fy, ecx, ecy - 1.4, 7.5, 4.8) <= 1.0 {
        t += 0.07 * (1.0 - p.eo * 0.6); // brow shadow
    }
    let m = p.meme;
    if m != 0.0
        && ell(
            fx,
            fy,
            ecx + if left { 2.6 } else { -2.6 },
            ecy - 3.4,
            3.6,
            1.5,
        ) <= 1.0
    {
        t += 0.13 * m; // furrowed, judging brow
    }
    let mut col = ramp(&FUR, t);

    // eyes: lids lift from the judging squint (eo 0) to wide (eo 1)
    let eo = p.eo;
    if happy || p.chew {
        let band = if minw != 0.0 { 0.55 } else { 0.3 };
        if (ell(fx, fy, ecx, ecy + 1.2, 5.0, 3.0) - 1.0).abs() < band && fy < ecy + 1.2 {
            col = LINE;
        }
    } else {
        let ry = 3.0 + 1.4 * eo;
        let lid0 = ecy - ((if left { 0.95 } else { 0.0 }) * m + 0.5 * (1.0 - m));
        let slope = ((if left { 0.17 } else { -0.26 }) * m
            + (if left { 0.13 } else { -0.13 }) * (1.0 - m))
            * (1.0 - eo);
        let lowlid = ecy + (if left { 2.3 } else { 1.3 }) * m + 3.2 * (1.0 - m);
        let lidline = (lid0 + slope * (fx - ecx)) * (1.0 - eo) + (ecy - 3.7) * eo;
        let de = ell(fx, fy, ecx, ecy, 5.4 - 0.1 * eo, ry);
        if de <= 1.0 && fy >= lidline && (fy <= lowlid || eo > 0.5) {
            let dark_stops = if m > 0.5 {
                [c(0x0f0a0d), c(0x2a2521), c(0x4d4f40)]
            } else {
                [c(0x1f181c), c(0x4a4b3f), c(0x77815b)]
            };
            let dark = ramp(&dark_stops, (fy - lidline) / 2.6);
            let iris = ramp(&[c(0xecebb0), c(0xbcc77c), c(0x7d8f4f)], de.sqrt());
            col = mix(dark, iris, min(1.0, eo * 1.6));
            let (px, py) = (ecx + p.gx * 1.9, ecy + 0.2 + p.gy * 1.4);
            let (prx, pry) = (1.3 + 1.5 * eo, 2.6 + 1.1 * eo);
            if eo > 0.25 && ell(fx, fy, px, py, prx, pry) <= 1.0 {
                col = c(0x110b10);
            }
            let (hlx, hly) = if eo > 0.25 {
                (ecx - 1.6, ecy - 1.8)
            } else {
                (ecx + 1.4, lidline + 0.9)
            };
            if pw(fx - hlx, 2.0) + pw(fy - hly, 2.0) < (if eo > 0.25 { 1.0 } else { 0.35 }) {
                col = if eo > 0.25 { WHITE } else { c(0xdfe2cf) };
            }
        } else if de <= 1.3 + 0.1 * eo && (fy - lidline).abs() < max(0.8 + 0.4 * eo, minw) {
            col = LINE;
        } else if de <= 1.0 && fy > lowlid && eo <= 0.5 {
            col = mix(col, c(0x8a7c91), 0.45); // puffy lower lid
        } else if 1.0 < de && de <= 1.35 && fy > ecy {
            col = mix(col, c(0x8a7c91), 0.6);
        }
    }

    // muzzle, whisker pads
    let chin_y = 46.0 + 7.6 * op;
    if ell(xm, fy, 26.2, 39.8, 6.0, 4.3) <= 1.0
        || ell(fx, fy, 31.0, chin_y, 4.8 + 0.6 * op, 2.4) <= 1.0
    {
        col = ramp(&[c(0xffffff), c(0xece4ed)], (fy - 36.0) / 9.0);
        if [(21.8, 39.8), (22.4, 41.6), (23.9, 40.6)]
            .iter()
            .any(|&(a, b)| pw(xm - a, 2.0) + pw(fy - b, 2.0) < 0.35)
        {
            col = c(0xc9bdcc);
        }
    }
    if in_tri(fx, fy, (28.4, 35.8), (33.6, 35.8), (31.0, 38.6)) {
        col = ramp(&[c(0xf6c2d1), c(0xe59ab1), c(0xc47790)], (fy - 35.8) / 2.8);
    }

    // mouth: interpolates from the resting half-open mouth to fully open
    if seg(xm, fy, (31.0, 38.6), (31.0, 40.2)) < max(0.6, minw) {
        col = LINE;
    }
    if !happy {
        let mm = m * (1.0 - op);
        let mcx = 31.0 - 1.4 * mm;
        let (mcy, mrx, mry) = (
            42.1 + 3.7 * op + 0.2 * mm,
            3.3 + 4.5 * op + 1.1 * mm,
            1.35 + 5.85 * op + 0.55 * mm,
        );
        let top = 40.7 - 0.9 * op - 0.3 * mm + 0.16 * mm * (fx - 31.0);
        let md = ell(fx, fy, mcx, mcy, mrx, mry);
        if op < 0.5 {
            for (a, b) in [((31.0, 40.2), (28.8, 41.2)), ((28.8, 41.2), (27.2, 40.7))] {
                if seg(xm, fy, a, b) < max(0.6, minw) {
                    col = LINE;
                }
            }
        }
        if fy >= top {
            if md <= 1.0 {
                col = if op > 0.2 {
                    ramp(&[c(0x1f0610), c(0x4f1024), c(0x86223f)], md.sqrt())
                } else {
                    ramp(&[c(0x2b1320), c(0x5a2338)], md)
                };
                let (ty, trx, tr_y) = (
                    42.9 + 7.3 * op + 0.45 * mm,
                    2.4 + 3.0 * op + 0.9 * mm,
                    0.85 + 2.05 * op + 0.35 * mm,
                );
                if ell(fx, fy, mcx, ty, trx, tr_y) <= 1.0 {
                    col = ramp(&[c(0xffa9c2), c(0xe06a8e)], ell(fx, fy, mcx, ty, trx, tr_y));
                    if op > 0.5 && (fx - 31.0).abs() < 0.45 && fy < ty + 1.0 {
                        col = c(0xc14a72);
                    }
                }
            } else if md <= 1.2 && op > 0.2 {
                col = LINE;
            } else if (ell(fx, fy, mcx, mcy, mrx + 0.4, mry + 0.45) - 1.0).abs() < 0.28
                && fy > mcy
                && op <= 0.2
            {
                col = c(0xb3a3b8);
            }
        }
        if op > 0.35 {
            let f = (op - 0.35) / 0.65;
            if in_tri(
                xm,
                fy,
                (31.0 - mrx * 0.78, top),
                (31.0 - mrx * 0.45, top),
                (31.0 - mrx * 0.6, top + 4.4 * f),
            ) {
                col = c(0xfdfafc);
            }
            let tooth = mcy + mry * 0.95;
            if op > 0.7
                && md <= 1.25
                && in_tri(
                    xm,
                    fy,
                    (26.2, tooth),
                    (27.9, tooth),
                    (27.2, tooth - 2.6 * f),
                )
            {
                col = c(0xf3eef4);
            }
        }
    }
    whiskers(x, y, p, Some(col), cx)
}

fn whiskers(x: f64, y: f64, p: &Pose, mut col: Option<Rgb>, cx: &Ctx) -> Option<Rgb> {
    for fade in whisker_hits(x, y, p, cx.minw).into_iter().flatten() {
        col = Some(mix(col.unwrap_or(cx.bg), c(0xfbf7fb), 0.8 * fade));
    }
    col
}

/// For each of the three whiskers (top to bottom), its fade at model point
/// `(x, y)` if the point is on it.
pub(super) fn whisker_hits(x: f64, y: f64, p: &Pose, minw: f64) -> [Option<f64>; 3] {
    let op = p.mouth;
    let wx = x - 26.0 * libm::sin(p.yaw) * 0.85;
    let wy = y + 22.0 * libm::sin(p.pitch) * 0.85;
    let xm = 31.0 - (wx - 31.0).abs();
    let lift = -1.6 * op;
    [
        ((22.5, 40.0), (1.0, 36.5 + lift)),
        ((22.5, 41.5), (0.5, 42.2 + lift)),
        ((23.0, 43.0), (2.5, 47.6 + lift)),
    ]
    .map(|(a, b)| {
        (seg(xm, wy, a, b) < max(0.4, minw * 0.6))
            .then(|| 1.0 - max(0.0, (a.0 - xm) / (a.0 - b.0)) * 0.5)
    })
}

/// The dinner plate's colour at plate-pixel point `(x, y)` (generate.py `plate_px`).
pub(super) fn plate_px(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64, w: f64) -> Option<Rgb> {
    let e = ell(x, y, cx, cy, rx, ry);
    if e > 1.0 {
        return None;
    }
    if e > 0.86 && y < cy {
        return Some(c(0x1a1117)); // far edge in shadow, as in the photo
    }
    if e > 0.74 && y < cy {
        return Some(WHITE); // lit rim
    }
    let ie = ell(x, y, cx, cy + 0.8, rx * 0.8, ry * 0.72);
    if (ie - 1.0).abs() < 0.07 {
        return Some(c(0xc9c0cf)); // inner rim
    }
    if ie < 1.0 {
        if y < cy - 3.2
            && w * 0.14 < x
            && x < w * 0.36
            && hsh((x * 1.3) as i64, (y * 1.7) as i64) < 0.6
        {
            const FOOD: [Rgb; 5] = [
                c(0x6d9b4a),
                c(0xa3c96a),
                c(0x4f7a36),
                c(0xc0392b),
                c(0xe8d27a),
            ];
            return Some(FOOD[(hsh((x * 2.0) as i64, y as i64) * 5.0) as usize]);
        }
        return Some(ramp(&[c(0xd9d5e2), c(0xeeebf3)], (y - cy + 6.0) / 6.0)); // the well
    }
    Some(ramp(&[c(0xf1eef6), c(0xc9c3d3)], (x - cx).abs() / rx))
}
