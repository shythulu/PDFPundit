//! Renumbering, reconciliation and the page tree (T-10; TD §17.2–17.3).
//!
//! [`plan_ids`] decides which carved object each output number holds:
//! 1. **Duplicates** (D-032, defaulted): copies are keyed by `(num, gen)`,
//!    the object identity of ISO 32000-1 7.3.10, as pdf.js keys them (qpdf
//!    and PDFium let the higher generation of a number win instead; noted,
//!    not adopted). Among the copies of one key the **last well-formed copy
//!    in byte order wins** ([`winning_copies`]): "last" as qpdf, PDFium,
//!    lopdf, hayro and ISO 32000-1 7.5.6 have it; "well-formed" is pdf.js's
//!    gate (a later copy that does not parse does not replace a good one),
//!    and a later stream cut by EOF does not replace a good one either, our
//!    own guard. With no well-formed copy the last copy wins. Byte order is
//!    the carve's container-offset order (D-031). The losers are
//!    [`Shadow`]s: listed for the report (forensic evidence of shadow
//!    attacks), not written unless step 3 claims one.
//! 2. **Numbers**: a winner keeps its number, except object 0 and every
//!    generation of a number but the lowest (the writer writes generation 0
//!    only). Fresh numbers start at the highest declared number plus one: the
//!    orphans first, all of them, in byte order, then the winners that need
//!    one, in byte order, then each shadow step 3 claims, as it is claimed.
//!    Object and cross-reference streams are not written (their contents are
//!    already carved) and get no number. When the numbers would pass
//!    [`MAX_OBJECT_NUMBER`], or the highest is far above the count of
//!    objects written, every written object is instead numbered `1..=n` in
//!    byte order, and [`IdRemap::renumbered`] says why.
//! 3. **Dangling references** ([`ObjectGraph::dangling_refs`]), taken one
//!    missing id at a time in the byte order of their first referrer:
//!    - (a) another generation of the same number, the nearest earlier
//!      one, else the nearest later one (pdf.js's generation fallback);
//!    - (b) else, when the key-path tail of some reference to it names a
//!      kind ([`Want`], TD's table), the unclaimed orphan of that kind
//!      nearest that referrer in bytes, ties to the earlier; when no orphan
//!      of the kind is left, the nearest unclaimed shadow of it (TD: shadows
//!      stay unreferenced "unless step 3 claims them"). The match carries
//!      `Evidence::Metric{"matched_by_position_delta"}`, the candidate's
//!      start less the referrer's;
//!    - (c) else each reference to it is [`Unmatched`], a Warning finding
//!      input (the object may be genuinely gone), and [`IdRemap::rewrite`]
//!      turns it into `null`, since its number may now be a fresh object's.
//!      A dangling `/Parent`, or a catalog's dangling `/Pages`, is not
//!      reported: the flat page tree replaces those links.
//!
//! [`rebuild_page_tree`] then lays every page under one flat `/Pages` node,
//! in document order (the catalog's tree, else each surviving root `/Pages`
//! node's tree, then the pages only byte order finds), with the inheritable
//! attributes (`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate`) resolved
//! and pinned per page so dropping the intermediate nodes loses nothing
//! (ISO 32000-1 7.7.3.4). The MediaBox chain is the page's own, the nearest
//! `/Parent` ancestor's, the modal box of the other pages (its first in
//! document order on a tie), then the configured default page size.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use lopdf::{Dictionary, Object};

use crate::engine::PageSize;
use crate::pdf::carver::{Body, CarveReport, Orphan};
use crate::pdf::graph::{KeyPath, MAX_TREE_DEPTH, ObjectGraph, PathSeg, winning_copies};
use crate::pdf::model::{ByteSpan, Evidence, Location, MetricValue, ObjId, ObjectKind, Severity};

/// A carved object the output can hold: an index into
/// [`CarveReport::objects`] or into [`CarveReport::orphans`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Held {
    Object(usize),
    Orphan(usize),
}

/// A copy that lost to a later one of the same key (D-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Shadow {
    pub(crate) id: ObjId,
    /// Its index in [`CarveReport::objects`].
    pub(crate) at: usize,
    pub(crate) span: ByteSpan,
}

/// How a missing id was matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchedBy {
    /// The same number with this other generation.
    Generation { generation: u16 },
    /// The nearest candidate of the expected kind, `delta` bytes from the
    /// referrer (negative when the candidate comes first).
    Position { delta: i64 },
}

/// A missing id that now names a carved object.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reconciled {
    pub(crate) missing: ObjId,
    /// The reference the match was made for.
    pub(crate) from: ObjId,
    pub(crate) path: KeyPath,
    pub(crate) target: Held,
    pub(crate) by: MatchedBy,
}

impl Reconciled {
    /// What the report records for the match.
    pub(crate) fn evidence(&self) -> Vec<Evidence> {
        let metric = match self.by {
            MatchedBy::Generation { generation } => Evidence::Metric {
                name: "matched_by_generation".into(),
                value: MetricValue::Int(i64::from(generation)),
            },
            MatchedBy::Position { delta } => Evidence::Metric {
                name: "matched_by_position_delta".into(),
                value: MetricValue::Int(delta),
            },
        };
        vec![
            Evidence::ObjectRef(self.missing),
            Evidence::Text(path_text(&self.path)),
            metric,
        ]
    }
}

/// A reference to an id nothing could be matched to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Unmatched {
    pub(crate) from: ObjId,
    /// The referrer's span (every referrer is a graph node, so it has one).
    pub(crate) from_span: Option<ByteSpan>,
    pub(crate) path: KeyPath,
    pub(crate) missing: ObjId,
}

/// The parts of a [`crate::pdf::model::Finding`] an [`Unmatched`] reference
/// supplies; diagnosis gives it its id, class and repairability.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FindingInput {
    pub(crate) severity: Severity,
    pub(crate) location: Location,
    pub(crate) summary: String,
    pub(crate) evidence: Vec<Evidence>,
}

impl Unmatched {
    /// A Warning: the object may be genuinely gone.
    pub(crate) fn finding_input(&self) -> FindingInput {
        let (n, g) = self.missing;
        let path = path_text(&self.path);
        FindingInput {
            severity: Severity::Warning,
            location: Location::Object {
                id: self.from,
                span: self.from_span,
            },
            summary: format!(
                "{path} of {} {} obj names {n} {g} R, which no carved object matches",
                self.from.0, self.from.1
            ),
            evidence: vec![Evidence::ObjectRef(self.missing), Evidence::Text(path)],
        }
    }
}

/// Which carved object each output number holds, and what each reference
/// becomes (module docs).
#[derive(Debug, Clone, Default)]
pub(crate) struct IdRemap {
    /// What a reference to each id names: the winners, and every missing id
    /// step 3 matched.
    targets: BTreeMap<ObjId, Held>,
    /// The output number of everything written.
    numbers: BTreeMap<Held, u32>,
    shadows: Vec<Shadow>,
    reconciled: Vec<Reconciled>,
    unmatched: Vec<Unmatched>,
    next_free: u32,
    /// Set when a fresh number would pass `u32::MAX`.
    exhausted: bool,
    renumbered: Option<Renumbering>,
}

/// Why every written object was numbered afresh, `1..=n` in byte order,
/// instead of keeping its declared number (module docs, step 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Renumbering {
    /// The numbers would have passed [`MAX_OBJECT_NUMBER`] (or `u32::MAX`).
    PastLimit,
    /// The highest number, `highest`, was far above the `count` objects
    /// written ([`is_sparse`]), which would make the cross-reference table
    /// mostly free entries.
    Sparse { highest: u32, count: u32 },
}

impl Renumbering {
    /// The report's note.
    pub(crate) fn note(&self) -> String {
        match self {
            Renumbering::PastLimit => {
                format!(
                    "object numbers passed {MAX_OBJECT_NUMBER}, so every object was renumbered \
                     in file order"
                )
            }
            Renumbering::Sparse { highest, count } => format!(
                "object numbers reached {highest} for {count} objects, so every object was \
                 renumbered in file order"
            ),
        }
    }
}

/// The highest output number: 2^23 - 1, the indirect-object limit of
/// ISO 32000-1 Annex C.2, so a reader with that limit opens the output. The
/// two numbers after the last written object (the flat `/Pages` node, a
/// synthesized catalog) must fit under it too.
pub(crate) const MAX_OBJECT_NUMBER: u32 = 8_388_607;

/// Whether `highest` is far enough above the `count` objects written that
/// the cross-reference table would be mostly free entries: more than four
/// times the count plus 1024.
fn is_sparse(highest: u32, count: u32) -> bool {
    u64::from(highest) > 4 * u64::from(count) + 1024
}

impl IdRemap {
    /// What a reference to `id` names, if anything.
    pub(crate) fn target(&self, id: ObjId) -> Option<Held> {
        self.targets.get(&id).copied()
    }

    /// The output number a reference to `id` becomes.
    pub(crate) fn number(&self, id: ObjId) -> Option<u32> {
        self.number_of(self.target(id)?)
    }

    /// The output number of `held`, when it is written.
    pub(crate) fn number_of(&self, held: Held) -> Option<u32> {
        self.numbers.get(&held).copied()
    }

    /// Everything to write, by output number.
    pub(crate) fn objects(&self) -> Vec<(u32, Held)> {
        let mut v: Vec<(u32, Held)> = self.numbers.iter().map(|(&h, &n)| (n, h)).collect();
        v.sort_unstable();
        v
    }

    /// The first number nothing holds: where new objects (the flat `/Pages`
    /// node, a synthesized catalog) start.
    pub(crate) fn next_free(&self) -> u32 {
        self.next_free
    }

    /// Set when the declared numbers were not kept (module docs, step 2):
    /// a note for the report.
    pub(crate) fn renumbered(&self) -> Option<Renumbering> {
        self.renumbered
    }

    /// The losing copies, in byte order.
    pub(crate) fn shadows(&self) -> &[Shadow] {
        &self.shadows
    }

    /// The missing ids step 3 matched, in the order it matched them.
    pub(crate) fn reconciled(&self) -> &[Reconciled] {
        &self.reconciled
    }

    /// The references step 3 could not match, in the referrers' byte order.
    pub(crate) fn unmatched(&self) -> &[Unmatched] {
        &self.unmatched
    }

    /// Rewrites every reference in `v` to its output number (generation 0),
    /// and a reference that names nothing written to `null` (ISO 32000-1
    /// 7.3.10 reads a reference to a missing object as `null`).
    pub(crate) fn rewrite(&self, v: &mut Object) {
        match v {
            Object::Reference(id) => {
                *v = match self.number(*id) {
                    Some(n) => Object::Reference((n, 0)),
                    None => Object::Null,
                }
            }
            Object::Array(a) => a.iter_mut().for_each(|o| self.rewrite(o)),
            Object::Dictionary(d) => self.rewrite_dict(d),
            Object::Stream(s) => self.rewrite_dict(&mut s.dict),
            _ => {}
        }
    }

    /// Leaves `held` out of the output: it is not written and has no number,
    /// so every reference that named it becomes `null` ([`Self::rewrite`]).
    /// The other numbers do not move.
    pub(crate) fn forget(&mut self, held: Held) {
        self.numbers.remove(&held);
    }

    /// [`Self::rewrite`] over a dictionary's values.
    pub(crate) fn rewrite_dict(&self, d: &mut Dictionary) {
        for (_, v) in d.iter_mut() {
            self.rewrite(v);
        }
    }

    /// Gives `held` the next fresh number. Past `u32::MAX` it marks the
    /// remap exhausted, and [`Self::settle_numbers`] renumbers everything.
    fn fresh(&mut self, held: Held) {
        let n = self.next_free;
        self.numbers.insert(held, n);
        match n.checked_add(1) {
            Some(m) => self.next_free = m,
            None => self.exhausted = true,
        }
    }

    /// Keeps the numbers step 2 and 3 gave, unless they ran out or are
    /// sparse: then every written object is numbered `1..=n` in byte order
    /// (its start offset, then its place in the carve).
    fn settle_numbers(&mut self, carve: &CarveReport) {
        let count = u32::try_from(self.numbers.len()).unwrap_or(u32::MAX);
        let highest = self.next_free.saturating_sub(1);
        // The flat `/Pages` node and a synthesized catalog come after.
        let room = MAX_OBJECT_NUMBER - 2;
        self.renumbered = if self.exhausted || highest > room {
            Some(Renumbering::PastLimit)
        } else if is_sparse(highest, count) {
            Some(Renumbering::Sparse { highest, count })
        } else {
            None
        };
        if self.renumbered.is_none() {
            return;
        }
        let mut order: Vec<(u64, Held)> = self
            .numbers
            .keys()
            .map(|&h| {
                let start = match h {
                    Held::Object(i) => carve.objects[i].span.start,
                    Held::Orphan(i) => carve.orphans[i].span().start,
                };
                (start, h)
            })
            .collect();
        order.sort_unstable();
        let mut next = 1u32;
        for (_, h) in order {
            self.numbers.insert(h, next);
            next = next.saturating_add(1);
        }
        self.next_free = next;
    }
}

/// The ids, numbers, shadows and reference matches of the output (module
/// docs). `graph` must be built from `carve`.
pub(crate) fn plan_ids(carve: &CarveReport, graph: &ObjectGraph) -> IdRemap {
    let winners = winning_copies(carve);
    let max_declared = carve.objects.iter().map(|o| o.declared_id.0).max();
    let mut remap = IdRemap {
        next_free: max_declared.map_or(1, |m| m.saturating_add(1)),
        ..IdRemap::default()
    };

    // Step 1: winners and shadows.
    let winner_at: BTreeSet<usize> = winners.values().copied().collect();
    for (at, o) in carve.objects.iter().enumerate() {
        if !winner_at.contains(&at) {
            remap.shadows.push(Shadow {
                id: o.declared_id,
                at,
                span: o.span,
            });
        }
    }
    let written: Vec<(ObjId, usize)> = winners
        .iter()
        .filter(|&(_, &at)| {
            !matches!(
                carve.objects[at].kind,
                ObjectKind::ObjStm | ObjectKind::XRefStream
            )
        })
        .map(|(&id, &at)| (id, at))
        .collect();
    for &(id, at) in &written {
        remap.targets.insert(id, Held::Object(at));
    }

    // Step 2: numbers. `written` is sorted by id, so the lowest generation of
    // a number comes first and keeps it.
    for i in 0..carve.orphans.len() {
        remap.fresh(Held::Orphan(i));
    }
    let mut taken = BTreeSet::new();
    let mut need_fresh = Vec::new();
    for &(id, at) in &written {
        if id.0 != 0 && taken.insert(id.0) {
            remap.numbers.insert(Held::Object(at), id.0);
        } else {
            need_fresh.push(at);
        }
    }
    need_fresh.sort_unstable();
    for at in need_fresh {
        remap.fresh(Held::Object(at));
    }

    // Step 3: dangling references, grouped by missing id; each reference
    // keeps its place in the graph's order for the unmatched list.
    // (missing, [(place, from, path)]).
    type Group = (ObjId, Vec<(usize, ObjId, KeyPath)>);
    let mut groups: Vec<Group> = Vec::new();
    let mut group_of: BTreeMap<ObjId, usize> = BTreeMap::new();
    for (i, (from, path, missing)) in graph.dangling_refs().into_iter().enumerate() {
        let g = *group_of.entry(missing).or_insert_with(|| {
            groups.push((missing, Vec::new()));
            groups.len() - 1
        });
        groups[g].1.push((i, from, path));
    }
    let declared = remap.targets.clone();
    let mut pools = Pools::new(carve, &remap.shadows);
    let mut unmatched: Vec<(usize, Unmatched)> = Vec::new();
    for (missing, refs) in groups {
        let found = generation_fallback(&declared, missing)
            .map(|(held, generation)| {
                let (_, from, path) = &refs[0];
                (
                    *from,
                    path.clone(),
                    held,
                    MatchedBy::Generation { generation },
                )
            })
            .or_else(|| {
                refs.iter().find_map(|(_, from, path)| {
                    let want = Want::of(path)?;
                    let at = referrer_span(carve, &winners, *from)?.start;
                    let (held, delta) = pools.claim_nearest(want, at)?;
                    Some((*from, path.clone(), held, MatchedBy::Position { delta }))
                })
            });
        match found {
            Some((from, path, target, by)) => {
                if remap.number_of(target).is_none() {
                    remap.fresh(target);
                }
                remap.targets.insert(missing, target);
                remap.reconciled.push(Reconciled {
                    missing,
                    from,
                    path,
                    target,
                    by,
                });
            }
            None => {
                for (i, from, path) in refs {
                    if page_tree_owns(carve, &winners, from, &path) {
                        continue;
                    }
                    let from_span = referrer_span(carve, &winners, from);
                    let u = Unmatched {
                        from,
                        from_span,
                        path,
                        missing,
                    };
                    unmatched.push((i, u));
                }
            }
        }
    }
    unmatched.sort_by_key(|&(i, _)| i);
    remap.unmatched = unmatched.into_iter().map(|(_, u)| u).collect();
    remap.settle_numbers(carve);
    remap
}

/// Whether [`rebuild_page_tree`] replaces the link at `path` in `from`:
/// every `/Parent`, and the catalog's `/Pages`, now name the flat node, so a
/// dangling one is not reported (diagnosis reports the broken tree as C4).
fn page_tree_owns(
    carve: &CarveReport,
    winners: &BTreeMap<ObjId, usize>,
    from: ObjId,
    path: &KeyPath,
) -> bool {
    match path.0.as_slice() {
        [PathSeg::Key(k)] if k == b"Parent" => true,
        [PathSeg::Key(k)] if k == b"Pages" => winners
            .get(&from)
            .is_some_and(|&at| carve.objects[at].kind == ObjectKind::Catalog),
        _ => false,
    }
}

/// Another generation of `missing`'s number among the declared ids: the
/// nearest earlier generation, else the nearest later one.
fn generation_fallback(declared: &BTreeMap<ObjId, Held>, missing: ObjId) -> Option<(Held, u16)> {
    let (num, generation) = missing;
    let earlier = declared.range((num, 0)..(num, generation)).next_back();
    let later = || {
        declared
            .range((num, generation)..=(num, u16::MAX))
            .find(|(id, _)| id.1 != generation)
    };
    earlier.or_else(later).map(|(&(_, g), &held)| (held, g))
}

/// Step 3b's candidates by the kind they answer, by start offset: the
/// orphans, and the shadows to fall back on.
struct Pools {
    orphans: BTreeMap<Want, BTreeSet<(u64, Held)>>,
    shadows: BTreeMap<Want, BTreeSet<(u64, Held)>>,
}

impl Pools {
    fn new(carve: &CarveReport, shadows: &[Shadow]) -> Self {
        let mut pools = Pools {
            orphans: BTreeMap::new(),
            shadows: BTreeMap::new(),
        };
        for (i, o) in carve.orphans.iter().enumerate() {
            if let Some(w) = Want::answered_by(o.kind()) {
                let entry = (o.span().start, Held::Orphan(i));
                pools.orphans.entry(w).or_default().insert(entry);
            }
        }
        for s in shadows {
            if let Some(w) = Want::answered_by(&carve.objects[s.at].kind) {
                let entry = (s.span.start, Held::Object(s.at));
                pools.shadows.entry(w).or_default().insert(entry);
            }
        }
        pools
    }

    /// Takes the unclaimed orphan of `want`'s kind nearest `at`, else the
    /// nearest such shadow; ties go to the earlier. Returns it with its start
    /// less `at`.
    fn claim_nearest(&mut self, want: Want, at: u64) -> Option<(Held, i64)> {
        for pool in [&mut self.orphans, &mut self.shadows] {
            let Some(set) = pool.get_mut(&want) else {
                continue;
            };
            let probe = (at, Held::Object(0));
            let before = set.range(..probe).next_back().copied();
            let after = set.range(probe..).next().copied();
            let best = match (before, after) {
                (Some(b), Some(a)) => Some(if at - b.0 <= a.0 - at { b } else { a }),
                (b, a) => b.or(a),
            };
            if let Some(entry) = best {
                set.remove(&entry);
                return Some((entry.1, signed_delta(entry.0, at)));
            }
        }
        None
    }
}

/// Where the referring copy `id` sits in the file: the graph's node for it,
/// which may be an object or cross-reference stream (those are not written,
/// so the remap does not name them).
fn referrer_span(
    carve: &CarveReport,
    winners: &BTreeMap<ObjId, usize>,
    id: ObjId,
) -> Option<ByteSpan> {
    winners.get(&id).map(|&at| carve.objects[at].span)
}

/// `to - from` in bytes, saturated to `i64`.
fn signed_delta(to: u64, from: u64) -> i64 {
    if to >= from {
        i64::try_from(to - from).unwrap_or(i64::MAX)
    } else {
        i64::try_from(from - to).map_or(i64::MIN, |d| -d)
    }
}

/// The kind a missing object must have, from the tail of a key path that
/// names it (TD §17.2's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Want {
    /// `/Contents`, or an element of a `/Contents` array.
    Content,
    /// `/FontFile`, `/FontFile2`, `/FontFile3`.
    FontFile,
    /// `/ToUnicode`: a ToUnicode stream or a CMap.
    ToUnicode,
    /// `/Kids [i]`.
    Page,
    /// `/Font /<name>`.
    Font,
    /// `/XObject /<name>`: an image or a form.
    XObject,
}

impl Want {
    const ALL: [Want; 6] = [
        Want::Content,
        Want::FontFile,
        Want::ToUnicode,
        Want::Page,
        Want::Font,
        Want::XObject,
    ];

    /// What the tail of `path` asks for; `None` when it does not say.
    pub(crate) fn of(path: &KeyPath) -> Option<Want> {
        let segs = path.0.as_slice();
        let last = segs.last()?;
        let parent = match segs.len().checked_sub(2).map(|i| &segs[i]) {
            Some(PathSeg::Key(k)) => Some(k.as_slice()),
            _ => None,
        };
        match (parent, last) {
            (_, PathSeg::Key(k)) => match k.as_slice() {
                b"Contents" => Some(Want::Content),
                b"FontFile" | b"FontFile2" | b"FontFile3" => Some(Want::FontFile),
                b"ToUnicode" => Some(Want::ToUnicode),
                _ => match parent {
                    Some(b"Font") => Some(Want::Font),
                    Some(b"XObject") => Some(Want::XObject),
                    _ => None,
                },
            },
            (Some(b"Contents"), PathSeg::Index(_)) => Some(Want::Content),
            (Some(b"Kids"), PathSeg::Index(_)) => Some(Want::Page),
            _ => None,
        }
    }

    /// Whether an object of `kind` answers this.
    pub(crate) fn admits(self, kind: &ObjectKind) -> bool {
        match self {
            Want::Content => *kind == ObjectKind::ContentStream,
            Want::FontFile => *kind == ObjectKind::FontFile,
            Want::ToUnicode => match kind {
                ObjectKind::ToUnicode => true,
                ObjectKind::Other(name) => name == "/CMap",
                _ => false,
            },
            Want::Page => *kind == ObjectKind::Page,
            Want::Font => *kind == ObjectKind::Font,
            Want::XObject => matches!(kind, ObjectKind::Image | ObjectKind::Form),
        }
    }

    /// The one want `kind` answers, if any (no kind answers two).
    fn answered_by(kind: &ObjectKind) -> Option<Want> {
        Want::ALL.into_iter().find(|w| w.admits(kind))
    }
}

/// `/Resources /Font /F1`, `/Kids [0]`; empty for a whole-value reference.
fn path_text(path: &KeyPath) -> String {
    let segs: Vec<String> = path
        .0
        .iter()
        .map(|s| match s {
            PathSeg::Key(k) => format!("/{}", String::from_utf8_lossy(k)),
            PathSeg::Index(i) => format!("[{i}]"),
        })
        .collect();
    segs.join(" ")
}

// ── the page tree ────────────────────────────────────────────────────────

/// Which rung of the MediaBox chain gave a page its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoxSource {
    /// The page's own `/MediaBox`.
    Own,
    /// The nearest `/Parent` ancestor's.
    Inherited,
    /// The box most of the other pages have.
    ModalSibling,
    /// The configured default page size.
    Default,
}

/// One page of the flat tree, with its inheritable attributes pinned.
/// `resources` and `cropbox` hold carved values, so their references are
/// still the carve's: pass them through [`IdRemap::rewrite`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PagePlan {
    /// The page's output number.
    pub(crate) id: u32,
    pub(crate) mediabox: [Object; 4],
    /// The page's `/Resources` value, or the nearest ancestor's; `None` when
    /// there is none.
    pub(crate) resources: Option<Object>,
    pub(crate) cropbox: Option<[Object; 4]>,
    pub(crate) rotate: Option<i64>,
    /// Where `mediabox` came from.
    pub(crate) source: BoxSource,
}

/// The document catalog the output uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CatalogPlan {
    /// This carved catalog, with `/Pages` rewired to the flat node.
    Reuse(ObjId),
    /// `<< /Type /Catalog /Pages … >>`, written new.
    Synthesize,
}

/// The output's page tree: one `/Pages` node over every page (module docs).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageTreePlan {
    /// In document order.
    pub(crate) pages: Vec<PagePlan>,
    pub(crate) catalog: CatalogPlan,
    /// The flat `/Pages` node's output number: [`IdRemap::next_free`].
    pub(crate) pages_id: u32,
    /// The catalog's output number: the reused catalog's, or the one after
    /// `pages_id`.
    pub(crate) root: u32,
}

impl PageTreePlan {
    /// `<< /Type /Pages /Kids [...] /Count n >>`, in output numbers.
    pub(crate) fn pages_dict(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Pages".to_vec()));
        let kids = self
            .pages
            .iter()
            .map(|p| Object::Reference((p.id, 0)))
            .collect();
        d.set("Kids", Object::Array(kids));
        d.set("Count", Object::Integer(self.pages.len() as i64));
        d
    }
}

/// The flat page tree (TD §17.3, module docs). `remap` must come from
/// [`plan_ids`] over the same `carve` and `graph`.
pub(crate) fn rebuild_page_tree(
    carve: &CarveReport,
    graph: &ObjectGraph,
    remap: &IdRemap,
    default_page_size: PageSize,
) -> PageTreePlan {
    let view = View { carve, remap };
    let order = view.pages_in_order(graph);

    struct Found {
        id: u32,
        mediabox: Option<([Object; 4], BoxSource)>,
        resources: Option<Object>,
        cropbox: Option<[Object; 4]>,
        rotate: Option<i64>,
    }
    let found: Vec<Found> = order
        .iter()
        .filter_map(|&held| {
            let id = remap.number_of(held)?;
            let dict = view.dict(held)?;
            Some(Found {
                id,
                mediabox: view
                    .inherited(dict, b"MediaBox", |v| view.rect(v))
                    .map(|(r, own)| {
                        (
                            r,
                            if own {
                                BoxSource::Own
                            } else {
                                BoxSource::Inherited
                            },
                        )
                    }),
                resources: view
                    .inherited(dict, b"Resources", |v| match v {
                        Object::Dictionary(_) => Some(v.clone()),
                        // A reference to nothing falls through to the ancestors.
                        Object::Reference(r) => remap.target(*r).map(|_| v.clone()),
                        _ => None,
                    })
                    .map(|(r, _)| r),
                cropbox: view
                    .inherited(dict, b"CropBox", |v| view.rect(v))
                    .map(|(r, _)| r),
                rotate: view
                    .inherited(dict, b"Rotate", |v| rotation(view.resolve(v)))
                    .map(|(r, _)| r),
            })
        })
        .collect();

    let modal = modal_box(
        found
            .iter()
            .filter_map(|f| f.mediabox.as_ref().map(|m| &m.0)),
    );
    let default = default_box(default_page_size);
    let pages = found
        .into_iter()
        .map(|f| {
            let (mediabox, source) = f.mediabox.unwrap_or_else(|| match &modal {
                Some(m) => (m.clone(), BoxSource::ModalSibling),
                None => (default.clone(), BoxSource::Default),
            });
            PagePlan {
                id: f.id,
                mediabox,
                resources: f.resources,
                cropbox: f.cropbox,
                rotate: f.rotate,
                source,
            }
        })
        .collect();

    let catalog = graph.catalog_candidates().first().copied().or_else(|| {
        let any = graph.objects_of_kind(ObjectKind::Catalog);
        any.iter()
            .enumerate()
            .max_by_key(|&(i, id)| (id.0, i))
            .map(|(_, &id)| id)
    });
    let pages_id = remap.next_free();
    let (catalog, root) = match catalog.and_then(|c| Some((c, remap.number(c)?))) {
        Some((c, n)) => (CatalogPlan::Reuse(c), n),
        None => (CatalogPlan::Synthesize, pages_id.saturating_add(1)),
    };
    PageTreePlan {
        pages,
        catalog,
        pages_id,
        root,
    }
}

/// The carve as the remap sees it: references resolve to what they now name.
struct View<'a> {
    carve: &'a CarveReport,
    remap: &'a IdRemap,
}

impl<'a> View<'a> {
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

    fn dict_of(&self, id: ObjId) -> Option<&'a Dictionary> {
        self.dict(self.remap.target(id)?)
    }

    /// `v`, or the value of the object it references (one hop).
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

    /// `v` as a rectangle: four numbers, directly or through a reference.
    fn rect(&self, v: &Object) -> Option<[Object; 4]> {
        let Object::Array(a) = self.resolve(v) else {
            return None;
        };
        let nums: [Object; 4] = a.clone().try_into().ok()?;
        nums.iter()
            .all(|n| match n {
                Object::Integer(_) => true,
                Object::Real(r) => r.is_finite(),
                _ => false,
            })
            .then_some(nums)
    }

    /// `key`'s value parsed by `parse`: the page's own (`true`), else the
    /// nearest `/Parent` ancestor's that parses (`false`).
    fn inherited<T>(
        &self,
        page: &Dictionary,
        key: &[u8],
        parse: impl Fn(&Object) -> Option<T>,
    ) -> Option<(T, bool)> {
        let mut seen = BTreeSet::new();
        let mut cur = page;
        for depth in 0..=MAX_TREE_DEPTH {
            if let Some(v) = cur.get(key).ok().and_then(&parse) {
                return Some((v, depth == 0));
            }
            let parent = cur.get(b"Parent").ok()?.as_reference().ok()?;
            if !seen.insert(parent) {
                return None;
            }
            cur = self.dict_of(parent)?;
        }
        None
    }

    /// `d`'s `/Kids` references, in order: an array, or a reference to one.
    fn kids(&self, d: &Dictionary) -> Vec<ObjId> {
        let Ok(v) = d.get(b"Kids") else {
            return Vec::new();
        };
        match self.resolve(v) {
            Object::Array(a) => a.iter().filter_map(|o| o.as_reference().ok()).collect(),
            _ => Vec::new(),
        }
    }

    /// The pages, in document order: the walk of the first catalog
    /// candidate's tree, through matched references too (so a page whose
    /// header was stripped keeps its place); with no catalog tree, the walk
    /// of each root `/Pages` node (one whose `/Parent` is absent or names
    /// nothing), in byte order; then the pages only the byte order finds,
    /// the graph's and the `/Type /Page` orphans no walk placed together,
    /// typed before untyped and each in byte order.
    fn pages_in_order(&self, graph: &ObjectGraph) -> Vec<Held> {
        let mut seen = BTreeSet::new();
        let mut pages = Vec::new();
        let root = graph
            .catalog_candidates()
            .first()
            .and_then(|&c| self.dict_of(c))
            .and_then(|c| c.get(b"Pages").ok()?.as_reference().ok())
            .and_then(|p| self.remap.target(p));
        let roots = match root {
            Some(r) => vec![r],
            None => self.root_page_nodes(),
        };
        for root in roots {
            self.walk_tree(root, &mut seen, &mut pages);
        }

        // (untyped, start, held): typed first, then byte order.
        let mut rest: Vec<(bool, u64, Held)> = Vec::new();
        for id in graph.pages_in_doc_order() {
            let Some(held) = self.remap.target(id) else {
                continue;
            };
            if let (Held::Object(i), Some(d)) = (held, self.dict(held))
                && !seen.contains(&held)
            {
                rest.push((!is_type(d, b"Page"), self.carve.objects[i].span.start, held));
            }
        }
        for (i, o) in self.carve.orphans.iter().enumerate() {
            let held = Held::Orphan(i);
            if let Orphan::Dict { dict, span, .. } = o
                && is_type(dict, b"Page")
                && !seen.contains(&held)
            {
                rest.push((false, span.start, held));
            }
        }
        rest.sort_unstable();
        for (_, _, held) in rest {
            if seen.insert(held) {
                pages.push(held);
            }
        }
        pages
    }

    /// Appends the pages under `root` to `pages` in `/Kids` order, depth
    /// first, skipping what `seen` holds, to [`MAX_TREE_DEPTH`] levels.
    fn walk_tree(&self, root: Held, seen: &mut BTreeSet<Held>, pages: &mut Vec<Held>) {
        let mut stack = vec![(root, 0usize)];
        while let Some((held, depth)) = stack.pop() {
            let Some(d) = self.dict(held) else { continue };
            if !seen.insert(held) {
                continue;
            }
            let has_kids = d.has(b"Kids");
            if is_type(d, b"Page") || (!has_kids && !d.has(b"Type")) {
                pages.push(held);
            } else if has_kids && depth < MAX_TREE_DEPTH {
                let kids: Vec<Held> = self
                    .kids(d)
                    .into_iter()
                    .filter_map(|k| self.remap.target(k))
                    .collect();
                stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
            }
        }
    }

    /// Every written `/Type /Pages` node with `/Kids` whose `/Parent` is
    /// absent or names nothing, in byte order.
    fn root_page_nodes(&self) -> Vec<Held> {
        let mut roots: Vec<(u64, Held)> = self
            .remap
            .numbers
            .keys()
            .filter_map(|&held| {
                let d = self.dict(held)?;
                let orphaned = match d.get(b"Parent") {
                    Ok(Object::Reference(p)) => self.dict_of(*p).is_none(),
                    _ => true,
                };
                (is_type(d, b"Pages") && d.has(b"Kids") && orphaned)
                    .then(|| (self.start(held), held))
            })
            .collect();
        roots.sort_unstable();
        roots.into_iter().map(|(_, h)| h).collect()
    }

    fn start(&self, held: Held) -> u64 {
        match held {
            Held::Object(i) => self.carve.objects[i].span.start,
            Held::Orphan(i) => self.carve.orphans[i].span().start,
        }
    }
}

/// A `/Rotate` value: an integer, or a real that is a whole multiple of 90.
fn rotation(v: &Object) -> Option<i64> {
    match *v {
        Object::Integer(i) => Some(i),
        Object::Real(r) => {
            let i = r as i64;
            (i as f64 == f64::from(r) && i % 90 == 0).then_some(i)
        }
        _ => None,
    }
}

fn is_type(d: &Dictionary, ty: &[u8]) -> bool {
    d.get(b"Type").ok().and_then(|o| o.as_name().ok()) == Some(ty)
}

/// The box most of `boxes` have; on a tie, the first of them.
fn modal_box<'b>(boxes: impl Iterator<Item = &'b [Object; 4]>) -> Option<[Object; 4]> {
    // Exact equality per component, an integer apart from a real.
    type Key = [(u8, i64); 4];
    fn key(b: &[Object; 4]) -> Key {
        b.each_ref().map(|o| match o {
            Object::Integer(i) => (0, *i),
            Object::Real(r) => (1, i64::from(r.to_bits())),
            _ => (2, 0),
        })
    }
    // key → (count, first index, the box).
    let mut counts: BTreeMap<Key, (usize, usize, &[Object; 4])> = BTreeMap::new();
    for (i, b) in boxes.enumerate() {
        counts.entry(key(b)).or_insert((0, i, b)).0 += 1;
    }
    counts
        .into_values()
        .max_by_key(|&(n, first, _)| (n, std::cmp::Reverse(first)))
        .map(|(_, _, b)| b.clone())
}

/// `[0 0 w h]` in points for the configured page size.
fn default_box(size: PageSize) -> [Object; 4] {
    let (w, h) = match size {
        PageSize::A4 => (595, 842),
        PageSize::Letter => (612, 792),
        PageSize::Custom { w_pt, h_pt } => (w_pt, h_pt),
    };
    [
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(i64::from(w)),
        Object::Integer(i64::from(h)),
    ]
}
