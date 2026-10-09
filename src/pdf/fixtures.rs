//! Programmatic PDF fixtures and corruptors (T-03a; T-03b adds the ObjStm
//! golden, the C9 corruptor and the adversarial builders).
//!
//! The goldens are built through [`crate::pdf::write`], read their glyph ids and
//! widths from the committed test font through `skrifa::raw` when they are
//! built, and are byte-identical on every run. Files the writer must not make
//! (object and xref streams, T-12a rule 3) or cannot make (duplicate numbers, a
//! missing `endobj`, odd EOLs, wrong lengths) are assembled byte by byte here.
//! [`corrupt`] reproduces REPDF's measured induction for each class (map #7):
//! it edits bytes only, never re-serialises, and is deterministic per seed.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use lopdf::xref::XrefEntry;
use lopdf::{Dictionary, Document, Object, Stream, StringFormat};
use sha2::{Digest, Sha256};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlineGlyphCollection, OutlinePen};
use skrifa::raw::{FontRef, TableProvider};
use skrifa::{GlyphId, MetadataProvider};

use crate::pdf::model::CorruptionClass;
use crate::pdf::write::Writer;

/// The test font: a 9,672-byte subset of Noto Sans Regular (OFL; recipe in
/// `tests/data/README.md`).
pub const TEST_FONT: &[u8] = include_bytes!("../../tests/data/NotoSans-Regular-subset.ttf");

/// Our own 8×8 baseline JPEG, the goldens' DCT image.
pub const TINY_JPEG: &[u8] = include_bytes!("../../tests/data/tiny.jpg");

/// The lines each golden page draws, top to bottom, one `Tj` per line.
pub const GOLDEN_TEXT: [&[&str]; 2] = [
    &[
        "PDFPundit golden page one.",
        "The quick brown fox jumps over the lazy dog.",
    ],
    &[
        "Le garçon a mangé la crème « au cœur ».",
        "El niño y el pingüino, l’été.",
    ],
];

/// The line [`outline_only_page`] draws as filled glyph outlines.
pub const OUTLINE_TEXT: &str = "Drawn as outlines, not as text.";

/// The line [`type3_only_page`] shows through its Type3 font.
pub const TYPE3_TEXT: &str = "Type three glyphs only.";

/// Where a C9 replacement landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReplacementSite {
    /// The data of a stream whose (first) filter is `/FlateDecode`.
    FlateBody,
    /// The data of any other stream: unfiltered, `/DCTDecode`, or a chain
    /// that does not start with Flate.
    OtherBody,
    /// An array element outside stream data (width arrays first, see
    /// [`corrupt_with_log`]).
    Array,
    /// A dictionary value outside stream data.
    DictValue,
    /// The value of an object that is a bare number.
    Number,
}

/// One byte C9 replaced: `pdf[at]` was `from` and is now `to` (`to != from`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Replacement {
    pub at: usize,
    pub from: u8,
    pub to: u8,
    pub site: ReplacementSite,
}

/// How [`with_wrong_length`] declares page 1's content-stream `/Length`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthKind {
    /// The true length.
    Correct,
    /// 16 bytes short of the data.
    TooShort,
    /// 16 bytes past the data.
    TooLong,
    /// `13 0 R`, a bare-number object written after the stream.
    Indirect,
    /// No `/Length` at all.
    Missing,
}

/// The bytes between `stream` and the data in [`with_eol_style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EolStyle {
    /// `\n` (the writer's own).
    Lf,
    /// `\r\n`.
    CrLf,
    /// A bare `\r`, which ISO 32000 does not allow.
    Cr,
    /// A space, then `\n`.
    SpaceEol,
}

/// Which copy of the doubled object comes first in [`with_objstm_and_plain_copy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyOrder {
    /// The top-level copy, then the object stream holding the other.
    PlainFirst,
    /// The object stream, then the top-level copy.
    ObjStmFirst,
}

/// The golden: two pages, one Type0/Identity-H font embedding [`TEST_FONT`]
/// (`/W`, `/CIDToGIDMap /Identity`, a hand-built `bfchar` `/ToUnicode`),
/// [`TINY_JPEG`] on page 1, one Flate content stream per page (each under
/// 512 bytes) and a classic xref table.
pub fn golden_pdf() -> Vec<u8> {
    let mut w = golden_writer(None);
    w.trailer((CATALOG, 0), golden_id_seed(), None);
    w.finish().expect("golden_pdf builds")
}

/// [`golden_pdf`] plus a signature field: the catalog's `/AcroForm` names a
/// widget on page 1 whose `/V` is a `/Type /Sig` dictionary with a dummy
/// `/Contents` and a `/ByteRange` that brackets that `/Contents` in the file.
pub fn golden_pdf_signed() -> Vec<u8> {
    let mut byte_range = [0i64; 4];
    // The byte range describes the file it sits in; its own digits can move the
    // offsets, so build until it is stable (two or three rounds).
    for _ in 0..8 {
        let mut w = golden_writer(Some(byte_range));
        w.trailer((CATALOG, 0), golden_id_seed(), None);
        let bytes = w.finish().expect("golden_pdf_signed builds");
        let contents = signature_contents_span(&bytes);
        let len = bytes.len() as i64;
        let next = [
            0,
            contents.start as i64,
            contents.end as i64,
            len - contents.end as i64,
        ];
        if next == byte_range {
            return bytes;
        }
        byte_range = next;
    }
    panic!("the signed golden's /ByteRange did not settle");
}

/// [`golden_pdf`]'s objects in a PDF 1.5 file: the seven dictionaries (catalog,
/// page tree, pages, fonts, descriptor) packed in object stream 13, the five
/// streams at top level, and cross-reference stream 14 (`/W [1 4 2]`, no
/// classic table, no `trailer`). Both containers are assembled by hand here,
/// never through [`crate::pdf::write`].
pub fn golden_pdf_objstm() -> Vec<u8> {
    let objects = golden_objects(None);
    let mut h = Hand::with_streams(&objects);
    let stm = ObjStm::pack(packable(&objects));
    h.object(OBJSTM, &stm.stream(vec![]));
    h.xref_stream(XREF_STREAM, &stm.packed(OBJSTM))
}

/// `pdf` with one corruption of `class` applied, reproducing REPDF's measured
/// induction:
/// - C1: the first 12 bytes overwritten with seeded random bytes;
/// - C2: the classic xref table deleted (`xref` up to `trailer`);
/// - C3: `trailer` through `%%EOF` deleted;
/// - C4: a seeded 45–128 byte span deleted that covers the page-tree node from
///   its `/Pages` type through its `/Count` value;
/// - C5: one seeded `N 0 obj` header and its EOL deleted (exactly 8 bytes);
/// - C6: the `/Font` entry of a seeded page's `/Resources` deleted;
/// - C7: every `/FontFile2` stream's data overwritten with 0x20 in place;
/// - C8: C7, and every `/ToUnicode` stream's data overwritten the same way;
/// - C9: single bytes replaced in place, as [`corrupt_with_log`] describes;
/// - C10: truncated to 70.0% of its length (rounded down).
///
/// The same `(class, pdf, seed)` always gives the same bytes. `pdf` is a
/// fixture: a file such as [`golden_pdf`] (C2 and C3 need a classic table);
/// this panics if it lacks the structure the class edits.
pub fn corrupt(class: CorruptionClass, pdf: &[u8], seed: u64) -> Vec<u8> {
    corrupt_with_log(class, pdf, seed).0
}

/// [`corrupt`], with every byte C9 replaced. The log is empty for every other
/// class, and sorted by offset for C9.
///
/// C9 follows REPDF's measured distribution (OBS-0005, OBS-0302: 2,162 bytes
/// over 100 files, 1,445 of them in 1,445 of the 3,429 Flate streams):
/// - about 42% of the Flate streams (1,445 / 3,429, at least one) get exactly
///   one replaced byte each;
/// - non-Flate stream bodies (unfiltered, DCT, chains not starting with Flate)
///   get 304 bytes per 1,445 Flate hits;
/// - about one replacement per three stream hits lands outside stream data,
///   spread over array elements, dictionary values and bare-number objects;
///   never an `N G obj` header, `endobj`, `stream`/`endstream`, a dictionary
///   key, the `/Root`, `/Pages`, `/Kids`, `/Count` or `/Type` values, nor a
///   classic xref table or trailer (they are not objects);
/// - "width arrays first": an array hit lands in a `/W` or `/Widths` array,
///   and in any other array (`/MediaBox`, `/BBox`, ...) only when the file has
///   no width-array byte left to hit;
/// - each new byte is one of the 255 values other than the old one.
///
/// Fractional counts round up with the leftover probability, so the expected
/// counts are the measured ratios. Object and xref streams count as Flate
/// streams like any other, as they do in REPDF (eng-r2-fr1: 69 `/ObjStm` and
/// 27 `/XRef` among the 1,445 damaged streams), and their dictionaries are
/// dictionary values like any other; an xref stream's `/W` holds field
/// widths, not glyph widths, so it is an ordinary array. Streams are told
/// apart by their `/Filter`. Xref offsets are read from the `%PDF-` header, as
/// readers do, so a junk prefix moves nothing.
///
/// A chain that does not start with Flate (`[/ASCII85Decode /FlateDecode]`)
/// is an `OtherBody`: C9 replaces one raw (ASCII85) byte, which changes a
/// whole 4-byte group of the zlib data or leaves it undecodable, and never
/// gives a single-byte hit in the Flate stage. A test that needs one (T-08's
/// filter-domain case) builds it itself: decode the earlier filters, replace
/// one Flate-stage byte, re-encode.
pub fn corrupt_with_log(
    class: CorruptionClass,
    pdf: &[u8],
    seed: u64,
) -> (Vec<u8>, Vec<Replacement>) {
    let mut rng = SplitMix64(seed);
    let out = match class {
        CorruptionClass::C1Header => {
            let mut out = pdf.to_vec();
            for b in out.iter_mut().take(12) {
                *b = rng.next() as u8;
            }
            out
        }
        CorruptionClass::C2XrefMissing => {
            let start = startxref(pdf);
            assert!(
                pdf[start..].starts_with(b"xref"),
                "startxref is not a table"
            );
            let end = start + find(&pdf[start..], b"trailer").expect("trailer after xref");
            delete(pdf, start..end)
        }
        CorruptionClass::C3TrailerDamaged => {
            let start = rfind(pdf, b"trailer").expect("trailer keyword");
            let end = rfind(pdf, b"%%EOF").expect("%%EOF marker") + 5;
            delete(pdf, start..end)
        }
        CorruptionClass::C4PageTreeBroken => delete(pdf, page_tree_span(pdf, &mut rng)),
        CorruptionClass::C5ObjectTagStripped => delete(pdf, object_header(pdf, &mut rng)),
        CorruptionClass::C6FontMapLost => delete(pdf, page_font_entry(pdf, &mut rng)),
        CorruptionClass::C7FontStreamDeleted => blank(pdf, &font_streams(pdf, false)),
        CorruptionClass::C8FontResourcesDeleted => blank(pdf, &font_streams(pdf, true)),
        CorruptionClass::C9ZlibTampered => {
            let log = c9_replacements(pdf, &mut rng);
            let mut out = pdf.to_vec();
            for rep in &log {
                out[rep.at] = rep.to;
            }
            return (out, log);
        }
        CorruptionClass::C10Truncated => pdf[..pdf.len() * 7 / 10].to_vec(),
    };
    (out, Vec::new())
}

// ---------------------------------------------------------------------------
// The adversarial builders (TD §20)

/// [`golden_pdf`] with page 1's content stream extended by a marked-content
/// point whose string holds `endstream`, `endobj` and `1 0 obj`, each at a line
/// start. The stream is Flate with stored (uncompressed) blocks, so the
/// keywords sit in the raw stream data; only `/Length` or an inflate probe
/// finds the true end.
pub fn with_stream_containing_keywords() -> Vec<u8> {
    let facts = font_facts();
    golden_with(|objects| {
        let mut bytes = content(&facts, 0);
        bytes.extend_from_slice(b"/Note <</Data (\nendstream\nendobj\n1 0 obj\n)>> DP\n");
        let stored = miniz_oxide::deflate::compress_to_vec_zlib(&bytes, 0);
        objects.insert(CONTENTS[0], stream(flate_dict(), stored));
    })
}

/// The golden written by hand with page 1's content stream declaring its
/// `/Length` as `kind` says; the data is the golden's.
pub fn with_wrong_length(kind: LengthKind) -> Vec<u8> {
    hand_classic(|h, id, object| {
        let Object::Stream(s) = object else {
            return h.object(id, object);
        };
        if id != CONTENTS[0] {
            return h.object(id, object);
        }
        let mut dict = s.dict.clone();
        let len = s.content.len() as i64;
        match kind {
            LengthKind::Correct => {}
            LengthKind::TooShort => dict.set("Length", len - 16),
            LengthKind::TooLong => dict.set("Length", len + 16),
            LengthKind::Indirect => dict.set("Length", r(LENGTH_OBJ)),
            LengthKind::Missing => {
                dict.remove(b"Length");
            }
        }
        h.stream_with(id, &dict, &s.content, b"\n", b"\n");
        if kind == LengthKind::Indirect {
            h.object(LENGTH_OBJ, &int(len));
        }
    })
}

/// The golden written by hand with `style` after every `stream` keyword (and
/// the matching EOL before every `endstream`).
pub fn with_eol_style(style: EolStyle) -> Vec<u8> {
    let (eol, end_eol): (&[u8], &[u8]) = match style {
        EolStyle::Lf => (b"\n", b"\n"),
        EolStyle::CrLf => (b"\r\n", b"\r\n"),
        EolStyle::Cr => (b"\r", b"\r"),
        EolStyle::SpaceEol => (b" \n", b"\n"),
    };
    hand_classic(|h, id, object| match object {
        Object::Stream(s) => h.stream_with(id, &s.dict, &s.content, eol, end_eol),
        _ => h.object(id, object),
    })
}

/// `n` junk bytes (no `%`) and then [`golden_pdf`] unchanged, so every offset
/// in the file counts from the `%PDF` header, not from the first byte.
pub fn with_junk_prefix(n: usize) -> Vec<u8> {
    let junk = b"Not a header; junk before it.\r\n";
    let mut out: Vec<u8> = junk.iter().cycle().take(n).copied().collect();
    out.extend_from_slice(&golden_pdf());
    out
}

/// The golden written by hand with two whole copies of page 2's content
/// stream (object 12): first a stale one that draws the page's lines in
/// reverse order, then the golden's own, which the xref table names.
pub fn with_duplicate_object() -> Vec<u8> {
    let facts = font_facts();
    hand_classic(|h, id, object| {
        if id == CONTENTS[1] {
            h.object(id, &stream(flate_dict(), flate(&stale_content(&facts))));
        }
        h.object(id, object);
    })
}

/// D-032's own fixture (FR-g2): [`golden_pdf`] whole, then a later copy of page
/// 2's content stream (object 12, the stale lines) whose data the end of the
/// file cuts off halfway: no `endstream`, no `endobj`, `/Length` past EOF.
pub fn with_truncated_later_duplicate() -> Vec<u8> {
    let data = flate(&stale_content(&font_facts()));
    let mut out = golden_pdf();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(format!("{} 0 obj\n", CONTENTS[1]).as_bytes());
    let mut dict = flate_dict();
    dict.set("Length", data.len() as i64);
    write_object(&mut out, &Object::Dictionary(dict));
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&data[..data.len() / 2]);
    out
}

/// The golden written by hand with the font descriptor (object 7) missing its
/// `endobj`: object 8's header follows the dictionary directly.
pub fn with_no_endobj() -> Vec<u8> {
    hand_classic(|h, id, object| {
        if id == DESCRIPTOR {
            h.begin(id);
            write_object(&mut h.out, object);
            h.out.push(b'\n');
        } else {
            h.object(id, object);
        }
    })
}

/// [`golden_pdf`] with page 1's content stream filtered
/// `[/ASCII85Decode /FlateDecode]`: deflated, then ASCII85-encoded.
pub fn with_multi_filter() -> Vec<u8> {
    let facts = font_facts();
    golden_with(|objects| {
        let filters = Object::Array(vec![name(b"ASCII85Decode"), name(b"FlateDecode")]);
        let data = ascii85(&flate(&content(&facts, 0)));
        objects.insert(CONTENTS[0], stream(dict(vec![("Filter", filters)]), data));
    })
}

/// [`golden_pdf`] with page 1's image replaced by an 8×8 RGB image filtered
/// `/FlateDecode` with `/DecodeParms << /Predictor 15 /Colors 3 /Columns 8 >>`:
/// every row carries its own PNG filter type, and all five types are used.
pub fn with_predictor_image() -> Vec<u8> {
    golden_with(|objects| {
        let parms = dict(vec![
            ("Predictor", int(15)),
            ("Colors", int(3)),
            ("Columns", int(8)),
        ]);
        let mut d = image_dict(name(b"FlateDecode"));
        d.set("DecodeParms", Object::Dictionary(parms));
        let rows = png_predict(&predictor_pixels(), 8 * 3, 3);
        objects.insert(IMAGE, stream(d, flate(&rows)));
    })
}

/// [`golden_pdf`] with page 1's image filtered `[/FlateDecode /DCTDecode]`:
/// [`TINY_JPEG`], deflated.
pub fn with_flate_then_dct() -> Vec<u8> {
    golden_with(|objects| {
        let filters = Object::Array(vec![name(b"FlateDecode"), name(b"DCTDecode")]);
        objects.insert(IMAGE, stream(image_dict(filters), flate(TINY_JPEG)));
    })
}

/// D-031's differential: [`golden_pdf_objstm`] with page 2's dictionary
/// (object 4) both at top level and in object stream 13, `order` deciding
/// which comes first in the file (the packed copy's position is its
/// container's). The earlier copy is the golden's (`/MediaBox [0 0 612 792]`),
/// the later one says `[0 0 595 842]`, and the xref stream names the later.
pub fn with_objstm_and_plain_copy(order: CopyOrder) -> Vec<u8> {
    let objects = golden_objects(None);
    let page = PAGE_IDS[1];
    let earlier = &objects[&page];
    let mut later = earlier.clone();
    if let Object::Dictionary(d) = &mut later {
        let a4 = Object::Array(vec![int(0), int(0), int(595), int(842)]);
        d.set("MediaBox", a4);
    }
    let packed_copy = match order {
        CopyOrder::PlainFirst => &later,
        CopyOrder::ObjStmFirst => earlier,
    };
    let mut h = Hand::with_streams(&objects);
    let stm = ObjStm::pack(
        packable(&objects).map(|(id, o)| (id, if id == page { packed_copy } else { o })),
    );
    let mut packed = stm.packed(OBJSTM);
    match order {
        CopyOrder::PlainFirst => {
            h.object(page, earlier);
            h.object(OBJSTM, &stm.stream(vec![]));
        }
        CopyOrder::ObjStmFirst => {
            h.object(OBJSTM, &stm.stream(vec![]));
            h.object(page, &later);
            packed.remove(&page);
        }
    }
    h.xref_stream(XREF_STREAM, &packed)
}

/// [`golden_pdf_objstm`] with its seven dictionaries split over two object
/// streams that extend each other in a cycle (13 `/Extends` 15, 15 `/Extends`
/// 13), and 13's index also listing object 13 itself, whose packed value is a
/// container-shaped dictionary. Object 14 is the xref stream.
pub fn with_recursive_objstm() -> Vec<u8> {
    const OTHER: u32 = 15;
    let objects = golden_objects(None);
    let mut h = Hand::with_streams(&objects);
    let fake = Object::Dictionary(dict(vec![
        ("Type", name(b"ObjStm")),
        ("N", int(0)),
        ("First", int(0)),
    ]));
    let dicts: Vec<(u32, &Object)> = packable(&objects).collect();
    let (front, back) = dicts.split_at(2);
    let first = ObjStm::pack(front.iter().copied().chain([(OBJSTM, &fake)]));
    let second = ObjStm::pack(back.iter().copied());
    h.object(OBJSTM, &first.stream(vec![("Extends", r(OTHER))]));
    h.object(OTHER, &second.stream(vec![("Extends", r(OBJSTM))]));
    let mut packed = first.packed(OBJSTM);
    packed.remove(&OBJSTM);
    packed.extend(second.packed(OTHER));
    h.xref_stream(XREF_STREAM, &packed)
}

/// [`golden_pdf_objstm`] with a bad object-stream index: objects 3 and 4 carry
/// each other's offsets (so the offsets are not monotonic) and object 7's
/// offset points past the end of the packed data. The objects themselves and
/// the xref stream are the golden's.
pub fn with_bad_objstm_offsets() -> Vec<u8> {
    let objects = golden_objects(None);
    let mut h = Hand::with_streams(&objects);
    let mut stm = ObjStm::pack(packable(&objects));
    let packed = stm.packed(OBJSTM);
    let (a, b) = (stm.pairs[2].1, stm.pairs[3].1);
    stm.pairs[2].1 = b;
    stm.pairs[3].1 = a;
    let last = stm.pairs.len() - 1;
    stm.pairs[last].1 = stm.objects.len() + 64;
    h.object(OBJSTM, &stm.stream(vec![]));
    h.xref_stream(XREF_STREAM, &packed)
}

/// One page whose only marks are the filled glyph outlines of
/// [`OUTLINE_TEXT`], taken from [`TEST_FONT`]: path operators, no `BT`/`ET`,
/// no font resource and no font program anywhere in the file.
pub fn outline_only_page() -> Vec<u8> {
    let font = FontRef::new(TEST_FONT).expect("test font parses");
    let outlines = font.outline_glyphs();
    let hmtx = font.hmtx().expect("hmtx");
    let upem = i64::from(font.head().expect("head").units_per_em());
    let mut s = String::new();
    let mut advance = 0i64; // font units so far
    for c in OUTLINE_TEXT.chars() {
        let gid = glyph_of(&font, c);
        let path = glyph_path(&outlines, gid);
        if !path.is_empty() {
            // 16 pt: font units × 16 / upem, from (72, 720).
            let scale = real(16.0 / upem as f64);
            let x = real(72.0 + (advance * 16) as f64 / upem as f64);
            s.push_str(&format!("q\n{scale} 0 0 {scale} {x} 720 cm\n{path}f\nQ\n"));
        }
        advance += i64::from(hmtx.advance(gid).expect("hmtx advance"));
    }
    single_page(BTreeMap::new(), s.into_bytes(), BTreeMap::new())
}

/// One page that shows [`TYPE3_TEXT`] through a Type3 font, whose glyph
/// procedures fill [`TEST_FONT`]'s outlines: the only text source in the
/// file, with no `/FontFile*` anywhere.
pub fn type3_only_page() -> Vec<u8> {
    const FONT: u32 = 4;
    const FIRST_PROC: u32 = 6;
    let font = FontRef::new(TEST_FONT).expect("test font parses");
    let outlines = font.outline_glyphs();
    let hmtx = font.hmtx().expect("hmtx");
    let head = font.head().expect("head");
    assert_eq!(head.units_per_em(), 1000, "glyph space is font units");

    let codes: BTreeSet<u8> = TYPE3_TEXT.bytes().collect();
    let first = *codes.first().expect("text");
    let last = *codes.last().expect("text");
    let mut extra = BTreeMap::new();
    let mut procs = Dictionary::new();
    let mut differences = Vec::new();
    let mut widths = vec![int(0); usize::from(last - first) + 1];
    for (k, &code) in codes.iter().enumerate() {
        let c = char::from(code);
        let glyph = glyph_name(c);
        let gid = glyph_of(&font, c);
        let width = i64::from(hmtx.advance(gid).expect("hmtx advance"));
        let path = glyph_path(&outlines, gid);
        let fill = if path.is_empty() { "" } else { "f\n" };
        let proc_bytes = format!("{width} 0 d0\n{path}{fill}").into_bytes();
        let id = FIRST_PROC + k as u32;
        extra.insert(id, stream(flate_dict(), flate(&proc_bytes)));
        procs.set(glyph.clone(), r(id));
        differences.push(int(i64::from(code)));
        differences.push(name(glyph.as_bytes()));
        widths[usize::from(code - first)] = int(width);
    }
    let matrix = vec![
        Object::Real(0.001),
        int(0),
        int(0),
        Object::Real(0.001),
        int(0),
        int(0),
    ];
    let encoding = dict(vec![
        ("Type", name(b"Encoding")),
        ("Differences", Object::Array(differences)),
    ]);
    let bbox = [head.x_min(), head.y_min(), head.x_max(), head.y_max()];
    extra.insert(
        FONT,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"Type3")),
            (
                "FontBBox",
                Object::Array(bbox.iter().map(|&v| int(i64::from(v))).collect()),
            ),
            ("FontMatrix", Object::Array(matrix)),
            ("CharProcs", Object::Dictionary(procs)),
            ("Encoding", Object::Dictionary(encoding)),
            ("FirstChar", int(i64::from(first))),
            ("LastChar", int(i64::from(last))),
            ("Widths", Object::Array(widths)),
            ("Resources", Object::Dictionary(Dictionary::new())),
        ])),
    );
    // Letters, spaces and a period (`glyph_name`): nothing to escape.
    let content = format!("BT\n/T1 16 Tf\n72 720 Td\n({TYPE3_TEXT}) Tj\nET\n");
    let fonts = BTreeMap::from([("T1", FONT)]);
    single_page(fonts, content.into_bytes(), extra)
}

/// One page that shows "Hello" through a Word-style system font: a
/// `/TrueType /Arial /WinAnsiEncoding` font whose descriptor has no
/// `/FontFile*` key and no `/ToUnicode`, never embedded on purpose (D-084).
pub fn word_style_arial_page() -> Vec<u8> {
    const FONT: u32 = 6;
    const DESCRIPTOR: u32 = 7;
    let widths = [722, 556, 222, 222, 556].map(int).to_vec();
    let mut extra = BTreeMap::new();
    extra.insert(
        FONT,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"TrueType")),
            ("BaseFont", name(b"Arial")),
            ("Encoding", name(b"WinAnsiEncoding")),
            ("FirstChar", int(i64::from(b'H'))),
            ("LastChar", int(i64::from(b'o'))),
            ("Widths", Object::Array(widths)),
            ("FontDescriptor", r(DESCRIPTOR)),
        ])),
    );
    extra.insert(
        DESCRIPTOR,
        Object::Dictionary(dict(vec![
            ("Type", name(b"FontDescriptor")),
            ("FontName", name(b"Arial")),
            ("Flags", int(32)),
            (
                "FontBBox",
                Object::Array([-665, -325, 2000, 1040].map(int).to_vec()),
            ),
            ("ItalicAngle", int(0)),
            ("Ascent", int(905)),
            ("Descent", int(-212)),
            ("CapHeight", int(716)),
            ("StemV", int(80)),
        ])),
    );
    let content = b"BT\n/F1 16 Tf\n72 720 Td\n(Hello) Tj\nET\n".to_vec();
    single_page(BTreeMap::from([("F1", FONT)]), content, extra)
}

/// [`golden_pdf`] with an incremental update that deletes page 2 (object 4):
/// a new copy of page-tree node 2 whose `/Kids` holds page 1 alone, and an
/// xref section that rewrites node 2, marks object 4 free (generation 1) and
/// names the golden's table as `/Prev`. Page 4's bytes stay in the file, as
/// every incremental update leaves them (D-112).
pub fn incremental_page_removed() -> Vec<u8> {
    let mut out = golden_pdf();
    let prev = startxref(&out);
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let node = out.len();
    let pages = dict(vec![
        ("Type", name(b"Pages")),
        ("Kids", Object::Array(vec![r(PAGE_IDS[0])])),
        ("Count", int(1)),
    ]);
    out.extend_from_slice(format!("{PAGES} 0 obj\n").as_bytes());
    write_object(&mut out, &Object::Dictionary(pages));
    out.extend_from_slice(b"\nendobj\n");
    let xref = out.len();
    let removed = PAGE_IDS[1];
    let size = CONTENTS[1] + 1;
    let table = format!(
        "xref\n0 1\n{removed:010} 65535 f \n{PAGES} 1\n{node:010} 00000 n \n\
         {removed} 1\n0000000000 00001 f \n"
    );
    out.extend_from_slice(table.as_bytes());
    let trailer = dict(vec![
        ("Size", int(i64::from(size))),
        ("Root", r(CATALOG)),
        ("Prev", int(prev as i64)),
        ("ID", Hand::id()),
    ]);
    out.extend_from_slice(b"trailer\n");
    write_object(&mut out, &Object::Dictionary(trailer));
    out.extend_from_slice(format!("\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

/// The blank-case constructions (goal-r2-fr2 §3): one content stream per
/// page of [`blank_cases_pdf`], in page order.
pub const BLANK_CASES: [&str; 11] = [
    "",
    "1 1 1 rg 0 0 612 792 re f",
    "BT /F1 12 Tf 3 Tr 72 700 Td (Hidden) Tj ET",
    "0 g 100 100 200 200 re f",
    "0 0 0 0 re W n 0 g 100 100 200 200 re f",
    "BT /F1 12 Tf 72 700 Td (     ) Tj ET",
    "q 100 0 0 100 50 50 cm /Im1 Do Q",
    "BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
    "0 G 1 w 10 10 m 500 500 l S",
    "0 g 10 10 m 500 500 l 500 10 l h n",
    "1 g BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
];

/// Eleven pages, one [`BLANK_CASES`] construction each, a non-embedded
/// Helvetica and a 1×1 white image.
pub fn blank_cases_pdf() -> Vec<u8> {
    const HELVETICA: u32 = 3;
    const WHITE_PIXEL: u32 = 4;
    let mut w = Writer::with_version("1.7");
    w.add(
        1,
        Object::Dictionary(dict(vec![("Type", name(b"Catalog")), ("Pages", r(2))])),
    );
    w.add(
        HELVETICA,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"Type1")),
            ("BaseFont", name(b"Helvetica")),
        ])),
    );
    w.add_stream_raw(
        WHITE_PIXEL,
        dict(vec![
            ("Type", name(b"XObject")),
            ("Subtype", name(b"Image")),
            ("Width", int(1)),
            ("Height", int(1)),
            ("ColorSpace", name(b"DeviceRGB")),
            ("BitsPerComponent", int(8)),
        ]),
        vec![0xFF; 3],
    );
    let mut kids = Vec::new();
    for (i, content) in BLANK_CASES.iter().enumerate() {
        let (page, contents) = (10 + 2 * i as u32, 11 + 2 * i as u32);
        let resources = dict(vec![
            ("Font", Object::Dictionary(dict(vec![("F1", r(HELVETICA))]))),
            (
                "XObject",
                Object::Dictionary(dict(vec![("Im1", r(WHITE_PIXEL))])),
            ),
        ]);
        w.add(
            page,
            Object::Dictionary(dict(vec![
                ("Type", name(b"Page")),
                ("Parent", r(2)),
                (
                    "MediaBox",
                    Object::Array(vec![int(0), int(0), int(612), int(792)]),
                ),
                ("Resources", Object::Dictionary(resources)),
                ("Contents", r(contents)),
            ])),
        );
        w.add_stream_raw(contents, Dictionary::new(), content.as_bytes().to_vec());
        kids.push(r(page));
    }
    w.add(
        2,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Pages")),
            ("Count", int(kids.len() as i64)),
            ("Kids", Object::Array(kids)),
        ])),
    );
    w.trailer((1, 0), [7; 32], None);
    w.finish().expect("blank cases build")
}

// ---------------------------------------------------------------------------
// The goldens

const CATALOG: u32 = 1;
const PAGES: u32 = 2;
const PAGE_IDS: [u32; 2] = [3, 4];
const TYPE0: u32 = 5;
const CIDFONT: u32 = 6;
const DESCRIPTOR: u32 = 7;
const FONTFILE2: u32 = 8;
const TOUNICODE: u32 = 9;
const IMAGE: u32 = 10;
const CONTENTS: [u32; 2] = [11, 12];
const SIG_FIELD: u32 = 13;
const SIG_VALUE: u32 = 14;
/// The ObjStm golden's object stream and cross-reference stream.
const OBJSTM: u32 = 13;
const XREF_STREAM: u32 = 14;
/// [`with_wrong_length`]'s indirect `/Length` object.
const LENGTH_OBJ: u32 = 13;

/// A real family name behind a subset tag, as a Save-As file carries it.
const BASE_FONT: &[u8] = b"AAAAAA+NotoSans-Regular";
/// The dummy signature: this many zero bytes.
const SIG_CONTENTS_LEN: usize = 64;

fn golden_id_seed() -> [u8; 32] {
    Sha256::digest(b"pdfpundit golden_pdf").into()
}

fn name(s: &[u8]) -> Object {
    Object::Name(s.to_vec())
}

fn r(id: u32) -> Object {
    Object::Reference((id, 0))
}

fn int(v: i64) -> Object {
    Object::Integer(v)
}

fn dict(entries: Vec<(&str, Object)>) -> Dictionary {
    let mut d = Dictionary::new();
    for (k, v) in entries {
        d.set(k, v);
    }
    d
}

fn flate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

/// What the goldens need from the test font, in PDF glyph space (1000/em).
struct FontFacts {
    /// gid → (the character drawn with it, its advance width).
    glyphs: BTreeMap<u16, (char, i64)>,
    /// char → gid, for every character in [`GOLDEN_TEXT`].
    gid_of: BTreeMap<char, u16>,
    bbox: [i64; 4],
    ascent: i64,
    descent: i64,
    cap_height: i64,
}

fn font_facts() -> FontFacts {
    let font = FontRef::new(TEST_FONT).expect("test font parses");
    let cmap = font.cmap().expect("cmap");
    let hmtx = font.hmtx().expect("hmtx");
    let head = font.head().expect("head");
    let hhea = font.hhea().expect("hhea");
    let os2 = font.os2().expect("OS/2");
    let upem = i64::from(head.units_per_em());
    let scale = |v: i64| v * 1000 / upem;

    let mut glyphs = BTreeMap::new();
    let mut gid_of = BTreeMap::new();
    for c in GOLDEN_TEXT
        .iter()
        .flat_map(|p| p.iter())
        .flat_map(|l| l.chars())
    {
        let gid = cmap
            .map_codepoint(c)
            .expect("test font maps every golden char");
        let gid = u16::try_from(gid.to_u32()).expect("gid fits Identity-H");
        let advance = hmtx.advance(gid.into()).expect("hmtx advance");
        let prev = glyphs.insert(gid, (c, scale(i64::from(advance))));
        assert!(
            prev.is_none_or(|(p, _)| p == c),
            "two chars share gid {gid}"
        );
        gid_of.insert(c, gid);
    }
    FontFacts {
        glyphs,
        gid_of,
        bbox: [
            scale(i64::from(head.x_min())),
            scale(i64::from(head.y_min())),
            scale(i64::from(head.x_max())),
            scale(i64::from(head.y_max())),
        ],
        ascent: scale(i64::from(hhea.ascender().to_i16())),
        descent: scale(i64::from(hhea.descender().to_i16())),
        cap_height: scale(i64::from(os2.s_cap_height().unwrap_or(0))),
    }
}

/// `<gid gid …>` for one line of text.
fn hex_glyphs(facts: &FontFacts, line: &str) -> String {
    let mut s = String::from("<");
    for c in line.chars() {
        s.push_str(&format!("{:04X}", facts.gid_of[&c]));
    }
    s.push('>');
    s
}

fn content(facts: &FontFacts, page: usize) -> Vec<u8> {
    let mut s = String::from("BT\n/F1 16 Tf\n72 720 Td\n");
    for (i, line) in GOLDEN_TEXT[page].iter().enumerate() {
        if i > 0 {
            s.push_str("0 -24 Td\n");
        }
        s.push_str(&hex_glyphs(facts, line));
        s.push_str(" Tj\n");
    }
    s.push_str("ET\n");
    if page == 0 {
        s.push_str("q\n96 0 0 96 72 560 cm\n/Im1 Do\nQ\n");
    }
    s.into_bytes()
}

/// The hand-built `bfchar` CMap: one entry per glyph the goldens draw.
fn tounicode(facts: &FontFacts) -> Vec<u8> {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &(char, i64))> = facts.glyphs.iter().collect();
    for block in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", block.len()));
        for (gid, (c, _)) in block {
            let mut utf16 = [0u16; 2];
            let units: String = c
                .encode_utf16(&mut utf16)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            s.push_str(&format!("<{gid:04X}> <{units}>\n"));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// Every golden object, ready for [`Writer`]; `signed` adds the signature
/// field with this `/ByteRange`.
fn golden_writer(signed: Option<[i64; 4]>) -> Writer {
    writer_of(golden_objects(signed))
}

/// A 1.7 [`Writer`] holding `objects`.
fn writer_of(objects: BTreeMap<u32, Object>) -> Writer {
    let mut w = Writer::with_version("1.7");
    for (id, object) in objects {
        w.add(id, object);
    }
    w
}

/// A stream object whose bytes are already encoded with `dict`'s `/Filter`.
fn stream(dict: Dictionary, bytes: Vec<u8>) -> Object {
    Object::Stream(Stream::new(dict, bytes))
}

/// Every golden object by number; `signed` adds the signature field with this
/// `/ByteRange`.
fn golden_objects(signed: Option<[i64; 4]>) -> BTreeMap<u32, Object> {
    let facts = font_facts();
    let mut w = BTreeMap::new();

    let mut catalog = dict(vec![("Type", name(b"Catalog")), ("Pages", r(PAGES))]);
    if signed.is_some() {
        let acroform = dict(vec![
            ("Fields", Object::Array(vec![r(SIG_FIELD)])),
            ("SigFlags", int(3)),
        ]);
        catalog.set("AcroForm", Object::Dictionary(acroform));
    }
    w.insert(CATALOG, Object::Dictionary(catalog));
    w.insert(
        PAGES,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Pages")),
            (
                "Kids",
                Object::Array(PAGE_IDS.iter().map(|&p| r(p)).collect()),
            ),
            ("Count", int(PAGE_IDS.len() as i64)),
        ])),
    );

    for (i, (&page, &contents)) in PAGE_IDS.iter().zip(CONTENTS.iter()).enumerate() {
        let mut resources = dict(vec![(
            "Font",
            Object::Dictionary(dict(vec![("F1", r(TYPE0))])),
        )]);
        if i == 0 {
            let xobjects = dict(vec![("Im1", r(IMAGE))]);
            resources.set("XObject", Object::Dictionary(xobjects));
        }
        let mut d = dict(vec![
            ("Type", name(b"Page")),
            ("Parent", r(PAGES)),
            (
                "MediaBox",
                Object::Array(vec![int(0), int(0), int(612), int(792)]),
            ),
            ("Resources", Object::Dictionary(resources)),
            ("Contents", r(contents)),
        ]);
        if i == 0 && signed.is_some() {
            d.set("Annots", Object::Array(vec![r(SIG_FIELD)]));
        }
        w.insert(page, Object::Dictionary(d));
        let filter = dict(vec![("Filter", name(b"FlateDecode"))]);
        w.insert(contents, stream(filter, flate(&content(&facts, i))));
    }

    w.insert(
        TYPE0,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"Type0")),
            ("BaseFont", name(BASE_FONT)),
            ("Encoding", name(b"Identity-H")),
            ("DescendantFonts", Object::Array(vec![r(CIDFONT)])),
            ("ToUnicode", r(TOUNICODE)),
        ])),
    );
    let mut widths = Vec::new();
    for (&gid, &(_, width)) in &facts.glyphs {
        widths.push(int(i64::from(gid)));
        widths.push(Object::Array(vec![int(width)]));
    }
    let system_info = dict(vec![
        ("Registry", Object::string_literal("Adobe")),
        ("Ordering", Object::string_literal("Identity")),
        ("Supplement", int(0)),
    ]);
    w.insert(
        CIDFONT,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"CIDFontType2")),
            ("BaseFont", name(BASE_FONT)),
            ("CIDSystemInfo", Object::Dictionary(system_info)),
            ("FontDescriptor", r(DESCRIPTOR)),
            ("W", Object::Array(widths)),
            ("CIDToGIDMap", name(b"Identity")),
        ])),
    );
    w.insert(
        DESCRIPTOR,
        Object::Dictionary(dict(vec![
            ("Type", name(b"FontDescriptor")),
            ("FontName", name(BASE_FONT)),
            ("Flags", int(32)),
            (
                "FontBBox",
                Object::Array(facts.bbox.iter().map(|&v| int(v)).collect()),
            ),
            ("ItalicAngle", int(0)),
            ("Ascent", int(facts.ascent)),
            ("Descent", int(facts.descent)),
            ("CapHeight", int(facts.cap_height)),
            ("StemV", int(80)),
            ("FontFile2", r(FONTFILE2)),
        ])),
    );
    w.insert(
        FONTFILE2,
        stream(
            dict(vec![
                ("Length1", int(TEST_FONT.len() as i64)),
                ("Filter", name(b"FlateDecode")),
            ]),
            flate(TEST_FONT),
        ),
    );
    w.insert(
        TOUNICODE,
        stream(
            dict(vec![("Filter", name(b"FlateDecode"))]),
            flate(&tounicode(&facts)),
        ),
    );
    w.insert(
        IMAGE,
        stream(image_dict(name(b"DCTDecode")), TINY_JPEG.to_vec()),
    );

    if let Some(byte_range) = signed {
        w.insert(
            SIG_FIELD,
            Object::Dictionary(dict(vec![
                ("Type", name(b"Annot")),
                ("Subtype", name(b"Widget")),
                ("FT", name(b"Sig")),
                ("T", Object::string_literal("Signature1")),
                ("Rect", Object::Array(vec![int(0), int(0), int(0), int(0)])),
                ("F", int(132)),
                ("P", r(PAGE_IDS[0])),
                ("V", r(SIG_VALUE)),
            ])),
        );
        w.insert(
            SIG_VALUE,
            Object::Dictionary(dict(vec![
                ("Type", name(b"Sig")),
                ("Filter", name(b"Adobe.PPKLite")),
                ("SubFilter", name(b"adbe.pkcs7.detached")),
                (
                    "ByteRange",
                    Object::Array(byte_range.iter().map(|&v| int(v)).collect()),
                ),
                (
                    "Contents",
                    Object::String(vec![0; SIG_CONTENTS_LEN], StringFormat::Hexadecimal),
                ),
            ])),
        );
    }
    w
}

/// An 8×8 RGB image dictionary with `filter` (the golden's image, unfiltered).
fn image_dict(filter: Object) -> Dictionary {
    dict(vec![
        ("Type", name(b"XObject")),
        ("Subtype", name(b"Image")),
        ("Width", int(8)),
        ("Height", int(8)),
        ("ColorSpace", name(b"DeviceRGB")),
        ("BitsPerComponent", int(8)),
        ("Filter", filter),
    ])
}

fn flate_dict() -> Dictionary {
    dict(vec![("Filter", name(b"FlateDecode"))])
}

fn is_stream(object: &Object) -> bool {
    matches!(object, Object::Stream(_))
}

/// Every object an object stream may hold (all but streams), in number order.
fn packable(objects: &BTreeMap<u32, Object>) -> impl Iterator<Item = (u32, &Object)> {
    objects
        .iter()
        .filter(|(_, o)| !is_stream(o))
        .map(|(&id, o)| (id, o))
}

/// Page 2's content with its lines in reverse order: the stale copy of a
/// doubled object.
fn stale_content(facts: &FontFacts) -> Vec<u8> {
    let mut s = String::from("BT\n/F1 16 Tf\n72 720 Td\n");
    for (i, line) in GOLDEN_TEXT[1].iter().rev().enumerate() {
        if i > 0 {
            s.push_str("0 -24 Td\n");
        }
        s.push_str(&hex_glyphs(facts, line));
        s.push_str(" Tj\n");
    }
    s.push_str("ET\n");
    s.into_bytes()
}

/// [`golden_pdf`] with some objects replaced, through the writer.
fn golden_with(edit: impl FnOnce(&mut BTreeMap<u32, Object>)) -> Vec<u8> {
    let mut objects = golden_objects(None);
    edit(&mut objects);
    let mut w = writer_of(objects);
    w.trailer((CATALOG, 0), golden_id_seed(), None);
    w.finish().expect("golden variant builds")
}

/// The golden's objects written by hand in number order, then a classic xref
/// table naming the last copy of each number. `write` writes each object, and
/// may write it differently or more than once.
fn hand_classic(mut write: impl FnMut(&mut Hand, u32, &Object)) -> Vec<u8> {
    let mut h = Hand::new("1.7");
    for (&id, object) in &golden_objects(None) {
        write(&mut h, id, object);
    }
    h.classic(CATALOG)
}

/// A one-page 1.7 file: catalog 1, page tree 2, page 3 (Letter) with `fonts`
/// as its `/Font` resources (none: no `/Font` key), Flate content stream 5,
/// and `extra` objects.
fn single_page(
    fonts: BTreeMap<&str, u32>,
    content: Vec<u8>,
    extra: BTreeMap<u32, Object>,
) -> Vec<u8> {
    const PAGE: u32 = 3;
    const CONTENT: u32 = 5;
    let mut resources = Dictionary::new();
    if !fonts.is_empty() {
        let mut f = Dictionary::new();
        for (slot, id) in fonts {
            f.set(slot, r(id));
        }
        resources.set("Font", Object::Dictionary(f));
    }
    let mut objects = extra;
    objects.insert(
        CATALOG,
        Object::Dictionary(dict(vec![("Type", name(b"Catalog")), ("Pages", r(PAGES))])),
    );
    objects.insert(
        PAGES,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Pages")),
            ("Kids", Object::Array(vec![r(PAGE)])),
            ("Count", int(1)),
        ])),
    );
    objects.insert(
        PAGE,
        Object::Dictionary(dict(vec![
            ("Type", name(b"Page")),
            ("Parent", r(PAGES)),
            (
                "MediaBox",
                Object::Array(vec![int(0), int(0), int(612), int(792)]),
            ),
            ("Resources", Object::Dictionary(resources)),
            ("Contents", r(CONTENT)),
        ])),
    );
    objects.insert(CONTENT, stream(flate_dict(), flate(&content)));
    let mut w = writer_of(objects);
    w.trailer(
        (CATALOG, 0),
        Sha256::digest(b"pdfpundit single_page").into(),
        None,
    );
    w.finish().expect("single page builds")
}

fn glyph_of(font: &FontRef, c: char) -> GlyphId {
    font.charmap()
        .map(c)
        .unwrap_or_else(|| panic!("the test font has no {c:?}"))
}

/// The glyph name a Type3 `/Differences` array gives `c` (ASCII letters, space
/// and period only).
fn glyph_name(c: char) -> String {
    match c {
        ' ' => "space".to_owned(),
        '.' => "period".to_owned(),
        c if c.is_ascii_alphabetic() => c.to_string(),
        c => panic!("no glyph name for {c:?}"),
    }
}

/// `gid`'s outline in font units as PDF path operators (`m`, `l`, `c`, `h`),
/// quadratic segments raised to cubics. Empty for a glyph with no contours.
fn glyph_path(outlines: &OutlineGlyphCollection, gid: GlyphId) -> String {
    let glyph = outlines.get(gid).expect("glyph outline");
    let mut pen = PathPen::default();
    glyph
        .draw(
            DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
            &mut pen,
        )
        .expect("outline draws");
    pen.ops
}

/// Writes an outline as PDF path operators. Only `+ - * /` on f64, so the
/// numbers are the same on every platform.
#[derive(Default)]
struct PathPen {
    ops: String,
    at: (f64, f64),
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = (f64::from(x), f64::from(y));
        let (x, y) = (real(self.at.0), real(self.at.1));
        self.ops.push_str(&format!("{x} {y} m\n"));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.at = (f64::from(x), f64::from(y));
        let (x, y) = (real(self.at.0), real(self.at.1));
        self.ops.push_str(&format!("{x} {y} l\n"));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (x0, y0) = self.at;
        let (qx, qy) = (f64::from(cx0), f64::from(cy0));
        let (x1, y1) = (f64::from(x), f64::from(y));
        let c1 = (x0 + 2.0 * (qx - x0) / 3.0, y0 + 2.0 * (qy - y0) / 3.0);
        let c2 = (x1 + 2.0 * (qx - x1) / 3.0, y1 + 2.0 * (qy - y1) / 3.0);
        self.curve(c1, c2, (x1, y1));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.curve(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(cx1), f64::from(cy1)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.ops.push_str("h\n");
    }
}

impl PathPen {
    fn curve(&mut self, c1: (f64, f64), c2: (f64, f64), to: (f64, f64)) {
        self.at = to;
        let points = [c1.0, c1.1, c2.0, c2.1, to.0, to.1].map(real);
        self.ops.push_str(&format!("{} c\n", points.join(" ")));
    }
}

/// A PDF real with at most three decimals and no trailing zeros.
fn real(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// ASCII85 with `z` for zero groups and the `~>` end marker.
fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut group = [0u8; 4];
        group[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(group);
        if chunk.len() == 4 && v == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = (v % 85) as u8 + b'!';
            v /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// [`with_predictor_image`]'s pixels: 8×8 RGB, row-major.
fn predictor_pixels() -> Vec<u8> {
    let mut px = Vec::with_capacity(8 * 8 * 3);
    for y in 0..8u8 {
        for x in 0..8u8 {
            px.extend_from_slice(&[x * 32, y * 32, (x + y) * 16]);
        }
    }
    px
}

/// PNG-predicted rows (`/Predictor` 10–15): row `k` uses filter type `k % 5`
/// (None, Sub, Up, Average, Paeth) and starts with that type byte.
fn png_predict(pixels: &[u8], row_len: usize, bpp: usize) -> Vec<u8> {
    let zero = vec![0u8; row_len];
    let mut out = Vec::with_capacity(pixels.len() + pixels.len() / row_len);
    for (k, row) in pixels.chunks(row_len).enumerate() {
        let up = if k == 0 {
            &zero[..]
        } else {
            &pixels[(k - 1) * row_len..k * row_len]
        };
        let kind = (k % 5) as u8;
        out.push(kind);
        for i in 0..row_len {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = up[i];
            let c = if i >= bpp { up[i - bpp] } else { 0 };
            let predicted = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                _ => paeth(a, b, c),
            };
            out.push(row[i].wrapping_sub(predicted));
        }
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// Hand assembly

/// A file written byte by byte, for what [`Writer`] must not make (object and
/// xref streams) or cannot make (doubled numbers, odd EOLs, wrong lengths).
struct Hand {
    out: Vec<u8>,
    /// Where each top-level `N 0 obj` starts; a later copy replaces an earlier.
    offsets: BTreeMap<u32, usize>,
}

impl Hand {
    /// `%PDF-<version>` and a binary-mark comment.
    fn new(version: &str) -> Hand {
        let mut out = format!("%PDF-{version}\n%").into_bytes();
        out.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3, b'\n']);
        Hand {
            out,
            offsets: BTreeMap::new(),
        }
    }

    /// A 1.5 file holding the streams among `objects` at top level, in number
    /// order: what every object-stream fixture starts with.
    fn with_streams(objects: &BTreeMap<u32, Object>) -> Hand {
        let mut h = Hand::new("1.5");
        for (&id, object) in objects.iter().filter(|(_, o)| is_stream(o)) {
            h.object(id, object);
        }
        h
    }

    /// `id 0 obj` and its EOL.
    fn begin(&mut self, id: u32) {
        self.offsets.insert(id, self.out.len());
        self.out
            .extend_from_slice(format!("{id} 0 obj\n").as_bytes());
    }

    fn object(&mut self, id: u32, object: &Object) {
        self.begin(id);
        write_object(&mut self.out, object);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    /// A stream object with `dict` as given (with or without `/Length`),
    /// `eol` after `stream` and `end_eol` before `endstream`.
    fn stream_with(&mut self, id: u32, dict: &Dictionary, data: &[u8], eol: &[u8], end_eol: &[u8]) {
        self.begin(id);
        write_object(&mut self.out, &Object::Dictionary(dict.clone()));
        self.out.extend_from_slice(b"\nstream");
        self.out.extend_from_slice(eol);
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(end_eol);
        self.out.extend_from_slice(b"endstream\nendobj\n");
    }

    /// The goldens' `/ID`.
    fn id() -> Object {
        let id = Object::String(golden_id_seed()[..16].to_vec(), StringFormat::Hexadecimal);
        Object::Array(vec![id.clone(), id])
    }

    /// A classic xref table (one subsection from 0, free entries for gaps),
    /// `trailer`, `startxref` and `%%EOF`.
    fn classic(mut self, root: u32) -> Vec<u8> {
        let size = self.offsets.keys().next_back().map_or(1, |&m| m + 1);
        let start = self.out.len();
        let head = format!("xref\n0 {size}\n0000000000 65535 f \n");
        self.out.extend_from_slice(head.as_bytes());
        for id in 1..size {
            let row = match self.offsets.get(&id) {
                Some(at) => format!("{at:010} 00000 n \n"),
                None => "0000000000 00000 f \n".to_owned(),
            };
            self.out.extend_from_slice(row.as_bytes());
        }
        let trailer = dict(vec![
            ("Size", int(i64::from(size))),
            ("Root", r(root)),
            ("ID", Hand::id()),
        ]);
        self.out.extend_from_slice(b"trailer\n");
        write_object(&mut self.out, &Object::Dictionary(trailer));
        let tail = format!("\nstartxref\n{start}\n%%EOF\n");
        self.out.extend_from_slice(tail.as_bytes());
        self.out
    }

    /// Cross-reference stream `id` (`/W [1 4 2]`, Flate, `/Root` the catalog)
    /// as the last object: type 2 rows for `packed` (number → container,
    /// index), type 1 rows for the other written objects and itself, then
    /// `startxref` and `%%EOF`.
    fn xref_stream(mut self, id: u32, packed: &BTreeMap<u32, (u32, u16)>) -> Vec<u8> {
        let start = self.out.len();
        self.offsets.insert(id, start);
        let max = self.offsets.keys().chain(packed.keys()).max().copied();
        let size = max.unwrap_or(0) + 1;
        let mut rows = Vec::with_capacity(size as usize * 7);
        for n in 0..size {
            let (kind, f2, f3) = if n == 0 {
                (0u8, 0u32, 0xFFFFu16)
            } else if let Some(&(container, index)) = packed.get(&n) {
                (2, container, index)
            } else if let Some(&at) = self.offsets.get(&n) {
                (
                    1,
                    u32::try_from(at).expect("fixture offsets fit 4 bytes"),
                    0,
                )
            } else {
                (0, 0, 0)
            };
            rows.push(kind);
            rows.extend_from_slice(&f2.to_be_bytes());
            rows.extend_from_slice(&f3.to_be_bytes());
        }
        let d = dict(vec![
            ("Type", name(b"XRef")),
            ("Size", int(i64::from(size))),
            ("W", Object::Array(vec![int(1), int(4), int(2)])),
            ("Root", r(CATALOG)),
            ("ID", Hand::id()),
            ("Filter", name(b"FlateDecode")),
        ]);
        self.object(id, &stream(d, flate(&rows)));
        let tail = format!("startxref\n{start}\n%%EOF\n");
        self.out.extend_from_slice(tail.as_bytes());
        self.out
    }
}

/// An object stream's contents before encoding: the `(number, offset)` pairs
/// of its index and the objects they point into.
struct ObjStm {
    pairs: Vec<(u32, usize)>,
    objects: Vec<u8>,
}

impl ObjStm {
    /// `objects` one after another, each followed by a newline.
    fn pack<'a>(objects: impl IntoIterator<Item = (u32, &'a Object)>) -> ObjStm {
        let mut stm = ObjStm {
            pairs: Vec::new(),
            objects: Vec::new(),
        };
        for (id, object) in objects {
            stm.pairs.push((id, stm.objects.len()));
            write_object(&mut stm.objects, object);
            stm.objects.push(b'\n');
        }
        stm
    }

    /// Where each packed number sits: (`container`, index).
    fn packed(&self, container: u32) -> BTreeMap<u32, (u32, u16)> {
        self.pairs
            .iter()
            .enumerate()
            .map(|(k, &(id, _))| (id, (container, k as u16)))
            .collect()
    }

    /// The `/Type /ObjStm` stream: the index as `pairs` says, `/N`, `/First`,
    /// the `extra` entries, Flate.
    fn stream(&self, extra: Vec<(&str, Object)>) -> Object {
        let index: Vec<String> = self
            .pairs
            .iter()
            .map(|(id, at)| format!("{id} {at}"))
            .collect();
        let mut data = format!("{}\n", index.join(" ")).into_bytes();
        let first = data.len();
        data.extend_from_slice(&self.objects);
        let mut d = dict(vec![
            ("Type", name(b"ObjStm")),
            ("N", int(self.pairs.len() as i64)),
            ("First", int(first as i64)),
        ]);
        for (k, v) in extra {
            d.set(k, v);
        }
        d.set("Filter", name(b"FlateDecode"));
        stream(d, flate(&data))
    }
}

/// `object` in PDF syntax, spaced: a space between a dictionary key and its
/// value and between entries and elements; a stream as its dictionary,
/// `stream`, LF, the bytes, LF, `endstream`.
fn write_object(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(v) => out.extend_from_slice(real(f64::from(*v)).as_bytes()),
        Object::Name(n) => {
            out.push(b'/');
            for &b in n {
                if (b'!'..=b'~').contains(&b) && !is_delimiter_or_space(b) && b != b'#' {
                    out.push(b);
                } else {
                    out.extend_from_slice(format!("#{b:02X}").as_bytes());
                }
            }
        }
        Object::String(s, StringFormat::Literal) => {
            out.push(b'(');
            for &b in s {
                if matches!(b, b'(' | b')' | b'\\') {
                    out.push(b'\\');
                }
                out.push(b);
            }
            out.push(b')');
        }
        Object::String(s, StringFormat::Hexadecimal) => {
            out.push(b'<');
            for b in s {
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (k, item) in items.iter().enumerate() {
                if k > 0 {
                    out.push(b' ');
                }
                write_object(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(d) => {
            out.extend_from_slice(b"<<");
            for (k, (key, value)) in d.iter().enumerate() {
                if k > 0 {
                    out.push(b' ');
                }
                write_object(out, &Object::Name(key.clone()));
                out.push(b' ');
                write_object(out, value);
            }
            out.extend_from_slice(b">>");
        }
        Object::Stream(s) => {
            write_object(out, &Object::Dictionary(s.dict.clone()));
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((id, generation)) => {
            out.extend_from_slice(format!("{id} {generation} R").as_bytes());
        }
    }
}

/// The `<…>` of the signature's `/Contents`, delimiters included (what a
/// `/ByteRange` leaves out).
fn signature_contents_span(pdf: &[u8]) -> Range<usize> {
    let sig = find(pdf, b"/Type/Sig").expect("signature dictionary");
    let key = sig + find(&pdf[sig..], b"/Contents").expect("/Contents in /Sig");
    let open = key + find(&pdf[key..], b"<").expect("hex string");
    let close = open + find(&pdf[open..], b">").expect("hex string end");
    open..close + 1
}

// ---------------------------------------------------------------------------
// The corruptors

/// SplitMix64: a small, fixed generator, so a seed means the same bytes on
/// every platform and every release.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn delete(pdf: &[u8], span: Range<usize>) -> Vec<u8> {
    let mut out = Vec::with_capacity(pdf.len() - span.len());
    out.extend_from_slice(&pdf[..span.start]);
    out.extend_from_slice(&pdf[span.end..]);
    out
}

fn blank(pdf: &[u8], spans: &[Range<usize>]) -> Vec<u8> {
    let mut out = pdf.to_vec();
    for span in spans {
        out[span.clone()].fill(b' ');
    }
    out
}

fn is_delimiter_or_space(b: u8) -> bool {
    b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b) || b == 0
}

/// The offset `startxref` names.
fn startxref(pdf: &[u8]) -> usize {
    let at = rfind(pdf, b"startxref").expect("startxref keyword") + 9;
    let digits: String = pdf[at..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| char::from(b))
        .collect();
    digits.parse().expect("startxref offset")
}

/// Where `%PDF-` starts. lopdf (like every reader) counts xref offsets from
/// here, not from the first byte of the file, so a junk prefix shifts them.
fn header_at(pdf: &[u8]) -> usize {
    find(pdf, b"%PDF-").unwrap_or(0)
}

/// The file position of an xref `offset`.
fn file_offset(pdf: &[u8], offset: u32) -> usize {
    header_at(pdf) + offset as usize
}

/// The first byte after `%PDF-x.y` and the binary-mark comment line.
fn after_header(pdf: &[u8]) -> usize {
    let header = header_at(pdf);
    let first = header + find(&pdf[header..], b"\n").expect("header line") + 1;
    if pdf.get(first) == Some(&b'%') {
        first + find(&pdf[first..], b"\n").expect("binary-mark line") + 1
    } else {
        first
    }
}

/// The fixture parsed by lopdf, for object numbers, kinds and xref offsets.
fn load(pdf: &[u8]) -> Document {
    Document::load_mem(pdf).expect("corrupt takes a loadable fixture")
}

/// `N G obj` through `endobj` of object `id`.
fn object_span(pdf: &[u8], doc: &Document, id: u32) -> Range<usize> {
    let start = match doc.reference_table.get(id) {
        Some(XrefEntry::Normal { offset, .. }) => file_offset(pdf, *offset),
        other => panic!("object {id} has no offset in the xref table: {other:?}"),
    };
    let end = start + find(&pdf[start..], b"endobj").expect("endobj") + 6;
    start..end
}

/// The data bytes of stream `id`: after `stream` and its EOL, `/Length` long.
fn stream_data(pdf: &[u8], doc: &Document, id: u32) -> Range<usize> {
    let span = object_span(pdf, doc, id);
    let kw = span.start + find(&pdf[span.clone()], b"stream").expect("stream keyword") + 6;
    let start = match &pdf[kw..] {
        [b'\r', b'\n', ..] => kw + 2,
        [b'\n' | b'\r', ..] => kw + 1,
        _ => panic!("no EOL after stream keyword in object {id}"),
    };
    let len = doc
        .get_object((id, 0))
        .and_then(Object::as_stream)
        .expect("stream object")
        .content
        .len();
    start..start + len
}

/// C4: a seeded 45–128 byte span covering the page-tree node from `/Pages` (its
/// `/Type` value) through its `/Count` value, after the header lines.
fn page_tree_span(pdf: &[u8], rng: &mut SplitMix64) -> Range<usize> {
    let doc = load(pdf);
    let pages_id = doc
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(Object::as_reference)
        .expect("catalog /Pages")
        .0;
    let span = object_span(pdf, &doc, pages_id);
    let body = &pdf[span.clone()];
    let pages_at = span.start + find(body, b"/Pages").expect("/Pages in the page-tree node");
    let count_at = span.start + find(body, b"/Count").expect("/Count in the page-tree node");
    let count_end = count_at
        + 6
        + pdf[count_at + 6..]
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .expect("count value");
    let count_end = count_end
        + pdf[count_end..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .expect("count digits end");
    let len = 45 + rng.below(128 - 45 + 1);
    let lo = count_end.saturating_sub(len).max(after_header(pdf));
    let hi = pages_at.min(pdf.len() - len);
    assert!(lo <= hi, "no {len}-byte span covers the page-tree node");
    let start = lo + rng.below(hi - lo + 1);
    start..start + len
}

/// C5: a seeded `N 0 obj` + EOL that is exactly 8 bytes (object 1 to 9).
fn object_header(pdf: &[u8], rng: &mut SplitMix64) -> Range<usize> {
    let doc = load(pdf);
    let candidates: Vec<usize> = (1..=9u32)
        .filter_map(|id| match doc.reference_table.get(id) {
            Some(XrefEntry::Normal { offset, .. }) => Some((id, file_offset(pdf, *offset))),
            _ => None,
        })
        .filter(|&(id, at)| pdf[at..].starts_with(format!("{id} 0 obj\n").as_bytes()))
        .map(|(_, at)| at)
        .collect();
    assert!(!candidates.is_empty(), "no 8-byte object header to strip");
    let at = candidates[rng.below(candidates.len())];
    at..at + 8
}

/// C6: `/Font` and its value inside a seeded page's inline `/Resources`.
fn page_font_entry(pdf: &[u8], rng: &mut SplitMix64) -> Range<usize> {
    let doc = load(pdf);
    let pages: Vec<u32> = doc.get_pages().values().map(|id| id.0).collect();
    assert!(!pages.is_empty(), "no pages");
    let page = pages[rng.below(pages.len())];
    let span = object_span(pdf, &doc, page);
    let res = span.start + find(&pdf[span.clone()], b"/Resources").expect("page /Resources") + 10;
    let mut at = res;
    let key = loop {
        let k = at + find(&pdf[at..span.end], b"/Font").expect("/Font in /Resources");
        if is_delimiter_or_space(pdf[k + 5]) {
            break k;
        }
        at = k + 5;
    };
    let value = key
        + 5
        + pdf[key + 5..]
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .expect("/Font value");
    let end = if pdf[value..].starts_with(b"<<") {
        let mut depth = 0usize;
        let mut i = value;
        loop {
            if pdf[i..].starts_with(b"<<") {
                depth += 1;
                i += 2;
            } else if pdf[i..].starts_with(b">>") {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    break i;
                }
            } else {
                i += 1;
            }
        }
    } else {
        // `N G R`
        value + find(&pdf[value..], b"R").expect("indirect /Font value") + 1
    };
    assert!(end <= span.end, "/Font value runs past the page object");
    key..end
}

/// C7/C8: the data of every `/FontFile2` stream, plus every `/ToUnicode` stream
/// when `tounicode`, in object-number order.
fn font_streams(pdf: &[u8], tounicode: bool) -> Vec<Range<usize>> {
    let doc = load(pdf);
    let mut ids = Vec::new();
    for object in doc.objects.values() {
        let Ok(d) = object.as_dict() else { continue };
        if let Ok(Object::Reference(id)) = d.get(b"FontFile2") {
            ids.push(id.0);
        }
        if tounicode && let Ok(Object::Reference(id)) = d.get(b"ToUnicode") {
            ids.push(id.0);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    assert!(!ids.is_empty(), "no font streams to blank");
    ids.into_iter()
        .map(|id| stream_data(pdf, &doc, id))
        .collect()
}

// ---------------------------------------------------------------------------
// C9

/// REPDF's C9 population (eng-r1-fr1, eng-r2-fr1): 1,445 of the 3,429 Flate
/// streams carry one replaced byte each.
const C9_FLATE_HIT: (u64, u64) = (1_445, 3_429);
/// 304 replaced bytes in non-Flate stream bodies per 1,445 Flate hits.
const C9_OTHER_PER_FLATE_HIT: (u64, u64) = (304, 1_445);
/// About one replacement outside stream data per three stream hits.
const C9_OUTSIDE_PER_STREAM_HIT: (u64, u64) = (1, 3);
/// Keys whose values C9 never touches (nor the keys themselves).
const C9_KEPT: [&[u8]; 5] = [b"Root", b"Pages", b"Kids", b"Count", b"Type"];

impl SplitMix64 {
    /// `n × num / den`, the fraction rounded up with its own probability.
    fn scaled(&mut self, n: usize, (num, den): (u64, u64)) -> usize {
        let x = n as u64 * num;
        let up = (self.next() % den) < x % den;
        (x / den) as usize + usize::from(up)
    }

    /// A byte other than `b`, each of the 255 equally likely.
    fn other_than(&mut self, b: u8) -> u8 {
        b.wrapping_add(1 + self.below(255) as u8)
    }
}

/// The bytes C9 may replace outside stream data, by kind.
#[derive(Default)]
struct OutsidePools {
    width: Vec<usize>,
    array: Vec<usize>,
    dict_value: Vec<usize>,
    number: Vec<usize>,
}

/// C9's replacements for `pdf`, sorted by offset (see [`corrupt_with_log`]).
fn c9_replacements(pdf: &[u8], rng: &mut SplitMix64) -> Vec<Replacement> {
    let doc = load(pdf);
    let mut flate = Vec::new();
    let mut other = Vec::new();
    let mut pools = OutsidePools::default();
    for (&id, entry) in &doc.reference_table.entries {
        let XrefEntry::Normal { offset, generation } = *entry else {
            continue;
        };
        let Ok(object) = doc.get_object((id, generation)) else {
            continue;
        };
        let xref = matches!(object, Object::Stream(s) if s.dict.has_type(b"XRef"));
        let at = file_offset(pdf, offset);
        let scan = scan_object(pdf, at, (id, generation), xref);
        for (span, kind) in scan.leaves {
            let pool = match kind {
                Leaf::Width => &mut pools.width,
                Leaf::Array => &mut pools.array,
                Leaf::DictValue => &mut pools.dict_value,
                Leaf::Number => &mut pools.number,
            };
            pool.extend(span);
        }
        if let (Object::Stream(s), Some(start)) = (object, scan.data_start) {
            let data = start..start + s.content.len();
            if data.is_empty() {
                continue;
            }
            if first_filter_is_flate(&s.dict) {
                flate.push(data);
            } else {
                other.push(data);
            }
        }
    }
    assert!(!flate.is_empty(), "no Flate stream to tamper with");

    let mut log = Vec::new();
    let mut used = BTreeSet::new();
    let mut replace = |at: usize, site, rng: &mut SplitMix64, used: &mut BTreeSet<usize>| {
        used.insert(at);
        let from = pdf[at];
        let to = rng.other_than(from);
        log.push(Replacement { at, from, to, site });
    };

    // About 42% of the Flate streams, at least one, one byte each.
    let hits = rng.scaled(flate.len(), C9_FLATE_HIT).clamp(1, flate.len());
    let mut order: Vec<usize> = (0..flate.len()).collect();
    for k in 0..hits {
        let pick = k + rng.below(order.len() - k);
        order.swap(k, pick);
        let data = &flate[order[k]];
        let at = data.start + rng.below(data.len());
        replace(at, ReplacementSite::FlateBody, rng, &mut used);
    }

    // Non-Flate bodies, in the measured proportion.
    let other_bytes: Vec<usize> = other.iter().flat_map(|r| r.clone()).collect();
    let mut other_hits = 0;
    for _ in 0..rng.scaled(hits, C9_OTHER_PER_FLATE_HIT) {
        if let Some(at) = pick_unused(&other_bytes, &used, rng) {
            replace(at, ReplacementSite::OtherBody, rng, &mut used);
            other_hits += 1;
        }
    }

    // Outside stream data: the three kinds in turn from a seeded start, empty
    // kinds skipped. An array hit draws from the width arrays, and from the
    // other arrays only once no width-array byte is left.
    let kinds: [(&[&Vec<usize>], ReplacementSite); 3] = [
        (&[&pools.width, &pools.array], ReplacementSite::Array),
        (&[&pools.dict_value], ReplacementSite::DictValue),
        (&[&pools.number], ReplacementSite::Number),
    ];
    let start = rng.below(kinds.len());
    let outside = rng.scaled(hits + other_hits, C9_OUTSIDE_PER_STREAM_HIT);
    for i in 0..outside {
        let pick = (0..kinds.len()).find_map(|k| {
            let (pools, site) = kinds[(start + i + k) % kinds.len()];
            pools
                .iter()
                .find_map(|pool| pick_unused(pool, &used, rng))
                .map(|at| (at, site))
        });
        if let Some((at, site)) = pick {
            replace(at, site, rng, &mut used);
        }
    }

    log.sort_by_key(|r| r.at);
    log
}

/// A seeded member of `pool` not in `used` (the next free one after a seeded
/// start), or `None` when every member is used.
fn pick_unused(pool: &[usize], used: &BTreeSet<usize>, rng: &mut SplitMix64) -> Option<usize> {
    if pool.is_empty() {
        return None;
    }
    let start = rng.below(pool.len());
    (0..pool.len())
        .map(|k| pool[(start + k) % pool.len()])
        .find(|at| !used.contains(at))
}

/// Whether a stream's raw bytes are zlib data: its `/Filter` is
/// `/FlateDecode`, or an array that starts with it.
fn first_filter_is_flate(dict: &Dictionary) -> bool {
    let flate = |o: &Object| matches!(o, Object::Name(n) if n == b"FlateDecode");
    match dict.get(b"Filter") {
        Ok(Object::Array(filters)) => filters.first().is_some_and(flate),
        Ok(filter) => flate(filter),
        Err(_) => false,
    }
}

/// What a leaf token outside stream data is, for C9.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    /// An element of a `/W` or `/Widths` array (or an array inside one).
    Width,
    Array,
    DictValue,
    /// The whole value of a bare-number object.
    Number,
}

/// One top-level object read from its `N G obj` header.
struct ObjectScan {
    /// The replaceable bytes of every leaf token C9 may touch: a number or
    /// keyword whole, a name after its `/`, a string inside its delimiters.
    leaves: Vec<(Range<usize>, Leaf)>,
    /// The first data byte, when the object is a stream.
    data_start: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    DictOpen,
    DictClose,
    ArrayOpen,
    ArrayClose,
    Name,
    Str,
    Word,
}

/// One token: the bytes C9 may replace (`span`) and where the next begins.
#[derive(Debug, Clone)]
struct Token {
    kind: TokKind,
    span: Range<usize>,
    end: usize,
}

/// The token at or after `i`, past whitespace and comments.
fn next_token(pdf: &[u8], mut i: usize) -> Option<Token> {
    loop {
        while i < pdf.len() && (pdf[i].is_ascii_whitespace() || pdf[i] == 0) {
            i += 1;
        }
        if pdf.get(i) != Some(&b'%') {
            break;
        }
        while i < pdf.len() && !matches!(pdf[i], b'\n' | b'\r') {
            i += 1;
        }
    }
    let at = |kind, end: usize| Token {
        kind,
        span: i..i,
        end,
    };
    let regular_end = |from: usize| {
        from + pdf[from..]
            .iter()
            .position(|&b| is_delimiter_or_space(b))
            .unwrap_or(pdf.len() - from)
    };
    Some(match *pdf.get(i)? {
        b'<' if pdf.get(i + 1) == Some(&b'<') => at(TokKind::DictOpen, i + 2),
        b'>' if pdf.get(i + 1) == Some(&b'>') => at(TokKind::DictClose, i + 2),
        b'[' => at(TokKind::ArrayOpen, i + 1),
        b']' => at(TokKind::ArrayClose, i + 1),
        b'(' => {
            let (mut depth, mut j) = (1usize, i + 1);
            while j < pdf.len() && depth > 0 {
                match pdf[j] {
                    b'\\' => j += 1,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let close = j.min(pdf.len());
            Token {
                kind: TokKind::Str,
                span: i + 1..close.saturating_sub(1).max(i + 1),
                end: close,
            }
        }
        b'<' => {
            let close = i + find(&pdf[i..], b">").unwrap_or(pdf.len() - i);
            Token {
                kind: TokKind::Str,
                span: i + 1..close,
                end: (close + 1).min(pdf.len()),
            }
        }
        b'/' => {
            let end = regular_end(i + 1);
            Token {
                kind: TokKind::Name,
                span: i + 1..end,
                end,
            }
        }
        _ => {
            let end = regular_end(i).max(i + 1);
            Token {
                kind: TokKind::Word,
                span: i..end,
                end,
            }
        }
    })
}

fn is_number(word: &[u8]) -> bool {
    word.iter().any(u8::is_ascii_digit)
        && word
            .iter()
            .all(|&b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.'))
}

/// An open container while scanning an object.
struct Frame {
    dict: bool,
    /// Dictionaries: the next token is a key.
    expect_key: bool,
    /// Dictionaries: the key of the value being read.
    key: Range<usize>,
    /// Inside a value C9 must not touch.
    kept: bool,
    /// Inside a width array.
    width: bool,
}

/// Whether the tokens from `tok` on are an `N G obj` header.
fn is_object_header(pdf: &[u8], tok: &Token) -> bool {
    let word = |t: &Token| (t.kind == TokKind::Word).then(|| &pdf[t.span.clone()]);
    let Some(g) = next_token(pdf, tok.end) else {
        return false;
    };
    let Some(o) = next_token(pdf, g.end) else {
        return false;
    };
    word(tok).is_some_and(is_number) && word(&g).is_some_and(is_number) && word(&o) == Some(b"obj")
}

/// Reads object `id` from its `N G obj` header at file position `at` up to
/// `stream`, `endstream`, `endobj` or the next `N G obj` header (whichever
/// comes first, at any depth: a file may lack `endobj`), and sorts its leaf
/// tokens into C9's outside-stream kinds. Keys, the values of [`C9_KEPT`]
/// keys, and top-level values other than a bare number are not leaves. `/W` is
/// a width array except in an xref stream (`xref`), where it holds the field
/// widths. Panics when `at` is not `id`'s header.
fn scan_object(pdf: &[u8], at: usize, id: (u32, u16), xref: bool) -> ObjectScan {
    let header = format!("{} {} obj", id.0, id.1);
    let mut i = at;
    let mut read = Vec::new();
    for _ in 0..3 {
        let tok = next_token(pdf, i).expect("object header");
        read.push(String::from_utf8_lossy(&pdf[tok.span.clone()]).into_owned());
        i = tok.end;
    }
    assert!(
        read.join(" ") == header && next_token(pdf, at).is_some_and(|t| t.span.start == at),
        "no `{header}` at {at}: the xref offset does not name the object"
    );
    let mut stack: Vec<Frame> = Vec::new();
    let mut leaves = Vec::new();
    let mut data_start = None;
    while let Some(tok) = next_token(pdf, i) {
        i = tok.end;
        let word = &pdf[tok.span.clone()];
        if tok.kind == TokKind::Word {
            if word == b"stream" {
                if stack.is_empty() {
                    data_start = Some(match &pdf[i..] {
                        [b'\r', b'\n', ..] => i + 2,
                        [b' ', b'\r', b'\n', ..] => i + 3,
                        [b' ', b'\n' | b'\r', ..] => i + 2,
                        [b'\n' | b'\r', ..] => i + 1,
                        _ => i,
                    });
                }
                break;
            }
            if word == b"endobj" || word == b"endstream" || is_object_header(pdf, &tok) {
                break;
            }
        }
        match tok.kind {
            TokKind::DictClose | TokKind::ArrayClose => {
                stack.pop();
                if let Some(f) = stack.last_mut()
                    && f.dict
                {
                    f.expect_key = true;
                }
                continue;
            }
            TokKind::Name if stack.last().is_some_and(|f| f.dict && f.expect_key) => {
                let f = stack.last_mut().expect("a dictionary");
                f.key = tok.span;
                f.expect_key = false;
                continue;
            }
            _ => {}
        }
        // A value: what holds it decides its kind.
        let (kept, width, site) = match stack.last() {
            None => (false, false, None),
            Some(f) if f.dict => {
                let key = &pdf[f.key.clone()];
                let width = key == b"Widths" || (key == b"W" && !xref);
                (
                    f.kept || C9_KEPT.contains(&key),
                    width,
                    Some(Leaf::DictValue),
                )
            }
            Some(f) => (
                f.kept,
                f.width,
                Some(if f.width { Leaf::Width } else { Leaf::Array }),
            ),
        };
        if matches!(tok.kind, TokKind::DictOpen | TokKind::ArrayOpen) {
            let dict = tok.kind == TokKind::DictOpen;
            stack.push(Frame {
                dict,
                expect_key: true,
                key: 0..0,
                kept,
                width: width && !dict,
            });
            continue;
        }
        let mut spans = vec![tok.span.clone()];
        // `N G R` is one value.
        if tok.kind == TokKind::Word
            && is_number(word)
            && let Some(g) = next_token(pdf, i)
            && let Some(r) = next_token(pdf, g.end)
            && g.kind == TokKind::Word
            && is_number(&pdf[g.span.clone()])
            && &pdf[r.span.clone()] == b"R"
        {
            spans.push(g.span);
            spans.push(r.span);
            i = r.end;
        }
        let leaf = match site {
            Some(leaf) => Some(leaf),
            // Top level: only a bare number.
            None if spans.len() == 1 && tok.kind == TokKind::Word && is_number(word) => {
                Some(Leaf::Number)
            }
            None => None,
        };
        if let Some(leaf) = leaf
            && !kept
        {
            leaves.extend(
                spans
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| (s, leaf)),
            );
        }
        if let Some(f) = stack.last_mut()
            && f.dict
        {
            f.expect_key = true;
        }
    }
    ObjectScan { leaves, data_start }
}

#[cfg(test)]
mod tests;
