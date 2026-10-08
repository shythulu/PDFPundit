//! Object carver (T-05, T-07; TD §14): every object in a damaged PDF, found
//! by its `num gen obj` header in byte order rather than through the xref
//! table, which C2/C3/C10 damage removes.
//!
//! T-05's rules, each with tests in `tests`:
//! 1. **Header landmark** (`landmarks`): `num gen obj` with one whitespace
//!    byte between the parts, at the start of the buffer or after whitespace,
//!    assembled by walking back at most 24 bytes from `obj`. `endobj` tails
//!    are not headers. Numbers past `u32`, or a generation past 65,535, are
//!    ignored with [`CarveNote::HeaderOutOfRange`].
//! 2. **Byte-order assembly**: headers inside the data of a stream already
//!    resolved are dead, so keyword bytes in stream data never start an
//!    object. A value is parsed only up to the next header.
//! 3. **Extent ladder** for stream data: (a) a direct `/Length` that lands
//!    exactly on EOL + `endstream`; (b) the one `endstream` between the data
//!    and the next header that is framed as EOL + `endstream` + optional
//!    whitespace + `endobj` (two or more: nothing, carry on); (c) and (d)
//!    belong to T-07; (e) the last `endstream` before the next header; (f)
//!    the end of the file ([`LengthSource::TruncatedAtEof`]). The scanned rungs
//!    never look past the next header, so their extent holds no foreign
//!    header (qpdf's check). Rung (a) is trusted as it stands: an exact fit
//!    is how a stream whose data holds `1 0 obj` keeps its data.
//! 4. **EOL after `stream`**: LF, CRLF, a bare CR (with
//!    [`CarveNote::BareCrAfterStream`], D-030) or a space and then one of those.
//! 5. A missing `endobj` ends the object at the next header or EOF.
//! 6. Caps: [`MAX_LANDMARKS`] and [`MAX_OBJECTS`], each recorded with
//!    [`CarveNote::CapHit`] when reached.
// T-07 and T-09 are the first callers outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

mod landmarks;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::mem::size_of;
use std::ops::Range;

use lopdf::{Dictionary, Object};
use memchr::memmem;

use crate::engine::Cancelled;
use crate::pdf::lexer::{self, LexNote, Lexer, Tok};
use crate::pdf::model::{ByteSpan, LengthSource, ObjId, ObjectKind};
use crate::pdf::streams::{self, Filter, StreamClass};
use landmarks::Landmarks;

/// Most landmarks the scan keeps; the rest of the file is not looked at.
pub(crate) const MAX_LANDMARKS: usize = 1_000_000;
/// Most objects a carve holds (lopdf's cap).
pub(crate) const MAX_OBJECTS: usize = 1_000_000;
/// `cancel` is polled once per this many landmarks.
const POLL_EVERY: u64 = 256;
/// Bytes of a typeless stream decoded to classify it by content.
const CLASSIFY_CAP: usize = 1 << 20;

/// The `%PDF-` header, wherever it is (junk may come before it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderInfo {
    pub(crate) offset: u64,
    /// The digits and dots after `%PDF-`, e.g. `"1.7"`; `None` when there are none.
    pub(crate) version: Option<String>,
}

/// Everything the carve found, in byte order.
#[derive(Debug, Default)]
pub(crate) struct CarveReport {
    pub(crate) header: Option<HeaderInfo>,
    pub(crate) objects: Vec<CarvedObject>,
    pub(crate) orphans: Vec<Orphan>,
    /// From each classic `xref` keyword to the next landmark (`trailer`,
    /// `startxref`, `%%EOF`, a header) or EOF.
    pub(crate) xref_spans: Vec<ByteSpan>,
    /// Each `trailer` keyword and the dictionary after it.
    pub(crate) trailer_spans: Vec<ByteSpan>,
    /// Each `startxref` keyword's offset and the number after it, if any.
    pub(crate) startxref: Vec<(u64, Option<u64>)>,
    /// Where each `%%EOF` starts.
    pub(crate) eof_markers: Vec<u64>,
    /// Notes about the file rather than one object.
    pub(crate) notes: Vec<CarveNote>,
    pub(crate) stats: CarveStats,
}

/// One object, as its header declared it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CarvedObject {
    pub(crate) declared_id: ObjId,
    /// From the header's first digit to just past `endobj` (or to the next
    /// header or EOF when `endobj` is missing).
    pub(crate) span: ByteSpan,
    pub(crate) body: Body,
    pub(crate) kind: ObjectKind,
    pub(crate) origin: Origin,
    pub(crate) notes: Vec<CarveNote>,
}

/// What an object holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    Dict(Dictionary),
    /// `data` is the stream's raw (still filtered) bytes in the file.
    Stream {
        dict: Dictionary,
        data: ByteSpan,
        length_source: LengthSource,
    },
    /// Any other value: a number, name, string, array, reference or null.
    Primitive(Object),
    /// No value could be read after the header.
    Unparsed,
}

/// Where an object was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    TopLevel,
    /// Packed in this object stream (T-07).
    #[cfg_attr(test, allow(dead_code))]
    Compressed(ObjId),
}

/// A dictionary or stream with no header in front of it, found by the gap
/// sweep. T-07 adds the variants; until then the carve finds none.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Orphan {}

/// Something the carve tolerated or gave up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CarveNote {
    /// A header at `at` whose number overflows `u32` or whose generation
    /// exceeds 65,535; it was ignored.
    HeaderOutOfRange { at: u64 },
    /// `stream` was followed by a bare CR at `at`. qpdf, pdf.js, hayro and
    /// lopdf accept it; ISO 32000-1 7.3.8.1 does not (D-030).
    BareCrAfterStream { at: u64 },
    /// The direct `/Length` did not land on `endstream`.
    LengthMismatch { declared: i64 },
    /// Rung (b) found this many framed `endstream`s, so it chose none.
    EndstreamAmbiguous { candidates: u32 },
    /// No `endstream` before the next header: the data runs up to it.
    NoEndstream,
    /// No `endobj` before the next header: the object ends there (or at EOF).
    NoEndobj,
    /// A cap was reached and the rest was not carved.
    CapHit(Cap),
    /// The lexer tolerated something in the object's value.
    Lex(LexNote),
}

/// The carve's caps (rule 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cap {
    Landmarks,
    Objects,
}

/// The carve in numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CarveStats {
    /// Landmarks the scan kept.
    pub(crate) landmarks: u64,
    pub(crate) rungs: RungCounts,
}

/// Streams whose extent each rung of the ladder settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RungCounts {
    /// (a) the direct `/Length`.
    pub(crate) declared: u32,
    /// (b) the one framed `endstream`.
    pub(crate) unique_endstream: u32,
    /// (c) an indirect `/Length` (T-07).
    pub(crate) deferred_length: u32,
    /// (d) the inflate probe (T-07).
    pub(crate) inflate_probe: u32,
    /// (e) the last `endstream` before the next header.
    pub(crate) last_endstream: u32,
    /// (f) no `endstream`: EOF or the next header.
    pub(crate) eof: u32,
}

impl CarveReport {
    /// Capacity of every owned `Vec` and `String`, in bytes; a dictionary
    /// counts its map's entry and index slots.
    pub(crate) fn heap_bytes(&self) -> u64 {
        let header = self
            .header
            .as_ref()
            .and_then(|h| h.version.as_ref())
            .map_or(0, |v| v.capacity() as u64);
        let objects: u64 = self.objects.iter().map(CarvedObject::heap_bytes).sum();
        header
            + vec_bytes(&self.objects)
            + objects
            + vec_bytes(&self.orphans)
            + vec_bytes(&self.xref_spans)
            + vec_bytes(&self.trailer_spans)
            + vec_bytes(&self.startxref)
            + vec_bytes(&self.eof_markers)
            + vec_bytes(&self.notes)
    }
}

impl CarvedObject {
    /// Heap bytes this object owns (not its own slot in the list).
    fn heap_bytes(&self) -> u64 {
        let body = match &self.body {
            Body::Dict(d) | Body::Stream { dict: d, .. } => dict_heap(d),
            Body::Primitive(o) => object_heap(o),
            Body::Unparsed => 0,
        };
        let kind = match &self.kind {
            ObjectKind::Other(s) => s.capacity() as u64,
            _ => 0,
        };
        body + kind + vec_bytes(&self.notes)
    }
}

fn vec_bytes<T>(v: &Vec<T>) -> u64 {
    (v.capacity() * size_of::<T>()) as u64
}

fn object_heap(o: &Object) -> u64 {
    match o {
        Object::Name(v) | Object::String(v, _) => v.capacity() as u64,
        Object::Array(a) => vec_bytes(a) + a.iter().map(object_heap).sum::<u64>(),
        Object::Dictionary(d) => dict_heap(d),
        Object::Stream(s) => dict_heap(&s.dict) + s.content.capacity() as u64,
        _ => 0,
    }
}

/// An `IndexMap`'s entries (hash, key, value) and its index table, plus what
/// each key and value owns.
fn dict_heap(d: &Dictionary) -> u64 {
    let map = d.as_hashmap();
    let slot = size_of::<(u64, Vec<u8>, Object)>() + size_of::<usize>() + 1;
    let entries: u64 = map
        .iter()
        .map(|(k, v)| k.capacity() as u64 + object_heap(v))
        .sum();
    (map.capacity() * slot) as u64 + entries
}

/// Counts landmarks and polls `cancel` once every [`POLL_EVERY`].
struct Poll<'a> {
    cancel: &'a dyn Fn() -> bool,
    steps: u64,
}

impl Poll<'_> {
    fn tick(&mut self) -> Result<(), Cancelled> {
        let due = self.steps.is_multiple_of(POLL_EVERY);
        self.steps += 1;
        if due && (self.cancel)() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The caps, apart so tests can reach them on small inputs.
#[derive(Debug, Clone, Copy)]
struct Caps {
    landmarks: usize,
    objects: usize,
}

const CAPS: Caps = Caps {
    landmarks: MAX_LANDMARKS,
    objects: MAX_OBJECTS,
};

/// Carves `buf`. Never fails but by cancellation: damage shows up as notes,
/// `Unparsed` bodies and scanned extents. `cancel` is polled every 256
/// landmarks.
pub(crate) fn carve(buf: &[u8], cancel: &dyn Fn() -> bool) -> Result<CarveReport, Cancelled> {
    carve_with(buf, cancel, CAPS)
}

fn carve_with(buf: &[u8], cancel: &dyn Fn() -> bool, caps: Caps) -> Result<CarveReport, Cancelled> {
    let mut poll = Poll { cancel, steps: 0 };
    let lm = landmarks::scan(buf, caps.landmarks, &mut poll)?;
    let mut report = CarveReport {
        header: header_info(buf),
        stats: CarveStats {
            landmarks: lm.count as u64,
            rungs: RungCounts::default(),
        },
        ..CarveReport::default()
    };
    if lm.capped {
        report.notes.push(CarveNote::CapHit(Cap::Landmarks));
    }
    let dead = assemble(buf, &lm, caps.objects, &mut poll, &mut report)?;
    structure(buf, &lm, &dead, &mut report);
    Ok(report)
}

fn header_info(buf: &[u8]) -> Option<HeaderInfo> {
    let offset = memmem::find(buf, b"%PDF-")?;
    let rest = &buf[offset + 5..];
    let len = rest
        .iter()
        .take(16)
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    let version = (len > 0).then(|| String::from_utf8_lossy(&rest[..len]).into_owned());
    Some(HeaderInfo {
        offset: offset as u64,
        version,
    })
}

/// Phase B (TD §14.2): every live header in byte order. Returns the data
/// ranges of the streams carved, sorted: the dead ranges.
fn assemble(
    buf: &[u8],
    lm: &Landmarks,
    max_objects: usize,
    poll: &mut Poll<'_>,
    report: &mut CarveReport,
) -> Result<Vec<Range<usize>>, Cancelled> {
    let mut dead = Vec::new();
    let mut cursor = 0;
    for (i, h) in lm.headers.iter().enumerate() {
        poll.tick()?;
        if h.at < cursor {
            continue;
        }
        if report.objects.len() == max_objects {
            report.notes.push(CarveNote::CapHit(Cap::Objects));
            break;
        }
        let next = lm
            .headers
            .get(i + 1)
            .map_or(buf.len(), |n| n.at.max(h.obj_end));
        let obj = carve_object(buf, lm, h, next, &mut report.stats.rungs);
        if let Body::Stream { data, .. } = &obj.body {
            dead.push(data.start as usize..data.end as usize);
        }
        cursor = obj.span.end as usize;
        report.objects.push(obj);
    }
    Ok(dead)
}

/// The object whose header is `h`; `next` is where the next header starts
/// (or EOF), the end of the window its value is read in.
fn carve_object(
    buf: &[u8],
    lm: &Landmarks,
    h: &landmarks::Header,
    next: usize,
    rungs: &mut RungCounts,
) -> CarvedObject {
    let window = &buf[..next];
    let mut notes = Vec::new();
    let (value, after) = match lexer::parse_value(window, h.obj_end, 0) {
        Ok(p) => {
            notes.extend(p.notes.into_iter().map(CarveNote::Lex));
            (Some(p.value), p.end)
        }
        Err(_) => (None, h.obj_end),
    };
    let mut lx = Lexer::new(window, after);
    let stream_follows =
        matches!(value, None | Some(Object::Dictionary(_))) && lx.next() == Tok::Kw(&b"stream"[..]);
    let (body, kind, end) = if stream_follows {
        let dict = match value {
            Some(Object::Dictionary(d)) => d,
            _ => Dictionary::new(),
        };
        let ext = extent(buf, lm, &dict, lx.pos, &mut notes, rungs);
        let end = close(buf, lm, ext.resume, &mut notes);
        let kind = stream_kind(&dict, &buf[ext.data.clone()]);
        let body = Body::Stream {
            dict,
            data: span(ext.data),
            length_source: ext.source,
        };
        (body, kind, end)
    } else {
        let end = close(buf, lm, after, &mut notes);
        let (body, kind) = match value {
            Some(Object::Dictionary(d)) => {
                let kind = dict_kind(&d);
                (Body::Dict(d), kind)
            }
            Some(o) => (Body::Primitive(o), ObjectKind::Other(String::new())),
            None => (Body::Unparsed, ObjectKind::Other(String::new())),
        };
        (body, kind, end)
    };
    CarvedObject {
        declared_id: (h.num, h.gen_nr),
        span: span(h.at..end),
        body,
        kind,
        origin: Origin::TopLevel,
        notes,
    }
}

fn span(r: Range<usize>) -> ByteSpan {
    ByteSpan {
        start: r.start as u64,
        end: r.end as u64,
    }
}

/// Where the next header at or after `from` starts.
fn next_header(lm: &Landmarks, from: usize) -> Option<usize> {
    let i = lm.headers.partition_point(|h| h.at < from);
    lm.headers.get(i).map(|h| h.at)
}

/// The landmarks in `list` within `range`.
fn within(list: &[usize], range: Range<usize>) -> &[usize] {
    let lo = list.partition_point(|&at| at < range.start);
    let hi = list.partition_point(|&at| at < range.end);
    &list[lo..hi.max(lo)]
}

/// Rule 5: just past the first `endobj` from `from` before the next header;
/// without one, the next header or EOF.
fn close(buf: &[u8], lm: &Landmarks, from: usize, notes: &mut Vec<CarveNote>) -> usize {
    let next = next_header(lm, from).unwrap_or(buf.len());
    match within(&lm.endobjs, from..next).first() {
        Some(&at) => at + b"endobj".len(),
        None => {
            notes.push(CarveNote::NoEndobj);
            next
        }
    }
}

/// A stream's data and how its end was settled. `resume` is where `endobj`
/// is looked for: past `endstream` when there is one.
struct Extent {
    data: Range<usize>,
    source: LengthSource,
    resume: usize,
}

/// Rule 3, the extent ladder. `kw_end` is just past the `stream` keyword.
fn extent(
    buf: &[u8],
    lm: &Landmarks,
    dict: &Dictionary,
    kw_end: usize,
    notes: &mut Vec<CarveNote>,
    rungs: &mut RungCounts,
) -> Extent {
    let start = data_start(buf, kw_end, notes);
    let region_end = next_header(lm, start).unwrap_or(buf.len());
    let scanned = |end: usize, at: usize| Extent {
        data: start..end,
        source: LengthSource::ScannedEndstream,
        resume: at + b"endstream".len(),
    };

    // (a) A direct /Length that lands exactly on EOL + `endstream`.
    if let Ok(&Object::Integer(declared)) = dict.get(b"Length") {
        if let Some((end, resume)) = declared_fit(buf, start, declared) {
            rungs.declared += 1;
            return Extent {
                data: start..end,
                source: LengthSource::Declared,
                resume,
            };
        }
        notes.push(CarveNote::LengthMismatch { declared });
    }

    // (b) The unique-endstream rule (lopdf's definition of the data's end).
    let candidates = within(&lm.endstreams, start..region_end);
    let framed: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&at| framed_endstream(buf, at))
        .collect();
    match framed.as_slice() {
        [at] => {
            rungs.unique_endstream += 1;
            return scanned(strip_eol(buf, start, *at), *at);
        }
        [] => {}
        many => notes.push(CarveNote::EndstreamAmbiguous {
            candidates: u32::try_from(many.len()).unwrap_or(u32::MAX),
        }),
    }

    // (c) The deferred indirect /Length and (d) the inflate probe are T-07's.

    // (e) The last `endstream` before the next header.
    if let Some(&at) = candidates.last() {
        rungs.last_endstream += 1;
        return scanned(strip_eol(buf, start, at), at);
    }

    // (f) No `endstream` at all: the data runs to EOF, or up to the next
    // header, never past it.
    rungs.eof += 1;
    let end = if region_end == buf.len() {
        region_end
    } else {
        notes.push(CarveNote::NoEndstream);
        strip_eol(buf, start, region_end)
    };
    Extent {
        data: start..end,
        source: LengthSource::TruncatedAtEof,
        resume: region_end,
    }
}

/// Rule 4: where the data starts after `stream` (ending at `kw_end`): past
/// CRLF, LF or a bare CR, each optionally after one space. Anything else:
/// right after the keyword.
fn data_start(buf: &[u8], kw_end: usize, notes: &mut Vec<CarveNote>) -> usize {
    let eol_at = |i: usize| matches!(buf.get(i), Some(b'\r' | b'\n'));
    let i = if buf.get(kw_end) == Some(&b' ') && eol_at(kw_end + 1) {
        kw_end + 1
    } else {
        kw_end
    };
    match (buf.get(i), buf.get(i + 1)) {
        (Some(b'\r'), Some(b'\n')) => i + 2,
        (Some(b'\n'), _) => i + 1,
        (Some(b'\r'), _) => {
            notes.push(CarveNote::BareCrAfterStream { at: i as u64 });
            i + 1
        }
        _ => kw_end,
    }
}

/// The length of the EOL at `at` (CRLF, LF or CR), 0 when there is none.
fn eol_len(buf: &[u8], at: usize) -> usize {
    match (buf.get(at), buf.get(at + 1)) {
        (Some(b'\r'), Some(b'\n')) => 2,
        (Some(b'\r' | b'\n'), _) => 1,
        _ => 0,
    }
}

/// Rung (a): `start + declared` followed by an optional EOL and `endstream`.
/// Returns the data's end and the offset just past `endstream`.
fn declared_fit(buf: &[u8], start: usize, declared: i64) -> Option<(usize, usize)> {
    let end = start.checked_add(usize::try_from(declared).ok()?)?;
    if end > buf.len() {
        return None;
    }
    let kw = end + eol_len(buf, end);
    buf[kw..]
        .starts_with(b"endstream")
        .then_some((end, kw + b"endstream".len()))
}

/// Rung (b)'s framing: an EOL right before `endstream` at `at`, and after it
/// optional whitespace and `endobj`.
fn framed_endstream(buf: &[u8], at: usize) -> bool {
    let eol_before = at > 0 && matches!(buf[at - 1], b'\r' | b'\n');
    let after = at + b"endstream".len();
    let ws = buf[after..]
        .iter()
        .take_while(|&&b| lexer::is_ws(b))
        .count();
    eol_before && buf[after + ws..].starts_with(b"endobj")
}

/// `at` less the EOL right before it, never before `start`.
fn strip_eol(buf: &[u8], start: usize, at: usize) -> usize {
    let end = if at >= 2 && &buf[at - 2..at] == b"\r\n" {
        at - 2
    } else if at >= 1 && matches!(buf[at - 1], b'\r' | b'\n') {
        at - 1
    } else {
        at
    };
    end.max(start).min(at)
}

/// A dictionary's kind from its `/Type`.
fn dict_kind(d: &Dictionary) -> ObjectKind {
    match name(d, b"Type") {
        Some(b"Catalog") => ObjectKind::Catalog,
        Some(b"Pages") => ObjectKind::Pages,
        Some(b"Page") => ObjectKind::Page,
        Some(b"Font") => ObjectKind::Font,
        Some(b"FontDescriptor") => ObjectKind::FontDescriptor,
        Some(b"Sig") => ObjectKind::Sig,
        _ => named(d),
    }
}

/// A stream's kind: from its dictionary when that decides it (T-06's
/// [`streams::classify`]), else from the first [`CLASSIFY_CAP`] bytes of its
/// decoded data (TD §4.2 step 5: a typeless stream is classified by content).
fn stream_kind(dict: &Dictionary, raw: &[u8]) -> ObjectKind {
    let class = match streams::classify(dict, &[]) {
        StreamClass::Other => streams::classify(dict, &decoded_prefix(dict, raw)),
        class => class,
    };
    match class {
        StreamClass::Content => ObjectKind::ContentStream,
        StreamClass::Image { .. } => ObjectKind::Image,
        StreamClass::Form => ObjectKind::Form,
        StreamClass::FontFile => ObjectKind::FontFile,
        StreamClass::CMap if name(dict, b"Type") != Some(b"CMap") => ObjectKind::ToUnicode,
        StreamClass::ObjStm => ObjectKind::ObjStm,
        StreamClass::XRef => ObjectKind::XRefStream,
        StreamClass::CMap | StreamClass::Metadata | StreamClass::Other => named(dict),
    }
}

/// What classification reads: the raw bytes when unfiltered; for Flate
/// alone, whatever inflates before the cap or the first error (so a C9
/// stream still classifies); otherwise the whole chain, or nothing.
fn decoded_prefix<'a>(dict: &Dictionary, raw: &'a [u8]) -> Cow<'a, [u8]> {
    let chain = streams::filters_of(dict);
    match chain.as_slice() {
        [] => Cow::Borrowed(raw),
        [(Filter::Flate, _)] => Cow::Owned(streams::inflate(raw, CLASSIFY_CAP).out),
        _ => Cow::Owned(streams::decode_chain(raw, &chain, CLASSIFY_CAP).unwrap_or_default()),
    }
}

/// `ObjectKind::Other` named by `/Type`, else `/Subtype`, else empty.
fn named(d: &Dictionary) -> ObjectKind {
    let label = name(d, b"Type")
        .or_else(|| name(d, b"Subtype"))
        .map_or_else(String::new, |n| format!("/{}", String::from_utf8_lossy(n)));
    ObjectKind::Other(label)
}

fn name<'d>(d: &'d Dictionary, key: &[u8]) -> Option<&'d [u8]> {
    d.get(key).ok().and_then(|o| o.as_name().ok())
}

/// The file-structure landmarks outside stream data (`dead`), for diagnosis:
/// xref and trailer spans, `startxref` values, `%%EOF` markers, and a note
/// for each out-of-range header.
fn structure(buf: &[u8], lm: &Landmarks, dead: &[Range<usize>], report: &mut CarveReport) {
    let live = |at: &&usize| {
        let i = dead.partition_point(|r| r.start <= **at);
        i == 0 || **at >= dead[i - 1].end
    };
    report.notes.extend(
        lm.bad_headers
            .iter()
            .filter(live)
            .map(|&at| CarveNote::HeaderOutOfRange { at: at as u64 }),
    );
    report.eof_markers = lm.eofs.iter().filter(live).map(|&at| at as u64).collect();
    report.startxref = lm
        .startxrefs
        .iter()
        .filter(live)
        .map(|&at| {
            let value = match Lexer::new(buf, at + b"startxref".len()).next() {
                Tok::Int(n) => u64::try_from(n).ok(),
                _ => None,
            };
            (at as u64, value)
        })
        .collect();
    report.trailer_spans = lm
        .trailers
        .iter()
        .filter(live)
        .map(|&at| {
            let from = at + b"trailer".len();
            let end = match lexer::parse_value(buf, from, 0) {
                Ok(p) if matches!(p.value, Object::Dictionary(_)) => p.end,
                _ => from,
            };
            span(at..end)
        })
        .collect();
    let first_after = |list: &[usize], at: usize| {
        let i = list.partition_point(|&x| x <= at);
        list.get(i).copied()
    };
    report.xref_spans = lm
        .xrefs
        .iter()
        .filter(live)
        .map(|&at| {
            let end = [
                first_after(&lm.trailers, at),
                first_after(&lm.startxrefs, at),
                first_after(&lm.eofs, at),
                first_after(&lm.xrefs, at),
                next_header(lm, at + 1),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(buf.len());
            span(at..end)
        })
        .collect();
}
