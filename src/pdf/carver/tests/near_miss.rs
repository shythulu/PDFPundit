//! F-07 acceptance: the near-miss lex over what the carve left outside every
//! object (rule 7). It reads the gaps and unexplained spans, never a carved
//! object or orphan, and adds only `NearMissKeyword` notes.

use super::*;

/// The file-level `NearMissKeyword` offsets of a carve.
fn near_misses(r: &CarveReport) -> Vec<u64> {
    r.notes
        .iter()
        .filter_map(|n| match n {
            CarveNote::Lex(LexNote::NearMissKeyword { at }) => Some(*at),
            _ => None,
        })
        .collect()
}

/// Every object-level near-miss note of a carve.
fn object_near_misses(r: &CarveReport) -> Vec<CarveNote> {
    r.objects
        .iter()
        .flat_map(|o| o.notes.iter().copied())
        .filter(|n| {
            matches!(
                n,
                CarveNote::Lex(LexNote::NearMissKeyword { .. } | LexNote::NearMissNumber { .. })
            )
        })
        .collect()
}

fn at(buf: &[u8], needle: &[u8]) -> u64 {
    memmem::find(buf, needle).expect("needle") as u64
}

#[test]
fn a_damaged_header_keyword_in_a_gap_is_noted() {
    let buf = b"1 0 obj\n1\nendobj\n2 0 obk\n<< /Type /Page >>\nendobj\n3 0 obj\n3\nendobj\n";
    let r = carved(buf);
    assert_eq!(ids(&r), [(1, 0), (3, 0)]);
    assert_eq!(near_misses(&r), [at(buf, b"obk")]);
    assert!(object_near_misses(&r).is_empty(), "{:?}", r.objects);
}

#[test]
fn a_short_gap_is_read_too() {
    // Fewer than 16 solid bytes: the sweep does not read it, the lex does.
    let buf = b"1 0 obj\n1\nendobj\n2 0 ob]\n3 0 obj\n3\nendobj\n";
    let r = carved(buf);
    assert!(r.orphans.is_empty());
    assert_eq!(near_misses(&r), [at(buf, b"ob]")]);
}

#[test]
fn carved_objects_and_orphans_are_never_lexed() {
    // A damaged `endobj` inside object 1 (it ends at the next header), and a
    // dictionary-less orphan stream whose data holds a near-miss bareword.
    let mut buf = b"1 0 obj\n<< /A 1 >>\nendobk\n".to_vec();
    buf.extend_from_slice(b"2 0 obj\n2\nendobj\nnot a dictionary stream\n");
    buf.extend_from_slice(b"q endstreak Q obk\nendstream\nendobj\n3 0 obj\n3\nendobj\n");
    let r = carved(&buf);
    assert_eq!(ids(&r), [(1, 0), (2, 0), (3, 0)]);
    assert_eq!(r.orphans.len(), 1, "{:?}", r.notes);
    assert!(near_misses(&r).is_empty(), "{:?}", r.notes);
    assert!(object_near_misses(&r).is_empty(), "{:?}", r.objects);
}

#[test]
fn a_near_miss_after_an_orphan_in_the_same_gap_is_noted() {
    let buf = b"1 0 obj\n1\nendobj\n<< /Type /Font /Subtype /Type1 >>\nendobj\n4 0 obk\n3 0 obj\n3\nendobj\n";
    let r = carved(buf);
    assert_eq!(r.orphans.len(), 1, "{:?}", r.notes);
    assert_eq!(near_misses(&r), [at(buf, b"obk")]);
}

#[test]
fn only_near_miss_keywords_are_kept() {
    // `11l` is a near-miss number and `(` never closes: neither is noted.
    let buf = b"1 0 obj\n1\nendobj\n11l 0 obj\nendstreax \x01\x02 (open\n3 0 obj\n3\nendobj\n";
    let r = carved(buf);
    let lex: Vec<&CarveNote> = r
        .notes
        .iter()
        .filter(|n| matches!(n, CarveNote::Lex(_)))
        .collect();
    assert!(
        lex.iter()
            .all(|n| matches!(n, CarveNote::Lex(LexNote::NearMissKeyword { .. }))),
        "{lex:?}"
    );
    assert_eq!(near_misses(&r), [at(buf, b"endstreax")]);
}

#[test]
fn clean_files_and_builders_have_no_near_misses() {
    let mut inputs = vec![
        fixtures::golden_pdf(),
        fixtures::golden_pdf_objstm(),
        fixtures::golden_pdf_signed(),
        fixtures::with_junk_prefix(1_000),
        fixtures::with_stream_containing_keywords(),
        fixtures::with_no_endobj(),
        fixtures::with_duplicate_object(),
    ];
    for style in [
        EolStyle::Lf,
        EolStyle::CrLf,
        EolStyle::Cr,
        EolStyle::SpaceEol,
    ] {
        inputs.push(fixtures::with_eol_style(style));
    }
    for buf in inputs {
        let r = carved(&buf);
        assert!(near_misses(&r).is_empty(), "{:?}", r.notes);
    }
}

#[test]
fn the_near_miss_notes_are_capped() {
    let mut buf = b"1 0 obj\n1\nendobj\n".to_vec();
    buf.extend(b"obk ".repeat(MAX_NEAR_MISSES + 10));
    let r = carved(&buf);
    assert_eq!(near_misses(&r).len(), MAX_NEAR_MISSES);
    let caps = r
        .notes
        .iter()
        .filter(|n| **n == CarveNote::CapHit(Cap::NearMisses));
    assert_eq!(caps.count(), 1);
}

#[test]
fn the_near_miss_lex_polls_cancel() {
    let buf = b"1 0 obj\n1\nendobj\nobk\n2 0 obj\n2\nendobj\n";
    let mut report = carved(buf);
    report.notes.clear();
    let mut poll = Poll {
        cancel: &|| false,
        steps: 0,
    };
    let lm = landmarks::scan(buf, MAX_LANDMARKS, &mut poll).expect("not cancelled");
    let mut cancelled = Poll {
        cancel: &|| true,
        steps: 0,
    };
    assert!(matches!(
        gaps::near_misses(buf, &lm, &mut cancelled, &mut report),
        Err(Cancelled)
    ));
    assert!(report.notes.is_empty());
    assert!(gaps::near_misses(buf, &lm, &mut poll, &mut report).is_ok());
    assert_eq!(near_misses(&report), [at(buf, b"obk")]);
}

/// The C9 corruptor never touches a keyword (an `N G obj` header, `endobj`,
/// `stream`, `endstream`; T-03b, D-106): its outside-stream replacements are
/// array elements, dictionary values and bare numbers, all inside carved
/// objects. So no seed opens a gap, and rule 7, which reads gaps only, has
/// nothing to note on them (F-07's third acceptance line, reported as a
/// deviation).
#[test]
fn c9_corruptor_outside_stream_seeds_open_no_gap() {
    use crate::pdf::fixtures::ReplacementSite;
    let mut outside = 0;
    for golden in [fixtures::golden_pdf(), fixtures::golden_pdf_objstm()] {
        for seed in 0..100 {
            let (buf, log) =
                fixtures::corrupt_with_log(CorruptionClass::C9ZlibTampered, &golden, seed);
            if !log.iter().any(|r| {
                matches!(
                    r.site,
                    ReplacementSite::Array | ReplacementSite::DictValue | ReplacementSite::Number
                )
            }) {
                continue;
            }
            outside += 1;
            let r = carved(&buf);
            assert!(r.orphans.is_empty(), "seed {seed}");
            assert!(unexplained_spans(&r).is_empty(), "seed {seed}");
            assert!(near_misses(&r).is_empty(), "seed {seed}: {:?}", r.notes);
        }
    }
    assert!(outside > 50, "{outside} seeds hit outside stream data");
}

fn unexplained_spans(r: &CarveReport) -> Vec<ByteSpan> {
    r.notes
        .iter()
        .filter_map(|n| match n {
            CarveNote::UnexplainedSpan { span } => Some(*span),
            _ => None,
        })
        .collect()
}
