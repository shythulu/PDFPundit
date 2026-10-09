//! T-11b acceptance: C6 per (page, slot) with the orphan fonts as re-link
//! candidates; C7 and C8 on the font corruptors; C9 from the salvage grades
//! on the measured C9 corruptor (and none of C1–C5/C10), with width-array
//! damage undetected; the `Ambiguous` survivors in the evidence; the
//! near-miss keyword warning; `OutlinedText` and `Type3Text`.

use std::num::NonZeroUsize;

use super::*;
use crate::engine::SalvageBudget;
use crate::pdf::carver::CarveNote;
use crate::pdf::fixtures::ReplacementSite;
use crate::pdf::lexer::LexNote;
use crate::pdf::model::InteractionKind;
use crate::pdf::streams::salvage::{
    CarveSource, FilterStage, Grade, OverMaxSearchStream, Salvage, SalvageEntry, salvage_all,
};
use crate::pdf::streams::{ImgCodec, StreamClass};

use CorruptionClass::{
    C5ObjectTagStripped, C6FontMapLost, C7FontStreamDeleted, C8FontResourcesDeleted, C9ZlibTampered,
};

// ── helpers ─────────────────────────────────────────────────────────────

/// Diagnoses `buf` against `salvage` and checks the id rule.
fn findings_with(buf: &[u8], salvage: &SalvageIndex) -> Vec<Finding> {
    let carve = carved(buf);
    let graph = ObjectGraph::from_carve(&carve);
    let out = diagnose(buf, &carve, &graph, salvage);
    check_ids(&out);
    out
}

/// A budget small enough for a debug test run.
fn test_budget() -> SalvageBudget {
    SalvageBudget {
        work: 20_000_000,
        deep_work: 0,
        deep_pool: 0,
        ..SalvageBudget::default()
    }
}

/// `buf`'s salvage index, as analysis builds it.
fn salvaged(buf: &[u8]) -> SalvageIndex {
    let carve = carved(buf);
    let source = CarveSource::new(&carve, buf);
    let one = NonZeroUsize::new(1).expect("one");
    match salvage_all(&source, &test_budget(), one, 1 << 30, &|| false) {
        Ok(index) => index,
        Err(Cancelled) => panic!("salvage cancelled without a cancel"),
    }
}

fn texts(f: &Finding) -> Vec<&str> {
    f.evidence
        .iter()
        .filter_map(|e| match e {
            Evidence::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

fn refs(f: &Finding) -> Vec<ObjId> {
    f.evidence
        .iter()
        .filter_map(|e| match e {
            Evidence::ObjectRef(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// An unfiltered stream object's body: `extra` dictionary entries, then `data`.
fn stream_body(extra: &str, data: &str) -> String {
    format!(
        "<< /Length {} {extra}>>\nstream\n{data}\nendstream",
        data.len()
    )
}

fn page_with(parent: u32, resources: &str, contents: u32) -> String {
    format!(
        "<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] /Resources {resources} \
         /Contents {contents} 0 R >>"
    )
}

/// The golden's objects.
const DESCRIPTOR: ObjId = (7, 0);
const CIDFONT: ObjId = (6, 0);
const CONTENT: ObjId = (11, 0);

// ── C6 ──────────────────────────────────────────────────────────────────

#[test]
fn c6_on_the_golden_is_one_finding_for_the_page_that_lost_its_fonts() {
    let golden = fixtures::golden_pdf();
    for seed in 0..6 {
        let buf = fixtures::corrupt(C6FontMapLost, &golden, seed);
        let found = findings(&buf);
        assert!(structural(&found).is_empty(), "seed {seed}: {found:#?}");
        let c6 = of_class(&found, C6FontMapLost);
        assert_eq!(c6.len(), 1, "seed {seed}: {found:#?}");
        let f = c6[0];
        assert_eq!(f.severity, Severity::Error);
        assert!(texts(f).contains(&"slots: F1"), "{f:#?}");
        let Location::Page { index, obj } = f.location else {
            panic!("C6 is located on its page: {f:#?}");
        };
        // The page whose `/Font` is gone.
        let page = obj.expect("the page's id");
        assert_eq!(page, ([3, 4][index as usize], 0));
        // The other page still uses the Type0 font, so no font is an orphan
        // and the slot needs a pick.
        assert!(refs(f).is_empty(), "{f:#?}");
        assert_eq!(
            f.repair,
            Repairability::Interactive(InteractionKind::FontPick)
        );
        assert_eq!(found.len(), 1, "seed {seed}: {found:#?}");
    }
}

#[test]
fn c6_names_the_orphan_font_as_its_re_link_candidate() {
    let buf = fixtures::corrupt(C6FontMapLost, &fixtures::type3_only_page(), 0);
    let found = findings(&buf);
    let c6 = of_class(&found, C6FontMapLost);
    assert_eq!(c6.len(), 1, "{found:#?}");
    assert!(texts(c6[0]).contains(&"slots: T1"));
    assert_eq!(refs(c6[0]), vec![(4, 0)], "the orphan Type3 font");
    assert_eq!(c6[0].repair, Repairability::Auto);
    assert!(structural(&found).is_empty(), "{found:#?}");
}

#[test]
fn a_descendant_font_is_not_a_candidate_of_its_own() {
    // Font 5 (Type0) and its CIDFont 6 are both unreachable once page 3
    // lost its `/Font`; only the top font is a re-link candidate.
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< >>", 4)),
            obj(
                4,
                &stream_body("", "BT /F1 12 Tf (Hi) Tj /F2 9 Tf (x) Tj ET"),
            ),
            obj(
                5,
                "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H \
                 /DescendantFonts [6 0 R] >>",
            ),
            obj(6, "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X >>"),
        ],
        "",
    );
    let found = findings(&buf);
    let c6 = of_class(&found, C6FontMapLost);
    assert_eq!(c6.len(), 2, "one per slot: {found:#?}");
    assert!(texts(c6[0]).contains(&"slots: F1"));
    assert!(texts(c6[1]).contains(&"slots: F2"));
    for f in c6 {
        assert_eq!(refs(f), vec![(5, 0)], "{f:#?}");
        assert_eq!(f.repair, Repairability::Auto);
        assert_eq!(
            f.location,
            Location::Page {
                index: 0,
                obj: Some((3, 0))
            }
        );
    }
}

#[test]
fn a_slot_carried_by_a_form_s_own_resources_is_not_c6() {
    let form = |resources: &str| {
        stream_body(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] {resources}"),
            "BT /F1 12 Tf (Hi) Tj ET",
        )
    };
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
    let file = |form_resources: &str| {
        classic(
            &[
                obj(1, CATALOG),
                obj(2, &pages(&[3], 1)),
                obj(3, &page_with(2, "<< /XObject << /X1 5 0 R >> >>", 4)),
                obj(4, &stream_body("", "q /X1 Do Q")),
                obj(5, &form(form_resources)),
                obj(6, font),
            ],
            "",
        )
    };
    let carried = findings(&file("/Resources << /Font << /F1 6 0 R >> >>"));
    assert!(of_class(&carried, C6FontMapLost).is_empty(), "{carried:#?}");

    // The form's own resources lack the slot, and so does the page.
    let lost = findings(&file("/Resources << >>"));
    let c6 = of_class(&lost, C6FontMapLost);
    assert_eq!(c6.len(), 1, "{lost:#?}");
    assert!(texts(c6[0]).contains(&"slots: F1"));
}

#[test]
fn a_slot_the_page_maps_is_not_c6() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< /Font << /F1 5 0 R >> >>", 4)),
            obj(4, &stream_body("", "BT /F1 12 Tf (Hi) Tj ET")),
            obj(5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ],
        "",
    );
    assert_eq!(findings(&buf), vec![]);
}

/// `buf` with `n 0 obj` blanked to spaces: the object loses its header and
/// every offset stays put.
fn strip_header(buf: &[u8], n: u32) -> Vec<u8> {
    let tag = format!("\n{n} 0 obj");
    let at = find(buf, tag.as_bytes()).expect("the object") + 1;
    let mut out = buf.to_vec();
    out[at..at + tag.len() - 1].fill(b' ');
    out
}

/// A one-page file whose `/F1` is mapped by resources object 5: from the
/// page (`inherited` false) or from its `/Pages` node.
fn indirect_resources(inherited: bool) -> Vec<u8> {
    let (node, leaf) = if inherited {
        ("/Resources 5 0 R ", "")
    } else {
        ("", "/Resources 5 0 R ")
    };
    classic(
        &[
            obj(1, CATALOG),
            obj(
                2,
                &format!("<< /Type /Pages /Kids [3 0 R] /Count 1 {node}>>"),
            ),
            obj(
                3,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] {leaf}/Contents 4 0 R >>"
                ),
            ),
            obj(4, &stream_body("", "BT /F1 12 Tf (Hi) Tj ET")),
            obj(5, "<< /Font << /F1 6 0 R >> >>"),
            obj(6, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ],
        "",
    )
}

#[test]
fn resources_that_lost_their_header_are_c5_and_not_c6() {
    for inherited in [false, true] {
        let buf = indirect_resources(inherited);
        assert_eq!(findings(&buf), vec![], "inherited {inherited}: clean");
        let found = findings(&strip_header(&buf, 5));
        assert_eq!(
            structural(&found),
            BTreeSet::from([C5ObjectTagStripped]),
            "inherited {inherited}: {found:#?}"
        );
        assert!(
            of_class(&found, C6FontMapLost).is_empty(),
            "inherited {inherited}: the slot is not orphaned: {found:#?}"
        );
    }
}

#[test]
fn resources_that_name_no_dictionary_are_not_c6() {
    // Object 5 is a number, so the page's `/Resources` maps nothing: that
    // break is not a lost font map.
    let buf = String::from_utf8(indirect_resources(false)).expect("ascii");
    let buf = buf.replace("<< /Font << /F1 6 0 R >> >>", "42");
    let found = findings(buf.as_bytes());
    assert!(of_class(&found, C6FontMapLost).is_empty(), "{found:#?}");
}

/// A one-page file whose `/Pages` node 2 maps `/F1` in inline inherited
/// `/Resources`, and whose page names `parent` as its `/Parent`.
fn inline_inherited(parent: u32) -> Vec<u8> {
    classic(
        &[
            obj(1, CATALOG),
            obj(
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 \
                 /Resources << /Font << /F1 5 0 R >> >> >>",
            ),
            obj(
                3,
                &format!(
                    "<< /Type /Page /Parent {parent} 0 R /MediaBox [0 0 612 792] \
                     /Contents 4 0 R >>"
                ),
            ),
            obj(4, &stream_body("", "BT /F1 12 Tf (Hi) Tj ET")),
            obj(5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
        ],
        "",
    )
}

#[test]
fn inherited_resources_on_a_headerless_pages_node_are_c5_and_not_c6() {
    let buf = inline_inherited(2);
    assert_eq!(findings(&buf), vec![], "clean");
    let found = findings(&strip_header(&buf, 2));
    assert_eq!(
        structural(&found),
        BTreeSet::from([C5ObjectTagStripped]),
        "{found:#?}"
    );
    assert!(
        of_class(&found, C6FontMapLost).is_empty(),
        "the node's resources still map the slot: {found:#?}"
    );
}

#[test]
fn a_page_whose_parent_names_nothing_is_not_c6() {
    // The catalog's tree reaches the page under a node that maps the slot,
    // but the page's `/Parent` names no object: the resources in force are
    // unknown, so the slot is not called lost.
    let found = findings(&inline_inherited(9));
    assert!(of_class(&found, C6FontMapLost).is_empty(), "{found:#?}");
}

#[test]
fn a_slot_mapped_to_null_or_to_no_object_at_all_is_c6() {
    for value in ["null", "8 0 R"] {
        let buf = classic(
            &[
                obj(1, CATALOG),
                obj(2, &pages(&[3], 1)),
                obj(
                    3,
                    &page_with(2, &format!("<< /Font << /F1 {value} >> >>"), 4),
                ),
                obj(4, &stream_body("", "BT /F1 12 Tf (Hi) Tj ET")),
            ],
            "",
        );
        let found = findings(&buf);
        let c6 = of_class(&found, C6FontMapLost);
        assert_eq!(c6.len(), 1, "{value}: {found:#?}");
        assert!(texts(c6[0]).contains(&"slots: F1"), "{value}");
        assert_eq!(
            c6[0].repair,
            Repairability::Interactive(InteractionKind::FontPick),
            "{value}"
        );
    }
}

#[test]
fn a_form_two_pages_draw_is_read_through_on_both() {
    // Pages 3 and 6 both draw form 5 (under a name that needs escaping),
    // which draws form 7, whose slot nothing maps: each page has its C6,
    // so the second page still reaches form 7 through form 5.
    let page = |contents: u32| page_with(2, "<< /XObject << /X#201 5 0 R >> >>", contents);
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3, 6], 2)),
            obj(3, &page(4)),
            obj(4, &stream_body("", "q /X#201 Do Q")),
            obj(
                5,
                &stream_body(
                    "/Type /XObject /Subtype /Form /BBox [0 0 10 10] \
                     /Resources << /XObject << /Inner 7 0 R >> >>",
                    "q /Inner Do Q",
                ),
            ),
            obj(6, &page(8)),
            obj(
                7,
                &stream_body(
                    "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << >>",
                    "BT /F9 12 Tf (Hi) Tj ET",
                ),
            ),
            obj(8, &stream_body("", "/X#201 Do")),
        ],
        "",
    );
    let found = findings(&buf);
    let c6 = of_class(&found, C6FontMapLost);
    assert_eq!(c6.len(), 2, "{found:#?}");
    for (f, page) in c6.iter().zip([3, 6]) {
        assert!(texts(f).contains(&"slots: F9"), "{f:#?}");
        assert!(matches!(f.location, Location::Page { obj: Some((p, 0)), .. } if p == page));
    }
}

#[test]
fn an_inline_type3_font_gives_no_type3_text() {
    // `Type3Text` names the font by object id; an inline font has none.
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(
                3,
                &page_with(
                    2,
                    "<< /Font << /T1 << /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] \
                     /FontMatrix [1 0 0 1 0 0] /CharProcs << >> >> >> >>",
                    4,
                ),
            ),
            obj(4, &stream_body("", "BT /T1 12 Tf (a) Tj ET")),
        ],
        "",
    );
    assert_eq!(findings(&buf), vec![]);
}

// ── C7 / C8 ─────────────────────────────────────────────────────────────

/// The descriptor's blank run: its `blanked_bytes` metric and its window.
fn blank_run(f: &Finding) -> (i64, &HexWindow) {
    let Some(MetricValue::Int(n)) = metric(f, "blanked_bytes") else {
        panic!("no blanked_bytes: {f:#?}");
    };
    let window = f
        .evidence
        .iter()
        .find_map(|e| match e {
            Evidence::HexWindow(w) => Some(w),
            _ => None,
        })
        .expect("the blank run");
    (*n, window)
}

#[test]
fn the_c7_corruptor_yields_c7_at_the_descriptor() {
    // REPDF's C7 blanks the `/FontFile2` entry and the program object: the
    // descriptor has no `/FontFile*` key, but a blank run where it was.
    let golden = fixtures::golden_pdf();
    let buf = fixtures::corrupt(C7FontStreamDeleted, &golden, 0);
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C7FontStreamDeleted));
    assert_eq!(f.severity, Severity::Error);
    let Location::Object {
        id: DESCRIPTOR,
        span: Some(span),
    } = f.location
    else {
        panic!("{f:#?}");
    };
    assert_eq!(refs(f), vec![DESCRIPTOR]);
    let entry_at = find(&golden, b"/FontFile2 ").expect("the golden's entry");
    let entry_len = find(&golden[entry_at..], b"R").expect("its reference") + 1;
    let (n, window) = blank_run(f);
    assert!(n >= entry_len as i64, "{n} < {entry_len}");
    assert!(span.start <= window.at() && window.at() <= entry_at as u64);
    assert!(window.bytes().iter().all(|&b| b == 0x20));
    assert!(f.summary.contains("overwritten"), "{}", f.summary);
    assert_eq!(
        f.repair,
        Repairability::Interactive(InteractionKind::FontPick)
    );
}

#[test]
fn the_c8_corruptor_yields_c8_and_not_c7() {
    let golden = fixtures::golden_pdf();
    let buf = fixtures::corrupt(C8FontResourcesDeleted, &golden, 0);
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C8FontResourcesDeleted));
    assert_eq!(f.severity, Severity::Error);
    assert!(matches!(
        f.location,
        Location::Object { id: DESCRIPTOR, .. }
    ));
    assert_eq!(refs(f), vec![DESCRIPTOR]);
    assert!(blank_run(f).0 > 0);
    assert_eq!(
        f.repair,
        Repairability::Interactive(InteractionKind::FontPick)
    );
}

#[test]
fn a_program_whose_stored_bytes_are_blank_is_c7() {
    // The entry survives and names a stream whose data is all 0x20.
    let buf = font_file(
        "/BaseFont /Garamond /ToUnicode 7 0 R ",
        "/FontName /Garamond /FontFile2 8 0 R ",
        &[
            obj(
                7,
                &stream_body("", "begincmap 1 beginbfchar <01> <0041> endbfchar endcmap"),
            ),
            obj(8, &stream_body("", &" ".repeat(40))),
        ],
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C7FontStreamDeleted));
    assert!(f.summary.contains("blank"), "{}", f.summary);
    assert!(metric(f, "fontfile_bytes").is_some());
}

/// `buf` with every occurrence of `text` overwritten with 0x20.
fn overwrite(buf: &[u8], text: &str) -> Vec<u8> {
    let mut out = buf.to_vec();
    let needle = text.as_bytes();
    let mut from = 0;
    while let Some(i) = find(&out[from..], needle) {
        out[from + i..from + i + needle.len()].fill(b' ');
        from += i + needle.len();
    }
    assert!(from > 0, "{text:?} is not in the file");
    out
}

/// `buf` with object `n` overwritten with 0x20, header through `endobj`.
fn blank_object(buf: &[u8], n: u32) -> Vec<u8> {
    let at = find(buf, format!("\n{n} 0 obj").as_bytes()).expect("the object") + 1;
    let end = at + find(&buf[at..], b"endobj").expect("endobj") + 6;
    let mut out = buf.to_vec();
    out[at..end].fill(b' ');
    out
}

/// A print-producer file (OBS-0001): Type0 font 5 holds its `CIDFont` and
/// that font's descriptor inline, one entry per line; 6 is the program, 7
/// the `/ToUnicode`. `descriptor` replaces the descriptor's entries.
fn inline_font(descriptor: &str) -> Vec<u8> {
    let type0 = format!(
        "<<\n/BaseFont /CIDFont+F1\n/DescendantFonts [<<\n/BaseFont /CIDFont+F1\n\
         /CIDSystemInfo << /Ordering (Identity) /Registry (Adobe) /Supplement 0 >>\n\
         /FontDescriptor <<\n{descriptor}>>\n/Subtype /CIDFontType2\n/Type /Font\n\
         /W [1 [500]]\n>> ]\n/Encoding /Identity-H\n/Subtype /Type0\n/ToUnicode 7 0 R\n\
         /Type /Font\n>>"
    );
    classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< /Font << /F1 5 0 R >> >>", 4)),
            obj(4, &stream_body("", "BT /F1 12 Tf <0001> Tj ET")),
            obj(5, &type0),
            obj(6, &stream_body("", "true and the rest of a font program")),
            obj(
                7,
                &stream_body(
                    "",
                    "begincmap 1 beginbfchar <0001> <0041> endbfchar endcmap",
                ),
            ),
        ],
        "",
    )
}

const INLINE_DESCRIPTOR: &str = "/Ascent 1068\n/Descent -292\n/Flags 6\n\
     /FontBBox [0 0 1000 1000]\n/FontFile2 6 0 R\n/FontName /CIDFont+F1\n/ItalicAngle 0\n\
     /StemV 0\n/Type /FontDescriptor\n";

#[test]
fn an_intact_inline_descriptor_gives_no_finding() {
    assert_eq!(findings(&inline_font(INLINE_DESCRIPTOR)), vec![]);
}

#[test]
fn repdf_c8_on_an_inline_descriptor_is_c8_at_its_font() {
    // REPDF's C8 on a print file: both entries and both objects blanked.
    let original = inline_font(INLINE_DESCRIPTOR);
    let mut buf = overwrite(&original, "/FontFile2 6 0 R");
    buf = overwrite(&buf, "/ToUnicode 7 0 R");
    buf = blank_object(&blank_object(&buf, 6), 7);
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C8FontResourcesDeleted));
    assert!(matches!(f.location, Location::Object { id: (5, 0), .. }));
    let entry_at = find(&original, b"/FontFile2 6 0 R").expect("the entry") as u64;
    let (n, window) = blank_run(f);
    assert_eq!(n, "/FontFile2 6 0 R".len() as i64);
    assert_eq!(window.at(), entry_at);
    assert_eq!(
        f.repair,
        Repairability::Interactive(InteractionKind::FontPick)
    );
}

#[test]
fn repdf_c7_on_an_inline_descriptor_is_c7_at_its_font() {
    let buf = blank_object(
        &overwrite(&inline_font(INLINE_DESCRIPTOR), "/FontFile2 6 0 R"),
        6,
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].class, FindingKind::Corruption(C7FontStreamDeleted));
    assert!(matches!(
        found[0].location,
        Location::Object { id: (5, 0), .. }
    ));
}

#[test]
fn an_inline_descriptor_that_never_had_a_program_is_not_embedded() {
    let never = INLINE_DESCRIPTOR.replace("/FontFile2 6 0 R\n", "");
    let found = findings(&inline_font(&never));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].class,
        FindingKind::FontNotEmbedded {
            font: (5, 0),
            base_font: "CIDFont+F1".into(),
        }
    );
}

/// A save-as-producer file (OBS-0002): Type0 font 5 names its `CIDFont` 9
/// through the array object 8 (`/DescendantFonts 8 0 R`), and descriptor 6
/// is an object of its own, written compactly; 10 is the program, 7 the
/// `/ToUnicode`.
fn indirect_descendants() -> Vec<u8> {
    classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< /Font << /F1 5 0 R >> >>", 4)),
            obj(4, &stream_body("", "BT /F1 12 Tf <0001> Tj ET")),
            obj(
                5,
                "<</Type/Font/Subtype/Type0/BaseFont/BCDEEE+Cambria/Encoding/Identity-H\
                 /DescendantFonts 8 0 R/ToUnicode 7 0 R>>",
            ),
            obj(
                6,
                "<</Type/FontDescriptor/FontName/BCDEEE+Cambria/Flags 32/ItalicAngle 0\
                 /FontBBox[ -1475 -222 2868 778] /FontFile2 10 0 R>>",
            ),
            obj(
                7,
                &stream_body(
                    "",
                    "begincmap 1 beginbfchar <0001> <0041> endbfchar endcmap",
                ),
            ),
            obj(8, "[ 9 0 R] "),
            obj(
                9,
                "<</Type/Font/Subtype/CIDFontType2/BaseFont/BCDEEE+Cambria\
                 /CIDSystemInfo<</Registry(Adobe)/Ordering(Identity)/Supplement 0>>\
                 /FontDescriptor 6 0 R/W[1[500]]>>",
            ),
            obj(10, &stream_body("", "true and the rest of a font program")),
        ],
        "",
    )
}

#[test]
fn repdf_c7_under_an_indirect_descendant_array_is_c7_not_c8() {
    // The `/ToUnicode` is on the Type0 parent, found through array 8.
    let original = indirect_descendants();
    assert_eq!(findings(&original), vec![]);
    let buf = blank_object(&overwrite(&original, "/FontFile2 10 0 R"), 10);
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].class, FindingKind::Corruption(C7FontStreamDeleted));
    assert!(matches!(
        found[0].location,
        Location::Object { id: (6, 0), .. }
    ));

    let c8 = blank_object(&overwrite(&buf, "/ToUnicode 7 0 R"), 7);
    let found = findings(&c8);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].class,
        FindingKind::Corruption(C8FontResourcesDeleted)
    );
}

#[test]
fn a_null_tounicode_is_lost_as_an_absent_one_is() {
    // A rebuilt file writes a reference to a lost object as `null`.
    let text = String::from_utf8(indirect_descendants()).expect("ascii");
    let original = text.replace("/ToUnicode 7 0 R", "/ToUnicode null ");
    let buf = blank_object(&overwrite(original.as_bytes(), "/FontFile2 10 0 R"), 10);
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].class,
        FindingKind::Corruption(C8FontResourcesDeleted)
    );
}

#[test]
fn a_standard_encoded_simple_font_needs_no_tounicode_so_its_lost_program_is_c7() {
    // Word's WinAnsi TrueType fonts never carry a `/ToUnicode`: their text
    // decodes through the encoding, so only the program is lost. A simple
    // font with no standard encoding needs one, so the same damage is C8.
    for (encoding, class) in [
        ("/Encoding /WinAnsiEncoding ", C7FontStreamDeleted),
        (
            "/Encoding << /BaseEncoding /MacRomanEncoding /Differences [32 /space] >> ",
            C7FontStreamDeleted,
        ),
        ("", C8FontResourcesDeleted),
    ] {
        let original = font_file(
            &format!("/BaseFont /BCDFEE+Cambria {encoding}"),
            "/FontName /BCDFEE+Cambria /Flags 32 /FontFile2 8 0 R ",
            &[obj(
                8,
                &stream_body("", "true and the rest of a font program"),
            )],
        );
        assert_eq!(findings(&original), vec![], "{encoding}");
        let buf = blank_object(&overwrite(&original, "/FontFile2 8 0 R"), 8);
        let found = findings(&buf);
        assert_eq!(found.len(), 1, "{encoding}: {found:#?}");
        assert_eq!(found[0].class, FindingKind::Corruption(class), "{encoding}");
    }
}

#[test]
fn an_indented_descriptor_that_never_had_a_program_is_not_embedded() {
    // Pretty-printed indentation is a run of 0x20 too, but one that sits
    // between a line break and the next entry: no entry was there.
    let indent = " ".repeat(24);
    let buf = font_file(
        "/BaseFont /Arial ",
        &format!("\n{indent}/FontName /Arial\n{indent}/Flags 32\n{indent}"),
        &[],
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].class,
        FindingKind::FontNotEmbedded {
            font: (5, 0),
            base_font: "Arial".into(),
        }
    );
}

/// A one-page file whose font 5 has descriptor 6 (`descriptor` entries) and
/// `font` entries.
fn font_file(font: &str, descriptor: &str, extra: &[Part]) -> Vec<u8> {
    let mut parts = vec![
        obj(1, CATALOG),
        obj(2, &pages(&[3], 1)),
        obj(3, &page_with(2, "<< /Font << /F1 5 0 R >> >>", 4)),
        obj(4, &stream_body("", "BT /F1 12 Tf (Hi) Tj ET")),
        obj(
            5,
            &format!("<< /Type /Font /Subtype /TrueType /FontDescriptor 6 0 R {font}>>"),
        ),
        obj(6, &format!("<< /Type /FontDescriptor {descriptor}>>")),
    ];
    for p in extra {
        parts.push(match p {
            Part::Obj(n, s) => obj(*n, s),
            Part::Raw(s) => Part::Raw(s.clone()),
        });
    }
    classic(&parts, "")
}

#[test]
fn a_descriptor_that_never_had_a_font_program_is_not_embedded_not_c7() {
    let tounicode = obj(
        7,
        &stream_body(
            "",
            "/CIDInit /ProcSet findresource begin begincmap 1 beginbfchar <01> <0041> \
             endbfchar endcmap end",
        ),
    );
    let buf = font_file(
        "/BaseFont /ABCDEF+Garamond /ToUnicode 7 0 R ",
        "/FontName /ABCDEF+Garamond ",
        &[tounicode],
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        found[0].class,
        FindingKind::FontNotEmbedded {
            font: (5, 0),
            base_font: "Garamond".into(),
        }
    );

    let helvetica = font_file(
        "/BaseFont /Helvetica ",
        "/FontName /ABCDEF+Helvetica-Bold ",
        &[],
    );
    assert_eq!(findings(&helvetica), vec![], "standard 14: not embedded");
}

#[test]
fn a_font_program_that_dangles_or_is_not_a_reference_is_still_c7() {
    let tounicode = || {
        obj(
            7,
            &stream_body("", "begincmap 1 beginbfchar <01> <0041> endbfchar endcmap"),
        )
    };
    for descriptor in [
        "/FontName /Garamond /FontFile2 9 0 R ",
        "/FontName /Garamond /FontFile2 /Gone ",
    ] {
        let buf = font_file(
            "/BaseFont /Garamond /ToUnicode 7 0 R ",
            descriptor,
            &[tounicode()],
        );
        let found = findings(&buf);
        assert_eq!(found.len(), 1, "{descriptor}: {found:#?}");
        let f = &found[0];
        assert_eq!(
            f.class,
            FindingKind::Corruption(C7FontStreamDeleted),
            "{descriptor}"
        );
        assert_eq!(f.severity, Severity::Error);
        assert!(f.summary.contains("gone"), "{f:#?}");
        assert_eq!(
            f.repair,
            Repairability::Interactive(InteractionKind::FontPick)
        );
    }
}

#[test]
fn a_font_file_that_does_not_sniff_as_a_font_is_c7() {
    let buf = font_file(
        "/BaseFont /Garamond /ToUnicode 7 0 R ",
        "/FontName /Garamond /FontFile2 8 0 R ",
        &[
            obj(
                7,
                &stream_body("", "begincmap 1 beginbfchar <01> <0041> endbfchar endcmap"),
            ),
            obj(8, &stream_body("", "this is not a font program at all")),
        ],
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].class, FindingKind::Corruption(C7FontStreamDeleted));
}

#[test]
fn a_surviving_but_unparsable_tounicode_is_c8() {
    let buf = font_file(
        "/BaseFont /Garamond /ToUnicode 7 0 R ",
        "/FontName /Garamond /FontFile2 8 0 R ",
        &[
            obj(7, &stream_body("", "random words, no cmap here")),
            obj(8, &stream_body("", "this is not a font program at all")),
        ],
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C8FontResourcesDeleted));
    assert!(texts(f).contains(&"tounicode unparsable"), "{f:#?}");
    assert_eq!(refs(f), vec![(6, 0), (7, 0)]);
}

#[test]
fn a_word_style_font_never_embedded_is_an_info_finding_not_damage() {
    // D-084 (b): a system font left out on purpose is not C7 or C8, even
    // with no `/ToUnicode`, and asks nothing.
    let arial = font_file(
        "/BaseFont /Arial /Encoding /WinAnsiEncoding ",
        "/FontName /Arial ",
        &[],
    );
    let found = findings(&arial);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(
        f.class,
        FindingKind::FontNotEmbedded {
            font: (5, 0),
            base_font: "Arial".into(),
        }
    );
    assert_eq!(f.id, "NOEMBED-001");
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.repair, Repairability::NotApplicable);
    assert_eq!(f.summary, "the font Arial is not embedded");
    assert!(matches!(f.location, Location::Object { id: (6, 0), .. }));
    assert_eq!(refs(f), vec![(6, 0), (5, 0)]);
}

#[test]
fn a_font_program_that_only_decodes_blank_keeps_its_c9() {
    // The stored bytes are not blank: the salvage's damaged decode is. C7
    // reports the blank program, and C9 still reports the damaged stream.
    let program = (8, 0);
    let buf = font_file(
        "/BaseFont /Garamond /ToUnicode 7 0 R ",
        "/FontName /Garamond /FontFile2 8 0 R ",
        &[
            obj(
                7,
                &stream_body("", "begincmap 1 beginbfchar <01> <0041> endbfchar endcmap"),
            ),
            obj(
                8,
                &stream_body("/Filter /FlateDecode ", "not zlib, but not blank"),
            ),
        ],
    );
    let index = index_of(vec![(
        program,
        Salvage::ChecksumMismatch { data: Vec::new() },
    )]);
    let found = findings_with(&buf, &index);
    let classes: Vec<FindingKind> = found.iter().map(|f| f.class.clone()).collect();
    assert_eq!(
        classes,
        vec![
            FindingKind::Corruption(C7FontStreamDeleted),
            FindingKind::Corruption(C9ZlibTampered),
        ],
        "{found:#?}"
    );
    assert!(matches!(
        found[1].location,
        Location::Object { id, .. } if id == program
    ));
}

// ── OutlinedText and Type3Text ──────────────────────────────────────────

#[test]
fn an_outline_only_page_is_outlined_text_and_not_c6() {
    let found = findings(&fixtures::outline_only_page());
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    let FindingKind::OutlinedText {
        glyph_runs,
        contours,
    } = f.class
    else {
        panic!("{f:#?}");
    };
    assert!(glyph_runs > 10, "one path per glyph drawn: {glyph_runs}");
    assert!(contours >= glyph_runs);
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.repair, Repairability::NotApplicable);
    assert_eq!(f.id, "OUTLINE-001");
    assert_eq!(
        f.location,
        Location::Page {
            index: 0,
            obj: Some((3, 0))
        }
    );
    assert_eq!(
        metric(f, "paths"),
        Some(&MetricValue::Int(i64::from(glyph_runs)))
    );
    assert_eq!(
        metric(f, "contours"),
        Some(&MetricValue::Int(i64::from(contours)))
    );
}

#[test]
fn filled_paths_with_fewer_than_three_curves_are_not_outlined_text() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< >>", 4)),
            // A rectangle, a two-curve fill and a stroked many-curve path.
            obj(
                4,
                &stream_body(
                    "",
                    "0 0 10 10 re f\n0 0 m 1 1 2 2 3 3 c 4 4 5 5 6 6 c f\n\
                     0 0 m 1 1 2 2 3 3 c 1 1 2 2 3 3 c 1 1 2 2 3 3 c S",
                ),
            ),
        ],
        "",
    );
    assert_eq!(findings(&buf), vec![]);
}

#[test]
fn a_type3_page_is_type3_text_and_not_c7() {
    let found = findings(&fixtures::type3_only_page());
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Type3Text { font: (4, 0) });
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.repair, Repairability::NotApplicable);
    assert_eq!(f.id, "TYPE3-001");
    assert_eq!(refs(f), vec![(4, 0)]);
    assert_eq!(
        f.location,
        Location::Page {
            index: 0,
            obj: Some((3, 0))
        }
    );
}

#[test]
fn a_type3_font_with_a_descriptor_is_not_c7() {
    let buf = classic(
        &[
            obj(1, CATALOG),
            obj(2, &pages(&[3], 1)),
            obj(3, &page_with(2, "<< /Font << /T1 5 0 R >> >>", 4)),
            obj(4, &stream_body("", "BT /T1 12 Tf (a) Tj ET")),
            obj(
                5,
                "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] \
                 /CharProcs << >> /FontDescriptor 6 0 R >>",
            ),
            obj(6, "<< /Type /FontDescriptor /FontName /T3Font >>"),
        ],
        "",
    );
    let found = findings(&buf);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].class, FindingKind::Type3Text { font: (5, 0) });
}

// ── C9 ──────────────────────────────────────────────────────────────────

/// The carved stream object whose data holds byte `at`, and whether its
/// `/Filter` still reads `/FlateDecode`.
fn stream_at(carve: &CarveReport, at: usize) -> Option<(ObjId, bool)> {
    carve.objects.iter().find_map(|o| match &o.body {
        Body::Stream { dict, data, .. } if (data.start..data.end).contains(&(at as u64)) => {
            let flate = dict.get(b"Filter").ok().and_then(|f| f.as_name().ok())
                == Some(&b"FlateDecode"[..]);
            Some((o.declared_id, flate))
        }
        _ => None,
    })
}

#[test]
fn the_c9_corruptor_yields_c9_and_no_structural_class() {
    let golden = fixtures::golden_pdf();
    let (mut width_hits, mut with_c9, mut filter_hits) = (0, 0, 0);
    for seed in 0..12 {
        let (buf, log) = fixtures::corrupt_with_log(C9ZlibTampered, &golden, seed);
        let index = salvaged(&buf);
        let found = findings_with(&buf, &index);
        let what = format!("seed {seed}: {found:#?}\n{log:?}");
        assert!(structural(&found).is_empty(), "{what}");
        let c9 = of_class(&found, C9ZlibTampered);
        if !c9.is_empty() {
            with_c9 += 1;
        }
        // One finding per damaged stream, at its stream object.
        let damaged: Vec<ObjId> = index
            .by_obj
            .iter()
            .filter(|(_, e)| !matches!(e.salvage, Salvage::Clean { .. }))
            .map(|(&id, _)| id)
            .collect();
        let located: Vec<ObjId> = c9
            .iter()
            .filter_map(|f| match f.location {
                Location::Object { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(located, damaged, "{what}");
        for f in &c9 {
            assert!(metric(f, "salvage").is_some(), "{f:#?}");
            match &f.severity {
                Severity::Warning => assert_eq!(f.repair, Repairability::Auto),
                Severity::Error => assert!(matches!(f.repair, Repairability::Partial(_))),
                Severity::Info => panic!("{f:#?}"),
            }
        }

        // Every Flate stream a replacement hit is a C9 finding, unless the
        // same run of damage also hit its `/Filter` name: the stream then
        // names no filter the salvage knows, and goes undetected.
        let carve = carved(&buf);
        for r in log.iter().filter(|r| r.site == ReplacementSite::FlateBody) {
            let (id, flate) = stream_at(&carve, r.at).expect("a hit lands in stream data");
            if flate {
                assert!(located.contains(&id), "{id:?} {what}");
            } else {
                filter_hits += 1;
            }
        }

        // Width-array damage changes no structure and no stream: nothing
        // reports the CIDFont whose `/W` was hit.
        let cidfont = carve
            .objects
            .iter()
            .find(|o| o.declared_id == CIDFONT)
            .expect("the CIDFont")
            .span;
        let in_widths = log
            .iter()
            .filter(|r| r.site == ReplacementSite::Array)
            .any(|r| (cidfont.start..cidfont.end).contains(&(r.at as u64)));
        if in_widths {
            width_hits += 1;
            assert!(
                found.iter().all(
                    |f| !matches!(f.location, Location::Object { id: CIDFONT, .. })
                        && !refs(f).contains(&CIDFONT)
                ),
                "width damage is undetected: {what}"
            );
        }
    }
    assert!(width_hits > 0, "some seed hits a width array");
    assert!(
        with_c9 >= 10,
        "{with_c9} of 12 seeds yield C9 ({filter_hits} filter-name hits)"
    );
}

fn entry(salvage: Salvage) -> SalvageEntry {
    SalvageEntry {
        salvage,
        stage: FilterStage {
            earlier: Vec::new(),
            flate_parms: None,
            later: Vec::new(),
        },
    }
}

fn index_of(entries: Vec<(ObjId, Salvage)>) -> SalvageIndex {
    SalvageIndex {
        by_obj: entries.into_iter().map(|(id, s)| (id, entry(s))).collect(),
        work_total: 0,
    }
}

#[test]
fn an_ambiguous_stream_is_a_partial_c9_listing_every_survivor() {
    let golden = fixtures::golden_pdf();
    let index = index_of(vec![(
        CONTENT,
        Salvage::Repaired {
            data: b"BT ET".to_vec(),
            edits: vec![(40, 0x31, 0x30)],
            grade: Grade::Ambiguous { outputs: 2 },
            survivors: vec![vec![(40, 0x31, 0x30)], vec![(97, 0x30, 0x31)]],
            work: 12_345,
            trailer_edit: false,
            adler_rerun: false,
        },
    )]);
    let found = findings_with(&golden, &index);
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C9ZlibTampered));
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(
        f.repair,
        Repairability::Partial("ambiguous: 2 candidate repairs".to_owned())
    );
    assert!(matches!(f.location, Location::Object { id: CONTENT, .. }));
    assert_eq!(
        metric(f, "salvage"),
        Some(&MetricValue::Text("Repaired".into()))
    );
    assert_eq!(
        metric(f, "grade"),
        Some(&MetricValue::Text("Ambiguous".into()))
    );
    assert_eq!(metric(f, "work"), Some(&MetricValue::Int(12_345)));
    let t = texts(f);
    assert!(t.contains(&"survivor 1: byte 40 0x31 -> 0x30"), "{t:?}");
    assert!(t.contains(&"survivor 2: byte 97 0x30 -> 0x31"), "{t:?}");
}

#[test]
fn each_salvage_outcome_maps_to_its_severity_and_repair() {
    let golden = fixtures::golden_pdf();
    let repaired = |grade| Salvage::Repaired {
        data: Vec::new(),
        edits: vec![(3, 1, 2)],
        grade,
        survivors: vec![vec![(3, 1, 2)]],
        work: 7,
        trailer_edit: false,
        adler_rerun: false,
    };
    let cases: Vec<(Salvage, Severity, bool, &str, Option<&str>)> = vec![
        (
            repaired(Grade::Exact),
            Severity::Warning,
            true,
            "Repaired",
            Some("Exact"),
        ),
        (
            repaired(Grade::Accepted {
                searched: 10,
                window: 20,
            }),
            Severity::Warning,
            true,
            "Repaired",
            Some("Accepted"),
        ),
        (
            Salvage::ChecksumMismatch { data: Vec::new() },
            Severity::Error,
            false,
            "ChecksumMismatch",
            None,
        ),
        (
            Salvage::Prefix {
                data: Vec::new(),
                in_used: 5,
                in_total: 9,
            },
            Severity::Error,
            false,
            "Prefix",
            None,
        ),
        (
            Salvage::Unrecoverable,
            Severity::Error,
            false,
            "Unrecoverable",
            None,
        ),
        (
            Salvage::Unsearched {
                reason: OverMaxSearchStream {
                    raw_len: 10,
                    limit: 5,
                },
            },
            Severity::Error,
            false,
            "Unsearched",
            Some("Unsearched"),
        ),
    ];
    for (salvage, severity, auto, name, grade) in cases {
        let found = findings_with(&golden, &index_of(vec![(CONTENT, salvage)]));
        assert_eq!(found.len(), 1, "{name}: {found:#?}");
        let f = &found[0];
        assert_eq!(f.class, FindingKind::Corruption(C9ZlibTampered), "{name}");
        assert_eq!(f.severity, severity, "{name}");
        if auto {
            assert_eq!(f.repair, Repairability::Auto, "{name}");
        } else {
            assert!(matches!(f.repair, Repairability::Partial(_)), "{name}");
        }
        assert_eq!(
            metric(f, "salvage"),
            Some(&MetricValue::Text(name.into())),
            "{name}"
        );
        assert_eq!(
            metric(f, "grade"),
            grade.map(|g| MetricValue::Text(g.into())).as_ref(),
            "{name}"
        );
    }

    // A clean stream is no finding.
    let clean = Salvage::Clean {
        decoded_len: 1,
        class: StreamClass::Image {
            codec: ImgCodec::Raw,
        },
        sha256: [0; 32],
    };
    assert_eq!(
        findings_with(&golden, &index_of(vec![(CONTENT, clean)])),
        vec![]
    );
}

#[test]
fn a_near_miss_keyword_is_a_c9_outside_stream_warning() {
    let golden = fixtures::golden_pdf();
    let mut carve = carved(&golden);
    let at = find(&golden, b"3 0 obj").expect("page 1") as u64 + 2;
    let page = carve
        .objects
        .iter_mut()
        .find(|o| o.declared_id == (3, 0))
        .expect("page 1");
    page.notes
        .push(CarveNote::Lex(LexNote::NearMissKeyword { at }));
    let graph = ObjectGraph::from_carve(&carve);
    let found = diagnose(&golden, &carve, &graph, &SalvageIndex::default());
    assert_eq!(found.len(), 1, "{found:#?}");
    let f = &found[0];
    assert_eq!(f.class, FindingKind::Corruption(C9ZlibTampered));
    assert_eq!(f.severity, Severity::Warning);
    assert!(f.summary.starts_with("C9-outside-stream"), "{}", f.summary);
    assert_eq!(
        metric(f, "salvage"),
        Some(&MetricValue::Text(OUTSIDE_STREAM.into()))
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, Evidence::HexWindow(w) if w.at() == at)),
        "{f:#?}"
    );
}

/// The golden with page 1's header keyword one byte off (`3 0 obk`): the
/// carve finds no object 3, and the near-miss lex reads `obk` in the gap.
fn with_a_near_miss_header() -> (Vec<u8>, u64) {
    let mut buf = fixtures::golden_pdf();
    let at = find(&buf, b"3 0 obj").expect("page 1") + 4;
    buf[at + 2] = b'k';
    (buf, at as u64)
}

#[test]
fn a_near_miss_keyword_outside_every_object_is_a_c9_outside_stream_warning() {
    let (buf, at) = with_a_near_miss_header();
    let found = findings_with(&buf, &SalvageIndex::default());
    let c9 = of_class(&found, C9ZlibTampered);
    assert_eq!(c9.len(), 1, "{found:#?}");
    let f = c9[0];
    assert_eq!(f.severity, Severity::Warning);
    assert!(f.summary.starts_with("C9-outside-stream"), "{}", f.summary);
    assert_eq!(
        f.location,
        Location::Span(ByteSpan {
            start: at,
            end: at + 3
        })
    );
    assert_eq!(
        metric(f, "salvage"),
        Some(&MetricValue::Text(OUTSIDE_STREAM.into()))
    );
    assert!(
        f.evidence
            .iter()
            .any(|e| matches!(e, Evidence::HexWindow(w) if w.at() == at)),
        "{f:#?}"
    );
}

#[test]
fn a_near_miss_outside_every_object_does_not_change_the_other_findings() {
    let (buf, _) = with_a_near_miss_header();
    let carve = carved(&buf);
    let graph = ObjectGraph::from_carve(&carve);
    let mut without = carved(&buf);
    without
        .notes
        .retain(|n| !matches!(n, CarveNote::Lex(LexNote::NearMissKeyword { .. })));
    let with: Vec<(FindingKind, Location)> =
        diagnose(&buf, &carve, &graph, &SalvageIndex::default())
            .into_iter()
            .filter(|f| f.class != FindingKind::Corruption(C9ZlibTampered))
            .map(|f| (f.class, f.location))
            .collect();
    let other: Vec<(FindingKind, Location)> =
        diagnose(&buf, &without, &graph, &SalvageIndex::default())
            .into_iter()
            .map(|f| (f.class, f.location))
            .collect();
    assert!(!other.is_empty());
    assert_eq!(with, other);
}

#[test]
fn under_a_real_salvage_the_font_corruptors_are_not_also_c9() {
    // C7 and C8 blank the zlib data, which the salvage cannot decode: the
    // stream is the font finding's, not a C9 of its own.
    let golden = fixtures::golden_pdf();
    for class in [C6FontMapLost, C7FontStreamDeleted, C8FontResourcesDeleted] {
        let buf = fixtures::corrupt(class, &golden, 0);
        let found = findings_with(&buf, &salvaged(&buf));
        let classes: Vec<FindingKind> = found.iter().map(|f| f.class.clone()).collect();
        assert_eq!(
            classes,
            vec![FindingKind::Corruption(class)],
            "{class:?}: {found:#?}"
        );
    }
}

#[test]
fn clean_files_stay_clean_under_a_real_salvage() {
    for (name, buf, expected) in [
        ("golden", fixtures::golden_pdf(), 0),
        ("objstm golden", fixtures::golden_pdf_objstm(), 0),
        ("outline page", fixtures::outline_only_page(), 1),
        ("type3 page", fixtures::type3_only_page(), 1),
    ] {
        let index = salvaged(&buf);
        let found = findings_with(&buf, &index);
        assert_eq!(found.len(), expected, "{name}: {found:#?}");
        assert!(
            found.iter().all(|f| f.severity == Severity::Info),
            "{name}: {found:#?}"
        );
        assert_eq!(found, findings(&buf), "{name}: the index changes nothing");
    }
}

#[test]
fn an_encrypted_file_gets_no_content_findings_under_a_real_salvage() {
    // Ciphertext reads as damaged Flate data and as fonts that are not font
    // programs: only the Encrypted finding stands.
    let golden = fixtures::golden_pdf();
    for (class, seed) in [
        (C9ZlibTampered, 0),
        (C9ZlibTampered, 1),
        (C7FontStreamDeleted, 0),
    ] {
        let buf = fixtures::corrupt(class, &golden, seed);
        let plain = findings_with(&buf, &salvaged(&buf));
        assert!(
            !of_class(&plain, class).is_empty(),
            "{class:?} seed {seed} is found unencrypted: {plain:#?}"
        );
        let trailer = rfind(&buf, b"trailer").expect("trailer");
        let at = trailer + find(&buf[trailer..], b"<<").expect("trailer dict") + 2;
        let buf = insert(&buf, at, b"/Encrypt 99 0 R");
        let found = findings_with(&buf, &salvaged(&buf));
        let classes: Vec<FindingKind> = found.iter().map(|f| f.class.clone()).collect();
        assert_eq!(
            classes,
            vec![FindingKind::Encrypted],
            "{class:?} seed {seed}: {found:#?}"
        );
    }
}
