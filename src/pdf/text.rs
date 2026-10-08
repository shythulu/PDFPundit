//! hayro-interpret `Device` adapter: glyphs in content order and per-page paint
//! counts (T-36). `verify`, `bench` and `export` read text through this module.
//!
//! One `interpret_page` pass per page feeds [`Collector`], which records every
//! glyph of every `draw_glyph_run` call (with `/ToUnicode` applied by hayro)
//! and tallies [`PaintCounts`] from `draw_path`, `draw_rect`, `draw_image`,
//! `draw_glyph_run` and `push_clip_path` (D-059: the blank-page measure is
//! these counts, never pixels). Nothing here classifies colour: deciding that a
//! paint is white goes through colour conversion that calls libm (FR-g1 §3), so
//! a white fill, a white image or geometry inside an empty clip count as paint.
//!
//! Determinism (FR-g1, goal-r2-fr2): the counts are integer tallies of calls;
//! the glyph geometry is hayro's own affine products plus one subtraction and
//! one `sqrt` of a dot product per glyph. No `hypot`, `powf` or trig.
//!
//! Known limits (D-038). Two depend on the file and are recorded in
//! [`PageText::warnings`] where they apply: glyphs in hidden optional-content
//! groups are never produced, and on a file whose page tree is unreadable hayro
//! finds pages by scanning and orders them by file offset. The rest hold for
//! every file, so they are not repeated per page but kept in [`KNOWN_LIMITS`]
//! for a report to quote once: `ActualText` is not surfaced, glyphs carry no
//! bounding box, text in render mode 7 (clip only) reaches the `Device` as a
//! clip and never as glyphs, and hayro gives no warning for a dropped Flate
//! stream or an unresolved font, so [`PageText::unmapped`] and the glyph count
//! per page are the integrity signals.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, PoisonError};

use hayro::kurbo::{Affine, BezPath, Rect};
use hayro_interpret::font::{Glyph, GlyphRun};
use hayro_interpret::hayro_cmap::BfString;
use hayro_interpret::{
    BlendMode, CacheKey, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps,
    InterpreterCache, InterpreterSettings, InterpreterWarning, SoftMask, TransformExt,
    interpret_page,
};
use hayro_syntax::object::dict::keys::{
    BASE_FONT, DESCENDANT_FONTS, FLAGS, FONT, FONT_DESC, FONT_NAME, FONT_WEIGHT, KIDS,
    OCPROPERTIES, PAGES, RESOURCES, SUBTYPE, XOBJECT,
};
use hayro_syntax::object::{Array, Dict, Name, Object, Stream};
use hayro_syntax::{LoadPdfError, Pdf};

use crate::pdf::model::PaintCounts;

/// What [`extract_text`] interprets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExtractOptions {
    /// Draw annotation appearance streams (hayro's default is `true`; ours is
    /// `false`: annotation text is not page text).
    pub render_annotations: bool,
    /// Keep the glyphs of render-mode-3 runs (OCR layers). Their runs are
    /// counted in [`PaintCounts::glyph_runs_invisible`] either way.
    pub include_invisible: bool,
}

impl Default for ExtractOptions {
    fn default() -> ExtractOptions {
        ExtractOptions {
            render_annotations: false,
            include_invisible: true,
        }
    }
}

/// Why nothing was extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ExtractError {
    /// hayro could not load the file at all (no catalog or no page found).
    #[error("the PDF did not load: {0:?}")]
    Load(LoadPdfError),
    /// hayro panicked; the panic was caught here.
    #[error("the interpreter panicked")]
    Panicked,
}

/// One page's glyphs and paint counts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageText {
    /// Position in hayro's page list (0-based).
    pub index: u32,
    /// Displayed size in points (crop box, rotation applied).
    pub width: f32,
    pub height: f32,
    /// Every glyph, in content-stream order.
    pub glyphs: Vec<GlyphItem>,
    /// Glyphs in `glyphs` whose `text` is `None`.
    pub unmapped: u32,
    pub paint: PaintCounts,
    /// Fixed sentences, in a fixed order: the D-038 limits that apply to this
    /// file, then what hayro's warning sink reported for this page. The limits
    /// that hold for every file are in [`KNOWN_LIMITS`], not here. An empty
    /// list is not a health signal: under this adapter and the default options
    /// the sink almost never fires (see `WarningTally`).
    pub warnings: Vec<String>,
}

/// A font, as hayro keys it: `Dict::cache_key()` of the font dictionary (a
/// fixed-key hash of its bytes, so the same on every run and platform).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FontKey(pub u128);

impl FontKey {
    /// The key of a glyph with no font dictionary behind it: hayro's fallback
    /// Helvetica (the font resource is missing or broken, as in C6) or a
    /// Type3 font (hayro exposes no key for one). [`GlyphItem::type3`] tells
    /// the two apart. Never in a [`FontTable`].
    pub const UNKEYED: FontKey = FontKey(0);
}

/// One glyph as drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphItem {
    /// The Unicode text hayro maps the glyph to (`/ToUnicode`, then the glyph
    /// name for simple fonts); `None` when nothing maps it.
    pub text: Option<String>,
    /// The glyph origin in points from the bottom-left corner of the page as
    /// displayed (crop box and `/Rotate` applied), y up.
    pub x: f64,
    pub y: f64,
    /// The em height in points: the length of the glyph's em vector along y.
    pub size: f64,
    /// The font's advance width in 1/1000 em (`None` for Type3 glyphs).
    pub advance: Option<f32>,
    /// The font. [`FontKey::UNKEYED`] with `type3` false means no font
    /// dictionary resolved and hayro drew with its fallback Helvetica (an
    /// integrity signal: the page lost its font); with `type3` true it is a
    /// Type3 font. Look it up with `FontTable::get`: an unkeyed glyph, or a
    /// font met only in an annotation appearance, has no entry.
    pub font: FontKey,
    /// Drawn in render mode 3.
    pub invisible: bool,
    pub type3: bool,
    /// The innermost marked-content id around the glyph.
    pub mcid: Option<i32>,
}

/// What the font dictionary and its descriptor say about one font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontInfo {
    pub base_font: Option<String>,
    pub subtype: String,
    /// The descriptor's `/Flags` (a Type0 font's from its first descendant).
    pub flags: Option<u32>,
    /// `/FontName` of the descriptor, else `/BaseFont`, subset tag removed.
    pub postscript_name: Option<String>,
    /// The descriptor's `/FontWeight`.
    pub weight: Option<u32>,
    /// `/Flags` bit 7 (Italic).
    pub italic: bool,
    /// `/Flags` bit 2 (Serif).
    pub serif: bool,
    /// `/Flags` bit 1 (FixedPitch).
    pub mono: bool,
}

/// Every font dictionary among the document's objects, plus those written
/// inline in a page's or a Form XObject's `/Resources /Font`. A glyph's font can
/// still be missing (see [`GlyphItem::font`]), so index it with `get`.
pub(crate) type FontTable = BTreeMap<FontKey, FontInfo>;

/// Glyphs and paint counts for every page hayro finds in `bytes`. The whole
/// call runs under `catch_unwind`.
pub(crate) fn extract_text(
    bytes: &[u8],
    opts: &ExtractOptions,
) -> Result<Vec<PageText>, ExtractError> {
    catch_unwind(AssertUnwindSafe(|| extract_pages(bytes, opts)))
        .unwrap_or(Err(ExtractError::Panicked))
}

/// The [`FontTable`] of `bytes`, built once per document from its objects.
pub(crate) fn font_table(bytes: &[u8]) -> Result<FontTable, ExtractError> {
    catch_unwind(AssertUnwindSafe(|| {
        let pdf = Pdf::new(bytes.to_vec()).map_err(ExtractError::Load)?;
        Ok(build_font_table(&pdf))
    }))
    .unwrap_or(Err(ExtractError::Panicked))
}

/// The D-038 limits that hold for every file, as fixed sentences in a fixed
/// order, for a report to quote once per document.
pub(crate) const KNOWN_LIMITS: [&str; 4] = [
    "text marked with ActualText is extracted as drawn; the ActualText is not used",
    "glyphs carry an origin and a size but no bounding box",
    "text drawn in render mode 7 (clip only) is not extracted",
    "the interpreter gives no warning for a dropped compressed stream or an unresolved font; \
     unmapped glyphs and the glyph count per page are the integrity signals",
];

const HIDDEN_LAYERS: &str =
    "the document has optional content: text in hidden layers is not extracted";
const SCANNED_PAGES: &str =
    "the page tree is unreadable: pages were found by scanning and are in file order";

fn extract_pages(bytes: &[u8], opts: &ExtractOptions) -> Result<Vec<PageText>, ExtractError> {
    let pdf = Pdf::new(bytes.to_vec()).map_err(ExtractError::Load)?;
    let mut doc_warnings = Vec::new();
    if has_optional_content(&pdf) {
        doc_warnings.push(HIDDEN_LAYERS.to_owned());
    }
    if !page_tree_readable(&pdf) {
        doc_warnings.push(SCANNED_PAGES.to_owned());
    }

    let cache = InterpreterCache::new();
    let mut out = Vec::new();
    for (index, page) in pdf.pages().iter().enumerate() {
        let (width, height) = page.render_dimensions();
        let reported = Arc::new(Mutex::new(WarningTally::default()));
        let sink = Arc::clone(&reported);
        let settings = InterpreterSettings {
            render_annotations: opts.render_annotations,
            warning_sink: Arc::new(move |w| {
                sink.lock().unwrap_or_else(PoisonError::into_inner).add(w);
            }),
            ..InterpreterSettings::default()
        };
        let mut ctx = Context::new(
            page.initial_transform(true).to_kurbo(),
            Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
            &cache,
            pdf.xref(),
            settings,
        );
        let mut dev = Collector::new(f64::from(height), opts.include_invisible);
        interpret_page(page, &mut ctx, &mut dev);

        let mut warnings = doc_warnings.clone();
        reported
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write(&mut warnings);
        out.push(PageText {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            width,
            height,
            glyphs: dev.glyphs,
            unmapped: dev.unmapped,
            paint: dev.paint,
            warnings,
        });
    }
    Ok(out)
}

/// Whether the catalog has `/OCProperties`, the test hayro's optional-content
/// state is built from. `XRef::has_optional_content_groups` is not used: it is
/// set only when the trailer is read, so a file whose catalog was found by
/// scanning reports `false` while its hidden layers are still suppressed.
fn has_optional_content(pdf: &Pdf) -> bool {
    let xref = pdf.xref();
    xref.get::<Dict<'_>>(xref.root_id())
        .is_some_and(|root| root.get::<Dict<'_>>(OCPROPERTIES).is_some())
}

/// hayro walks `/Root /Pages /Kids` when it can and scans for page
/// dictionaries when it cannot; this is the same test from outside.
fn page_tree_readable(pdf: &Pdf) -> bool {
    let xref = pdf.xref();
    xref.get::<Dict<'_>>(xref.root_id())
        .and_then(|root| root.get::<Dict<'_>>(PAGES))
        .is_some_and(|pages| pages.get::<Array<'_>>(KIDS).is_some())
}

/// What hayro's warning sink reported for one page. Under this adapter and
/// hayro-interpret 0.8.0 little of it can fire: `UnsupportedFont` is never
/// emitted, `ImageDecodeFailure` only when a device decodes an image (the
/// `Collector` never does), and `UnresolvedAnnotationAppearance` only with
/// `render_annotations` on (off by default). It is kept for later versions;
/// an empty tally says nothing about the file's health.
#[derive(Debug, Default)]
struct WarningTally {
    unsupported_font: u32,
    image_decode: u32,
    annotation: u32,
    other: u32,
}

impl WarningTally {
    fn add(&mut self, w: InterpreterWarning) {
        let n = match w {
            InterpreterWarning::UnsupportedFont => &mut self.unsupported_font,
            InterpreterWarning::ImageDecodeFailure => &mut self.image_decode,
            InterpreterWarning::UnresolvedAnnotationAppearance => &mut self.annotation,
            _ => &mut self.other,
        };
        *n = n.saturating_add(1);
    }

    fn write(&self, out: &mut Vec<String>) {
        let lines = [
            (
                self.unsupported_font,
                "a CID font with a non-identity encoding is not supported; its text is missing",
            ),
            (self.image_decode, "an image failed to decode"),
            (
                self.annotation,
                "an annotation's appearance could not be loaded and was not drawn",
            ),
            (self.other, "the interpreter reported a problem"),
        ];
        for (n, line) in lines {
            if n > 0 {
                out.push(format!("{line} (×{n})"));
            }
        }
    }
}

/// The `Device`: records glyphs and tallies paint calls for one page.
struct Collector {
    /// The page height, to turn hayro's y-down space into y-up.
    height: f64,
    include_invisible: bool,
    glyphs: Vec<GlyphItem>,
    unmapped: u32,
    paint: PaintCounts,
    /// The marked-content stack: each entry's MCID, if it has one.
    marked: Vec<Option<i32>>,
    /// Page-space transforms of the glyphs of the last call if it was a fill
    /// run. Render modes 2 and 6 draw one run twice (fill, then stroke); the
    /// stroke half is recognised by equal transforms and not recorded again.
    last_fill: Vec<Affine>,
}

impl Collector {
    fn new(height: f64, include_invisible: bool) -> Collector {
        Collector {
            height,
            include_invisible,
            glyphs: Vec::new(),
            unmapped: 0,
            paint: PaintCounts::default(),
            marked: Vec::new(),
            last_fill: Vec::new(),
        }
    }

    fn count_path(&mut self, mode: &DrawMode) {
        self.last_fill.clear();
        let c = &mut self.paint;
        match mode {
            DrawMode::Fill(_) => c.path_fills = c.path_fills.saturating_add(1),
            DrawMode::Stroke(_) => c.path_strokes = c.path_strokes.saturating_add(1),
            DrawMode::FillAndStroke(..) => {
                c.path_fills = c.path_fills.saturating_add(1);
                c.path_strokes = c.path_strokes.saturating_add(1);
            }
            DrawMode::Invisible => {}
        }
    }

    fn is_stroke_of_last_fill(&self, run: &GlyphRun<'_, '_>, page: Affine) -> bool {
        let glyphs = run.glyphs();
        glyphs.len() == self.last_fill.len()
            && glyphs
                .iter()
                .zip(&self.last_fill)
                .all(|(g, &t)| page * g.transform() == t)
    }
}

fn unicode(g: &Glyph<'_>) -> Option<String> {
    g.as_unicode().map(|b| match b {
        BfString::Char(c) => c.to_string(),
        BfString::String(s) => s,
    })
}

/// A glyph inks the page unless it maps to whitespace only (goal-r2-fr2's
/// cheap variant: no outline is built). An unmapped glyph counts as ink.
fn is_inked(text: Option<&str>) -> bool {
    text.is_none_or(|t| !t.chars().all(char::is_whitespace))
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, mode: &DrawMode) {
        self.count_path(mode);
    }

    fn draw_rect(&mut self, _: &Rect, _: DrawProps<'a>, mode: &DrawMode) {
        self.count_path(mode);
    }

    fn push_clip_path(&mut self, _: &ClipPath) {
        self.last_fill.clear();
        self.paint.clips = self.paint.clips.saturating_add(1);
    }

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {
        self.last_fill.clear();
    }

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        let page = props.transform;
        let invisible = matches!(mode, DrawMode::Invisible);
        let c = &mut self.paint;
        if invisible {
            c.glyph_runs_invisible = c.glyph_runs_invisible.saturating_add(1);
        } else {
            c.glyph_runs_visible = c.glyph_runs_visible.saturating_add(1);
        }

        let repeat = matches!(mode, DrawMode::Stroke(_)) && self.is_stroke_of_last_fill(run, page);
        self.last_fill.clear();
        if matches!(mode, DrawMode::Fill(_)) {
            self.last_fill
                .extend(run.glyphs().iter().map(|g| page * g.transform()));
        }
        if repeat || (invisible && !self.include_invisible) {
            return;
        }

        let mcid = self.marked.iter().rev().find_map(|m| *m);
        for g in run.glyphs() {
            // hayro's glyph transform is in 1/1000 em, so the glyph's em
            // vectors in page space are 1000 times its x and y columns.
            let [_, _, c, d, e, f] = (page * g.transform()).as_coeffs();
            let (dx, dy) = (c * 1000.0, d * 1000.0);
            let text = unicode(g);
            let (advance, font, type3) = match &**g {
                Glyph::Outline(o) => (o.advance_width(), FontKey(o.font_cache_key()), false),
                Glyph::Type3(_) => (None, FontKey::UNKEYED, true),
            };
            if text.is_none() {
                self.unmapped = self.unmapped.saturating_add(1);
            }
            if !invisible && is_inked(text.as_deref()) {
                self.paint.glyphs_inked = self.paint.glyphs_inked.saturating_add(1);
            }
            self.glyphs.push(GlyphItem {
                text,
                x: e,
                y: self.height - f,
                size: (dx * dx + dy * dy).sqrt(),
                advance,
                font,
                invisible,
                type3,
                mcid,
            });
        }
    }

    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {
        self.last_fill.clear();
        self.paint.images = self.paint.images.saturating_add(1);
    }

    fn pop_clip(&mut self) {
        self.last_fill.clear();
    }

    fn pop_transparency_group(&mut self) {
        self.last_fill.clear();
    }

    fn begin_marked_content(&mut self, _: &[u8], mcid: Option<i32>) {
        self.last_fill.clear();
        self.marked.push(mcid);
    }

    fn end_marked_content(&mut self) {
        self.last_fill.clear();
        self.marked.pop();
    }
}

// ---------------------------------------------------------------------------
// The font table

/// How deep [`add_resource_fonts`] follows Form XObjects inside Form XObjects.
const FORM_DEPTH: u32 = 8;

fn build_font_table(pdf: &Pdf) -> FontTable {
    let mut table = FontTable::new();
    for object in pdf.objects() {
        let Object::Dict(dict) = object else { continue };
        add_font(&mut table, &dict);
    }
    // Inline font dictionaries are not objects of their own; hayro keys them
    // by the same `cache_key`, so they are found through the resources that
    // hold them.
    let mut seen_forms = BTreeSet::new();
    for page in pdf.pages().iter() {
        let res = page.resources();
        add_resource_fonts(&mut table, &res.fonts, &res.x_objects, 0, &mut seen_forms);
    }
    table
}

/// Adds `dict` if it is a font dictionary.
fn add_font(table: &mut FontTable, dict: &Dict<'_>) {
    let Some(subtype) = dict.get::<Name<'_>>(SUBTYPE) else {
        return;
    };
    if !matches!(
        &*subtype,
        b"Type0" | b"Type1" | b"MMType1" | b"TrueType" | b"OpenType" | b"Type3"
    ) {
        return;
    }
    table
        .entry(FontKey(dict.cache_key()))
        .or_insert_with(|| font_info(dict, &subtype));
}

/// Adds every font of a `/Font` resource dictionary, then does the same for
/// each Form XObject in `xobjects`, once per form and at most [`FORM_DEPTH`]
/// deep.
fn add_resource_fonts(
    table: &mut FontTable,
    fonts: &Dict<'_>,
    xobjects: &Dict<'_>,
    depth: u32,
    seen_forms: &mut BTreeSet<u128>,
) {
    for (name, _) in fonts.entries() {
        if let Some(font) = fonts.get::<Dict<'_>>(&*name) {
            add_font(table, &font);
        }
    }
    if depth >= FORM_DEPTH {
        return;
    }
    for (name, _) in xobjects.entries() {
        let Some(form) = xobjects.get::<Stream<'_>>(&*name) else {
            continue;
        };
        let dict = form.dict();
        if dict.get::<Name<'_>>(SUBTYPE).as_deref() != Some(b"Form".as_slice())
            || !seen_forms.insert(dict.cache_key())
        {
            continue;
        }
        let Some(res) = dict.get::<Dict<'_>>(RESOURCES) else {
            continue;
        };
        let inner_fonts = res.get::<Dict<'_>>(FONT).unwrap_or_default();
        let inner_xobjects = res.get::<Dict<'_>>(XOBJECT).unwrap_or_default();
        add_resource_fonts(table, &inner_fonts, &inner_xobjects, depth + 1, seen_forms);
    }
}

fn font_info(font: &Dict<'_>, subtype: &[u8]) -> FontInfo {
    let descriptor = if subtype == b"Type0" {
        font.get::<Array<'_>>(DESCENDANT_FONTS)
            .and_then(|a| a.iter::<Dict<'_>>().next())
            .and_then(|cid| cid.get::<Dict<'_>>(FONT_DESC))
    } else {
        font.get::<Dict<'_>>(FONT_DESC)
    };
    let name = |d: &Dict<'_>, key| {
        d.get::<Name<'_>>(key)
            .map(|n| String::from_utf8_lossy(&n).into_owned())
    };
    let base_font = name(font, BASE_FONT);
    // `/Flags` is a 32-bit field; a writer may store it as a signed integer.
    let flags = descriptor
        .as_ref()
        .and_then(|d| d.get::<i64>(FLAGS))
        .map(|f| f as u32);
    let weight = descriptor.as_ref().and_then(|d| {
        d.get::<u32>(FONT_WEIGHT)
            .or_else(|| d.get::<f32>(FONT_WEIGHT).map(|w| w as u32))
    });
    let postscript_name = descriptor
        .as_ref()
        .and_then(|d| name(d, FONT_NAME))
        .or_else(|| base_font.clone())
        .map(|n| strip_subset_tag(&n).to_owned());
    let bit = |b: u32| flags.is_some_and(|f| f & (1 << (b - 1)) != 0);
    FontInfo {
        base_font,
        subtype: String::from_utf8_lossy(subtype).into_owned(),
        flags,
        postscript_name,
        weight,
        italic: bit(7),
        serif: bit(2),
        mono: bit(1),
    }
}

/// `ABCDEF+Name` → `Name` (six uppercase letters and a plus, ISO 32000-1 9.6.4).
fn strip_subset_tag(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::fixtures::{GOLDEN_TEXT, corrupt, golden_pdf};
    use crate::pdf::model::CorruptionClass;
    use crate::pdf::write::Writer;
    use lopdf::{Dictionary, Object};
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    fn extract(pdf: &[u8]) -> Vec<PageText> {
        extract_text(pdf, &ExtractOptions::default()).expect("extracts")
    }

    fn page_string(page: &PageText) -> String {
        page.glyphs
            .iter()
            .map(|g| g.text.as_deref().unwrap_or("\u{FFFD}"))
            .collect()
    }

    fn inked(s: &str) -> u32 {
        s.chars().filter(|c| !c.is_whitespace()).count() as u32
    }

    #[test]
    fn golden_glyphs_are_all_mapped_and_spell_the_known_strings() {
        let pages = extract(&golden_pdf());
        assert_eq!(pages.len(), GOLDEN_TEXT.len());
        for (i, (page, lines)) in pages.iter().zip(GOLDEN_TEXT).enumerate() {
            assert_eq!(page.index, i as u32);
            assert_eq!((page.width, page.height), (612.0, 792.0));
            assert_eq!(page.unmapped, 0, "page {i}");
            assert!(page.glyphs.iter().all(|g| g.text.is_some()));
            assert_eq!(page_string(page), lines.concat(), "page {i}");
            assert!(page.glyphs.iter().all(|g| !g.invisible && !g.type3));
            assert!(page.warnings.is_empty(), "{:?}", page.warnings);
        }
    }

    #[test]
    fn golden_geometry_is_page_space_with_y_up() {
        let pages = extract(&golden_pdf());
        let first = &pages[0].glyphs[0];
        // `/F1 16 Tf 72 720 Td`: the first glyph sits at (72, 720), 16 pt.
        assert_eq!((first.x, first.y, first.size), (72.0, 720.0, 16.0));
        assert!(first.advance.is_some_and(|a| a > 0.0));
        let second_line = &pages[0].glyphs[GOLDEN_TEXT[0][0].chars().count()];
        assert_eq!((second_line.x, second_line.y), (72.0, 696.0));
        // One font for every glyph, and the table knows it.
        let fonts = font_table(&golden_pdf()).expect("font table");
        let key = first.font;
        assert!(pages.iter().flat_map(|p| &p.glyphs).all(|g| g.font == key));
        let info = fonts.get(&key).expect("the golden's font is in the table");
        assert_eq!(info.subtype, "Type0");
        assert_eq!(info.base_font.as_deref(), Some("AAAAAA+NotoSans-Regular"));
        assert_eq!(info.postscript_name.as_deref(), Some("NotoSans-Regular"));
        assert_eq!(info.flags, Some(32));
        assert!(!info.italic && !info.serif && !info.mono);
    }

    #[test]
    fn golden_paint_counts() {
        let pages = extract(&golden_pdf());
        let expect = |i: usize, images: u32| PaintCounts {
            path_fills: 0,
            path_strokes: 0,
            images,
            glyph_runs_visible: GOLDEN_TEXT[i].len() as u32,
            glyph_runs_invisible: 0,
            glyphs_inked: GOLDEN_TEXT[i].iter().map(|l| inked(l)).sum(),
            clips: 0,
        };
        assert_eq!(pages[0].paint, expect(0, 1));
        assert_eq!(pages[1].paint, expect(1, 0));
    }

    /// `golden` with its `/ToUnicode` entry blanked to spaces (offsets kept).
    fn without_tounicode(golden: &[u8]) -> Vec<u8> {
        let key = b"/ToUnicode";
        let at = golden
            .windows(key.len())
            .position(|w| w == key)
            .expect("/ToUnicode entry");
        let end = at + golden[at..].iter().position(|&b| b == b'R').expect("ref") + 1;
        let mut out = golden.to_vec();
        out[at..end].fill(b' ');
        out
    }

    #[test]
    fn without_tounicode_type0_glyphs_are_unmapped_and_counted() {
        let golden = extract(&golden_pdf());
        let pages = extract(&without_tounicode(&golden_pdf()));
        assert_eq!(pages.len(), golden.len());
        for (page, gold) in pages.iter().zip(&golden) {
            assert_eq!(page.glyphs.len(), gold.glyphs.len());
            assert!(page.glyphs.iter().all(|g| g.text.is_none()));
            assert_eq!(page.unmapped as usize, page.glyphs.len());
            // Unmapped glyphs still count as ink: the page is not blank.
            assert_eq!(page.paint.glyphs_inked as usize, page.glyphs.len());
        }
    }

    #[test]
    fn unrepaired_c6_doubles_the_glyphs_on_the_page_that_lost_its_font() {
        let golden = extract(&golden_pdf());
        let c6 = corrupt(CorruptionClass::C6FontMapLost, &golden_pdf(), 7).expect("C6");
        let pages = extract(&c6);
        assert_eq!(pages.len(), golden.len());
        let hit: Vec<usize> = (0..pages.len())
            .filter(|&i| pages[i].glyphs.len() != golden[i].glyphs.len())
            .collect();
        assert_eq!(hit.len(), 1, "exactly one page lost its /Font");
        let (page, gold) = (&pages[hit[0]], &golden[hit[0]]);
        let (n, g) = (page.glyphs.len(), gold.glyphs.len());
        // Two-byte codes read one byte at a time by the Helvetica fallback.
        assert!(n * 10 >= g * 18 && n * 10 <= g * 22, "{n} vs {g}");
        assert!(
            page.unmapped as usize * 2 >= n,
            "unmapped {} of {n}",
            page.unmapped
        );
        // No font dictionary behind any of them: hayro's fallback, not Type3.
        assert!(
            page.glyphs
                .iter()
                .all(|g| g.font == FontKey::UNKEYED && !g.type3)
        );
        let fonts = font_table(&c6).expect("font table");
        assert!(!fonts.contains_key(&FontKey::UNKEYED));
    }

    #[test]
    fn two_runs_are_identical() {
        assert_eq!(extract(&golden_pdf()), extract(&golden_pdf()));
        assert_eq!(dump(&extract(&golden_pdf())), dump(&extract(&golden_pdf())));
        assert_eq!(extract(&blank_cases_pdf()), extract(&blank_cases_pdf()));
    }

    #[test]
    fn random_bytes_do_not_load() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let bytes: Vec<u8> = (0..5000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        assert_eq!(
            extract_text(&bytes, &ExtractOptions::default()),
            Err(ExtractError::Load(LoadPdfError::Invalid))
        );
        assert_eq!(
            extract_text(b"", &ExtractOptions::default()),
            Err(ExtractError::Load(LoadPdfError::Invalid))
        );
        assert_eq!(
            font_table(&bytes),
            Err(ExtractError::Load(LoadPdfError::Invalid))
        );
    }

    // -----------------------------------------------------------------------
    // The blank-case constructions (goal-r2-fr2 §3).

    const BLANK_CASES: [&str; 11] = [
        "",
        "1 1 1 rg 0 0 612 792 re f",
        "BT /F1 12 Tf 3 Tr 72 700 Td (Hidden) Tj ET",
        "0 g 100 100 200 200 re f",
        "0 0 0 0 re W n 0 g 100 100 200 200 re f",
        "BT /F1 12 Tf 72 700 Td (     ) Tj ET",
        "q 100 0 0 100 50 50 cm /Im1 Do Q",
        "BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
        "0 G 1 w 10 10 m 500 500 l S",
        "0 g 10 10 m 500 500 l 500 10 l h n",
        "1 g BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
    ];

    fn obj_dict(entries: Vec<(&str, Object)>) -> Dictionary {
        let mut d = Dictionary::new();
        for (k, v) in entries {
            d.set(k, v);
        }
        d
    }

    fn name(s: &str) -> Object {
        Object::Name(s.as_bytes().to_vec())
    }

    /// Eleven pages, one construction each, a non-embedded Helvetica and a
    /// 1×1 white image.
    fn blank_cases_pdf() -> Vec<u8> {
        const FONT: u32 = 3;
        const IMAGE: u32 = 4;
        let mut w = Writer::with_version("1.7");
        w.add(
            1,
            Object::Dictionary(obj_dict(vec![
                ("Type", name("Catalog")),
                ("Pages", Object::Reference((2, 0))),
            ])),
        );
        w.add(
            FONT,
            Object::Dictionary(obj_dict(vec![
                ("Type", name("Font")),
                ("Subtype", name("Type1")),
                ("BaseFont", name("Helvetica")),
            ])),
        );
        w.add_stream_raw(
            IMAGE,
            obj_dict(vec![
                ("Type", name("XObject")),
                ("Subtype", name("Image")),
                ("Width", Object::Integer(1)),
                ("Height", Object::Integer(1)),
                ("ColorSpace", name("DeviceRGB")),
                ("BitsPerComponent", Object::Integer(8)),
            ]),
            vec![0xFF; 3],
        );
        let mut kids = Vec::new();
        for (i, content) in BLANK_CASES.iter().enumerate() {
            let (page, contents) = (10 + 2 * i as u32, 11 + 2 * i as u32);
            let resources = obj_dict(vec![
                (
                    "Font",
                    Object::Dictionary(obj_dict(vec![("F1", Object::Reference((FONT, 0)))])),
                ),
                (
                    "XObject",
                    Object::Dictionary(obj_dict(vec![("Im1", Object::Reference((IMAGE, 0)))])),
                ),
            ]);
            w.add(
                page,
                Object::Dictionary(obj_dict(vec![
                    ("Type", name("Page")),
                    ("Parent", Object::Reference((2, 0))),
                    (
                        "MediaBox",
                        Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
                    ),
                    ("Resources", Object::Dictionary(resources)),
                    ("Contents", Object::Reference((contents, 0))),
                ])),
            );
            w.add_stream_raw(contents, Dictionary::new(), content.as_bytes().to_vec());
            kids.push(Object::Reference((page, 0)));
        }
        w.add(
            2,
            Object::Dictionary(obj_dict(vec![
                ("Type", name("Pages")),
                ("Count", Object::Integer(kids.len() as i64)),
                ("Kids", Object::Array(kids)),
            ])),
        );
        w.trailer((1, 0), [7; 32], None);
        w.finish().expect("blank cases build")
    }

    /// `(path_fills, path_strokes, images, runs_visible, runs_invisible,
    /// glyphs_inked, clips)` per construction, as goal-r2-fr2 tabulates them.
    const BLANK_EXPECTED: [[u32; 7]; 11] = [
        [0, 0, 0, 0, 0, 0, 0], // empty content
        [1, 0, 0, 0, 0, 0, 0], // white rectangle fill
        [0, 0, 0, 0, 1, 0, 0], // render mode 3
        [1, 0, 0, 0, 0, 0, 0], // black rectangle
        [1, 0, 0, 0, 0, 0, 1], // rectangle inside an empty clip
        [0, 0, 0, 1, 0, 0, 0], // spaces only
        [0, 0, 1, 0, 0, 0, 0], // 1×1 white image
        [0, 0, 0, 1, 0, 5, 0], // visible text
        [0, 1, 0, 0, 0, 0, 0], // stroked line
        [0, 0, 0, 0, 0, 0, 0], // path painted with `n`
        [0, 0, 0, 1, 0, 5, 0], // white text
    ];

    #[test]
    fn blank_cases_give_the_tabulated_paint_counts() {
        let pages = extract(&blank_cases_pdf());
        assert_eq!(pages.len(), BLANK_CASES.len());
        for (i, (page, e)) in pages.iter().zip(BLANK_EXPECTED).enumerate() {
            let want = PaintCounts {
                path_fills: e[0],
                path_strokes: e[1],
                images: e[2],
                glyph_runs_visible: e[3],
                glyph_runs_invisible: e[4],
                glyphs_inked: e[5],
                clips: e[6],
            };
            assert_eq!(page.paint, want, "page {i}: {}", BLANK_CASES[i]);
        }
        // What blank_pages (D-059) reads: empty, mode-3 and `n` pages paint
        // nothing; the spaces-only page inks nothing.
        for i in [0, 2, 9] {
            assert_eq!(pages[i].paint.paint_ops(), 0, "page {i}");
        }
        assert_eq!(pages[5].paint.glyphs_inked, 0);
        assert_eq!(page_string(&pages[5]), "     ");
        assert_eq!(page_string(&pages[7]), "Hello");
    }

    #[test]
    fn invisible_text_is_flagged_and_can_be_left_out() {
        let pages = extract(&blank_cases_pdf());
        assert_eq!(page_string(&pages[2]), "Hidden");
        assert!(pages[2].glyphs.iter().all(|g| g.invisible));
        assert!(pages[7].glyphs.iter().all(|g| !g.invisible));

        let opts = ExtractOptions {
            include_invisible: false,
            ..ExtractOptions::default()
        };
        let pages = extract_text(&blank_cases_pdf(), &opts).expect("extracts");
        assert!(pages[2].glyphs.is_empty());
        // The run is still counted.
        assert_eq!(pages[2].paint.glyph_runs_invisible, 1);
        assert_eq!(page_string(&pages[7]), "Hello");
    }

    /// One page drawing `content` with a non-embedded Helvetica written inline
    /// as `/F1`; `catalog`, `resources` and `page` add entries, `extra` adds
    /// objects from 5 on.
    fn one_page_pdf(
        content: &[u8],
        catalog: Vec<(&str, Object)>,
        resources: Vec<(&str, Object)>,
        page: Vec<(&str, Object)>,
        extra: Vec<Object>,
    ) -> Vec<u8> {
        let mut w = Writer::with_version("1.7");
        let mut cat = obj_dict(vec![
            ("Type", name("Catalog")),
            ("Pages", Object::Reference((2, 0))),
        ]);
        for (k, v) in catalog {
            cat.set(k, v);
        }
        w.add(1, Object::Dictionary(cat));
        w.add(
            2,
            Object::Dictionary(obj_dict(vec![
                ("Type", name("Pages")),
                ("Count", Object::Integer(1)),
                ("Kids", Object::Array(vec![Object::Reference((3, 0))])),
            ])),
        );
        let font = obj_dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("Type1")),
            ("BaseFont", name("Helvetica")),
        ]);
        let mut res = obj_dict(vec![(
            "Font",
            Object::Dictionary(obj_dict(vec![("F1", Object::Dictionary(font))])),
        )]);
        for (k, v) in resources {
            res.set(k, v);
        }
        let mut page_dict = obj_dict(vec![
            ("Type", name("Page")),
            ("Parent", Object::Reference((2, 0))),
            (
                "MediaBox",
                Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            ),
            ("Resources", Object::Dictionary(res)),
            ("Contents", Object::Reference((4, 0))),
        ]);
        for (k, v) in page {
            page_dict.set(k, v);
        }
        w.add(3, Object::Dictionary(page_dict));
        w.add_stream_raw(4, Dictionary::new(), content.to_vec());
        for (i, o) in extra.into_iter().enumerate() {
            w.add(5 + i as u32, o);
        }
        w.trailer((1, 0), [9; 32], None);
        w.finish().expect("builds")
    }

    #[test]
    fn fill_then_stroke_text_is_extracted_once() {
        // Mode 2 (fill, then stroke), then mode 1 (stroke) for a run of the
        // same length right after a filled one.
        let content = b"BT /F1 12 Tf 2 Tr 72 700 Td (Bold) Tj 0 Tr (ab) Tj 1 Tr (cd) Tj ET";
        let pages = extract(&one_page_pdf(content, vec![], vec![], vec![], vec![]));
        assert_eq!(page_string(&pages[0]), "Boldabcd");
        // Each paint call is counted; each glyph is inked once.
        assert_eq!(pages[0].paint.glyph_runs_visible, 4);
        assert_eq!(pages[0].paint.glyphs_inked, 8);
    }

    #[test]
    fn glyphs_carry_the_innermost_mcid() {
        let content = b"BT /F1 12 Tf 72 700 Td /P << /MCID 3 >> BDC (Tag) Tj \
                        /Span BMC (in) Tj EMC EMC (Free) Tj ET";
        let pages = extract(&one_page_pdf(content, vec![], vec![], vec![], vec![]));
        let mcids: Vec<Option<i32>> = pages[0].glyphs.iter().map(|g| g.mcid).collect();
        let mut want = vec![Some(3); 5];
        want.extend([None; 4]);
        assert_eq!(mcids, want);
    }

    /// One page whose `Secret` sits in an optional-content group that is off
    /// by default and whose `Shown` does not.
    fn hidden_layer_pdf() -> Vec<u8> {
        let ocg = Object::Reference((5, 0));
        let catalog = vec![(
            "OCProperties",
            Object::Dictionary(obj_dict(vec![
                ("OCGs", Object::Array(vec![ocg.clone()])),
                (
                    "D",
                    Object::Dictionary(obj_dict(vec![("OFF", Object::Array(vec![ocg.clone()]))])),
                ),
            ])),
        )];
        let resources = vec![(
            "Properties",
            Object::Dictionary(obj_dict(vec![("oc1", ocg)])),
        )];
        let layer = Object::Dictionary(obj_dict(vec![
            ("Type", name("OCG")),
            ("Name", Object::string_literal("Hidden")),
        ]));
        let content = b"BT /F1 12 Tf 72 700 Td /OC /oc1 BDC (Secret) Tj EMC (Shown) Tj ET";
        one_page_pdf(content, catalog, resources, vec![], vec![layer])
    }

    /// `pdf` with everything from its last `xref` keyword to the end blanked
    /// to spaces: no xref table, no trailer, no `startxref`.
    fn without_xref_and_trailer(pdf: &[u8]) -> Vec<u8> {
        let at = pdf
            .windows(5)
            .rposition(|w| w == b"\nxref")
            .expect("an xref table")
            + 1;
        let mut out = pdf.to_vec();
        out[at..].fill(b' ');
        out
    }

    #[test]
    fn hidden_layer_text_is_absent_and_the_page_says_so() {
        let pages = extract(&hidden_layer_pdf());
        assert_eq!(page_string(&pages[0]), "Shown");
        assert_eq!(pages[0].paint.glyph_runs_visible, 1);
        assert_eq!(pages[0].warnings, vec![HIDDEN_LAYERS.to_owned()]);
    }

    #[test]
    fn hidden_layers_are_reported_when_the_catalog_was_found_by_scanning() {
        let damaged = without_xref_and_trailer(&hidden_layer_pdf());
        assert!(!damaged.windows(7).any(|w| w == b"trailer"));
        let pages = extract(&damaged);
        assert_eq!(pages.len(), 1);
        assert_eq!(page_string(&pages[0]), "Shown");
        assert!(
            pages[0].warnings.contains(&HIDDEN_LAYERS.to_owned()),
            "{:?}",
            pages[0].warnings
        );
    }

    #[test]
    fn inline_fonts_in_page_and_form_resources_are_in_the_table() {
        // `/F1` is written inline in the page's resources; `/Fx` inline in a
        // Form XObject's own resources.
        let courier = obj_dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("Type1")),
            ("BaseFont", name("Courier")),
        ]);
        let form = Object::Stream(lopdf::Stream::new(
            obj_dict(vec![
                ("Type", name("XObject")),
                ("Subtype", name("Form")),
                (
                    "BBox",
                    Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
                ),
                (
                    "Resources",
                    Object::Dictionary(obj_dict(vec![(
                        "Font",
                        Object::Dictionary(obj_dict(vec![("Fx", Object::Dictionary(courier))])),
                    )])),
                ),
            ]),
            b"BT /Fx 10 Tf 72 600 Td (Form) Tj ET".to_vec(),
        ));
        let resources = vec![(
            "XObject",
            Object::Dictionary(obj_dict(vec![("Fm1", Object::Reference((5, 0)))])),
        )];
        let content = b"BT /F1 12 Tf 72 700 Td (Page) Tj ET /Fm1 Do";
        let pdf = one_page_pdf(content, vec![], resources, vec![], vec![form]);
        let pages = extract(&pdf);
        assert_eq!(page_string(&pages[0]), "PageForm");
        let fonts = font_table(&pdf).expect("font table");
        let names: Vec<Option<&str>> = pages[0]
            .glyphs
            .iter()
            .map(|g| fonts.get(&g.font).and_then(|f| f.base_font.as_deref()))
            .collect();
        let mut want = vec![Some("Helvetica"); 4];
        want.extend([Some("Courier"); 4]);
        assert_eq!(names, want);
        assert_eq!(fonts.len(), 2);
        assert!(!fonts.contains_key(&FontKey::UNKEYED));
    }

    #[test]
    fn annotation_text_is_left_out_unless_asked_for() {
        let appearance = Object::Stream(lopdf::Stream::new(
            obj_dict(vec![
                ("Type", name("XObject")),
                ("Subtype", name("Form")),
                (
                    "BBox",
                    Object::Array(vec![0.into(), 0.into(), 200.into(), 20.into()]),
                ),
            ]),
            b"BT /F1 10 Tf 2 5 Td (Note) Tj ET".to_vec(),
        ));
        let annot = Object::Dictionary(obj_dict(vec![
            ("Type", name("Annot")),
            ("Subtype", name("FreeText")),
            (
                "Rect",
                Object::Array(vec![100.into(), 100.into(), 300.into(), 120.into()]),
            ),
            (
                "AP",
                Object::Dictionary(obj_dict(vec![("N", Object::Reference((5, 0)))])),
            ),
        ]));
        let page = vec![("Annots", Object::Array(vec![Object::Reference((6, 0))]))];
        let content = b"BT /F1 12 Tf 72 700 Td (Body) Tj ET";
        let pdf = one_page_pdf(content, vec![], vec![], page, vec![appearance, annot]);

        let pages = extract(&pdf);
        assert_eq!(page_string(&pages[0]), "Body");
        assert_eq!(pages[0].paint.glyph_runs_visible, 1);

        let opts = ExtractOptions {
            render_annotations: true,
            ..ExtractOptions::default()
        };
        let pages = extract_text(&pdf, &opts).expect("extracts");
        assert_eq!(page_string(&pages[0]), "BodyNote");
        assert_eq!(pages[0].paint.glyph_runs_visible, 2);
    }

    #[test]
    fn clip_only_text_produces_a_clip_and_no_glyphs() {
        let content = b"BT /F1 12 Tf 7 Tr 72 700 Td (Clip) Tj ET";
        let pages = extract(&one_page_pdf(content, vec![], vec![], vec![], vec![]));
        assert!(pages[0].glyphs.is_empty());
        assert_eq!(pages[0].paint.clips, 1);
        assert_eq!(pages[0].paint.paint_ops(), 0);
        // A limit of every file: documented once, not repeated per page.
        assert!(KNOWN_LIMITS.iter().any(|l| l.contains("render mode 7")));
        assert!(pages[0].warnings.is_empty(), "{:?}", pages[0].warnings);
    }

    #[test]
    fn scanned_page_order_is_recorded() {
        let c4 = corrupt(CorruptionClass::C4PageTreeBroken, &golden_pdf(), 3).expect("C4");
        let pages = extract(&c4);
        assert_eq!(pages.len(), GOLDEN_TEXT.len());
        for page in &pages {
            assert_eq!(page.warnings, vec![SCANNED_PAGES.to_owned()]);
        }
    }

    // -----------------------------------------------------------------------
    // The cross-OS dump (FR-g1, goal-r2-fr2): `GlyphItem` fields and
    // `PaintCounts` only, floats as raw bits, no derived geometry. Pixels are
    // never part of it. The `test` job runs this on all three OSes.

    fn dump(pages: &[PageText]) -> String {
        let mut s = String::new();
        for p in pages {
            let c = &p.paint;
            writeln!(
                s,
                "P {} {:08x} {:08x} unmapped={} fills={} strokes={} images={} runs={} \
                 invisible_runs={} inked={} clips={}",
                p.index,
                p.width.to_bits(),
                p.height.to_bits(),
                p.unmapped,
                c.path_fills,
                c.path_strokes,
                c.images,
                c.glyph_runs_visible,
                c.glyph_runs_invisible,
                c.glyphs_inked,
                c.clips,
            )
            .unwrap();
            for g in &p.glyphs {
                writeln!(
                    s,
                    "G {:?} {:016x} {:016x} {:016x} {:?} {:032x} {} {} {:?}",
                    g.text,
                    g.x.to_bits(),
                    g.y.to_bits(),
                    g.size.to_bits(),
                    g.advance.map(f32::to_bits),
                    g.font.0,
                    g.invisible,
                    g.type3,
                    g.mcid,
                )
                .unwrap();
            }
        }
        s
    }

    /// sha256 of `dump(extract(golden_pdf()))`. A change here is a change in
    /// what verify and export read: re-baseline only with a reason.
    const GOLDEN_DUMP_SHA256: &str =
        "429109edc8884ee5ef14e24f82dc7963c3cda50f3578a04109049f735d1ecf80";

    #[test]
    fn golden_dump_hash_is_the_committed_one() {
        let d = dump(&extract(&golden_pdf()));
        let hex: String = Sha256::digest(d.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, GOLDEN_DUMP_SHA256, "dump:\n{d}");
    }
}
