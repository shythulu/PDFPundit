//! Programmatic PDF fixtures and corruptors (T-03a; T-03b adds the ObjStm
//! golden, the C9 corruptor and the adversarial builders).
//!
//! The goldens are built through [`crate::pdf::write`], read their glyph ids and
//! widths from the committed test font through `skrifa::raw` when they are
//! built, and are byte-identical on every run. [`corrupt`] reproduces REPDF's
//! measured induction for each structural class (map #7): it edits bytes only,
//! never re-serialises, and is deterministic per seed.

use std::collections::BTreeMap;
use std::ops::Range;

use lopdf::xref::XrefEntry;
use lopdf::{Dictionary, Document, Object, StringFormat};
use sha2::{Digest, Sha256};
use skrifa::raw::{FontRef, TableProvider};

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

/// `corrupt` has no corruptor for this class yet (C9 until T-03b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("this corruptor is not built yet")]
pub struct NotYet;

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
/// - C10: truncated to 70.0% of its length (rounded down).
///
/// C9 returns `Err(NotYet)` until T-03b. The same `(class, pdf, seed)` always
/// gives the same bytes. `pdf` is a fixture: a classic-xref file such as
/// [`golden_pdf`]; this panics if it lacks the structure the class edits.
pub fn corrupt(class: CorruptionClass, pdf: &[u8], seed: u64) -> Result<Vec<u8>, NotYet> {
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
        CorruptionClass::C9ZlibTampered => return Err(NotYet),
        CorruptionClass::C10Truncated => pdf[..pdf.len() * 7 / 10].to_vec(),
    };
    Ok(out)
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

/// Every golden object; `signed` adds the signature field with this
/// `/ByteRange`.
fn golden_writer(signed: Option<[i64; 4]>) -> Writer {
    let facts = font_facts();
    let mut w = Writer::with_version("1.7");

    let mut catalog = dict(vec![("Type", name(b"Catalog")), ("Pages", r(PAGES))]);
    if signed.is_some() {
        let acroform = dict(vec![
            ("Fields", Object::Array(vec![r(SIG_FIELD)])),
            ("SigFlags", int(3)),
        ]);
        catalog.set("AcroForm", Object::Dictionary(acroform));
    }
    w.add(CATALOG, Object::Dictionary(catalog));
    w.add(
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
        w.add(page, Object::Dictionary(d));
        let filter = dict(vec![("Filter", name(b"FlateDecode"))]);
        w.add_stream_raw(contents, filter, flate(&content(&facts, i)));
    }

    w.add(
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
    w.add(
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
    w.add(
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
    w.add_stream_raw(
        FONTFILE2,
        dict(vec![
            ("Length1", int(TEST_FONT.len() as i64)),
            ("Filter", name(b"FlateDecode")),
        ]),
        flate(TEST_FONT),
    );
    w.add_stream_raw(
        TOUNICODE,
        dict(vec![("Filter", name(b"FlateDecode"))]),
        flate(&tounicode(&facts)),
    );
    w.add_stream_raw(
        IMAGE,
        dict(vec![
            ("Type", name(b"XObject")),
            ("Subtype", name(b"Image")),
            ("Width", int(8)),
            ("Height", int(8)),
            ("ColorSpace", name(b"DeviceRGB")),
            ("BitsPerComponent", int(8)),
            ("Filter", name(b"DCTDecode")),
        ]),
        TINY_JPEG.to_vec(),
    );

    if let Some(byte_range) = signed {
        w.add(
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
        w.add(
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

/// The first byte after `%PDF-x.y` and the binary-mark comment line.
fn after_header(pdf: &[u8]) -> usize {
    let first = find(pdf, b"\n").expect("header line") + 1;
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
        Some(XrefEntry::Normal { offset, .. }) => *offset as usize,
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
            Some(XrefEntry::Normal { offset, .. }) => Some((id, *offset as usize)),
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

#[cfg(test)]
mod tests;
