//! Repair passes and generate-and-validate (T-13a; SE Q1 Layers B and C,
//! SE Q2, D-008).
//!
//! [`generate_and_validate`] builds every candidate toolpath the plan lists,
//! verifies each ([`verify`]) and keeps the best by the lexicographic
//! [`SelectionKey`]. The candidate set is the plan's, never cut by time.
//!
//! **Building a candidate.** `Resave` is the T-10 rebuild (renumbering, the
//! flat page tree) emitted by T-12a, with the passes run on the
//! [`RebuildDoc`] in between. `TemplateAssemble` is skipped with a log until
//! T-30 builds it. A candidate whose emit fails is logged and not verified.
//! A stream is written from its salvage only when the C9 pass swapped it in;
//! with no C9 pass every stream is copied exactly as carved.
//!
//! **Passes**, one [`RepairPass`] per class, in the order C1, C2, C3, then
//! [`CorruptionClass::REPAIR_ORDER`]. A pass runs when the file has a
//! finding of its class and `RepairOptions.passes` selects it; a class with
//! findings that is not selected, or has no pass in this version, gets a
//! `Skipped` report and a log line. The classes whose passes ran are the
//! ones verification targets; the findings of a pass that reported
//! `Partial` may stay in the output (D-074).
//!
//! `RepairOptions.passes` restricts the reports, the verification targets
//! and the C10 drop. It does not restrict the `Resave` rebuild itself: that
//! always writes a new header, cross-reference table and trailer, every
//! orphan and the flat page tree. So a C1–C5 class left out of `passes` is
//! reported `Skipped`, yet the output is usually rebuilt past it, and its
//! finding may be gone on re-diagnosis. The `Skipped` reason and the log
//! line say so.
//! - **C1–C3**: re-emission fixes them (a new header, a classic table and a
//!   trailer); one action per finding, "resolved by re-emission".
//! - **C10**: a cut stream is written with the bytes before the cut and a
//!   `/Length` to match (clamped); a cut object nothing reachable names is
//!   dropped. Always `Partial`: what came after the cut is gone. Reachable
//!   means from what the output keeps (the catalog the rebuild chose, every
//!   page of the flat tree, the trailer's `/Info`, every orphan's
//!   references), through the carve's references as the rebuild resolves
//!   them, its reference matches included.
//! - **C5**: each headerless object is written under the number the
//!   rebuild gave it, and named by the references the rebuild matched to it.
//! - **C4**: the flat page tree is written: the catalog points at one
//!   `/Pages` node holding every page, in document order, attributes pinned.
//! - **C9** (T-13b; D-041, D-074): each damaged stream's salvage is swapped
//!   in, and its action carries the stream's `SalvageGrade` when it has
//!   one. `Exact` and `Accepted` repairs are `Fixed`; an `Ambiguous` one
//!   writes the first survivor in the pinned order and is
//!   `Partial("ambiguous: n candidate repairs, see report")`; a
//!   `ChecksumMismatch` or `Prefix` stream is written decoded and is
//!   `Partial`; an `Unrecoverable` or `Unsearched` one is written as carved
//!   and is `Partial("unrecoverable stream")` or `Partial("stream over
//!   max_search_stream, not searched")`. The pass fills the report's
//!   `c9_summary` (`accepted` apart from `exact`) and `c9_survivors` (every
//!   repaired stream's surviving edit lists). A `C9-outside-stream` keyword is
//!   resolved by re-emission.
//! - **C6** (T-13b; RR change #3), per (page, slot): the slot is re-linked
//!   in the page's `/Resources` to one of the fonts no reference from the
//!   catalog reaches (diagnose's re-link candidates, [`orphan_fonts`]). A
//!   candidate whose `/Widths` survive must have a nonzero width for every
//!   code the page shows through the slot, or it is no match. The rest rank
//!   by: `/Name` equal to the slot first, then widths that agree over widths
//!   unknown, then the nearest object to the page in the file, then the
//!   lowest id; each page takes a font for at most one slot. `/BaseFont` is
//!   never read: a `CIDFont+Fn` name matches nothing. A slot with no match
//!   is left as it was, the pass is `Partial` and the slot gets a
//!   `FontPick` escalation (unless diagnose found no candidate at all: the
//!   plan escalated that one already).
//!
//! A [`RepairAction`] names an object by its declared id in the input; an
//! orphan, which has none, by the number the rebuild gave it; a finding
//! about the whole file by `(0, 0)`.
//!
//! A `Partial` pass excuses its findings' locations from verification's
//! "clean for the targeted classes" (D-074): the locations the pass left
//! partial when it names them ([`PassNotes::partial`]), every finding of
//! its class otherwise.
//!
//! **Selection** (SE Q2; fixed tiers in v1, D-008). Candidates compare
//! lexicographically by [`compare`]:
//! 1. V0: every gate passes;
//! 2. V1, retention: fewer pages gone blank, then more of the glyphs, text
//!    operators, images, path fills and content bytes, each as a fraction of
//!    the baseline and counted at most whole (keeping more than the input
//!    showed is not better than keeping all of it);
//! 3. V2, plausibility: fewer U+FFFD glyphs, fewer unmapped glyphs, more
//!    script-consistent tokens, a higher dictionary hit rate;
//! 4. V3, preservation ([`Preservation`]): more of outlines, AcroForm, XMP
//!    metadata and `/Info` kept, then more annotations;
//! 5. V4: the plan's prior first, then the smaller output, then plan order.
//!
//! The best candidate is the output only when it passes V0; otherwise there
//! is no output and a `NoCandidatePassed` escalation says so.
// T-14 is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object};

use crate::engine::{
    ByteEdit, C9Summary, Cancelled, CandidateReport, Escalation, EscalationKind, FontDb, Interact,
    LogLevel, PassOutcome, PassReport, Progress, RepairAction, RepairOptions, RepairPlan,
    RepairReport, Toolpath,
};
use crate::pdf::carver::{Body, CarveReport, Orphan, carve};
use crate::pdf::diagnose::{OUTSIDE_STREAM, orphan_fonts};
use crate::pdf::emit::{EmitCtx, EmitNotes, RebuildDoc, emit_doc};
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::lexer;
use crate::pdf::meta::info_object;
use crate::pdf::model::{
    ByteSpan, CorruptionClass, Evidence, Finding, FindingKind, InteractionKind, Location,
    MetricValue, ObjId, Ratio, Repairability,
};
use crate::pdf::rebuild::{
    BoxSource, CatalogPlan, Held, IdRemap, PageTreePlan, plan_ids, rebuild_page_tree,
};
use crate::pdf::streams::salvage::{CarveSource, Grade, Salvage, SalvageIndex};
use crate::pdf::streams::{DEFAULT_CAP, content_ops};
use crate::pdf::verify::{Plausibility, Retention, Verification, baseline, verify};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped,
    C6FontMapLost, C9ZlibTampered, C10Truncated,
};

/// How a [`RepairAction`] names the whole file.
const FILE: ObjId = (0, 0);

// ── passes ───────────────────────────────────────────────────────────────

/// One class's repair, run on the output being built.
pub(crate) trait RepairPass {
    fn class(&self) -> CorruptionClass;
    /// Repairs what `findings` (all of [`Self::class`]) describe in
    /// `ctx.doc`, and says what it did.
    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport;
}

/// What a pass reads, and the output it edits. `remap` and `page_tree` are
/// the rebuild's plan; `doc` holds the numbering as the passes so far left
/// it. Cancellation is read through `sink` ([`Self::cancelled`]).
pub(crate) struct RepairCtx<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) carve: &'a CarveReport,
    pub(crate) graph: &'a ObjectGraph,
    pub(crate) remap: &'a IdRemap,
    pub(crate) page_tree: &'a PageTreePlan,
    pub(crate) doc: &'a mut RebuildDoc,
    pub(crate) salvage: &'a SalvageIndex,
    // Read by the font passes (T-30).
    #[allow(dead_code)]
    pub(crate) fonts: &'a FontDb,
    #[allow(dead_code)]
    pub(crate) ask: &'a mut dyn Interact,
    pub(crate) sink: &'a mut dyn Progress,
    /// What the passes report beside their [`PassReport`]s.
    pub(crate) notes: PassNotes,
}

/// What the passes of one candidate report beside their [`PassReport`]s.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PassNotes {
    /// Escalations a pass raised (C6: a slot with no match).
    pub(crate) escalations: Vec<Escalation>,
    /// The C9 pass's counts (D-041).
    pub(crate) c9_summary: C9Summary,
    /// Every repaired stream's surviving edit lists, by input id.
    pub(crate) c9_survivors: Vec<(ObjId, Vec<Vec<ByteEdit>>)>,
    /// The input locations a `Partial` pass left partial, by class (module
    /// docs). A class with none listed excuses all of its findings.
    pub(crate) partial: Vec<(CorruptionClass, Location)>,
}

impl RepairCtx<'_> {
    /// The job was cancelled ([`Progress::cancelled`]).
    pub(crate) fn cancelled(&self) -> bool {
        self.sink.cancelled()
    }

    /// The input's id for `held`: its declared id, or for an orphan the
    /// number the rebuild gave it.
    fn input_id(&self, held: Held) -> ObjId {
        match held {
            Held::Object(at) => self.carve.objects[at].declared_id,
            Held::Orphan(_) => (self.remap.number_of(held).unwrap_or(0), 0),
        }
    }
}

/// The passes this version has.
const PASSES: [&dyn RepairPass; 8] = [
    &ReEmitted(C1Header),
    &ReEmitted(C2XrefMissing),
    &ReEmitted(C3TrailerDamaged),
    &Salvaged,
    &Truncation,
    &Orphans,
    &PageTree,
    &Relink,
];

/// `class`'s pass, when this version has one.
fn pass_for(class: CorruptionClass) -> Option<&'static dyn RepairPass> {
    PASSES.into_iter().find(|p| p.class() == class)
}

/// The order passes run in: C1–C3, then [`CorruptionClass::REPAIR_ORDER`].
fn pass_order() -> impl Iterator<Item = CorruptionClass> {
    [C1Header, C2XrefMissing, C3TrailerDamaged]
        .into_iter()
        .chain(CorruptionClass::REPAIR_ORDER)
}

/// C1–C3: the re-emission is the repair.
struct ReEmitted(CorruptionClass);

impl RepairPass for ReEmitted {
    fn class(&self) -> CorruptionClass {
        self.0
    }

    fn repair(&self, _ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        let actions = findings
            .iter()
            .map(|f| RepairAction {
                object: match f.location {
                    Location::Object { id, .. } => id,
                    _ => FILE,
                },
                what: format!("{}: resolved by re-emission", f.id),
                grade: None,
            })
            .collect();
        PassReport {
            class: self.0,
            outcome: PassOutcome::Fixed,
            actions,
        }
    }
}

/// C10: clamp the cut streams, drop the cut objects nothing reaches.
struct Truncation;

impl RepairPass for Truncation {
    fn class(&self) -> CorruptionClass {
        C10Truncated
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        let reached = reachable(ctx.bytes, ctx.carve, ctx.graph, ctx.remap, ctx.page_tree);
        let mut actions = Vec::new();
        let mut dropped = 0u32;
        for (held, cut) in cut_objects(ctx.bytes, ctx.carve, ctx.doc.remap()) {
            let object = ctx.input_id(held);
            if !reached.contains(&held) {
                ctx.doc.forget(held);
                dropped += 1;
                actions.push(RepairAction {
                    object,
                    what: "cut by the end of the file and named by nothing reachable: dropped"
                        .to_owned(),
                    grade: None,
                });
            } else if let Some(Cut { kept, declared }) = cut {
                let declared = declared.map_or(String::new(), |d| format!(" of {d} declared"));
                actions.push(RepairAction {
                    object,
                    what: format!(
                        "stream data clamped at the end of the file: {kept} bytes kept{declared}"
                    ),
                    grade: None,
                });
            }
        }
        let mut why = vec!["the file was cut short".to_owned()];
        why.extend(findings.iter().filter_map(kept_fraction).map(|r| {
            let percent = (u128::from(r.num) * 100)
                .checked_div(u128::from(r.den))
                .unwrap_or(0);
            format!("{} of {} bytes ({percent}%) kept", r.num, r.den)
        }));
        if dropped > 0 {
            why.push(format!(
                "cut objects named by nothing reachable dropped: {dropped}"
            ));
        }
        why.push("what came after the cut is gone".to_owned());
        PassReport {
            class: C10Truncated,
            outcome: PassOutcome::Partial(why.join("; ")),
            actions,
        }
    }
}

/// A cut stream's data: the bytes kept and the `/Length` it declared.
#[derive(Debug, Clone, Copy)]
struct Cut {
    kept: u64,
    declared: Option<u64>,
}

/// The written objects the end of the file cut: each reaches the last
/// non-blank byte and does not end with `endobj`. A stream among them
/// carries its [`Cut`]. In byte order.
fn cut_objects(bytes: &[u8], carve: &CarveReport, remap: &IdRemap) -> Vec<(Held, Option<Cut>)> {
    let end = bytes
        .iter()
        .rposition(|b| !lexer::is_ws(*b))
        .map_or(0, |i| i as u64 + 1);
    let mut cut: Vec<(u64, Held, Option<Cut>)> = remap
        .objects()
        .into_iter()
        .filter_map(|(_, held)| {
            let (span, stream) = match held {
                Held::Object(at) => {
                    let o = &carve.objects[at];
                    let stream = match &o.body {
                        Body::Stream { dict, data, .. } => Some((dict, *data)),
                        _ => None,
                    };
                    (o.span, stream)
                }
                Held::Orphan(i) => match &carve.orphans[i] {
                    Orphan::Dict { span, .. } => (*span, None),
                    Orphan::Stream {
                        span, dict, data, ..
                    } => (*span, Some((dict, *data))),
                },
            };
            let whole = slice(bytes, span).trim_ascii_end().ends_with(b"endobj");
            if span.end < end || whole {
                return None;
            }
            let cut = stream.map(|(dict, data)| Cut {
                kept: data.end.saturating_sub(data.start),
                declared: dict
                    .get(b"Length")
                    .ok()
                    .and_then(|v| v.as_i64().ok())
                    .and_then(|n| u64::try_from(n).ok()),
            });
            Some((span.start, held, cut))
        })
        .collect();
    cut.sort_by_key(|&(start, held, _)| (start, held));
    cut.into_iter().map(|(_, h, c)| (h, c)).collect()
}

/// Everything the output's roots reach through the rebuild's references
/// (module docs, C10). The roots are what the output keeps whatever else
/// happens: the catalog the rebuild chose, every page of the flat tree, the
/// `/Info` dictionary the trailer will name ([`info_object`]), every other
/// catalog and page the graph found, every orphan page and every reference
/// an orphan holds. The walk resolves each reference with [`IdRemap::target`],
/// so the rebuild's reference matches count as references.
fn reachable(
    bytes: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    remap: &IdRemap,
    tree: &PageTreePlan,
) -> BTreeSet<Held> {
    let mut ids: Vec<ObjId> = graph.catalog_candidates();
    ids.extend(graph.pages_in_doc_order());
    if let CatalogPlan::Reuse(id) = tree.catalog {
        ids.push(id);
    }
    for orphan in &carve.orphans {
        let dict = match orphan {
            Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. } => dict,
        };
        dict.iter().for_each(|(_, v)| references(v, &mut ids));
    }
    let mut queue: Vec<Held> = ids.into_iter().filter_map(|id| remap.target(id)).collect();
    // What each output number holds, sorted by number.
    let numbered = remap.objects();
    queue.extend(tree.pages.iter().filter_map(|p| {
        let i = numbered.binary_search_by_key(&p.id, |&(n, _)| n).ok()?;
        Some(numbered[i].1)
    }));
    queue.extend(info_object(bytes, carve, graph).map(Held::Object));
    // An orphan page is a page.
    let is_page =
        |d: &Dictionary| d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Page");
    queue.extend(
        carve
            .orphans
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o, Orphan::Dict { dict, .. } if is_page(dict)))
            .map(|(i, _)| Held::Orphan(i)),
    );
    let mut seen = BTreeSet::new();
    let mut refs = Vec::new();
    while let Some(held) = queue.pop() {
        if !seen.insert(held) {
            continue;
        }
        refs.clear();
        match held {
            Held::Object(at) => match carve.objects.get(at).map(|o| &o.body) {
                Some(Body::Dict(d) | Body::Stream { dict: d, .. }) => {
                    d.iter().for_each(|(_, v)| references(v, &mut refs));
                }
                Some(Body::Primitive(v)) => references(v, &mut refs),
                Some(Body::Unparsed) | None => {}
            },
            // Every orphan's references are roots already.
            Held::Orphan(_) => {}
        }
        queue.extend(refs.iter().filter_map(|&id| remap.target(id)));
    }
    seen
}

/// Every reference in `v`, appended to `out`.
fn references(v: &Object, out: &mut Vec<ObjId>) {
    match v {
        Object::Reference(id) => out.push(*id),
        Object::Array(a) => a.iter().for_each(|o| references(o, out)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| references(o, out)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| references(o, out)),
        _ => {}
    }
}

/// A finding's `kept_fraction`, when it has one.
fn kept_fraction(f: &Finding) -> Option<Ratio> {
    f.evidence.iter().find_map(|e| match e {
        Evidence::Metric {
            name,
            value: MetricValue::Ratio(r),
        } if name == "kept_fraction" => Some(*r),
        _ => None,
    })
}

/// C5: each headerless object is placed under its rebuild number.
struct Orphans;

impl RepairPass for Orphans {
    fn class(&self) -> CorruptionClass {
        C5ObjectTagStripped
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        let by_span: BTreeMap<(u64, u64), usize> = (ctx.carve.orphans.iter().enumerate())
            .map(|(i, o)| ((o.span().start, o.span().end), i))
            .collect();
        let mut actions = Vec::new();
        for f in findings {
            let Location::Span(span) = f.location else {
                continue;
            };
            let Some(&i) = by_span.get(&(span.start, span.end)) else {
                continue;
            };
            let held = Held::Orphan(i);
            // Dropped as a cut object by the C10 pass, which said so.
            let Some(n) = ctx.doc.remap().number_of(held) else {
                continue;
            };
            let shape = match ctx.carve.orphans[i] {
                Orphan::Dict { .. } => "dictionary",
                Orphan::Stream { .. } => "stream",
            };
            let mut what = format!(
                "the headerless {shape} at byte {} written as {n} 0 obj",
                span.start
            );
            let named: Vec<String> = ctx
                .remap
                .reconciled()
                .iter()
                .filter(|r| r.target == held)
                .map(|r| format!("{} {} R", r.missing.0, r.missing.1))
                .collect();
            if !named.is_empty() {
                what.push_str(&format!(", for every reference to {}", named.join(", ")));
            }
            actions.push(RepairAction {
                object: (n, 0),
                what,
                grade: None,
            });
        }
        PassReport {
            class: C5ObjectTagStripped,
            outcome: PassOutcome::Fixed,
            actions,
        }
    }
}

/// C4: the flat page tree.
struct PageTree;

impl RepairPass for PageTree {
    fn class(&self) -> CorruptionClass {
        C4PageTreeBroken
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, _findings: &[Finding]) -> PassReport {
        let tree = ctx.page_tree;
        let pages_id = tree.pages_id;
        let mut actions = vec![match tree.catalog {
            CatalogPlan::Reuse(id) => RepairAction {
                object: id,
                what: format!("the catalog's /Pages points at the rebuilt tree, {pages_id} 0 obj"),
                grade: None,
            },
            CatalogPlan::Synthesize => RepairAction {
                object: FILE,
                what: format!(
                    "a catalog was written as {} 0 obj over the rebuilt tree, {pages_id} 0 obj",
                    tree.root
                ),
                grade: None,
            },
        }];
        let total = tree.pages.len();
        // What each output number holds, sorted by number.
        let numbered = ctx.remap.objects();
        for (k, page) in tree.pages.iter().enumerate() {
            let object = numbered
                .binary_search_by_key(&page.id, |&(n, _)| n)
                .map_or((page.id, 0), |i| ctx.input_id(numbered[i].1));
            let mediabox = match page.source {
                BoxSource::Own => "its own",
                BoxSource::Inherited => "inherited from its parent",
                BoxSource::ModalSibling => "that of most other pages",
                BoxSource::Default => "the default page size",
            };
            actions.push(RepairAction {
                object,
                what: format!(
                    "page {} of {total} placed under {pages_id} 0 obj; MediaBox {mediabox}",
                    k + 1
                ),
                grade: None,
            });
        }
        let outcome = if total == 0 {
            PassOutcome::Partial("no page was found".to_owned())
        } else {
            PassOutcome::Fixed
        };
        PassReport {
            class: C4PageTreeBroken,
            outcome,
            actions,
        }
    }
}

/// C9: swap each damaged stream's salvage in (module docs).
struct Salvaged;

impl RepairPass for Salvaged {
    fn class(&self) -> CorruptionClass {
        C9ZlibTampered
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        let mut actions = Vec::new();
        let mut partial = Vec::new();
        let mut summary = C9Summary::default();
        let mut survivors_of = Vec::new();
        for f in findings {
            let Location::Object { id, .. } = f.location else {
                continue;
            };
            let entry = ctx.salvage.by_obj.get(&id);
            let Some(entry) = entry.filter(|_| salvage_name(f) != Some(OUTSIDE_STREAM)) else {
                actions.push(RepairAction {
                    object: id,
                    what: format!("{}: resolved by re-emission", f.id),
                    grade: None,
                });
                continue;
            };
            let s = &entry.salvage;
            let (what, why) = match s {
                Salvage::Clean { .. } => continue,
                Salvage::Repaired {
                    edits,
                    grade,
                    survivors,
                    ..
                } => {
                    summary.repaired += 1;
                    survivors_of.push((id, survivors.clone()));
                    let edits = edit_list(edits);
                    match *grade {
                        Grade::Exact => {
                            summary.exact += 1;
                            let what = format!(
                                "Flate data repaired ({edits}); no other repair fits its window"
                            );
                            (what, None)
                        }
                        Grade::Accepted { searched, window } => {
                            summary.accepted += 1;
                            let what = format!(
                                "Flate data repaired ({edits}); accepted without a uniqueness \
                                 check: {searched} of {window} candidates searched"
                            );
                            (what, None)
                        }
                        Grade::Ambiguous { outputs } => {
                            summary.ambiguous += 1;
                            let what = format!(
                                "Flate data repaired with the first of {outputs} candidate \
                                 repairs in the pinned order ({edits}); every survivor is in the \
                                 report"
                            );
                            let why = format!("ambiguous: {outputs} candidate repairs, see report");
                            (what, Some(why))
                        }
                    }
                }
                Salvage::ChecksumMismatch { .. } => (
                    "written decoded: its Adler-32 disagrees and no repair was found, so the \
                     bytes are unverified"
                        .to_owned(),
                    Some("checksum mismatch: the decoded bytes are unverified".to_owned()),
                ),
                Salvage::Prefix {
                    in_used, in_total, ..
                } => (
                    format!(
                        "written decoded up to input byte {in_used} of {in_total}; the rest is lost"
                    ),
                    Some("only the bytes before the damage decode".to_owned()),
                ),
                Salvage::Unrecoverable => {
                    summary.unrecoverable += 1;
                    (
                        "written as carved: nothing decodes and no repair was found".to_owned(),
                        Some("unrecoverable stream".to_owned()),
                    )
                }
                Salvage::Unsearched { reason } => {
                    summary.unsearched += 1;
                    (
                        format!(
                            "written as carved: its {} raw bytes are over the {}-byte search limit",
                            reason.raw_len, reason.limit
                        ),
                        Some("stream over max_search_stream, not searched".to_owned()),
                    )
                }
            };
            summary.streams_damaged += 1;
            ctx.doc.swap_salvaged(id);
            if let Some(why) = why {
                partial.push(format!("{} {} obj: {why}", id.0, id.1));
                ctx.notes.partial.push((C9ZlibTampered, f.location));
            }
            actions.push(RepairAction {
                object: id,
                what,
                grade: s.grade(),
            });
        }
        survivors_of.sort_by_key(|&(id, _)| id);
        ctx.notes.c9_summary = summary;
        ctx.notes.c9_survivors = survivors_of;
        PassReport {
            class: C9ZlibTampered,
            outcome: if partial.is_empty() {
                PassOutcome::Fixed
            } else {
                PassOutcome::Partial(partial.join("; "))
            },
            actions,
        }
    }
}

/// A C9 finding's `Metric{"salvage"}`.
fn salvage_name(f: &Finding) -> Option<&str> {
    f.evidence.iter().find_map(|e| match e {
        Evidence::Metric {
            name,
            value: MetricValue::Text(t),
        } if name == "salvage" => Some(t.as_str()),
        _ => None,
    })
}

/// `byte 40 0x31 -> 0x30, ...`, offsets in the Flate stage's input.
fn edit_list(edits: &[ByteEdit]) -> String {
    let each: Vec<String> = edits
        .iter()
        .map(|(at, from, to)| format!("byte {at} 0x{from:02x} -> 0x{to:02x}"))
        .collect();
    each.join(", ")
}

/// C6: re-link each lost slot to an unreachable font (module docs).
struct Relink;

/// How a candidate's `/Widths` fit the codes a page shows through a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Widths {
    /// Some code has no width, or a zero one: not this slot's font.
    Disagree,
    /// No `/Widths` to read, or nothing shown.
    Unknown,
    /// Every code shown has a nonzero width.
    Agree,
}

impl RepairPass for Relink {
    fn class(&self) -> CorruptionClass {
        C6FontMapLost
    }

    fn repair(&self, ctx: &mut RepairCtx<'_>, findings: &[Finding]) -> PassReport {
        // The findings by page, in order: one per (page, slot).
        let mut by_page: BTreeMap<(u32, ObjId), Vec<&Finding>> = BTreeMap::new();
        for f in findings {
            if let Location::Page {
                index,
                obj: Some(page),
            } = f.location
            {
                by_page.entry((index, page)).or_default().push(f);
            }
        }
        let candidates: Vec<ObjId> = orphan_fonts(ctx.graph)
            .into_iter()
            .filter(|&id| ctx.doc.remap().number(id).is_some())
            .collect();
        let mut actions = Vec::new();
        let mut unmatched = 0usize;
        for ((index, page), slots) in by_page {
            let shown = shown_by_slot(ctx, page);
            let wanted: Vec<(&Finding, Vec<u8>)> = slots
                .into_iter()
                .filter_map(|f| {
                    let slot = slot_of(f)?;
                    // The bytes of the slot the finding names (its text is lossy).
                    let bytes = shown
                        .keys()
                        .find(|k| String::from_utf8_lossy(k) == slot)
                        .cloned()
                        .unwrap_or_else(|| slot.as_bytes().to_vec());
                    Some((f, bytes))
                })
                .collect();
            let page_at = span_of(ctx, page).map_or(0, |s| s.start);
            // Every (slot, candidate) pair that may match, best first.
            let mut pairs = Vec::new();
            for (k, (_, slot)) in wanted.iter().enumerate() {
                let codes = shown.get(slot).map_or(&[][..], Vec::as_slice);
                for &font in &candidates {
                    let Some(dict) = font_dict(ctx, font) else {
                        continue;
                    };
                    let widths = widths_fit(ctx, dict, codes);
                    if widths == Widths::Disagree {
                        continue;
                    }
                    let named = dict.get(b"Name").ok().and_then(|n| n.as_name().ok())
                        == Some(slot.as_slice());
                    let distance =
                        span_of(ctx, font).map_or(u64::MAX, |s| s.start.abs_diff(page_at));
                    let key = (
                        std::cmp::Reverse(named),
                        std::cmp::Reverse(widths),
                        distance,
                        font,
                        k,
                    );
                    pairs.push((key, k, font, named, widths));
                }
            }
            pairs.sort_by_key(|p| p.0);
            let mut chosen: BTreeMap<usize, (ObjId, bool, Widths)> = BTreeMap::new();
            let mut used = BTreeSet::new();
            for (_, k, font, named, widths) in pairs {
                if chosen.contains_key(&k) || used.contains(&font) {
                    continue;
                }
                used.insert(font);
                chosen.insert(k, (font, named, widths));
            }
            let page_number = u64::from(index) + 1;
            for (k, (f, slot)) in wanted.iter().enumerate() {
                let name = String::from_utf8_lossy(slot);
                match chosen.get(&k) {
                    Some(&(font, named, widths)) => {
                        if let Some(n) = ctx.remap.number(page) {
                            ctx.doc.relink_font(n, slot.clone(), font);
                        }
                        let by = if named {
                            "its /Name"
                        } else {
                            "its place in the file"
                        };
                        let widths = match widths {
                            Widths::Agree => "; its widths fit the codes shown",
                            _ => "",
                        };
                        actions.push(RepairAction {
                            object: page,
                            what: format!(
                                "page {page_number}: /{name} re-linked to {} {} obj, matched by \
                                 {by}{widths}",
                                font.0, font.1
                            ),
                            grade: None,
                        });
                    }
                    None => {
                        unmatched += 1;
                        ctx.notes.partial.push((C6FontMapLost, f.location));
                        // With no candidate at all the plan escalated it already.
                        if f.repair != Repairability::Interactive(InteractionKind::FontPick) {
                            ctx.notes.escalations.push(Escalation {
                                kind: EscalationKind::Interaction(InteractionKind::FontPick),
                                page: Some(index),
                                slot: Some(name.to_string()),
                                note: format!(
                                    "{}: no unreachable font fits /{name} on page {page_number}",
                                    f.id
                                ),
                            });
                        }
                    }
                }
            }
        }
        let outcome = if unmatched == 0 {
            PassOutcome::Fixed
        } else {
            PassOutcome::Partial(format!(
                "{unmatched} font slots not re-linked: no unreachable font fits them, so a font \
                 pick is needed"
            ))
        };
        PassReport {
            class: C6FontMapLost,
            outcome,
            actions,
        }
    }
}

/// A C6 finding's slot: its `slots:` evidence.
fn slot_of(f: &Finding) -> Option<&str> {
    f.evidence.iter().find_map(|e| match e {
        Evidence::Text(t) => t.strip_prefix("slots: "),
        _ => None,
    })
}

/// The span of the copy a reference to `id` names.
fn span_of(ctx: &RepairCtx<'_>, id: ObjId) -> Option<ByteSpan> {
    match ctx.remap.target(id)? {
        Held::Object(at) => Some(ctx.carve.objects.get(at)?.span),
        Held::Orphan(at) => Some(ctx.carve.orphans.get(at)?.span()),
    }
}

/// The dictionary of the carved object a reference to `id` names.
fn font_dict<'c>(ctx: &RepairCtx<'c>, id: ObjId) -> Option<&'c Dictionary> {
    match ctx.remap.target(id)? {
        Held::Object(at) => match &ctx.carve.objects.get(at)?.body {
            Body::Dict(d) => Some(d),
            _ => None,
        },
        Held::Orphan(_) => None,
    }
}

/// `v`, or what the reference `v` names when that is a value.
fn resolved<'c>(ctx: &RepairCtx<'c>, v: &'c Object) -> Option<&'c Object> {
    match v {
        Object::Reference(id) => match ctx.remap.target(*id)? {
            Held::Object(at) => match &ctx.carve.objects.get(at)?.body {
                Body::Primitive(p) => Some(p),
                _ => None,
            },
            Held::Orphan(_) => None,
        },
        other => Some(other),
    }
}

/// How `font`'s `/Widths` fit `codes`, one byte per code (module docs).
fn widths_fit<'c>(ctx: &RepairCtx<'c>, font: &'c Dictionary, codes: &[u8]) -> Widths {
    let Some(Object::Array(widths)) = font.get(b"Widths").ok().and_then(|v| resolved(ctx, v))
    else {
        return Widths::Unknown;
    };
    let Some(first) = (font.get(b"FirstChar").ok())
        .and_then(|v| resolved(ctx, v))
        .and_then(|v| v.as_i64().ok())
    else {
        return Widths::Unknown;
    };
    if codes.is_empty() {
        return Widths::Unknown;
    }
    let inked = |code: u8| {
        let at = usize::try_from(i64::from(code) - first).ok();
        let width = at
            .and_then(|i| widths.get(i))
            .and_then(|w| resolved(ctx, w));
        match width {
            Some(Object::Integer(w)) => *w != 0,
            Some(Object::Real(w)) => *w != 0.0,
            _ => false,
        }
    };
    if codes.iter().all(|&c| inked(c)) {
        Widths::Agree
    } else {
        Widths::Disagree
    }
}

/// Every slot `page`'s content selects with `Tf`, with the string bytes its
/// text operators show through it, read through T-09's `page_content` and
/// T-06's `content_ops`.
fn shown_by_slot(ctx: &RepairCtx<'_>, page: ObjId) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let source = CarveSource::new(ctx.carve, ctx.bytes);
    let decode = |id: ObjId| ctx.salvage.decoded(&source, id, DEFAULT_CAP).ok();
    let mut out: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    for piece in ctx.graph.page_content(ctx.carve, page, decode) {
        let Some(bytes) = decode(piece.stream) else {
            continue;
        };
        let mut slot: Option<Vec<u8>> = None;
        for op in content_ops(&bytes) {
            let strings: Vec<&[u8]> = match (op.op, op.operands.last()) {
                (b"Tf", _) => {
                    if let [.., Object::Name(s), _] = op.operands.as_slice() {
                        out.entry(s.clone()).or_default();
                        slot = Some(s.clone());
                    }
                    continue;
                }
                (b"Tj" | b"'" | b"\"", Some(Object::String(t, _))) => vec![t],
                (b"TJ", Some(Object::Array(items))) => items
                    .iter()
                    .filter_map(|i| match i {
                        Object::String(t, _) => Some(t.as_slice()),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            if let Some(s) = &slot {
                let shown = out.entry(s.clone()).or_default();
                strings.into_iter().for_each(|t| shown.extend_from_slice(t));
            }
        }
    }
    out
}

// ── generate and validate ────────────────────────────────────────────────

/// What analysis left for repair: the input, its carve, graph, salvage
/// index and findings (the `AnalysisState` contents), all from `bytes`.
pub(crate) struct Analysed<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) carve: &'a CarveReport,
    pub(crate) graph: &'a ObjectGraph,
    pub(crate) salvage: &'a SalvageIndex,
    pub(crate) findings: &'a [Finding],
    pub(crate) input_sha256: [u8; 32],
}

/// What generate-and-validate built and chose.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Generated {
    /// The chosen candidate's bytes; `None` when none passed V0.
    pub(crate) output: Option<Vec<u8>>,
    pub(crate) chosen: Option<Toolpath>,
    /// Every candidate built, in plan order.
    pub(crate) candidates: Vec<CandidateReport>,
    /// The passes of the chosen candidate. When none passed V0, those of
    /// the best failed candidate: what was tried on a file never written.
    pub(crate) passes: Vec<PassReport>,
    /// What the chosen candidate's emit changed.
    pub(crate) notes: EmitNotes,
    /// Escalations generation raised beyond the plan's: those of the passes
    /// of the candidate [`Self::passes`] describes, then `NoCandidatePassed`.
    pub(crate) escalations: Vec<Escalation>,
    /// Each `Partial` pass's reason, for the output; empty when there is
    /// none.
    pub(crate) partial_reasons: Vec<String>,
    /// The C9 pass's counts and survivors, of the same candidate.
    pub(crate) c9_summary: C9Summary,
    pub(crate) c9_survivors: Vec<(ObjId, Vec<Vec<ByteEdit>>)>,
}

impl Generated {
    /// Puts the run in `report`. With no output, `report.passes` holds the
    /// best failed candidate's passes and `partial_reasons` is empty.
    pub(crate) fn record(&self, report: &mut RepairReport) {
        report.passes = self.passes.clone();
        report.candidates = self.candidates.clone();
        report.chosen = self.chosen;
        report.partial_reasons = self.partial_reasons.clone();
        report.c9_summary = self.c9_summary;
        report.c9_survivors = self.c9_survivors.clone();
        self.notes.record(report);
    }
}

/// One candidate built and verified.
struct Built {
    toolpath: Toolpath,
    output: Vec<u8>,
    verification: Verification,
    key: SelectionKey,
    passes: Vec<PassReport>,
    pass_notes: PassNotes,
    notes: EmitNotes,
}

/// What each class with findings gets (module docs, "Passes").
enum Scheduled {
    Run(&'static dyn RepairPass, Vec<Finding>),
    Skip(String),
}

/// Builds every candidate in `plan`, verifies each and picks the best
/// (module docs). `Err(Cancelled)` when `sink` says so between steps.
pub(crate) fn generate_and_validate(
    input: &Analysed<'_>,
    plan: &RepairPlan,
    opts: &RepairOptions,
    fonts: &FontDb,
    ask: &mut dyn Interact,
    sink: &mut dyn Progress,
) -> Result<Generated, Cancelled> {
    if plan.candidates.is_empty() {
        return Ok(Generated::default());
    }
    let schedule = schedule(input.findings, opts.passes.as_deref(), sink);
    let remap = plan_ids(input.carve, input.graph);
    let tree = rebuild_page_tree(input.carve, input.graph, &remap, opts.default_page_size);
    let base = baseline(input.bytes, input.carve);

    let total = u32::try_from(plan.candidates.len()).unwrap_or(u32::MAX);
    let mut built: Vec<Built> = Vec::new();
    for (i, &toolpath) in plan.candidates.iter().enumerate() {
        if sink.cancelled() {
            return Err(Cancelled);
        }
        sink.phase("repairing", u32::try_from(i).unwrap_or(u32::MAX), total);
        if toolpath == Toolpath::TemplateAssemble {
            sink.log(
                LogLevel::Info,
                "the TemplateAssemble toolpath is not built in this version; skipped".to_owned(),
            );
            continue;
        }
        let mut doc = RebuildDoc::new(remap.clone());
        let mut ctx = RepairCtx {
            bytes: input.bytes,
            carve: input.carve,
            graph: input.graph,
            remap: &remap,
            page_tree: &tree,
            doc: &mut doc,
            salvage: input.salvage,
            fonts,
            ask: &mut *ask,
            sink: &mut *sink,
            notes: PassNotes::default(),
        };
        let passes = run_passes(&schedule, &mut ctx)?;
        let pass_notes = std::mem::take(&mut ctx.notes);
        let (targeted, partial) = targets(&passes, &schedule, &pass_notes, doc.remap());
        let mut emit = EmitCtx {
            bytes: input.bytes,
            input_sha256: input.input_sha256,
            salvage: input.salvage,
            sink: &mut *sink,
            notes: EmitNotes::default(),
        };
        let output = match emit_doc(doc, input.carve, input.graph, &tree, &mut emit) {
            Ok(out) => out,
            Err(e) => {
                let msg = format!("the {toolpath:?} candidate could not be written: {e}");
                sink.log(LogLevel::Warn, msg);
                continue;
            }
        };
        let notes = emit.notes;
        if sink.cancelled() {
            return Err(Cancelled);
        }
        sink.phase("verifying", u32::try_from(i).unwrap_or(u32::MAX), total);
        let verification = verify(&output, input.carve, &base, &targeted, &partial);
        let key = SelectionKey {
            v0: verification.v0.all_pass(),
            v1: verification.v1.clone(),
            v2: verification.v2.clone(),
            v3: preservation(&output),
            prior: toolpath == plan.prior,
            size: output.len() as u64,
        };
        built.push(Built {
            toolpath,
            output,
            verification,
            key,
            passes,
            pass_notes,
            notes,
        });
    }
    Ok(choose(built))
}

/// The best of `built`, as the run's result (module docs).
fn choose(built: Vec<Built>) -> Generated {
    let mut best: Option<usize> = None;
    for (i, b) in built.iter().enumerate() {
        if best.is_none_or(|j| compare(&b.key, &built[j].key) == Ordering::Greater) {
            best = Some(i);
        }
    }
    let Some(best) = best else {
        return Generated::default();
    };
    let passed = built[best].key.v0;
    let candidates = built
        .iter()
        .enumerate()
        .map(|(i, b)| CandidateReport {
            toolpath: b.toolpath,
            verification: b.verification.clone(),
            chosen: passed && i == best,
        })
        .collect();
    let mut built = built;
    let b = built.swap_remove(best);
    // Reasons for a partial repair describe an output; with none, there are none.
    let partial_reasons = if passed {
        b.passes
            .iter()
            .filter_map(|p| match &p.outcome {
                PassOutcome::Partial(why) => Some(format!("{}: {why}", p.class.code())),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut escalations = b.pass_notes.escalations;
    if !passed {
        escalations.push(Escalation {
            kind: EscalationKind::NoCandidatePassed,
            page: None,
            slot: None,
            note: "no candidate passed the hard gates (V0), so nothing was written".to_owned(),
        });
    }
    Generated {
        output: passed.then_some(b.output),
        chosen: passed.then_some(b.toolpath),
        candidates,
        passes: b.passes,
        notes: if passed {
            b.notes
        } else {
            EmitNotes::default()
        },
        escalations,
        partial_reasons,
        c9_summary: b.pass_notes.c9_summary,
        c9_survivors: b.pass_notes.c9_survivors,
    }
}

/// The classes the `Resave` rebuild repairs whether or not their pass runs
/// (module docs).
const REBUILT: [CorruptionClass; 5] = [
    C1Header,
    C2XrefMissing,
    C3TrailerDamaged,
    C4PageTreeBroken,
    C5ObjectTagStripped,
];

/// What happens to each class with findings, in pass order; logs each
/// class skipped, once per run.
fn schedule(
    findings: &[Finding],
    passes: Option<&[CorruptionClass]>,
    sink: &mut dyn Progress,
) -> Vec<(CorruptionClass, Scheduled)> {
    let mut out = Vec::new();
    for class in pass_order() {
        let of_class: Vec<Finding> = findings
            .iter()
            .filter(|f| f.class == FindingKind::Corruption(class))
            .cloned()
            .collect();
        if of_class.is_empty() {
            continue;
        }
        let name = format!("{} ({})", class.code(), class.label());
        let scheduled = if passes.is_some_and(|chosen| !chosen.contains(&class)) {
            let why = if REBUILT.contains(&class) {
                "not selected for this repair; the rebuild re-emits the file regardless"
            } else {
                "not selected for this repair"
            };
            sink.log(LogLevel::Info, format!("{name}: {why}; skipped"));
            Scheduled::Skip(why.to_owned())
        } else if let Some(pass) = pass_for(class) {
            Scheduled::Run(pass, of_class)
        } else {
            sink.log(
                LogLevel::Info,
                format!("{name} has no repair pass in this version; skipped"),
            );
            Scheduled::Skip("no repair pass for this class in this version".to_owned())
        };
        out.push((class, scheduled));
    }
    out
}

/// Runs the schedule on `ctx.doc`: one report per class, in order.
fn run_passes(
    schedule: &[(CorruptionClass, Scheduled)],
    ctx: &mut RepairCtx<'_>,
) -> Result<Vec<PassReport>, Cancelled> {
    let mut reports = Vec::new();
    for (class, scheduled) in schedule {
        if ctx.cancelled() {
            return Err(Cancelled);
        }
        reports.push(match scheduled {
            Scheduled::Run(pass, findings) => pass.repair(ctx, findings),
            Scheduled::Skip(why) => PassReport {
                class: *class,
                outcome: PassOutcome::Skipped(why.clone()),
                actions: Vec::new(),
            },
        });
    }
    Ok(reports)
}

/// The classes verification targets (every pass that ran and did not skip
/// itself), and the output locations each `Partial` one left partial: those
/// it listed in `notes`, or else all of its findings'.
fn targets(
    reports: &[PassReport],
    schedule: &[(CorruptionClass, Scheduled)],
    notes: &PassNotes,
    remap: &IdRemap,
) -> (Vec<CorruptionClass>, Vec<Location>) {
    let mut targeted = Vec::new();
    let mut partial = Vec::new();
    for (report, (_, scheduled)) in reports.iter().zip(schedule) {
        let Scheduled::Run(_, findings) = scheduled else {
            continue;
        };
        match report.outcome {
            PassOutcome::Skipped(_) => continue,
            PassOutcome::Partial(_) => {
                let listed: Vec<&Location> = (notes.partial.iter())
                    .filter(|(c, _)| *c == report.class)
                    .map(|(_, l)| l)
                    .collect();
                let locations: Vec<&Location> = if listed.is_empty() {
                    findings.iter().map(|f| &f.location).collect()
                } else {
                    listed
                };
                partial.extend(locations.into_iter().filter_map(|l| in_output(l, remap)));
            }
            PassOutcome::Fixed => {}
        }
        targeted.push(report.class);
    }
    (targeted, partial)
}

/// `location` in the output's numbering; `None` for a byte span, which the
/// output does not keep.
fn in_output(location: &Location, remap: &IdRemap) -> Option<Location> {
    let out = |id: ObjId| remap.number(id).map(|n| (n, 0));
    match *location {
        Location::File => Some(Location::File),
        Location::Span(_) => None,
        Location::Object { id, .. } => out(id).map(|id| Location::Object { id, span: None }),
        Location::Page { index, obj } => Some(Location::Page {
            index,
            obj: obj.and_then(out),
        }),
    }
}

fn slice(bytes: &[u8], span: ByteSpan) -> &[u8] {
    let range = usize::try_from(span.start)
        .ok()
        .zip(usize::try_from(span.end).ok());
    range.and_then(|(s, e)| bytes.get(s..e)).unwrap_or(&[])
}

// ── selection ────────────────────────────────────────────────────────────

/// One candidate's place in the selection order (module docs, "Selection").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionKey {
    /// Every V0 gate passed.
    pub(crate) v0: bool,
    pub(crate) v1: Retention,
    pub(crate) v2: Plausibility,
    pub(crate) v3: Preservation,
    /// The toolpath is the plan's prior.
    pub(crate) prior: bool,
    /// Output bytes.
    pub(crate) size: u64,
}

/// V3: what of the input's document-level structure an output keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Preservation {
    /// How many of the catalog's `/Outlines`, `/AcroForm` and `/Metadata`
    /// and the trailer's `/Info` are present.
    pub(crate) features: u32,
    /// Entries of the pages' `/Annots` arrays.
    pub(crate) annotations: u64,
}

/// How `a` ranks against `b`: `Greater` when `a` is the better candidate.
/// A pure function of the two keys (module docs, "Selection").
pub(crate) fn compare(a: &SelectionKey, b: &SelectionKey) -> Ordering {
    a.v0.cmp(&b.v0)
        .then_with(|| retention(&a.v1, &b.v1))
        .then_with(|| plausibility(&a.v2, &b.v2))
        .then_with(|| a.v3.cmp(&b.v3))
        .then_with(|| a.prior.cmp(&b.prior))
        .then_with(|| b.size.cmp(&a.size))
}

/// V1: fewer blank pages, then more kept (each at most whole).
fn retention(a: &Retention, b: &Retention) -> Ordering {
    let whole = |r: Ratio| r.min(Ratio { num: 1, den: 1 });
    let kept = |v: &Retention| {
        [
            v.glyph_count,
            v.text_ops,
            v.images,
            v.path_fills,
            v.content_bytes,
        ]
        .map(whole)
    };
    b.blank_pages
        .cmp(&a.blank_pages)
        .then_with(|| kept(a).cmp(&kept(b)))
}

/// V2: fewer U+FFFD, fewer unmapped, more consistent, more dictionary hits.
fn plausibility(a: &Plausibility, b: &Plausibility) -> Ordering {
    b.fffd
        .cmp(&a.fffd)
        .then_with(|| b.unmapped_glyph.cmp(&a.unmapped_glyph))
        .then_with(|| a.script_consistency.cmp(&b.script_consistency))
        .then_with(|| a.dictionary_hit.cmp(&b.dictionary_hit))
}

/// [`Preservation`] of `output`, read through our own carve.
pub(crate) fn preservation(output: &[u8]) -> Preservation {
    let carve = carve(output, &|| false).unwrap_or_default();
    let graph = ObjectGraph::from_carve(&carve);
    let winners = winning_copies(&carve);
    let dict = |id: ObjId| -> Option<&Dictionary> {
        match &carve.objects[*winners.get(&id)?].body {
            Body::Dict(d) => Some(d),
            _ => None,
        }
    };
    let resolve = |v: &Object| -> Option<Object> {
        match v {
            Object::Reference(id) => match &carve.objects[*winners.get(id)?].body {
                Body::Primitive(p) => Some(p.clone()),
                Body::Dict(d) => Some(Object::Dictionary(d.clone())),
                _ => None,
            },
            other => Some(other.clone()),
        }
    };
    let present = |d: &Dictionary, key: &[u8]| {
        d.get(key)
            .ok()
            .and_then(&resolve)
            .is_some_and(|v| !matches!(v, Object::Null))
    };
    let catalog = graph.catalog_candidates().first().and_then(|&c| dict(c));
    let mut features = catalog.map_or(0, |c| {
        [&b"Outlines"[..], b"AcroForm", b"Metadata"]
            .into_iter()
            .filter(|k| present(c, k))
            .count() as u32
    });
    features += u32::from(info_object(output, &carve, &graph).is_some());
    let annotations = graph
        .pages_in_doc_order()
        .into_iter()
        .filter_map(dict)
        .filter_map(|p| p.get(b"Annots").ok().and_then(&resolve))
        .map(|a| match a {
            Object::Array(items) => items.len() as u64,
            _ => 0,
        })
        .sum();
    Preservation {
        features,
        annotations,
    }
}
