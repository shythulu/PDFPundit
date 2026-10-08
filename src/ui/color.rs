//! Colour math (T-17, D-018): sRGB to OKLab and back, `mix`, `ramp` and `avg`,
//! ported from the mockup generator (generate.py lines 34-92) so gradients match
//! the T-00 goldens cell for cell.
//!
//! Everything is `f64`, as in the Python oracle (eng-r2-q8). Every
//! transcendental goes through the `libm` crate, never platform libm, so the
//! results are the same on every OS (D-076). Where generate.py writes `x ** y`
//! this calls `libm::pow`, including the cube root, which Python spells
//! `abs(v) ** (1 / 3)`: `libm::cbrt` would follow it less closely. Python's
//! `round(x)` is half-to-even, so `unlab` uses `round_ties_even`.
#![cfg_attr(not(test), allow(dead_code))]

use std::fmt;

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    /// `0xrrggbb`.
    pub const fn from_u32(v: u32) -> Rgb {
        Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8])
    }

    /// Parses `#rrggbb` in either case, as generate.py's `hx` does.
    pub fn parse(s: &str) -> Option<Rgb> {
        let hex = s.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(hex, 16).ok().map(Rgb::from_u32)
    }
}

/// Lowercase `#rrggbb`, the goldens' spelling.
impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

/// What the terminal can show: 24-bit colour, the xterm 256-colour palette, or
/// only the 16 ANSI colours. `Theme::downgrade` maps a theme onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorCaps {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Python's `max(0.0, min(1.0, x))`, NaN and signed zero included (`min`
/// returns its first argument unless the second is smaller).
fn clamp01(x: f64) -> f64 {
    let x = if x < 1.0 { x } else { 1.0 };
    if x > 0.0 { x } else { 0.0 }
}

fn s2l(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        libm::pow((c + 0.055) / 1.055, 2.4)
    }
}

fn l2s(c: f64) -> f64 {
    let c = clamp01(c);
    255.0
        * if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * libm::pow(c, 1.0 / 2.4) - 0.055
        }
}

/// sRGB to OKLab `[L, a, b]`.
pub fn lab(c: Rgb) -> [f64; 3] {
    let [r, g, b] = c.0.map(s2l);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let [l, m, s] = [l, m, s].map(|v| libm::pow(v.abs(), 1.0 / 3.0).copysign(v));
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

/// OKLab to sRGB, each channel clamped and rounded half-to-even.
pub fn unlab([ll, a, b]: [f64; 3]) -> Rgb {
    let l = libm::pow(ll + 0.3963377774 * a + 0.2158037573 * b, 3.0);
    let m = libm::pow(ll - 0.1055613458 * a - 0.0638541728 * b, 3.0);
    let s = libm::pow(ll - 0.0894841775 * a - 1.2914855480 * b, 3.0);
    let rgb = [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ];
    // l2s is within [0, 255], so the cast neither wraps nor saturates.
    Rgb(rgb.map(|v| l2s(v).round_ties_even() as u8))
}

/// Python's `round(t, 3)`: the decimal nearest the exact binary value, ties to
/// even, read back as the nearest `f64`. Rust's fixed-precision formatting and
/// float parsing are both correctly rounded, ties to even.
fn round3(t: f64) -> f64 {
    format!("{t:.3}").parse().unwrap_or(t)
}

/// `a` mixed toward `b` by `t` in OKLab; `t` is clamped to [0, 1] and rounded to
/// three decimals first, as the mockup does.
pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = round3(clamp01(t));
    let (a, b) = (lab(a), lab(b));
    unlab([0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t))
}

/// The colour at `t` (clamped to [0, 1]) along evenly spaced `stops`. One stop
/// gives that stop as the mockup computes it; no stops give black.
pub fn ramp(stops: &[Rgb], t: f64) -> Rgb {
    let t = clamp01(t);
    match stops {
        [] => Rgb([0, 0, 0]),
        // Python indexes stops[-1] and stops[0] here: the same colour, t = 1.
        [only] => mix(*only, *only, 1.0),
        _ => {
            let n = stops.len() - 1;
            // t * n >= 0, so the cast truncates as Python's int() does.
            let i = ((t * n as f64) as usize).min(n - 1);
            mix(stops[i], stops[i + 1], t * n as f64 - i as f64)
        }
    }
}

/// The OKLab mean of `cols` (summed in order, as Python's `sum`); black if empty.
pub fn avg(cols: &[Rgb]) -> Rgb {
    if cols.is_empty() {
        return Rgb([0, 0, 0]);
    }
    let mut sum = [0.0; 3];
    for c in cols {
        let l = lab(*c);
        for i in 0..3 {
            sum[i] += l[i];
        }
    }
    let n = cols.len() as f64;
    unlab(sum.map(|v| v / n))
}

/// Squared OKLab distance.
fn dist2(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// A fixed palette to snap colours onto: the nearest entry in OKLab wins, the
/// lowest index on a tie.
pub(crate) struct Snap {
    entries: Vec<(Rgb, [f64; 3])>,
}

impl Snap {
    pub(crate) fn new(palette: &[Rgb]) -> Snap {
        Snap {
            entries: palette.iter().map(|&c| (c, lab(c))).collect(),
        }
    }

    pub(crate) fn nearest(&self, c: Rgb) -> Rgb {
        let target = lab(c);
        let mut best: Option<(f64, Rgb)> = None;
        for &(rgb, l) in &self.entries {
            let d = dist2(target, l);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, rgb));
            }
        }
        best.map_or(c, |(_, rgb)| rgb)
    }
}

/// xterm's colours 16-255: the 6×6×6 cube, then the 24 greys. Colours 0-15 are
/// whatever the terminal's theme says, so a 256-colour downgrade never uses them.
pub fn xterm256() -> [Rgb; 240] {
    const LEVELS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    let mut out = [Rgb([0, 0, 0]); 240];
    for (i, c) in out.iter_mut().enumerate() {
        *c = if i < 216 {
            Rgb([LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6]])
        } else {
            let v = 8 + 10 * (i - 216) as u8;
            Rgb([v, v, v])
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(Rgb::parse("#1d121a"), Some(Rgb([0x1d, 0x12, 0x1a])));
        assert_eq!(Rgb::parse("#AA5500"), Some(Rgb([0xaa, 0x55, 0x00])));
        assert_eq!(Rgb::from_u32(0xaa5500).to_string(), "#aa5500");
        for bad in ["1d121a", "#1d121", "#1d121a0", "#1d12g0", "#+d121a", ""] {
            assert_eq!(Rgb::parse(bad), None, "{bad:?}");
        }
    }

    /// `round3` against exact rational rounding over every k / 4096, a grid
    /// with exact decimal ties (0.0625 → 0.062, 0.1875 → 0.188).
    #[test]
    fn round3_is_pythons_round() {
        for k in 0u64..=4096 {
            let t = k as f64 / 4096.0;
            // t * 1000 = k * 125 / 512 exactly; round half to even.
            let (q, r) = (k * 125 / 512, k * 125 % 512);
            let q = match r.cmp(&256) {
                std::cmp::Ordering::Less => q,
                std::cmp::Ordering::Greater => q + 1,
                std::cmp::Ordering::Equal => q + (q & 1),
            };
            assert_eq!(round3(t), q as f64 / 1000.0, "k = {k}");
        }
        assert_eq!(round3(0.0005), 0.001); // just above the tie in binary
    }

    #[test]
    fn clamp_follows_python_min_max() {
        assert_eq!(clamp01(f64::NAN), 1.0);
        assert_eq!(clamp01(-0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(clamp01(-2.0), 0.0);
        assert_eq!(clamp01(7.0), 1.0);
        assert_eq!(clamp01(0.25), 0.25);
    }

    /// Blackwater's lightbar (`base` 28% toward `jam`) is #452136 in the goldens.
    #[test]
    fn mix_matches_the_generator() {
        let base = Rgb::from_u32(0x1d121a);
        let jam = Rgb::from_u32(0xba4889);
        let p = |s| Rgb::parse(s).unwrap();
        assert_eq!(mix(base, jam, 0.28), p("#452136"));
        assert_eq!(mix(base, jam, 0.0), base);
        assert_eq!(mix(base, jam, 1.0), jam);
        // t is clamped before rounding.
        assert_eq!(mix(base, jam, -3.0), base);
        assert_eq!(mix(base, jam, 9.0), jam);
    }

    #[test]
    fn ramp_edges() {
        let a = Rgb::from_u32(0x000000);
        let b = Rgb::from_u32(0xffffff);
        let c = Rgb::from_u32(0xff0000);
        assert_eq!(ramp(&[a, b, c], 0.0), a);
        assert_eq!(ramp(&[a, b, c], 0.5), b);
        assert_eq!(ramp(&[a, b, c], 1.0), c);
        assert_eq!(ramp(&[a, b, c], 2.0), c);
        assert_eq!(ramp(&[a, b, c], -1.0), a);
        assert_eq!(ramp(&[c], 0.3), c);
        assert_eq!(ramp(&[], 0.3), Rgb([0, 0, 0]));
        assert_eq!(avg(&[a, a]), a);
        assert_eq!(avg(&[]), Rgb([0, 0, 0]));
    }

    #[test]
    fn snap_picks_nearest_then_lowest_index() {
        let red = Rgb::from_u32(0xff0000);
        let snap = Snap::new(&[red, Rgb::from_u32(0x0000ff), red]);
        assert_eq!(snap.nearest(Rgb::from_u32(0xee1111)), red);
        assert_eq!(
            snap.nearest(Rgb::from_u32(0x1111ee)),
            Rgb::from_u32(0x0000ff)
        );
        assert_eq!(Snap::new(&[]).nearest(red), red);
    }

    #[test]
    fn xterm256_layout() {
        let p = xterm256();
        assert_eq!(p[0], Rgb([0, 0, 0])); // 16
        assert_eq!(p[1], Rgb([0, 0, 0x5f])); // 17
        assert_eq!(p[215], Rgb([0xff, 0xff, 0xff])); // 231
        assert_eq!(p[216], Rgb([8, 8, 8])); // 232
        assert_eq!(p[239], Rgb([0xee, 0xee, 0xee])); // 255
    }
}
