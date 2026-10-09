//! Verification gates V0, V1 and V2 (T-12b; SE Q2/Q5, D-054, D-059, D-074).
//!
//! [`verify`] judges one candidate output against the input's [`Baseline`].
//! The record shapes are fixed by T-02b. Every value is a `bool`, a count or
//! an integer [`Ratio`] of interpreted or carved data: no number read from
//! rendered pixels reaches verification (D-059). The hayro render is a V0
//! pass/fail gate and nothing else ([`hayro_renders_every_page`]).
//!
//! **V0, hard gates:**
//! - `lopdf_reload`: lopdf loads the output with `strict: true` and a
//!   256 MiB decompression cap;
//! - `hayro_render_all_pages`: hayro loads it, finds as many pages as the
//!   output's page tree holds, and renders each without panicking. Its
//!   warnings are not read: hayro self-repairs (FR-04 §6), so the render is
//!   a gate on the output only, never a detector;
//! - `rediagnose_clean`: the output is carved and diagnosed again under a
//!   zero salvage budget (classify only, never search, D-074) and carries no
//!   finding of a targeted class, except a finding of a pass's own class
//!   at a location that pass already reported `Partial` (the `partial`
//!   argument; see [`clean_for`]);
//! - `page_count_ok`: the output's page tree holds at least one page and at
//!   least as many as the carver discovered in the input
//!   (`pages_in_doc_order`). The tree is walked by lopdf when the output
//!   reloads, else by our own graph;
//! - `root_and_pages_present`: our own graph of the output has a valid
//!   catalog and reaches at least one page from it.
//!
//! **V1, retention** (output over baseline). With a [`Baseline::Extracted`]
//! baseline every count comes from T-36 on both sides: `text_ops` counts
//! glyph runs (visible and invisible), `path_fills` fills, `images` image
//! draws, `glyph_count` glyphs. With a [`Baseline::CarveProxy`] (the input
//! did not load in hayro) `text_ops` is the output's show operators over the
//! input's, `glyph_count` the output's glyphs over the codes the input's
//! show strings hold (advisory, D-054), `images` the image objects carved
//! from the output over those carved from the input, and `path_fills` has no
//! input count, so its denominator is 0. `content_bytes` is always carved
//! data: the raw (still filtered) bytes of the `/Contents` streams of the
//! output's pages over those of every page the carver found in the input.
//! It depends on encoding, not only on what was kept: a C9 repair writes a
//! `Repaired` stream with its ASCII stages dropped (smaller) and a
//! `ChecksumMismatch` or `Prefix` stream uncompressed (often several times
//! larger), so a stream that lost data can score above 1/1. It must not
//! decide between candidates that differ in the C9 pass. (Decoded lengths
//! would need the input's bytes, which the frozen interface does not pass.)
//! `blank_pages` is the output's blank pages ([`is_blank`]) less the
//! baseline's: a page that was blank in the input is not a loss. A zero
//! denominator means the baseline had none of the thing; [`Ratio`] orders
//! `0/0` as zero.
//!
//! **Glyphs drawn through the fallback font (D-088).** When a font does not
//! resolve, hayro draws its text with a fallback Helvetica that reads one
//! byte per code, so a Type 0 font's two-byte codes would count as two
//! glyphs each and a perfect repair would score 1/2. On a side (the input or
//! the output) where hayro drew any glyph that way, V1 counts glyphs from
//! the carve instead, each string read at the code width of the font its
//! `Tf` selected ([`CodeWidths`]). So each code counts once, whichever font
//! drew it. Against a [`Baseline::Extracted`] the count covers what the
//! side's pages draw ([`page_show_counts`]): each page's `/Contents` and the
//! forms they draw, not annotation appearances or objects no page draws,
//! as T-36 counts. Against a [`Baseline::CarveProxy`] the output's count
//! covers every stream ([`show_counts`]), as the proxy's does. The side
//! keeps hayro's count when no string is read at two bytes a code (hayro's
//! count is then already right), or when some string's width is not known:
//! its font name is bound to no font, or to fonts of both widths, and the
//! file's fonts are not all of one width.
//!
//! **V2, plausibility** of the output's extracted text: unmapped glyphs and
//! glyphs that map to U+FFFD over all glyphs, and the share of letter-bearing
//! tokens whose letters are all of one script ([`plausibility`]).
//! `dictionary_hit` is `None`: no word list ships (D-011).
// T-13a is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};

use lopdf::{Dictionary, Document, LoadOptions, Object};
use serde::{Deserialize, Serialize};

use crate::engine::SalvageBudget;
use crate::pdf::carver::{Body, CarveReport, Orphan, carve};
use crate::pdf::diagnose::diagnose;
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::model::{
    CorruptionClass, Finding, FindingKind, Location, ObjId, ObjectKind, PaintCounts, Ratio,
};
use crate::pdf::rebuild::{Held, IdRemap, plan_ids};
use crate::pdf::streams::salvage::{CarveSource, SalvageIndex, salvage_all};
use crate::pdf::streams::{DEFAULT_CAP, content_ops, decode_chain, filters_of};
use crate::pdf::text::{ExtractOptions, FontKey, PageText, extract_text};

/// One candidate output's verification, against the input's baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub baseline: BaselineKind,
    pub v0: Gates,
    pub v1: Retention,
    pub v2: Plausibility,
}

/// What the input's text baseline came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselineKind {
    /// Text extracted from the input (T-36).
    Extracted,
    /// Show-operator counts over the carved content streams, when the input
    /// does not load.
    CarveProxy,
    None,
}

/// V0: pass/fail gates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gates {
    pub lopdf_reload: bool,
    pub hayro_render_all_pages: bool,
    pub rediagnose_clean: bool,
    pub page_count_ok: bool,
    pub root_and_pages_present: bool,
}

impl Gates {
    /// Every gate passed: the first key of the selection tuple (SE Q2).
    pub fn all_pass(&self) -> bool {
        self.lopdf_reload
            && self.hayro_render_all_pages
            && self.rediagnose_clean
            && self.page_count_ok
            && self.root_and_pages_present
    }
}

/// V1: what the output kept, as fractions of the baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retention {
    pub text_ops: Ratio,
    pub path_fills: Ratio,
    pub images: Ratio,
    pub content_bytes: Ratio,
    /// Pages blank in the output but not in the baseline, from paint counts.
    pub blank_pages: u32,
    pub glyph_count: Ratio,
}

/// V2: whether the output's text reads as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plausibility {
    pub unmapped_glyph: Ratio,
    pub fffd: Ratio,
    pub script_consistency: Ratio,
    /// `None` without a dictionary (D-011).
    pub dictionary_hit: Option<Ratio>,
}

/// What V1 measures a candidate against (D-054), built once per input by
/// [`baseline`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Baseline {
    /// T-36 on the input, when hayro loads it. `codes` is the carve's count
    /// of the codes the input's pages show ([`page_show_counts`]) when hayro
    /// drew any of its glyphs with the fallback font, some string is two
    /// bytes a code and every code width is known, else `None` (D-088).
    Extracted {
        pages: Vec<PageText>,
        codes: Option<u64>,
    },
    /// When it does not: the `Tj`, `TJ`, `'` and `"` operators over every
    /// carved content stream and Form XObject, and the codes of the strings
    /// they show ([`show_counts`]).
    CarveProxy { show_ops: u64, codes: u64 },
}

impl Baseline {
    /// The kind the verification record carries.
    pub(crate) fn kind(&self) -> BaselineKind {
        match self {
            Baseline::Extracted { .. } => BaselineKind::Extracted,
            Baseline::CarveProxy { .. } => BaselineKind::CarveProxy,
        }
    }
}

/// The input's baseline: T-36's pages when hayro loads `input`, else the
/// carve proxy over `carve`, which must come from `input`.
pub(crate) fn baseline(input: &[u8], carve: &CarveReport) -> Baseline {
    let graph = || ObjectGraph::from_carve(carve);
    let salvage = || classify_only(carve, input);
    match extract_text(input, &ExtractOptions::default()) {
        Ok(pages) => {
            let codes = drew_with_fallback(&pages)
                .then(|| {
                    let graph = graph();
                    let pages = graph.pages_in_doc_order();
                    page_show_counts(input, carve, &graph, &salvage(), &pages)
                })
                .and_then(|s| s.codes_counted());
            Baseline::Extracted { pages, codes }
        }
        Err(_) => {
            let s = show_counts(input, carve, &graph(), &salvage());
            Baseline::CarveProxy {
                show_ops: s.ops,
                codes: s.codes,
            }
        }
    }
}

/// V0, V1 and V2 of `output` (module docs). `carve` is the input's carve,
/// `input_text` its [`baseline`], `targeted` the classes the passes set out
/// to fix, and `partial` the locations that a pass reported `Partial`, each
/// with that pass's class (a finding of that class there may stay, D-074,
/// D-137).
///
/// `partial` is not in the plan's interface: without it `verify` cannot tell
/// which findings a pass already owned up to. Its locations are in the
/// output's object numbers and page indexes, and match per page, not per
/// slot ([`same_site`]).
pub(crate) fn verify(
    output: &[u8],
    carve: &CarveReport,
    input_text: &Baseline,
    targeted: &[CorruptionClass],
    partial: &[(CorruptionClass, Location)],
) -> Verification {
    verify_spending(output, carve, input_text, targeted, partial).0
}

/// [`verify`], and the salvage work W its re-diagnosis spent: 0 by
/// construction (D-074), and the tests check it.
fn verify_spending(
    output: &[u8],
    carve: &CarveReport,
    input_text: &Baseline,
    targeted: &[CorruptionClass],
    partial: &[(CorruptionClass, Location)],
) -> (Verification, u64) {
    let out_carve = carve_all(output);
    let out_graph = ObjectGraph::from_carve(&out_carve);
    let out_salvage = classify_only(&out_carve, output);
    let findings = diagnose(output, &out_carve, &out_graph, &out_salvage);

    let in_graph = ObjectGraph::from_carve(carve);
    let reloaded = strict_reload(output);
    let reached = reached_pages(&out_graph);
    // lopdf's page-tree walk when the output loads, else our own graph's.
    let out_pages = reloaded
        .as_ref()
        .map_or(reached.len(), |d| d.get_pages().len());

    let v0 = Gates {
        lopdf_reload: reloaded.is_some(),
        hayro_render_all_pages: hayro_renders_every_page(output, out_pages),
        rediagnose_clean: clean_for(&findings, targeted, partial),
        page_count_ok: out_pages > 0 && out_pages >= in_graph.pages_in_doc_order().len(),
        root_and_pages_present: !reached.is_empty(),
    };

    let text = extract_text(output, &ExtractOptions::default()).unwrap_or_default();
    let content_bytes = ratio(
        contents_bytes(&out_carve, &out_graph, &reached),
        contents_bytes(carve, &in_graph, &in_graph.pages_in_doc_order()),
    );
    let v1 = match input_text {
        Baseline::Extracted { pages: base, codes } => {
            let (o, b) = (Tally::of(&text), Tally::of(base));
            let shown = || page_show_counts(output, &out_carve, &out_graph, &out_salvage, &reached);
            Retention {
                text_ops: ratio(o.runs, b.runs),
                path_fills: ratio(o.fills, b.fills),
                images: ratio(o.images, b.images),
                content_bytes,
                blank_pages: o.blank.saturating_sub(b.blank),
                glyph_count: ratio(
                    glyphs_counted(&text, o.glyphs, shown),
                    codes.unwrap_or(b.glyphs),
                ),
            }
        }
        Baseline::CarveProxy { show_ops, codes } => {
            let o = Tally::of(&text);
            let out = show_counts(output, &out_carve, &out_graph, &out_salvage);
            Retention {
                text_ops: ratio(out.ops, *show_ops),
                path_fills: ratio(o.fills, 0),
                images: ratio(image_objects(&out_carve), image_objects(carve)),
                content_bytes,
                blank_pages: o.blank,
                glyph_count: ratio(glyphs_counted(&text, o.glyphs, || out), *codes),
            }
        }
    };

    let verification = Verification {
        baseline: input_text.kind(),
        v0,
        v1,
        v2: plausibility(&text),
    };
    (verification, out_salvage.work_total)
}

fn ratio(num: u64, den: u64) -> Ratio {
    Ratio { num, den }
}

// ── V0 ───────────────────────────────────────────────────────────────────

fn carve_all(bytes: &[u8]) -> CarveReport {
    // Never cancelled: the closure always answers no.
    carve(bytes, &|| false).unwrap_or_default()
}

/// The salvage index under a zero budget: every Flate stream is inflated
/// once and classified, and no candidate is ever tried (D-074).
pub(crate) fn classify_only(carve: &CarveReport, bytes: &[u8]) -> SalvageIndex {
    let budget = SalvageBudget {
        work: 0,
        deep_work: 0,
        deep_pool: 0,
        ..SalvageBudget::default()
    };
    salvage_all(
        &CarveSource::new(carve, bytes),
        &budget,
        NonZeroUsize::MIN,
        1 << 30,
        &|| false,
    )
    .unwrap_or_default()
}

/// lopdf's strict load with the plan's decompression cap, under
/// `catch_unwind`.
fn strict_reload(output: &[u8]) -> Option<Document> {
    let opts = LoadOptions {
        strict: true,
        max_decompressed_size: Some(256 << 20),
        ..LoadOptions::default()
    };
    catch_unwind(AssertUnwindSafe(|| {
        Document::load_mem_with_options(output, opts).ok()
    }))
    .unwrap_or(None)
}

/// The longest side, in pixels, of any page the V0 render gate draws.
const RENDER_MAX_SIDE: f32 = 256.0;

/// The V0 render gate, and the only place this module renders: hayro loads
/// `output`, finds `pages` pages, and draws each without panicking. The
/// pixmaps are dropped unread: no pixel count, hash or threshold is taken
/// from them (D-059).
///
/// A page is drawn at 1/8 scale, shrunk further so that its longer side is
/// at most [`RENDER_MAX_SIDE`] pixels. Nothing upstream clamps `/MediaBox`
/// (emit copies it as carved), and hayro allocates width × height pixels for
/// whatever scale it is given: a 524280 pt page at 1/8 scale would be a
/// 17 GiB pixmap, and an allocation failure aborts the process, which
/// `catch_unwind` cannot catch. The scale is a plain division (no libm) and
/// never reaches the record.
fn hayro_renders_every_page(output: &[u8], pages: usize) -> bool {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};

    catch_unwind(AssertUnwindSafe(|| {
        let Ok(pdf) = Pdf::new(output.to_vec()) else {
            return false;
        };
        if pdf.pages().len() != pages {
            return false;
        }
        let cache = RenderCache::new();
        let interpreter = InterpreterSettings::default();
        let settings = RenderSettings::default();
        for page in pdf.pages().iter() {
            let (w, h) = page.render_dimensions();
            let scale = gate_scale(w, h);
            let pixmap = PixmapSettings {
                x_scale: scale,
                y_scale: scale,
                ..PixmapSettings::default()
            };
            drop(render(page, &cache, &interpreter, &settings, &pixmap));
        }
        true
    }))
    .unwrap_or(false)
}

/// The render gate's scale for a `w` × `h` point page: 1/8, or less so that
/// the longer side fits [`RENDER_MAX_SIDE`]. An infinite or NaN side (a
/// `/MediaBox` beyond `f32`) gives 0: a 0 × 0 pixmap, still a full
/// interpretation of the page.
fn gate_scale(w: f32, h: f32) -> f32 {
    let side = w.max(h);
    if !side.is_finite() || side <= 0.0 {
        return 0.0;
    }
    (RENDER_MAX_SIDE / side).min(0.125)
}

/// No finding of a `targeted` class outside the `partial` locations of its
/// own class (D-074, D-137): a location a pass reported `Partial` (an
/// unrecoverable, ambiguous or unsearched stream, a truncated tail) may
/// keep a finding of that pass's class, and no other.
fn clean_for(
    findings: &[Finding],
    targeted: &[CorruptionClass],
    partial: &[(CorruptionClass, Location)],
) -> bool {
    findings.iter().all(|f| match f.class {
        FindingKind::Corruption(class) if targeted.contains(&class) => {
            (partial.iter()).any(|(c, p)| *c == class && same_site(&f.location, p))
        }
        _ => true,
    })
}

/// Two locations name the same place (symmetric):
/// - the same object, spans aside;
/// - the same page index, when both name the same object or either names
///   none (a page-wide `Partial` covers every finding on that page);
/// - an object and a page location that names that object;
/// - otherwise, equal locations (the same span, or both the file).
///
/// Matching is per page, not per page × slot (D-074): [`Location`] has no
/// slot, so a `Partial` on one slot of a page excuses every slot there. The
/// locations must be in the output's object numbers and page indexes (the
/// remapped numbers emit writes), as the re-diagnosis reports them.
fn same_site(a: &Location, b: &Location) -> bool {
    match (a, b) {
        (Location::Object { id: x, .. }, Location::Object { id: y, .. }) => x == y,
        (
            Location::Page {
                index: i, obj: x, ..
            },
            Location::Page {
                index: j, obj: y, ..
            },
        ) => i == j && (x == y || x.is_none() || y.is_none()),
        (Location::Object { id, .. }, Location::Page { obj, .. })
        | (Location::Page { obj, .. }, Location::Object { id, .. }) => obj.as_ref() == Some(id),
        _ => a == b,
    }
}

/// The pages the first valid catalog of `graph` reaches, in document order;
/// empty when there is no valid catalog.
fn reached_pages(graph: &ObjectGraph) -> Vec<ObjId> {
    let Some(&root) = graph.catalog_candidates().first() else {
        return Vec::new();
    };
    let reachable = graph.reachable(root);
    graph
        .pages_in_doc_order()
        .into_iter()
        .filter(|p| reachable.contains(p))
        .collect()
}

// ── V1 ───────────────────────────────────────────────────────────────────

/// A page with nothing on it (D-059): no paint call at all, or none that
/// inks (a glyph that is not whitespace, a fill, a stroke, an image).
pub(crate) fn is_blank(paint: &PaintCounts) -> bool {
    let inked = u64::from(paint.glyphs_inked)
        + u64::from(paint.path_fills)
        + u64::from(paint.path_strokes)
        + u64::from(paint.images);
    paint.paint_ops() == 0 || inked == 0
}

/// The counts V1 compares, summed over a document's pages.
#[derive(Debug, Default)]
struct Tally {
    runs: u64,
    fills: u64,
    images: u64,
    glyphs: u64,
    blank: u32,
}

impl Tally {
    fn of(pages: &[PageText]) -> Tally {
        let mut t = Tally::default();
        for p in pages {
            t.runs +=
                u64::from(p.paint.glyph_runs_visible) + u64::from(p.paint.glyph_runs_invisible);
            t.fills += u64::from(p.paint.path_fills);
            t.images += u64::from(p.paint.images);
            t.glyphs += p.glyphs.len() as u64;
            t.blank += u32::from(is_blank(&p.paint));
        }
        t
    }
}

/// Whether hayro drew any glyph of `pages` with its fallback font: an
/// unkeyed glyph that is not a Type 3 one ([`GlyphItem::font`]).
///
/// [`GlyphItem::font`]: crate::pdf::text::GlyphItem::font
fn drew_with_fallback(pages: &[PageText]) -> bool {
    (pages.iter().flat_map(|p| &p.glyphs)).any(|g| g.font == FontKey::UNKEYED && !g.type3)
}

/// The glyphs V1 counts on one side (D-088): hayro's `drawn`, unless it drew
/// any of `pages`' glyphs with its fallback font, then the codes `shown`
/// counts, when [`Shown::codes_counted`] gives them.
fn glyphs_counted(pages: &[PageText], drawn: u64, shown: impl FnOnce() -> Shown) -> u64 {
    if !drew_with_fallback(pages) {
        return drawn;
    }
    shown().codes_counted().unwrap_or(drawn)
}

/// Raw bytes of the distinct `/Contents` streams of `pages`.
fn contents_bytes(carve: &CarveReport, graph: &ObjectGraph, pages: &[ObjId]) -> u64 {
    let winners = winning_copies(carve);
    let streams: BTreeSet<ObjId> = pages
        .iter()
        .flat_map(|&p| graph.page_content(carve, p, |_| None::<Cow<'_, [u8]>>))
        .filter(|piece| piece.via.is_empty())
        .map(|piece| piece.stream)
        .collect();
    streams
        .iter()
        .filter_map(|id| winners.get(id))
        .map(|&at| match &carve.objects[at].body {
            Body::Stream { data, .. } => data.end - data.start,
            _ => 0,
        })
        .sum()
}

/// Image XObjects among the carved objects (one per id).
fn image_objects(carve: &CarveReport) -> u64 {
    winning_copies(carve)
        .values()
        .filter(|&&at| carve.objects[at].kind == ObjectKind::Image)
        .count() as u64
}

/// The show operators (`Tj`, `TJ`, `'`, `"`) and the codes of the strings
/// they show, over every carved content stream and Form XObject (one copy
/// per id) and every orphan stream of those kinds, each stream read on its
/// own ([`Shown::read`]). Damaged Flate data counts as far as it decodes
/// (`salvage` must come from `carve` and `bytes`, `graph` from `carve`).
fn show_counts(
    bytes: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    salvage: &SalvageIndex,
) -> Shown {
    let shows_text =
        |kind: &ObjectKind| matches!(kind, ObjectKind::ContentStream | ObjectKind::Form);
    let source = CarveSource::new(carve, bytes);
    let mut decoded: Vec<Cow<'_, [u8]>> = winning_copies(carve)
        .iter()
        .filter(|&(_, &at)| shows_text(&carve.objects[at].kind))
        .filter_map(|(&id, _)| salvage.decoded(&source, id, DEFAULT_CAP).ok())
        .collect();
    for orphan in &carve.orphans {
        if let Orphan::Stream { dict, data, .. } = orphan
            && shows_text(orphan.kind())
        {
            let raw = usize::try_from(data.start)
                .ok()
                .zip(usize::try_from(data.end).ok())
                .and_then(|(s, e)| bytes.get(s..e));
            if let Some(Ok(out)) = raw.map(|raw| decode_chain(raw, &filters_of(dict), DEFAULT_CAP))
            {
                decoded.push(Cow::Owned(out));
            }
        }
    }
    let widths = CodeWidths::of_carve(carve, graph);
    let mut shown = Shown::default();
    for content in &decoded {
        shown.read(&widths, [content.as_ref()]);
    }
    shown
}

/// [`show_counts`] over what `pages` draw, as T-36 counts it: each page's
/// `/Contents`, read in order as one stream, and each Form XObject its `Do`
/// operators draw ([`ObjectGraph::page_content`]), read on its own. A
/// stream two pages draw counts on each. Annotation appearances and streams
/// no page draws are left out.
fn page_show_counts(
    bytes: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    salvage: &SalvageIndex,
    pages: &[ObjId],
) -> Shown {
    let source = CarveSource::new(carve, bytes);
    let decoded = |id: ObjId| salvage.decoded(&source, id, DEFAULT_CAP).ok();
    let widths = CodeWidths::of_carve(carve, graph);
    let mut shown = Shown::default();
    for &page in pages {
        let pieces = graph.page_content(carve, page, decoded);
        let (own, forms): (Vec<_>, Vec<_>) = pieces.iter().partition(|p| p.via.is_empty());
        let own: Vec<Cow<'_, [u8]>> = own.iter().filter_map(|p| decoded(p.stream)).collect();
        shown.read(&widths, own.iter().map(|c| c.as_ref()));
        for form in forms.iter().filter_map(|p| decoded(p.stream)) {
            shown.read(&widths, [form.as_ref()]);
        }
    }
    shown
}

/// The `q` levels whose font [`Shown::read`] restores on `Q`; deeper levels
/// are counted but not saved, so unbalanced input cannot grow the stack.
const SAVED_FONTS: usize = 64;

/// What [`show_counts`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    /// `Tj`, `TJ`, `'` and `"` operators.
    ops: u64,
    /// The codes of the strings they show.
    codes: u64,
    /// Every string was read at a known code width.
    exact: bool,
    /// Some string was read at two bytes a code.
    wide: bool,
}

impl Default for Shown {
    fn default() -> Shown {
        Shown {
            ops: 0,
            codes: 0,
            exact: true,
            wide: false,
        }
    }
}

impl Shown {
    /// The codes V1 counts in place of hayro's glyphs (D-088): `codes`, when
    /// every width is known and some string is two bytes a code. With one
    /// byte a code everywhere, hayro's fallback font already counts right.
    fn codes_counted(&self) -> Option<u64> {
        (self.exact && self.wide).then_some(self.codes)
    }

    /// Adds the show operators of `content`, a sequence of decoded streams
    /// read as one.
    ///
    /// A string holds its bytes over the code width of the font name the
    /// last `Tf` in force selected ([`CodeWidths::of`]), rounded down. `Tf`
    /// sets the graphics state, so `Q` restores the width the matching `q`
    /// saved. Before any `Tf` the width is the file's own, if its fonts
    /// agree. A string whose width is not known is read a byte a code and
    /// clears [`Shown::exact`]. Nothing is carried in from outside
    /// `content`: a form does not see its caller's font.
    fn read<'a>(&mut self, widths: &CodeWidths, content: impl IntoIterator<Item = &'a [u8]>) {
        let mut width = widths.of(None);
        let mut saved: Vec<Option<u64>> = Vec::new();
        let mut depth = 0usize;
        for op in content.into_iter().flat_map(content_ops) {
            match op.op {
                b"q" => {
                    if saved.len() < SAVED_FONTS {
                        saved.push(width);
                    }
                    depth += 1;
                }
                b"Q" => {
                    if depth == 0 {
                        continue;
                    }
                    depth -= 1;
                    if depth < saved.len()
                        && let Some(w) = saved.pop()
                    {
                        width = w;
                    }
                }
                b"Tf" => {
                    width = match op.operands.first() {
                        Some(Object::Name(name)) => widths.of(Some(name)),
                        _ => widths.of(None),
                    };
                }
                b"Tj" | b"TJ" | b"'" | b"\"" => {
                    self.ops += 1;
                    let strings: &[Object] = match op.operands.last() {
                        Some(Object::Array(items)) if op.op == b"TJ" => items,
                        Some(last) => std::slice::from_ref(last),
                        None => &[],
                    };
                    for s in strings.iter().filter_map(|o| o.as_str().ok()) {
                        self.exact &= width.is_some();
                        self.wide |= width.is_some_and(|w| w > 1);
                        self.codes += s.len() as u64 / width.unwrap_or(1);
                    }
                }
                _ => {}
            }
        }
    }
}

/// The code widths in bytes of a file's fonts (D-088): 2 for a Type 0 font
/// (as the font slots count them), 1 for any other. A Type 0 font whose
/// CMap is not Identity and mixes code lengths (`90ms-RKSJ-H`) is taken as
/// 2 bytes a code all the same, as the font slots take it.
#[derive(Debug, Default)]
struct CodeWidths {
    /// Every font name the `/Font` dictionary of some `/Resources` binds:
    /// its width, or `None` when it is bound to fonts of both widths.
    names: BTreeMap<Vec<u8>, Option<u64>>,
    /// The width every font of the file has, if they agree: its font
    /// dictionaries (a Type 0 font's descendant aside) and the fonts the
    /// names bind.
    file: Option<u64>,
}

impl CodeWidths {
    /// The widths of `carve` (`graph` built from it). Resources and fonts
    /// are read from every winning copy and every orphan; a font dictionary
    /// is never a stream, so stream dictionaries (a `FontFile3`'s among
    /// them) count only for their `/Resources`. A reference resolves as the
    /// rebuild resolves it ([`plan_ids`]), so a font whose header was
    /// stripped (C5) is still found through the orphan the rebuild matched
    /// to it.
    fn of_carve(carve: &CarveReport, graph: &ObjectGraph) -> CodeWidths {
        let remap = plan_ids(carve, graph);
        let resolve = |v| resolve_dict(carve, &remap, v);
        let holders: Vec<(&Dictionary, bool)> = (winning_copies(carve).into_values())
            .map(Held::Object)
            .chain((0..carve.orphans.len()).map(Held::Orphan))
            .filter_map(|held| Some((held_dict(carve, held)?, is_stream(carve, held))))
            .collect();
        let mut seen = BTreeSet::new();
        let mut names: BTreeMap<Vec<u8>, Option<u64>> = BTreeMap::new();
        for &(holder, stream) in &holders {
            if !stream {
                seen.extend(font_width(holder));
            }
            let Some(fonts) = (holder.get(b"Resources").ok().and_then(resolve))
                .and_then(|r| r.get(b"Font").ok().and_then(resolve))
            else {
                continue;
            };
            for (name, v) in fonts.iter() {
                let Some(width) = resolve(v).and_then(font_width) else {
                    continue;
                };
                seen.insert(width);
                names
                    .entry(name.clone())
                    .and_modify(|w| {
                        if *w != Some(width) {
                            *w = None;
                        }
                    })
                    .or_insert(Some(width));
            }
        }
        let file = match (seen.first(), seen.last()) {
            (Some(a), Some(b)) if a == b => Some(*a),
            _ => None,
        };
        CodeWidths { names, file }
    }

    /// The width a `Tf` of `name` selects (no name: before any `Tf`): the
    /// name's own, else the file's, else not known.
    fn of(&self, name: Option<&[u8]>) -> Option<u64> {
        name.and_then(|n| self.names.get(n).copied().flatten())
            .or(self.file)
    }
}

/// The code width of `dict` if it is a font a `Tf` can select: 2 for Type 0,
/// 1 for a simple font; `None` for anything else, a CID font (a Type 0
/// font's descendant) included. (`/OpenType` is a `FontFile3` stream's
/// subtype, not a font's.)
fn font_width(dict: &Dictionary) -> Option<u64> {
    match dict.get(b"Subtype").ok()?.as_name().ok()? {
        b"Type0" => Some(2),
        b"Type1" | b"MMType1" | b"TrueType" | b"Type3" => Some(1),
        _ => None,
    }
}

/// Whether a carved object or orphan is a stream.
fn is_stream(carve: &CarveReport, held: Held) -> bool {
    match held {
        Held::Object(at) => matches!(carve.objects[at].body, Body::Stream { .. }),
        Held::Orphan(i) => matches!(carve.orphans[i], Orphan::Stream { .. }),
    }
}

/// The dictionary of a carved object or orphan; a stream's dictionary.
fn held_dict(carve: &CarveReport, held: Held) -> Option<&Dictionary> {
    match held {
        Held::Object(at) => match &carve.objects[at].body {
            Body::Dict(d) | Body::Stream { dict: d, .. } => Some(d),
            _ => None,
        },
        Held::Orphan(i) => match &carve.orphans[i] {
            Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. } => Some(dict),
        },
    }
}

/// `v` as a dictionary: itself, or what the reference names under `remap`.
fn resolve_dict<'c>(
    carve: &'c CarveReport,
    remap: &IdRemap,
    v: &'c Object,
) -> Option<&'c Dictionary> {
    match v {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => held_dict(carve, remap.target(*id)?),
        _ => None,
    }
}

// ── V2 ───────────────────────────────────────────────────────────────────

/// V2 over the output's pages. A token is a run of mapped glyph text between
/// whitespace or unmapped glyphs; `script_consistency` is the share of the
/// tokens holding a letter whose letters are all of one [`Script`].
fn plausibility(pages: &[PageText]) -> Plausibility {
    let (mut glyphs, mut unmapped, mut fffd) = (0u64, 0u64, 0u64);
    let (mut tokens, mut consistent) = (0u64, 0u64);
    for page in pages {
        let mut token = Token::default();
        for g in &page.glyphs {
            glyphs += 1;
            let Some(text) = &g.text else {
                unmapped += 1;
                token.end(&mut tokens, &mut consistent);
                continue;
            };
            if text.contains('\u{FFFD}') {
                fffd += 1;
            }
            for c in text.chars() {
                if c.is_whitespace() {
                    token.end(&mut tokens, &mut consistent);
                } else {
                    token.add(c);
                }
            }
        }
        token.end(&mut tokens, &mut consistent);
    }
    Plausibility {
        unmapped_glyph: ratio(unmapped, glyphs),
        fffd: ratio(fffd, glyphs),
        script_consistency: ratio(consistent, tokens),
        dictionary_hit: None,
    }
}

/// The scripts of one token's letters so far.
#[derive(Debug, Default)]
struct Token {
    first: Option<Script>,
    mixed: bool,
}

impl Token {
    fn add(&mut self, c: char) {
        if !c.is_alphabetic() {
            return;
        }
        let s = Script::of(c);
        match self.first {
            None => self.first = Some(s),
            Some(f) if f != s => self.mixed = true,
            Some(_) => {}
        }
    }

    /// Counts the token if it held a letter, and starts the next.
    fn end(&mut self, tokens: &mut u64, consistent: &mut u64) {
        if self.first.is_some() {
            *tokens += 1;
            *consistent += u64::from(!self.mixed);
        }
        *self = Token::default();
    }
}

/// A coarse script of a letter, by Unicode block. Han, kana and bopomofo are
/// one script (Japanese mixes them in a word); a letter outside the blocks
/// below is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Greek,
    Cyrillic,
    Armenian,
    Hebrew,
    Arabic,
    Indic,
    Thai,
    Hangul,
    Cjk,
    Other,
}

impl Script {
    fn of(c: char) -> Script {
        match u32::from(c) {
            0x0041..=0x024F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF => Script::Latin,
            0xFB00..=0xFB06 | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A => Script::Latin,
            0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
            0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => {
                Script::Cyrillic
            }
            0x0530..=0x058F | 0xFB13..=0xFB17 => Script::Armenian,
            0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
            0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF => Script::Arabic,
            0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
            0x0900..=0x0DFF => Script::Indic,
            0x0E00..=0x0E7F => Script::Thai,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => Script::Hangul,
            0x2E80..=0x2FDF | 0x3005..=0x3007 | 0x3040..=0x31BF | 0x31F0..=0x31FF => Script::Cjk,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF66..=0xFF9F => Script::Cjk,
            0x20000..=0x3FFFF => Script::Cjk,
            _ => Script::Other,
        }
    }
}
