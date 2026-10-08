//! The adversarial builders of TD §20 (T-03b): each claimed property is read
//! back from the bytes.

use super::*;

use super::objstm::objstm_entries;

/// Every offset where `needle` starts.
fn all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    hay.windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(i, _)| i)
        .collect()
}

/// Every `N 0 obj` header at a line start, as `(offset, N)`.
fn headers(pdf: &[u8]) -> Vec<(usize, u32)> {
    all(pdf, b" 0 obj")
        .into_iter()
        .filter_map(|at| {
            let start = pdf[..at].iter().rposition(|b| !b.is_ascii_digit())? + 1;
            let line_start = start == 0 || matches!(pdf[start - 1], b'\n' | b'\r');
            let n = std::str::from_utf8(&pdf[start..at]).ok()?.parse().ok()?;
            (line_start && start < at).then_some((start, n))
        })
        .collect()
}

/// The data of the stream whose `N 0 obj` header is at `header`, sliced by the
/// bytes alone: after `stream` and `eol`, up to the `endstream` that follows.
fn raw_body(pdf: &[u8], header: usize, eol: &[u8], end_eol: &[u8]) -> Range<usize> {
    let kw = header + find(&pdf[header..], b"stream").unwrap() + 6;
    assert_eq!(&pdf[kw..kw + eol.len()], eol, "EOL after `stream`");
    let start = kw + eol.len();
    let end = start + find(&pdf[start..], b"endstream").unwrap() - end_eol.len();
    assert_eq!(
        &pdf[end..end + end_eol.len()],
        end_eol,
        "EOL before `endstream`"
    );
    start..end
}

fn inflate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::inflate::decompress_to_vec_zlib(bytes).expect("zlib data")
}

fn ascii85_decode(data: &[u8]) -> Vec<u8> {
    let end = find(data, b"~>").expect("A85 end marker");
    let mut out = Vec::new();
    let mut group = Vec::new();
    for &c in &data[..end] {
        match c {
            b'z' => out.extend_from_slice(&[0; 4]),
            b'!'..=b'u' => {
                group.push(u32::from(c - b'!'));
                if group.len() == 5 {
                    let v = group.iter().fold(0u32, |v, d| v * 85 + d);
                    out.extend_from_slice(&v.to_be_bytes());
                    group.clear();
                }
            }
            c if c.is_ascii_whitespace() => {}
            other => panic!("byte {other:#x} in A85 data"),
        }
    }
    if !group.is_empty() {
        let n = group.len();
        group.resize(5, 84);
        let v = group.iter().fold(0u32, |v, d| v * 85 + d);
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
    }
    out
}

/// The content stream the golden draws on `page` (0-based).
fn golden_content(page: usize) -> Vec<u8> {
    content(&font_facts(), page)
}

fn page_content_stream(doc: &Document, page: u32) -> &Stream {
    let page = doc.get_dictionary(doc.get_pages()[&page]).unwrap();
    deref(doc, page.get(b"Contents").unwrap())
        .as_stream()
        .unwrap()
}

fn image_on_page_1(doc: &Document) -> &Stream {
    let page1 = doc.get_dictionary(doc.get_pages()[&1]).unwrap();
    let resources = page1.get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    deref(doc, xobjects.get(b"Im1").unwrap())
        .as_stream()
        .unwrap()
}

fn hayro_image_on_page_1(bytes: Vec<u8>) -> Vec<u8> {
    let pdf = Pdf::new(bytes).expect("hayro loads");
    let page = &pdf.pages()[0];
    let image = page
        .resources()
        .get_x_object(&Name::new(b"Im1").unwrap())
        .expect("page 1 /Im1");
    let params = ImageDecodeParams {
        bpc: Some(8),
        num_components: Some(3),
        width: 8,
        height: 8,
        ..ImageDecodeParams::default()
    };
    image
        .decoded_image(&params)
        .expect("image decodes")
        .data
        .into_owned()
}

#[test]
fn keywords_sit_inside_stream_data() {
    let b = with_stream_containing_keywords();
    let doc = load_strict(&b);
    let s = page_content_stream(&doc, 1);
    let data = data_range(&b, &doc, CONTENTS[0]);
    for kw in [b"endstream".as_slice(), b"endobj", b"1 0 obj"] {
        let inside: Vec<usize> = all(&b, kw)
            .into_iter()
            .filter(|at| data.contains(at))
            .collect();
        assert!(!inside.is_empty(), "{:?} not in the raw data", kw);
        // At a line start, as a landmark scan would see it.
        assert!(inside.iter().any(|&at| b[at - 1] == b'\n'));
        let decoded = s.decompressed_content().unwrap();
        assert!(find(&decoded, kw).is_some());
    }
    assert_eq!(s.dict.get(b"Filter").unwrap(), &name(b"FlateDecode"));
    let (inked, warnings) = render_pages(b);
    assert_eq!(inked, [true, true]);
    assert_eq!(warnings, 0);
}

#[test]
fn wrong_length_declares_what_the_kind_says() {
    let actual_of = |b: &[u8]| {
        let header = find(b, b"\n11 0 obj\n").unwrap() + 1;
        raw_body(b, header, b"\n", b"\n").len() as i64
    };
    let declared = |b: &[u8]| -> Option<Object> {
        let doc = Document::load_mem(b).ok()?;
        let s = doc.get_object((CONTENTS[0], 0)).ok()?.as_stream().ok()?;
        s.dict.get(b"Length").ok().cloned()
    };
    let actual = actual_of(&with_wrong_length(LengthKind::Correct));
    assert_eq!(
        actual,
        flate(&golden_content(0)).len() as i64,
        "the data is the golden's"
    );
    for kind in [
        LengthKind::Correct,
        LengthKind::TooShort,
        LengthKind::TooLong,
        LengthKind::Indirect,
        LengthKind::Missing,
    ] {
        let b = with_wrong_length(kind);
        assert_eq!(actual_of(&b), actual, "{kind:?}: data length");
        let header = find(&b, b"\n11 0 obj\n").unwrap() + 1;
        let dict = &b[header..header + find(&b[header..], b"stream").unwrap()];
        let length = find(dict, b"/Length ").map(|at| {
            let v = &dict[at + 8..];
            let end = v.iter().position(|&c| c == b'/' || c == b'>').unwrap();
            String::from_utf8(v[..end].to_vec()).unwrap()
        });
        match kind {
            LengthKind::Correct => assert_eq!(length, Some(actual.to_string())),
            LengthKind::TooShort => {
                let l: i64 = length.unwrap().trim().parse().unwrap();
                assert!(l < actual, "{l} < {actual}");
            }
            LengthKind::TooLong => {
                let l: i64 = length.unwrap().trim().parse().unwrap();
                assert!(l > actual, "{l} > {actual}");
            }
            LengthKind::Indirect => {
                assert_eq!(length.as_deref().map(str::trim), Some("13 0 R"));
                let def = find(&b, b"\n13 0 obj\n").unwrap() + 1;
                assert!(def > header, "a forward reference");
                let body = &b[def + 9..def + find(&b[def..], b"endobj").unwrap()];
                assert_eq!(
                    std::str::from_utf8(body).unwrap().trim(),
                    actual.to_string()
                );
                assert_eq!(declared(&b), Some(int(actual)), "lopdf resolves it");
            }
            LengthKind::Missing => assert_eq!(length, None),
        }
    }
    for kind in [LengthKind::Correct, LengthKind::Indirect] {
        let b = with_wrong_length(kind);
        let doc = load_strict(&b);
        assert_eq!(
            page_content_stream(&doc, 1).decompressed_content().unwrap(),
            golden_content(0),
            "{kind:?}"
        );
    }
}

#[test]
fn eol_style_follows_every_stream_keyword() {
    for (style, eol, end_eol) in [
        (EolStyle::Lf, b"\n".as_slice(), b"\n".as_slice()),
        (EolStyle::CrLf, b"\r\n", b"\r\n"),
        (EolStyle::Cr, b"\r", b"\r"),
        (EolStyle::SpaceEol, b" \n", b"\n"),
    ] {
        let b = with_eol_style(style);
        let golden = load_strict(&golden_pdf());
        let mut streams = 0;
        for (at, id) in headers(&b) {
            let Ok(s) = golden.get_object((id, 0)).unwrap().as_stream() else {
                continue;
            };
            let body = raw_body(&b, at, eol, end_eol);
            assert_eq!(&b[body], s.content.as_slice(), "{style:?}: object {id}");
            streams += 1;
        }
        assert_eq!(streams, 5, "{style:?}");
        if matches!(style, EolStyle::Lf | EolStyle::CrLf) {
            assert_eq!(load_strict(&b).get_pages().len(), 2, "{style:?}");
        }
    }
}

#[test]
fn junk_prefix_puts_n_bytes_before_the_header() {
    let g = golden_pdf();
    for n in [1, 7, 100, 1_000] {
        let b = with_junk_prefix(n);
        assert_eq!(b.len(), g.len() + n);
        assert_eq!(&b[n..], g.as_slice(), "{n}");
        assert_eq!(find(&b, b"%PDF-"), Some(n), "{n}");
        assert!(b[..n].iter().all(|&c| c != b'%'), "{n}");
    }
}

#[test]
fn duplicate_object_has_two_whole_copies_and_the_xref_names_the_later() {
    let b = with_duplicate_object();
    let copies: Vec<usize> = headers(&b)
        .into_iter()
        .filter(|&(_, n)| n == CONTENTS[1])
        .map(|(at, _)| at)
        .collect();
    assert_eq!(copies.len(), 2);
    let bodies: Vec<Vec<u8>> = copies
        .iter()
        .map(|&at| {
            let data = raw_body(&b, at, b"\n", b"\n");
            let tail = &b[data.end..];
            assert!(tail.starts_with(b"\nendstream\nendobj\n"), "a whole copy");
            inflate(&b[data])
        })
        .collect();
    assert_ne!(bodies[0], bodies[1]);
    assert_eq!(
        bodies[1],
        golden_content(1),
        "the later copy is the golden's"
    );
    let doc = load_strict(&b);
    let entry = doc.reference_table.get(CONTENTS[1]);
    assert!(
        matches!(entry, Some(lopdf::xref::XrefEntry::Normal { offset, generation: 0 }) if *offset as usize == copies[1]),
        "{entry:?}"
    );
    assert_eq!(
        page_content_stream(&doc, 2).decompressed_content().unwrap(),
        golden_content(1)
    );
}

#[test]
fn truncated_later_duplicate_is_cut_by_eof() {
    let b = with_truncated_later_duplicate();
    let g = golden_pdf();
    assert!(b.starts_with(&g), "the whole golden comes first");
    let copies: Vec<usize> = headers(&b)
        .into_iter()
        .filter(|&(_, n)| n == CONTENTS[1])
        .map(|(at, _)| at)
        .collect();
    assert_eq!(copies.len(), 2);
    assert!(copies[0] < g.len() && copies[1] >= g.len());
    let later = &b[copies[1]..];
    assert_eq!(find(later, b"endstream"), None);
    assert_eq!(find(later, b"endobj"), None);
    let dict_end = find(later, b"stream\n").unwrap();
    let declared: usize = {
        let at = find(&later[..dict_end], b"/Length ").unwrap() + 8;
        let digits: String = later[at..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .map(|&c| char::from(c))
            .collect();
        digits.parse().unwrap()
    };
    let present = later.len() - (dict_end + 7);
    assert!(present > 0 && present < declared, "{present} of {declared}");
    // The earlier copy is whole and is the golden's.
    let doc = load_strict(&g);
    let early = data_range(&b[..g.len()], &doc, CONTENTS[1]);
    let tail = &b[early.end..g.len()];
    assert!(tail.starts_with(b"\nendstream") && find(tail, b"endobj").is_some());
}

#[test]
fn no_endobj_drops_exactly_one_endobj() {
    let b = with_no_endobj();
    let hs = headers(&b);
    assert_eq!(hs.len(), 12);
    for (k, &(at, id)) in hs.iter().enumerate() {
        let end = hs.get(k + 1).map_or(b.len(), |h| h.0);
        let has = find(&b[at..end], b"endobj").is_some();
        assert_eq!(has, id != DESCRIPTOR, "object {id}");
    }
}

#[test]
fn multi_filter_is_ascii85_then_flate() {
    let b = with_multi_filter();
    let doc = load_strict(&b);
    let s = page_content_stream(&doc, 1);
    assert_eq!(
        s.dict.get(b"Filter").unwrap(),
        &Object::Array(vec![name(b"ASCII85Decode"), name(b"FlateDecode")])
    );
    assert!(s.content.ends_with(b"~>"));
    assert_eq!(inflate(&ascii85_decode(&s.content)), golden_content(0));
    let (inked, warnings) = render_pages(b);
    assert_eq!(inked, [true, true]);
    assert_eq!(warnings, 0);
}

#[test]
fn predictor_image_declares_png_prediction_and_decodes_to_its_pixels() {
    let b = with_predictor_image();
    let doc = load_strict(&b);
    let image = image_on_page_1(&doc);
    assert_eq!(image.dict.get(b"Filter").unwrap(), &name(b"FlateDecode"));
    let parms = image.dict.get(b"DecodeParms").unwrap().as_dict().unwrap();
    assert_eq!(parms.get(b"Predictor").unwrap(), &int(15));
    assert_eq!(parms.get(b"Colors").unwrap(), &int(3));
    assert_eq!(parms.get(b"Columns").unwrap(), &int(8));
    let rows = inflate(&image.content);
    assert_eq!(rows.len(), 8 * (1 + 8 * 3));
    let kinds: std::collections::BTreeSet<u8> = rows.chunks(25).map(|r| r[0]).collect();
    assert_eq!(kinds, (0..=4).collect(), "every PNG filter type is used");
    assert_eq!(hayro_image_on_page_1(b.clone()), predictor_pixels());
    let (inked, warnings) = render_pages(b);
    assert_eq!(inked, [true, true]);
    assert_eq!(warnings, 0);
}

#[test]
fn flate_then_dct_wraps_the_jpeg() {
    let b = with_flate_then_dct();
    let doc = load_strict(&b);
    let image = image_on_page_1(&doc);
    assert_eq!(
        image.dict.get(b"Filter").unwrap(),
        &Object::Array(vec![name(b"FlateDecode"), name(b"DCTDecode")])
    );
    assert_eq!(inflate(&image.content), TINY_JPEG);
    assert_eq!(hayro_image_on_page_1(b).len(), 8 * 8 * 3);
}

#[test]
fn objstm_and_plain_copy_puts_the_named_copy_first() {
    for order in [CopyOrder::PlainFirst, CopyOrder::ObjStmFirst] {
        let b = with_objstm_and_plain_copy(order);
        let doc = Document::load_mem(&b).unwrap();
        let page = PAGE_IDS[1];
        let plain: Vec<usize> = headers(&b)
            .into_iter()
            .filter(|&(_, n)| n == page)
            .map(|(at, _)| at)
            .collect();
        assert_eq!(plain.len(), 1, "{order:?}: one plain copy");
        let container = headers(&b).into_iter().find(|&(_, n)| n == 13).unwrap().0;
        let (pairs, objects) = objstm_entries(&doc, 13);
        let k = pairs.iter().position(|p| p.0 == page).expect("packed copy");
        let packed_end = pairs.get(k + 1).map_or(objects.len(), |p| p.1);
        let packed = &objects[pairs[k].1..packed_end];
        let plain_body = &b[plain[0]..plain[0] + find(&b[plain[0]..], b"endobj").unwrap()];
        let (earlier, later) = match order {
            CopyOrder::PlainFirst => {
                assert!(plain[0] < container, "{order:?}");
                (plain_body, packed)
            }
            CopyOrder::ObjStmFirst => {
                assert!(container < plain[0], "{order:?}");
                (packed, plain_body)
            }
        };
        assert!(
            find(earlier, b"[0 0 612 792]").is_some(),
            "{order:?}: earlier"
        );
        assert!(find(later, b"[0 0 595 842]").is_some(), "{order:?}: later");
        // The xref stream names the later copy.
        let entry = doc.reference_table.get(page).unwrap();
        match order {
            CopyOrder::PlainFirst => assert!(
                matches!(
                    entry,
                    lopdf::xref::XrefEntry::Compressed { container: 13, .. }
                ),
                "{entry:?}"
            ),
            CopyOrder::ObjStmFirst => assert!(
                matches!(entry, lopdf::xref::XrefEntry::Normal { offset, .. } if *offset as usize == plain[0]),
                "{entry:?}"
            ),
        }
    }
}

/// The integer after `key` in `dict` text.
fn int_after(dict: &[u8], key: &[u8]) -> usize {
    let at = find(dict, key).unwrap_or_else(|| panic!("no {key:?}")) + key.len();
    let digits: String = dict[at..]
        .iter()
        .skip_while(|c| **c == b' ')
        .take_while(|c| c.is_ascii_digit())
        .map(|&c| char::from(c))
        .collect();
    digits.parse().unwrap()
}

#[test]
fn recursive_objstm_extends_in_a_cycle_and_holds_itself() {
    let b = with_recursive_objstm();
    // Read by the bytes alone: the dictionary and the inflated data of `id`.
    let container = |id: u32| {
        let at = headers(&b).into_iter().find(|&(_, n)| n == id).unwrap().0;
        let kw = at + find(&b[at..], b"stream\n").unwrap();
        let dict = &b[at..kw];
        assert!(
            find(dict, b"/Type /ObjStm").is_some(),
            "{id} is a container"
        );
        let data = &b[kw + 7..kw + 7 + int_after(dict, b"/Length")];
        (dict, inflate(data))
    };
    let (dict13, data13) = container(13);
    let (dict15, _) = container(15);
    assert_eq!(int_after(dict13, b"/Extends"), 15);
    assert_eq!(int_after(dict15, b"/Extends"), 13);
    // 13 lists itself, with a container-shaped value.
    let first = int_after(dict13, b"/First");
    let nums: Vec<usize> = std::str::from_utf8(&data13[..first])
        .unwrap()
        .split_ascii_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    let own = nums
        .chunks(2)
        .find(|p| p[0] == 13)
        .expect("13 lists itself");
    assert!(data13[first + own[1]..].starts_with(b"<</Type /ObjStm"));
}

#[test]
fn bad_objstm_offsets_are_non_monotonic_and_run_past_the_data() {
    let b = with_bad_objstm_offsets();
    let doc = Document::load_mem(&b).unwrap();
    let (pairs, objects) = objstm_entries(&doc, 13);
    assert_eq!(
        pairs.iter().map(|p| p.0).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6, 7]
    );
    assert!(
        pairs.windows(2).any(|w| w[1].1 < w[0].1),
        "non-monotonic: {pairs:?}"
    );
    assert!(pairs.iter().any(|p| p.1 >= objects.len()), "past the data");
    let good = golden_pdf_objstm();
    let (good_pairs, good_objects) = objstm_entries(&load_strict(&good), 13);
    assert_eq!(objects, good_objects, "only the header is bad");
    let same = pairs
        .iter()
        .zip(&good_pairs)
        .filter(|(a, b)| a == b)
        .count();
    assert_eq!(same, 4, "three entries are bad, the rest are right");
}

#[test]
fn outline_only_page_draws_paths_and_no_text() {
    let b = outline_only_page();
    let doc = load_strict(&b);
    assert_eq!(doc.get_pages().len(), 1);
    let ops = Content::decode(&page_content_stream(&doc, 1).decompressed_content().unwrap())
        .unwrap()
        .operations;
    let names: std::collections::BTreeSet<&str> =
        ops.iter().map(|op| op.operator.as_str()).collect();
    for text_op in ["BT", "ET", "Tf", "Tj", "TJ", "'", "\""] {
        assert!(!names.contains(text_op), "{text_op} drawn");
    }
    for path_op in ["m", "l", "c", "h", "f"] {
        assert!(names.contains(path_op), "{path_op} missing");
    }
    assert!(ops.iter().filter(|op| op.operator == "f").count() >= 10);
    for kw in [
        b"/Font".as_slice(),
        b"/FontFile",
        b"/Type /Font",
        b"/Type/Font",
    ] {
        assert_eq!(find(&b, kw), None, "{:?}", String::from_utf8_lossy(kw));
    }
    let (inked, warnings) = render_pages(b);
    assert_eq!(inked, [true]);
    assert_eq!(warnings, 0);
}

#[test]
fn type3_only_page_draws_text_through_glyph_procedures_only() {
    let b = type3_only_page();
    let doc = load_strict(&b);
    assert_eq!(doc.get_pages().len(), 1);
    let page = doc.get_pages()[&1];
    let fonts = doc.get_page_fonts(page).unwrap();
    assert_eq!(fonts.len(), 1);
    let font = fonts.values().next().unwrap();
    assert_eq!(font.get(b"Subtype").unwrap(), &name(b"Type3"));
    let procs = font.get(b"CharProcs").unwrap().as_dict().unwrap();
    assert!(procs.len() >= 10, "{} glyph procedures", procs.len());
    for (_, p) in procs.iter() {
        assert!(deref(&doc, p).as_stream().is_ok());
    }
    assert_eq!(find(&b, b"/FontFile"), None, "no font program to blank");
    let ops = Content::decode(&page_content_stream(&doc, 1).decompressed_content().unwrap())
        .unwrap()
        .operations;
    assert!(ops.iter().any(|op| op.operator == "Tj"));
    let (inked, warnings) = render_pages(b);
    assert_eq!(inked, [true]);
    assert_eq!(warnings, 0);
}

#[test]
fn builders_are_byte_identical_across_builds() {
    let all: [fn() -> Vec<u8>; 11] = [
        with_stream_containing_keywords,
        with_duplicate_object,
        with_truncated_later_duplicate,
        with_no_endobj,
        with_multi_filter,
        with_predictor_image,
        with_flate_then_dct,
        with_recursive_objstm,
        with_bad_objstm_offsets,
        outline_only_page,
        type3_only_page,
    ];
    for f in all {
        assert_eq!(f(), f());
    }
}

#[test]
fn hand_built_fixtures_match_their_committed_hashes() {
    // Same-process builds cannot see drift across platforms or toolchains
    // (skrifa outlines, miniz_oxide output); a committed hash checked on every
    // CI OS can.
    type Builder = fn() -> Vec<u8>;
    let all: [(&str, Builder, &str); 3] = [
        (
            "golden_pdf_objstm",
            golden_pdf_objstm,
            "7096ef6ba208e1f0fc64a00b731af024fb3721be0773a08961f83dc0e592b4d3",
        ),
        (
            "outline_only_page",
            outline_only_page,
            "18655937988ab97ce731bdf55fafcd4d3f18bcb36073aa0caa7ae4e45f9f538e",
        ),
        (
            "type3_only_page",
            type3_only_page,
            "a9b44e9820002b67a12569015094b3b04c7771ae97dc81e1dda673f505f556c7",
        ),
    ];
    for (label, f, want) in all {
        assert_eq!(hex(&Sha256::digest(f())), want, "{label}");
    }
}
