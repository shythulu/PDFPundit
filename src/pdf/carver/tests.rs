//! T-05 acceptance: the carve of the goldens, the structural corruptors and
//! the adversarial builders, the header and extent rules on hand-written
//! bytes, the caps, cancellation and a fuzz run.

use std::cell::Cell;

use super::*;
use crate::pdf::fixtures::{self, EolStyle, LengthKind};
use crate::pdf::model::CorruptionClass;

fn carved(buf: &[u8]) -> CarveReport {
    match carve(buf, &|| false) {
        Ok(r) => r,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    }
}

fn carved_with(buf: &[u8], caps: Caps) -> CarveReport {
    match carve_with(buf, &|| false, caps) {
        Ok(r) => r,
        Err(Cancelled) => panic!("carve cancelled without a cancel"),
    }
}

fn ids(r: &CarveReport) -> Vec<ObjId> {
    r.objects.iter().map(|o| o.declared_id).collect()
}

fn range(s: ByteSpan) -> Range<usize> {
    s.start as usize..s.end as usize
}

/// The data span and length source of a stream object.
fn stream(o: &CarvedObject) -> (Range<usize>, LengthSource) {
    match &o.body {
        Body::Stream {
            data,
            length_source,
            ..
        } => (range(*data), *length_source),
        other => panic!("object {:?} is not a stream: {other:?}", o.declared_id),
    }
}

fn is_stream(o: &CarvedObject) -> bool {
    matches!(o.body, Body::Stream { .. })
}

fn object(r: &CarveReport, num: u32) -> &CarvedObject {
    r.objects
        .iter()
        .find(|o| o.declared_id == (num, 0))
        .unwrap_or_else(|| panic!("no object {num}"))
}

/// Every stream's data in a carve, by object number (first copy).
fn stream_data<'a>(buf: &'a [u8], r: &CarveReport) -> Vec<(u32, &'a [u8])> {
    r.objects
        .iter()
        .filter(|o| is_stream(o))
        .map(|o| (o.declared_id.0, &buf[stream(o).0]))
        .collect()
}

fn golden_ids() -> Vec<ObjId> {
    (1..=12).map(|n| (n, 0)).collect()
}

/// The golden carve's stream data by object number, owned.
fn golden_streams() -> Vec<(u32, Vec<u8>)> {
    let golden = fixtures::golden_pdf();
    let r = carved(&golden);
    stream_data(&golden, &r)
        .into_iter()
        .map(|(n, d)| (n, d.to_vec()))
        .collect()
}

fn golden_stream(num: u32) -> Vec<u8> {
    golden_streams()
        .into_iter()
        .find(|(n, _)| *n == num)
        .map(|(_, d)| d)
        .unwrap_or_else(|| panic!("golden has no stream {num}"))
}

fn has_note(o: &CarvedObject, f: impl Fn(&CarveNote) -> bool) -> bool {
    o.notes.iter().any(f)
}

// ---- the goldens ----

#[test]
fn golden_carves_every_object_with_its_declared_length() {
    let buf = fixtures::golden_pdf();
    let r = carved(&buf);
    assert_eq!(ids(&r), golden_ids());
    assert_eq!(
        r.header,
        Some(HeaderInfo {
            offset: 0,
            version: Some("1.7".into())
        })
    );
    let mut streams = 0;
    for o in &r.objects {
        assert_eq!(o.origin, Origin::TopLevel);
        assert!(o.notes.is_empty(), "{:?}: {:?}", o.declared_id, o.notes);
        assert!(buf[range(o.span)].ends_with(b"endobj"));
        let Body::Stream {
            dict,
            data,
            length_source,
        } = &o.body
        else {
            continue;
        };
        streams += 1;
        assert_eq!(*length_source, LengthSource::Declared);
        let declared = dict.get(b"Length").and_then(Object::as_i64).unwrap();
        assert_eq!(data.end - data.start, declared as u64);
        assert!(buf[..data.start as usize].ends_with(b"stream\n"));
        assert!(buf[data.end as usize..].starts_with(b"\nendstream"));
    }
    assert_eq!(streams, 5);
    assert_eq!(r.stats.rungs.declared, 5);
    assert!(r.notes.is_empty(), "{:?}", r.notes);
    assert!(r.orphans.is_empty());

    // The file structure, for diagnosis.
    assert_eq!(r.xref_spans.len(), 1);
    let xref = range(r.xref_spans[0]);
    assert!(buf[xref.clone()].starts_with(b"xref\n0 13\n"));
    assert!(buf[xref.end..].starts_with(b"trailer"));
    assert_eq!(r.trailer_spans.len(), 1);
    let trailer = &buf[range(r.trailer_spans[0])];
    assert!(trailer.starts_with(b"trailer") && trailer.ends_with(b">>"));
    assert_eq!(
        r.startxref,
        vec![(r.trailer_spans[0].end + 1, Some(xref.start as u64))]
    );
    assert_eq!(r.eof_markers.len(), 1);
    assert!(buf[r.eof_markers[0] as usize..].starts_with(b"%%EOF"));
}

#[test]
fn golden_kinds_come_from_type_subtype_and_content() {
    let r = carved(&fixtures::golden_pdf());
    let kinds: Vec<ObjectKind> = r.objects.iter().map(|o| o.kind.clone()).collect();
    use ObjectKind::*;
    assert_eq!(
        kinds,
        [
            Catalog,
            Pages,
            Page,
            Page,
            Font,
            Font,
            FontDescriptor,
            FontFile,
            ToUnicode,
            Image,
            ContentStream,
            ContentStream,
        ]
    );
}

// ---- the structural corruptors ----

/// Every object of `damaged` is one of the golden's, carved with a declared
/// length and the golden's stream data.
fn assert_declared_survivors(damaged: &[u8], expect: usize, what: &str) {
    let r = carved(damaged);
    assert_eq!(r.objects.len(), expect, "{what}: {:?}", ids(&r));
    let golden = golden_streams();
    for o in &r.objects {
        assert!(golden_ids().contains(&o.declared_id), "{what}");
        if is_stream(o) {
            let (data, source) = stream(o);
            assert_eq!(
                source,
                LengthSource::Declared,
                "{what}: {:?}",
                o.declared_id
            );
            let want = &golden
                .iter()
                .find(|(n, _)| *n == o.declared_id.0)
                .unwrap()
                .1;
            assert_eq!(&damaged[data], &want[..], "{what}: {:?}", o.declared_id);
        }
    }
}

#[test]
fn c2_and_c3_carve_every_object() {
    let golden = fixtures::golden_pdf();
    for class in [
        CorruptionClass::C2XrefMissing,
        CorruptionClass::C3TrailerDamaged,
    ] {
        let damaged = fixtures::corrupt(class, &golden, 7);
        assert_declared_survivors(&damaged, 12, class.code());
    }
    let c2 = carved(&fixtures::corrupt(
        CorruptionClass::C2XrefMissing,
        &golden,
        7,
    ));
    assert!(c2.xref_spans.is_empty());
    assert_eq!(c2.trailer_spans.len(), 1);
    let c3 = carved(&fixtures::corrupt(
        CorruptionClass::C3TrailerDamaged,
        &golden,
        7,
    ));
    assert!(c3.trailer_spans.is_empty() && c3.startxref.is_empty() && c3.eof_markers.is_empty());
}

#[test]
fn c5_carves_every_object_whose_header_survives() {
    let golden = fixtures::golden_pdf();
    for seed in 0..16 {
        let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, seed);
        assert_declared_survivors(&damaged, 11, &format!("C5 seed {seed}"));
    }
}

#[test]
fn c10_carves_the_survivors_and_clamps_the_cut_stream() {
    let golden = fixtures::golden_pdf();
    let full = carved(&golden);
    let damaged = fixtures::corrupt(CorruptionClass::C10Truncated, &golden, 0);
    let cut = damaged.len();
    let r = carved(&damaged);
    let survivors: Vec<&CarvedObject> = full
        .objects
        .iter()
        .filter(|o| (o.span.start as usize) + 8 < cut)
        .collect();
    assert_eq!(
        ids(&r),
        survivors.iter().map(|o| o.declared_id).collect::<Vec<_>>()
    );
    let mut clamped = 0;
    for (o, before) in r.objects.iter().zip(&survivors) {
        if before.span.end as usize <= cut {
            assert_eq!(o.span, before.span);
            if is_stream(o) {
                assert_eq!(stream(o), stream(before));
            }
        } else if is_stream(before) && (stream(before).0.end > cut) {
            let (data, source) = stream(o);
            assert_eq!(source, LengthSource::TruncatedAtEof);
            assert_eq!(data, stream(before).0.start..cut);
            assert!(has_note(o, |n| *n == CarveNote::NoEndobj));
            clamped += 1;
        }
    }
    assert_eq!(clamped, 1, "the 70% cut lands inside a stream's data");
}

// ---- the adversarial builders ----

#[test]
fn keywords_inside_stream_data_make_no_phantom_objects() {
    let buf = fixtures::with_stream_containing_keywords();
    let r = carved(&buf);
    assert_eq!(ids(&r), golden_ids());
    let (data, source) = stream(object(&r, 11));
    assert_eq!(source, LengthSource::Declared);
    let bytes = &buf[data];
    for kw in [&b"\nendstream\n"[..], b"\nendobj\n", b"\n1 0 obj\n"] {
        assert!(
            memmem::find(bytes, kw).is_some(),
            "the fixture's data holds {kw:?}"
        );
    }
}

#[test]
fn structure_keywords_inside_stream_data_are_not_reported() {
    let data = b"\nxref\n0 1\ntrailer\n<< /Size 1 >>\nstartxref\n9\n%%EOF\n 9999999999 0 obj\n";
    let mut buf = format!("%PDF-1.4\n1 0 obj\n<< /Length {} >>\nstream\n", data.len()).into_bytes();
    buf.extend_from_slice(data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    let r = carved(&buf);
    assert_eq!(ids(&r), [(1, 0)]);
    assert_eq!(stream(&r.objects[0]).1, LengthSource::Declared);
    assert!(r.xref_spans.is_empty() && r.trailer_spans.is_empty());
    assert!(r.startxref.is_empty() && r.eof_markers.is_empty());
    assert!(r.notes.is_empty(), "{:?}", r.notes);
}

#[test]
fn wrong_lengths_fall_to_the_unique_endstream() {
    let want = golden_stream(11);
    for kind in [
        LengthKind::Correct,
        LengthKind::TooShort,
        LengthKind::TooLong,
        LengthKind::Missing,
        LengthKind::Indirect,
    ] {
        let buf = fixtures::with_wrong_length(kind);
        let r = carved(&buf);
        let o = object(&r, 11);
        let (data, source) = stream(o);
        assert_eq!(&buf[data], &want[..], "{kind:?}");
        let mismatch = has_note(o, |n| matches!(n, CarveNote::LengthMismatch { .. }));
        let rungs = r.stats.rungs;
        if kind == LengthKind::Correct {
            assert_eq!(source, LengthSource::Declared);
            assert_eq!(rungs.declared, 5);
            assert_eq!(rungs.unique_endstream, 0);
        } else {
            // (c) is T-07's: an indirect length scans for now too.
            assert_eq!(source, LengthSource::ScannedEndstream, "{kind:?}");
            assert_eq!(rungs.declared, 4, "{kind:?}");
            assert_eq!(rungs.unique_endstream, 1, "{kind:?}");
            assert_eq!(rungs.last_endstream, 0, "{kind:?}");
        }
        assert_eq!(
            mismatch,
            matches!(kind, LengthKind::TooShort | LengthKind::TooLong),
            "{kind:?}"
        );
        if kind == LengthKind::Indirect {
            let len = object(&r, 13);
            assert_eq!(
                len.body,
                Body::Primitive(Object::Integer(want.len() as i64))
            );
        }
    }
}

#[test]
fn two_framed_endstreams_fall_past_rung_b_to_the_last() {
    let buf = b"%PDF-1.7\n1 0 obj\n<< >>\nstream\nAAA\nendstream\nendobj\nBBB\nendstream\nendobj\n2 0 obj\n<< /Type /Catalog >>\nendobj\n";
    let r = carved(buf);
    assert_eq!(ids(&r), [(1, 0), (2, 0)]);
    let o = &r.objects[0];
    let (data, source) = stream(o);
    assert_eq!(&buf[data], b"AAA\nendstream\nendobj\nBBB");
    assert_eq!(source, LengthSource::ScannedEndstream);
    assert!(has_note(o, |n| *n == CarveNote::EndstreamAmbiguous { candidates: 2 }));
    assert_eq!(r.stats.rungs.unique_endstream, 0);
    assert_eq!(r.stats.rungs.last_endstream, 1);
    assert!(buf[..o.span.end as usize].ends_with(b"BBB\nendstream\nendobj"));
    assert_eq!(object(&r, 2).kind, ObjectKind::Catalog);
}

#[test]
fn an_unframed_endstream_falls_to_rung_e() {
    // No EOL before `endstream`, then no `endobj` after it.
    for (src, want) in [
        (
            &b"1 0 obj\n<< >>\nstream\nABCendstream\nendobj\n"[..],
            &b"ABC"[..],
        ),
        (
            b"1 0 obj\n<< /Length 99 >>\nstream\nABC\nendstream\n2 0 obj 5 endobj",
            b"ABC",
        ),
    ] {
        let r = carved(src);
        let (data, source) = stream(&r.objects[0]);
        assert_eq!(&src[data], want);
        assert_eq!(source, LengthSource::ScannedEndstream);
        assert_eq!(r.stats.rungs.last_endstream, 1);
    }
}

#[test]
fn a_scanned_extent_never_holds_a_foreign_header() {
    // Object 1 has no /Length and no `endstream`; object 2's `endstream`
    // must not end it.
    let buf =
        b"1 0 obj\n<< >>\nstream\nxyz\n2 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj\n";
    let r = carved(buf);
    assert_eq!(ids(&r), [(1, 0), (2, 0)]);
    let first = &r.objects[0];
    let (data, source) = stream(first);
    assert_eq!(&buf[data], b"xyz");
    assert_eq!(source, LengthSource::TruncatedAtEof);
    assert!(has_note(first, |n| *n == CarveNote::NoEndstream));
    assert!(has_note(first, |n| *n == CarveNote::NoEndobj));
    assert_eq!(first.span.end, r.objects[1].span.start);
    assert_eq!(stream(&r.objects[1]).1, LengthSource::Declared);
}

#[test]
fn an_exact_declared_length_is_trusted_over_a_header() {
    // Rung (a) is exempt from the foreign-header check (module doc, rule 3):
    // a `/Length` landing exactly on object 2's EOL + `endstream` swallows
    // object 2, which becomes dead stream data. This pins the choice.
    let tail = b"stream\nAAA\nendobj\n2 0 obj\n<< /Length 3 >>\nstream\nBBB\nendstream\nendobj";
    let n = memmem::find(tail, b"\nendstream").expect("endstream") - b"stream\n".len();
    let buf = [format!("1 0 obj << /Length {n} >> ").as_bytes(), &tail[..]].concat();
    let r = carved(&buf);
    assert_eq!(ids(&r), [(1, 0)]);
    let (data, source) = stream(&r.objects[0]);
    assert_eq!(source, LengthSource::Declared);
    assert!(buf[data].ends_with(b"stream\nBBB"));
    assert!(r.objects[0].notes.is_empty(), "{:?}", r.objects[0].notes);
}

#[test]
fn every_eol_after_stream_is_accepted() {
    let golden = golden_streams();
    for style in [
        EolStyle::Lf,
        EolStyle::CrLf,
        EolStyle::Cr,
        EolStyle::SpaceEol,
    ] {
        let buf = fixtures::with_eol_style(style);
        let r = carved(&buf);
        assert_eq!(ids(&r), golden_ids(), "{style:?}");
        let got = stream_data(&buf, &r);
        assert_eq!(got.len(), golden.len());
        for ((n, data), (gn, want)) in got.iter().zip(&golden) {
            assert_eq!((n, *data), (gn, &want[..]), "{style:?}");
        }
        for o in r.objects.iter().filter(|o| is_stream(o)) {
            assert_eq!(stream(o).1, LengthSource::Declared, "{style:?}");
            let bare_cr = has_note(o, |n| matches!(n, CarveNote::BareCrAfterStream { .. }));
            assert_eq!(bare_cr, style == EolStyle::Cr, "{style:?}");
        }
    }
}

#[test]
fn eol_rules_on_hand_written_streams() {
    for (eol, bare_cr) in [
        (&b"\n"[..], false),
        (b"\r\n", false),
        (b"\r", true),
        (b" \n", false),
        (b" \r\n", false),
        (b" \r", true),
    ] {
        let mut buf = b"1 0 obj\n<< /Length 4 >>\nstream".to_vec();
        buf.extend_from_slice(eol);
        buf.extend_from_slice(b"x\r\nb\nendstream\nendobj\n");
        let r = carved(&buf);
        let o = &r.objects[0];
        let (data, source) = stream(o);
        assert_eq!(&buf[data], b"x\r\nb", "{eol:?}");
        assert_eq!(source, LengthSource::Declared);
        let noted = has_note(o, |n| matches!(n, CarveNote::BareCrAfterStream { .. }));
        assert_eq!(noted, bare_cr, "{eol:?}");
    }
}

#[test]
fn a_junk_prefix_moves_nothing() {
    for n in [1, 31, 100, 1500] {
        let buf = fixtures::with_junk_prefix(n);
        let r = carved(&buf);
        assert_eq!(ids(&r), golden_ids());
        assert_eq!(r.header.as_ref().map(|h| h.offset), Some(n as u64));
        assert!(
            r.objects
                .iter()
                .filter(|o| is_stream(o))
                .all(|o| stream(o).1 == LengthSource::Declared)
        );
    }
}

#[test]
fn junk_glued_to_a_header_hides_it() {
    // Rule 1: a header starts a line or follows whitespace.
    let r = carved(b"junk1 0 obj 5 endobj\njunk 2 0 obj 6 endobj\n");
    assert_eq!(ids(&r), [(2, 0)]);
}

#[test]
fn duplicate_ids_are_both_carved() {
    let buf = fixtures::with_duplicate_object();
    let r = carved(&buf);
    let twelves: Vec<&CarvedObject> = r
        .objects
        .iter()
        .filter(|o| o.declared_id == (12, 0))
        .collect();
    assert_eq!(twelves.len(), 2);
    assert!(twelves[0].span.end <= twelves[1].span.start);
    for o in &twelves {
        assert_eq!(stream(o).1, LengthSource::Declared);
    }
    assert_eq!(&buf[stream(twelves[1]).0], &golden_stream(12)[..]);
    assert_ne!(&buf[stream(twelves[0]).0], &golden_stream(12)[..]);

    let buf = fixtures::with_truncated_later_duplicate();
    let r = carved(&buf);
    assert_eq!(r.objects.len(), 13);
    let last = r.objects.last().unwrap();
    assert_eq!(last.declared_id, (12, 0));
    let (data, source) = stream(last);
    assert_eq!(source, LengthSource::TruncatedAtEof);
    assert_eq!(data.end, buf.len());
    assert!(has_note(last, |n| *n == CarveNote::NoEndobj));
    assert_eq!(stream(object(&r, 12)).1, LengthSource::Declared);
}

#[test]
fn a_missing_endobj_ends_at_the_next_header() {
    let buf = fixtures::with_no_endobj();
    let r = carved(&buf);
    assert_eq!(ids(&r), golden_ids());
    let (seven, eight) = (object(&r, 7), object(&r, 8));
    assert!(has_note(seven, |n| *n == CarveNote::NoEndobj));
    assert_eq!(seven.span.end, eight.span.start);
    assert_eq!(seven.kind, ObjectKind::FontDescriptor);
    assert!(eight.notes.is_empty());

    let r = carved(b"1 0 obj << /A 1 >>");
    assert_eq!(r.objects[0].span, ByteSpan { start: 0, end: 18 });
    assert!(has_note(&r.objects[0], |n| *n == CarveNote::NoEndobj));
}

// ---- rule 1: the header landmark ----

#[test]
fn header_shapes() {
    let cases: [(&[u8], &[ObjId]); 9] = [
        (b"1 0 obj 5 endobj", &[(1, 0)]),
        (b"x 7 0 obj 5 endobj", &[(7, 0)]),
        (b"7\n0\robj 5 endobj", &[(7, 0)]),
        (b"%c\r9 3 obj<<>>endobj", &[(9, 3)]),
        (b"x7 0 obj 5 endobj", &[]),
        (b"7  0 obj 5 endobj", &[]),
        (b"7 0  obj 5 endobj", &[]),
        (b"7 0 objx 5 endobj", &[]),
        // `endobj` tails are never headers.
        (b"1 0 obj 1 endobj 2 0 endobj", &[(1, 0)]),
    ];
    for (src, want) in cases {
        assert_eq!(
            ids(&carved(src)),
            want,
            "{:?}",
            String::from_utf8_lossy(src)
        );
    }
}

#[test]
fn header_numbers_out_of_range_are_ignored_with_a_note() {
    let r = carved(b"4294967295 65535 obj 1 endobj");
    assert_eq!(ids(&r), [(u32::MAX, 65535)]);
    for src in [&b"4294967296 0 obj 1 endobj"[..], b"1 65536 obj 1 endobj"] {
        let r = carved(src);
        assert!(r.objects.is_empty());
        assert_eq!(r.notes, [CarveNote::HeaderOutOfRange { at: 0 }]);
    }
}

#[test]
fn the_walk_back_stops_at_24_bytes() {
    // 21 digits, a space, `0`, a space: the header starts exactly 24 bytes
    // before `obj`.
    let fits = b"000000000000000000001 0 obj 1 endobj";
    assert_eq!(ids(&carved(fits)), [(1, 0)]);
    // One digit more and the number cannot be assembled in 24 bytes; the
    // note still says where the header starts.
    let r = carved(b"x 0000000000000000000001 0 obj 1 endobj");
    assert!(r.objects.is_empty());
    assert_eq!(r.notes, [CarveNote::HeaderOutOfRange { at: 2 }]);
    // A generation that long: the header starts at its number.
    let r = carved(b"x 7 0000000000000000000000001 obj 1 endobj");
    assert_eq!(r.notes, [CarveNote::HeaderOutOfRange { at: 2 }]);
    // ... or at the generation's first digit when no number stands before it.
    let r = carved(b"x 0000000000000000000000001 obj 1 endobj");
    assert_eq!(r.notes, [CarveNote::HeaderOutOfRange { at: 2 }]);
}

#[test]
fn bodies_dict_primitive_unparsed_and_dictless_stream() {
    let src = b"1 0 obj << /Type /Page >> endobj 2 0 obj [1 2] endobj 3 0 obj endobj 4 0 obj\nstream\nabc\nendstream\nendobj";
    let r = carved(src);
    assert!(matches!(r.objects[0].body, Body::Dict(_)));
    assert_eq!(r.objects[0].kind, ObjectKind::Page);
    assert!(matches!(
        r.objects[1].body,
        Body::Primitive(Object::Array(_))
    ));
    assert_eq!(r.objects[2].body, Body::Unparsed);
    assert_eq!(r.objects[2].kind, ObjectKind::Other(String::new()));
    let (data, source) = stream(&r.objects[3]);
    assert_eq!(&src[data], b"abc");
    assert_eq!(source, LengthSource::ScannedEndstream);
}

#[test]
fn an_unterminated_string_stops_at_the_next_header() {
    let r = carved(b"1 0 obj (abc\n2 0 obj 5 endobj\n");
    assert_eq!(ids(&r), [(1, 0), (2, 0)]);
    assert!(has_note(&r.objects[0], |n| matches!(
        n,
        CarveNote::Lex(LexNote::UnterminatedString { .. })
    )));
    assert_eq!(r.objects[1].body, Body::Primitive(Object::Integer(5)));
}

// ---- rule 6: caps ----

#[test]
fn the_landmark_cap_stops_the_scan_with_a_note() {
    let src = b"1 0 obj 1 endobj 2 0 obj 2 endobj 3 0 obj 3 endobj";
    let r = carved_with(
        src,
        Caps {
            landmarks: 3,
            objects: 10,
        },
    );
    assert_eq!(ids(&r), [(1, 0), (2, 0)]);
    assert_eq!(r.notes, [CarveNote::CapHit(Cap::Landmarks)]);
    assert_eq!(r.stats.landmarks, 3);
    // Exactly at the cap is not a hit.
    let r = carved_with(
        src,
        Caps {
            landmarks: 6,
            objects: 10,
        },
    );
    assert_eq!(r.objects.len(), 3);
    assert!(r.notes.is_empty());

    let r = carved_with(
        src,
        Caps {
            landmarks: 100,
            objects: 2,
        },
    );
    assert_eq!(ids(&r), [(1, 0), (2, 0)]);
    assert_eq!(r.notes, [CarveNote::CapHit(Cap::Objects)]);
}

#[test]
fn a_million_landmarks_is_the_cap() {
    let buf = b"%%EOF\n".repeat(MAX_LANDMARKS + 1);
    let r = carved(&buf);
    assert_eq!(r.eof_markers.len(), MAX_LANDMARKS);
    assert_eq!(r.notes, [CarveNote::CapHit(Cap::Landmarks)]);
    assert_eq!(MAX_OBJECTS, 1_000_000);
}

// ---- cancellation ----

#[test]
fn cancel_is_polled_at_least_every_256_landmarks() {
    let buf = b"1 0 obj << /A 1 >> endobj\n".repeat(10_000);
    let polls = Cell::new(0u64);
    let cancel = || {
        polls.set(polls.get() + 1);
        false
    };
    let r = carve(&buf, &cancel).expect("not cancelled");
    assert_eq!(r.objects.len(), 10_000);
    // Phase A visits 20,000 landmarks, phase B 10,000 headers.
    let landmarks = r.stats.landmarks + r.objects.len() as u64;
    assert_eq!(landmarks, 30_000);
    assert!(
        polls.get() >= landmarks.div_ceil(256),
        "{} polls",
        polls.get()
    );
}

#[test]
fn cancelling_mid_carve_on_64_mib_returns_at_the_next_poll() {
    let unit = b"1 0 obj << /A 1 >> endobj\n";
    let buf = unit.repeat((64 << 20) / unit.len());
    assert!(buf.len() > (64 << 20) - unit.len());
    let polls = Cell::new(0u64);
    let cancel = || {
        polls.set(polls.get() + 1);
        polls.get() > 1_000
    };
    assert_eq!(carve(&buf, &cancel).map(|_| ()), Err(Cancelled));
    // The poll that saw the cancel was the last: at most 256 landmarks after
    // the cancel was raised, the carve had returned.
    assert_eq!(polls.get(), 1_001);

    let polls = Cell::new(0u64);
    let cancel = || {
        polls.set(polls.get() + 1);
        true
    };
    assert_eq!(carve(&buf, &cancel).map(|_| ()), Err(Cancelled));
    assert_eq!(polls.get(), 1);
}

#[test]
fn structure_reads_stay_linear_and_poll_cancel() {
    // Each `trailer(` opens a string that never ends and each `startxref%` a
    // comment with no EOL: unbounded, every read would run to EOF and the
    // pass would be quadratic (minutes at this size). Each read stops at the
    // next landmark instead.
    for unit in [&b"trailer("[..], b"startxref%"] {
        let n = 200_000;
        let buf = unit.repeat(n);
        let r = carved(&buf);
        assert_eq!(r.trailer_spans.len() + r.startxref.len(), n);
        assert!(r.trailer_spans.iter().all(|s| s.end - s.start == 7));
        assert!(r.startxref.iter().all(|&(_, v)| v.is_none()));

        // No headers, so phase B never ticks: phase A ticks once per
        // landmark and the structure pass once more per landmark. A cancel
        // raised after phase A's polls is seen in the structure pass.
        let phase_a = (n as u64).div_ceil(POLL_EVERY);
        let polls = Cell::new(0u64);
        let cancel = || {
            polls.set(polls.get() + 1);
            polls.get() > phase_a
        };
        assert_eq!(carve(&buf, &cancel).map(|_| ()), Err(Cancelled));
        assert_eq!(polls.get(), phase_a + 1);
    }
}

#[test]
fn classification_decodes_a_bounded_prefix_within_the_budget() {
    let content = b"BT /F1 12 Tf 72 712 Td (Hi) Tj ET\n".repeat(10_000);
    assert!(content.len() > CLASSIFY_CAP);
    let flate = miniz_oxide::deflate::compress_to_vec_zlib(&content, 6);
    let mut dict = Dictionary::new();
    dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    // A long content stream classifies on its first CLASSIFY_CAP bytes.
    let mut c = Classifier::new(content.len());
    assert_eq!(c.kind(&dict, &flate), ObjectKind::ContentStream);
    assert_eq!(c.prefix(&dict, &flate).len(), CLASSIFY_CAP);
    // ... and so does a chain, a stage at a time.
    let mut hex_dict = dict.clone();
    hex_dict.set(
        "Filter",
        Object::Array(vec![
            Object::Name(b"ASCIIHexDecode".to_vec()),
            Object::Name(b"FlateDecode".to_vec()),
        ]),
    );
    let hex: Vec<u8> = flate
        .iter()
        .flat_map(|b| format!("{b:02x}").into_bytes())
        .collect();
    assert_eq!(
        Classifier::new(content.len()).kind(&hex_dict, &hex),
        ObjectKind::ContentStream
    );

    // A small file buys CLASSIFY_CAP bytes of decoding in all: the first
    // stream spends it, the next is classified by its dictionary alone.
    let mut c = Classifier::new(1);
    assert_eq!(c.prefix(&dict, &flate).len(), CLASSIFY_CAP);
    assert!(c.prefix(&dict, &flate).is_empty());
    assert_eq!(c.kind(&dict, &flate), ObjectKind::Other(String::new()));
    // Unfiltered data costs nothing and is still cut to the prefix.
    assert_eq!(
        c.kind(&Dictionary::new(), &content),
        ObjectKind::ContentStream
    );
    assert_eq!(c.prefix(&Dictionary::new(), &content).len(), CLASSIFY_CAP);
}

// ---- memory ----

#[test]
fn heap_bytes_counts_what_the_report_owns() {
    assert_eq!(CarveReport::default().heap_bytes(), 0);
    let buf = fixtures::golden_pdf();
    let r = carved(&buf);
    let floor = (r.objects.capacity() * size_of::<CarvedObject>()) as u64;
    assert!(r.heap_bytes() > floor);
    // Stream data stays in the file: a stream a thousand times longer costs
    // the report nothing more.
    let with_data = |n: usize| {
        let mut b = format!("1 0 obj\n<< /Length {n} >>\nstream\n").into_bytes();
        b.resize(b.len() + n, b'x');
        b.extend_from_slice(b"\nendstream\nendobj\n");
        carved(&b).heap_bytes()
    };
    assert_eq!(with_data(1_000), with_data(1_000_000));

    let mut more = r.objects[0].clone();
    more.origin = Origin::Compressed((13, 0));
    more.kind = ObjectKind::Other("/SomeLongTypeName".into());
    assert!(more.heap_bytes() > r.objects[0].heap_bytes());
}

// ---- robustness ----

/// xorshift64: the tests' deterministic generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn ten_thousand_fuzzed_buffers_never_panic() {
    let mut rng = Rng(0xca7_5eed);
    let mut sources = vec![
        fixtures::golden_pdf(),
        fixtures::golden_pdf_objstm(),
        fixtures::with_stream_containing_keywords(),
        fixtures::with_wrong_length(LengthKind::Missing),
        fixtures::with_eol_style(EolStyle::Cr),
        fixtures::with_no_endobj(),
        fixtures::with_truncated_later_duplicate(),
    ];
    sources.push((0..4096).map(|_| rng.next() as u8).collect());
    const SPICE: [&[u8]; 16] = [
        b" 1 0 obj ",
        b"\n2 0 obj\n",
        b"endobj",
        b"\nendstream\n",
        b"endstream",
        b"stream\r",
        b"stream\n",
        b"<<",
        b">>",
        b"(",
        b"%%EOF",
        b"xref",
        b"trailer",
        b"startxref 12",
        b"/Length 7 ",
        b"/Length 99999999999999999999 ",
    ];
    for case in 0..10_000 {
        let src = &sources[rng.below(sources.len())];
        let start = rng.below(src.len());
        let len = rng.below(4096.min(src.len() - start) + 1);
        let mut buf = src[start..start + len].to_vec();
        for _ in 0..rng.below(8) {
            let at = rng.below(buf.len() + 1);
            if rng.next().is_multiple_of(3) {
                let spice = SPICE[rng.below(SPICE.len())];
                buf.splice(at..at, spice.iter().copied());
            } else if at < buf.len() {
                buf[at] = rng.next() as u8;
            }
        }
        let r = carved(&buf);
        let n = buf.len() as u64;
        let mut prev_end = 0;
        for o in &r.objects {
            assert!(o.span.start >= prev_end, "case {case}: objects overlap");
            assert!(o.span.start < o.span.end && o.span.end <= n, "case {case}");
            if let Body::Stream { data, .. } = &o.body {
                assert!(data.start <= data.end, "case {case}");
                assert!(
                    o.span.start <= data.start && data.end <= o.span.end,
                    "case {case}"
                );
            }
            prev_end = o.span.end;
        }
        for s in r.xref_spans.iter().chain(&r.trailer_spans) {
            assert!(s.start <= s.end && s.end <= n, "case {case}");
        }
        assert!(r.eof_markers.iter().all(|&at| at < n));
        assert!(r.startxref.iter().all(|&(at, _)| at < n));
    }
}
