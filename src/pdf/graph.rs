//! Object graph (T-09; TD §4.3, §17.1): who references whom among the carved
//! objects, hand-rolled over sorted vectors (petgraph is rejected, TD §4.3:
//! everything here is a lookup or a walk).
//!
//! - **Nodes** are object ids. When the carve holds several copies of one id,
//!   the node is the last well-formed copy in container-offset order (a copy
//!   that is `Unparsed`, or a stream cut by EOF, does not replace an earlier
//!   one), else the last copy: D-032's rule, which T-10 applies to the
//!   rebuild. The copies that lose are not nodes and their references are not
//!   edges. Orphans have no id and are not nodes.
//! - **Edges** are every `N G R` in a node's value (a stream's dictionary,
//!   never its data), each carrying its key path from the value's root, e.g.
//!   `/Resources /Font /F1` or `/Kids [0]`. Key paths are interned: each
//!   distinct path is allocated once and shared by its edges.
//! - **Byte order** is the carve's container-offset order: a packed object
//!   sits where its object stream does.
//! - **Catalog candidates** are `/Type /Catalog` dictionaries whose `/Pages`
//!   resolves to a dictionary carrying `/Kids` or `/Count` (pdf.js and hayro
//!   validate the Pages link), the highest object number first and, within
//!   one number, the last in byte order first (qpdf prefers the
//!   highest-numbered valid catalog).
//! - **Pages in document order** walk the first candidate's page tree in
//!   `/Kids` order, depth first so that pages come out in reading order,
//!   with a visited set (a cycle terminates, a page listed twice appears
//!   once) and a depth cap of [`MAX_TREE_DEPTH`]. A tree node is a page when
//!   it is `/Type /Page`, or has neither `/Type` nor `/Kids` (pdf.js treats a
//!   node without `/Kids` as a page). Then come, in byte order, the pages the
//!   walk did not reach: `/Type /Page` dictionaries first, then dictionaries
//!   with no `/Type` and no `/Kids` whose `/Contents` is a reference or an
//!   array (hayro requires only `/Contents`; a text string there is an
//!   annotation's). `/MediaBox` is not required.
//! - **Page content** ([`ObjectGraph::page_content`]) is the page's
//!   `/Contents` in order, then the Form XObjects their `Do` operators reach
//!   (found with T-06's `content_ops`), each with the resources in force for
//!   it, breadth first, to a `Do` depth of [`MAX_FORM_DEPTH`], with a visited
//!   set per page so a form that draws itself terminates.
#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
mod tests;

use std::borrow::{Borrow, Cow};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

use lopdf::{Dictionary, Object};

use crate::pdf::carver::{Body, CarveReport, CarvedObject};
use crate::pdf::model::{LengthSource, ObjId, ObjectKind, Ratio};
use crate::pdf::streams::content_ops;

/// Deepest `/Kids` level the page-tree walk descends to.
pub(crate) const MAX_TREE_DEPTH: usize = 100;
/// Longest `Do` chain [`ObjectGraph::page_content`] follows.
pub(crate) const MAX_FORM_DEPTH: usize = 16;
/// Longest `/Parent` chain searched for inherited `/Resources`.
const MAX_PARENT_DEPTH: usize = MAX_TREE_DEPTH;

/// One step from a value's root to a reference inside it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PathSeg {
    /// A dictionary key, without the slash.
    Key(Vec<u8>),
    /// An array index, from 0.
    Index(u32),
}

/// Where in an object's value a reference sits, e.g. `/Resources /Font /F1`.
/// Empty for an object whose whole value is a reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct KeyPath(pub(crate) Vec<PathSeg>);

impl Borrow<[PathSeg]> for KeyPath {
    fn borrow(&self) -> &[PathSeg] {
        &self.0
    }
}

impl KeyPath {
    /// The key of the first step, when it is a key.
    pub(crate) fn first_key(&self) -> Option<&[u8]> {
        match self.0.first() {
            Some(PathSeg::Key(k)) => Some(k),
            _ => None,
        }
    }

    fn heap_bytes(&self) -> u64 {
        let segs: u64 = self
            .0
            .iter()
            .map(|s| match s {
                PathSeg::Key(k) => k.capacity() as u64,
                PathSeg::Index(_) => 0,
            })
            .sum();
        (self.0.capacity() * size_of::<PathSeg>()) as u64 + segs
    }
}

/// A reference from `from` to `to`, at `path` in `from`'s value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefEdge {
    pub(crate) from: ObjId,
    pub(crate) to: ObjId,
    pub(crate) path: Arc<KeyPath>,
}

/// The resources dictionary in force for a [`ContentPiece`].
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Resources {
    /// The dictionary, resolved when `/Resources` is a reference; empty when
    /// none is in force or the value does not resolve to a dictionary.
    pub(crate) dict: Dictionary,
    /// The object whose `/Resources` entry this is: the page, the `/Pages`
    /// ancestor it is inherited from, or the Form XObject. `None` when no
    /// `/Resources` is in force.
    pub(crate) owner: Option<ObjId>,
}

/// One content stream a page draws.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContentPiece {
    pub(crate) stream: ObjId,
    pub(crate) resources: Resources,
    /// The streams the `Do` chain went through to reach this one, outermost
    /// first: empty for the page's own `/Contents`, `[c]` for a form drawn
    /// from page content stream `c`.
    pub(crate) via: Vec<ObjId>,
}

/// What [`ObjectGraph`] keeps of a node's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Shape {
    dict: bool,
    stream: bool,
    has_type: bool,
    has_kids: bool,
    has_count: bool,
    /// `/Contents` is a reference or an array.
    has_contents: bool,
}

#[derive(Debug)]
struct Node {
    id: ObjId,
    /// The chosen copy's index in `CarveReport::objects`: byte order.
    at: u32,
    kind: ObjectKind,
    shape: Shape,
    /// Into `out_edges`.
    out: Range<u32>,
}

/// References between carved objects (module docs).
#[derive(Debug, Default)]
pub(crate) struct ObjectGraph {
    /// Sorted by id.
    nodes: Vec<Node>,
    /// Every edge, sorted by (target, source byte order, source, path).
    edges: Vec<RefEdge>,
    /// Each node's out-edges as indices into `edges`, in the order the
    /// references occur in its value; a node's slice is `Node::out`.
    out_edges: Vec<u32>,
    /// Each distinct key path once.
    paths: Vec<Arc<KeyPath>>,
}

impl ObjectGraph {
    /// One node per object id (the copy D-032 keeps, [`winning_copies`])
    /// and one edge per reference in its value. Linear in the carve's references, plus a sort.
    pub(crate) fn from_carve(carve: &CarveReport) -> Self {
        let chosen = winning_copies(carve);

        let mut interner: BTreeMap<KeyPath, Arc<KeyPath>> = BTreeMap::new();
        let mut raw: Vec<(u32, RefEdge)> = Vec::new();
        let mut nodes = Vec::with_capacity(chosen.len());
        for (&id, &at) in &chosen {
            let o = &carve.objects[at];
            let start = raw.len() as u32;
            let mut stack = Vec::new();
            let mut emit = |path: &[PathSeg], to: ObjId| {
                let path = match interner.get(path) {
                    Some(p) => p.clone(),
                    None => {
                        let p = Arc::new(KeyPath(path.to_vec()));
                        interner.insert(KeyPath(path.to_vec()), p.clone());
                        p
                    }
                };
                raw.push((at as u32, RefEdge { from: id, to, path }));
            };
            let shape = match &o.body {
                Body::Dict(d) => {
                    walk_dict(d, &mut stack, &mut emit);
                    dict_shape(d)
                }
                Body::Stream { dict, .. } => {
                    walk_dict(dict, &mut stack, &mut emit);
                    Shape {
                        stream: true,
                        ..Shape::default()
                    }
                }
                Body::Primitive(v) => {
                    walk(v, &mut stack, &mut emit);
                    Shape::default()
                }
                Body::Unparsed => Shape::default(),
            };
            nodes.push(Node {
                id,
                at: at as u32,
                kind: o.kind.clone(),
                shape,
                out: start..raw.len() as u32,
            });
        }

        let mut order: Vec<u32> = (0..raw.len() as u32).collect();
        order.sort_by(|&a, &b| {
            let (aa, ea) = &raw[a as usize];
            let (ab, eb) = &raw[b as usize];
            (ea.to, aa, ea.from, &ea.path).cmp(&(eb.to, ab, eb.from, &eb.path))
        });
        let mut out_edges = vec![0u32; raw.len()];
        for (sorted, &discovered) in order.iter().enumerate() {
            out_edges[discovered as usize] = sorted as u32;
        }
        let mut slots: Vec<Option<RefEdge>> = raw.into_iter().map(|(_, e)| Some(e)).collect();
        let edges = order
            .iter()
            .map(|&i| slots[i as usize].take().expect("each edge once"))
            .collect();
        ObjectGraph {
            nodes,
            edges,
            out_edges,
            paths: interner.into_values().collect(),
        }
    }

    /// Capacity of the node and edge vectors and of every interned key path,
    /// in bytes.
    pub(crate) fn heap_bytes(&self) -> u64 {
        let kinds: u64 = self
            .nodes
            .iter()
            .map(|n| match &n.kind {
                ObjectKind::Other(s) => s.capacity() as u64,
                _ => 0,
            })
            .sum();
        // An `Arc`'s two counts, then the path.
        let arc = 2 * size_of::<usize>() + size_of::<KeyPath>();
        let paths: u64 = self.paths.iter().map(|p| arc as u64 + p.heap_bytes()).sum();
        vec_bytes(&self.nodes)
            + kinds
            + vec_bytes(&self.edges)
            + vec_bytes(&self.out_edges)
            + vec_bytes(&self.paths)
            + paths
    }

    /// Valid catalogs, best first (module docs).
    pub(crate) fn catalog_candidates(&self) -> Vec<ObjId> {
        let mut found: Vec<&Node> = self
            .nodes
            .iter()
            .filter(|n| n.shape.dict && n.kind == ObjectKind::Catalog)
            .filter(|n| {
                self.out_of(n)
                    .find(|e| is_key(&e.path, b"Pages"))
                    .and_then(|e| self.node(e.to))
                    .is_some_and(|p| p.shape.dict && (p.shape.has_kids || p.shape.has_count))
            })
            .collect();
        found.sort_by_key(|n| Reverse((n.id.0, n.at)));
        found.into_iter().map(|n| n.id).collect()
    }

    /// Every node a chain of references from `root` reaches, `root` included
    /// (empty when `root` is not a node).
    pub(crate) fn reachable(&self, root: ObjId) -> BTreeSet<ObjId> {
        let mut seen = BTreeSet::new();
        let Some(start) = self.node(root) else {
            return seen;
        };
        seen.insert(start.id);
        let mut queue = VecDeque::from([start]);
        while let Some(n) = queue.pop_front() {
            for e in self.out_of(n) {
                if let Some(t) = self.node(e.to)
                    && seen.insert(t.id)
                {
                    queue.push_back(t);
                }
            }
        }
        seen
    }

    /// Every node [`Self::reachable`] does not reach from `root`.
    pub(crate) fn unreachable(&self, root: ObjId) -> BTreeSet<ObjId> {
        let reached = self.reachable(root);
        self.nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| !reached.contains(id))
            .collect()
    }

    /// Every node of `kind`, in byte order.
    pub(crate) fn objects_of_kind(&self, kind: ObjectKind) -> Vec<ObjId> {
        let mut found: Vec<&Node> = self.nodes.iter().filter(|n| n.kind == kind).collect();
        found.sort_by_key(|n| n.at);
        found.into_iter().map(|n| n.id).collect()
    }

    /// Reachable nodes over all nodes.
    pub(crate) fn reachable_fraction(&self, root: ObjId) -> Ratio {
        Ratio {
            num: self.reachable(root).len() as u64,
            den: self.nodes.len() as u64,
        }
    }

    /// The references to `id`, by the referring node's byte order and then
    /// key path; `id` need not be a node.
    pub(crate) fn referrers(&self, id: ObjId) -> &[RefEdge] {
        let lo = self.edges.partition_point(|e| e.to < id);
        let hi = self.edges.partition_point(|e| e.to <= id);
        &self.edges[lo..hi]
    }

    /// Every reference to an id that is not a node: (from, key path,
    /// missing), by the referring node's byte order and then key path.
    pub(crate) fn dangling_refs(&self) -> Vec<(ObjId, KeyPath, ObjId)> {
        let mut found: Vec<(u32, &RefEdge)> = self
            .edges
            .iter()
            .filter(|e| self.node(e.to).is_none())
            .map(|e| (self.node(e.from).map_or(u32::MAX, |n| n.at), e))
            .collect();
        found.sort_by(|(a, ea), (b, eb)| (a, &ea.path, ea.to).cmp(&(b, &eb.path, eb.to)));
        found
            .into_iter()
            .map(|(_, e)| (e.from, KeyPath::clone(&e.path), e.to))
            .collect()
    }

    /// The pages, in document order (module docs).
    pub(crate) fn pages_in_doc_order(&self) -> Vec<ObjId> {
        let mut pages = Vec::new();
        let mut seen = BTreeSet::new();
        let tree = self.catalog_candidates().first().and_then(|&c| {
            let catalog = self.node(c)?;
            self.out_of(catalog).find(|e| is_key(&e.path, b"Pages"))
        });
        if let Some(edge) = tree {
            let mut stack = vec![(edge.to, 0usize)];
            while let Some((id, depth)) = stack.pop() {
                let Some(n) = self.node(id) else { continue };
                if !n.shape.dict || !seen.insert(id) {
                    continue;
                }
                if n.kind == ObjectKind::Page || (!n.shape.has_kids && !n.shape.has_type) {
                    pages.push(id);
                } else if n.shape.has_kids && depth < MAX_TREE_DEPTH {
                    let kids: Vec<ObjId> = self.kids(n);
                    stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
                }
            }
        }
        let mut by_bytes: Vec<&Node> = self.nodes.iter().filter(|n| n.shape.dict).collect();
        by_bytes.sort_by_key(|n| n.at);
        let typed = by_bytes.iter().filter(|n| n.kind == ObjectKind::Page);
        let untyped = by_bytes
            .iter()
            .filter(|n| !n.shape.has_type && !n.shape.has_kids && n.shape.has_contents);
        for n in typed.chain(untyped) {
            if !seen.contains(&n.id) {
                seen.insert(n.id);
                pages.push(n.id);
            }
        }
        pages
    }

    /// The content streams `page` draws (module docs). `carve` must be the
    /// carve the graph was built from; `decoded` gives a stream's decoded
    /// bytes (analysis passes `SalvageIndex::decoded` over a `CarveSource`),
    /// `None` when it cannot, and such a stream is listed but not searched for
    /// `Do`. Each stream is searched on its own, so a `Do` whose operand ends
    /// the previous stream of a `/Contents` array is not followed.
    pub(crate) fn page_content<'b>(
        &self,
        carve: &CarveReport,
        page: ObjId,
        decoded: impl Fn(ObjId) -> Option<Cow<'b, [u8]>>,
    ) -> Vec<ContentPiece> {
        let Some(dict) = self.object(carve, page).and_then(body_dict) else {
            return Vec::new();
        };
        let resources = self.inherited_resources(carve, page);
        let mut visited = BTreeSet::new();
        let mut pieces: Vec<ContentPiece> = Vec::new();
        for stream in self.contents_of(carve, dict) {
            visited.insert(stream);
            pieces.push(ContentPiece {
                stream,
                resources: resources.clone(),
                via: Vec::new(),
            });
        }
        let mut i = 0;
        while i < pieces.len() {
            if pieces[i].via.len() < MAX_FORM_DEPTH
                && let Some(bytes) = decoded(pieces[i].stream)
            {
                let piece = &pieces[i];
                let mut found = Vec::new();
                for op in content_ops(&bytes).filter(|op| op.op == b"Do") {
                    let Some(Object::Name(name)) = op.operands.last() else {
                        continue;
                    };
                    let Some(form) = self.form_named(carve, &piece.resources.dict, name) else {
                        continue;
                    };
                    if !visited.insert(form) {
                        continue;
                    }
                    let resources = match self.object(carve, form).map(|o| &o.body) {
                        Some(Body::Stream { dict, .. }) => match dict.get(b"Resources") {
                            Ok(v) => Resources {
                                dict: self.resolve_dict(carve, v).cloned().unwrap_or_default(),
                                owner: Some(form),
                            },
                            // A form with no `/Resources` uses its caller's
                            // (ISO 32000-1 8.10.1, PDF 1.1 practice).
                            Err(_) => piece.resources.clone(),
                        },
                        _ => piece.resources.clone(),
                    };
                    let mut via = piece.via.clone();
                    via.push(piece.stream);
                    found.push(ContentPiece {
                        stream: form,
                        resources,
                        via,
                    });
                }
                pieces.extend(found);
            }
            i += 1;
        }
        pieces
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn node(&self, id: ObjId) -> Option<&Node> {
        self.nodes
            .binary_search_by_key(&id, |n| n.id)
            .ok()
            .map(|i| &self.nodes[i])
    }

    /// `n`'s out-edges in the order its value holds them.
    fn out_of<'g>(&'g self, n: &Node) -> impl Iterator<Item = &'g RefEdge> {
        let r = n.out.start as usize..n.out.end as usize;
        self.out_edges[r]
            .iter()
            .map(move |&i| &self.edges[i as usize])
    }

    /// The references in `n`'s `/Kids`, in array order; a `/Kids` that is a
    /// reference to an array object is followed once.
    fn kids(&self, n: &Node) -> Vec<ObjId> {
        let mut kids = Vec::new();
        for e in self.out_of(n) {
            match e.path.0.as_slice() {
                [PathSeg::Key(k), PathSeg::Index(_)] if k == b"Kids" => kids.push(e.to),
                [PathSeg::Key(k)] if k == b"Kids" => {
                    if let Some(array) = self.node(e.to) {
                        kids.extend(
                            self.out_of(array)
                                .filter(|e| matches!(e.path.0.as_slice(), [PathSeg::Index(_)]))
                                .map(|e| e.to),
                        );
                    }
                }
                _ => {}
            }
        }
        kids
    }

    /// The copy of `id` the graph holds.
    fn object<'c>(&self, carve: &'c CarveReport, id: ObjId) -> Option<&'c CarvedObject> {
        carve.objects.get(self.node(id)?.at as usize)
    }

    /// `v` as a dictionary: itself, or the dictionary object it references.
    fn resolve_dict<'c>(&self, carve: &'c CarveReport, v: &'c Object) -> Option<&'c Dictionary> {
        match v {
            Object::Dictionary(d) => Some(d),
            Object::Reference(id) => self.object(carve, *id).and_then(body_dict),
            _ => None,
        }
    }

    /// `v` as an array: itself, or the array object it references.
    fn resolve_array<'c>(&self, carve: &'c CarveReport, v: &'c Object) -> Option<&'c [Object]> {
        match v {
            Object::Array(a) => Some(a),
            Object::Reference(id) => match &self.object(carve, *id)?.body {
                Body::Primitive(Object::Array(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    /// The streams a page's `/Contents` names, in order: one reference, an
    /// array of them, or a reference to such an array. Entries that are not
    /// stream nodes are left out.
    fn contents_of(&self, carve: &CarveReport, page: &Dictionary) -> Vec<ObjId> {
        let Ok(v) = page.get(b"Contents") else {
            return Vec::new();
        };
        let is_stream = |id: ObjId| self.node(id).is_some_and(|n| n.shape.stream);
        match v {
            Object::Reference(id) if is_stream(*id) => vec![*id],
            v => self
                .resolve_array(carve, v)
                .unwrap_or_default()
                .iter()
                .filter_map(|o| o.as_reference().ok())
                .filter(|&id| is_stream(id))
                .collect(),
        }
    }

    /// The `/Resources` in force for `page`: its own entry, else the nearest
    /// `/Parent` ancestor's. An entry that is present but does not resolve to
    /// a dictionary is in force as an empty one.
    fn inherited_resources(&self, carve: &CarveReport, page: ObjId) -> Resources {
        let mut seen = BTreeSet::new();
        let mut cur = page;
        for _ in 0..=MAX_PARENT_DEPTH {
            if !seen.insert(cur) {
                break;
            }
            let Some(d) = self.object(carve, cur).and_then(body_dict) else {
                break;
            };
            if let Ok(v) = d.get(b"Resources") {
                return Resources {
                    dict: self.resolve_dict(carve, v).cloned().unwrap_or_default(),
                    owner: Some(cur),
                };
            }
            match d.get(b"Parent") {
                Ok(Object::Reference(p)) => cur = *p,
                _ => break,
            }
        }
        Resources::default()
    }

    /// The Form XObject `/XObject /<name>` names in `resources`, if it is one.
    fn form_named(
        &self,
        carve: &CarveReport,
        resources: &Dictionary,
        name: &[u8],
    ) -> Option<ObjId> {
        let xobjects = self.resolve_dict(carve, resources.get(b"XObject").ok()?)?;
        let id = xobjects.get(name).ok()?.as_reference().ok()?;
        let n = self.node(id)?;
        let Body::Stream { dict, .. } = &self.object(carve, id)?.body else {
            return None;
        };
        let subtype = dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        (subtype == Some(b"Form") || n.kind == ObjectKind::Form).then_some(id)
    }
}

/// D-032's winner for every declared id: the index in
/// [`CarveReport::objects`] of its last well-formed copy, else of its last
/// copy (module docs). The graph's nodes are these copies, and T-10's
/// rebuild writes them.
pub(crate) fn winning_copies(carve: &CarveReport) -> BTreeMap<ObjId, usize> {
    let mut chosen: BTreeMap<ObjId, usize> = BTreeMap::new();
    for (at, o) in carve.objects.iter().enumerate() {
        let keep = match chosen.get(&o.declared_id) {
            Some(&prev) => well_formed(o) || !well_formed(&carve.objects[prev]),
            None => true,
        };
        if keep {
            chosen.insert(o.declared_id, at);
        }
    }
    chosen
}

/// D-032's well-formed gate: parsed, and not a stream cut by EOF.
fn well_formed(o: &CarvedObject) -> bool {
    !matches!(
        o.body,
        Body::Unparsed
            | Body::Stream {
                length_source: LengthSource::TruncatedAtEof,
                ..
            }
    )
}

fn body_dict(o: &CarvedObject) -> Option<&Dictionary> {
    match &o.body {
        Body::Dict(d) => Some(d),
        _ => None,
    }
}

fn dict_shape(d: &Dictionary) -> Shape {
    Shape {
        dict: true,
        stream: false,
        has_type: d.has(b"Type"),
        has_kids: d.has(b"Kids"),
        has_count: d.has(b"Count"),
        has_contents: matches!(
            d.get(b"Contents"),
            Ok(Object::Reference(_) | Object::Array(_))
        ),
    }
}

/// `path` is exactly `/<key>`.
fn is_key(path: &KeyPath, key: &[u8]) -> bool {
    path.0.len() == 1 && path.first_key() == Some(key)
}

fn walk_dict(d: &Dictionary, stack: &mut Vec<PathSeg>, emit: &mut impl FnMut(&[PathSeg], ObjId)) {
    for (k, v) in d.iter() {
        stack.push(PathSeg::Key(k.clone()));
        walk(v, stack, emit);
        stack.pop();
    }
}

/// Emits every reference in `v`. The lexer caps nesting at `MAX_DEPTH`, so
/// the recursion is bounded.
fn walk(v: &Object, stack: &mut Vec<PathSeg>, emit: &mut impl FnMut(&[PathSeg], ObjId)) {
    match v {
        Object::Reference(id) => emit(stack, *id),
        Object::Array(a) => {
            for (i, item) in a.iter().enumerate() {
                stack.push(PathSeg::Index(i as u32));
                walk(item, stack, emit);
                stack.pop();
            }
        }
        Object::Dictionary(d) => walk_dict(d, stack, emit),
        Object::Stream(s) => walk_dict(&s.dict, stack, emit),
        _ => {}
    }
}

fn vec_bytes<T>(v: &Vec<T>) -> u64 {
    (v.capacity() * size_of::<T>()) as u64
}
