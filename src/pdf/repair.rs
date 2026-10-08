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
//! Until the C9 pass (T-13b) swaps salvaged bytes in, every stream is copied
//! exactly as carved.
//!
//! **Passes**, one [`RepairPass`] per class, in the order C1, C2, C3, then
//! [`CorruptionClass::REPAIR_ORDER`]. A pass runs when the file has a
//! finding of its class and `RepairOptions.passes` selects it; a class with
//! findings that is not selected, or has no pass in this version, gets a
//! `Skipped` report and a log line. The classes whose passes ran are the
//! ones verification targets; the findings of a pass that reported
//! `Partial` may stay in the output (D-074).
//! - **C1–C3**: re-emission fixes them (a new header, a classic table and a
//!   trailer); one action per finding, "resolved by re-emission".
//! - **C10**: a cut stream is written with the bytes before the cut and a
//!   `/Length` to match (clamped); a cut object nothing reachable names is
//!   dropped. Always `Partial`: what came after the cut is gone. Reachable
//!   means from a catalog or a page, through the carve's references, the
//!   rebuild's reference matches and every orphan's references.
//! - **C5**: each headerless object is written under the number the
//!   rebuild gave it, and named by the references the rebuild matched to it.
//! - **C4**: the flat page tree is written: the catalog points at one
//!   `/Pages` node holding every page, in document order, attributes pinned.
//!
//! A [`RepairAction`] names an object by its declared id in the input; an
//! orphan, which has none, by the number the rebuild gave it; a finding
//! about the whole file by `(0, 0)`.
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
    Cancelled, CandidateReport, Escalation, EscalationKind, FontDb, Interact, LogLevel,
    PassOutcome, PassReport, Progress, RepairAction, RepairOptions, RepairPlan, RepairReport,
    Toolpath,
};
use crate::pdf::carver::{Body, CarveReport, Orphan, carve};
use crate::pdf::emit::{EmitCtx, EmitNotes, RebuildDoc, emit_doc};
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::lexer;
use crate::pdf::meta::info_object;
use crate::pdf::model::{
    ByteSpan, CorruptionClass, Evidence, Finding, FindingKind, Location, MetricValue, ObjId, Ratio,
};
use crate::pdf::rebuild::{
    BoxSource, CatalogPlan, Held, IdRemap, PageTreePlan, plan_ids, rebuild_page_tree,
};
use crate::pdf::streams::salvage::SalvageIndex;
use crate::pdf::verify::{Plausibility, Retention, Verification, baseline, verify};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped, C10Truncated,
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
    // Read by the C9 swap (T-13b).
    #[allow(dead_code)]
    pub(crate) salvage: &'a SalvageIndex,
    // Read by the C6 re-link (T-13b) and the font passes (T-30).
    #[allow(dead_code)]
    pub(crate) fonts: &'a FontDb,
    #[allow(dead_code)]
    pub(crate) ask: &'a mut dyn Interact,
    pub(crate) sink: &'a mut dyn Progress,
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
const PASSES: [&dyn RepairPass; 6] = [
    &ReEmitted(C1Header),
    &ReEmitted(C2XrefMissing),
    &ReEmitted(C3TrailerDamaged),
    &Truncation,
    &Orphans,
    &PageTree,
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
        let reached = reachable(ctx.carve, ctx.graph, ctx.remap);
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
                kept: data.end - data.start,
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

/// Everything a catalog or a page reaches (module docs, C10).
fn reachable(carve: &CarveReport, graph: &ObjectGraph, remap: &IdRemap) -> BTreeSet<Held> {
    let mut roots: Vec<ObjId> = graph.catalog_candidates();
    roots.extend(graph.pages_in_doc_order());
    for orphan in &carve.orphans {
        let dict = match orphan {
            Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. } => dict,
        };
        dict.iter().for_each(|(_, v)| references(v, &mut roots));
    }
    let mut ids = BTreeSet::new();
    for root in roots {
        if ids.contains(&root) {
            continue;
        }
        ids.extend(graph.reachable(root));
        // A root that is not a node (a reference only the rebuild matched).
        ids.insert(root);
    }
    let mut held: BTreeSet<Held> = ids.into_iter().filter_map(|id| remap.target(id)).collect();
    held.extend(remap.reconciled().iter().map(|r| r.target));
    // An orphan page is a page.
    let is_page =
        |d: &Dictionary| d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Page");
    held.extend(
        carve
            .orphans
            .iter()
            .enumerate()
            .filter(|(_, o)| matches!(o, Orphan::Dict { dict, .. } if is_page(dict)))
            .map(|(i, _)| Held::Orphan(i)),
    );
    held
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
    /// The passes of the chosen candidate (of the best one when none passed).
    pub(crate) passes: Vec<PassReport>,
    /// What the chosen candidate's emit changed.
    pub(crate) notes: EmitNotes,
    /// Escalations generation raised (`NoCandidatePassed`), beyond the plan's.
    pub(crate) escalations: Vec<Escalation>,
    /// Each `Partial` pass's reason.
    pub(crate) partial_reasons: Vec<String>,
}

impl Generated {
    /// Puts the run in `report`.
    pub(crate) fn record(&self, report: &mut RepairReport) {
        report.passes = self.passes.clone();
        report.candidates = self.candidates.clone();
        report.chosen = self.chosen;
        report.partial_reasons = self.partial_reasons.clone();
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
    // No C9 pass yet (T-13b): every stream is copied as carved.
    let unsalvaged = SalvageIndex::default();

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
        };
        let passes = run_passes(&schedule, &mut ctx)?;
        let (targeted, partial) = targets(&passes, &schedule, doc.remap());
        let mut emit = EmitCtx {
            bytes: input.bytes,
            input_sha256: input.input_sha256,
            salvage: &unsalvaged,
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
    let partial_reasons = b
        .passes
        .iter()
        .filter_map(|p| match &p.outcome {
            PassOutcome::Partial(why) => Some(format!("{}: {why}", p.class.code())),
            _ => None,
        })
        .collect();
    let escalations = if passed {
        Vec::new()
    } else {
        vec![Escalation {
            kind: EscalationKind::NoCandidatePassed,
            page: None,
            slot: None,
            note: "no candidate passed the hard gates (V0), so nothing was written".to_owned(),
        }]
    };
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
    }
}

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
            sink.log(
                LogLevel::Info,
                format!("{name} was not selected for this repair; skipped"),
            );
            Scheduled::Skip("not selected for this repair".to_owned())
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
/// itself), and the output locations of the findings of each `Partial` one.
fn targets(
    reports: &[PassReport],
    schedule: &[(CorruptionClass, Scheduled)],
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
                partial.extend(
                    findings
                        .iter()
                        .filter_map(|f| in_output(&f.location, remap)),
                );
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
