//! Resave (T-12a, TD §17.4): the repaired file, written through the writer
//! ([`crate::pdf::write`], whose rules (1)–(8) hold every output) from the
//! carve, the remap and the flat page tree.
//!
//! What goes in:
//! - every object [`IdRemap::objects`] numbers, in number order, with every
//!   reference rewritten to its output number (a reference to nothing
//!   written becomes `null`). An object with no readable value is written
//!   as `null`;
//! - each page of the [`PageTreePlan`] with its inheritable attributes
//!   pinned (`/MediaBox`, `/Resources`, `/CropBox`, `/Rotate`) and `/Parent`
//!   pointed at the one flat `/Pages` node, which is written after them; the
//!   catalog's `/Pages` points there too (a synthesized catalog when the
//!   carve had none);
//! - the trailer: `/Root`, `/ID` = the first 16 bytes of the input's
//!   SHA-256 twice, and `/Info` only when the input had a document
//!   information dictionary ([`info_object`]). Nothing adds `/Info`,
//!   `/CreationDate`, `/ModDate` or `/Producer`; an input's own are copied
//!   as found.
//!
//! Streams (rule 4): bytes are never recompressed. A stream is copied raw,
//! exactly as carved with its own `/Filter` chain, unless the C9 salvage
//! changed what its Flate stage holds and the output takes that salvage
//! ([`emit_resave`] takes every stream's; through [`emit_doc`], only the
//! streams the C9 pass named with [`RebuildDoc::swap_salvaged`]):
//! - `Repaired`: the Flate stage's input (the raw bytes through the filters
//!   before `/FlateDecode`) with the repair's edits applied; the earlier
//!   filters leave `/Filter`, the Flate stage, its `/DecodeParms` and every
//!   later filter stay;
//! - `ChecksumMismatch` or `Prefix`: the Flate stage's output with its
//!   predictor undone; `/Filter` keeps only the later filters, with their
//!   parameters re-indexed;
//! - `Clean`, `Unrecoverable` and `Unsearched`: raw (D-074: an unrecoverable
//!   stream is evidence and is kept as found).
//!
//! Each changed chain is a `(object, before, after)` line in
//! [`EmitNotes::filter_rewrites`]. The salvage index describes the last
//! stream copy of each id; a stream written from another copy (D-032's
//! winner can be an earlier one) is copied raw.
//!
//! Pages: a slot the C6 pass re-linked ([`RebuildDoc::relink_font`]) is
//! written into the page's own `/Resources`, an inline copy of the resources
//! in force with `/Font` holding the slot; every other page keeps its
//! resources as the page tree plan has them.
//!
//! Reals (rule 6, D-075): lopdf holds a real as `f32`, so a carved real that
//! does not survive `f32` is changed in the output. Each such token in an
//! object written is counted ([`EmitNotes::reals_narrowed`]) and the report
//! says so. A NaN or infinite real is written as `0` and logged.
// T-13a and T-14 are the first callers outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object};

use crate::engine::{LogLevel, Progress, RepairReport};
use crate::pdf::carver::{Body, CarveNote, CarveReport, Orphan};
use crate::pdf::graph::ObjectGraph;
use crate::pdf::lexer::{self, LexNote};
use crate::pdf::meta::info_object;
use crate::pdf::model::{ByteSpan, ObjId};
use crate::pdf::rebuild::{CatalogPlan, Held, IdRemap, PagePlan, PageTreePlan};
use crate::pdf::streams::salvage::{Salvage, SalvageIndex, flate_input};
use crate::pdf::streams::{Filter, UNRESOLVED_PARMS, filters_of};
use crate::pdf::write::{EmitError, Writer};

/// The version every output declares.
const VERSION: &str = "1.7";

/// What emit reads besides the carve, and what it reports.
pub(crate) struct EmitCtx<'a> {
    /// The input the carve's spans point into.
    pub(crate) bytes: &'a [u8],
    /// SHA-256 of `bytes`: the output's `/ID`.
    pub(crate) input_sha256: [u8; 32],
    /// The C9 outcomes, from the same carve.
    pub(crate) salvage: &'a SalvageIndex,
    pub(crate) sink: &'a mut dyn Progress,
    /// Filled by [`emit_resave`].
    pub(crate) notes: EmitNotes,
}

/// What an emit changed that the report records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EmitNotes {
    /// `(input object, /Filter before, /Filter after)`, in output order.
    pub(crate) filter_rewrites: Vec<(ObjId, String, String)>,
    /// Real tokens in the objects written that do not survive `f32`.
    pub(crate) reals_narrowed: u32,
    /// NaN or infinite reals written as `0`.
    pub(crate) non_finite_reals: u32,
}

impl EmitNotes {
    /// Puts these notes in `report`, the record of the candidate they came
    /// from.
    pub(crate) fn record(&self, report: &mut RepairReport) {
        report.filter_rewrites = self.filter_rewrites.clone();
        report.reals_narrowed = self.reals_narrowed;
    }
}

/// The output being built: the writer and the numbering it follows. Repair
/// passes (T-13a) edit it between [`RebuildDoc::new`] and [`emit_doc`].
pub(crate) struct RebuildDoc {
    w: Writer,
    remap: IdRemap,
    /// The streams whose C9 salvage is written (module docs, "Streams").
    swapped: BTreeSet<ObjId>,
    /// Per page output number, each re-linked slot and the input id of its
    /// font (module docs, "Pages").
    relinks: BTreeMap<u32, BTreeMap<Vec<u8>, ObjId>>,
}

impl RebuildDoc {
    /// An empty output that will hold what `remap` numbers.
    pub(crate) fn new(remap: IdRemap) -> Self {
        RebuildDoc {
            w: Writer::with_version(VERSION),
            remap,
            swapped: BTreeSet::new(),
            relinks: BTreeMap::new(),
        }
    }

    /// The numbering the output follows, with every [`Self::forget`] applied.
    pub(crate) fn remap(&self) -> &IdRemap {
        &self.remap
    }

    /// Leaves `held` out of the output; references to it become `null`
    /// ([`IdRemap::forget`]).
    pub(crate) fn forget(&mut self, held: Held) {
        self.remap.forget(held);
    }

    /// Writes stream `id` from its C9 salvage entry, when the salvage changed
    /// what its Flate stage holds (module docs, "Streams").
    pub(crate) fn swap_salvaged(&mut self, id: ObjId) {
        self.swapped.insert(id);
    }

    /// Maps `slot` to the input object `font` in the `/Resources` of the
    /// page written as `page` (module docs, "Pages").
    pub(crate) fn relink_font(&mut self, page: u32, slot: Vec<u8>, font: ObjId) {
        self.relinks.entry(page).or_default().insert(slot, font);
    }
}

/// The Resave output (module docs). `carve`, `graph`, `remap` and
/// `page_tree` must come from `ctx.bytes`, and `page_tree` from `remap`.
pub(crate) fn emit_resave(
    carve: &CarveReport,
    graph: &ObjectGraph,
    remap: &IdRemap,
    page_tree: &PageTreePlan,
    ctx: &mut EmitCtx<'_>,
) -> Result<Vec<u8>, EmitError> {
    let mut doc = RebuildDoc::new(remap.clone());
    doc.swapped.extend(ctx.salvage.by_obj.keys().copied());
    emit_doc(doc, carve, graph, page_tree, ctx)
}

/// [`emit_resave`] of `doc`, as the repair passes left it. `page_tree` must
/// come from the remap `doc` was made with; the passes never forget a page.
pub(crate) fn emit_doc(
    mut doc: RebuildDoc,
    carve: &CarveReport,
    graph: &ObjectGraph,
    page_tree: &PageTreePlan,
    ctx: &mut EmitCtx<'_>,
) -> Result<Vec<u8>, EmitError> {
    ctx.notes = EmitNotes::default();
    doc.copy_carved(carve, page_tree, ctx);
    doc.write_page_tree(page_tree);
    let info = info_object(ctx.bytes, carve, graph)
        .and_then(|at| doc.remap.number_of(Held::Object(at)))
        .map(|n| (n, 0));
    doc.w.trailer((page_tree.root, 0), ctx.input_sha256, info);

    let non_finite = doc.w.non_finite_reals();
    ctx.notes.non_finite_reals = non_finite;
    if non_finite > 0 {
        ctx.sink.log(
            LogLevel::Warn,
            format!("{non_finite} NaN or infinite numbers were written as 0"),
        );
    }
    doc.w.finish()
}

impl RebuildDoc {
    /// Every numbered carved object, pages pinned and the catalog rewired.
    fn copy_carved(&mut self, carve: &CarveReport, tree: &PageTreePlan, ctx: &mut EmitCtx<'_>) {
        let pages: BTreeMap<u32, &PagePlan> = tree.pages.iter().map(|p| (p.id, p)).collect();
        let last_streams = last_stream_copies(carve);
        for (n, held) in self.remap.objects() {
            let mut object = match held {
                Held::Object(at) => {
                    let obj = &carve.objects[at];
                    ctx.notes.reals_narrowed = ctx.notes.reals_narrowed.saturating_add(narrowed(
                        obj.notes.iter().filter_map(|n| match n {
                            CarveNote::Lex(l) => Some(l),
                            _ => None,
                        }),
                    ));
                    match &obj.body {
                        Body::Dict(d) => Object::Dictionary(d.clone()),
                        Body::Primitive(v) => v.clone(),
                        Body::Unparsed => Object::Null,
                        Body::Stream { dict, data, .. } => {
                            let salvaged = last_streams.get(&obj.declared_id) == Some(&at);
                            self.stream(n, obj.declared_id, salvaged, dict, *data, ctx);
                            continue;
                        }
                    }
                }
                Held::Orphan(at) => {
                    let orphan = &carve.orphans[at];
                    ctx.notes.reals_narrowed = ctx
                        .notes
                        .reals_narrowed
                        .saturating_add(orphan_narrowed(ctx.bytes, orphan.span()));
                    match orphan {
                        Orphan::Dict { dict, .. } => Object::Dictionary(dict.clone()),
                        Orphan::Stream { dict, data, .. } => {
                            let mut dict = dict.clone();
                            self.remap.rewrite_dict(&mut dict);
                            self.w
                                .add_stream_raw(n, dict, slice(ctx.bytes, *data).to_vec());
                            continue;
                        }
                    }
                }
            };
            self.remap.rewrite(&mut object);
            if let Object::Dictionary(d) = &mut object {
                if let Some(page) = pages.get(&n) {
                    self.pin_page(d, page, tree.pages_id, carve);
                } else if n == tree.root && matches!(tree.catalog, CatalogPlan::Reuse(_)) {
                    d.set("Pages", Object::Reference((tree.pages_id, 0)));
                }
            }
            self.w.add(n, object);
        }
    }

    /// The page's inheritable attributes, resolved by T-10, written on it,
    /// with the fonts re-linked to it.
    fn pin_page(&self, d: &mut Dictionary, page: &PagePlan, pages_id: u32, carve: &CarveReport) {
        d.set("Type", Object::Name(b"Page".to_vec()));
        d.set("Parent", Object::Reference((pages_id, 0)));
        d.set("MediaBox", Object::Array(page.mediabox.to_vec()));
        let mut set = |key: &str, value: Option<Object>| match value {
            Some(mut v) => {
                self.remap.rewrite(&mut v);
                d.set(key, v);
            }
            None => {
                d.remove(key.as_bytes());
            }
        };
        let resources = match self.relinks.get(&page.id) {
            Some(slots) => Some(Object::Dictionary(self.with_fonts(
                carve,
                page.resources.as_ref(),
                slots,
            ))),
            None => page.resources.clone(),
        };
        set("Resources", resources);
        set(
            "CropBox",
            page.cropbox.clone().map(|b| Object::Array(b.to_vec())),
        );
        set("Rotate", page.rotate.map(Object::Integer));
    }

    /// A copy of `resources` (carved, inline or a reference) whose `/Font`
    /// maps each of `slots` to its font, still in the carve's ids.
    fn with_fonts(
        &self,
        carve: &CarveReport,
        resources: Option<&Object>,
        slots: &BTreeMap<Vec<u8>, ObjId>,
    ) -> Dictionary {
        let resolve = |v: &Object| -> Option<Dictionary> {
            match v {
                Object::Dictionary(d) => Some(d.clone()),
                Object::Reference(id) => carved_dict(carve, self.remap.target(*id)?).cloned(),
                _ => None,
            }
        };
        let mut out = resources.and_then(resolve).unwrap_or_default();
        let mut fonts = (out.get(b"Font").ok())
            .and_then(resolve)
            .unwrap_or_default();
        for (slot, &font) in slots {
            fonts.set(slot.clone(), Object::Reference(font));
        }
        out.set("Font", Object::Dictionary(fonts));
        out
    }

    /// The flat `/Pages` node, and the catalog when none was carved.
    fn write_page_tree(&mut self, tree: &PageTreePlan) {
        self.w
            .add(tree.pages_id, Object::Dictionary(tree.pages_dict()));
        if tree.catalog == CatalogPlan::Synthesize {
            let mut d = Dictionary::new();
            d.set("Type", Object::Name(b"Catalog".to_vec()));
            d.set("Pages", Object::Reference((tree.pages_id, 0)));
            self.w.add(tree.root, Object::Dictionary(d));
        }
    }

    /// One carved stream (module docs, "Streams"). `salvaged` says the C9
    /// index describes this copy of `id`.
    fn stream(
        &mut self,
        n: u32,
        id: ObjId,
        salvaged: bool,
        carved: &Dictionary,
        data: ByteSpan,
        ctx: &mut EmitCtx<'_>,
    ) {
        let raw = slice(ctx.bytes, data);
        let mut dict = carved.clone();
        self.remap.rewrite_dict(&mut dict);
        let entry = (salvaged && self.swapped.contains(&id))
            .then(|| ctx.salvage.by_obj.get(&id))
            .flatten();
        let rewritten = entry.and_then(|e| match &e.salvage {
            Salvage::Repaired { edits, .. } => {
                let mut input = flate_input(carved, raw)?;
                for &(at, old, new) in edits {
                    let b = input.get_mut(at).filter(|b| **b == old)?;
                    *b = new;
                }
                let (chain, flate_at) = Chain::of(&dict)?;
                let after = chain.tail(flate_at);
                Some(((chain, after), input))
            }
            Salvage::ChecksumMismatch { data } | Salvage::Prefix { data, .. } => {
                let (chain, flate_at) = Chain::of(&dict)?;
                let after = chain.tail(flate_at + 1);
                Some(((chain, after), data.clone()))
            }
            Salvage::Clean { .. } | Salvage::Unrecoverable | Salvage::Unsearched { .. } => None,
        });
        match rewritten {
            Some(((before, after), bytes)) => {
                if before != after {
                    ctx.notes
                        .filter_rewrites
                        .push((id, before.text(), after.text()));
                }
                after.set_on(&mut dict);
                self.w.add_stream_raw(n, dict, bytes);
            }
            None => self.w.add_stream_raw(n, dict, raw.to_vec()),
        }
    }
}

/// The dictionary `held` carries: a carved dictionary or stream's, or an
/// orphan's.
fn carved_dict(carve: &CarveReport, held: Held) -> Option<&Dictionary> {
    match held {
        Held::Object(at) => match &carve.objects.get(at)?.body {
            Body::Dict(d) | Body::Stream { dict: d, .. } => Some(d),
            Body::Primitive(_) | Body::Unparsed => None,
        },
        Held::Orphan(at) => match carve.orphans.get(at)? {
            Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. } => Some(dict),
        },
    }
}

/// For each id, the index in `carve.objects` of its last stream copy: the
/// copy the salvage index describes (T-08's `CarveSource`).
fn last_stream_copies(carve: &CarveReport) -> BTreeMap<ObjId, usize> {
    let mut last = BTreeMap::new();
    for (at, obj) in carve.objects.iter().enumerate() {
        if let Body::Stream { .. } = obj.body {
            last.insert(obj.declared_id, at);
        }
    }
    last
}

/// The bytes of `span` in `bytes`; empty when it lies outside them.
fn slice(bytes: &[u8], span: ByteSpan) -> &[u8] {
    let range = usize::try_from(span.start)
        .ok()
        .zip(usize::try_from(span.end).ok());
    range.and_then(|(s, e)| bytes.get(s..e)).unwrap_or(&[])
}

fn narrowed<'n>(notes: impl Iterator<Item = &'n LexNote>) -> u32 {
    let n = notes
        .filter(|n| matches!(n, LexNote::RealNarrowed { .. }))
        .count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// An orphan carries no lexer notes, so its dictionary is read again.
fn orphan_narrowed(bytes: &[u8], span: ByteSpan) -> u32 {
    let end = usize::try_from(span.end).map_or(bytes.len(), |e| e.min(bytes.len()));
    let Ok(start) = usize::try_from(span.start) else {
        return 0;
    };
    match lexer::parse_value(&bytes[..end], start, 0) {
        Ok(p) => narrowed(p.notes.iter()),
        Err(_) => 0,
    }
}

// ── filter chains ────────────────────────────────────────────────────────

/// A stream's `/Filter` chain as written: each stage's name as written and
/// its `/DecodeParms` entry.
#[derive(Debug, Clone, PartialEq)]
struct Chain(Vec<(Vec<u8>, Option<Object>)>);

impl Chain {
    /// The chain of `dict` and where its first Flate stage is, when the chain
    /// is read from `/Filter`, names every stage and has a Flate stage.
    fn of(dict: &Dictionary) -> Option<(Chain, usize)> {
        let names: Vec<Vec<u8>> = match dict.get(b"Filter").ok()? {
            Object::Name(n) => vec![n.clone()],
            Object::Array(items) => items
                .iter()
                .filter(|o| !matches!(o, Object::Null))
                .map(|o| o.as_name().ok().map(<[u8]>::to_vec))
                .collect::<Option<_>>()?,
            _ => return None,
        };
        let parsed = filters_of(dict);
        if parsed.len() != names.len() {
            return None;
        }
        let flate_at = parsed.iter().position(|(f, _)| *f == Filter::Flate)?;
        let stages = names
            .into_iter()
            .zip(parsed)
            .map(|(name, (_, parms))| {
                let parms = parms.map(|p| match p.get(UNRESOLVED_PARMS) {
                    Ok(v) => v.clone(),
                    Err(_) => Object::Dictionary(p),
                });
                (name, parms)
            })
            .collect();
        Some((Chain(stages), flate_at))
    }

    /// The stages from `from` on.
    fn tail(&self, from: usize) -> Chain {
        Chain(self.0[from..].to_vec())
    }

    /// `/Filter` as PDF syntax: `/A`, `[/A /B]`, or `[]` for no filter.
    fn text(&self) -> String {
        let names: Vec<String> = self
            .0
            .iter()
            .map(|(n, _)| format!("/{}", String::from_utf8_lossy(n)))
            .collect();
        match names.as_slice() {
            [one] => one.clone(),
            _ => format!("[{}]", names.join(" ")),
        }
    }

    /// Writes this chain as `dict`'s `/Filter` and `/DecodeParms`: a name
    /// and a dictionary for one stage, arrays (with `null` for a stage
    /// without parameters) for several, and neither key for none.
    fn set_on(&self, dict: &mut Dictionary) {
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        match self.0.as_slice() {
            [] => {}
            [(name, parms)] => {
                dict.set("Filter", Object::Name(name.clone()));
                if let Some(p) = parms {
                    dict.set("DecodeParms", p.clone());
                }
            }
            stages => {
                let names = stages.iter().map(|(n, _)| Object::Name(n.clone()));
                dict.set("Filter", Object::Array(names.collect()));
                if stages.iter().any(|(_, p)| p.is_some()) {
                    let parms = stages
                        .iter()
                        .map(|(_, p)| p.clone().unwrap_or(Object::Null));
                    dict.set("DecodeParms", Object::Array(parms.collect()));
                }
            }
        }
    }
}
