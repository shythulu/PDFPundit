//! Writes an HTML page of the cat for a human likeness review (G-35; not a gate).
//!
//! Every cat golden in `tests/data/ui` is drawn twice, side by side: the Rust
//! renderer's grid and the grid the mockup generator dumped, with the count of
//! cells that differ. The tests already require 0; the page is for looking at
//! the cat.
//!
//!     cargo run --example cat_dump [OUT.html]
//!
//! The output defaults to `pdfpundit-cat-dump.html` in the system temp
//! directory. The renderer is crate-private, so this example compiles the UI's
//! colour, theme and cat sources into itself by path (each allows its own dead
//! code outside tests).

#[path = "../src/ui/cat.rs"]
mod cat;
#[path = "../src/ui/color.rs"]
mod color;
#[path = "../src/ui/theme.rs"]
mod theme;

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::Value;

use cat::{CellGrid, Pose, render};
use color::Rgb;
use theme::Theme;

/// One cell's two pixels; `None` is transparent.
type Halves = (Option<Rgb>, Option<Rgb>);

fn main() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/ui");
    let read = |name: &str| -> Result<Value, String> {
        let path = root.join(format!("{name}.json"));
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let index = read("index")?;
    let theme = Theme::default_theme();
    let bg = theme.roles.bg;

    let mut body = String::new();
    let mut total_bad = 0usize;
    let frames = index["frames"].as_array().ok_or("index.json: no frames")?;
    for f in frames.iter().filter(|f| f["kind"] == "cat") {
        let name = f["name"]
            .as_str()
            .ok_or("index.json: frame without a name")?;
        let g = read(name)?;
        let scale = g["scale"]
            .as_f64()
            .ok_or_else(|| format!("{name}: no scale"))?;
        let glow = g["glow"]
            .as_f64()
            .ok_or_else(|| format!("{name}: no glow"))?;
        let pose = pose(&g["pose"]).ok_or_else(|| format!("{name}: bad pose"))?;
        let ours = render(&pose, scale, theme, glow);
        let theirs = golden_cells(&g).ok_or_else(|| format!("{name}: bad cells"))?;
        let bad = differing(&ours, &theirs);
        total_bad += bad;
        let _ = writeln!(
            body,
            "<section><h2>{name} <small>scale {scale} · glow {glow} · {bad} cells differ</small></h2>\
             <div class=pair><figure>{}<figcaption>rust</figcaption></figure>\
             <figure>{}<figcaption>generate.py</figcaption></figure></div></section>",
            grid_html(usize::from(ours.cols), &rust_cells(&ours), bg),
            grid_html(usize::from(ours.cols).max(1), &theirs, bg),
        );
    }

    let page = format!(
        "<!doctype html><meta charset=utf-8><title>PDFPundit cat likeness review</title>\
         <style>body{{background:{bg};color:#ddd;font-family:monospace}}\
         pre{{line-height:1;font-size:12px;margin:0}}.pair{{display:flex;gap:2em}}\
         figure{{margin:0}}h2{{font-size:14px}}small{{color:#999}}</style>\
         <h1>Cat likeness review</h1><p>{} frames, {total_bad} differing cells in total.</p>\n{body}",
        frames.iter().filter(|f| f["kind"] == "cat").count()
    );
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("pdfpundit-cat-dump.html"));
    std::fs::write(&out, page).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}

fn pose(v: &Value) -> Option<Pose> {
    let f = |k: &str| v[k].as_f64();
    Some(Pose {
        yaw: f("yaw")?,
        pitch: f("pitch")?,
        eo: f("eo")?,
        mouth: f("mouth")?,
        gx: f("gx")?,
        gy: f("gy")?,
        ears: f("ears")?,
        happy: v["happy"].as_bool()?,
        meme: f("meme")?,
        plate: f("plate"),
        chew: v["chew"].as_bool()?,
        puff: f("puff")?,
    })
}

fn rgb(v: &Value) -> Option<Rgb> {
    v.as_str().and_then(Rgb::parse)
}

/// The golden's cells, row-major: `null` or `["HB", top, bottom]`.
fn golden_cells(g: &Value) -> Option<Vec<Halves>> {
    g["cells"]
        .as_array()?
        .iter()
        .map(|c| match c {
            Value::Null => Some((None, None)),
            Value::Array(a) if a.len() == 3 => Some((rgb(&a[1]), rgb(&a[2]))),
            _ => None,
        })
        .collect()
}

fn rust_cells(grid: &CellGrid) -> Vec<Halves> {
    grid.cells
        .iter()
        .map(|c| c.map_or((None, None), |c| c.halves()))
        .collect()
}

fn differing(ours: &CellGrid, theirs: &[Halves]) -> usize {
    let mine = rust_cells(ours);
    if mine.len() != theirs.len() {
        return mine.len().max(theirs.len());
    }
    mine.iter().zip(theirs).filter(|(a, b)| a != b).count()
}

/// The cells as `▀` spans, transparent halves in the page background.
fn grid_html(cols: usize, cells: &[Halves], bg: Rgb) -> String {
    let mut out = String::from("<pre>");
    for row in cells.chunks(cols) {
        for &(top, bottom) in row {
            let _ = write!(
                out,
                "<span style=\"color:{};background:{}\">▀</span>",
                top.unwrap_or(bg),
                bottom.unwrap_or(bg)
            );
        }
        out.push('\n');
    }
    out.push_str("</pre>");
    out
}
