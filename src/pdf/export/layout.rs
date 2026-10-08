//! Layout analysis for Markdown export (T-32a, D-024, TD §5.6).
//!
//! [`analyse`] turns one page of T-36's glyphs into [`Block`]s in reading
//! order, plus [`LayoutNote`]s for text that is on the page but was not
//! extracted, so T-32b can say so instead of dropping words silently. The
//! code is our own: no other extractor's layout rules are used.
//!
//! Geometry is converted once, at entry, to integer milli-points (pt × 1000,
//! ties to even). Every comparison after that is on `i64`. The conversion is
//! the module's only floating-point code; a test greps for that, and the libm
//! ban in clippy.toml is crate-wide. "milli-em" is a thousandth of the glyph's
//! (or run's) font size.
//!
//! Passes, each a private function with its own tests:
//! 1. [`drop_shadows`]: a glyph repeating an earlier one's text and font within
//!    [`SHADOW_MILLI_EM`] in x and y is dropped (fake bold, drop shadows).
//! 2. [`merge_runs`]: consecutive glyphs of one font, one baseline and one
//!    link join a run; a gap of [`SPACE_GAP_MILLI_EM`] or more inserts one
//!    space; a gap of [`RUN_BREAK_MILLI_EM`] or more ends the run.
//! 3. [`cluster_lines`]: runs whose baselines differ by less than
//!    [`LINE_MILLI_SIZE`] join a line; lines sort by (baseline down, x).
//! 4. [`drop_unprintable`]: glyphs under [`MIN_SIZE_MILLI_PT`], invisible
//!    glyphs (render mode 3) and unmapped glyphs never enter a run. The first
//!    two are counted into the `Unmapped` note when they carry text.
//! 5. [`Cut::split`]: XY-cut. Horizontal gaps of [`BAND_GAP_MILLI_PITCH`] of
//!    the median baseline pitch split the region into bands; failing that, the
//!    widest vertical gap of [`COLUMN_GAP_MILLI_EM`] of the page's median em
//!    splits it into columns. Depth at most [`MAX_CUT_DEPTH`].
//! 6. [`is_rtl`]: reading order is bands top to bottom and columns left to
//!    right; a page whose mapped glyphs are mostly Arabic or Hebrew reads its
//!    columns (and the runs of a line) right to left. Glyphs keep their
//!    content (logical) order inside a run.
//! 7. [`heading_levels`]: headings by size and weight against the page's
//!    modal body size, levels by distinct size, at most three.
//! 8. [`list_marker`]: list items by their leading marker.
//! 9. [`find_tables`]: three or more consecutive lines sharing two or more
//!    whitespace channels become a table.
//! 10. [`link_at`]: a [`UriAnnot`] whose rect holds a glyph's origin links it.
//! 11. Images follow the text, in the order given (see [`analyse`]).
//! 12. [`notes`]: outlined text, Type 3 text, unmapped glyphs and image-only
//!     pages become [`LayoutNote`]s.
//!
//! Thresholds are the `const`s below. Tuning one is a re-baseline of the
//! goldens, not a design change.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::engine::ExtractedImage;
use crate::pdf::model::{Finding, FindingKind, Location};
use crate::pdf::text::{FontKey, FontTable, PageText};

// ── thresholds ──────────────────────────────────────────────────────────

/// Pass 1. A shadow or fake-bold copy sits a few hundredths of an em from the
/// glyph it copies; two real glyphs with the same text never sit that close.
const SHADOW_MILLI_EM: i64 = 50;
/// Pass 2. A quarter em is about a word space in most text faces; tighter
/// gaps are kerning or tracking inside a word.
const SPACE_GAP_MILLI_EM: i64 = 250;
/// Pass 2. A gap of a full em (or a jump back by one) is wider than any word
/// space, so it ends the run: table cells and column gutters stay apart.
const RUN_BREAK_MILLI_EM: i64 = 1000;
/// Pass 2. "One baseline": rounding of the glyph origins aside, a run's glyphs
/// share a baseline; a tenth of an em absorbs it, a superscript does not.
const SAME_BASELINE_MILLI_EM: i64 = 100;
/// Pass 3. Baselines closer than 0.4 of the size are one line (super- and
/// subscripts included); the next line is at least a size away.
const LINE_MILLI_SIZE: i64 = 400;
/// Pass 4. Text under 2 pt is not meant to be read (hidden keywords, hairline
/// artefacts).
const MIN_SIZE_MILLI_PT: i64 = 2000;
/// Pass 5. A column gutter is at least one and a half median ems; word spaces
/// are far narrower.
const COLUMN_GAP_MILLI_EM: i64 = 1500;
/// Pass 5. A band break is an empty strip of at least 0.8 of the median
/// baseline pitch (about a blank line); paragraph spacing alone is less.
const BAND_GAP_MILLI_PITCH: i64 = 800;
/// Pass 5. Page → bands → columns → paragraphs → one more: enough for
/// ordinary layouts, and it bounds the recursion.
const MAX_CUT_DEPTH: u32 = 4;
/// Pass 5. A column holds at least two lines: one wide gap on a single line
/// (a right-aligned page number, a dotted leader) is not a gutter. Bands may
/// be one line: a title spans the columns under it.
const MIN_COLUMN_LINES: usize = 2;
/// Pass 5. Columns are drawn one after the other, so content order switches
/// side once (a few stray runs aside). A table drawn row by row switches on
/// every row and is not cut into columns.
const MAX_COLUMN_SWITCHES: usize = 3;
/// Pass 5. Run boxes: glyphs carry no bounding box (D-038), so a run spans
/// from 0.2 of its size below the baseline to 0.8 above it.
const ASCENT_MILLI: i64 = 800;
const DESCENT_MILLI: i64 = 200;
/// Pass 5. With fewer than two lines there is no pitch; 1.2 ems is the usual
/// leading.
const DEFAULT_PITCH_MILLI_EM: i64 = 1200;
/// Pass 7. A bold line 15% over the body size, or any line 40% over it.
const HEADING_BOLD_MILLI: i64 = 1150;
const HEADING_MILLI: i64 = 1400;
const MAX_HEADING_LEVEL: u8 = 3;
/// Pass 7. `/FontWeight` 600 is semibold, the lightest weight read as bold.
const BOLD_WEIGHT: u32 = 600;
/// Pass 7. Sizes are compared in tenths of a point, so 24 and 23.999 are one
/// size.
const SIZE_BUCKET_MILLI_PT: i64 = 100;
/// Pass 9. Table shape: three rows and two channels (three columns).
const TABLE_MIN_LINES: usize = 3;
const TABLE_MIN_CHANNELS: usize = 2;
/// Pass 9. A cell gap is a run break (see [`RUN_BREAK_MILLI_EM`]).
const TABLE_CELL_GAP_MILLI_EM: i64 = 1000;
/// Pass 9. Rows' gaps line up within half a median em.
const TABLE_ALIGN_MILLI_EM: i64 = 500;
/// A Type 3 glyph has no advance width; half an em is a typical one.
const TYPE3_ADVANCE_MILLI_EM: i64 = 500;
/// A line of three or more of one of these and nothing else is a rule.
const RULE_CHARS: [char; 6] = ['-', '_', '=', '*', '—', '─'];
const RULE_MIN_CHARS: usize = 3;

// ── the interface ───────────────────────────────────────────────────────

/// One page, in reading order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PageLayout {
    pub index: u32,
    pub width_pt: u32,
    pub height_pt: u32,
    pub blocks: Vec<Block>,
    pub notes: Vec<LayoutNote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum Block {
    /// `level` is 1..=3.
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Line>),
    /// Each item is its lines, the marker removed.
    List {
        ordered: bool,
        items: Vec<Vec<Line>>,
    },
    /// Every row has the same number of cells.
    Table {
        rows: Vec<Vec<String>>,
        header: bool,
    },
    Image {
        name: String,
        sha256: [u8; 32],
        len: u64,
    },
    Rule,
}

/// One line's runs, in reading order. Adjacent runs differ in style; a word
/// space between runs is at the end of the earlier run or the start of a
/// plain one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Line(pub Vec<Run>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub link: Option<String>,
}

/// Text on the page that is not in the blocks (TD §5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum LayoutNote {
    OutlinedText { paths: u32, contours: u32 },
    Type3Text,
    ImageOnly,
    Unmapped { glyphs: u32 },
}

/// A link annotation with a URI action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct UriAnnot {
    /// `[x0, y0, x1, y1]` in milli-points, in the glyphs' page space (origin
    /// bottom left, y up); either corner order.
    pub rect: [i32; 4],
    pub uri: String,
}

/// The layout of `page`. `findings` may hold any page's; only those located
/// on `page.index` become notes. `images` are this page's, in paint order:
/// [`ExtractedImage`] carries no position, so they follow the text blocks.
pub(crate) fn analyse(
    page: &PageText,
    fonts: &FontTable,
    findings: &[Finding],
    images: &[ExtractedImage],
    annots: &[UriAnnot],
) -> PageLayout {
    let entry = convert(page);
    let (glyphs, hidden) = drop_unprintable(entry.glyphs);
    let glyphs = drop_shadows(glyphs);
    let rtl = is_rtl(&glyphs);
    let median_em = median(glyphs.iter().map(|g| g.size).collect()).unwrap_or(0);
    let spans = merge_runs(&glyphs, fonts, annots, rtl);
    let leaves = reading_order(&spans, median_em, rtl);
    let mut blocks = classify(&spans, &leaves, annots, median_em, rtl);
    blocks.extend(images.iter().map(|i| Block::Image {
        name: i.name.clone(),
        sha256: i.sha256,
        len: i.len,
    }));
    PageLayout {
        index: page.index,
        width_pt: entry.width_pt,
        height_pt: entry.height_pt,
        blocks,
        notes: notes(page, findings, images, hidden),
    }
}

// ── entry conversion: the module's only floating-point code ─────────────

/// Coordinates beyond ±10⁷ pt are clamped, so no product below overflows.
const COORD_LIMIT: f64 = 1.0e10;
/// Advance widths beyond 10 em are clamped.
const MAX_ADVANCE: f64 = 10_000.0;

struct Entry {
    width_pt: u32,
    height_pt: u32,
    glyphs: Vec<Glyph>,
}

fn convert(page: &PageText) -> Entry {
    let milli = |v: f64| {
        (v * 1000.0)
            .clamp(-COORD_LIMIT, COORD_LIMIT)
            .round_ties_even() as i64
    };
    let glyphs = page
        .glyphs
        .iter()
        .enumerate()
        .map(|(seq, g)| Glyph {
            seq,
            text: g.text.clone(),
            font: g.font,
            x: milli(g.x),
            y: milli(g.y),
            size: milli(g.size).max(0),
            advance: g.advance.map_or(TYPE3_ADVANCE_MILLI_EM, |a| {
                f64::from(a).clamp(0.0, MAX_ADVANCE).round_ties_even() as i64
            }),
            invisible: g.invisible,
        })
        .collect();
    // `as` saturates and maps NaN to 0.
    let whole = |v: f32| f64::from(v).round_ties_even() as u32;
    Entry {
        width_pt: whole(page.width),
        height_pt: whole(page.height),
        glyphs,
    }
}

// ── end of entry conversion ─────────────────────────────────────────────

/// A glyph in milli-points.
#[derive(Debug, Clone)]
struct Glyph {
    /// Index in content order.
    seq: usize,
    text: Option<String>,
    font: FontKey,
    x: i64,
    /// Baseline, y up.
    y: i64,
    size: i64,
    /// Milli-em.
    advance: i64,
    invisible: bool,
}

impl Glyph {
    fn width(&self) -> i64 {
        self.advance * self.size / 1000
    }
}

fn has_ink(text: &str) -> bool {
    text.chars().any(|c| !c.is_whitespace())
}

// ── pass 4: tiny, invisible and unmapped glyphs ─────────────────────────

/// The glyphs that may enter a run, and how many of the rest carry text.
fn drop_unprintable(glyphs: Vec<Glyph>) -> (Vec<Glyph>, u32) {
    let mut hidden = 0u32;
    let mut kept = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        let Some(text) = g.text.as_deref() else {
            continue;
        };
        if g.invisible || g.size < MIN_SIZE_MILLI_PT {
            if has_ink(text) {
                hidden = hidden.saturating_add(1);
            }
            continue;
        }
        kept.push(g);
    }
    (kept, hidden)
}

// ── pass 1: shadow dedup ────────────────────────────────────────────────

/// The `(x, y, size)` of every kept glyph, by text and font.
type Seen = BTreeMap<(String, FontKey), Vec<(i64, i64, i64)>>;

fn drop_shadows(glyphs: Vec<Glyph>) -> Vec<Glyph> {
    // Looked up by key, never iterated.
    let mut seen = Seen::new();
    let near = |d: i64, size: i64| d.abs() * 1000 <= SHADOW_MILLI_EM * size;
    let mut out = Vec::with_capacity(glyphs.len());
    for g in glyphs {
        let key = (g.text.clone().unwrap_or_default(), g.font);
        let earlier = seen.entry(key).or_default();
        if earlier
            .iter()
            .any(|&(x, y, size)| near(g.x - x, size) && near(g.y - y, size))
        {
            continue;
        }
        earlier.push((g.x, g.y, g.size));
        out.push(g);
    }
    out
}

// ── pass 6 (direction) ──────────────────────────────────────────────────

fn is_rtl_char(c: char) -> bool {
    matches!(c,
        '\u{0590}'..='\u{05FF}'     // Hebrew
        | '\u{0600}'..='\u{06FF}'   // Arabic
        | '\u{0750}'..='\u{077F}'   // Arabic Supplement
        | '\u{0870}'..='\u{08FF}'   // Arabic Extended-B and -A
        | '\u{FB1D}'..='\u{FB4F}'   // Hebrew presentation forms
        | '\u{FB50}'..='\u{FDFF}'   // Arabic presentation forms A
        | '\u{FE70}'..='\u{FEFC}') // Arabic presentation forms B
}

/// Whether most mapped, inked glyphs are Arabic or Hebrew.
fn is_rtl(glyphs: &[Glyph]) -> bool {
    let (mut rtl, mut mapped) = (0usize, 0usize);
    for text in glyphs.iter().filter_map(|g| g.text.as_deref()) {
        if has_ink(text) {
            mapped += 1;
            if text.chars().any(is_rtl_char) {
                rtl += 1;
            }
        }
    }
    rtl * 2 > mapped
}

// ── passes 2 and 10: runs and links ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    bold: bool,
    italic: bool,
    mono: bool,
}

fn style_of(fonts: &FontTable, key: FontKey) -> Style {
    let Some(f) = fonts.get(&key) else {
        return Style::default();
    };
    let names = [f.base_font.as_deref(), f.postscript_name.as_deref()];
    let named = |word: &str| {
        names
            .iter()
            .flatten()
            .any(|n| n.to_ascii_lowercase().contains(word))
    };
    Style {
        bold: f.weight.is_some_and(|w| w >= BOLD_WEIGHT) || named("bold"),
        italic: f.italic || named("italic") || named("oblique"),
        mono: f.mono || named("mono") || named("courier"),
    }
}

/// Pass 10: the first annotation whose rect holds the point.
fn link_at(annots: &[UriAnnot], x: i64, y: i64) -> Option<usize> {
    annots.iter().position(|a| {
        let [x0, y0, x1, y1] = a.rect.map(i64::from);
        x0.min(x1) <= x && x <= x0.max(x1) && y0.min(y1) <= y && y <= y0.max(y1)
    })
}

/// A run: glyphs of one font, baseline and link, in content order.
#[derive(Debug, Clone)]
struct Span {
    /// The first glyph's `seq`.
    seq: usize,
    /// Whitespace collapsed to single spaces, none leading; may end in one.
    text: String,
    font: FontKey,
    style: Style,
    /// Index into the annotations.
    link: Option<usize>,
    x0: i64,
    x1: i64,
    y: i64,
    size: i64,
    /// The last glyph's left edge and right edge.
    pen: (i64, i64),
}

impl Span {
    fn top(&self) -> i64 {
        self.y + ASCENT_MILLI * self.size / 1000
    }

    fn bottom(&self) -> i64 {
        self.y - DESCENT_MILLI * self.size / 1000
    }

    fn ink(&self) -> usize {
        self.text.chars().filter(|c| !c.is_whitespace()).count()
    }
}

/// Appends `text` with whitespace collapsed and control characters dropped.
fn push_text(buf: &mut String, text: &str) {
    for c in text.chars() {
        if c.is_whitespace() {
            if !buf.is_empty() && !buf.ends_with(' ') {
                buf.push(' ');
            }
        } else if !c.is_control() {
            buf.push(c);
        }
    }
}

fn merge_runs(glyphs: &[Glyph], fonts: &FontTable, annots: &[UriAnnot], rtl: bool) -> Vec<Span> {
    let mut out = Vec::new();
    let mut cur: Option<Span> = None;
    for g in glyphs {
        let Some(text) = g.text.as_deref() else {
            continue;
        };
        let width = g.width();
        let link = link_at(annots, g.x, g.y);
        if let Some(r) = cur.as_mut()
            && r.font == g.font
            && r.link == link
        {
            let em = r.size.max(g.size);
            let gap = if rtl {
                r.pen.0 - (g.x + width)
            } else {
                g.x - r.pen.1
            };
            let baseline = (r.y - g.y).abs() * 1000 <= SAME_BASELINE_MILLI_EM * em;
            let near =
                gap * 1000 < RUN_BREAK_MILLI_EM * em && gap * 1000 > -RUN_BREAK_MILLI_EM * em;
            if baseline && near {
                if r.text.is_empty() {
                    // Only spaces so far: the box starts at the first ink.
                    (r.x0, r.x1) = (g.x, g.x + width);
                }
                if gap * 1000 >= SPACE_GAP_MILLI_EM * em {
                    push_text(&mut r.text, " ");
                }
                push_text(&mut r.text, text);
                r.x0 = r.x0.min(g.x);
                r.x1 = r.x1.max(g.x + width);
                r.size = em;
                r.pen = (g.x, g.x + width);
                continue;
            }
        }
        out.extend(cur.take());
        let mut buf = String::new();
        push_text(&mut buf, text);
        cur = Some(Span {
            seq: g.seq,
            text: buf,
            font: g.font,
            style: style_of(fonts, g.font),
            link,
            x0: g.x,
            x1: g.x + width,
            y: g.y,
            size: g.size,
            pen: (g.x, g.x + width),
        });
    }
    out.extend(cur);
    // A run of spaces only carries nothing a gap does not.
    out.retain(|s| has_ink(&s.text));
    out
}

// ── pass 3: lines ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct LineG {
    /// Indices into the spans, in reading order.
    runs: Vec<usize>,
    y: i64,
    size: i64,
    x0: i64,
    x1: i64,
}

fn cluster_lines(spans: &[Span], idx: &[usize], rtl: bool) -> Vec<LineG> {
    let mut order = idx.to_vec();
    order.sort_by_key(|&i| (Reverse(spans[i].y), spans[i].x0, spans[i].seq));
    let mut lines: Vec<LineG> = Vec::new();
    for i in order {
        let s = &spans[i];
        if let Some(l) = lines.last_mut()
            && (l.y - s.y).abs() * 1000 < LINE_MILLI_SIZE * l.size.max(s.size)
        {
            l.runs.push(i);
            l.size = l.size.max(s.size);
            l.x0 = l.x0.min(s.x0);
            l.x1 = l.x1.max(s.x1);
            continue;
        }
        lines.push(LineG {
            runs: vec![i],
            y: s.y,
            size: s.size,
            x0: s.x0,
            x1: s.x1,
        });
    }
    for l in &mut lines {
        if rtl {
            l.runs
                .sort_by_key(|&i| (Reverse(spans[i].x1), spans[i].seq));
        } else {
            l.runs.sort_by_key(|&i| (spans[i].x0, spans[i].seq));
        }
    }
    lines.sort_by_key(|l| (Reverse(l.y), l.x0));
    lines
}

/// The lower middle value.
fn median(mut v: Vec<i64>) -> Option<i64> {
    v.sort_unstable();
    v.get(v.len().checked_sub(1)? / 2).copied()
}

// ── passes 5 and 6: XY-cut and reading order ────────────────────────────

/// Every leaf region's lines, leaves in reading order.
fn reading_order(spans: &[Span], median_em: i64, rtl: bool) -> Vec<Vec<LineG>> {
    if spans.is_empty() {
        return Vec::new();
    }
    let all: Vec<usize> = (0..spans.len()).collect();
    let lines = cluster_lines(spans, &all, rtl);
    let pitch = median(
        lines
            .windows(2)
            .map(|w| w[0].y - w[1].y)
            .filter(|&d| d > 0)
            .collect(),
    )
    .unwrap_or(DEFAULT_PITCH_MILLI_EM * median_em / 1000);
    let cut = Cut {
        spans,
        rtl,
        column_gap: (COLUMN_GAP_MILLI_EM * median_em / 1000).max(1),
        band_gap: (BAND_GAP_MILLI_PITCH * pitch / 1000).max(1),
    };
    let mut leaves = Vec::new();
    cut.split(all, 0, &mut leaves);
    leaves
        .iter()
        .map(|leaf| cluster_lines(spans, leaf, rtl))
        .collect()
}

struct Cut<'a> {
    spans: &'a [Span],
    rtl: bool,
    /// Milli-points.
    column_gap: i64,
    band_gap: i64,
}

impl Cut<'_> {
    fn split(&self, idx: Vec<usize>, depth: u32, out: &mut Vec<Vec<usize>>) {
        if depth >= MAX_CUT_DEPTH || idx.len() < 2 {
            out.push(idx);
            return;
        }
        let bands = self.bands(&idx);
        if bands.len() > 1 {
            for band in bands {
                self.split(band, depth + 1, out);
            }
            return;
        }
        match self.columns(&idx) {
            Some((left, right)) => {
                let (first, second) = if self.rtl {
                    (right, left)
                } else {
                    (left, right)
                };
                self.split(first, depth + 1, out);
                self.split(second, depth + 1, out);
            }
            None => out.push(idx),
        }
    }

    /// The region cut at every empty horizontal strip of `band_gap` or more,
    /// top first.
    fn bands(&self, idx: &[usize]) -> Vec<Vec<usize>> {
        let s = self.spans;
        let mut order = idx.to_vec();
        order.sort_by_key(|&i| (Reverse(s[i].top()), s[i].x0, s[i].seq));
        let mut out: Vec<Vec<usize>> = Vec::new();
        let mut floor = 0;
        for i in order {
            if let Some(band) = out.last_mut()
                && floor - s[i].top() < self.band_gap
            {
                band.push(i);
                floor = floor.min(s[i].bottom());
                continue;
            }
            out.push(vec![i]);
            floor = s[i].bottom();
        }
        out
    }

    /// The region cut at its widest acceptable vertical gap of `column_gap`
    /// or more: both sides hold two lines and were drawn one after the other.
    fn columns(&self, idx: &[usize]) -> Option<(Vec<usize>, Vec<usize>)> {
        let s = self.spans;
        let mut order = idx.to_vec();
        order.sort_by_key(|&i| (s[i].x0, s[i].seq));
        let mut gaps = Vec::new();
        let mut reach = s[*order.first()?].x1;
        for &i in &order[1..] {
            if s[i].x0 - reach >= self.column_gap {
                gaps.push((s[i].x0 - reach, s[i].x0));
            }
            reach = reach.max(s[i].x1);
        }
        gaps.sort_by_key(|&(width, at)| (Reverse(width), at));
        gaps.into_iter().find_map(|(_, at)| {
            let (left, right): (Vec<usize>, Vec<usize>) = idx.iter().partition(|&&i| s[i].x0 < at);
            let lines = |side: &[usize]| cluster_lines(s, side, self.rtl).len();
            (lines(&left) >= MIN_COLUMN_LINES
                && lines(&right) >= MIN_COLUMN_LINES
                && self.switches(&left, &right) <= MAX_COLUMN_SWITCHES)
                .then_some((left, right))
        })
    }

    /// How often content order crosses between the two sides.
    fn switches(&self, left: &[usize], right: &[usize]) -> usize {
        let mut sides: Vec<(usize, bool)> = left
            .iter()
            .map(|&i| (self.spans[i].seq, false))
            .chain(right.iter().map(|&i| (self.spans[i].seq, true)))
            .collect();
        sides.sort_unstable();
        sides.windows(2).filter(|w| w[0].1 != w[1].1).count()
    }
}

// ── passes 7–9: blocks ──────────────────────────────────────────────────

fn bucket(size: i64) -> i64 {
    (size + SIZE_BUCKET_MILLI_PT / 2) / SIZE_BUCKET_MILLI_PT * SIZE_BUCKET_MILLI_PT
}

/// The size most of a line's ink is set in (ties: the larger).
fn line_size(spans: &[Span], line: &LineG) -> i64 {
    let mut ink: BTreeMap<i64, usize> = BTreeMap::new();
    for &i in &line.runs {
        *ink.entry(bucket(spans[i].size)).or_default() += spans[i].ink();
    }
    let mut best = (0, 0);
    for (size, n) in ink {
        if n >= best.1 {
            best = (size, n);
        }
    }
    best.0
}

/// The size most of the page's ink is set in (ties: the smaller).
fn body_size(spans: &[Span]) -> i64 {
    let mut ink: BTreeMap<i64, usize> = BTreeMap::new();
    for s in spans {
        *ink.entry(bucket(s.size)).or_default() += s.ink();
    }
    let mut best = (0, 0);
    for (size, n) in ink {
        if n > best.1 {
            best = (size, n);
        }
    }
    best.0
}

fn line_bold(spans: &[Span], line: &LineG) -> bool {
    line.runs.iter().all(|&i| spans[i].style.bold)
}

fn is_heading(size: i64, bold: bool, body: i64) -> bool {
    body > 0
        && (size * 1000 >= HEADING_MILLI * body
            || (bold && size * 1000 >= HEADING_BOLD_MILLI * body))
}

/// Pass 7: the level of each distinct heading size, largest first.
fn heading_levels(sizes: BTreeSet<i64>) -> BTreeMap<i64, u8> {
    let mut levels = BTreeMap::new();
    let mut level = 1;
    for size in sizes.into_iter().rev() {
        levels.insert(size, level);
        level = (level + 1).min(MAX_HEADING_LEVEL);
    }
    levels
}

/// Pass 8: the marker's length in chars (with its space) and whether it
/// numbers the item. Markers: `•`, `◦`, `-`, `–`, `*`, `N.`, `N)`, `a)`,
/// `(a)`, each followed by a space and text.
fn list_marker(text: &str) -> Option<(usize, bool)> {
    let c: Vec<char> = text.chars().take(12).collect();
    let spaced = |n: usize| c.get(n) == Some(&' ') && c.len() > n + 1;
    if matches!(c.first(), Some('•' | '◦' | '-' | '–' | '*')) && spaced(1) {
        return Some((2, false));
    }
    let digits = c.iter().take_while(|d| d.is_ascii_digit()).count();
    if (1..=9).contains(&digits) && matches!(c.get(digits), Some('.' | ')')) && spaced(digits + 1) {
        return Some((digits + 2, true));
    }
    if c.first().is_some_and(char::is_ascii_alphabetic) && c.get(1) == Some(&')') && spaced(2) {
        return Some((3, true));
    }
    if c.first() == Some(&'(')
        && c.get(1).is_some_and(char::is_ascii_alphabetic)
        && c.get(2) == Some(&')')
        && spaced(3)
    {
        return Some((4, true));
    }
    None
}

fn is_rule(text: &str) -> bool {
    let mut chars = text.chars().filter(|c| *c != ' ');
    let Some(first) = chars.next() else {
        return false;
    };
    RULE_CHARS.contains(&first)
        && text.chars().filter(|c| *c != ' ').count() >= RULE_MIN_CHARS
        && chars.all(|c| c == first)
}

/// A table found by pass 9: lines `start..end` of a leaf, cells split at
/// `cuts` (milli-points, ascending).
#[derive(Debug, Clone, PartialEq, Eq)]
struct TableSpan {
    start: usize,
    end: usize,
    cuts: Vec<i64>,
}

/// A line's cell gaps, `(left edge, right edge)`, left to right.
fn cell_gaps(spans: &[Span], line: &LineG) -> Vec<(i64, i64)> {
    let mut runs = line.runs.clone();
    runs.sort_by_key(|&i| (spans[i].x0, spans[i].seq));
    runs.windows(2)
        .filter_map(|w| {
            let (p, s) = (&spans[w[0]], &spans[w[1]]);
            let gap = s.x0 - p.x1;
            (gap * 1000 >= TABLE_CELL_GAP_MILLI_EM * p.size.max(s.size)).then_some((p.x1, s.x0))
        })
        .collect()
}

/// Pass 9. A channel is the strip every row so far leaves empty; a row
/// continues it when one of the row's gaps overlaps it within `tol`.
fn find_tables(spans: &[Span], lines: &[LineG], median_em: i64) -> Vec<TableSpan> {
    let tol = TABLE_ALIGN_MILLI_EM * median_em / 1000;
    let gaps: Vec<Vec<(i64, i64)>> = lines.iter().map(|l| cell_gaps(spans, l)).collect();
    let narrow = |channels: &[(i64, i64)], row: &[(i64, i64)]| {
        let mut out: Vec<(i64, i64)> = channels
            .iter()
            .filter_map(|&(a, b)| {
                row.iter()
                    .find(|&&(c, d)| a.max(c) <= b.min(d) + tol)
                    .map(|&(c, d)| (a.max(c), b.min(d)))
            })
            .collect();
        out.dedup();
        out
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let mut channels = gaps[i].clone();
        let mut j = i + 1;
        while j < lines.len() {
            let next = narrow(&channels, &gaps[j]);
            if next.len() < TABLE_MIN_CHANNELS {
                break;
            }
            channels = next;
            j += 1;
        }
        if j - i >= TABLE_MIN_LINES && channels.len() >= TABLE_MIN_CHANNELS {
            let cuts = channels.iter().map(|&(a, b)| a + (b - a) / 2).collect();
            out.push(TableSpan {
                start: i,
                end: j,
                cuts,
            });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// One row's cell texts, in reading order.
fn table_row(spans: &[Span], line: &LineG, cuts: &[i64], rtl: bool) -> Vec<String> {
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); cuts.len() + 1];
    for &i in &line.runs {
        cells[cuts.partition_point(|&m| m <= spans[i].x0)].push(i);
    }
    let mut row: Vec<String> = cells
        .iter()
        .map(|cell| {
            let mut text = String::new();
            for (k, &i) in cell.iter().enumerate() {
                if k > 0 && wide_gap(&spans[cell[k - 1]], &spans[i], rtl) {
                    push_text(&mut text, " ");
                }
                push_text(&mut text, &spans[i].text);
            }
            text.trim_end().to_owned()
        })
        .collect();
    if rtl {
        row.reverse();
    }
    row
}

/// Whether the gap between consecutive runs `p` then `s` of a line is a word
/// space.
fn wide_gap(p: &Span, s: &Span, rtl: bool) -> bool {
    let gap = if rtl { p.x0 - s.x1 } else { s.x0 - p.x1 };
    gap * 1000 >= SPACE_GAP_MILLI_EM * p.size.max(s.size)
}

fn plain_style(r: &Run) -> bool {
    !r.bold && !r.italic && !r.mono && r.link.is_none()
}

fn same_style(a: &Run, b: &Run) -> bool {
    (a.bold, a.italic, a.mono, &a.link) == (b.bold, b.italic, b.mono, &b.link)
}

/// Appends `run`, after a word space if `space`, merging it into the last run
/// when the styles match. A space between differently styled runs goes into
/// whichever is plain, else into a plain run of its own.
fn push_run(out: &mut Vec<Run>, mut run: Run, space: bool) {
    if let Some(last) = out.last_mut() {
        if same_style(last, &run) {
            if space {
                last.text.push(' ');
            }
            last.text.push_str(&run.text);
            return;
        }
        if space {
            if plain_style(last) {
                last.text.push(' ');
            } else if plain_style(&run) {
                run.text.insert(0, ' ');
            } else {
                out.push(Run {
                    text: " ".to_owned(),
                    bold: false,
                    italic: false,
                    mono: false,
                    link: None,
                });
            }
        }
    }
    out.push(run);
}

fn out_line(spans: &[Span], line: &LineG, annots: &[UriAnnot], rtl: bool) -> Line {
    let mut out = Vec::new();
    let mut prev: Option<&Span> = None;
    for &i in &line.runs {
        let s = &spans[i];
        let space = prev.is_some_and(|p| p.text.ends_with(' ') || wide_gap(p, s, rtl));
        prev = Some(s);
        let run = Run {
            text: s.text.trim_end().to_owned(),
            bold: s.style.bold,
            italic: s.style.italic,
            mono: s.style.mono,
            link: s.link.map(|k| annots[k].uri.clone()),
        };
        push_run(&mut out, run, space);
    }
    Line(out)
}

fn line_text(line: &Line) -> String {
    line.0.iter().map(|r| r.text.as_str()).collect()
}

/// `line` without its first `n` chars and the spaces after them.
fn strip_marker(line: Line, n: usize) -> Line {
    let mut left = n;
    let mut out = Vec::new();
    for mut run in line.0 {
        if left > 0 || out.is_empty() {
            let len = run.text.chars().count();
            let skip = left.min(len);
            left -= skip;
            run.text = run.text.chars().skip(skip).collect::<String>();
            if left == 0 && out.is_empty() {
                run.text = run.text.trim_start().to_owned();
            }
        }
        if !run.text.is_empty() {
            out.push(run);
        }
    }
    Line(out)
}

/// The list being built: its items and the marker line's leading edge.
struct ListAcc {
    ordered: bool,
    items: Vec<Vec<Line>>,
    indent: i64,
    size: i64,
}

/// Blocks from one leaf's lines, appended to `out`.
struct Emitter<'a> {
    spans: &'a [Span],
    annots: &'a [UriAnnot],
    rtl: bool,
    body: i64,
    levels: &'a BTreeMap<i64, u8>,
    headings: bool,
}

impl Emitter<'_> {
    /// How far in a line starts, in reading direction.
    fn indent(&self, line: &LineG) -> i64 {
        if self.rtl { -line.x1 } else { line.x0 }
    }

    fn heading_level(&self, line: &LineG) -> Option<u8> {
        let size = line_size(self.spans, line);
        (self.headings && is_heading(size, line_bold(self.spans, line), self.body))
            .then(|| self.levels.get(&size).copied())
            .flatten()
    }

    fn leaf(&self, lines: &[LineG], tables: &[TableSpan], out: &mut Vec<Block>) {
        let mut para: Vec<Line> = Vec::new();
        let mut list: Option<ListAcc> = None;
        let mut last_heading: Option<u8> = None;
        let flush = |para: &mut Vec<Line>, list: &mut Option<ListAcc>, out: &mut Vec<Block>| {
            if !para.is_empty() {
                out.push(Block::Paragraph(std::mem::take(para)));
            }
            if let Some(l) = list.take() {
                out.push(Block::List {
                    ordered: l.ordered,
                    items: l.items,
                });
            }
        };
        let mut k = 0;
        while k < lines.len() {
            if let Some(t) = tables.iter().find(|t| t.start == k) {
                flush(&mut para, &mut list, out);
                last_heading = None;
                let rows: Vec<Vec<String>> = lines[t.start..t.end]
                    .iter()
                    .map(|l| table_row(self.spans, l, &t.cuts, self.rtl))
                    .collect();
                let header = line_bold(self.spans, &lines[t.start]);
                out.push(Block::Table { rows, header });
                k = t.end;
                continue;
            }
            let g = &lines[k];
            k += 1;
            let line = out_line(self.spans, g, self.annots, self.rtl);
            let text = line_text(&line);
            if is_rule(&text) {
                flush(&mut para, &mut list, out);
                last_heading = None;
                out.push(Block::Rule);
            } else if let Some(level) = self.heading_level(g) {
                flush(&mut para, &mut list, out);
                if last_heading == Some(level)
                    && let Some(Block::Heading { runs, .. }) = out.last_mut()
                {
                    for (n, run) in line.0.into_iter().enumerate() {
                        push_run(runs, run, n == 0);
                    }
                } else {
                    out.push(Block::Heading {
                        level,
                        runs: line.0,
                    });
                }
                last_heading = Some(level);
            } else if let Some((n, ordered)) = list_marker(&text) {
                last_heading = None;
                if !para.is_empty() || list.as_ref().is_some_and(|l| l.ordered != ordered) {
                    flush(&mut para, &mut list, out);
                }
                let acc = list.get_or_insert_with(|| ListAcc {
                    ordered,
                    items: Vec::new(),
                    indent: self.indent(g),
                    size: g.size,
                });
                acc.indent = self.indent(g);
                acc.size = g.size;
                acc.items.push(vec![strip_marker(line, n)]);
            } else if let Some(acc) = list.as_mut()
                && (self.indent(g) - acc.indent) * 1000 > SPACE_GAP_MILLI_EM * acc.size
                && let Some(item) = acc.items.last_mut()
            {
                item.push(line);
            } else {
                last_heading = None;
                if list.is_some() {
                    flush(&mut para, &mut list, out);
                }
                para.push(line);
            }
        }
        flush(&mut para, &mut list, out);
    }
}

/// Passes 7–9 over every leaf, in order.
fn classify(
    spans: &[Span],
    leaves: &[Vec<LineG>],
    annots: &[UriAnnot],
    median_em: i64,
    rtl: bool,
) -> Vec<Block> {
    let tables: Vec<Vec<TableSpan>> = leaves
        .iter()
        .map(|lines| find_tables(spans, lines, median_em))
        .collect();
    let body = body_size(spans);
    let total: usize = leaves.iter().map(Vec::len).sum();
    let mut sizes = BTreeSet::new();
    for (lines, tables) in leaves.iter().zip(&tables) {
        for (k, line) in lines.iter().enumerate() {
            let in_table = tables.iter().any(|t| (t.start..t.end).contains(&k));
            let size = line_size(spans, line);
            if !in_table && is_heading(size, line_bold(spans, line), body) {
                sizes.insert(size);
            }
        }
    }
    let levels = heading_levels(sizes);
    let emitter = Emitter {
        spans,
        annots,
        rtl,
        body,
        levels: &levels,
        // A single-line page is never a heading.
        headings: total > 1,
    };
    let mut out = Vec::new();
    for (lines, tables) in leaves.iter().zip(&tables) {
        emitter.leaf(lines, tables, &mut out);
    }
    out
}

// ── pass 12: notes ──────────────────────────────────────────────────────

/// `hidden`: tiny or invisible glyphs that carry text (pass 4).
fn notes(
    page: &PageText,
    findings: &[Finding],
    images: &[ExtractedImage],
    hidden: u32,
) -> Vec<LayoutNote> {
    let mut outlined: Option<(u32, u32)> = None;
    let mut type3 = false;
    for f in findings {
        if !matches!(f.location, Location::Page { index, .. } if index == page.index) {
            continue;
        }
        match f.class {
            FindingKind::OutlinedText {
                glyph_runs,
                contours,
            } => {
                let (p, c) = outlined.unwrap_or_default();
                outlined = Some((p.saturating_add(glyph_runs), c.saturating_add(contours)));
            }
            FindingKind::Type3Text { .. } => type3 = true,
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some((paths, contours)) = outlined {
        out.push(LayoutNote::OutlinedText { paths, contours });
    }
    if type3 {
        out.push(LayoutNote::Type3Text);
    }
    if page.glyphs.is_empty() && (!images.is_empty() || page.paint.images > 0) {
        out.push(LayoutNote::ImageOnly);
    }
    let unmapped = page.unmapped.saturating_add(hidden);
    if unmapped > 0 {
        out.push(LayoutNote::Unmapped { glyphs: unmapped });
    }
    out
}
