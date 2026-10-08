//! T-11a acceptance: each structural corruptor yields its class and no other
//! structural class on the goldens; a junk prefix past the first KiB is C1
//! with a valid body; an off-by-one `startxref` is a Warning; `/Encrypt`
//! refuses with "decrypt first"; the signed golden is one `Signed` finding;
//! clean files yield nothing. Plus the C4 adversarial table rows, finding
//! ids and `file_meta`.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::engine::Cancelled;
use crate::pdf::carver::carve;
use crate::pdf::fixtures;
use crate::pdf::meta::file_meta;
use crate::pdf::model::{
    CorruptionClass, Evidence, FileMeta, FindingKind, Location, MetricValue, Ratio, Repairability,
    Severity,
};

use CorruptionClass::{
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken, C5ObjectTagStripped, C10Truncated,
};

const STRUCTURAL: [CorruptionClass; 6] = [
    C1Header,
    C2XrefMissing,
    C3TrailerDamaged,
    C4PageTreeBroken,
    C5ObjectTagStripped,
    C10Truncated,
];

fn carved(buf: &[u8]) -> CarveReport {
    match carve(buf, &|| false) {
        Ok(r) => r,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    }
}

/// Diagnoses `buf` and checks the id rule on the way out.
fn findings(buf: &[u8]) -> Vec<Finding> {
    let carve = carved(buf);
    let graph = ObjectGraph::from_carve(&carve);
    let out = diagnose(buf, &carve, &graph, &SalvageIndex::default());
    check_ids(&out);
    out
}

fn meta(buf: &[u8]) -> FileMeta {
    let carve = carved(buf);
    let graph = ObjectGraph::from_carve(&carve);
    file_meta(&carve, &graph)
}

/// Every id is `<prefix>-<nnn>`, numbered from 001 per prefix in the order
/// the list holds them.
fn check_ids(found: &[Finding]) {
    let mut next: BTreeMap<String, u32> = BTreeMap::new();
    for f in found {
        let (prefix, n) = f.id.rsplit_once('-').expect("an id is <prefix>-<nnn>");
        assert_eq!(n.len(), 3, "{}", f.id);
        let expected = next.entry(prefix.to_owned()).or_insert(1);
        assert_eq!(
            n,
            format!("{:03}", *expected),
            "ids number in order: {}",
            f.id
        );
        *expected += 1;
        let want = match f.class {
            FindingKind::Corruption(c) => c.code(),
            FindingKind::Encrypted => "ENC",
            FindingKind::Signed { .. } => "SIG",
            FindingKind::OutlinedText { .. } => "OUTLINE",
            FindingKind::Type3Text { .. } => "TYPE3",
        };
        assert_eq!(prefix, want, "{}", f.id);
    }
}

/// The structural classes among `found`.
fn structural(found: &[Finding]) -> BTreeSet<CorruptionClass> {
    found
        .iter()
        .filter_map(|f| match f.class {
            FindingKind::Corruption(c) if STRUCTURAL.contains(&c) => Some(c),
            _ => None,
        })
        .collect()
}

fn of_class(found: &[Finding], class: CorruptionClass) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.class == FindingKind::Corruption(class))
        .collect()
}

fn metric<'f>(f: &'f Finding, name: &str) -> Option<&'f MetricValue> {
    f.evidence.iter().find_map(|e| match e {
        Evidence::Metric { name: n, value } if n == name => Some(value),
        _ => None,
    })
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn insert(buf: &[u8], at: usize, what: &[u8]) -> Vec<u8> {
    let mut out = buf[..at].to_vec();
    out.extend_from_slice(what);
    out.extend_from_slice(&buf[at..]);
    out
}

/// `buf` with its last `startxref` value moved by `delta`.
fn shift_startxref(buf: &[u8], delta: i64) -> Vec<u8> {
    let kw = rfind(buf, b"startxref").expect("startxref") + 9;
    let start = kw
        + buf[kw..]
            .iter()
            .position(u8::is_ascii_digit)
            .expect("digits");
    let len = buf[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let value: i64 = std::str::from_utf8(&buf[start..start + len])
        .expect("ascii")
        .parse()
        .expect("number");
    let mut out = buf[..start].to_vec();
    out.extend_from_slice((value + delta).to_string().as_bytes());
    out.extend_from_slice(&buf[start + len..]);
    out
}

// ── hand-made classic files ─────────────────────────────────────────────

/// One piece of a hand-made file: a numbered object or bytes with no header.
enum Part {
    Obj(u32, String),
    Raw(String),
}

fn obj(n: u32, body: &str) -> Part {
    Part::Obj(n, body.to_owned())
}

/// `%PDF-1.7`, the parts, a correct classic xref table, a trailer with
/// `/Root 1 0 R` and `extra`, `startxref` and `%%EOF`.
fn classic(parts: &[Part], extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    for p in parts {
        match p {
            Part::Obj(n, body) => {
                offsets.insert(*n, out.len());
                out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
            }
            Part::Raw(s) => out.extend_from_slice(s.as_bytes()),
        }
    }
    let size = offsets.keys().next_back().map_or(1, |m| m + 1);
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for n in 1..size {
        let line = match offsets.get(&n) {
            Some(at) => format!("{at:010} 00000 n \n"),
            None => "0000000000 65535 f \n".to_owned(),
        };
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R {extra}>>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";

fn page(parent: u32) -> String {
    format!("<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] >>")
}

fn pages(kids: &[u32], count: i64) -> String {
    let kids: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
    format!(
        "<< /Type /Pages /Kids [{}] /Count {count} >>",
        kids.join(" ")
    )
}

// ── clean files ─────────────────────────────────────────────────────────

#[test]
fn clean_files_yield_no_findings() {
    for (name, buf) in [
        ("golden", fixtures::golden_pdf()),
        ("objstm golden", fixtures::golden_pdf_objstm()),
        (
            "hand-made",
            classic(
                &[obj(1, CATALOG), obj(2, &pages(&[3], 1)), obj(3, &page(2))],
                "",
            ),
        ),
    ] {
        assert_eq!(findings(&buf), vec![], "{name}");
    }
}

#[test]
fn an_intact_xref_stream_file_is_not_c2() {
    let found = findings(&fixtures::golden_pdf_objstm());
    assert!(of_class(&found, C2XrefMissing).is_empty());
    assert!(of_class(&found, C3TrailerDamaged).is_empty());
}

// ── the structural corruptors ───────────────────────────────────────────

fn assert_only(class: CorruptionClass, buf: &[u8], what: &str) {
    let found = findings(buf);
    assert_eq!(
        structural(&found),
        BTreeSet::from([class]),
        "{what}: {found:#?}"
    );
    assert!(
        of_class(&found, class)
            .iter()
            .any(|f| f.severity == Severity::Error),
        "{what}: an Error {class:?}"
    );
    assert!(
        found
            .iter()
            .all(|f| !matches!(f.class, FindingKind::Encrypted | FindingKind::Signed { .. })),
        "{what}"
    );
}

#[test]
fn each_structural_corruptor_yields_its_class_and_no_other_on_the_golden() {
    let golden = fixtures::golden_pdf();
    for class in STRUCTURAL {
        for seed in 0..12 {
            let buf = fixtures::corrupt(class, &golden, seed);
            assert_only(class, &buf, &format!("{class:?} seed {seed}"));
        }
    }
}

#[test]
fn a_c4_cut_that_also_takes_the_next_header_is_c4_and_c5() {
    // One seed in 400 deletes all of the page-tree node and the `3 0 obj`
    // header after it: page 1 is then a dictionary with no header, which is
    // what C5 names.
    let golden = fixtures::golden_pdf();
    let buf = fixtures::corrupt(C4PageTreeBroken, &golden, 108);
    let cut = golden.iter().zip(&buf).take_while(|(a, b)| a == b).count();
    let gone = &golden[cut..cut + golden.len() - buf.len()];
    assert!(
        gone.ends_with(b"endobj\n3 0 obj\n"),
        "{}",
        String::from_utf8_lossy(gone)
    );
    let found = findings(&buf);
    assert_eq!(
        structural(&found),
        BTreeSet::from([C4PageTreeBroken, C5ObjectTagStripped])
    );
    let c5 = of_class(&found, C5ObjectTagStripped);
    assert_eq!(
        metric(c5[0], "kind"),
        Some(&MetricValue::Text("Page".into()))
    );
}

#[test]
fn each_applicable_corruptor_yields_its_class_and_no_other_on_the_objstm_golden() {
    // C2 and C3 edit a classic table, which this file does not have; C4's
    // page-tree node is packed in the object stream, so its bytes are not in
    // the file to delete. C10 is below.
    let golden = fixtures::golden_pdf_objstm();
    for class in [C1Header, C5ObjectTagStripped] {
        for seed in 0..12 {
            let buf = fixtures::corrupt(class, &golden, seed);
            assert_only(class, &buf, &format!("{class:?} seed {seed} (objstm)"));
        }
    }
}

#[test]
fn truncating_the_objstm_golden_loses_its_page_tree_too() {
    // The cut at 70% lands in the font stream, before the object stream that
    // holds every dictionary: the catalog and the page tree are gone, which
    // is a second, real finding. The missing xref stream and trailer are
    // the cut's and are not reported apart from it.
    let golden = fixtures::golden_pdf_objstm();
    let buf = fixtures::corrupt(C10Truncated, &golden, 0);
    let objstm = rfind(&golden, b"13 0 obj").expect("the object stream");
    assert!(buf.len() < objstm);
    let found = findings(&buf);
    assert_eq!(
        structural(&found),
        BTreeSet::from([C4PageTreeBroken, C10Truncated]),
        "{found:#?}"
    );
}

// ── C1 ──────────────────────────────────────────────────────────────────

#[test]
fn a_junk_prefix_past_the_first_kib_is_c1_with_a_valid_body() {
    let n = 2048;
    let found = findings(&fixtures::with_junk_prefix(n));
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C1Header));
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.repair, Repairability::Auto);
    assert_eq!(f.id, "C1-001");
    assert_eq!(
        metric(f, "header_offset"),
        Some(&MetricValue::Int(n as i64))
    );
    let window = f.evidence.iter().find_map(|e| match e {
        Evidence::HexWindow(w) => Some(w),
        _ => None,
    });
    let window = window.expect("a hex window at 0");
    assert_eq!(window.at(), 0);
    assert_eq!(window.bytes().len(), 64);
}

#[test]
fn a_junk_prefix_inside_the_first_kib_is_read_header_relative() {
    // Offsets count from `%PDF-`, so neither C2 nor C3 fires, and a header
    // inside the first KiB is where readers look for it.
    assert_eq!(findings(&fixtures::with_junk_prefix(100)), vec![]);
}

#[test]
fn a_header_with_a_bad_version_is_c1() {
    let mut buf = fixtures::golden_pdf();
    buf[5..8].copy_from_slice(b"x.7");
    let found = findings(&buf);
    assert_eq!(structural(&found), BTreeSet::from([C1Header]));
    assert_eq!(metric(&found[0], "header_offset"), None);
}

#[test]
fn an_overwritten_header_has_no_header_offset() {
    let buf = fixtures::corrupt(C1Header, &fixtures::golden_pdf(), 3);
    let found = findings(&buf);
    let c1 = of_class(&found, C1Header);
    assert_eq!(c1.len(), 1);
    assert_eq!(metric(c1[0], "header_offset"), None);
}

// ── C2 / C3 ─────────────────────────────────────────────────────────────

#[test]
fn c2_carries_the_object_count_and_the_startxref_target() {
    let golden = fixtures::golden_pdf();
    let buf = fixtures::corrupt(C2XrefMissing, &golden, 0);
    let found = findings(&buf);
    let c2 = of_class(&found, C2XrefMissing);
    assert_eq!(c2.len(), 1);
    assert_eq!(metric(c2[0], "objects_carved"), Some(&MetricValue::Int(12)));
    assert_eq!(
        metric(c2[0], "startxref_target"),
        Some(&MetricValue::Int(9937))
    );
}

#[test]
fn an_off_by_one_startxref_is_a_warning() {
    for delta in [1, -1, 64, -64] {
        let found = findings(&shift_startxref(&fixtures::golden_pdf(), delta));
        assert_eq!(found.len(), 1, "delta {delta}: {found:#?}");
        let f = &found[0];
        assert_eq!(f.class, FindingKind::Corruption(C3TrailerDamaged));
        assert_eq!(f.severity, Severity::Warning, "delta {delta}");
        assert_eq!(f.repair, Repairability::Auto);
        assert_eq!(metric(f, "startxref_delta"), Some(&MetricValue::Int(delta)));
    }
}

#[test]
fn a_startxref_more_than_64_bytes_off_is_an_error() {
    for delta in [65, -65, 300] {
        let found = findings(&shift_startxref(&fixtures::golden_pdf(), delta));
        assert_eq!(found.len(), 1, "delta {delta}: {found:#?}");
        assert_eq!(found[0].class, FindingKind::Corruption(C3TrailerDamaged));
        assert_eq!(found[0].severity, Severity::Error, "delta {delta}");
        assert_eq!(
            metric(&found[0], "startxref_delta"),
            Some(&MetricValue::Int(delta))
        );
    }
}

#[test]
fn an_off_startxref_in_an_xref_stream_file_is_measured_from_the_header() {
    let mut buf = b"junk before the header\n".to_vec();
    buf.extend_from_slice(&shift_startxref(&fixtures::golden_pdf_objstm(), 2));
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        metric(&found[0], "startxref_delta"),
        Some(&MetricValue::Int(2))
    );
}

#[test]
fn a_missing_tail_is_c3_with_the_tail_as_evidence() {
    let buf = fixtures::corrupt(C3TrailerDamaged, &fixtures::golden_pdf(), 0);
    let found = findings(&buf);
    let c3 = of_class(&found, C3TrailerDamaged);
    assert_eq!(c3.len(), 1);
    assert_eq!(c3[0].severity, Severity::Error);
    let window = c3[0].evidence.iter().find_map(|e| match e {
        Evidence::HexWindow(w) => Some(w),
        _ => None,
    });
    let window = window.expect("a hex window at the tail");
    assert_eq!(window.at() as usize + window.bytes().len(), buf.len());
}

// ── C4 ──────────────────────────────────────────────────────────────────

#[test]
fn a_count_that_disagrees_is_c4_with_both_numbers() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3, 4], 3)),
            obj(3, &page(2)),
            obj(4, &page(2)),
        ],
        "",
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C4PageTreeBroken));
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(metric(f, "pages_discovered"), Some(&MetricValue::Int(2)));
    assert_eq!(metric(f, "count_declared"), Some(&MetricValue::Int(3)));
    assert!(f.evidence.contains(&Evidence::ObjectRef((1, 0))));
    assert!(f.evidence.contains(&Evidence::ObjectRef((2, 0))));
}

/// The C4 Error findings and the C4 Warning findings.
fn c4_split(found: &[Finding]) -> (Vec<&Finding>, Vec<&Finding>) {
    of_class(found, C4PageTreeBroken)
        .into_iter()
        .partition(|f| f.severity == Severity::Error)
}

#[test]
fn cyclic_kids_are_a_warning_and_c4_fires_once() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3, 5], 2)),
            obj(3, &page(2)),
            obj(5, "<< /Type /Pages /Parent 2 0 R /Kids [2 0 R] /Count 1 >>"),
        ],
        "",
    );
    let found = findings(&buf);
    let (errors, warnings) = c4_split(&found);
    assert_eq!(errors.len(), 1, "{found:#?}");
    assert_eq!(warnings.len(), 1, "{found:#?}");
    assert!(
        warnings[0].summary.contains("cycle"),
        "{}",
        warnings[0].summary
    );
    assert_eq!(found.len(), 2);
}

#[test]
fn a_page_in_two_kids_arrays_is_a_warning_and_c4_fires_once() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3, 5], 3)),
            obj(3, &page(2)),
            obj(4, &page(5)),
            obj(
                5,
                "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 3 0 R] /Count 2 >>",
            ),
        ],
        "",
    );
    let found = findings(&buf);
    let (errors, warnings) = c4_split(&found);
    assert_eq!(errors.len(), 1, "{found:#?}");
    assert_eq!(warnings.len(), 1, "{found:#?}");
    assert!(
        warnings[0].summary.contains("twice"),
        "{}",
        warnings[0].summary
    );
    assert!(
        matches!(warnings[0].location, Location::Object { id: (3, 0), .. }),
        "{:?}",
        warnings[0].location
    );
}

#[test]
fn a_tree_deeper_than_100_levels_is_a_warning_and_c4_fires_once() {
    // Nodes 2..=103 chain down one kid each; the page is at depth 102.
    let mut parts = vec![obj(1, CATALOG)];
    for n in 2..=103u32 {
        let parent = if n == 2 {
            String::new()
        } else {
            format!("/Parent {} 0 R ", n - 1)
        };
        parts.push(obj(
            n,
            &format!("<< /Type /Pages {parent}/Kids [{} 0 R] /Count 1 >>", n + 1),
        ));
    }
    parts.push(obj(104, &page(103)));
    let found = findings(&classic(&parts, ""));
    let (errors, warnings) = c4_split(&found);
    assert_eq!(errors.len(), 1, "{found:#?}");
    assert_eq!(warnings.len(), 1, "{found:#?}");
    assert!(
        warnings[0].summary.contains("100"),
        "{}",
        warnings[0].summary
    );
}

#[test]
fn a_kid_that_names_nothing_is_c4() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3, 9], 2)),
            obj(3, &page(2)),
        ],
        "",
    );
    let found = findings(&buf);
    let (errors, warnings) = c4_split(&found);
    assert_eq!(errors.len(), 1, "{found:#?}");
    assert!(warnings.is_empty());
}

#[test]
fn no_catalog_with_a_page_tree_is_c4() {
    let buf = classic(
        &[
            obj(1, "<< /Type /Catalog >>"),
            obj(2, &pages(&[3], 1)),
            obj(3, &page(2)),
        ],
        "",
    );
    let found = findings(&buf);
    assert_eq!(structural(&found), BTreeSet::from([C4PageTreeBroken]));
}

// ── C5 ──────────────────────────────────────────────────────────────────

#[test]
fn each_orphan_is_its_own_c5_finding_numbered_in_byte_order() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n";
    let buf = classic(
        &[
            obj(1, CATALOG),
            Part::Raw(font.to_owned()),
            obj(2, &pages(&[3], 1)),
            Part::Raw(font.to_owned()),
            obj(3, &page(2)),
        ],
        "",
    );
    let found = findings(&buf);
    let c5 = of_class(&found, C5ObjectTagStripped);
    assert_eq!(c5.len(), 2, "{found:#?}");
    assert_eq!(c5[0].id, "C5-001");
    assert_eq!(c5[1].id, "C5-002");
    let start = |f: &Finding| match f.location {
        Location::Span(s) => s.start,
        ref other => panic!("an orphan's location is its span, got {other:?}"),
    };
    assert!(start(c5[0]) < start(c5[1]));
    assert_eq!(
        metric(c5[0], "kind"),
        Some(&MetricValue::Text("Font".into()))
    );
    assert_eq!(structural(&found), BTreeSet::from([C5ObjectTagStripped]));
}

// ── C10 ─────────────────────────────────────────────────────────────────

#[test]
fn a_cut_stream_is_c10_at_its_object_with_the_kept_fraction() {
    let golden = fixtures::golden_pdf();
    let buf = fixtures::corrupt(C10Truncated, &golden, 0);
    let found = findings(&buf);
    let c10 = of_class(&found, C10Truncated);
    assert_eq!(c10.len(), 1);
    let f = c10[0];
    assert_eq!(f.severity, Severity::Error);
    assert!(matches!(f.repair, Repairability::Partial(_)));
    let Location::Object { id, span } = f.location else {
        panic!("C10 sits at the clamped object: {:?}", f.location);
    };
    assert_eq!(id, (8, 0));
    assert_eq!(span.map(|s| s.end), Some(buf.len() as u64));
    let Some(MetricValue::Ratio(kept)) = metric(f, "kept_fraction") else {
        panic!("a kept fraction");
    };
    assert!(*kept < Ratio { num: 1, den: 1 });
    assert!(*kept > Ratio { num: 0, den: 1 });
}

#[test]
fn trailing_junk_cut_after_eof_is_not_c10() {
    let mut buf = fixtures::golden_pdf();
    buf.extend_from_slice(b"\nsome trailing junk that a transfer appended");
    let cut = buf.len() - 10;
    assert_eq!(findings(&buf[..cut]), vec![]);
}

#[test]
fn a_startxref_past_the_end_is_c10() {
    let golden = fixtures::golden_pdf();
    let buf = shift_startxref(&golden, 100_000);
    let found = findings(&buf);
    assert_eq!(
        structural(&found),
        BTreeSet::from([C10Truncated]),
        "{found:#?}"
    );
}

// ── Encrypted ───────────────────────────────────────────────────────────

fn assert_encrypted(found: &[Finding], what: &str) {
    assert_eq!(found.len(), 1, "{what}: {found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Encrypted, "{what}");
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(
        f.repair,
        Repairability::Unrepairable("decrypt first".into())
    );
    assert_eq!(f.id, "ENC-001");
    assert!(f.evidence.contains(&Evidence::ObjectRef((99, 0))), "{what}");
}

#[test]
fn encrypt_in_a_trailer_is_unrepairable() {
    let golden = fixtures::golden_pdf();
    let trailer = rfind(&golden, b"trailer").expect("trailer");
    let at = trailer + find(&golden[trailer..], b"<<").expect("trailer dict") + 2;
    let buf = insert(&golden, at, b"/Encrypt 99 0 R");
    assert_encrypted(&findings(&buf), "classic trailer");
}

#[test]
fn encrypt_in_an_xref_stream_dict_is_unrepairable() {
    let golden = fixtures::golden_pdf_objstm();
    let at = rfind(&golden, b"<</Type /XRef").expect("xref stream dict") + 2;
    let buf = insert(&golden, at, b"/Encrypt 99 0 R ");
    assert_encrypted(&findings(&buf), "xref stream");
}

#[test]
fn encrypt_only_in_a_shadowed_trailer_still_fires() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page(2)),
            Part::Raw("trailer\n<< /Root 1 0 R /Encrypt 99 0 R >>\n".into()),
        ],
        "",
    );
    assert_encrypted(&findings(&buf), "shadowed trailer");
}

// ── Signed ──────────────────────────────────────────────────────────────

#[test]
fn the_signed_golden_is_one_signed_finding_and_nothing_else() {
    let found = findings(&fixtures::golden_pdf_signed());
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Signed { fields: 1 });
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.repair, Repairability::NotApplicable);
    assert_eq!(f.id, "SIG-001");
    assert_eq!(
        f.summary,
        "the original is digitally signed; a repaired file is not"
    );
    assert!(f.evidence.contains(&Evidence::ObjectRef((14, 0))));
    assert_eq!(metric(f, "fields"), Some(&MetricValue::Int(1)));
}

#[test]
fn a_byte_range_alone_or_a_shadow_copy_still_counts_as_signed() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page(2)),
            obj(7, "<< /ByteRange [0 10 20 30] /Contents <00> >>"),
            obj(8, "<< /Type /Sig /Contents <00> >>"),
            // A later copy of 8 that is not a signature: the earlier one is a
            // shadow and still counts, once.
            obj(8, "<< /Type /Annot >>"),
        ],
        "",
    );
    let found = findings(&buf);
    let signed: Vec<&Finding> = found
        .iter()
        .filter(|f| matches!(f.class, FindingKind::Signed { .. }))
        .collect();
    assert_eq!(signed.len(), 1, "{found:#?}");
    assert_eq!(signed[0].class, FindingKind::Signed { fields: 2 });
}

// ── order and determinism ───────────────────────────────────────────────

#[test]
fn findings_come_in_byte_order_and_repeat_exactly() {
    let golden = fixtures::golden_pdf();
    // C1 at the head, C5 in the body: a header overwrite and a stripped tag.
    let both = fixtures::corrupt(
        C1Header,
        &fixtures::corrupt(C5ObjectTagStripped, &golden, 2),
        1,
    );
    let a = findings(&both);
    let b = findings(&both);
    assert_eq!(a, b);
    let codes: Vec<&str> = a.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(codes, ["C1-001", "C5-001"], "{a:#?}");
}

// ── file_meta ───────────────────────────────────────────────────────────

#[test]
fn file_meta_of_the_goldens() {
    let expected = FileMeta {
        version: Some("1.7".into()),
        pages: 2,
        title: None,
        page_sizes: vec![(612, 792), (612, 792)],
    };
    assert_eq!(meta(&fixtures::golden_pdf()), expected);
    let objstm = meta(&fixtures::golden_pdf_objstm());
    assert_eq!(objstm.pages, 2);
    assert_eq!(objstm.page_sizes, expected.page_sizes);
}

#[test]
fn file_meta_reads_the_title_from_the_info_dictionary() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page(2)),
            obj(9, "<< /Title (Quarterly figures) /Producer (hand) >>"),
        ],
        "/Info 9 0 R ",
    );
    assert_eq!(meta(&buf).title.as_deref(), Some("Quarterly figures"));

    let utf16 = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page(2)),
            obj(9, "<< /Title <FEFF00E9007400E9> >>"),
        ],
        "/Info 9 0 R ",
    );
    assert_eq!(meta(&utf16).title.as_deref(), Some("été"));
}

#[test]
fn file_meta_inherits_the_media_box_and_rounds_to_whole_points() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595.3 841.9] >>",
            ),
            obj(3, "<< /Type /Page /Parent 2 0 R >>"),
        ],
        "",
    );
    let m = meta(&buf);
    assert_eq!(m.pages, 1);
    assert_eq!(m.page_sizes, vec![(595, 842)]);
}

#[test]
fn file_meta_without_a_readable_header_has_no_version() {
    let buf = fixtures::corrupt(C1Header, &fixtures::golden_pdf(), 0);
    let m = meta(&buf);
    assert_eq!(m.version, None);
    assert_eq!(m.pages, 2);
}
