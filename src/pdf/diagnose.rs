//! Corruption detectors (T-11a, T-11b; TD §19, map #14): each signal over
//! the carve, the object graph and the salvage index becomes a [`Finding`].
//!
//! T-11a's structural rules, each with tests in `tests`:
//! - **C1**: no `%PDF-` in the first KiB, or version bytes that are not
//!   `\d\.\d`. A header found later is `Metric{"header_offset"}`. Every offset
//!   below is **header-relative**: it counts from the `%PDF-` the carve found
//!   (from byte 0 when there is none), as qpdf rebases, so a junk prefix is
//!   not also read as C2 or C3.
//! - **C2**: objects exist, but no classic `xref` table and no
//!   `/Type /XRef` stream.
//! - **C3**: no `trailer` (an xref stream's dictionary is one), `startxref`
//!   or `%%EOF`; or the last `startxref` misses the nearest table or xref
//!   stream. Within ±64 bytes (lopdf's recovery window) the miss is a
//!   Warning, past that an Error. A miss with no table or stream to land on
//!   is C2's. A miss that the table's own entries share is not C3: when the
//!   in-use entry with the highest offset misses its object's header by the
//!   same amount, bytes were deleted or inserted before the table (a C4 or C5
//!   cut), and the trailer is as it was written.
//! - **C4**: no catalog whose `/Pages` reaches a page-tree node (one with
//!   `/Kids` or `/Count`), a `/Count` that is not the pages the walk reached,
//!   or a `/Kids` entry that names nothing usable. References resolve as
//!   T-10's [`IdRemap`] has them, so a node whose header was stripped (C5)
//!   is still reached; a catalog whose `/Pages` names nothing takes the
//!   first root `/Pages` orphan. A cycle, a node listed twice, and a level
//!   past [`MAX_TREE_DEPTH`] are each a Warning of their own, and C4 fires
//!   once.
//! - **C5**: one finding per dictionary or stream with no header.
//! - **C10**: a stream cut by the end of the file, objects running to the
//!   end with no `%%EOF`, or the last `startxref` or an xref entry pointing
//!   past the end. A truncated file has lost its tail, so C2 and C3 do not
//!   fire beside C10. Junk cut after `%%EOF` is not C10.
//! - **Encrypted**: `/Encrypt` in any trailer or xref-stream dictionary,
//!   shadowed ones too: one finding for the file, `Unrepairable("decrypt
//!   first")`.
//! - **Signed** (D-052): any dictionary with `/Type /Sig` or a `/ByteRange`
//!   array, in any copy of any object (a shadow revision too) or orphan: one
//!   Info finding for the file, `fields` counting the objects that hold one.
//!
//! Ids are `<code>-<nnn>` (`C2-001`, `ENC-001`, `SIG-001`), numbered per
//! code in byte order of where each finding sits, so they are stable for
//! the same input.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object};

use crate::pdf::carver::{Body, CarveReport, Origin, Orphan};
use crate::pdf::graph::{MAX_TREE_DEPTH, ObjectGraph};
use crate::pdf::lexer;
use crate::pdf::model::{
    ByteSpan, CorruptionClass, Evidence, Finding, FindingKind, HexWindow, LengthSource, Location,
    MetricValue, ObjId, ObjectKind, Ratio, Repairability, Severity,
};
use crate::pdf::rebuild::{Held, IdRemap, plan_ids};
use crate::pdf::streams::salvage::SalvageIndex;

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped, C10Truncated,
};

/// Where readers look for the `%PDF-` header.
const HEADER_WINDOW: u64 = 1024;
/// A `startxref` this close to its table is a Warning (lopdf's window).
const STARTXREF_SLACK: u64 = 64;
/// The fixed summary of a signed original (D-052).
pub(crate) const SIGNED_SUMMARY: &str = "the original is digitally signed; a repaired file is not";

/// Every finding in `bytes`, in byte order with ids assigned (module docs).
/// `carve`, `graph` and `salvage` must all come from `bytes`.
pub(crate) fn diagnose(
    bytes: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    salvage: &SalvageIndex,
) -> Vec<Finding> {
    let cx = Cx::new(bytes, carve, graph);
    let mut drafts = Vec::new();
    drafts.extend(c1(&cx));
    let c10 = c10(&cx);
    let truncated = c10.is_some();
    drafts.extend(c10);
    if !truncated {
        drafts.extend(c2(&cx));
        drafts.extend(c3(&cx));
    }
    drafts.extend(c4(&cx));
    drafts.extend(c5(&cx));
    drafts.extend(encrypted(&cx));
    drafts.extend(signed(&cx));
    drafts.extend(content(&cx, salvage));
    number(drafts)
}

/// C6–C9, `OutlinedText` and `Type3Text`: T-11b fills this arm.
fn content(_cx: &Cx<'_>, _salvage: &SalvageIndex) -> Vec<Draft> {
    Vec::new()
}

// ── drafts and ids ───────────────────────────────────────────────────────

/// A finding before its id: `at` is where it sits in the file, its sort key.
struct Draft {
    at: u64,
    kind: FindingKind,
    severity: Severity,
    location: Location,
    summary: String,
    evidence: Vec<Evidence>,
    repair: Repairability,
}

impl Draft {
    fn corruption(class: CorruptionClass, severity: Severity, at: u64, location: Location) -> Self {
        Draft {
            at,
            kind: FindingKind::Corruption(class),
            severity,
            location,
            summary: class.label().to_owned(),
            evidence: Vec::new(),
            repair: Repairability::Auto,
        }
    }

    fn summary(mut self, summary: String) -> Self {
        self.summary = summary;
        self
    }

    fn evidence(mut self, evidence: Vec<Evidence>) -> Self {
        self.evidence = evidence;
        self
    }
}

/// The id prefix and the sort rank of a kind.
fn prefix(kind: &FindingKind) -> (&'static str, usize) {
    match kind {
        FindingKind::Corruption(c) => (c.code(), *c as usize),
        FindingKind::Encrypted => ("ENC", 10),
        FindingKind::Signed { .. } => ("SIG", 11),
        FindingKind::OutlinedText { .. } => ("OUTLINE", 12),
        FindingKind::Type3Text { .. } => ("TYPE3", 13),
    }
}

/// Sorts by place, then kind (stable, so ties keep detector order), and
/// numbers each prefix from 001.
fn number(mut drafts: Vec<Draft>) -> Vec<Finding> {
    drafts.sort_by_key(|d| (d.at, prefix(&d.kind).1));
    let mut next: BTreeMap<&'static str, u32> = BTreeMap::new();
    drafts
        .into_iter()
        .map(|d| {
            let code = prefix(&d.kind).0;
            let n = next.entry(code).or_insert(0);
            *n += 1;
            Finding {
                id: format!("{code}-{n:03}"),
                class: d.kind,
                severity: d.severity,
                location: d.location,
                summary: d.summary,
                evidence: d.evidence,
                repair: d.repair,
            }
        })
        .collect()
}

fn metric(name: &str, value: i64) -> Evidence {
    Evidence::Metric {
        name: name.to_owned(),
        value: MetricValue::Int(value),
    }
}

fn int(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

// ── the context ──────────────────────────────────────────────────────────

/// The inputs, and what several detectors read.
struct Cx<'a> {
    bytes: &'a [u8],
    carve: &'a CarveReport,
    graph: &'a ObjectGraph,
    remap: IdRemap,
    /// Where offsets count from: the header, else byte 0.
    base: u64,
    len: u64,
    /// Every classic table and xref stream, in byte order.
    xrefs: Vec<Xref>,
    /// The last `startxref` that has a value: (keyword offset, value).
    startxref: Option<(u64, u64)>,
}

/// A cross-reference section: where it starts and its in-use entries
/// `(number, generation, offset)`.
struct Xref {
    start: u64,
    entries: Vec<(u64, u64, u64)>,
}

impl<'a> Cx<'a> {
    fn new(bytes: &'a [u8], carve: &'a CarveReport, graph: &'a ObjectGraph) -> Self {
        let mut xrefs: Vec<Xref> = carve
            .xref_spans
            .iter()
            .map(|s| Xref {
                start: s.start,
                entries: table_entries(slice(bytes, *s)),
            })
            .collect();
        xrefs.extend(carve.xref_streams.iter().map(|x| {
            Xref {
                start: x.span.start,
                entries: x
                    .rows
                    .iter()
                    .filter(|r| r.fields[0] == 1)
                    .map(|r| (r.num, r.fields[2], r.fields[1]))
                    .collect(),
            }
        }));
        xrefs.sort_by_key(|x| x.start);
        Cx {
            bytes,
            carve,
            graph,
            remap: plan_ids(carve, graph),
            base: carve.header.as_ref().map_or(0, |h| h.offset),
            len: bytes.len() as u64,
            xrefs,
            startxref: carve
                .startxref
                .iter()
                .rev()
                .find_map(|&(at, v)| Some((at, v?))),
        }
    }

    /// The last ≤64 bytes of the file.
    fn tail(&self) -> HexWindow {
        let from = self.bytes.len().saturating_sub(HexWindow::MAX_BYTES);
        HexWindow::new(from as u64, &self.bytes[from..])
    }

    fn dict(&self, held: Held) -> Option<&'a Dictionary> {
        match held {
            Held::Object(i) => match &self.carve.objects.get(i)?.body {
                Body::Dict(d) => Some(d),
                _ => None,
            },
            Held::Orphan(i) => match self.carve.orphans.get(i)? {
                Orphan::Dict { dict, .. } => Some(dict),
                Orphan::Stream { .. } => None,
            },
        }
    }

    /// `v`, or the value of the carved object it references (one hop).
    fn resolve<'v>(&self, v: &'v Object) -> &'v Object
    where
        'a: 'v,
    {
        let Object::Reference(id) = v else { return v };
        match self.remap.target(*id) {
            Some(Held::Object(i)) => match &self.carve.objects[i].body {
                Body::Primitive(p) => p,
                _ => v,
            },
            _ => v,
        }
    }

    /// Where `held` is: its location, sort key and id (an orphan has none).
    fn place(&self, held: Held) -> (Location, u64, Option<ObjId>) {
        match held {
            Held::Object(i) => {
                let o = &self.carve.objects[i];
                let location = Location::Object {
                    id: o.declared_id,
                    span: Some(o.span),
                };
                (location, o.span.start, Some(o.declared_id))
            }
            Held::Orphan(i) => {
                let span = self.carve.orphans[i].span();
                (Location::Span(span), span.start, None)
            }
        }
    }

    fn name(&self, held: Held) -> String {
        match self.place(held).2 {
            Some((n, g)) => format!("{n} {g} obj"),
            None => format!("the headerless object at byte {}", self.place(held).1),
        }
    }
}

fn slice(bytes: &[u8], s: ByteSpan) -> &[u8] {
    let end = usize::try_from(s.end)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let start = usize::try_from(s.start).unwrap_or(usize::MAX).min(end);
    &bytes[start..end]
}

/// The in-use entries of a classic table: subsections of `first count`,
/// each followed by `offset generation n|f` rows. Reading stops at the first
/// token that does not fit.
fn table_entries(span: &[u8]) -> Vec<(u64, u64, u64)> {
    let body = span.strip_prefix(b"xref").unwrap_or(span);
    let mut toks = body.split(|b| lexer::is_ws(*b)).filter(|t| !t.is_empty());
    let num = |t: Option<&[u8]>| -> Option<u64> { std::str::from_utf8(t?).ok()?.parse().ok() };
    let mut out = Vec::new();
    'sections: while let (Some(first), Some(count)) = (num(toks.next()), num(toks.next())) {
        for i in 0..count {
            let (Some(off), Some(generation), Some(ty)) =
                (num(toks.next()), num(toks.next()), toks.next())
            else {
                break 'sections;
            };
            match ty {
                b"n" => out.push((first.saturating_add(i), generation, off)),
                b"f" => {}
                _ => break 'sections,
            }
        }
    }
    out
}

// ── C1 ───────────────────────────────────────────────────────────────────

fn c1(cx: &Cx<'_>) -> Option<Draft> {
    let header = cx.carve.header.as_ref();
    let in_window = header.is_some_and(|h| h.offset + 5 <= HEADER_WINDOW);
    let version_ok = header.is_some_and(|h| {
        let at = usize::try_from(h.offset)
            .unwrap_or(usize::MAX)
            .saturating_add(5);
        matches!(
            cx.bytes.get(at..at.saturating_add(3)),
            Some([a, b'.', b]) if a.is_ascii_digit() && b.is_ascii_digit()
        )
    });
    if in_window && version_ok {
        return None;
    }
    let mut evidence = vec![Evidence::HexWindow(HexWindow::new(0, cx.bytes))];
    let summary = match header {
        Some(h) if !in_window => {
            evidence.push(metric("header_offset", int(h.offset)));
            format!(
                "the %PDF- header is at byte {}, past the first KiB",
                h.offset
            )
        }
        Some(_) => "the %PDF- header's version is not x.y".to_owned(),
        None => "no %PDF- header".to_owned(),
    };
    let span = ByteSpan {
        start: 0,
        end: cx.len.min(HexWindow::MAX_BYTES as u64),
    };
    Some(
        Draft::corruption(C1Header, Severity::Error, 0, Location::Span(span))
            .summary(summary)
            .evidence(evidence),
    )
}

// ── C2 / C3 ──────────────────────────────────────────────────────────────

fn c2(cx: &Cx<'_>) -> Option<Draft> {
    let carved = cx.carve.objects.len() + cx.carve.orphans.len();
    if !cx.xrefs.is_empty() || carved == 0 {
        return None;
    }
    let mut evidence = vec![metric("objects_carved", carved as i64)];
    evidence.extend(
        cx.carve
            .startxref
            .iter()
            .filter_map(|&(_, v)| Some(metric("startxref_target", int(v?)))),
    );
    Some(
        Draft::corruption(C2XrefMissing, Severity::Error, 0, Location::File)
            .summary("no cross-reference table or stream".to_owned())
            .evidence(evidence),
    )
}

fn c3(cx: &Cx<'_>) -> Vec<Draft> {
    let mut out = Vec::new();
    let trailer = !cx.carve.xref_streams.is_empty()
        || cx
            .carve
            .trailer_spans
            .iter()
            .any(|s| trailer_dict(cx.bytes, *s).is_some());
    let mut missing = Vec::new();
    if !trailer {
        missing.push("trailer");
    }
    if cx.startxref.is_none() {
        missing.push("startxref");
    }
    if cx.carve.eof_markers.is_empty() {
        missing.push("%%EOF");
    }
    if !missing.is_empty() {
        out.push(
            Draft::corruption(C3TrailerDamaged, Severity::Error, cx.len, Location::File)
                .summary(format!("no {}", missing.join(", no ")))
                .evidence(vec![
                    Evidence::Text(format!("missing: {}", missing.join(", "))),
                    Evidence::HexWindow(cx.tail()),
                ]),
        );
    }
    out.extend(startxref_miss(cx));
    out
}

/// The last `startxref`, when it misses the nearest table or xref stream
/// and the table's entries do not share the miss (module docs).
fn startxref_miss(cx: &Cx<'_>) -> Option<Draft> {
    let (at, value) = cx.startxref?;
    let target = int(cx.base.saturating_add(value));
    let xref = cx
        .xrefs
        .iter()
        .min_by_key(|x| (target - int(x.start)).unsigned_abs())?;
    let miss = target - int(xref.start);
    if miss == 0 || body_shift(cx, xref) == Some(miss) {
        return None;
    }
    let severity = if miss.unsigned_abs() <= STARTXREF_SLACK {
        Severity::Warning
    } else {
        Severity::Error
    };
    let span = ByteSpan {
        start: at,
        end: at + b"startxref".len() as u64,
    };
    Some(
        Draft::corruption(C3TrailerDamaged, severity, at, Location::Span(span))
            .summary(format!(
                "startxref names byte {target}, {miss:+} from the cross-reference section at {}",
                xref.start
            ))
            .evidence(vec![
                metric("startxref_delta", miss),
                Evidence::HexWindow(cx.tail()),
            ]),
    )
}

/// How far the in-use entry with the highest offset misses its object's
/// header, when that object was carved at top level before the section.
fn body_shift(cx: &Cx<'_>, xref: &Xref) -> Option<i64> {
    // Each id's last top-level copy before the section.
    let mut starts: BTreeMap<ObjId, u64> = BTreeMap::new();
    for o in &cx.carve.objects {
        if o.origin == Origin::TopLevel && o.span.start < xref.start {
            starts.insert(o.declared_id, o.span.start);
        }
    }
    xref.entries
        .iter()
        .filter_map(|&(num, generation, off)| {
            let id = (u32::try_from(num).ok()?, u16::try_from(generation).ok()?);
            let actual = *starts.get(&id)?;
            Some((off, int(cx.base.saturating_add(off)) - int(actual)))
        })
        .max_by_key(|&(off, _)| off)
        .map(|(_, shift)| shift)
}

/// The dictionary after a `trailer` keyword, when one parses.
fn trailer_dict(bytes: &[u8], span: ByteSpan) -> Option<Dictionary> {
    let end = usize::try_from(span.end).ok()?.min(bytes.len());
    let from = usize::try_from(span.start).ok()? + b"trailer".len();
    match lexer::parse_value(&bytes[..end], from, 0).ok()?.value {
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

// ── C4 ───────────────────────────────────────────────────────────────────

/// What the page-tree walk met.
#[derive(Default)]
struct Walk {
    pages: u64,
    /// Why `/Kids` is broken, one line per entry.
    broken: Vec<String>,
    /// (the node met again, a summary).
    warnings: Vec<(Held, String)>,
}

fn c4(cx: &Cx<'_>) -> Vec<Draft> {
    let Some((catalog, root, root_dict)) = page_tree_root(cx) else {
        let catalogs = catalogs(cx);
        let (location, at, _) = match catalogs.first() {
            Some(&c) => cx.place(c),
            None => (Location::File, 0, None),
        };
        let mut evidence: Vec<Evidence> = catalogs
            .into_iter()
            .filter_map(|c| cx.place(c).2.map(Evidence::ObjectRef))
            .collect();
        evidence.push(metric(
            "pages_discovered",
            cx.graph.pages_in_doc_order().len() as i64,
        ));
        return vec![
            Draft::corruption(C4PageTreeBroken, Severity::Error, at, location)
                .summary("no catalog reaches a page tree".to_owned())
                .evidence(evidence),
        ];
    };

    let mut walk = Walk::default();
    let mut seen = BTreeSet::new();
    walk_tree(cx, root, 0, &mut Vec::new(), &mut seen, &mut walk);
    let declared = root_dict
        .get(b"Count")
        .ok()
        .and_then(|v| cx.resolve(v).as_i64().ok());

    let mut out: Vec<Draft> = walk
        .warnings
        .iter()
        .map(|(held, summary)| {
            let (location, at, _) = cx.place(*held);
            Draft::corruption(C4PageTreeBroken, Severity::Warning, at, location)
                .summary(summary.clone())
        })
        .collect();

    let mut reasons = walk.broken.clone();
    if declared != Some(int(walk.pages)) {
        reasons.push(match declared {
            Some(n) => format!("/Count says {n}, the tree holds {}", walk.pages),
            None => format!("no /Count; the tree holds {}", walk.pages),
        });
    }
    reasons.extend(walk.warnings.iter().map(|(_, s)| s.clone()));
    if reasons.is_empty() {
        return out;
    }
    let (location, at, root_id) = cx.place(root);
    let mut evidence: Vec<Evidence> = [cx.place(catalog).2, root_id]
        .into_iter()
        .flatten()
        .map(Evidence::ObjectRef)
        .collect();
    evidence.push(metric("pages_discovered", int(walk.pages)));
    if let Some(n) = declared {
        evidence.push(metric("count_declared", n));
    }
    evidence.push(Evidence::Text(reasons.join("; ")));
    out.push(
        Draft::corruption(C4PageTreeBroken, Severity::Error, at, location)
            .summary(format!("the page tree is broken: {}", reasons[0]))
            .evidence(evidence),
    );
    out
}

/// Every catalog, best first: the graph's valid candidates, the other
/// catalog nodes (highest number first, then latest), then catalog orphans
/// in byte order.
fn catalogs(cx: &Cx<'_>) -> Vec<Held> {
    let mut ids = cx.graph.catalog_candidates();
    let valid: BTreeSet<ObjId> = ids.iter().copied().collect();
    let mut rest: Vec<(usize, ObjId)> = cx
        .graph
        .objects_of_kind(ObjectKind::Catalog)
        .into_iter()
        .enumerate()
        .filter(|(_, id)| !valid.contains(id))
        .collect();
    rest.sort_by_key(|&(i, id)| std::cmp::Reverse((id.0, i)));
    ids.extend(rest.into_iter().map(|(_, id)| id));
    let mut out: Vec<Held> = ids
        .into_iter()
        .filter_map(|id| cx.remap.target(id))
        .collect();
    out.extend(
        cx.carve
            .orphans
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                matches!(
                    o,
                    Orphan::Dict {
                        kind: ObjectKind::Catalog,
                        ..
                    }
                )
            })
            .map(|(i, _)| Held::Orphan(i)),
    );
    out
}

/// The catalog, and the page-tree node its `/Pages` reaches with its
/// dictionary.
fn page_tree_root<'a>(cx: &Cx<'a>) -> Option<(Held, Held, &'a Dictionary)> {
    let catalogs = catalogs(cx);
    let pages_ref = |c: Held| cx.dict(c)?.get(b"Pages").ok()?.as_reference().ok();
    if let Some(found) = catalogs.iter().find_map(|&c| {
        let root = cx.remap.target(pages_ref(c)?)?;
        let d = cx.dict(root)?;
        (d.has(b"Kids") || d.has(b"Count")).then_some((c, root, d))
    }) {
        return Some(found);
    }
    // A `/Pages` that names nothing: its node lost its header (C5).
    let dangling = catalogs
        .iter()
        .copied()
        .find(|&c| pages_ref(c).is_some_and(|p| cx.remap.target(p).is_none()))?;
    let orphan = cx.carve.orphans.iter().enumerate().find_map(|(i, o)| {
        let Orphan::Dict { dict, .. } = o else {
            return None;
        };
        let parent_gone = match dict.get(b"Parent") {
            Ok(Object::Reference(p)) => cx.remap.target(*p).is_none(),
            _ => true,
        };
        (is_type(dict, b"Pages") && dict.has(b"Kids") && parent_gone)
            .then_some((Held::Orphan(i), dict))
    })?;
    Some((dangling, orphan.0, orphan.1))
}

/// Walks the tree under `held` depth first, `path` holding the nodes above.
fn walk_tree(
    cx: &Cx<'_>,
    held: Held,
    depth: usize,
    path: &mut Vec<Held>,
    seen: &mut BTreeSet<Held>,
    walk: &mut Walk,
) {
    let Some(d) = cx.dict(held) else {
        walk.broken
            .push(format!("/Kids names {}, not a dictionary", cx.name(held)));
        return;
    };
    if path.contains(&held) {
        walk.warnings.push((
            held,
            format!("/Kids lead back to {}: a cycle", cx.name(held)),
        ));
        return;
    }
    if !seen.insert(held) {
        walk.warnings.push((
            held,
            format!("{} is listed twice in the page tree", cx.name(held)),
        ));
        return;
    }
    let has_kids = d.has(b"Kids");
    if is_type(d, b"Page") || (!has_kids && !d.has(b"Type")) {
        walk.pages += 1;
        return;
    }
    if !has_kids {
        walk.broken.push(format!(
            "{} is in /Kids but is not a page-tree node",
            cx.name(held)
        ));
        return;
    }
    if depth >= MAX_TREE_DEPTH {
        walk.warnings.push((
            held,
            format!(
                "the page tree goes deeper than {MAX_TREE_DEPTH} levels below {}",
                cx.name(held)
            ),
        ));
        return;
    }
    let kids = match d.get(b"Kids").map(|v| cx.resolve(v)) {
        Ok(Object::Array(a)) => a.as_slice(),
        _ => {
            walk.broken
                .push(format!("the /Kids of {} is not an array", cx.name(held)));
            return;
        }
    };
    path.push(held);
    for kid in kids {
        match kid {
            Object::Reference(id) => match cx.remap.target(*id) {
                Some(next) => walk_tree(cx, next, depth + 1, path, seen, walk),
                None => walk.broken.push(format!(
                    "/Kids of {} names {} {} R, which no carved object matches",
                    cx.name(held),
                    id.0,
                    id.1
                )),
            },
            _ => walk.broken.push(format!(
                "/Kids of {} holds a value that is not a reference",
                cx.name(held)
            )),
        }
    }
    path.pop();
}

fn is_type(d: &Dictionary, ty: &[u8]) -> bool {
    d.get(b"Type").ok().and_then(|o| o.as_name().ok()) == Some(ty)
}

// ── C5 ───────────────────────────────────────────────────────────────────

fn c5(cx: &Cx<'_>) -> Vec<Draft> {
    cx.carve
        .orphans
        .iter()
        .map(|o| {
            let span = o.span();
            let kind = kind_name(o.kind());
            let shape = match o {
                Orphan::Dict { .. } => "dictionary",
                Orphan::Stream { .. } => "stream",
            };
            Draft::corruption(
                C5ObjectTagStripped,
                Severity::Error,
                span.start,
                Location::Span(span),
            )
            .summary(format!(
                "a {kind} {shape} at byte {} has no object header",
                span.start
            ))
            .evidence(vec![Evidence::Metric {
                name: "kind".to_owned(),
                value: MetricValue::Text(kind),
            }])
        })
        .collect()
}

/// `Font`, `Page`, `Annot`; `unknown` for an untyped object.
fn kind_name(kind: &ObjectKind) -> String {
    match kind {
        ObjectKind::Other(name) if name.is_empty() => "unknown".to_owned(),
        ObjectKind::Other(name) => name.trim_start_matches('/').to_owned(),
        known => format!("{known:?}"),
    }
}

// ── C10 ──────────────────────────────────────────────────────────────────

fn c10(cx: &Cx<'_>) -> Option<Draft> {
    let len = cx.len;
    // A stream whose data runs to the last byte, top level or orphan.
    let cut_stream = cx
        .carve
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.origin == Origin::TopLevel)
        .find_map(|(i, o)| match &o.body {
            Body::Stream {
                dict,
                data,
                length_source: LengthSource::TruncatedAtEof,
            } if data.end >= len => Some((Held::Object(i), dict, *data)),
            _ => None,
        })
        .or_else(|| {
            cx.carve
                .orphans
                .iter()
                .enumerate()
                .find_map(|(i, o)| match o {
                    Orphan::Stream {
                        dict,
                        data,
                        length_source: LengthSource::TruncatedAtEof,
                        ..
                    } if data.end >= len => Some((Held::Orphan(i), dict, *data)),
                    _ => None,
                })
        });
    if let Some((held, dict, data)) = cut_stream {
        let (location, at, _) = cx.place(held);
        let kept = data.end - data.start;
        let mut evidence = Vec::new();
        if let Some(declared) = dict
            .get(b"Length")
            .ok()
            .and_then(|v| v.as_i64().ok())
            .and_then(|n| u64::try_from(n).ok())
            .filter(|&n| n > kept)
        {
            evidence.push(Evidence::Metric {
                name: "kept_fraction".to_owned(),
                value: MetricValue::Ratio(Ratio {
                    num: kept,
                    den: declared,
                }),
            });
        }
        return Some(truncated(
            at,
            location,
            evidence,
            format!("the file ends inside the data of {}", cx.name(held)),
        ));
    }

    // Objects run to the end and no `%%EOF` follows them.
    let trimmed = cx
        .bytes
        .iter()
        .rposition(|b| !lexer::is_ws(*b))
        .map_or(0, |i| i as u64 + 1);
    if cx.carve.eof_markers.is_empty() {
        let last = cx
            .carve
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.origin == Origin::TopLevel)
            .map(|(i, o)| (o.span.end, Held::Object(i)))
            .chain(
                cx.carve
                    .orphans
                    .iter()
                    .enumerate()
                    .map(|(i, o)| (o.span().end, Held::Orphan(i))),
            )
            .filter(|&(end, _)| end >= trimmed && trimmed > 0)
            .min_by_key(|&(_, h)| cx.place(h).1);
        if let Some((_, held)) = last {
            let (location, at, _) = cx.place(held);
            return Some(truncated(
                at,
                location,
                Vec::new(),
                format!(
                    "{} runs to the end of the file and no %%EOF follows",
                    cx.name(held)
                ),
            ));
        }
    }

    // Offsets past the end.
    let past = cx
        .startxref
        .map(|(_, v)| cx.base.saturating_add(v))
        .into_iter()
        .chain(cx.xrefs.iter().flat_map(|x| {
            x.entries
                .iter()
                .map(|&(_, _, off)| cx.base.saturating_add(off))
        }))
        .filter(|&end| end >= len)
        .max()?;
    let evidence = vec![Evidence::Metric {
        name: "kept_fraction".to_owned(),
        value: MetricValue::Ratio(Ratio {
            num: len,
            den: past,
        }),
    }];
    Some(truncated(
        len,
        Location::File,
        evidence,
        format!("offsets reach byte {past}, past the end of the file at {len}"),
    ))
}

fn truncated(at: u64, location: Location, evidence: Vec<Evidence>, summary: String) -> Draft {
    let mut d = Draft::corruption(C10Truncated, Severity::Error, at, location)
        .summary(summary)
        .evidence(evidence);
    d.repair = Repairability::Partial("what came after the cut is gone".to_owned());
    d
}

// ── Encrypted and Signed ─────────────────────────────────────────────────

fn encrypted(cx: &Cx<'_>) -> Option<Draft> {
    // (where, its location, the /Encrypt value)
    let mut hits: Vec<(u64, Location, Object)> = Vec::new();
    for &span in &cx.carve.trailer_spans {
        if let Some(v) = trailer_dict(cx.bytes, span).and_then(|d| d.get(b"Encrypt").ok().cloned())
        {
            hits.push((span.start, Location::Span(span), v));
        }
    }
    for x in &cx.carve.xref_streams {
        if let Some(v) = &x.trailer.encrypt {
            let location = Location::Object {
                id: x.id,
                span: Some(x.span),
            };
            hits.push((x.span.start, location, v.clone()));
        }
    }
    hits.sort_by_key(|h| h.0);
    let (at, location, _) = hits.first()?.clone();
    let mut evidence: Vec<Evidence> = Vec::new();
    for (_, _, v) in &hits {
        let e = match v {
            Object::Reference(id) => Evidence::ObjectRef(*id),
            _ => Evidence::Text("/Encrypt is a direct dictionary".to_owned()),
        };
        if !evidence.contains(&e) {
            evidence.push(e);
        }
    }
    Some(Draft {
        at,
        kind: FindingKind::Encrypted,
        severity: Severity::Error,
        location,
        summary: "the file is encrypted".to_owned(),
        evidence,
        repair: Repairability::Unrepairable("decrypt first".to_owned()),
    })
}

fn signed(cx: &Cx<'_>) -> Option<Draft> {
    // Each holder once: objects by id (any copy), orphans by place.
    let mut ids: BTreeSet<ObjId> = BTreeSet::new();
    let mut holders: Vec<(u64, Location, Option<ObjId>)> = Vec::new();
    for o in &cx.carve.objects {
        let holds = match &o.body {
            Body::Dict(d) | Body::Stream { dict: d, .. } => dict_signs(d),
            Body::Primitive(v) => value_signs(v),
            Body::Unparsed => false,
        };
        if holds && ids.insert(o.declared_id) {
            let location = Location::Object {
                id: o.declared_id,
                span: Some(o.span),
            };
            holders.push((o.span.start, location, Some(o.declared_id)));
        }
    }
    for o in &cx.carve.orphans {
        let (Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. }) = o;
        if dict_signs(dict) {
            holders.push((o.span().start, Location::Span(o.span()), None));
        }
    }
    holders.sort_by_key(|h| h.0);
    let (at, location, _) = *holders.first()?;
    let fields = u32::try_from(holders.len()).unwrap_or(u32::MAX);
    let mut evidence: Vec<Evidence> = holders
        .iter()
        .filter_map(|h| h.2.map(Evidence::ObjectRef))
        .collect();
    evidence.push(metric("fields", i64::from(fields)));
    Some(Draft {
        at,
        kind: FindingKind::Signed { fields },
        severity: Severity::Info,
        location,
        summary: SIGNED_SUMMARY.to_owned(),
        evidence,
        repair: Repairability::NotApplicable,
    })
}

/// `d`, or a dictionary inside it, is a signature: `/Type /Sig`, or a
/// `/ByteRange` array.
fn dict_signs(d: &Dictionary) -> bool {
    is_type(d, b"Sig")
        || matches!(d.get(b"ByteRange"), Ok(Object::Array(_)))
        || d.iter().any(|(_, v)| value_signs(v))
}

/// The lexer caps nesting, so this recursion is bounded.
fn value_signs(v: &Object) -> bool {
    match v {
        Object::Dictionary(d) => dict_signs(d),
        Object::Array(a) => a.iter().any(value_signs),
        Object::Stream(s) => dict_signs(&s.dict),
        _ => false,
    }
}
