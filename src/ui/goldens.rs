//! The cell-exact goldens dumped from the mockup generator (T-00, D-055).
//!
//! `nimbalyst-local/mockups/pdfpundit-ansi-bbs/dump_goldens.py` writes one JSON
//! file per frame to `tests/data/ui/`; this module loads them for the UI tests,
//! which must match 100% of cells, characters and colours both. A mismatch is
//! reported as `(x, y, expected, got)`; a platform-specific diff is a bug, not a
//! tolerance. Test-only: nothing here ships.

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The directory the goldens live in.
pub(crate) fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ui")
}

/// An sRGB colour, read from the goldens' lowercase `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Rgb(pub(crate) [u8; 3]);

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let hex = s
            .strip_prefix('#')
            .filter(|h| {
                h.len() == 6
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or_else(|| format!("not a lowercase #rrggbb colour: {s:?}"))?;
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
        Ok(Rgb([byte(0)?, byte(2)?, byte(4)?]))
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0;
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

/// One cell of a golden frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Option<(String, Option<Rgb>, Option<Rgb>)>")]
pub(crate) enum GoldenCell {
    /// `[ch, fg, bg]`: one character.
    Text { ch: char, fg: Rgb, bg: Rgb },
    /// `["HB", top, bottom]`: two pixels, drawn as `▀` with fg = top and
    /// bg = bottom. In a canvas both halves are set; in a cat grid a pixel the
    /// cat does not draw is `None`.
    Half {
        top: Option<Rgb>,
        bottom: Option<Rgb>,
    },
    /// `null`: a cat-grid cell with neither pixel drawn.
    Empty,
}

impl TryFrom<Option<(String, Option<Rgb>, Option<Rgb>)>> for GoldenCell {
    type Error = String;

    fn try_from(v: Option<(String, Option<Rgb>, Option<Rgb>)>) -> Result<Self, String> {
        let Some((ch, a, b)) = v else {
            return Ok(GoldenCell::Empty);
        };
        if ch == "HB" {
            if a.is_none() && b.is_none() {
                return Err("a half-block cell with no pixel must be null".into());
            }
            return Ok(GoldenCell::Half { top: a, bottom: b });
        }
        let mut chars = ch.chars();
        match (chars.next(), chars.next(), a, b) {
            (Some(ch), None, Some(fg), Some(bg)) => Ok(GoldenCell::Text { ch, fg, bg }),
            _ => Err(format!("bad text cell: {ch:?} {a:?} {b:?}")),
        }
    }
}

impl fmt::Display for GoldenCell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opt = |c: &Option<Rgb>| c.map_or_else(|| "none".to_string(), |c| c.to_string());
        match self {
            GoldenCell::Text { ch, fg, bg } => write!(f, "{ch:?} fg {fg} bg {bg}"),
            GoldenCell::Half { top, bottom } => {
                write!(f, "half top {} bottom {}", opt(top), opt(bottom))
            }
            GoldenCell::Empty => f.write_str("empty"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    /// A whole 112×38 or 32×16 screen.
    Canvas,
    /// One `cat_grid` at one scale and pose.
    Cat,
}

/// generate.py's `pose()` fields, as the frame passed them to `cat_grid`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoldenPose {
    pub(crate) yaw: f64,
    pub(crate) pitch: f64,
    pub(crate) eo: f64,
    pub(crate) mouth: f64,
    pub(crate) gx: f64,
    pub(crate) gy: f64,
    pub(crate) ears: f64,
    pub(crate) happy: bool,
    pub(crate) meme: f64,
    pub(crate) plate: Option<f64>,
    pub(crate) chew: bool,
    pub(crate) puff: f64,
}

/// The placeholder policy for embedded (browser-rendered) text, 04-font-pick only.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Placeholder {
    pub(crate) policy: String,
    pub(crate) text: String,
    pub(crate) spans: Vec<EmbeddedSpan>,
}

/// One embedded span: `w` cells from `(x, y)`, replaced by the placeholder text.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmbeddedSpan {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) w: u16,
    pub(crate) rtl: bool,
    pub(crate) fg: Rgb,
    pub(crate) bg: Rgb,
    /// The mockup's original string, for reference.
    pub(crate) text: String,
}

/// One golden frame.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Golden {
    pub(crate) name: String,
    pub(crate) kind: Kind,
    pub(crate) w: u16,
    pub(crate) h: u16,
    /// Cats only.
    #[serde(default)]
    pub(crate) scale: Option<f64>,
    /// Cats only.
    #[serde(default)]
    pub(crate) glow: Option<f64>,
    /// Cats only.
    #[serde(default)]
    pub(crate) pose: Option<GoldenPose>,
    /// Row-major, `w * h` cells.
    pub(crate) cells: Vec<GoldenCell>,
    /// `(x, y)`, sorted by row then column.
    pub(crate) blink: Vec<(u16, u16)>,
    /// Canvases only; `Some` on 04-font-pick.
    #[serde(default)]
    pub(crate) placeholder: Option<Placeholder>,
}

impl Golden {
    pub(crate) fn cell(&self, x: u16, y: u16) -> GoldenCell {
        self.cells[usize::from(y) * usize::from(self.w) + usize::from(x)]
    }

    /// Every cell where `got` differs, as `(x, y, expected, got)`. A size
    /// mismatch is reported as a single entry at `(w, h)`.
    pub(crate) fn mismatches(
        &self,
        w: u16,
        h: u16,
        got: impl Fn(u16, u16) -> GoldenCell,
    ) -> Vec<(u16, u16, GoldenCell, GoldenCell)> {
        if (w, h) != (self.w, self.h) {
            return vec![(w, h, GoldenCell::Empty, GoldenCell::Empty)];
        }
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let (want, have) = (self.cell(x, y), got(x, y));
                if want != have {
                    out.push((x, y, want, have));
                }
            }
        }
        out
    }

    /// Panics with the mismatch list unless `got` matches every cell.
    #[track_caller]
    pub(crate) fn assert_matches(&self, w: u16, h: u16, got: impl Fn(u16, u16) -> GoldenCell) {
        let bad = self.mismatches(w, h, got);
        if bad.is_empty() {
            return;
        }
        let mut msg = format!(
            "{}: {} of {} cells differ",
            self.name,
            bad.len(),
            self.cells.len()
        );
        if (w, h) != (self.w, self.h) {
            msg = format!("{}: size {w}x{h}, golden {}x{}", self.name, self.w, self.h);
        }
        for (x, y, want, have) in bad.iter().take(40) {
            msg.push_str(&format!("\n  ({x}, {y}) expected {want}, got {have}"));
        }
        panic!("{msg}");
    }
}

/// `index.json`: the frame list and the generator pin.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Index {
    pub(crate) generator: String,
    pub(crate) pinned_commit: String,
    pub(crate) generate_py_sha256: String,
    pub(crate) palette_sha256: String,
    pub(crate) theme: String,
    pub(crate) scales: Vec<f64>,
    pub(crate) frames: Vec<IndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IndexEntry {
    pub(crate) name: String,
    pub(crate) kind: Kind,
}

pub(crate) fn index() -> Index {
    let path = dir().join("index.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Loads one golden by frame name, e.g. `01-idle` or `cat-0.93-drag-4`.
pub(crate) fn load(name: &str) -> Golden {
    let path = dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let g: Golden =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(g.name, name, "{}: name field", path.display());
    g
}

/// Self-goldens (D-048): screens the mockup has no frame for yet (the browse
/// picker, T-37) are checked against a frame this crate drew once and
/// committed, in the T-00 goldens' format, under `tests/data/ui-self/`. They
/// hold the look still until the user supplies a mockup frame, which then
/// replaces them. To accept a deliberate change, run the test with
/// `PDFPUNDIT_BLESS_UI=1` and review the diff.
pub(crate) fn self_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ui-self")
}

/// `c` in the goldens' JSON, one cell a line so a diff names the cells.
pub(crate) fn to_json(c: &crate::ui::canvas::Canvas, name: &str) -> String {
    let q = |s: &str| serde_json::to_string(s).expect("a string serialises");
    let mut out = format!(
        "{{\n\"name\": {},\n\"kind\": \"canvas\",\n\"w\": {},\n\"h\": {},\n\"cells\": [\n",
        q(name),
        c.w,
        c.h
    );
    let mut cells = Vec::with_capacity(c.cells.len());
    for y in 0..c.h {
        for x in 0..c.w {
            cells.push(match c.golden_cell(x, y) {
                GoldenCell::Text { ch, fg, bg } => {
                    format!("[{}, \"{fg}\", \"{bg}\"]", q(&ch.to_string()))
                }
                GoldenCell::Half { top, bottom } => {
                    let px =
                        |p: Option<Rgb>| p.map_or_else(|| "null".into(), |p| format!("\"{p}\""));
                    format!("[\"HB\", {}, {}]", px(top), px(bottom))
                }
                GoldenCell::Empty => "null".into(),
            });
        }
    }
    out.push_str(&cells.join(",\n"));
    out.push_str("\n],\n\"blink\": [");
    let blink: Vec<String> = c.blink.iter().map(|(x, y)| format!("[{x}, {y}]")).collect();
    out.push_str(&blink.join(", "));
    out.push_str("]\n}\n");
    out
}

/// Panics unless `c` equals the committed self-golden `name`, cell for cell
/// and in its blink set.
#[track_caller]
pub(crate) fn assert_self_golden(c: &crate::ui::canvas::Canvas, name: &str) {
    let path = self_dir().join(format!("{name}.json"));
    if std::env::var_os("PDFPUNDIT_BLESS_UI").is_some() {
        std::fs::create_dir_all(self_dir()).expect("create tests/data/ui-self");
        std::fs::write(&path, to_json(c, name)).expect("write the self-golden");
        return;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run with PDFPUNDIT_BLESS_UI=1 to write it",
            path.display()
        )
    });
    let g: Golden =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(g.name, name, "{}: name field", path.display());
    c.assert_matches(&g);
}

/// The cat-golden name for a scale and pose key (`cat-0.465-wchomp-3`).
pub(crate) fn cat_name(scale: f64, pose: &str) -> String {
    format!("cat-{scale}-{pose}")
}

/// The four cat scales generate.py draws at (full, widget, batch, result).
pub(crate) const SCALES: [f64; 4] = [0.93, 0.465, 0.68, 0.52];

/// The pose keys every scale has a cat golden for.
pub(crate) fn pose_keys() -> Vec<String> {
    let mut keys: Vec<String> = ["closed", "open", "happy"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    keys.extend(
        ["idle", "working", "needs", "done"]
            .iter()
            .map(|s| format!("widget-{s}")),
    );
    for prefix in ["drag", "chomp", "wdrag", "wchomp"] {
        keys.extend((1..=8).map(|i| format!("{prefix}-{i}")));
    }
    keys
}

/// The canvas frames the UI tickets compare against (T-21, T-22a/b, T-24, and
/// T-19's keyframes through the full and widget layouts).
pub(crate) const CANVASES: &[&str] = &[
    "01-idle",
    "02-drag-1-file-enters",
    "02-drag-2-ears-up",
    "02-drag-3-turns",
    "02-drag-4-looks-up",
    "02-drag-5-tracks-it",
    "02-drag-6-jaw-drops",
    "02-drag-7-wider",
    "02-drag-8-ready-to-eat",
    "02b-chomp-1-plop",
    "02b-chomp-2-looks-up",
    "02b-chomp-3-jaw-drops",
    "02b-chomp-4-chomp",
    "02b-chomp-5-nom",
    "02b-chomp-6-nom-nom",
    "02b-chomp-7-gulp",
    "02b-chomp-8-burp",
    "03-batch",
    "04-font-pick",
    "05-result",
    "06-theme-chooser",
    "07-widget-idle",
    "07-widget-working",
    "07-widget-needs-you",
    "07-widget-done",
    "08-widget-drop-1-file-enters",
    "08-widget-drop-2-ears-up",
    "08-widget-drop-3-turns",
    "08-widget-drop-4-looks-up",
    "08-widget-drop-5-tracks-it",
    "08-widget-drop-6-jaw-drops",
    "08-widget-drop-7-wider",
    "08-widget-drop-8-ready-to-eat",
    "08b-widget-chomp-1-plop",
    "08b-widget-chomp-2-looks-up",
    "08b-widget-chomp-3-jaw-drops",
    "08b-widget-chomp-4-chomp",
    "08b-widget-chomp-5-nom",
    "08b-widget-chomp-6-nom-nom",
    "08b-widget-chomp-7-gulp",
    "08b-widget-chomp-8-burp",
];

fn check_shape(g: &Golden) {
    let name = &g.name;
    assert_eq!(
        g.cells.len(),
        usize::from(g.w) * usize::from(g.h),
        "{name}: cell count"
    );
    let mut sorted = g.blink.clone();
    sorted.sort_by_key(|&(x, y)| (y, x));
    sorted.dedup();
    assert_eq!(sorted, g.blink, "{name}: blink is sorted and unique");
    assert!(
        g.blink.iter().all(|&(x, y)| x < g.w && y < g.h),
        "{name}: blink in bounds"
    );
    match g.kind {
        Kind::Canvas => {
            assert!(
                (g.w, g.h) == (112, 38) || (g.w, g.h) == (32, 16),
                "{name}: canvas size"
            );
            assert!(
                g.scale.is_none() && g.glow.is_none() && g.pose.is_none(),
                "{name}: cat fields"
            );
            for c in &g.cells {
                let ok = match c {
                    GoldenCell::Text { .. } => true,
                    GoldenCell::Half { top, bottom } => top.is_some() && bottom.is_some(),
                    GoldenCell::Empty => false,
                };
                assert!(ok, "{name}: canvas cell {c}");
            }
        }
        Kind::Cat => {
            let scale = g.scale.unwrap_or_else(|| panic!("{name}: no scale"));
            assert!(SCALES.contains(&scale), "{name}: scale {scale}");
            assert!(
                g.glow.is_some() && g.pose.is_some(),
                "{name}: glow and pose"
            );
            assert!(
                g.placeholder.is_none() && g.blink.is_empty(),
                "{name}: canvas fields"
            );
            assert!(
                g.cells
                    .iter()
                    .all(|c| !matches!(c, GoldenCell::Text { .. })),
                "{name}: a cat grid holds only pixels"
            );
            assert!(
                g.cells.iter().any(|c| *c != GoldenCell::Empty),
                "{name}: no cat drawn"
            );
        }
    }
}

/// Every golden in the index loads, has a consistent shape, and the directory
/// holds exactly the indexed set; every frame the UI tickets name is present.
#[test]
fn load_all() {
    let index = index();
    let mut names: Vec<&str> = index.frames.iter().map(|f| f.name.as_str()).collect();
    names.sort_unstable();
    let total = names.len();
    names.dedup();
    assert_eq!(names.len(), total, "duplicate frame names in index.json");

    let mut on_disk: Vec<String> = std::fs::read_dir(dir())
        .expect("tests/data/ui")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter_map(|f| f.strip_suffix(".json").map(str::to_string))
        .filter(|f| f != "index")
        .collect();
    on_disk.sort_unstable();
    assert_eq!(
        on_disk, names,
        "tests/data/ui holds exactly the indexed frames"
    );

    for entry in &index.frames {
        let g = load(&entry.name);
        assert_eq!(g.kind, entry.kind, "{}: kind", entry.name);
        check_shape(&g);
    }

    for name in CANVASES {
        let entry = index.frames.iter().find(|f| f.name == *name);
        assert_eq!(
            entry.map(|f| f.kind),
            Some(Kind::Canvas),
            "canvas golden {name} missing"
        );
    }
    assert_eq!(index.scales, SCALES);
    for scale in SCALES {
        for key in pose_keys() {
            let name = cat_name(scale, &key);
            let g = load(&name);
            assert_eq!(g.scale, Some(scale), "{name}: scale");
        }
    }
}

/// 04-font-pick records its placeholder policy, and the cells of each embedded
/// span hold the placeholder text then spaces, in the span's colours.
#[test]
fn font_pick_placeholder() {
    let g = load("04-font-pick");
    let p = g
        .placeholder
        .clone()
        .expect("04-font-pick records its placeholder policy");
    assert!(!p.policy.is_empty() && !p.text.is_empty());
    assert_eq!(p.spans.len(), 5);
    assert!(p.spans.iter().filter(|s| s.rtl).count() == 4);
    for s in &p.spans {
        let text: Vec<char> = p.text.chars().collect();
        for i in 0..s.w {
            let ch = text.get(usize::from(i)).copied().unwrap_or(' ');
            let want = GoldenCell::Text {
                ch,
                fg: s.fg,
                bg: s.bg,
            };
            assert_eq!(
                g.cell(s.x + i, s.y),
                want,
                "span at ({}, {}) cell {i}",
                s.x,
                s.y
            );
        }
    }
    for name in CANVASES.iter().filter(|n| **n != "04-font-pick") {
        assert!(
            load(name).placeholder.is_none(),
            "{name}: unexpected placeholder"
        );
    }
}

/// The goldens were dumped from the generate.py in this tree: editing the
/// generator or the palette without re-running dump_goldens.py fails here.
/// Line endings are normalised so a CRLF checkout hashes the same.
#[test]
fn pinned_to_this_generator() {
    let index = index();
    let mockup = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("nimbalyst-local/mockups/pdfpundit-ansi-bbs");
    for (file, want) in [
        ("generate.py", &index.generate_py_sha256),
        ("darkberry-palette.json", &index.palette_sha256),
    ] {
        let bytes = std::fs::read(mockup.join(file)).expect(file);
        let mut lf = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                lf.push(b);
            }
        }
        let got: String = Sha256::digest(&lf)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            &got, want,
            "{file} changed since the goldens were dumped; re-run dump_goldens.py"
        );
    }
    assert_eq!(index.theme, "DarkBerry Blackwater");
}

/// Pose floats such as the computed gaze have 17 significant digits; they must
/// read back bit-exact, so serde_json parses floats with `float_roundtrip`.
#[test]
fn pose_floats_read_back_exactly() {
    for key in pose_keys() {
        let name = cat_name(0.93, &key);
        let text = std::fs::read_to_string(dir().join(format!("{name}.json"))).expect("golden");
        let pose_line = text
            .lines()
            .find(|l| l.starts_with("\"pose\": "))
            .expect("pose line");
        let pose = load(&name).pose.expect("pose");
        for (field, value) in [("gx", pose.gx), ("gy", pose.gy)] {
            let tag = format!("\"{field}\": ");
            let start = pose_line.find(&tag).expect("field") + tag.len();
            let raw: String = pose_line[start..]
                .chars()
                .take_while(|c| !matches!(c, ',' | '}'))
                .collect();
            let exact: f64 = raw.parse().expect("float");
            assert_eq!(value.to_bits(), exact.to_bits(), "{name}: {field} {raw}");
        }
    }
}

/// A canvas written as a self-golden reads back as the same canvas: text,
/// pixel pairs, characters JSON escapes, and the blink set.
#[test]
fn a_self_golden_reads_back_as_its_canvas() {
    use crate::ui::canvas::Canvas;
    use crate::ui::color::Rgb as C;
    use crate::ui::theme::Theme;
    let theme = Theme::default_theme();
    let mut c = Canvas::new(6, 3, theme);
    c.text(0, 0, "\"\\é►", C([1, 2, 3]), Some(C([4, 5, 6])));
    c.pix(1, 3, &[[Some(C([200, 0, 0]))], [Some(C([0, 200, 0]))]]);
    c.set_blink(5, 2);
    let g: Golden = serde_json::from_str(&to_json(&c, "self-test")).expect("parses");
    assert_eq!(g.name, "self-test");
    assert_eq!(g.kind, Kind::Canvas);
    c.assert_matches(&g);
    let mut other = c.clone();
    other.put(0, 0, '\'', None, None);
    assert!(std::panic::catch_unwind(|| other.assert_matches(&g)).is_err());
}

#[test]
fn mismatches_are_listed_by_cell() {
    let g = load("07-widget-idle");
    g.assert_matches(g.w, g.h, |x, y| g.cell(x, y));
    let other = GoldenCell::Text {
        ch: '?',
        fg: Rgb([1, 2, 3]),
        bg: Rgb([4, 5, 6]),
    };
    let bad = g.mismatches(g.w, g.h, |x, y| {
        if (x, y) == (3, 14) {
            other
        } else {
            g.cell(x, y)
        }
    });
    assert_eq!(bad, vec![(3, 14, g.cell(3, 14), other)]);
    assert_eq!(g.mismatches(31, 16, |x, y| g.cell(x, y)).len(), 1);
}

#[test]
#[should_panic(expected = "(3, 14) expected")]
fn assert_matches_prints_the_mismatch_list() {
    let g = load("07-widget-idle");
    let other = GoldenCell::Text {
        ch: '?',
        fg: Rgb([1, 2, 3]),
        bg: Rgb([4, 5, 6]),
    };
    g.assert_matches(g.w, g.h, |x, y| {
        if (x, y) == (3, 14) {
            other
        } else {
            g.cell(x, y)
        }
    });
}
