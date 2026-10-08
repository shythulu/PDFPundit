//! Themes (T-17): the seven themes of the mockup with their colour roles, the
//! border and logo gradients, and the downgrade to 256 or 16 colours.
//!
//! Ported from generate.py's `mk_theme` and `db_theme`. The four DarkBerry
//! flavours are read from `assets/theme/darkberry-palette.json` (v0.3.0, the
//! file the T-00 goldens were dumped with); the other three are generate.py's
//! literal colours. A theme's sixteen role slots follow the mockup's CGA order
//! `KbgcrmywDBGCRMYW`; `Roles` names the ones the screens use (MOCK "Theme roles").
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use super::color::{ColorCaps, Rgb, Snap, mix, xterm256};

/// What each part of the screen is drawn in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roles {
    /// Background (`K`).
    pub bg: Rgb,
    /// Lightbar and status bar (`b`).
    pub lightbar: Rgb,
    /// Headings (`W`).
    pub heading: Rgb,
    /// Body text (`w`).
    pub body: Rgb,
    /// Dim text and shadows (`D`).
    pub dim: Rgb,
    /// File names (`C`).
    pub file: Rgb,
    /// Hotkeys and warnings (`Y`).
    pub hotkey: Rgb,
    /// Errors (`R`).
    pub error: Rgb,
    /// Repaired, ok (`G`).
    pub ok: Rgb,
    /// Needs input (`M`).
    pub needs_input: Rgb,
    /// Accents (`m`).
    pub accent: Rgb,
    /// Info (`c`).
    pub info: Rgb,
    /// The box-border gradient, interpolated in OKLab.
    pub border_stops: Vec<Rgb>,
}

/// The gradients the screens draw with, besides the border (generate.py's
/// `mk_theme` keys of the same names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gradients {
    /// Modal borders (font pick, theme chooser).
    pub modal: Vec<Rgb>,
    /// The per-file menu's border.
    pub menu: Vec<Rgb>,
    /// The current file's progress bar.
    pub bar: Vec<Rgb>,
    /// The batch progress bar (the widget's too).
    pub bar2: Vec<Rgb>,
    /// The recovery bars.
    pub ok: Vec<Rgb>,
    /// The VU meter.
    pub vu: Vec<Rgb>,
    /// Box separators.
    pub sep: Vec<Rgb>,
    /// The tagline and the quiet hints.
    pub tag: Vec<Rgb>,
}

/// One theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    /// A dark background (DarkBerry's `dark` flag; the other three are dark).
    pub dark: bool,
    /// The terminal's ANSI colours 0-15 for this theme: 0-7 normal, 8-15
    /// bright, in ANSI order (black, red, green, yellow, blue, magenta, cyan,
    /// white). DarkBerry's come from the palette's `ansiColors`.
    pub ansi: [Rgb; 16],
    /// The sixteen role slots in the mockup's CGA order `KbgcrmywDBGCRMYW`,
    /// the letters its inline colour markup names; see [`Theme::slot`].
    pub slots: [Rgb; 16],
    pub roles: Roles,
    pub gradients: Gradients,
    /// The logo gradient, top to bottom.
    pub logo_ramp: Vec<Rgb>,
}

/// generate.py's `KEYS`: the slot letters in [`Theme::slots`] order.
pub const SLOT_KEYS: [char; 16] = [
    'K', 'b', 'g', 'c', 'r', 'm', 'y', 'w', 'D', 'B', 'G', 'C', 'R', 'M', 'Y', 'W',
];

/// The default theme's name (D3).
const DEFAULT: &str = "DarkBerry Blackwater";

/// generate.py's ANSI_PAIRS: the role slot behind ANSI 0-7 and 8-15.
const ANSI_PAIRS: [(usize, usize); 8] = [
    (K, D),
    (R_LO, R),
    (G_LO, G),
    (Y_LO, Y),
    (B_LO, B),
    (M_LO, M),
    (C_LO, C),
    (W_LO, W),
];

// Role-slot indices in generate.py's KEYS order, `KbgcrmywDBGCRMYW`.
const K: usize = 0;
const B_LO: usize = 1;
const G_LO: usize = 2;
const C_LO: usize = 3;
const R_LO: usize = 4;
const M_LO: usize = 5;
const Y_LO: usize = 6;
const W_LO: usize = 7;
const D: usize = 8;
const B: usize = 9;
const G: usize = 10;
const C: usize = 11;
const R: usize = 12;
const M: usize = 13;
const Y: usize = 14;
const W: usize = 15;

impl Theme {
    /// Every theme, in the chooser's order.
    pub fn all() -> &'static [Theme] {
        &THEMES
    }

    /// DarkBerry Blackwater.
    pub fn default_theme() -> &'static Theme {
        &THEMES[0]
    }

    /// The slot a markup letter names (`'K'` the background, `'M'` needs
    /// input, …); `None` for any other character.
    pub fn slot(&self, key: char) -> Option<Rgb> {
        SLOT_KEYS
            .iter()
            .position(|&k| k == key)
            .map(|i| self.slots[i])
    }

    /// This theme as a terminal with `caps` can show it. True colour is the
    /// theme itself. 256 colours snap every role and gradient stop to the
    /// nearest of xterm's colours 16-255; 16 colours snap them to the nearest of
    /// the theme's own `ansi`, so each is drawn as one ANSI index. Nearest is by
    /// OKLab distance, lowest index on a tie; `ansi` itself is never changed.
    pub fn downgrade(&self, caps: ColorCaps) -> Theme {
        let snap = match caps {
            ColorCaps::TrueColor => return self.clone(),
            ColorCaps::Ansi256 => Snap::new(&xterm256()),
            ColorCaps::Ansi16 => Snap::new(&self.ansi),
        };
        let s = |c: Rgb| snap.nearest(c);
        let v = |stops: &[Rgb]| stops.iter().map(|&c| s(c)).collect::<Vec<_>>();
        let r = &self.roles;
        let g = &self.gradients;
        Theme {
            name: self.name,
            dark: self.dark,
            ansi: self.ansi,
            slots: self.slots.map(s),
            roles: Roles {
                bg: s(r.bg),
                lightbar: s(r.lightbar),
                heading: s(r.heading),
                body: s(r.body),
                dim: s(r.dim),
                file: s(r.file),
                hotkey: s(r.hotkey),
                error: s(r.error),
                ok: s(r.ok),
                needs_input: s(r.needs_input),
                accent: s(r.accent),
                info: s(r.info),
                border_stops: v(&r.border_stops),
            },
            gradients: Gradients {
                modal: v(&g.modal),
                menu: v(&g.menu),
                bar: v(&g.bar),
                bar2: v(&g.bar2),
                ok: v(&g.ok),
                vu: v(&g.vu),
                sep: v(&g.sep),
                tag: v(&g.tag),
            },
            logo_ramp: v(&self.logo_ramp),
        }
    }
}

/// generate.py's `mk_theme`: roles from the sixteen slots, the border defaulting
/// to `[W, C, B, b, D]`, the other gradients to `mk_theme`'s defaults and ANSI
/// 0-15 to the slots through ANSI_PAIRS.
fn mk_theme(
    name: &'static str,
    dark: bool,
    slots: [Rgb; 16],
    logo_ramp: Vec<Rgb>,
    border: Option<(Vec<Rgb>, Gradients)>,
    ansi: Option<[Rgb; 16]>,
) -> Theme {
    let a = slots;
    let (border, gradients) = match border {
        Some((b, g)) => (b, g),
        None => (
            vec![a[W], a[C], a[B], a[B_LO], a[D]],
            Gradients {
                modal: vec![a[W], a[M], a[M_LO], a[B_LO]],
                menu: vec![a[W], a[Y], a[Y_LO], a[D]],
                bar: vec![a[B_LO], a[B], a[C]],
                bar2: vec![a[M_LO], a[M], a[W]],
                ok: vec![a[G_LO], a[G]],
                vu: vec![a[R_LO], a[R], a[Y], a[G]],
                sep: vec![a[K], a[D], a[B]],
                tag: vec![a[D], a[B], a[M], a[W]],
            },
        ),
    };
    let ansi = ansi.unwrap_or_else(|| {
        let mut out = [Rgb([0, 0, 0]); 16];
        for (i, (normal, bright)) in ANSI_PAIRS.into_iter().enumerate() {
            out[i] = a[normal];
            out[i + 8] = a[bright];
        }
        out
    });
    Theme {
        name,
        dark,
        ansi,
        slots,
        roles: Roles {
            bg: a[K],
            lightbar: a[B_LO],
            heading: a[W],
            body: a[W_LO],
            dim: a[D],
            file: a[C],
            hotkey: a[Y],
            error: a[R],
            ok: a[G],
            needs_input: a[M],
            accent: a[M_LO],
            info: a[C_LO],
            border_stops: border,
        },
        gradients,
        logo_ramp,
    }
}

/// The DarkBerry palette, vendored from the mockup (see THIRD_PARTY_NOTICES.md).
const PALETTE_JSON: &str = include_str!("../../assets/theme/darkberry-palette.json");

/// The flavours in chooser order: palette key and theme name.
const DARKBERRY: [(&str, &str); 4] = [
    ("blackwater", "DarkBerry Blackwater"),
    ("mire", "DarkBerry Mire"),
    ("fen", "DarkBerry Fen"),
    ("wisp", "DarkBerry Wisp"),
];

/// ANSI 0-7, in the palette's `ansiColors` names.
const ANSI_ORDER: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

#[derive(Deserialize)]
struct Palette {
    blackwater: Flavour,
    mire: Flavour,
    fen: Flavour,
    wisp: Flavour,
}

impl Palette {
    fn flavour(&self, key: &str) -> &Flavour {
        match key {
            "blackwater" => &self.blackwater,
            "mire" => &self.mire,
            "fen" => &self.fen,
            _ => &self.wisp,
        }
    }
}

#[derive(Deserialize)]
struct Flavour {
    dark: bool,
    colors: BTreeMap<String, Swatch>,
    #[serde(rename = "ansiColors")]
    ansi_colors: BTreeMap<String, AnsiPair>,
}

#[derive(Deserialize)]
struct Swatch {
    hex: String,
}

#[derive(Deserialize)]
struct AnsiPair {
    normal: Swatch,
    bright: Swatch,
}

fn swatch(s: &Swatch, what: &str) -> Result<Rgb, String> {
    Rgb::parse(&s.hex).ok_or_else(|| format!("{what}: bad colour {:?}", s.hex))
}

/// generate.py's `db_theme`: a DarkBerry flavour mapped onto the roles.
fn darkberry(name: &'static str, fl: &Flavour) -> Result<Theme, String> {
    let c = |key: &str| {
        fl.colors
            .get(key)
            .ok_or_else(|| format!("{name}: no palette colour {key}"))
            .and_then(|s| swatch(s, key))
    };
    let (base, jam) = (c("base")?, c("jam")?);
    let slots = [
        base,
        mix(base, jam, 0.28),
        c("gooseberry")?,
        c("juniper")?,
        c("cranberry")?,
        jam,
        c("apricot")?,
        c("subtext0")?,
        c("overlay1")?,
        c("lavender")?,
        c("gooseberry")?,
        c("blueberry")?,
        c("cranberry")?,
        c("berry")?,
        c("honey")?,
        c("text")?,
    ];
    let logo = vec![
        c("text")?,
        c("petal")?,
        c("berry")?,
        jam,
        mix(jam, base, 0.5),
    ];
    let stops = |names: &[&str]| names.iter().map(|&n| c(n)).collect::<Result<Vec<_>, _>>();
    let border = stops(&["petal", "berry", "jam", "overlay0", "surface2"])?;
    let gradients = Gradients {
        modal: stops(&["text", "berry", "jam", "surface2"])?,
        menu: stops(&["text", "honey", "apricot", "overlay0"])?,
        bar: stops(&["surface2", "bilberry", "blueberry", "plum"])?,
        bar2: stops(&["jam", "berry", "petal"])?,
        ok: stops(&["surface2", "gooseberry"])?,
        vu: stops(&["cranberry", "apricot", "honey", "gooseberry"])?,
        sep: stops(&["base", "surface2", "overlay1"])?,
        tag: stops(&["overlay0", "lavender", "berry", "text"])?,
    };
    let mut ansi = [Rgb([0, 0, 0]); 16];
    for (i, colour) in ANSI_ORDER.into_iter().enumerate() {
        let pair = fl
            .ansi_colors
            .get(colour)
            .ok_or_else(|| format!("{name}: no ANSI colour {colour}"))?;
        ansi[i] = swatch(&pair.normal, colour)?;
        ansi[i + 8] = swatch(&pair.bright, colour)?;
    }
    Ok(mk_theme(
        name,
        fl.dark,
        slots,
        logo,
        Some((border, gradients)),
        Some(ansi),
    ))
}

fn hexes<const N: usize>(v: [u32; N]) -> [Rgb; N] {
    v.map(Rgb::from_u32)
}

fn build_all() -> Vec<Theme> {
    let palette: Palette = serde_json::from_str(PALETTE_JSON).expect("the vendored palette parses");
    let mut themes: Vec<Theme> = DARKBERRY
        .iter()
        .map(|&(key, name)| darkberry(name, palette.flavour(key)).expect("the vendored palette"))
        .collect();
    themes.push(mk_theme(
        "ACiD Classic",
        true,
        hexes([
            0x000000, 0x0000aa, 0x00aa00, 0x00aaaa, 0xaa0000, 0xaa00aa, 0xaa5500, 0xaaaaaa,
            0x555555, 0x5555ff, 0x55ff55, 0x55ffff, 0xff5555, 0xff55ff, 0xffff55, 0xffffff,
        ]),
        hexes([0xffffff, 0x55ffff, 0x00aaaa, 0x5555ff, 0x0000aa, 0xaa00aa]).to_vec(),
        None,
        None,
    ));
    themes.push(mk_theme(
        "Pastel Parlour",
        true,
        hexes([
            0x1c1a22, 0x3a3350, 0x7fb8a4, 0x9fb4d8, 0xd9798f, 0xb889c4, 0xd9a877, 0xd8d2dc,
            0x6b6474, 0xc0c4e6, 0xbfe6c9, 0xcfe3f7, 0xff9fb5, 0xe1c0e9, 0xf4ddc7, 0xf9f6ed,
        ]),
        hexes([0xf9f6ed, 0xfcbbdb, 0xe1c0e9, 0xc0c4e6, 0xdcf1f0]).to_vec(),
        None,
        None,
    ));
    themes.push(mk_theme(
        "Mono Ink",
        true,
        hexes([
            0x0c0c0c, 0x2e2e2e, 0x8a8a8a, 0xa8a8a8, 0x6e6e6e, 0x7a7a7a, 0x9a9a9a, 0xbdbdbd,
            0x555555, 0xcfcfcf, 0xe0e0e0, 0xf0f0f0, 0xffffff, 0xd8d8d8, 0xeeeeee, 0xffffff,
        ]),
        hexes([0xffffff, 0xd0d0d0, 0xa0a0a0, 0x707070, 0x404040]).to_vec(),
        None,
        None,
    ));
    debug_assert_eq!(themes[0].name, DEFAULT);
    themes
}

static THEMES: LazyLock<Vec<Theme>> = LazyLock::new(build_all);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::color::ramp;
    use crate::ui::goldens::{self, Golden, GoldenCell};

    const PALETTE_PATH: &str = "assets/theme/darkberry-palette.json";
    const FLAVOURS: [&str; 4] = ["blackwater", "mire", "fen", "wisp"];

    fn palette_json() -> serde_json::Value {
        serde_json::from_str(PALETTE_JSON).expect("palette JSON")
    }

    /// A DarkBerry palette colour, read straight from the JSON.
    fn pc(flavour: &str, name: &str) -> Rgb {
        let v = palette_json();
        let hex = v[flavour]["colors"][name]["hex"]
            .as_str()
            .unwrap_or_else(|| panic!("{flavour}.{name}"));
        Rgb::parse(hex).expect("hex")
    }

    fn by_name(name: &str) -> &'static Theme {
        Theme::all()
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no theme {name}"))
    }

    /// The character and foreground of a text cell, or the two pixels of a
    /// half-block cell as (top, bottom).
    fn text_fg(g: &Golden, x: u16, y: u16) -> (char, Rgb) {
        match g.cell(x, y) {
            GoldenCell::Text { ch, fg, .. } => (ch, Rgb(fg.0)),
            other => panic!("{}: ({x}, {y}) is {other}", g.name),
        }
    }

    fn half(g: &Golden, x: u16, y: u16) -> (Rgb, Rgb) {
        match g.cell(x, y) {
            GoldenCell::Half {
                top: Some(t),
                bottom: Some(b),
            } => (Rgb(t.0), Rgb(b.0)),
            other => panic!("{}: ({x}, {y}) is {other}", g.name),
        }
    }

    /// The vendored palette is the one the goldens were dumped with.
    #[test]
    fn palette_asset_is_the_mockup_palette() {
        let bytes =
            std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PALETTE_PATH))
                .expect(PALETTE_PATH);
        assert_eq!(
            bytes,
            PALETTE_JSON.as_bytes(),
            "include_str! reads the asset"
        );
        let mut lf = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                lf.push(b);
            }
        }
        use sha2::{Digest, Sha256};
        let got: String = Sha256::digest(&lf)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(got, goldens::index().palette_sha256);
        assert_eq!(palette_json()["version"], "0.3.0");
    }

    #[test]
    fn seven_themes_in_chooser_order() {
        let names: Vec<&str> = Theme::all().iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "DarkBerry Blackwater",
                "DarkBerry Mire",
                "DarkBerry Fen",
                "DarkBerry Wisp",
                "ACiD Classic",
                "Pastel Parlour",
                "Mono Ink",
            ]
        );
        assert_eq!(Theme::default_theme().name, goldens::index().theme);
        assert!(std::ptr::eq(Theme::default_theme(), &Theme::all()[0]));
        let v = palette_json();
        for (key, t) in FLAVOURS.iter().zip(Theme::all()) {
            let json_name = v[key]["name"].as_str().expect("name");
            assert_eq!(t.name, format!("DarkBerry {json_name}"));
            assert_eq!(Some(t.dark), v[key]["dark"].as_bool(), "{key}: dark");
        }
        assert!(!by_name("DarkBerry Wisp").dark);
    }

    /// Every role resolves for all four flavours, and a palette that lacks a
    /// colour a role needs is refused by name.
    #[test]
    fn every_role_resolves_for_the_four_darkberry_flavours() {
        let palette: Palette = serde_json::from_str(PALETTE_JSON).expect("palette");
        for (key, name) in FLAVOURS.iter().zip(DARKBERRY) {
            let t = darkberry(name.1, palette.flavour(key)).expect(key);
            assert_eq!(t.name, name.1);
        }
        for missing in ["honey", "petal", "surface2", "base"] {
            let mut v = palette_json();
            v["mire"]["colors"].as_object_mut().unwrap().remove(missing);
            let p: Palette = serde_json::from_value(v).expect("palette");
            let err = darkberry("DarkBerry Mire", p.flavour("mire")).unwrap_err();
            assert!(err.contains(missing), "{err}");
        }
        let mut v = palette_json();
        v["fen"]["ansiColors"]
            .as_object_mut()
            .unwrap()
            .remove("cyan");
        let p: Palette = serde_json::from_value(v).expect("palette");
        let err = darkberry("DarkBerry Fen", p.flavour("fen")).unwrap_err();
        assert!(err.contains("cyan"), "{err}");
    }

    /// MOCK "Theme roles": the palette colour behind each role.
    #[test]
    fn role_table_matches_mock() {
        let v = palette_json();
        for (key, t) in FLAVOURS.iter().zip(Theme::all()) {
            let c = |n| pc(key, n);
            let r = &t.roles;
            assert_eq!(r.bg, c("base"), "{key}");
            assert_eq!(r.lightbar, mix(c("base"), c("jam"), 0.28), "{key}");
            assert_eq!(r.heading, c("text"), "{key}");
            assert_eq!(r.body, c("subtext0"), "{key}");
            assert_eq!(r.dim, c("overlay1"), "{key}");
            assert_eq!(r.file, c("blueberry"), "{key}");
            assert_eq!(r.hotkey, c("honey"), "{key}");
            assert_eq!(r.error, c("cranberry"), "{key}");
            assert_eq!(r.ok, c("gooseberry"), "{key}");
            assert_eq!(r.needs_input, c("berry"), "{key}");
            assert_eq!(r.accent, c("jam"), "{key}");
            assert_eq!(r.info, c("juniper"), "{key}");
            let border: Vec<Rgb> = ["petal", "berry", "jam", "overlay0", "surface2"]
                .into_iter()
                .map(c)
                .collect();
            assert_eq!(r.border_stops, border, "{key}");
            let logo = vec![
                c("text"),
                c("petal"),
                c("berry"),
                c("jam"),
                mix(c("jam"), c("base"), 0.5),
            ];
            assert_eq!(t.logo_ramp, logo, "{key}");
            for (i, colour) in [
                "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
            ]
            .into_iter()
            .enumerate()
            {
                for (j, shade) in ["normal", "bright"].into_iter().enumerate() {
                    let hex = v[key]["ansiColors"][colour][shade]["hex"].as_str().unwrap();
                    assert_eq!(
                        t.ansi[i + 8 * j],
                        Rgb::parse(hex).unwrap(),
                        "{key} {colour}"
                    );
                }
            }
        }
    }

    /// generate.py's `db_theme` gradients and `KEYS` slots, per flavour.
    #[test]
    fn darkberry_gradients_and_slots_match_db_theme() {
        for (key, t) in FLAVOURS.iter().zip(Theme::all()) {
            let c = |n: &str| pc(key, n);
            let v = |names: &[&str]| names.iter().map(|&n| c(n)).collect::<Vec<_>>();
            let g = &t.gradients;
            assert_eq!(g.modal, v(&["text", "berry", "jam", "surface2"]), "{key}");
            assert_eq!(
                g.menu,
                v(&["text", "honey", "apricot", "overlay0"]),
                "{key}"
            );
            assert_eq!(
                g.bar,
                v(&["surface2", "bilberry", "blueberry", "plum"]),
                "{key}"
            );
            assert_eq!(g.bar2, v(&["jam", "berry", "petal"]), "{key}");
            assert_eq!(g.ok, v(&["surface2", "gooseberry"]), "{key}");
            assert_eq!(
                g.vu,
                v(&["cranberry", "apricot", "honey", "gooseberry"]),
                "{key}"
            );
            assert_eq!(g.sep, v(&["base", "surface2", "overlay1"]), "{key}");
            assert_eq!(
                g.tag,
                v(&["overlay0", "lavender", "berry", "text"]),
                "{key}"
            );
            let r = &t.roles;
            for (k, want) in [
                ('K', r.bg),
                ('b', r.lightbar),
                ('W', r.heading),
                ('w', r.body),
                ('D', r.dim),
                ('C', r.file),
                ('Y', r.hotkey),
                ('R', r.error),
                ('G', r.ok),
                ('M', r.needs_input),
                ('m', r.accent),
                ('c', r.info),
                ('B', c("lavender")),
                ('y', c("apricot")),
                ('g', c("gooseberry")),
                ('r', c("cranberry")),
            ] {
                assert_eq!(t.slot(k), Some(want), "{key} {k}");
            }
            assert_eq!(t.slot('x'), None);
            assert_eq!(t.slot('_'), None);
        }
    }

    /// The other three themes take `mk_theme`'s default gradients.
    #[test]
    fn literal_themes_take_the_default_gradients() {
        for name in ["ACiD Classic", "Pastel Parlour", "Mono Ink"] {
            let t = by_name(name);
            let s = |k: char| t.slot(k).expect("slot");
            let v = |keys: &str| keys.chars().map(s).collect::<Vec<_>>();
            let g = &t.gradients;
            assert_eq!(g.modal, v("WMmb"), "{name}");
            assert_eq!(g.menu, v("WYyD"), "{name}");
            assert_eq!(g.bar, v("bBC"), "{name}");
            assert_eq!(g.bar2, v("mMW"), "{name}");
            assert_eq!(g.ok, v("gG"), "{name}");
            assert_eq!(g.vu, v("rRYG"), "{name}");
            assert_eq!(g.sep, v("KDB"), "{name}");
            assert_eq!(g.tag, v("DBMW"), "{name}");
        }
    }

    /// The other three themes: generate.py's slots, its default border, and
    /// ANSI 0-15 taken from the slots through ANSI_PAIRS.
    #[test]
    fn literal_themes_follow_mk_theme() {
        let acid = by_name("ACiD Classic");
        let hex = |v: u32| Rgb::from_u32(v);
        assert_eq!(acid.roles.bg, hex(0x000000));
        assert_eq!(acid.roles.lightbar, hex(0x0000aa));
        assert_eq!(acid.roles.heading, hex(0xffffff));
        assert_eq!(acid.roles.error, hex(0xff5555));
        assert_eq!(acid.roles.accent, hex(0xaa00aa));
        assert_eq!(acid.roles.info, hex(0x00aaaa));
        // [W, C, B, b, D]
        assert_eq!(
            acid.roles.border_stops,
            [0xffffff, 0x55ffff, 0x5555ff, 0x0000aa, 0x555555].map(hex)
        );
        // black, red, green, yellow, blue, magenta, cyan, white; then bright.
        assert_eq!(
            acid.ansi,
            [
                0x000000, 0xaa0000, 0x00aa00, 0xaa5500, 0x0000aa, 0xaa00aa, 0x00aaaa, 0xaaaaaa,
                0x555555, 0xff5555, 0x55ff55, 0xffff55, 0x5555ff, 0xff55ff, 0x55ffff, 0xffffff,
            ]
            .map(hex)
        );
        assert_eq!(acid.logo_ramp.len(), 6);
        for t in &Theme::all()[4..] {
            assert!(t.dark, "{}", t.name);
        }
    }

    /// 06-theme-chooser draws every theme's ANSI 0-15 (half blocks, bright on
    /// top) and an 18-cell logo ramp at t = k / 17; the default theme's role
    /// swatches; and its border and logo gradients at t = i / 53.
    #[test]
    fn chooser_golden_swatches_and_ramps() {
        let g = goldens::load("06-theme-chooser");
        for (i, t) in Theme::all().iter().enumerate() {
            let ry = 4 + 3 * i as u16;
            for k in 0..8u16 {
                let want = (t.ansi[usize::from(k) + 8], t.ansi[usize::from(k)]);
                assert_eq!(half(&g, 8 + k, ry + 1), want, "{} ansi {k}", t.name);
            }
            for k in 0..18u16 {
                let want = ('█', ramp(&t.logo_ramp, f64::from(k) / 17.0));
                assert_eq!(text_fg(&g, 17 + k, ry + 1), want, "{} logo {k}", t.name);
            }
        }
        let r = &Theme::default_theme().roles;
        let left = [r.heading, r.body, r.file, r.hotkey, r.dim];
        let right = [r.error, r.ok, r.needs_input, r.info, r.lightbar];
        for (x, col) in [(40, left), (73, right)] {
            for (i, want) in col.into_iter().enumerate() {
                let y = 8 + i as u16;
                assert_eq!(text_fg(&g, x, y), ('█', want), "role swatch ({x}, {y})");
                assert_eq!(text_fg(&g, x + 1, y), ('█', want), "role swatch ({x}, {y})");
            }
        }
        let th = Theme::default_theme();
        let c = |n| pc("blackwater", n);
        let rows: [(u16, Vec<Rgb>); 5] = [
            (14, th.roles.border_stops.clone()),
            (15, th.logo_ramp.clone()),
            (
                16,
                ["surface2", "bilberry", "blueberry", "plum"]
                    .map(c)
                    .to_vec(),
            ),
            (17, ["text", "berry", "jam", "surface2"].map(c).to_vec()),
            (18, ["jam", "berry", "petal"].map(c).to_vec()),
        ];
        for (y, stops) in rows {
            for i in 0..54u16 {
                let want = ('█', ramp(&stops, f64::from(i) / 53.0));
                assert_eq!(text_fg(&g, 51 + i, y), want, "gradient row {y} cell {i}");
            }
        }
        // The divider is dim mixed 20% toward the background.
        let div = mix(th.roles.dim, th.roles.bg, 0.2);
        let mut n = 0;
        for y in 3..34 {
            if let (ch @ '│', fg) = text_fg(&g, 38, y) {
                assert_eq!((ch, fg), ('│', div), "divider row {y}");
                n += 1;
            }
        }
        assert!(n >= 20, "{n} divider cells");
    }

    /// 01-idle's LAST CALLERS box: border colours at t = (i/(w-1) + j/(h-1)) / 2.
    #[test]
    fn idle_golden_box_border() {
        let g = goldens::load("01-idle");
        let stops = &Theme::default_theme().roles.border_stops;
        let (x0, y0, w, h) = (1u16, 9u16, 25u16, 10u16);
        let col = |i: u16, j: u16| {
            ramp(
                stops,
                (f64::from(i) / f64::from(w - 1) + f64::from(j) / f64::from(h - 1)) / 2.0,
            )
        };
        for j in 1..h - 1 {
            assert_eq!(text_fg(&g, x0, y0 + j), ('║', col(0, j)), "left {j}");
            assert_eq!(
                text_fg(&g, x0 + w - 1, y0 + j),
                ('║', col(w - 1, j)),
                "right {j}"
            );
        }
        for i in 0..w {
            let ch = match i {
                0 => '╚',
                i if i == w - 1 => '╝',
                _ => '═',
            };
            assert_eq!(
                text_fg(&g, x0 + i, y0 + h - 1),
                (ch, col(i, h - 1)),
                "bottom {i}"
            );
        }
    }

    /// The header: three logo blocks at t = .1, .4, .8, then a rule along the
    /// mirrored border gradient (nine stops) to column 110.
    #[test]
    fn header_golden_ramps() {
        let th = Theme::default_theme();
        let b = &th.roles.border_stops;
        let mut mirrored: Vec<Rgb> = b.iter().rev().copied().collect();
        mirrored.extend_from_slice(&b[1..]);
        for name in ["03-batch", "05-result"] {
            let g = goldens::load(name);
            for (x, ch, t) in [(1, '▐', 0.1), (2, '█', 0.4), (3, '▌', 0.8)] {
                assert_eq!(
                    text_fg(&g, x, 0),
                    (ch, ramp(&th.logo_ramp, t)),
                    "{name} logo {x}"
                );
            }
            let end = g.w - 2;
            let mut start = end;
            while text_fg(&g, start - 1, 0).0 == '─' {
                start -= 1;
            }
            let n = end - start + 1;
            assert!(n > 40, "{name}: rule of {n}");
            for i in 0..n {
                let want = ('─', ramp(&mirrored, f64::from(i) / f64::from(n - 1)));
                assert_eq!(text_fg(&g, start + i, 0), want, "{name} rule {i}");
            }
        }
    }

    fn colours(t: &Theme) -> Vec<Rgb> {
        let r = &t.roles;
        let mut v = vec![
            r.bg,
            r.lightbar,
            r.heading,
            r.body,
            r.dim,
            r.file,
            r.hotkey,
            r.error,
            r.ok,
            r.needs_input,
            r.accent,
            r.info,
        ];
        v.extend(&r.border_stops);
        v.extend(&t.logo_ramp);
        v
    }

    #[test]
    fn truecolor_downgrade_is_identity() {
        for t in Theme::all() {
            assert_eq!(&t.downgrade(ColorCaps::TrueColor), t);
        }
    }

    /// 16 colours: every role and stop becomes one of the theme's ANSI 0-15,
    /// the same one on every run and platform. The indices were computed
    /// independently in Python with generate.py's `lab` (nearest by squared
    /// OKLab distance, lowest index on a tie).
    #[test]
    fn sixteen_colour_downgrade_is_deterministic() {
        // roles (bg lightbar heading body dim file hotkey error ok needs_input
        // accent info), then border stops, then logo stops.
        let pinned: [(&str, &[usize]); 7] = [
            (
                "DarkBerry Blackwater",
                &[
                    0, 8, 15, 7, 1, 4, 3, 1, 2, 5, 9, 6, 15, 5, 9, 8, 8, 15, 15, 5, 9, 8,
                ],
            ),
            (
                "DarkBerry Mire",
                &[
                    0, 8, 15, 7, 12, 4, 3, 1, 2, 5, 9, 6, 7, 5, 9, 8, 8, 15, 7, 5, 9, 8,
                ],
            ),
            (
                "DarkBerry Fen",
                &[
                    0, 0, 15, 7, 8, 4, 3, 1, 2, 5, 13, 6, 15, 5, 13, 8, 8, 15, 15, 5, 13, 8,
                ],
            ),
            (
                "DarkBerry Wisp",
                &[
                    15, 15, 0, 8, 11, 4, 3, 1, 2, 5, 13, 6, 5, 5, 13, 7, 7, 0, 5, 5, 13, 7,
                ],
            ),
            (
                "ACiD Classic",
                &[
                    0, 4, 15, 7, 8, 14, 11, 9, 10, 13, 5, 6, 15, 14, 12, 4, 8, 15, 14, 6, 12, 4, 5,
                ],
            ),
            (
                "Pastel Parlour",
                &[
                    0, 4, 15, 7, 8, 14, 11, 9, 10, 13, 5, 6, 15, 14, 12, 4, 8, 15, 13, 13, 12, 15,
                ],
            ),
            (
                "Mono Ink",
                &[
                    0, 4, 9, 7, 8, 14, 11, 9, 10, 13, 5, 6, 9, 14, 12, 4, 8, 9, 12, 3, 1, 4,
                ],
            ),
        ];
        for (name, idx) in pinned {
            let t = by_name(name);
            let d = t.downgrade(ColorCaps::Ansi16);
            assert_eq!(d, t.downgrade(ColorCaps::Ansi16), "{name}: same twice");
            assert_eq!(d.ansi, t.ansi, "{name}: ansi unchanged");
            assert_eq!((d.name, d.dark), (t.name, t.dark));
            let want: Vec<Rgb> = idx.iter().map(|&i| t.ansi[i]).collect();
            assert_eq!(colours(&d), want, "{name}");
        }
        // ACiD Classic is built from its own sixteen colours, so nothing moves.
        let acid = by_name("ACiD Classic");
        assert_eq!(&acid.downgrade(ColorCaps::Ansi16), acid);
    }

    #[test]
    fn ansi256_downgrade_uses_the_xterm_cube_and_greys() {
        let xterm = xterm256();
        for t in Theme::all() {
            let d = t.downgrade(ColorCaps::Ansi256);
            assert_eq!(d, t.downgrade(ColorCaps::Ansi256), "{}: same twice", t.name);
            assert_eq!(d.ansi, t.ansi);
            let g = &d.gradients;
            let stops = [
                &g.modal, &g.menu, &g.bar, &g.bar2, &g.ok, &g.vu, &g.sep, &g.tag,
            ];
            let all = colours(&d)
                .into_iter()
                .chain(d.slots)
                .chain(stops.into_iter().flatten().copied());
            for c in all {
                assert!(xterm.contains(&c), "{}: {c} is not an xterm colour", t.name);
            }
        }
        // A colour already on the cube stays put.
        let acid = by_name("ACiD Classic").downgrade(ColorCaps::Ansi256);
        assert_eq!(acid.roles.bg, Rgb::from_u32(0x000000));
        assert_eq!(acid.roles.heading, Rgb::from_u32(0xffffff));
    }

    /// Tripwire (eng-r2-q8): no single-precision float in the colour or cat code.
    #[test]
    fn no_f32_in_colour_or_cat_code() {
        let needle = concat!("f", "32");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui");
        let mut files = vec![root.join("color.rs"), root.join("cat.rs")];
        if let Ok(dir) = std::fs::read_dir(root.join("cat")) {
            files.extend(dir.map(|e| e.expect("dir entry").path()));
        }
        for f in files {
            let text =
                std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            assert!(!text.contains(needle), "{} mentions {needle}", f.display());
        }
    }
}
