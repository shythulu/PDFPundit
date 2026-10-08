//! T-09 acceptance: key paths on edges, dangling refs from the C5 and C10
//! corruptors, the page-tree walk (cycles, shared pages, depth, byte-order
//! candidates), catalog choice over stale shadows, unreachable fonts after
//! C6, kinds in byte order, and the page-level content walk.

use std::collections::BTreeSet;

use super::*;
use crate::engine::Cancelled;
use crate::pdf::carver::carve;
use crate::pdf::fixtures;
use crate::pdf::model::CorruptionClass;
use crate::pdf::streams::DEFAULT_CAP;
use crate::pdf::streams::salvage::{CarveSource, SalvageIndex};

fn carved(buf: &[u8]) -> CarveReport {
    match carve(buf, &|| false) {
        Ok(r) => r,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    }
}

fn graph(buf: &[u8]) -> ObjectGraph {
    ObjectGraph::from_carve(&carved(buf))
}

fn id(n: u32) -> ObjId {
    (n, 0)
}

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.as_bytes().to_vec())
}

fn path(segs: Vec<PathSeg>) -> KeyPath {
    KeyPath(segs)
}

/// `%PDF-1.7`, then each `(num, gen, body)` as `num gen obj` … `endobj` in
/// the order given: no xref table, which the carver does not need.
fn raw_pdf_gen(objects: &[(u32, u16, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    for (num, gen_nr, body) in objects {
        out.extend_from_slice(format!("{num} {gen_nr} obj\n{body}\nendobj\n").as_bytes());
    }
    out.extend_from_slice(b"%%EOF\n");
    out
}

fn raw_pdf(objects: &[(u32, &str)]) -> Vec<u8> {
    let objects: Vec<(u32, u16, String)> = objects
        .iter()
        .map(|(n, b)| (*n, 0, (*b).to_owned()))
        .collect();
    raw_pdf_gen(&objects)
}

/// An unfiltered stream object's body with an exact `/Length`.
fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// What `page` draws, decoded through the salvage index as analysis does.
fn pieces(buf: &[u8], page: u32) -> Vec<ContentPiece> {
    let carve = carved(buf);
    let g = ObjectGraph::from_carve(&carve);
    let source = CarveSource::new(&carve, buf);
    let index = SalvageIndex::default();
    g.page_content(&carve, id(page), |s| {
        index.decoded(&source, s, DEFAULT_CAP).ok()
    })
}

fn summary(p: &[ContentPiece]) -> Vec<(u32, Option<ObjId>, Vec<u32>)> {
    p.iter()
        .map(|p| {
            (
                p.stream.0,
                p.resources.owner,
                p.via.iter().map(|v| v.0).collect(),
            )
        })
        .collect()
}

// ── edges ────────────────────────────────────────────────────────────────

#[test]
fn edges_carry_key_paths() {
    let g = graph(&fixtures::golden_pdf());
    let font = path(vec![key("Resources"), key("Font"), key("F1")]);
    let to_font = g.referrers(id(5));
    let got: Vec<(ObjId, &KeyPath)> = to_font.iter().map(|e| (e.from, &*e.path)).collect();
    assert_eq!(got, [(id(3), &font), (id(4), &font)]);
    assert!(to_font.iter().all(|e| e.to == id(5)));
    assert!(
        Arc::ptr_eq(&to_font[0].path, &to_font[1].path),
        "one allocation per distinct key path"
    );

    let kids0 = path(vec![key("Kids"), PathSeg::Index(0)]);
    let kids1 = path(vec![key("Kids"), PathSeg::Index(1)]);
    assert_eq!(*g.referrers(id(3))[0].path, kids0);
    assert_eq!(*g.referrers(id(4))[0].path, kids1);
    let to_pages: Vec<(ObjId, KeyPath)> = g
        .referrers(id(2))
        .iter()
        .map(|e| (e.from, KeyPath::clone(&e.path)))
        .collect();
    assert_eq!(
        to_pages,
        [
            (id(1), path(vec![key("Pages")])),
            (id(3), path(vec![key("Parent")])),
            (id(4), path(vec![key("Parent")])),
        ]
    );
    let descendant = path(vec![key("DescendantFonts"), PathSeg::Index(0)]);
    assert_eq!(*g.referrers(id(6))[0].path, descendant);
    let image = path(vec![key("Resources"), key("XObject"), key("Im1")]);
    assert_eq!(*g.referrers(id(10))[0].path, image);
    assert!(
        g.referrers(id(1)).is_empty(),
        "only the trailer names the catalog"
    );
    assert!(g.dangling_refs().is_empty());

    // A whole value that is a reference has the empty path.
    let g = graph(&raw_pdf(&[(1, "2 0 R"), (2, "42")]));
    assert_eq!(*g.referrers(id(2))[0].path, KeyPath::default());
}

/// Every golden edge as (from, path, to).
fn edge_set(g: &ObjectGraph) -> BTreeSet<(ObjId, KeyPath, ObjId)> {
    g.edges
        .iter()
        .map(|e| (e.from, KeyPath::clone(&e.path), e.to))
        .collect()
}

fn node_ids(g: &ObjectGraph) -> BTreeSet<ObjId> {
    g.nodes.iter().map(|n| n.id).collect()
}

#[test]
fn dangling_refs_of_c5_name_the_stripped_object() {
    let golden = fixtures::golden_pdf();
    let whole = graph(&golden);
    let mut seen = 0;
    for seed in 0..16 {
        let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, seed);
        let g = graph(&damaged);
        let missing: BTreeSet<ObjId> = node_ids(&whole)
            .difference(&node_ids(&g))
            .copied()
            .collect();
        assert_eq!(missing.len(), 1, "seed {seed}: one header stripped");
        let expected: BTreeSet<(ObjId, KeyPath, ObjId)> = edge_set(&whole)
            .into_iter()
            .filter(|(from, _, to)| missing.contains(to) && !missing.contains(from))
            .collect();
        let got = g.dangling_refs();
        assert_eq!(
            got.iter().cloned().collect::<BTreeSet<_>>(),
            expected,
            "seed {seed}"
        );
        assert_eq!(got.len(), expected.len(), "seed {seed}: no duplicates");
        seen += got.len();
    }
    assert!(seen > 0, "some stripped object is referenced");
}

#[test]
fn dangling_refs_of_c10_name_objects_past_the_cut() {
    let golden = fixtures::golden_pdf();
    let whole_carve = carved(&golden);
    let whole = ObjectGraph::from_carve(&whole_carve);
    let damaged = fixtures::corrupt(CorruptionClass::C10Truncated, &golden, 0);
    let cut = damaged.len() as u64;
    let g = graph(&damaged);
    let missing: BTreeSet<ObjId> = node_ids(&whole)
        .difference(&node_ids(&g))
        .copied()
        .collect();
    assert!(!missing.is_empty(), "the cut drops objects");
    let intact: BTreeSet<ObjId> = whole_carve
        .objects
        .iter()
        .filter(|o| o.span.end <= cut)
        .map(|o| o.declared_id)
        .collect();
    let into_missing: BTreeSet<(ObjId, KeyPath, ObjId)> = edge_set(&whole)
        .into_iter()
        .filter(|(_, _, to)| missing.contains(to))
        .collect();
    let got: BTreeSet<_> = g.dangling_refs().into_iter().collect();
    assert!(got.is_subset(&into_missing), "{got:?}");
    for edge in into_missing
        .iter()
        .filter(|(from, _, _)| intact.contains(from))
    {
        assert!(got.contains(edge), "{edge:?} missing from {got:?}");
    }
    assert!(!got.is_empty());
}

#[test]
fn dangling_refs_are_in_byte_order_of_the_referrer() {
    let g = graph(&raw_pdf(&[
        (9, "<< /A 50 0 R /B [51 0 R 52 0 R] >>"),
        (2, "<< /X 53 0 R >>"),
    ]));
    let got: Vec<(u32, KeyPath, u32)> = g
        .dangling_refs()
        .into_iter()
        .map(|(f, p, t)| (f.0, p, t.0))
        .collect();
    assert_eq!(
        got,
        [
            (9, path(vec![key("A")]), 50),
            (9, path(vec![key("B"), PathSeg::Index(0)]), 51),
            (9, path(vec![key("B"), PathSeg::Index(1)]), 52),
            (2, path(vec![key("X")]), 53),
        ]
    );
}

// ── page tree ────────────────────────────────────────────────────────────

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";

fn page(parent: u32) -> String {
    format!("<< /Type /Page /Parent {parent} 0 R >>")
}

#[test]
fn golden_pages_and_catalog() {
    for (label, pdf) in [
        ("classic", fixtures::golden_pdf()),
        ("objstm", fixtures::golden_pdf_objstm()),
    ] {
        let g = graph(&pdf);
        assert_eq!(g.catalog_candidates(), [id(1)], "{label}");
        assert_eq!(g.pages_in_doc_order(), [id(3), id(4)], "{label}");
        // The object and xref streams of the ObjStm golden are nodes that
        // nothing references.
        let all = g.nodes.len() as u64;
        assert_eq!(all, if label == "classic" { 12 } else { 14 });
        let reach = g.reachable_fraction(id(1));
        assert_eq!(reach, Ratio { num: 12, den: all }, "{label}");
    }
}

#[test]
fn cyclic_kids_terminates() {
    let pdf = raw_pdf(&[
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [2 0 R 3 0 R 4 0 R] /Count 2 >>"),
        (3, &page(2)),
        (
            4,
            "<< /Type /Pages /Parent 2 0 R /Kids [2 0 R 5 0 R 4 0 R] >>",
        ),
        (5, &page(4)),
    ]);
    let g = graph(&pdf);
    assert_eq!(g.pages_in_doc_order(), [id(3), id(5)]);
    assert_eq!(g.reachable(id(1)).len(), 5);
}

#[test]
fn a_page_in_two_kids_arrays_appears_once() {
    let pdf = raw_pdf(&[
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [6 0 R 7 0 R] /Count 3 >>"),
        (3, &page(6)),
        (4, &page(6)),
        (5, &page(7)),
        (6, "<< /Type /Pages /Parent 2 0 R /Kids [3 0 R 4 0 R] >>"),
        (7, "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] >>"),
    ]);
    assert_eq!(graph(&pdf).pages_in_doc_order(), [id(3), id(4), id(5)]);
}

#[test]
fn pages_come_in_reading_order_through_nested_trees() {
    // Breadth first would give 5 before 3 and 4.
    let pdf = raw_pdf(&[
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [6 0 R 5 0 R] /Count 3 >>"),
        (5, &page(2)),
        (4, &page(6)),
        (3, &page(6)),
        (6, "<< /Type /Pages /Parent 2 0 R /Kids [3 0 R 4 0 R] >>"),
    ]);
    assert_eq!(graph(&pdf).pages_in_doc_order(), [id(3), id(4), id(5)]);
}

#[test]
fn the_tree_walk_stops_at_depth_100() {
    // Pages nodes 2..=151 nest one in the next; 151 holds an untyped leaf
    // with no /Contents, which only the walk can find. Node 51, at depth 49,
    // also holds one.
    let mut objects: Vec<(u32, String)> = vec![(1, CATALOG.to_owned())];
    for n in 2..=151u32 {
        let mut kids = format!("{} 0 R", n + 1);
        if n == 51 {
            kids.push_str(" 500 0 R");
        }
        if n == 151 {
            kids = "600 0 R".to_owned();
        }
        objects.push((n, format!("<< /Type /Pages /Kids [{kids}] >>")));
    }
    objects.push((500, "<< /Parent 51 0 R >>".to_owned()));
    objects.push((600, "<< /Parent 151 0 R >>".to_owned()));
    let objects: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    let g = graph(&raw_pdf(&objects));
    assert_eq!(g.pages_in_doc_order(), [id(500)]);
}

#[test]
fn byte_order_candidates_follow_the_tree() {
    let pdf = raw_pdf(&[
        // Untyped, no /Kids, content by reference: a candidate, scored lower.
        (8, "<< /Contents 20 0 R /MediaBox [0 0 10 10] >>"),
        // An annotation's /Contents is a string: not a page.
        (9, "<< /Subtype /Text /Contents (note) >>"),
        // Untyped with /Kids: not a page.
        (10, "<< /Kids [] /Contents [20 0 R] >>"),
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page(2)),
        // Typed, outside the tree, no /MediaBox: a candidate.
        (11, "<< /Type /Page >>"),
        (12, "<< /Contents [20 0 R] >>"),
        (20, &stream("", "0 0 m 1 1 l S")),
    ]);
    let g = graph(&pdf);
    assert_eq!(g.pages_in_doc_order(), [id(3), id(11), id(8), id(12)]);

    // With no catalog, the candidates alone.
    let pdf = raw_pdf(&[(5, "<< /Contents 6 0 R >>"), (4, "<< /Type /Page >>")]);
    assert_eq!(graph(&pdf).pages_in_doc_order(), [id(4), id(5)]);
}

// ── catalog ──────────────────────────────────────────────────────────────

#[test]
fn catalog_chosen_over_a_stale_shadow() {
    // An incremental update rewrites catalog 1 to point at a new tree; the
    // earlier copy is the stale shadow.
    let pdf = raw_pdf(&[
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page(2)),
        (4, "<< /Type /Pages /Kids [5 0 R] /Count 1 >>"),
        (5, &page(4)),
        (1, "<< /Type /Catalog /Pages 4 0 R >>"),
    ]);
    let g = graph(&pdf);
    assert_eq!(g.catalog_candidates(), [id(1)]);
    assert_eq!(g.pages_in_doc_order()[0], id(5));
    let reach = g.reachable(id(1));
    assert!(reach.contains(&id(5)) && !reach.contains(&id(3)));
    assert!(g.referrers(id(2)).iter().all(|e| e.from != id(1)));

    // A later copy that does not parse does not replace a good one.
    let mut cut = pdf.clone();
    cut.extend_from_slice(b"1 0 obj\nendobj\n");
    assert_eq!(graph(&cut).pages_in_doc_order()[0], id(5));

    // A stale catalog under another number loses to the higher number;
    // catalogs whose /Pages does not resolve to a page-tree node are none.
    let pdf = raw_pdf(&[
        (9, "<< /Type /Catalog /Pages 4 0 R >>"),
        (1, CATALOG),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page(2)),
        (4, "<< /Type /Pages /Count 0 >>"),
        (10, "<< /Type /Catalog /Pages 99 0 R >>"),
        (12, "<< /Type /Catalog /Pages 3 0 R >>"),
        (13, "<< /Type /Catalog >>"),
    ]);
    assert_eq!(graph(&pdf).catalog_candidates(), [id(9), id(1)]);

    // One number, two generations: the later in byte order first.
    let objects = vec![
        (7, 1, CATALOG.to_owned()),
        (2, 0, "<< /Type /Pages /Kids [] /Count 0 >>".to_owned()),
        (7, 0, CATALOG.to_owned()),
    ];
    assert_eq!(
        graph(&raw_pdf_gen(&objects)).catalog_candidates(),
        [(7, 0), (7, 1)]
    );
}

// ── reachability and kinds ───────────────────────────────────────────────

#[test]
fn unreachable_lists_the_c6_fixtures_unlinked_font() {
    let clean = graph(&fixtures::type3_only_page());
    let root = clean.catalog_candidates()[0];
    assert!(clean.unreachable(root).is_empty());
    assert_eq!(clean.objects_of_kind(ObjectKind::Font), [id(4)]);

    let damaged = fixtures::corrupt(
        CorruptionClass::C6FontMapLost,
        &fixtures::type3_only_page(),
        0,
    );
    let g = graph(&damaged);
    let root = g.catalog_candidates()[0];
    let lost = g.unreachable(root);
    let fonts: Vec<ObjId> = g
        .objects_of_kind(ObjectKind::Font)
        .into_iter()
        .filter(|f| lost.contains(f))
        .collect();
    assert_eq!(fonts, [id(4)]);
    let r = g.reachable_fraction(root);
    assert!(r < Ratio { num: 1, den: 1 }, "{r:?}");
    assert_eq!(r.den, g.nodes.len() as u64);

    // The golden's font is shared by both pages: one C6 leaves it linked.
    let damaged = fixtures::corrupt(CorruptionClass::C6FontMapLost, &fixtures::golden_pdf(), 0);
    let g = graph(&damaged);
    assert!(!g.unreachable(id(1)).contains(&id(5)));
}

#[test]
fn objects_of_kind_font_is_in_byte_order() {
    let pdf = raw_pdf(&[
        (9, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        (3, "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>"),
        (4, "<< /Type /FontDescriptor >>"),
        (5, "<< /Type /Font /Subtype /TrueType >>"),
    ]);
    let g = graph(&pdf);
    assert_eq!(g.objects_of_kind(ObjectKind::Font), [id(9), id(3), id(5)]);
    assert_eq!(g.objects_of_kind(ObjectKind::FontDescriptor), [id(4)]);
    assert_eq!(
        graph(&fixtures::golden_pdf()).objects_of_kind(ObjectKind::Font),
        [id(5), id(6)]
    );
}

#[test]
fn reachable_follows_references_from_the_root_only() {
    let g = graph(&raw_pdf(&[
        (1, "<< /A 2 0 R /Gone 77 0 R >>"),
        (2, "[3 0 R 1 0 R]"),
        (3, "<< >>"),
        (4, "<< /Back 1 0 R >>"),
    ]));
    assert_eq!(g.reachable(id(1)), [id(1), id(2), id(3)].into());
    assert_eq!(g.unreachable(id(1)), [id(4)].into());
    assert_eq!(g.reachable_fraction(id(1)), Ratio { num: 3, den: 4 });
    assert!(g.reachable(id(77)).is_empty());
}

#[test]
fn heap_bytes_counts_the_edges() {
    assert_eq!(ObjectGraph::default().heap_bytes(), 0);
    let golden = fixtures::golden_pdf();
    let g = graph(&golden);
    let bytes = g.heap_bytes();
    let floor = (g.edges.len() * size_of::<RefEdge>() + g.nodes.len() * size_of::<Node>()) as u64;
    assert!(bytes >= floor, "{bytes} < {floor}");
    assert!(
        bytes < golden.len() as u64,
        "{bytes} bytes for {}",
        golden.len()
    );
}

// ── page content ─────────────────────────────────────────────────────────

#[test]
fn page_content_walks_a_contents_array_and_a_self_drawing_form() {
    let pdf = raw_pdf(&[
        (1, CATALOG),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 \
             /Resources << /Font << /F1 7 0 R >> /XObject << /Fm0 6 0 R >> >> >>",
        ),
        // No /Resources of its own: they are inherited from 2.
        (3, "<< /Type /Page /Parent 2 0 R /Contents [4 0 R 5 0 R] >>"),
        (4, &stream("", "BT /F1 12 Tf 72 720 Td (a) Tj ET")),
        (5, &stream("", "q /Fm0 Do Q")),
        (
            6,
            &stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
                 /Resources << /XObject << /Fm0 6 0 R >> >>",
                "/Fm0 Do 0 0 m 1 1 l S",
            ),
        ),
        (7, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]);
    let got = pieces(&pdf, 3);
    assert_eq!(
        summary(&got),
        [
            (4, Some(id(2)), vec![]),
            (5, Some(id(2)), vec![]),
            (6, Some(id(6)), vec![5]),
        ]
    );
    let inherited = &got[0].resources.dict;
    let font = inherited.get(b"Font").unwrap().as_dict().unwrap();
    assert_eq!(font.get(b"F1").unwrap(), &Object::Reference(id(7)));
    assert_eq!(got[1].resources, got[0].resources);
    assert!(
        got[2].resources.dict.get(b"Font").is_err(),
        "the form's own"
    );
}

#[test]
fn page_content_resolves_indirect_resources_and_caps_the_do_depth() {
    // Page 3 draws form 10, which draws 11, …, each the next: twenty deep.
    // Forms carry no /Resources, so each uses its caller's, indirect object 8.
    let mut xobjects = String::new();
    for n in 10..30u32 {
        xobjects.push_str(&format!("/X{n} {n} 0 R "));
    }
    let resources = format!("<< /XObject << {xobjects}>> >>");
    let mut objects: Vec<(u32, String)> = vec![
        (1, CATALOG.to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Resources 8 0 R /Contents 4 0 R >>".to_owned(),
        ),
        (4, stream("", "/X10 Do")),
        (8, resources),
    ];
    for n in 10..30u32 {
        let next = format!("/X{} Do", n + 1);
        objects.push((n, stream("/Subtype /Form /BBox [0 0 1 1]", &next)));
    }
    let objects: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    let got = pieces(&raw_pdf(&objects), 3);
    assert_eq!(got.len(), 1 + MAX_FORM_DEPTH);
    assert!(got.iter().all(|p| p.resources.owner == Some(id(3))));
    assert!(got[0].resources.dict.get(b"XObject").is_ok());
    let deepest = got.last().unwrap();
    assert_eq!(deepest.stream, id(10 + MAX_FORM_DEPTH as u32 - 1));
    assert_eq!(deepest.via.len(), MAX_FORM_DEPTH);
    assert_eq!(deepest.via[0], id(4));
}

#[test]
fn page_content_skips_what_is_not_a_form_or_not_there() {
    let pdf = raw_pdf(&[
        (
            3,
            "<< /Type /Page /Resources << /XObject << /Im 5 0 R /Gone 9 0 R >> >> \
             /Contents [4 0 R 99 0 R 6 0 R] >>",
        ),
        (4, &stream("", "/Im Do /Gone Do /Nope Do")),
        (5, &stream("/Subtype /Image /Width 1 /Height 1", "x")),
        (6, "<< /NotAStream true >>"),
    ]);
    assert_eq!(summary(&pieces(&pdf, 3)), [(4, Some(id(3)), vec![])]);
    // No such page, or a page that is not a dictionary: nothing.
    assert!(pieces(&pdf, 42).is_empty());
    assert!(pieces(&pdf, 4).is_empty());
}

#[test]
fn page_content_on_the_golden_lists_its_content_streams() {
    let golden = fixtures::golden_pdf();
    let p1 = pieces(&golden, 3);
    // Page 1 draws an image with Do: not a form, so no second piece.
    assert_eq!(summary(&p1), [(11, Some(id(3)), vec![])]);
    assert_eq!(summary(&pieces(&golden, 4)), [(12, Some(id(4)), vec![])]);
}

#[test]
fn the_node_is_the_copy_d032_keeps() {
    // A later copy cut by EOF does not replace a complete one.
    let pdf = fixtures::with_truncated_later_duplicate();
    let carve = carved(&pdf);
    let g = ObjectGraph::from_carve(&carve);
    let copies: Vec<&CarvedObject> = carve.copies(id(12)).collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(g.object(&carve, id(12)), Some(copies[0]));

    // A later complete copy does.
    let pdf = fixtures::with_duplicate_object();
    let carve = carved(&pdf);
    let g = ObjectGraph::from_carve(&carve);
    let copies: Vec<&CarvedObject> = carve.copies(id(12)).collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(g.object(&carve, id(12)), Some(copies[1]));
    assert_eq!(g.referrers(id(12)).len(), 1, "one node, one edge");
}
