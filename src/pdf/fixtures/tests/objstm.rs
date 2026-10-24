//! The ObjStm golden (T-03b): hand-assembled object and cross-reference
//! streams that strict lopdf and hayro both read. The rows and the header are
//! re-read here from the file's own bytes.

use super::*;

use lopdf::xref::XrefEntry;

/// The ObjStm's `(objnum, offset)` pairs and the bytes after `/First`.
pub(super) fn objstm_entries(doc: &Document, id: u32) -> (Vec<(u32, usize)>, Vec<u8>) {
    let stm = doc.get_object((id, 0)).unwrap().as_stream().unwrap();
    assert_eq!(stm.dict.get(b"Type").unwrap(), &name(b"ObjStm"));
    let n = stm.dict.get(b"N").unwrap().as_i64().unwrap() as usize;
    let first = stm.dict.get(b"First").unwrap().as_i64().unwrap() as usize;
    let data = stm.decompressed_content().unwrap();
    let header = std::str::from_utf8(&data[..first]).unwrap();
    let nums: Vec<usize> = header
        .split_ascii_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    assert_eq!(nums.len(), 2 * n, "{n} pairs in the header");
    let pairs = nums.chunks(2).map(|p| (p[0] as u32, p[1])).collect();
    (pairs, data[first..].to_vec())
}

/// The xref stream's rows as `(type, field 2, field 3)`, checked against `/W [1 4 2]`.
fn xref_rows(pdf: &[u8]) -> Vec<(u8, u32, u16)> {
    let at = startxref(pdf);
    assert!(
        pdf[at..].starts_with(b"14 0 obj"),
        "startxref names the xref stream"
    );
    let doc = load_strict(pdf);
    let xref = doc.get_object((14, 0)).unwrap().as_stream().unwrap();
    assert_eq!(xref.dict.get(b"Type").unwrap(), &name(b"XRef"));
    assert_eq!(
        xref.dict.get(b"W").unwrap(),
        &Object::Array(vec![int(1), int(4), int(2)])
    );
    assert_eq!(xref.dict.get(b"Size").unwrap(), &int(15));
    assert_eq!(xref.dict.get(b"Root").unwrap(), &r(1));
    let rows = xref.decompressed_content().unwrap();
    assert_eq!(rows.len(), 15 * 7);
    rows.chunks(7)
        .map(|row| {
            let f2 = u32::from_be_bytes([row[1], row[2], row[3], row[4]]);
            (row[0], f2, u16::from_be_bytes([row[5], row[6]]))
        })
        .collect()
}

#[test]
fn objstm_golden_loads_in_strict_lopdf_with_the_goldens_objects() {
    let g = golden_pdf_objstm();
    assert!(g.starts_with(b"%PDF-1.5\n%"));
    assert_eq!(find(&g, b"\nxref\n"), None, "no classic table");
    assert_eq!(find(&g, b"trailer"), None, "the xref stream is the trailer");

    let doc = load_strict(&g);
    assert_eq!(doc.get_pages().len(), 2);
    for id in 1..=7 {
        let entry = doc.reference_table.get(id);
        assert!(
            matches!(entry, Some(XrefEntry::Compressed { container: 13, index }) if u32::from(*index) == id - 1),
            "object {id} is packed: {entry:?}"
        );
    }
    for id in 8..=14 {
        assert!(
            matches!(doc.reference_table.get(id), Some(XrefEntry::Normal { .. })),
            "object {id} is top level"
        );
    }

    // Every object is the classic golden's, value for value.
    let classic = load_strict(&golden_pdf());
    for id in 1..=12 {
        assert_eq!(
            doc.get_object((id, 0)).unwrap(),
            classic.get_object((id, 0)).unwrap(),
            "object {id}"
        );
    }
    let descriptor = deref(&doc, cidfont(&doc).get(b"FontDescriptor").unwrap())
        .as_dict()
        .unwrap();
    let file = doc
        .get_object((stream_id(descriptor, b"FontFile2"), 0))
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(file.decompressed_content().unwrap(), TEST_FONT);
}

#[test]
fn objstm_golden_renders_in_hayro() {
    let (inked, warnings) = render_pages(golden_pdf_objstm());
    assert_eq!(inked, [true, true]);
    assert_eq!(warnings, 0, "hayro warned");
}

#[test]
fn objstm_golden_rows_and_header_are_what_the_bytes_say() {
    let g = golden_pdf_objstm();
    let rows = xref_rows(&g);
    let doc = load_strict(&g);
    let (pairs, objects) = objstm_entries(&doc, 13);
    assert_eq!(
        pairs.iter().map(|p| p.0).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(rows[0], (0, 0, 65_535), "the free head");
    for (id, &(kind, f2, f3)) in rows.iter().enumerate().skip(1) {
        let id = id as u32;
        match kind {
            1 => {
                assert_eq!(f3, 0, "object {id} generation");
                let header = format!("{id} 0 obj");
                assert!(
                    g[f2 as usize..].starts_with(header.as_bytes()),
                    "object {id}: row offset {f2}"
                );
            }
            2 => {
                assert_eq!(f2, 13, "object {id} container");
                let (num, offset) = pairs[f3 as usize];
                assert_eq!(num, id, "object {id} index {f3}");
                assert!(objects[offset..].starts_with(b"<<"), "object {id} offset");
            }
            other => panic!("object {id}: row type {other}"),
        }
    }
    // The offsets climb, and each object ends where the next begins.
    assert!(pairs.windows(2).all(|w| w[0].1 < w[1].1));
}

#[test]
fn objstm_golden_is_byte_identical_across_builds() {
    assert_eq!(golden_pdf_objstm(), golden_pdf_objstm());
}
