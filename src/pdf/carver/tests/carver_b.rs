//! T-07 acceptance: the inflate probe and its end guard, deferred indirect
//! lengths, object-stream expansion, container-offset precedence (D-031),
//! xref-stream harvesting and the gap sweep.

use super::*;
use crate::pdf::fixtures::CopyOrder;

fn zlib(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

fn stored_zlib(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 0)
}

/// `num 0 obj`, `dict`, `stream`, LF, `data`, then `tail` (the bytes from
/// the end of the data on, e.g. `\nendstream\nendobj\n`).
fn stream_object(num: u32, dict: &str, data: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut out = format!("{num} 0 obj\n{dict}\nstream\n").into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(tail);
    out
}

fn winner_dict(r: &CarveReport, id: ObjId) -> &Dictionary {
    match &r
        .winner(id)
        .unwrap_or_else(|| panic!("no copy of {id:?}"))
        .body
    {
        Body::Dict(d) => d,
        other => panic!("{id:?} is not a dictionary: {other:?}"),
    }
}

fn media_box(d: &Dictionary) -> Vec<i64> {
    d.get(b"MediaBox")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|o| o.as_i64().unwrap())
        .collect()
}

fn container_notes(r: &CarveReport, num: u32) -> Vec<CarveNote> {
    r.objects
        .iter()
        .find(|o| o.declared_id == (num, 0) && o.origin == Origin::TopLevel)
        .unwrap_or_else(|| panic!("no container {num}"))
        .notes
        .clone()
}

fn compressed_ids(r: &CarveReport) -> Vec<ObjId> {
    r.objects
        .iter()
        .filter(|o| matches!(o.origin, Origin::Compressed(_)))
        .map(|o| o.declared_id)
        .collect()
}

fn unexplained(r: &CarveReport) -> Vec<ByteSpan> {
    r.notes
        .iter()
        .filter_map(|n| match n {
            CarveNote::UnexplainedSpan { span } => Some(*span),
            _ => None,
        })
        .collect()
}

// ---- rule 1: the inflate probe and its end guard ----

/// Data whose stored-deflate bytes hold a framed `endstream`, so rung (b)
/// sees two candidates and cannot choose.
fn content_with_a_framed_endstream() -> Vec<u8> {
    b"BT /F1 12 Tf (x) Tj ET\n% \nendstream\nendobj\n trailing comment\n".repeat(3)
}

#[test]
fn a_wrong_length_on_flate_data_is_resolved_by_the_probe() {
    let data = stored_zlib(&content_with_a_framed_endstream());
    for declared in [data.len() - 9, data.len() + 9] {
        let dict = format!("<< /Length {declared} /Filter /FlateDecode >>");
        let mut buf = b"%PDF-1.7\n".to_vec();
        buf.extend(stream_object(1, &dict, &data, b"\nendstream\nendobj\n"));
        buf.extend_from_slice(b"2 0 obj\n<< /Type /Catalog >>\nendobj\n");
        let r = carved(&buf);
        assert_eq!(ids(&r), [(1, 0), (2, 0)]);
        let o = object(&r, 1);
        let (got, source) = stream(o);
        assert_eq!(&buf[got], &data[..]);
        assert_eq!(source, LengthSource::InflateProbe);
        assert!(has_note(o, |n| matches!(
            n,
            CarveNote::EndstreamAmbiguous { .. }
        )));
        assert!(has_note(o, |n| matches!(
            n,
            CarveNote::LengthMismatch { .. }
        )));
        assert_eq!(r.stats.rungs.inflate_probe, 1);
        assert_eq!(r.stats.rungs.last_endstream, 0);
        assert!(buf[..o.span.end as usize].ends_with(b"endstream\nendobj"));
    }
}

#[test]
fn a_probe_without_endstream_right_after_it_is_not_trusted() {
    let data = stored_zlib(&content_with_a_framed_endstream());
    // Three bytes of junk between the deflate end and the EOL: past the
    // two-byte slack.
    let mut tail = b"XYZ".to_vec();
    tail.extend_from_slice(b"\nendstream\nendobj\n");
    let mut buf = b"%PDF-1.7\n".to_vec();
    buf.extend(stream_object(1, "<< /Filter /FlateDecode >>", &data, &tail));
    let r = carved(&buf);
    let o = object(&r, 1);
    let (got, source) = stream(o);
    assert_eq!(source, LengthSource::ScannedEndstream);
    assert_eq!(&buf[got.clone()], [&data[..], b"XYZ"].concat());
    let end = (got.start + data.len()) as u64;
    assert!(has_note(o, |n| *n == CarveNote::InflateProbeUntrusted { end }));
    assert_eq!(r.stats.rungs.inflate_probe, 0);
    assert_eq!(r.stats.rungs.last_endstream, 1);
}

#[test]
fn the_slack_is_two_bytes_of_whitespace() {
    let data = stored_zlib(&content_with_a_framed_endstream());
    for (gap, trusted) in [
        (&b""[..], true),
        (b"\n", true),
        (b"\r\n", true),
        (b" \n", true),
        (b"  \n", false),
        (b"x\n", false),
    ] {
        let mut tail = gap.to_vec();
        tail.extend_from_slice(b"endstream\nendobj\n");
        let buf = stream_object(1, "<< /Filter /FlateDecode >>", &data, &tail);
        let r = carved(&buf);
        let (got, source) = stream(&r.objects[0]);
        if trusted {
            assert_eq!(source, LengthSource::InflateProbe, "{gap:?}");
            assert_eq!(&buf[got], &data[..], "{gap:?}");
        } else {
            assert_ne!(source, LengthSource::InflateProbe, "{gap:?}");
        }
    }
}

#[test]
fn a_probe_ending_at_eof_is_trusted() {
    // C10: the file ends right after the deflate data (or one EOL later).
    let data = zlib(&content_with_a_framed_endstream());
    for tail in [&b""[..], b"\n", b"\r\n"] {
        let buf = stream_object(1, "<< /Filter /FlateDecode /Length 1 >>", &data, tail);
        let r = carved(&buf);
        let o = &r.objects[0];
        let (got, source) = stream(o);
        assert_eq!(source, LengthSource::InflateProbe, "{tail:?}");
        assert_eq!(&buf[got], &data[..]);
        assert!(has_note(o, |n| *n == CarveNote::NoEndobj));
        assert_eq!(o.span.end as usize, buf.len());
    }
}

#[test]
fn unfiltered_zlib_looking_data_is_probed_and_other_data_is_not() {
    let data = zlib(b"q 1 0 0 1 0 0 cm Q\n");
    assert_eq!(data[0], 0x78);
    let tail = b"\nendstream\n";
    // No `endobj` after `endstream`: rung (b) has no framed candidate.
    let buf = stream_object(1, "<< >>", &data, tail);
    assert_eq!(
        stream(&carved(&buf).objects[0]).1,
        LengthSource::InflateProbe
    );
    // A chain that does not start with Flate is not probed.
    let buf = stream_object(
        1,
        "<< /Filter [/ASCIIHexDecode /FlateDecode] >>",
        &data,
        tail,
    );
    assert_eq!(
        stream(&carved(&buf).objects[0]).1,
        LengthSource::ScannedEndstream
    );
}

#[test]
fn a_cut_flate_stream_still_clamps_at_eof() {
    let golden = fixtures::golden_pdf();
    let damaged = fixtures::corrupt(CorruptionClass::C10Truncated, &golden, 0);
    let r = carved(&damaged);
    let last = r.objects.last().unwrap();
    assert_eq!(stream(last).1, LengthSource::TruncatedAtEof);
    assert_eq!(stream(last).0.end, damaged.len());
    assert_eq!(r.stats.rungs.inflate_probe, 0);
    assert_eq!(r.stats.rungs.eof, 1);
    assert!(r.orphans.is_empty());
}

#[test]
fn the_probe_never_runs_past_the_next_header() {
    // The deflate data is whole but a header stands inside it at a line
    // start: the probe sees only the bytes before that header and fails, so
    // its extent cannot hold a foreign header.
    let inner = b"(\n2 0 obj\n) Tj\n".repeat(4);
    let data = stored_zlib(&inner);
    let buf = stream_object(1, "<< /Filter /FlateDecode >>", &data, b"\nendstream\n");
    let r = carved(&buf);
    let (got, source) = stream(&r.objects[0]);
    assert_eq!(source, LengthSource::TruncatedAtEof);
    assert!(got.end < buf.len() - 11);
    assert_eq!(r.stats.rungs.inflate_probe, 0);
}

// ---- rule 2: deferred indirect lengths ----

#[test]
fn an_indirect_length_is_resolved_in_pass_2() {
    let want = golden_stream(11);
    let buf = fixtures::with_wrong_length(LengthKind::Indirect);
    let r = carved(&buf);
    let o = object(&r, 11);
    let (data, source) = stream(o);
    assert_eq!(&buf[data], &want[..]);
    assert_eq!(source, LengthSource::DeclaredIndirect((13, 0)));
    assert_eq!(r.stats.rungs.deferred_length, 1);
    assert_eq!(r.stats.rungs.unique_endstream, 0);
    assert_eq!(r.stats.rungs.declared, 4);
    assert!(o.notes.is_empty(), "{:?}", o.notes);
}

#[test]
fn a_forward_indirect_length_removes_the_phantoms_pass_1_made() {
    // The data holds a framed `endstream` and a header at a line start.
    // Pass 1 cuts the stream at the first and carves a phantom object 9;
    // pass 2 knows the length from object 3, which comes later.
    let data = b"AAA\nendstream\nendobj\n9 0 obj\n<< /Phantom true >>\nendobj\nBBB";
    let mut buf = b"%PDF-1.4\n".to_vec();
    buf.extend(stream_object(
        1,
        "<< /Length 3 0 R >>",
        data,
        b"\nendstream\nendobj\n",
    ));
    buf.extend(format!("3 0 obj\n{}\nendobj\n", data.len()).into_bytes());
    let r = carved(&buf);
    assert_eq!(ids(&r), [(1, 0), (3, 0)]);
    let (got, source) = stream(&r.objects[0]);
    assert_eq!(&buf[got], &data[..]);
    assert_eq!(source, LengthSource::DeclaredIndirect((3, 0)));
    assert!(unexplained(&r).is_empty(), "{:?}", r.notes);
}

#[test]
fn an_indirect_length_that_does_not_fit_or_resolve_is_noted() {
    let data = b"abc";
    for (len_obj, note) in [
        (
            "3 0 obj\n7\nendobj\n",
            CarveNote::LengthMismatch { declared: 7 },
        ),
        (
            "3 0 obj\n/NotANumber\nendobj\n",
            CarveNote::LengthUnresolved((3, 0)),
        ),
        ("", CarveNote::LengthUnresolved((3, 0))),
    ] {
        let mut buf = stream_object(1, "<< /Length 3 0 R >>", data, b"\nendstream\nendobj\n");
        buf.extend_from_slice(len_obj.as_bytes());
        let r = carved(&buf);
        let o = object(&r, 1);
        let (got, source) = stream(o);
        assert_eq!(&buf[got], data);
        assert_eq!(source, LengthSource::ScannedEndstream, "{len_obj:?}");
        assert!(o.notes.contains(&note), "{len_obj:?}: {:?}", o.notes);
    }
}

// ---- rules 3 and 4: object streams and container-offset precedence ----

#[test]
fn the_objstm_golden_yields_every_packed_object() {
    let buf = fixtures::golden_pdf_objstm();
    let r = carved(&buf);
    let classic = carved(&fixtures::golden_pdf());
    use ObjectKind::*;
    let kinds = [Catalog, Pages, Page, Page, Font, Font, FontDescriptor];
    for (num, kind) in (1..=7).zip(kinds) {
        let copies: Vec<&CarvedObject> = r.copies((num, 0)).collect();
        assert_eq!(copies.len(), 1, "{num}");
        let o = copies[0];
        assert_eq!(o.origin, Origin::Compressed((13, 0)), "{num}");
        assert_eq!(o.kind, kind, "{num}");
        assert_eq!(o.body, object(&classic, num).body, "{num}");
        assert_eq!(o.span, object(&r, 13).span, "the container's span");
    }
    // The pages are reachable only through the object stream.
    for page in [3, 4] {
        assert!(memmem::find(&buf, format!("\n{page} 0 obj").as_bytes()).is_none());
        assert_eq!(r.winner((page, 0)).unwrap().kind, Page);
    }
    // Packed objects follow their container in the list.
    let order: Vec<ObjId> = ids(&r);
    let at13 = order.iter().position(|&id| id == (13, 0)).unwrap();
    assert_eq!(
        &order[at13 + 1..at13 + 8],
        &(1..=7).map(|n| (n, 0)).collect::<Vec<_>>()[..]
    );
    assert_eq!(object(&r, 13).kind, ObjStm);
    assert_eq!(object(&r, 14).kind, XRefStream);
    assert!(
        object(&r, 13).notes.is_empty(),
        "{:?}",
        object(&r, 13).notes
    );
    assert!(r.orphans.is_empty());
    assert!(r.notes.is_empty(), "{:?}", r.notes);
}

#[test]
fn both_copy_orders_pick_the_greater_container_offset() {
    for order in [CopyOrder::PlainFirst, CopyOrder::ObjStmFirst] {
        let buf = fixtures::with_objstm_and_plain_copy(order);
        let r = carved(&buf);
        let copies: Vec<&CarvedObject> = r.copies((4, 0)).collect();
        assert_eq!(copies.len(), 2, "{order:?}");
        assert!(copies[0].span.start < copies[1].span.start, "{order:?}");
        let winner = r.winner((4, 0)).unwrap();
        assert!(std::ptr::eq(winner, copies[1]));
        // The later copy says A4, whichever kind of copy it is.
        assert_eq!(
            media_box(winner_dict(&r, (4, 0))),
            [0, 0, 595, 842],
            "{order:?}"
        );
        let want = match order {
            CopyOrder::PlainFirst => Origin::Compressed((13, 0)),
            CopyOrder::ObjStmFirst => Origin::TopLevel,
        };
        assert_eq!(winner.origin, want, "{order:?}");
        // Every other number has one copy.
        for num in [1, 2, 3, 5, 6, 7] {
            assert_eq!(r.copies((num, 0)).count(), 1, "{order:?} {num}");
        }
    }
}

#[test]
fn a_recursive_objstm_terminates() {
    let buf = fixtures::with_recursive_objstm();
    let r = carved(&buf);
    let mut packed = compressed_ids(&r);
    packed.sort_unstable();
    assert_eq!(packed, (1..=7).map(|n| (n, 0)).collect::<Vec<_>>());
    // Container 13 lists itself third: that entry is refused.
    assert!(
        container_notes(&r, 13).contains(&CarveNote::ObjStmEntryBad {
            index: 2,
            fault: EntryFault::SelfReference
        })
    );
    assert_eq!(r.copies((13, 0)).count(), 1);
    assert!(container_notes(&r, 15).is_empty());
    for (num, container) in [(1, 13), (2, 13), (3, 15), (7, 15)] {
        assert_eq!(
            r.winner((num, 0)).unwrap().origin,
            Origin::Compressed((container, 0))
        );
    }
}

#[test]
fn bad_objstm_offsets_skip_those_entries_only() {
    let buf = fixtures::with_bad_objstm_offsets();
    let r = carved(&buf);
    let notes = container_notes(&r, 13);
    let bad: Vec<(u32, EntryFault)> = notes
        .iter()
        .filter_map(|n| match n {
            CarveNote::ObjStmEntryBad { index, fault } => Some((*index, *fault)),
            _ => None,
        })
        .collect();
    assert_eq!(
        bad,
        [
            (2, EntryFault::NonMonotonic),
            (3, EntryFault::NonMonotonic),
            (6, EntryFault::OutOfRange),
        ]
    );
    assert_eq!(compressed_ids(&r), [(1, 0), (2, 0), (5, 0), (6, 0)]);
    let classic = carved(&fixtures::golden_pdf());
    for num in [1, 2, 5, 6] {
        assert_eq!(r.winner((num, 0)).unwrap().body, object(&classic, num).body);
    }
}

/// A one-container file: object stream 1 with `dict_extra` in its
/// dictionary and `data` (the header and objects) as its decoded data.
fn objstm_file(n: i64, first: usize, data: &[u8], encode: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    let enc = encode(data);
    let dict = format!(
        "<< /Type /ObjStm /N {n} /First {first} /Length {} /Filter /FlateDecode >>",
        enc.len()
    );
    stream_object(1, &dict, &enc, b"\nendstream\nendobj\n")
}

#[test]
fn an_objstm_header_is_checked_before_expansion() {
    let header = b"2 0 3 5 ";
    let data = [&header[..], b"<<>>\n(three)\n"].concat();
    let first = header.len();

    let r = carved(&objstm_file(2, first, &data, zlib));
    assert_eq!(compressed_ids(&r), [(2, 0), (3, 0)]);
    assert_eq!(
        r.winner((3, 0)).unwrap().body,
        Body::Primitive(Object::String(
            b"three".to_vec(),
            lopdf::StringFormat::Literal
        ))
    );

    for (n, first, fault) in [
        (1_000_001, first, ObjStmFault::BadCount),
        (-1, first, ObjStmFault::BadCount),
        (2, data.len() + 1, ObjStmFault::BadFirst),
    ] {
        let r = carved(&objstm_file(n, first, &data, zlib));
        assert!(compressed_ids(&r).is_empty(), "{n} {first}");
        assert_eq!(
            container_notes(&r, 1),
            [CarveNote::ObjStmRejected(fault)],
            "{n} {first}"
        );
    }

    // An index shorter than /N: the pairs that are there are used.
    let r = carved(&objstm_file(3, first, &data, zlib));
    assert_eq!(compressed_ids(&r), [(2, 0), (3, 0)]);
    assert!(container_notes(&r, 1).contains(&CarveNote::ObjStmEntryBad {
        index: 2,
        fault: EntryFault::Missing
    }));
}

#[test]
fn a_damaged_objstm_keeps_what_inflates() {
    let header = b"2 0 3 5 ";
    let mut data = [&header[..], b"<<>>\n(three)\n"].concat();
    for i in 0..400u32 {
        data.extend(format!("{:05} ", i.wrapping_mul(2_654_435_761) % 99_991).into_bytes());
    }
    // Cut the deflate data short: the prefix still holds both objects.
    let cut = |d: &[u8]| {
        let z = zlib(d);
        z[..z.len() - 40].to_vec()
    };
    let r = carved(&objstm_file(2, header.len(), &data, cut));
    assert_eq!(compressed_ids(&r), [(2, 0), (3, 0)]);
    assert!(container_notes(&r, 1).contains(&CarveNote::ObjStmPartial));
}

#[test]
fn packed_objects_count_toward_the_object_cap() {
    let buf = fixtures::golden_pdf_objstm();
    let r = carved_with(
        &buf,
        Caps {
            landmarks: MAX_LANDMARKS,
            objects: 9,
        },
    );
    assert_eq!(r.objects.len(), 9);
    assert!(r.notes.contains(&CarveNote::CapHit(Cap::Objects)));
}

// ---- rule 5: xref streams ----

#[test]
fn xref_streams_are_decoded_and_their_trailer_keys_harvested() {
    let buf = fixtures::golden_pdf_objstm();
    let r = carved(&buf);
    assert_eq!(r.xref_streams.len(), 1);
    let x = &r.xref_streams[0];
    assert_eq!(x.id, (14, 0));
    assert_eq!(x.span, object(&r, 14).span);
    assert_eq!(x.trailer.root, Some(Object::Reference((1, 0))));
    assert!(matches!(x.trailer.id, Some(Object::Array(ref a)) if a.len() == 2));
    assert_eq!(x.trailer.info, None);
    assert_eq!(x.trailer.encrypt, None);
    assert_eq!(x.rows.len(), 15);
    for (num, row) in x.rows.iter().enumerate() {
        assert_eq!(row.num, num as u64);
        let want = match num {
            0 => [0, 0, 0xFFFF],
            1..=7 => [2, 13, num as u64 - 1],
            _ => [1, object(&r, num as u32).span.start, 0],
        };
        assert_eq!(row.fields, want, "row {num}");
    }
    // The classic golden has none.
    assert!(carved(&fixtures::golden_pdf()).xref_streams.is_empty());
}

#[test]
fn an_xref_stream_honours_index_and_a_zero_type_width() {
    // /W [0 2 1]: no type field, so every row is type 1.
    let rows = [0u8, 9, 0, 0, 17, 1];
    let data = zlib(&rows);
    let dict = format!(
        "<< /Type /XRef /Size 12 /Index [10 2] /W [0 2 1] /Encrypt 5 0 R /Info 6 0 R /Length {} /Filter /FlateDecode >>",
        data.len()
    );
    let buf = stream_object(1, &dict, &data, b"\nendstream\nendobj\n");
    let r = carved(&buf);
    let x = &r.xref_streams[0];
    assert_eq!(
        x.rows,
        [
            XrefRow {
                num: 10,
                fields: [1, 9, 0]
            },
            XrefRow {
                num: 11,
                fields: [1, 17, 1]
            }
        ]
    );
    assert_eq!(x.trailer.encrypt, Some(Object::Reference((5, 0))));
    assert_eq!(x.trailer.info, Some(Object::Reference((6, 0))));
    assert_eq!(x.trailer.root, None);

    // Undecodable data still yields the keys, and a note.
    let dict = "<< /Type /XRef /Size 1 /W [1 2 1] /Root 1 0 R /Length 4 /Filter /FlateDecode >>";
    let buf = stream_object(1, dict, b"junk", b"\nendstream\nendobj\n");
    let r = carved(&buf);
    let x = &r.xref_streams[0];
    assert!(x.rows.is_empty());
    assert_eq!(x.trailer.root, Some(Object::Reference((1, 0))));
    assert!(has_note(&r.objects[0], |n| *n == CarveNote::XrefStreamUndecodable));
}

// ---- rule 6: the gap sweep ----

#[test]
fn c5_yields_one_orphan_of_the_right_kind() {
    let golden = fixtures::golden_pdf();
    let full = carved(&golden);
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..16 {
        let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, seed);
        let r = carved(&damaged);
        let lost = golden_ids()
            .into_iter()
            .find(|id| !ids(&r).contains(id))
            .expect("one header is gone");
        seen.insert(lost);
        assert_eq!(r.orphans.len(), 1, "seed {seed}: {:?}", r.orphans);
        assert!(unexplained(&r).is_empty(), "seed {seed}: {:?}", r.notes);
        let before = object(&full, lost.0);
        let orphan = &r.orphans[0];
        assert_eq!(orphan.kind(), &before.kind, "seed {seed}");
        match (orphan, &before.body) {
            (Orphan::Dict { dict, .. }, Body::Dict(want)) => assert_eq!(dict, want),
            (
                Orphan::Stream {
                    dict,
                    data,
                    length_source,
                    ..
                },
                Body::Stream { dict: want, .. },
            ) => {
                assert_eq!(dict, want);
                assert_eq!(*length_source, LengthSource::Declared);
                assert_eq!(&damaged[range(*data)], &golden[stream(before).0]);
            }
            (o, b) => panic!("seed {seed}: orphan {o:?} for {b:?}"),
        }
        let span = &damaged[range(orphan.span())];
        assert!(
            span.starts_with(b"<<") && span.ends_with(b"endobj"),
            "seed {seed}"
        );
    }
    assert!(seen.len() > 4, "the seeds cover several objects: {seen:?}");
}

#[test]
fn goldens_leave_no_gap() {
    for buf in [
        fixtures::golden_pdf(),
        fixtures::golden_pdf_objstm(),
        fixtures::golden_pdf_signed(),
        fixtures::with_eol_style(EolStyle::CrLf),
    ] {
        let r = carved(&buf);
        assert!(r.orphans.is_empty());
        assert!(unexplained(&r).is_empty(), "{:?}", r.notes);
    }
}

#[test]
fn gaps_of_junk_are_unexplained_spans() {
    let a = "1 0 obj\n1\nendobj\n";
    let b = "2 0 obj\n2\nendobj\n";
    // 16 non-whitespace bytes, spread out: reported.
    let junk = "  abcd efgh\nijkl  mnop \n";
    let buf = format!("{a}{junk}{b}");
    let r = carved(buf.as_bytes());
    let start = a.len() as u64;
    assert_eq!(
        unexplained(&r),
        [ByteSpan {
            start: start + 2,
            end: start + junk.len() as u64
        }]
    );
    // Fifteen, or comments of any length: nothing.
    for junk in [
        "abcdefghijklmno\n",
        "%a comment that is much longer than sixteen bytes\n",
    ] {
        let r = carved(format!("{a}{junk}{b}").as_bytes());
        assert!(unexplained(&r).is_empty(), "{junk:?}: {:?}", r.notes);
        assert!(r.orphans.is_empty());
    }
}

#[test]
fn a_gap_with_a_dictless_stream_is_an_orphan_stream() {
    let data = b"BT /F1 12 Tf (orphan) Tj ET";
    let mut buf = b"1 0 obj\n1\nendobj\nnot a dictionary stream\n".to_vec();
    let data_start = buf.len();
    buf.extend_from_slice(data);
    buf.extend_from_slice(b"\nendstream\nendobj\n2 0 obj\n2\nendobj\n");
    let r = carved(&buf);
    assert_eq!(r.orphans.len(), 1, "{:?}", r.notes);
    let Orphan::Stream {
        span,
        dict,
        data: got,
        length_source,
        kind,
    } = &r.orphans[0]
    else {
        panic!("{:?}", r.orphans[0]);
    };
    assert!(dict.is_empty());
    assert_eq!(range(*got), data_start..data_start + data.len());
    assert_eq!(*length_source, LengthSource::ScannedEndstream);
    assert_eq!(*kind, ObjectKind::ContentStream);
    assert!(damaged_text(&buf, *span).starts_with("not a dictionary stream"));
    assert!(damaged_text(&buf, *span).ends_with("endobj"));
}

fn damaged_text(buf: &[u8], span: ByteSpan) -> String {
    String::from_utf8_lossy(&buf[range(span)]).into_owned()
}

#[test]
fn two_orphans_in_one_gap_are_both_found() {
    let buf = b"1 0 obj\n1\nendobj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n<< /Type /Font /Subtype /Type1 >>\nendobj\n3 0 obj\n3\nendobj\n";
    let r = carved(buf);
    let kinds: Vec<&ObjectKind> = r.orphans.iter().map(Orphan::kind).collect();
    assert_eq!(kinds, [&ObjectKind::Page, &ObjectKind::Font]);
    assert!(unexplained(&r).is_empty());
}

// ---- memory and robustness ----

#[test]
fn heap_bytes_counts_orphans_packed_objects_and_xref_rows() {
    let golden = fixtures::golden_pdf();
    let damaged = fixtures::corrupt(CorruptionClass::C5ObjectTagStripped, &golden, 0);
    let r = carved(&damaged);
    let without = CarveReport {
        orphans: Vec::new(),
        ..carved(&damaged)
    };
    assert!(r.heap_bytes() > without.heap_bytes());

    let r = carved(&fixtures::golden_pdf_objstm());
    let mut without = carved(&fixtures::golden_pdf_objstm());
    without.xref_streams = Vec::new();
    assert!(r.heap_bytes() > without.heap_bytes());
    let rows = (r.xref_streams[0].rows.capacity() * size_of::<XrefRow>()) as u64;
    assert!(r.heap_bytes() - without.heap_bytes() >= rows);
}

#[test]
fn carving_is_deterministic() {
    for buf in [
        fixtures::golden_pdf_objstm(),
        fixtures::with_recursive_objstm(),
        fixtures::corrupt(
            CorruptionClass::C5ObjectTagStripped,
            &fixtures::golden_pdf(),
            3,
        ),
        fixtures::corrupt(
            CorruptionClass::C9ZlibTampered,
            &fixtures::golden_pdf_objstm(),
            1,
        ),
    ] {
        let (a, b) = (carved(&buf), carved(&buf));
        assert_eq!(a.objects, b.objects);
        assert_eq!(a.orphans, b.orphans);
        assert_eq!(a.xref_streams, b.xref_streams);
        assert_eq!(a.notes, b.notes);
        assert_eq!(a.stats, b.stats);
    }
}
