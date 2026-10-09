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
//!   is C2's. A miss that a C4, C5 or C6 cut explains is not C3: when the
//!   in-use entry with the highest offset misses its object's header by the
//!   same amount, and a C5 orphan, a C4 Error or a C6 page (whose `/Font`
//!   entry was cut, T-11b) sits at or before that object, the cut moved the
//!   table and the trailer is as it was written. Bytes inserted or deleted
//!   with no such finding to explain them are C3.
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
//! T-11b's font and stream rules, each with tests in `tests::content`. The
//! page rules read every page's content through T-09's `page_content` (its
//! `/Contents`, then the Form XObjects they draw) and T-06's `content_ops`:
//! - **C6**, one finding per (page, slot): a `Tf` selects a slot that neither
//!   the resources in force for that stream nor the page's own carry (a slot
//!   only a form uses, from its own `/Resources`, is not C6). The re-link
//!   candidates are the fonts no reference from the catalog reaches, less
//!   the descendants of another candidate; with none the slot needs a pick.
//!   The page's resources are its own `/Resources`, else the nearest
//!   `/Parent` ancestor's, read through the remap, so a node that lost its
//!   header still maps its slots. A slot mapped to `null`, or to a reference
//!   no object or orphan carries, is unmapped: C6. A slot whose font lost
//!   its header resolves to the orphan and is C5's alone. No slot is C6
//!   when the resources in force are unknown: a `/Resources` or `/Font`
//!   entry names nothing usable (its target is not a dictionary), or the
//!   page's `/Parent` chain names no object, loops or is not a reference.
//! - **C7**: a `/FontDescriptor` with no `/FontFile`, `/FontFile2` or
//!   `/FontFile3`, or one whose stream is missing, empty, all 0x20 or does not
//!   read as a font. A font never embedded (no such key) is C7 too, as #14
//!   has it. A standard-14 name without a program is how such fonts are
//!   written and gives no finding: #14's "not embedded" Warning has no
//!   `FindingKind` to carry it. A Type3 font's descriptor is skipped. A font
//!   program whose Flate data does not decode is C9's.
//! - **C8**: C7, and the font's `/ToUnicode` (a CIDFont's is on its Type0
//!   parent) is missing, empty, all 0x20 or holds no `bfchar`/`bfrange`
//!   CMap. C8 replaces C7 for that font. Both need a font pick: diagnose has
//!   no font DB to look the name up in; T-30 upgrades a name T-28's DB
//!   knows to `Auto`.
//! - **C9**: one finding per stream whose salvage is not `Clean`: `Exact` and
//!   `Accepted` repairs are Warnings repaired automatically; `Ambiguous`,
//!   `ChecksumMismatch`, `Prefix`, `Unrecoverable` and `Unsearched` are
//!   Errors repaired in part, an `Ambiguous` one listing every survivor's
//!   edits. A font stream whose stored bytes C7 or C8 found blank is theirs,
//!   not C9's: it holds no zlib data to repair. A near-miss keyword the lexer
//!   read (`C9-outside-stream`) is a Warning on its object, or on its byte
//!   span when the carver's near-miss lex found it outside every object,
//!   marked `Metric{"salvage": "OutsideStream"}`.
//! - None of these run on an encrypted file: its streams are ciphertext.
//! - **OutlinedText**: a page whose content fills paths of three or more
//!   curve segments, counted with their contours. **Type3Text**: one per
//!   (page, Type3 font) a `Tf` selects. Both Info. A Type 3 font written
//!   inline in `/Font` gives no `Type3Text`: the finding names its font by
//!   object id, and an inline font has none.
//!
//! Ids are `<code>-<nnn>` (`C2-001`, `ENC-001`, `SIG-001`), numbered per
//! code in byte order of where each finding sits, so they are stable for
//! the same input.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object};

use crate::pdf::carver::{Body, CarveNote, CarveReport, Origin, Orphan};
use crate::pdf::graph::{ContentPiece, MAX_TREE_DEPTH, ObjectGraph};
use crate::pdf::lexer::{self, LexNote};
use crate::pdf::model::{
    ByteSpan, CorruptionClass, Evidence, Finding, FindingKind, HexWindow, InteractionKind,
    LengthSource, Location, MetricValue, ObjId, ObjectKind, Ratio, Repairability, Severity,
};
use crate::pdf::rebuild::{Held, IdRemap, plan_ids};
use crate::pdf::streams::salvage::{CarveSource, Edit, Grade, Salvage, SalvageIndex};
use crate::pdf::streams::{DEFAULT_CAP, content_ops};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped,
    C6FontMapLost, C7FontStreamDeleted, C8FontResourcesDeleted, C9ZlibTampered, C10Truncated,
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
    let c4 = c4(&cx);
    let c5 = c5(&cx);
    let encrypted = encrypted(&cx);
    // An encrypted file's streams are ciphertext: its fonts, content and
    // Flate data cannot be read, so the content rules do not run.
    let content = match encrypted {
        Some(_) => Vec::new(),
        None => content(&cx, salvage),
    };
    // Where a C4, C5 or C6 cut may have moved the bytes after it.
    let cuts: Vec<u64> = c4
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .chain(&c5)
        .chain(
            content
                .iter()
                .filter(|d| d.kind == FindingKind::Corruption(C6FontMapLost)),
        )
        .map(|d| d.at)
        .collect();
    if !truncated {
        drafts.extend(c2(&cx));
        drafts.extend(c3(&cx, &cuts));
    }
    drafts.extend(c4);
    drafts.extend(c5);
    drafts.extend(encrypted);
    drafts.extend(signed(&cx));
    drafts.extend(content);
    number(drafts)
}

/// C6–C9, `OutlinedText` and `Type3Text` (module docs).
fn content(cx: &Cx<'_>, salvage: &SalvageIndex) -> Vec<Draft> {
    let mut out = pages(cx, salvage);
    let (fonts, blanked) = font_programs(cx, salvage);
    out.extend(fonts);
    out.extend(c9(cx, salvage, &blanked));
    out.extend(near_misses(cx));
    out
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
    /// The streams, as the salvage index reads them.
    source: CarveSource<'a>,
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
            source: CarveSource::new(carve, bytes),
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

    /// The dictionary of object `id`, a stream's included.
    fn dict_or_stream(&self, id: ObjId) -> Option<&'a Dictionary> {
        match self.remap.target(id)? {
            Held::Object(i) => match &self.carve.objects.get(i)?.body {
                Body::Dict(d) | Body::Stream { dict: d, .. } => Some(d),
                _ => None,
            },
            Held::Orphan(i) => {
                let (Orphan::Dict { dict, .. } | Orphan::Stream { dict, .. }) =
                    self.carve.orphans.get(i)?;
                Some(dict)
            }
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

    /// `v` as a dictionary: itself, or the carved dictionary it references.
    fn dict_of<'v>(&self, v: &'v Object) -> Option<&'v Dictionary>
    where
        'a: 'v,
    {
        match v {
            Object::Dictionary(d) => Some(d),
            Object::Reference(id) => self.dict(self.remap.target(*id)?),
            _ => None,
        }
    }

    /// The copy of stream `id` the salvage index reads: the object's span,
    /// its data's span and the raw data.
    fn stream(&self, id: ObjId) -> Option<(ByteSpan, ByteSpan, &'a [u8])> {
        let o = &self.carve.objects[self.source.last_index(id)?];
        match &o.body {
            Body::Stream { data, .. } => Some((o.span, *data, slice(self.bytes, *data))),
            _ => None,
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

/// `cuts` are where the C4 Errors and C5 findings sit.
fn c3(cx: &Cx<'_>, cuts: &[u64]) -> Vec<Draft> {
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
    out.extend(startxref_miss(cx, cuts));
    out
}

/// The last `startxref`, when it misses the nearest table or xref stream
/// and no cut in `cuts` explains the miss (module docs).
fn startxref_miss(cx: &Cx<'_>, cuts: &[u64]) -> Option<Draft> {
    let (at, value) = cx.startxref?;
    let target = int(cx.base.saturating_add(value));
    let xref = cx
        .xrefs
        .iter()
        .min_by_key(|x| (target - int(x.start)).unsigned_abs())?;
    let miss = target - int(xref.start);
    if miss == 0 {
        return None;
    }
    if let Some((shift, moved)) = body_shift(cx, xref)
        && shift == miss
        && cuts.iter().any(|&at| at <= moved)
    {
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
/// header, when that object was carved at top level before the section, and
/// where that object's header is.
fn body_shift(cx: &Cx<'_>, xref: &Xref) -> Option<(i64, u64)> {
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
            Some((off, int(cx.base.saturating_add(off)) - int(actual), actual))
        })
        .max_by_key(|&(off, _, _)| off)
        .map(|(_, shift, actual)| (shift, actual))
}

/// The dictionary after a `trailer` keyword, when one parses.
pub(crate) fn trailer_dict(bytes: &[u8], span: ByteSpan) -> Option<Dictionary> {
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
                "a headerless {shape} (kind: {kind}) at byte {}",
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
            .and_then(|v| cx.resolve(v).as_i64().ok())
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

// ── pages: C6, OutlinedText, Type3Text ──────────────────────────────────

/// A filled path with at least this many curve segments is drawn text
/// (TD §5.6).
const OUTLINE_CURVES: u32 = 3;

/// What one content stream draws, as the page rules read it.
#[derive(Default)]
struct Scan {
    /// The slots its `Tf` operators select.
    slots: BTreeSet<Vec<u8>>,
    /// Filled paths of at least [`OUTLINE_CURVES`] curves.
    paths: u32,
    /// Their subpaths (`m` and `re`).
    contours: u32,
    /// Its `Do` operators that name an XObject, written again as a content
    /// stream: what `page_content`'s `Do` search reads on a second visit.
    dos: Vec<u8>,
}

fn scan(content: &[u8]) -> Scan {
    let mut s = Scan::default();
    let (mut curves, mut contours) = (0u32, 0u32);
    for op in content_ops(content) {
        match op.op {
            b"Tf" => {
                if let [.., Object::Name(slot), _] = op.operands.as_slice() {
                    s.slots.insert(slot.clone());
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = op.operands.last() {
                    push_name(&mut s.dos, name);
                    s.dos.extend_from_slice(b" Do\n");
                }
            }
            b"m" | b"re" => contours = contours.saturating_add(1),
            b"c" | b"v" | b"y" => curves = curves.saturating_add(1),
            // Every painting operator ends the path; the fills count.
            b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"S" | b"s" | b"n" => {
                let filled = !matches!(op.op, b"S" | b"s" | b"n");
                if filled && curves >= OUTLINE_CURVES {
                    s.paths = s.paths.saturating_add(1);
                    s.contours = s.contours.saturating_add(contours);
                }
                (curves, contours) = (0, 0);
            }
            _ => {}
        }
    }
    s
}

/// `/name`, every byte but a letter or digit as `#xx`, so the lexer reads
/// back the same name.
fn push_name(out: &mut Vec<u8>, name: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'/');
    for &b in name {
        if b.is_ascii_alphanumeric() {
            out.push(b);
        } else {
            out.extend_from_slice(&[b'#', HEX[usize::from(b >> 4)], HEX[usize::from(b & 15)]]);
        }
    }
}

/// The resources dictionary in force for a stream: `None` when it is
/// unknown, `Some(None)` when none is, `Some(Some(d))` when `d` is.
type InForce<'d> = Option<Option<&'d Dictionary>>;

/// What `resources` maps `slot` to: `Some(None)` when it has no `/Font`, or
/// its `/Font` lacks the slot or maps it to `null` or to a reference no
/// object or orphan carries (ISO 32000-1 7.3.10: both read as `null`, and a
/// `null` entry as an absent one); `None` when its `/Font` names nothing
/// usable.
fn font_slot<'v>(
    cx: &Cx<'v>,
    resources: &'v Dictionary,
    slot: &[u8],
) -> Option<Option<&'v Object>> {
    let names_nothing = |v: &Object| match v {
        Object::Null => true,
        Object::Reference(id) => cx.remap.target(*id).is_none(),
        _ => false,
    };
    match resources.get(b"Font") {
        Err(_) => Some(None),
        Ok(v) => Some(cx.dict_of(v)?.get(slot).ok().filter(|v| !names_nothing(v))),
    }
}

/// The resources in force for `page`'s own content: its `/Resources`, else
/// the nearest `/Parent` ancestor's, each read through the remap, so an
/// ancestor that lost its header (C5) still counts. Unknown when an entry
/// names nothing usable (the break is C5's or C4's), or when the `/Parent`
/// chain names no object, is not a reference, loops or runs past
/// [`MAX_TREE_DEPTH`]: none of those is a lost font map.
fn page_resources<'a>(cx: &Cx<'a>, page: ObjId) -> InForce<'a> {
    let mut seen = BTreeSet::new();
    let mut cur = page;
    for _ in 0..=MAX_TREE_DEPTH {
        if !seen.insert(cur) {
            return None;
        }
        let d = cx.dict_or_stream(cur)?;
        if let Ok(v) = d.get(b"Resources") {
            return cx.dict_of(v).map(Some);
        }
        match d.get(b"Parent") {
            Err(_) => return Some(None),
            Ok(Object::Reference(p)) => cur = *p,
            Ok(_) => return None,
        }
    }
    None
}

/// The resources `piece`'s slots resolve against. A form's own
/// `/Resources` is read through the remap from the form; anything else
/// (the page's `/Contents`, a form with no `/Resources` of its own) uses
/// `page`, the page's.
fn piece_resources<'p>(cx: &Cx<'p>, piece: &'p ContentPiece, page: InForce<'p>) -> InForce<'p> {
    match piece.resources.owner {
        Some(form) if form == piece.stream || piece.via.contains(&form) => {
            match cx
                .dict_or_stream(form)
                .and_then(|d| d.get(b"Resources").ok())
            {
                Some(v) => cx.dict_of(v).map(Some),
                None => Some(Some(&piece.resources.dict)),
            }
        }
        _ => page,
    }
}

fn pages(cx: &Cx<'_>, salvage: &SalvageIndex) -> Vec<Draft> {
    // Every stream is decoded and scanned once, on its first visit, however
    // many pages draw it. Later visits hand `page_content` only the scan's
    // `Do` operators, so no decoded stream is kept or copied.
    let scans: RefCell<BTreeMap<ObjId, Option<Scan>>> = RefCell::default();
    let decode = |id: ObjId| salvage.decoded(&cx.source, id, DEFAULT_CAP).ok();
    let visit = |id: ObjId| {
        if let Some(seen) = scans.borrow().get(&id) {
            return seen.as_ref().map(|s| Cow::Owned(s.dos.clone()));
        }
        let bytes = decode(id);
        scans.borrow_mut().insert(id, bytes.as_deref().map(scan));
        bytes
    };
    let mut candidates: Option<Vec<ObjId>> = None;
    let mut out = Vec::new();
    for (index, page) in cx.graph.pages_in_doc_order().into_iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let pieces: Vec<ContentPiece> = cx.graph.page_content(cx.carve, page, visit);
        let mut scans = scans.borrow_mut();
        // The page's own resources: what its `/Contents` draw with.
        let own = page_resources(cx, page);
        let mut missing: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut type3: BTreeSet<ObjId> = BTreeSet::new();
        let (mut paths, mut contours) = (0u32, 0u32);
        for piece in &pieces {
            // `page_content` does not decode the deepest forms it lists.
            let Some(scan) = scans
                .entry(piece.stream)
                .or_insert_with(|| decode(piece.stream).map(|b| scan(&b)))
            else {
                continue;
            };
            paths = paths.saturating_add(scan.paths);
            contours = contours.saturating_add(scan.contours);
            let here = piece_resources(cx, piece, own);
            for slot in &scan.slots {
                let mut value = None;
                let mut unknown = false;
                for resources in [here, own] {
                    match resources {
                        None => unknown = true,
                        Some(None) => {}
                        Some(Some(r)) => match font_slot(cx, r, slot) {
                            Some(Some(v)) => {
                                value = Some(v);
                                break;
                            }
                            Some(None) => {}
                            None => unknown = true,
                        },
                    }
                }
                match value {
                    // An inline Type 3 font has no id for `Type3Text`.
                    Some(v @ Object::Reference(font))
                        if cx.dict_of(v).is_some_and(|d| is_subtype(d, b"Type3")) =>
                    {
                        type3.insert(*font);
                    }
                    Some(_) => {}
                    None if !unknown => {
                        missing.insert(slot.clone());
                    }
                    None => {}
                }
            }
        }

        let at = cx.remap.target(page).map_or(0, |h| cx.place(h).1);
        let location = Location::Page {
            index,
            obj: Some(page),
        };
        for slot in missing {
            let candidates = candidates.get_or_insert_with(|| orphan_fonts(cx.graph));
            let slot = String::from_utf8_lossy(&slot);
            let mut evidence = vec![Evidence::Text(format!("slots: {slot}"))];
            evidence.extend(candidates.iter().copied().map(Evidence::ObjectRef));
            let mut d = Draft::corruption(C6FontMapLost, Severity::Error, at, location)
                .summary(format!(
                    "page {} selects font /{slot}, which its resources do not map",
                    u64::from(index) + 1
                ))
                .evidence(evidence);
            if candidates.is_empty() {
                d.repair = Repairability::Interactive(InteractionKind::FontPick);
            }
            out.push(d);
        }
        if paths > 0 {
            out.push(Draft {
                at,
                kind: FindingKind::OutlinedText {
                    glyph_runs: paths,
                    contours,
                },
                severity: Severity::Info,
                location,
                summary: format!(
                    "text drawn as outlines on page {} ({paths} paths, {contours} contours)",
                    u64::from(index) + 1
                ),
                evidence: vec![
                    metric("paths", i64::from(paths)),
                    metric("contours", i64::from(contours)),
                ],
                repair: Repairability::NotApplicable,
            });
        }
        for font in type3 {
            out.push(Draft {
                at,
                kind: FindingKind::Type3Text { font },
                severity: Severity::Info,
                location,
                summary: format!(
                    "page {} shows text through the Type 3 font {} {} obj",
                    u64::from(index) + 1,
                    font.0,
                    font.1
                ),
                evidence: vec![Evidence::ObjectRef(font)],
                repair: Repairability::NotApplicable,
            });
        }
    }
    out
}

/// C6's re-link candidates: the fonts no reference from the catalog
/// reaches, in byte order, less those another candidate holds as a
/// descendant. None without a catalog. The C6 pass (T-13b) re-links from
/// the same list.
pub(crate) fn orphan_fonts(graph: &ObjectGraph) -> Vec<ObjId> {
    let Some(&root) = graph.catalog_candidates().first() else {
        return Vec::new();
    };
    let unreachable = graph.unreachable(root);
    let fonts: Vec<ObjId> = graph
        .objects_of_kind(ObjectKind::Font)
        .into_iter()
        .filter(|id| unreachable.contains(id))
        .collect();
    let set: BTreeSet<ObjId> = fonts.iter().copied().collect();
    fonts
        .into_iter()
        .filter(|&id| {
            !graph.referrers(id).iter().any(|e| {
                set.contains(&e.from) && e.path.first_key() == Some(&b"DescendantFonts"[..])
            })
        })
        .collect()
}

fn is_subtype(d: &Dictionary, subtype: &[u8]) -> bool {
    d.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(subtype)
}

// ── C7 / C8 ──────────────────────────────────────────────────────────────

/// The keys a descriptor embeds its font program under.
const FONT_FILES: [&[u8]; 3] = [b"FontFile", b"FontFile2", b"FontFile3"];

/// Fonts every reader carries, so a file need not embed them.
const STANDARD_14: [&str; 14] = [
    "Times-Roman",
    "Times-Bold",
    "Times-Italic",
    "Times-BoldItalic",
    "Helvetica",
    "Helvetica-Bold",
    "Helvetica-Oblique",
    "Helvetica-BoldOblique",
    "Courier",
    "Courier-Bold",
    "Courier-Oblique",
    "Courier-BoldOblique",
    "Symbol",
    "ZapfDingbats",
];

/// What a stream a font names holds.
enum Data<'s> {
    /// Nothing by that id is a stream.
    Missing,
    /// Empty, or every byte 0x20: `raw` when the stored bytes are, else
    /// only what they decode to.
    Blank {
        raw: bool,
    },
    Bytes(Cow<'s, [u8]>),
    /// It does not decode, or it lost its header: C9's or C5's.
    Unknown,
}

fn data_of<'s>(cx: &Cx<'_>, salvage: &'s SalvageIndex, id: ObjId) -> Data<'s> {
    let Some((_, _, raw)) = cx.stream(id) else {
        return match cx.remap.target(id) {
            Some(Held::Orphan(_)) => Data::Unknown,
            _ => Data::Missing,
        };
    };
    if blank(raw) {
        return Data::Blank { raw: true };
    }
    match salvage.decoded(&cx.source, id, DEFAULT_CAP) {
        Ok(b) if blank(&b) => Data::Blank { raw: false },
        Ok(b) => Data::Bytes(b),
        Err(_) => Data::Unknown,
    }
}

fn blank(b: &[u8]) -> bool {
    b.iter().all(|&x| x == 0x20)
}

/// The bytes start like a TrueType, OpenType, Type 1 or CFF font program.
fn sniffs_as_font(b: &[u8]) -> bool {
    const MAGIC: [&[u8]; 7] = [
        b"\x00\x01\x00\x00",
        b"OTTO",
        b"true",
        b"typ1",
        b"ttcf",
        b"%!",
        b"\x80\x01",
    ];
    // CFF: major version 1, a header of at least 4 bytes, offsets of 1–4.
    let cff = matches!(b, [1, _, size, off, ..] if *size >= 4 && (1..=4).contains(off));
    cff || MAGIC.iter().any(|m| b.starts_with(m))
}

/// A `/ToUnicode` CMap maps codes with `bfchar` or `bfrange`.
fn is_tounicode(b: &[u8]) -> bool {
    [&b"beginbfchar"[..], b"beginbfrange"]
        .iter()
        .any(|k| memchr::memmem::find(b, k).is_some())
}

/// `ABCDEF+Name` → `Name`.
fn without_subset_tag(name: &[u8]) -> &[u8] {
    match name.split_at_checked(7) {
        Some((tag, rest))
            if !rest.is_empty()
                && tag[6] == b'+'
                && tag[..6].iter().all(u8::is_ascii_uppercase) =>
        {
            rest
        }
        _ => name,
    }
}

/// The C7 and C8 findings, and the streams they found blank.
fn font_programs(cx: &Cx<'_>, salvage: &SalvageIndex) -> (Vec<Draft>, BTreeSet<ObjId>) {
    let mut out = Vec::new();
    let mut blanked = BTreeSet::new();
    for desc in cx.graph.objects_of_kind(ObjectKind::FontDescriptor) {
        let Some(held) = cx.remap.target(desc) else {
            continue;
        };
        let Some(d) = cx.dict(held) else { continue };
        let users: Vec<ObjId> = cx
            .graph
            .referrers(desc)
            .iter()
            .filter(|e| e.path.0.len() == 1 && e.path.first_key() == Some(&b"FontDescriptor"[..]))
            .map(|e| e.from)
            .collect();
        let font = |id: ObjId| cx.dict(cx.remap.target(id)?);
        if users
            .iter()
            .any(|&u| font(u).is_some_and(|f| is_subtype(f, b"Type3")))
        {
            continue;
        }
        let user = users.first().copied();
        let name = d
            .get(b"FontName")
            .ok()
            .or_else(|| font(user?)?.get(b"BaseFont").ok())
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(without_subset_tag(n)).into_owned());

        // C7: the program.
        let mut evidence = vec![Evidence::ObjectRef(desc)];
        let why = match FONT_FILES.iter().find_map(|&k| Some((k, d.get(k).ok()?))) {
            // The #14 table wants a Warning "not embedded" here, but no
            // FindingKind carries one: only Corruption(C7) fits, which the
            // table rules out. No finding until one exists (decision pending).
            None if name.as_deref().is_some_and(|n| STANDARD_14.contains(&n)) => continue,
            // Deliberate, per the #14 table: a font that was never embedded
            // (no `/FontFile*` key at all, a system font left out on purpose)
            // is C7, and C8 with no `/ToUnicode`. Whether such a font should
            // be a Warning instead is a pending decision.
            None => "not embedded".to_owned(),
            Some((key, Object::Reference(id))) => {
                if let Some((_, data, raw)) = cx.stream(*id) {
                    evidence.push(metric("fontfile_bytes", raw.len() as i64));
                    evidence.push(Evidence::HexWindow(HexWindow::new(data.start, raw)));
                }
                match data_of(cx, salvage, *id) {
                    Data::Missing => format!(
                        "gone: its /{} names {} {} R, which is not a stream",
                        String::from_utf8_lossy(key),
                        id.0,
                        id.1
                    ),
                    Data::Blank { raw } => {
                        if raw {
                            blanked.insert(*id);
                        }
                        "blank (empty or all 0x20)".to_owned()
                    }
                    Data::Bytes(b) if !sniffs_as_font(&b) => "not a font program".to_owned(),
                    Data::Bytes(_) | Data::Unknown => continue,
                }
            }
            Some((key, _)) => format!(
                "gone: its /{} is not a reference",
                String::from_utf8_lossy(key)
            ),
        };

        // C8: and the `/ToUnicode` (a CIDFont's is on its Type0 parent).
        let top = user.map(|u| {
            cx.graph
                .referrers(u)
                .iter()
                .find(|e| e.path.first_key() == Some(&b"DescendantFonts"[..]))
                .map_or(u, |e| e.from)
        });
        // (its reference, a note) when it is lost.
        let tounicode_lost = match top.and_then(font).and_then(|f| f.get(b"ToUnicode").ok()) {
            None => Some((None, None)),
            Some(Object::Reference(id)) => match data_of(cx, salvage, *id) {
                Data::Missing => Some((Some(*id), None)),
                Data::Blank { raw } => {
                    if raw {
                        blanked.insert(*id);
                    }
                    Some((Some(*id), None))
                }
                Data::Bytes(b) if !is_tounicode(&b) => {
                    Some((Some(*id), Some("tounicode unparsable")))
                }
                Data::Bytes(_) | Data::Unknown => None,
            },
            // A name (`/Identity-H`) is not a CMap stream, but it is not lost.
            Some(_) => None,
        };
        let name = name.unwrap_or_else(|| "an unnamed font".to_owned());
        let (location, at, _) = cx.place(held);
        let draft = match tounicode_lost {
            None => Draft::corruption(C7FontStreamDeleted, Severity::Error, at, location)
                .summary(format!("the font program of {name} is {why}")),
            Some((tounicode, note)) => {
                evidence.extend(tounicode.map(Evidence::ObjectRef));
                evidence.extend(note.map(|n| Evidence::Text(n.to_owned())));
                Draft::corruption(C8FontResourcesDeleted, Severity::Error, at, location).summary(
                    format!("the font program of {name} is {why}, and its /ToUnicode is lost"),
                )
            }
        };
        let mut draft = draft.evidence(evidence);
        // Diagnose has no font DB to look the name up in (T-28): the table's
        // Auto for a name the DB knows is for T-30's pass to upgrade to.
        draft.repair = Repairability::Interactive(InteractionKind::FontPick);
        out.push(draft);
    }
    (out, blanked)
}

// ── C9 ───────────────────────────────────────────────────────────────────

/// One finding per stream whose salvage is not `Clean`, except the font
/// streams C7 and C8 found blank: they hold no zlib data to repair.
fn c9(cx: &Cx<'_>, salvage: &SalvageIndex, blanked: &BTreeSet<ObjId>) -> Vec<Draft> {
    let mut out = Vec::new();
    for (&id, entry) in salvage
        .by_obj
        .iter()
        .filter(|(id, _)| !blanked.contains(id))
    {
        let s = &entry.salvage;
        let partial = |why: &str| Repairability::Partial(why.to_owned());
        let (name, outcome, severity, repair) = match s {
            Salvage::Clean { .. } => continue,
            Salvage::Repaired {
                grade: Grade::Exact,
                ..
            } => (
                "Repaired",
                "repaired; no other repair fits its window".to_owned(),
                Severity::Warning,
                Repairability::Auto,
            ),
            Salvage::Repaired {
                grade: Grade::Accepted { .. },
                ..
            } => (
                "Repaired",
                "repaired; the budget ran out before another repair could be ruled out".to_owned(),
                Severity::Warning,
                Repairability::Auto,
            ),
            Salvage::Repaired {
                grade: Grade::Ambiguous { outputs },
                ..
            } => (
                "Repaired",
                format!("{outputs} different repairs fit; the first in the pinned order is kept"),
                Severity::Error,
                partial(&format!("ambiguous: {outputs} candidate repairs")),
            ),
            Salvage::ChecksumMismatch { .. } => (
                "ChecksumMismatch",
                "it decodes, but its Adler-32 disagrees and no repair was found".to_owned(),
                Severity::Error,
                partial("checksum mismatch: the decoded bytes are unverified"),
            ),
            Salvage::Prefix {
                in_used, in_total, ..
            } => (
                "Prefix",
                format!("only what comes before input byte {in_used} of {in_total} decodes"),
                Severity::Error,
                partial("only the bytes before the damage decode"),
            ),
            Salvage::Unrecoverable => (
                "Unrecoverable",
                "nothing decodes and no repair was found".to_owned(),
                Severity::Error,
                partial("unrecoverable stream"),
            ),
            Salvage::Unsearched { reason } => (
                "Unsearched",
                format!(
                    "its {} raw bytes are over the {}-byte search limit, so it was not searched",
                    reason.raw_len, reason.limit
                ),
                Severity::Error,
                partial("over the search size limit: not searched"),
            ),
        };
        let mut evidence = vec![Evidence::Metric {
            name: "salvage".to_owned(),
            value: MetricValue::Text(name.to_owned()),
        }];
        if let Some(grade) = s.grade() {
            evidence.push(Evidence::Metric {
                name: "grade".to_owned(),
                value: MetricValue::Text(format!("{grade:?}")),
            });
        }
        if let Salvage::Repaired {
            edits,
            grade,
            survivors,
            work,
            trailer_edit,
            adler_rerun,
            ..
        } = s
        {
            evidence.push(metric("work", int(*work)));
            if let Grade::Accepted { searched, window } = grade {
                evidence.push(metric("searched", int(*searched)));
                evidence.push(metric("window", int(*window)));
            }
            evidence.push(Evidence::Text(format!("edits: {}", edit_list(edits))));
            if let Grade::Ambiguous { .. } = grade {
                evidence.extend(survivors.iter().enumerate().map(|(k, edits)| {
                    Evidence::Text(format!("survivor {}: {}", k + 1, edit_list(edits)))
                }));
            }
            if *trailer_edit {
                evidence.push(Evidence::Text(
                    "the repair rewrites the Adler-32 trailer".to_owned(),
                ));
            }
            if *adler_rerun {
                evidence.push(Evidence::Text(
                    "found under the Adler-32 alone, past the localizer's window".to_owned(),
                ));
            }
        }
        let span = cx.stream(id).map(|(span, _, _)| span);
        let mut d = Draft::corruption(
            C9ZlibTampered,
            severity,
            span.map_or(0, |s| s.start),
            Location::Object { id, span },
        )
        .summary(format!(
            "the Flate data of {} {} obj is damaged: {outcome}",
            id.0, id.1
        ))
        .evidence(evidence);
        d.repair = repair;
        out.push(d);
    }
    out
}

/// `byte 40 0x31 -> 0x30, ...`, offsets in the Flate stage's input.
fn edit_list(edits: &[Edit]) -> String {
    let each: Vec<String> = edits
        .iter()
        .map(|(at, from, to)| format!("byte {at} 0x{from:02x} -> 0x{to:02x}"))
        .collect();
    each.join(", ")
}

/// The `Metric{"salvage"}` of a `C9-outside-stream` finding: it names no
/// stream, so no [`SalvageIndex`] entry is behind it.
pub(crate) const OUTSIDE_STREAM: &str = "OutsideStream";

/// `C9-outside-stream`: a keyword the lexer read one byte off, in a
/// top-level object (in a packed one it is the object stream's data), or
/// outside every object, where the carver's near-miss lex (its rule 7)
/// finds it. One outside every object is located by its byte span: the run
/// of regular bytes from the note's offset, at least one byte.
fn near_misses(cx: &Cx<'_>) -> Vec<Draft> {
    let mut out = Vec::new();
    let mut warn = |at: u64, location: Location, place: String| {
        let from = usize::try_from(at)
            .unwrap_or(usize::MAX)
            .min(cx.bytes.len());
        out.push(
            Draft::corruption(C9ZlibTampered, Severity::Warning, at, location)
                .summary(format!(
                    "C9-outside-stream: a keyword one byte off at byte {at} {place}"
                ))
                .evidence(vec![
                    Evidence::Metric {
                        name: "salvage".to_owned(),
                        value: MetricValue::Text(OUTSIDE_STREAM.to_owned()),
                    },
                    Evidence::HexWindow(HexWindow::new(at, &cx.bytes[from..])),
                ]),
        );
    };
    for o in cx
        .carve
        .objects
        .iter()
        .filter(|o| o.origin == Origin::TopLevel)
    {
        for note in &o.notes {
            let CarveNote::Lex(LexNote::NearMissKeyword { at }) = *note else {
                continue;
            };
            let location = Location::Object {
                id: o.declared_id,
                span: Some(o.span),
            };
            let (num, generation) = o.declared_id;
            warn(at, location, format!("in {num} {generation} obj"));
        }
    }
    for note in &cx.carve.notes {
        let CarveNote::Lex(LexNote::NearMissKeyword { at }) = *note else {
            continue;
        };
        let from = usize::try_from(at)
            .unwrap_or(usize::MAX)
            .min(cx.bytes.len());
        let run = cx.bytes[from..]
            .iter()
            .take_while(|&&b| lexer::is_reg(b))
            .count();
        let span = ByteSpan {
            start: at,
            end: at + run.max(1) as u64,
        };
        warn(at, Location::Span(span), "outside every object".to_owned());
    }
    out
}
