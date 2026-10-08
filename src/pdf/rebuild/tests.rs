//! T-10 acceptance: D-032's duplicate rule on its two fixtures, keys by
//! `(num, gen)`, fresh numbers for orphans, the generation fallback before
//! positional matching, nearest-orphan matching with delta evidence, the
//! Warning input for an unmatched reference, the flat page tree of the C4
//! fixture and each rung of the MediaBox chain.

use std::collections::BTreeSet;

use super::*;
use crate::engine::Cancelled;
use crate::pdf::carver::carve;
use crate::pdf::fixtures;
use crate::pdf::graph::PathSeg;
use crate::pdf::model::{CorruptionClass, Location, MetricValue, Severity};

fn carved(buf: &[u8]) -> CarveReport {
    match carve(buf, &|| false) {
        Ok(r) => r,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    }
}

/// The carve, its graph and the remap.
fn planned(buf: &[u8]) -> (CarveReport, ObjectGraph, IdRemap) {
    let carve = carved(buf);
    let graph = ObjectGraph::from_carve(&carve);
    let remap = plan_ids(&carve, &graph);
    (carve, graph, remap)
}

fn tree(buf: &[u8], size: PageSize) -> (CarveReport, IdRemap, PageTreePlan) {
    let (carve, graph, remap) = planned(buf);
    let plan = rebuild_page_tree(&carve, &graph, &remap, size);
    (carve, remap, plan)
}

fn id(n: u32) -> ObjId {
    (n, 0)
}

fn int(v: i64) -> Object {
    Object::Integer(v)
}

fn rect(a: i64, b: i64, c: i64, d: i64) -> [Object; 4] {
    [int(a), int(b), int(c), int(d)]
}

/// One piece of a hand-made file: a numbered object or bytes with no header.
enum Part {
    Obj(u32, u16, String),
    Raw(String),
}

fn obj(n: u32, body: &str) -> Part {
    Part::Obj(n, 0, body.to_owned())
}

/// `%PDF-1.7`, then each part in order: no xref table, which the carver does
/// not need.
fn pdf(parts: &[Part]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    for p in parts {
        match p {
            Part::Obj(n, g, body) => {
                out.extend_from_slice(format!("{n} {g} obj\n{body}\nendobj\n").as_bytes());
            }
            Part::Raw(s) => out.extend_from_slice(s.as_bytes()),
        }
    }
    out.extend_from_slice(b"%%EOF\n");
    out
}

/// An unfiltered stream object's body with an exact `/Length`.
fn stream(data: &str) -> String {
    format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len())
}

/// A content stream with no `N G obj` in front of it: the gap sweep's orphan.
fn headerless(data: &str) -> Part {
    Part::Raw(format!("{}\nendobj\n", stream(data)))
}

const CONTENT: &str = "BT /F1 12 Tf 72 720 Td (Hello, orphan) Tj ET";

fn evidence_delta(r: &Reconciled) -> Option<i64> {
    r.evidence().iter().find_map(|e| match e {
        Evidence::Metric {
            name,
            value: MetricValue::Int(v),
        } if name == "matched_by_position_delta" => Some(*v),
        _ => None,
    })
}

// ── D-032: duplicates ────────────────────────────────────────────────────

#[test]
fn with_duplicate_object_the_later_well_formed_copy_wins() {
    let (carve, _, remap) = planned(&fixtures::with_duplicate_object());
    let copies: Vec<usize> = (0..carve.objects.len())
        .filter(|&i| carve.objects[i].declared_id == id(12))
        .collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(remap.target(id(12)), Some(Held::Object(copies[1])));
    assert_eq!(
        remap.number(id(12)),
        Some(12),
        "the winner keeps its number"
    );
    assert_eq!(
        remap.shadows(),
        [Shadow {
            id: id(12),
            at: copies[0],
            span: carve.objects[copies[0]].span,
        }],
        "the earlier copy is a shadow"
    );
    assert!(
        !remap
            .objects()
            .iter()
            .any(|&(_, h)| h == Held::Object(copies[0])),
        "an unclaimed shadow is not written"
    );
}

#[test]
fn with_truncated_later_duplicate_the_truncated_copy_loses() {
    let (carve, _, remap) = planned(&fixtures::with_truncated_later_duplicate());
    let copies: Vec<usize> = (0..carve.objects.len())
        .filter(|&i| carve.objects[i].declared_id == id(12))
        .collect();
    assert_eq!(copies.len(), 2);
    assert!(matches!(
        carve.objects[copies[1]].body,
        Body::Stream {
            length_source: LengthSource::TruncatedAtEof,
            ..
        }
    ));
    assert_eq!(remap.target(id(12)), Some(Held::Object(copies[0])));
    let shadows: Vec<(ObjId, ByteSpan)> = remap.shadows().iter().map(|s| (s.id, s.span)).collect();
    assert_eq!(shadows, [(id(12), carve.objects[copies[1]].span)]);
}

#[test]
fn a_later_unparsed_copy_does_not_replace_a_good_one_but_two_bad_copies_keep_the_last() {
    let buf = pdf(&[obj(1, "<< /A 1 >>"), obj(1, ")"), obj(2, "]"), obj(2, ")")]);
    let (carve, _, remap) = planned(&buf);
    assert!(matches!(carve.objects[1].body, Body::Unparsed));
    assert_eq!(remap.target(id(1)), Some(Held::Object(0)));
    assert_eq!(
        remap.target(id(2)),
        Some(Held::Object(3)),
        "else the last copy"
    );
    let shadows: Vec<usize> = remap.shadows().iter().map(|s| s.at).collect();
    assert_eq!(shadows, [1, 2]);
}

#[test]
fn copies_of_one_number_with_other_generations_are_separate_objects() {
    let buf = pdf(&[
        Part::Obj(5, 1, "<< /Gen 1 >>".into()),
        Part::Obj(5, 0, "<< /Gen 0 >>".into()),
        obj(6, "[5 0 R 5 1 R]"),
    ]);
    let (_, _, remap) = planned(&buf);
    assert!(remap.shadows().is_empty(), "(5 0) and (5 1) are two keys");
    assert_eq!(
        remap.number((5, 0)),
        Some(5),
        "the lowest generation keeps 5"
    );
    assert_eq!(
        remap.number((5, 1)),
        Some(7),
        "the other gets a fresh number"
    );
    let mut v = Object::Array(vec![Object::Reference((5, 0)), Object::Reference((5, 1))]);
    remap.rewrite(&mut v);
    assert_eq!(
        v,
        Object::Array(vec![Object::Reference((5, 0)), Object::Reference((7, 0))])
    );
}

// ── numbering ────────────────────────────────────────────────────────────

#[test]
fn the_golden_keeps_every_number_and_needs_no_reconciliation() {
    let (carve, graph, remap) = planned(&fixtures::golden_pdf());
    for o in &carve.objects {
        assert_eq!(remap.number(o.declared_id), Some(o.declared_id.0));
    }
    assert!(remap.shadows().is_empty());
    assert!(remap.reconciled().is_empty());
    assert!(remap.unmatched().is_empty());
    assert_eq!(remap.next_free(), 13);
    let written: Vec<u32> = remap.objects().iter().map(|&(n, _)| n).collect();
    assert_eq!(written, (1..=12).collect::<Vec<u32>>());
    assert!(graph.dangling_refs().is_empty());
}

#[test]
fn orphans_get_fresh_numbers_from_the_highest_declared_in_byte_order() {
    let buf = pdf(&[
        obj(3, "<< /A 1 >>"),
        headerless("first orphan, long enough"),
        obj(9, "<< /B 2 >>"),
        headerless("second orphan, long enough"),
        obj(4, "<< /C 3 >>"),
    ]);
    let (carve, _, remap) = planned(&buf);
    assert_eq!(carve.orphans.len(), 2);
    assert_eq!(remap.number_of(Held::Orphan(0)), Some(10));
    assert_eq!(remap.number_of(Held::Orphan(1)), Some(11));
    assert_eq!(remap.next_free(), 12);
    let written: Vec<(u32, Held)> = remap.objects();
    assert_eq!(
        written,
        [
            (3, Held::Object(0)),
            (4, Held::Object(2)),
            (9, Held::Object(1)),
            (10, Held::Orphan(0)),
            (11, Held::Orphan(1)),
        ]
    );
}

#[test]
fn object_streams_and_xref_streams_are_not_written() {
    let (carve, _, remap) = planned(&fixtures::golden_pdf_objstm());
    let containers: Vec<ObjId> = carve
        .objects
        .iter()
        .filter(|o| matches!(o.kind, ObjectKind::ObjStm | ObjectKind::XRefStream))
        .map(|o| o.declared_id)
        .collect();
    assert_eq!(containers, [(13, 0), (14, 0)]);
    for c in containers {
        assert_eq!(remap.number(c), None);
    }
    assert_eq!(remap.objects().len(), 12, "the twelve golden objects");
}

// ── reconciliation ───────────────────────────────────────────────────────

#[test]
fn dangling_contents_match_the_nearest_orphan_stream_with_delta_evidence() {
    let buf = pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        obj(2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
        headerless(CONTENT),
        obj(3, "<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>"),
        obj(4, "<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>"),
        headerless(&CONTENT.replace("Hello", "Bye")),
    ]);
    let (carve, _, remap) = planned(&buf);
    assert_eq!(carve.orphans.len(), 2);
    assert!(
        carve
            .orphans
            .iter()
            .all(|o| *o.kind() == ObjectKind::ContentStream)
    );
    let span_of = |n: u32| {
        carve
            .objects
            .iter()
            .find(|o| o.declared_id == id(n))
            .map(|o| o.span)
            .expect("object")
    };
    let rec = remap.reconciled();
    assert_eq!(rec.len(), 2);

    // Page 3's nearest orphan is the one just before it, not the one past page 4.
    assert_eq!(rec[0].missing, id(7));
    assert_eq!(rec[0].from, id(3));
    assert_eq!(rec[0].target, Held::Orphan(0));
    let delta0 = carve.orphans[0].span().start as i64 - span_of(3).start as i64;
    assert!(delta0 < 0);
    assert_eq!(rec[0].by, MatchedBy::Position { delta: delta0 });
    assert_eq!(evidence_delta(&rec[0]), Some(delta0));

    // Page 4's is the other, once the first is claimed.
    assert_eq!(rec[1].missing, id(8));
    assert_eq!(rec[1].target, Held::Orphan(1));
    let delta1 = carve.orphans[1].span().start as i64 - span_of(4).start as i64;
    assert_eq!(evidence_delta(&rec[1]), Some(delta1));

    // References follow the match.
    assert_eq!(remap.number(id(7)), remap.number_of(Held::Orphan(0)));
    let mut contents = Object::Reference(id(8));
    remap.rewrite(&mut contents);
    assert_eq!(contents, Object::Reference((6, 0)));
    assert!(remap.unmatched().is_empty());
}

#[test]
fn a_matched_reference_names_its_orphan_from_every_referrer() {
    let buf = pdf(&[
        obj(3, "<< /Type /Page /Contents 7 0 R >>"),
        headerless(CONTENT),
        obj(4, "<< /Type /Page /Contents [7 0 R] >>"),
    ]);
    let (_, _, remap) = planned(&buf);
    assert_eq!(remap.reconciled().len(), 1, "one match per missing id");
    assert_eq!(remap.number(id(7)), Some(5));
    assert!(remap.unmatched().is_empty());
}

#[test]
fn the_generation_fallback_comes_before_positional_matching() {
    let buf = pdf(&[
        obj(1, "<< /Type /Page /Contents 5 1 R >>"),
        headerless(CONTENT),
        obj(5, &stream(CONTENT)),
    ]);
    let (_, _, remap) = planned(&buf);
    let rec = remap.reconciled();
    assert_eq!(rec.len(), 1);
    assert_eq!(rec[0].missing, (5, 1));
    assert_eq!(rec[0].target, Held::Object(1));
    assert_eq!(rec[0].by, MatchedBy::Generation { generation: 0 });
    assert!(rec[0].evidence().contains(&Evidence::Metric {
        name: "matched_by_generation".into(),
        value: MetricValue::Int(0),
    }));
    assert_eq!(remap.number((5, 1)), Some(5));
    assert_eq!(
        remap.number_of(Held::Orphan(0)),
        Some(6),
        "the orphan stays unclaimed"
    );
}

#[test]
fn the_generation_fallback_prefers_the_nearest_earlier_generation() {
    let buf = pdf(&[
        obj(1, "[5 2 R]"),
        Part::Obj(5, 3, "(three)".into()),
        Part::Obj(5, 0, "(zero)".into()),
        Part::Obj(5, 1, "(one)".into()),
    ]);
    let (_, _, remap) = planned(&buf);
    assert_eq!(
        remap.reconciled()[0].by,
        MatchedBy::Generation { generation: 1 }
    );
    let buf = pdf(&[
        obj(1, "[5 2 R]"),
        Part::Obj(5, 4, "(four)".into()),
        Part::Obj(5, 3, "(three)".into()),
    ]);
    let (_, _, remap) = planned(&buf);
    assert_eq!(
        remap.reconciled()[0].by,
        MatchedBy::Generation { generation: 3 },
        "else the nearest later one"
    );
}

#[test]
fn the_key_path_tail_decides_the_kind_an_orphan_must_have() {
    // An orphan stream that is not a font file is not a /FontFile2's match.
    let buf = pdf(&[
        obj(1, "<< /Type /FontDescriptor /FontFile2 9 0 R >>"),
        headerless(CONTENT),
    ]);
    let (_, _, remap) = planned(&buf);
    assert!(remap.reconciled().is_empty());
    assert_eq!(remap.unmatched().len(), 1);

    let wants = |segs: &[&str]| {
        let path = KeyPath(
            segs.iter()
                .map(|s| match s.parse::<u32>() {
                    Ok(i) => PathSeg::Index(i),
                    Err(_) => PathSeg::Key(s.as_bytes().to_vec()),
                })
                .collect(),
        );
        [
            ObjectKind::ContentStream,
            ObjectKind::FontFile,
            ObjectKind::ToUnicode,
            ObjectKind::Other("/CMap".into()),
            ObjectKind::Page,
            ObjectKind::Font,
            ObjectKind::Image,
            ObjectKind::Form,
        ]
        .into_iter()
        .filter(|k| Want::of(&path).is_some_and(|w| w.admits(k)))
        .collect::<Vec<_>>()
    };
    assert_eq!(wants(&["Contents"]), [ObjectKind::ContentStream]);
    assert_eq!(wants(&["Contents", "1"]), [ObjectKind::ContentStream]);
    for ff in ["FontFile", "FontFile2", "FontFile3"] {
        assert_eq!(wants(&["FontDescriptor", ff]), [ObjectKind::FontFile]);
    }
    assert_eq!(
        wants(&["ToUnicode"]),
        [ObjectKind::ToUnicode, ObjectKind::Other("/CMap".into())]
    );
    assert_eq!(wants(&["Kids", "0"]), [ObjectKind::Page]);
    assert_eq!(wants(&["Resources", "Font", "F1"]), [ObjectKind::Font]);
    assert_eq!(
        wants(&["Resources", "XObject", "Im1"]),
        [ObjectKind::Image, ObjectKind::Form]
    );
    assert!(wants(&["Parent"]).is_empty());
    assert!(wants(&["Pages"]).is_empty());
    assert!(wants(&["Resources", "Font"]).is_empty());
}

#[test]
fn an_unmatched_dangling_reference_is_a_warning_input_and_becomes_null() {
    let buf = pdf(&[obj(3, "<< /Type /Page /Contents 9 0 R /Rotate 0 >>")]);
    let (carve, _, remap) = planned(&buf);
    assert!(remap.reconciled().is_empty());
    let un = remap.unmatched();
    assert_eq!(un.len(), 1);
    assert_eq!(un[0].from, id(3));
    assert_eq!(un[0].missing, id(9));
    let input = un[0].finding_input();
    assert_eq!(input.severity, Severity::Warning);
    assert_eq!(
        input.location,
        Location::Object {
            id: id(3),
            span: Some(carve.objects[0].span),
        }
    );
    assert!(input.evidence.contains(&Evidence::ObjectRef(id(9))));
    assert!(input.evidence.contains(&Evidence::Text("/Contents".into())));
    assert!(input.summary.contains("9 0 R"), "{}", input.summary);

    // The missing number may later be a fresh one, so the reference goes.
    let mut v = Object::Reference(id(9));
    remap.rewrite(&mut v);
    assert_eq!(v, Object::Null);
}

#[test]
fn a_shadow_is_claimed_only_when_no_orphan_of_the_kind_is_left() {
    let buf = pdf(&[
        obj(5, &stream("BT /F1 1 Tf (old) Tj ET")),
        obj(5, &stream("BT /F1 1 Tf (new) Tj ET")),
        obj(1, "<< /Type /Page /Contents 7 0 R >>"),
        obj(2, "<< /Type /Page /Contents 8 0 R >>"),
        headerless(CONTENT),
    ]);
    let (_, _, remap) = planned(&buf);
    let rec = remap.reconciled();
    assert_eq!(rec.len(), 2);
    assert_eq!(rec[0].target, Held::Orphan(0), "the orphan first");
    assert_eq!(rec[1].target, Held::Object(0), "then the shadow");
    assert_eq!(remap.shadows()[0].at, 0, "still listed as a shadow");
    assert_eq!(remap.number_of(Held::Orphan(0)), Some(6));
    assert_eq!(remap.number(id(8)), Some(7), "a claimed shadow is written");
    assert!(remap.objects().contains(&(7, Held::Object(0))));
}

#[test]
fn c5_references_to_the_stripped_object_find_its_orphan() {
    let golden = fixtures::golden_pdf();
    let whole = carved(&golden);
    let mut matched = 0;
    for seed in 0..32 {
        let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, seed);
        let (carve, graph, remap) = planned(&damaged);
        let missing: BTreeSet<ObjId> = graph.dangling_refs().iter().map(|d| d.2).collect();
        for r in remap.reconciled() {
            let Held::Orphan(i) = r.target else {
                panic!("seed {seed}: matched a shadow")
            };
            let o = &carve.orphans[i];
            let original = whole
                .objects
                .iter()
                .find(|w| w.declared_id == r.missing)
                .expect("the stripped object");
            assert_eq!(o.kind(), &original.kind, "seed {seed}");
            matched += 1;
        }
        let unmatched: BTreeSet<ObjId> = remap.unmatched().iter().map(|u| u.missing).collect();
        let reconciled: BTreeSet<ObjId> = remap.reconciled().iter().map(|r| r.missing).collect();
        assert_eq!(
            reconciled
                .union(&unmatched)
                .copied()
                .collect::<BTreeSet<_>>(),
            missing,
            "seed {seed}: every dangling id is matched or reported"
        );
    }
    assert!(
        matched > 0,
        "some seed strips an object the table can match"
    );
}

// ── the page tree ────────────────────────────────────────────────────────

fn assert_flat(plan: &PageTreePlan, ids: &[u32], what: &str) {
    let got: Vec<u32> = plan.pages.iter().map(|p| p.id).collect();
    assert_eq!(got, ids, "{what}");
    let pages = plan.pages_dict();
    assert_eq!(
        pages.get(b"Type").and_then(Object::as_name).ok(),
        Some(&b"Pages"[..]),
        "{what}"
    );
    assert_eq!(
        pages.get(b"Count").and_then(Object::as_i64).ok(),
        Some(ids.len() as i64),
        "{what}"
    );
    let kids: Vec<Object> = ids.iter().map(|&n| Object::Reference((n, 0))).collect();
    assert_eq!(
        pages.get(b"Kids").and_then(Object::as_array).ok(),
        Some(&kids),
        "{what}"
    );
}

#[test]
fn c4_rebuilds_a_flat_pages_node_with_the_right_count() {
    // The golden's page-tree node is compact, so REPDF's 45–128 byte cut
    // always reaches into page 3 as well: page 4 is whole, and what is left
    // of page 3 is a page only when its `/Contents` survives in a dictionary
    // (T-09's rule; seed 3 keeps all of it under object 2's header).
    let golden = fixtures::golden_pdf();
    let mut remnants = 0;
    for seed in 0..16 {
        let damaged = fixtures::corrupt(CorruptionClass::C4PageTreeBroken, &golden, seed);
        let (carve, graph, remap) = planned(&damaged);
        let plan = rebuild_page_tree(&carve, &graph, &remap, PageSize::A4);
        let what = format!("seed {seed}");
        let found: Vec<u32> = graph.pages_in_doc_order().iter().map(|p| p.0).collect();
        assert!(found.starts_with(&[4]), "{what}: {found:?}");
        assert!(found.len() <= 2, "{what}: {found:?}");
        remnants += found.len() - 1;
        assert_flat(&plan, &found, &what);
        assert_eq!(plan.pages_id, remap.next_free(), "{what}");
        let p4 = &plan.pages[0];
        assert_eq!(p4.mediabox, rect(0, 0, 612, 792), "{what}");
        assert_eq!(p4.source, BoxSource::Own, "{what}");
        assert!(
            matches!(p4.resources, Some(Object::Dictionary(_))),
            "{what}"
        );
        for p in &plan.pages[1..] {
            assert_eq!(p.mediabox, rect(0, 0, 612, 792), "{what}: own or modal");
        }
        let catalog_survives = carve
            .objects
            .iter()
            .any(|o| o.declared_id == id(1) && o.kind == ObjectKind::Catalog);
        if catalog_survives {
            assert_eq!(plan.catalog, CatalogPlan::Reuse(id(1)), "{what}");
            assert_eq!(plan.root, 1, "{what}");
        } else {
            assert_eq!(plan.catalog, CatalogPlan::Synthesize, "{what}");
            assert_eq!(plan.root, plan.pages_id + 1, "{what}");
        }
    }
    assert!(remnants > 0, "some seed leaves page 3's remains a page");

    // The page-tree node gone and both pages whole: both come back, in
    // order, under the reused catalog.
    let buf = pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        obj(
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R >>",
        ),
        obj(
            4,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R >>",
        ),
        obj(5, &stream(CONTENT)),
        obj(6, &stream(CONTENT)),
    ]);
    let (_, remap, plan) = tree(&buf, PageSize::A4);
    assert_flat(&plan, &[3, 4], "no /Pages node");
    assert_eq!(plan.catalog, CatalogPlan::Reuse(id(1)));
    assert_eq!(plan.pages_id, 7);
    assert_eq!(remap.unmatched().len(), 3, "/Pages and both /Parent");
}

#[test]
fn the_golden_reuses_its_catalog() {
    let (_, remap, plan) = tree(&fixtures::golden_pdf(), PageSize::A4);
    assert_eq!(plan.catalog, CatalogPlan::Reuse(id(1)));
    assert_eq!(plan.root, 1);
    assert_eq!(plan.pages_id, 13);
    assert_eq!(remap.next_free(), 13);
    assert_eq!(plan.pages.len(), 2);
}

#[test]
fn a_stripped_page_header_keeps_the_page_in_its_place() {
    let golden = fixtures::golden_pdf();
    let mut seen = 0;
    for seed in 0..64 {
        let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, seed);
        let (carve, remap, plan) = tree(&damaged, PageSize::A4);
        let stripped = [3, 4]
            .into_iter()
            .find(|&p| !carve.objects.iter().any(|o| o.declared_id == id(p)));
        let Some(page) = stripped else { continue };
        seen += 1;
        let ids: Vec<u32> = plan.pages.iter().map(|p| p.id).collect();
        let orphan = remap.number(id(page)).expect("the page was matched");
        let want = if page == 3 { [orphan, 4] } else { [3, orphan] };
        assert_eq!(ids, want, "seed {seed}");
        assert!(plan.pages.iter().all(|p| p.source == BoxSource::Own));
    }
    assert!(seen > 0, "some seed strips a page header");
}

/// Pages 2 (root, no box) → [10 (Pages, 100×100, rotate 90, crop, resources) → [7],
/// 3 (200), 4 (300), 5 (300), 6 (none)].
fn box_tree() -> Vec<u8> {
    pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        obj(
            2,
            "<< /Type /Pages /Kids [10 0 R 3 0 R 4 0 R 5 0 R 6 0 R] /Count 5 >>",
        ),
        obj(
            10,
            "<< /Type /Pages /Parent 2 0 R /Kids [7 0 R] /Count 1 /MediaBox [0 0 100 100] \
             /CropBox 11 0 R /Rotate 90 /Resources << /Font << /F1 12 0 R >> >> >>",
        ),
        obj(11, "[10 10 90 90]"),
        obj(12, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        obj(7, "<< /Type /Page /Parent 10 0 R >>"),
        obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        obj(
            4,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Rotate 180 >>",
        ),
        obj(5, "<< /Type /Page /Parent 2 0 R /MediaBox 13 0 R >>"),
        obj(13, "[0 0 300 300]"),
        obj(6, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 bad] >>"),
    ])
}

#[test]
fn the_mediabox_chain_hits_each_rung() {
    let (_, _, plan) = tree(&box_tree(), PageSize::A4);
    let got: Vec<(u32, [Object; 4], BoxSource)> = plan
        .pages
        .iter()
        .map(|p| (p.id, p.mediabox.clone(), p.source))
        .collect();
    assert_eq!(
        got,
        [
            (7, rect(0, 0, 100, 100), BoxSource::Inherited),
            (3, rect(0, 0, 200, 200), BoxSource::Own),
            (4, rect(0, 0, 300, 300), BoxSource::Own),
            (5, rect(0, 0, 300, 300), BoxSource::Own),
            (6, rect(0, 0, 300, 300), BoxSource::ModalSibling),
        ]
    );

    // No box anywhere: the configured size.
    for (size, want) in [
        (PageSize::A4, rect(0, 0, 595, 842)),
        (PageSize::Letter, rect(0, 0, 612, 792)),
        (
            PageSize::Custom {
                w_pt: 300,
                h_pt: 400,
            },
            rect(0, 0, 300, 400),
        ),
    ] {
        let buf = pdf(&[
            obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
            obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            obj(3, "<< /Type /Page /Parent 2 0 R >>"),
        ]);
        let (_, _, plan) = tree(&buf, size);
        assert_eq!(plan.pages[0].mediabox, want);
        assert_eq!(plan.pages[0].source, BoxSource::Default);
    }
}

#[test]
fn a_modal_tie_goes_to_the_first_box_in_document_order() {
    let buf = pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        obj(2, "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>"),
        obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 5 5] >>"),
        obj(4, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 4 4] >>"),
        obj(5, "<< /Type /Page /Parent 2 0 R >>"),
    ]);
    let (_, _, plan) = tree(&buf, PageSize::A4);
    assert_eq!(plan.pages[2].mediabox, rect(0, 0, 5, 5));
    assert_eq!(plan.pages[2].source, BoxSource::ModalSibling);
}

#[test]
fn inheritable_attributes_are_pinned_per_page() {
    let (_, _, plan) = tree(&box_tree(), PageSize::A4);
    let p7 = &plan.pages[0];
    assert_eq!(p7.rotate, Some(90));
    assert_eq!(p7.cropbox, Some(rect(10, 10, 90, 90)));
    let Some(Object::Dictionary(res)) = &p7.resources else {
        panic!("inherited resources: {:?}", p7.resources)
    };
    assert!(res.has(b"Font"));
    let p4 = &plan.pages[2];
    assert_eq!(p4.rotate, Some(180), "a page's own value wins");
    assert_eq!(p4.cropbox, None);
    assert_eq!(p4.resources, None, "no /Resources anywhere above page 4");
}

#[test]
fn no_catalog_means_one_is_synthesized() {
    let buf = pdf(&[
        obj(3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
        obj(4, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    let (_, remap, plan) = tree(&buf, PageSize::A4);
    assert_eq!(plan.catalog, CatalogPlan::Synthesize);
    assert_eq!(plan.pages_id, remap.next_free());
    assert_eq!(plan.root, plan.pages_id + 1);
    let ids: Vec<u32> = plan.pages.iter().map(|p| p.id).collect();
    assert_eq!(ids, [3, 4]);
}

#[test]
fn a_catalog_whose_pages_link_is_gone_is_still_reused() {
    let buf = pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R /Outlines 9 0 R >>"),
        obj(3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    let (_, _, plan) = tree(&buf, PageSize::A4);
    assert_eq!(plan.catalog, CatalogPlan::Reuse(id(1)));
    assert_eq!(plan.root, 1);
}

#[test]
fn an_unclaimed_orphan_page_joins_the_tree_after_the_others() {
    let buf = pdf(&[
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        Part::Raw("<< /Type /Page /MediaBox [0 0 7 7] /Note (headerless) >>\nendobj\n".into()),
        obj(3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] >>"),
    ]);
    let (carve, remap, plan) = tree(&buf, PageSize::A4);
    assert_eq!(carve.orphans.len(), 1);
    let ids: Vec<u32> = plan.pages.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        [3, remap.number_of(Held::Orphan(0)).expect("numbered")]
    );
    assert_eq!(plan.pages[1].mediabox, rect(0, 0, 7, 7));
}
