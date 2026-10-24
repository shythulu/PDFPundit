//! The lopdf writer: the only place output and template documents are built
//! through lopdf's `Document` (plan §3.1; T-03a, T-12a). What goes into a
//! repaired file is decided in `emit`; this module only writes it.
//!
//! The writer rules (FR-06 §4), each with a test below:
//! 1. `Document::with_version`, then a classic cross-reference **table**
//!    (lopdf defaults to an xref stream);
//! 2. `max_id` is set to the highest object number by hand after the last
//!    `set_object` (lopdf's `/Size` is `max_id + 1` and `set_object` never
//!    moves it), so gapped numbers give separate xref subsections;
//! 3. no object streams and no xref streams: nothing here calls `compress`,
//!    `save_modern`, `save_with_options`, `use_object_streams` or
//!    `use_xref_streams`;
//! 4. stream bytes are written exactly as given, with the caller's `/Filter`
//!    ([`Writer::add_stream_raw`]), or as plain bytes with no filter
//!    ([`Writer::add_stream_uncompressed`]); nothing is ever recompressed;
//! 5. a fresh `Document` for every write, never a re-saved loaded one; the
//!    trailer holds only `/Root`, `/ID`, `/Info` (when given) and `/Size`;
//! 6. `Object::Real` is `f32` and lopdf writes NaN and infinity as `NaN` and
//!    `inf`, which no reader parses: [`Writer::add`] writes them as `0` and
//!    counts them ([`Writer::non_finite_reals`]) for the caller to log;
//! 7. an output above [`MAX_OUTPUT_BYTES`] is refused with
//!    [`EmitError::TooLarge`];
//! 8. the header is `%PDF-` + the version, LF, then `%` and four bytes at or
//!    above 0x80 (lopdf's own binary marker).

use std::collections::BTreeMap;

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, Stream, StringFormat};

use crate::pdf::model::ObjId;

/// The largest output written: 4 GiB (rule 7).
pub const MAX_OUTPUT_BYTES: u64 = 4 << 30;

/// Why a write failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmitError {
    /// `finish` was called before `trailer`.
    #[error("no trailer was set")]
    NoTrailer,
    /// The output would be above [`MAX_OUTPUT_BYTES`]; `bytes` is how much
    /// was counted when it was refused.
    #[error("the output would be {bytes} bytes, above the 4 GiB limit")]
    TooLarge { bytes: u64 },
    /// lopdf refused to serialise the document.
    #[error("write failed: {0}")]
    Write(String),
}

/// Builds one PDF from objects with chosen numbers. Every object has
/// generation 0.
#[derive(Debug, Clone)]
pub struct Writer {
    version: String,
    objects: BTreeMap<u32, Object>,
    trailer: Option<Trailer>,
    /// Non-finite reals written as `0` (rule 6).
    non_finite: u32,
    /// [`MAX_OUTPUT_BYTES`], lowered by the tests.
    limit: u64,
}

#[derive(Debug, Clone)]
struct Trailer {
    root: ObjId,
    id: [u8; 16],
    info: Option<ObjId>,
}

impl Writer {
    /// An empty document with header version `version` (e.g. `"1.7"`).
    pub fn with_version(version: &str) -> Writer {
        Writer {
            version: version.to_owned(),
            objects: BTreeMap::new(),
            trailer: None,
            non_finite: 0,
            limit: MAX_OUTPUT_BYTES,
        }
    }

    /// Object `id 0 obj`. A second `add` with the same `id` replaces the first.
    /// A NaN or infinite real anywhere in it is written as `0` (rule 6).
    pub fn add(&mut self, id: u32, mut object: Object) {
        self.non_finite = self.non_finite.saturating_add(finite_only(&mut object));
        self.objects.insert(id, object);
    }

    /// A stream whose bytes are written exactly as given; `dict` carries the
    /// `/Filter` those bytes are already encoded with. `/Length` is set here.
    pub fn add_stream_raw(&mut self, id: u32, dict: Dictionary, bytes: Vec<u8>) {
        self.add(id, Object::Stream(Stream::new(dict, bytes)));
    }

    /// A stream whose bytes are written exactly as given and carry no
    /// filter: `/Filter` and `/DecodeParms` are dropped from `dict`.
    /// `/Length` is set here.
    pub fn add_stream_uncompressed(&mut self, id: u32, mut dict: Dictionary, bytes: Vec<u8>) {
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
        self.add_stream_raw(id, dict, bytes);
    }

    /// How many NaN or infinite reals [`Self::add`] has written as `0`.
    pub fn non_finite_reals(&self) -> u32 {
        self.non_finite
    }

    /// The writer with a lower output limit, so rule 7 is testable without
    /// 4 GiB of objects.
    #[cfg(test)]
    pub(crate) fn with_limit(mut self, limit: u64) -> Writer {
        self.limit = limit;
        self
    }

    /// The trailer: `/Root root`, `/ID` = the first 16 bytes of `id_seed`
    /// twice, and `/Info` only when `info` is given.
    pub fn trailer(&mut self, root: ObjId, id_seed: [u8; 32], info: Option<ObjId>) {
        let mut id = [0u8; 16];
        id.copy_from_slice(&id_seed[..16]);
        self.trailer = Some(Trailer { root, id, info });
    }

    /// The file's bytes. The same objects give the same bytes on every run.
    /// Above the limit it is [`EmitError::TooLarge`]: refused before
    /// serialising when the stream data alone passes it, else after.
    pub fn finish(self) -> Result<Vec<u8>, EmitError> {
        let trailer = self.trailer.ok_or(EmitError::NoTrailer)?;
        let limit = self.limit;
        let stream_bytes: u64 = self
            .objects
            .values()
            .map(|o| match o {
                Object::Stream(s) => s.content.len() as u64,
                _ => 0,
            })
            .sum();
        if stream_bytes > limit {
            return Err(EmitError::TooLarge {
                bytes: stream_bytes,
            });
        }
        let mut doc = Document::with_version(self.version);
        doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
        let max_id = self.objects.keys().next_back().copied().unwrap_or(0);
        for (id, object) in self.objects {
            doc.set_object((id, 0), object);
        }
        doc.max_id = max_id;

        let mut t = Dictionary::new();
        t.set("Root", Object::Reference(trailer.root));
        let id = Object::String(trailer.id.to_vec(), StringFormat::Hexadecimal);
        t.set("ID", Object::Array(vec![id.clone(), id]));
        if let Some(info) = trailer.info {
            t.set("Info", Object::Reference(info));
        }
        doc.trailer = t;

        let mut out = Vec::new();
        doc.save_to(&mut out)
            .map_err(|e| EmitError::Write(e.to_string()))?;
        if out.len() as u64 > limit {
            return Err(EmitError::TooLarge {
                bytes: out.len() as u64,
            });
        }
        Ok(out)
    }
}

/// Replaces every NaN or infinite real in `v` (a stream's dictionary, not its
/// data) with `0`; returns how many there were.
fn finite_only(v: &mut Object) -> u32 {
    match v {
        Object::Real(r) if !r.is_finite() => {
            *v = Object::Integer(0);
            1
        }
        Object::Array(a) => a.iter_mut().map(finite_only).fold(0, u32::saturating_add),
        Object::Dictionary(d) => finite_only_dict(d),
        Object::Stream(s) => finite_only_dict(&mut s.dict),
        _ => 0,
    }
}

fn finite_only_dict(d: &mut Dictionary) -> u32 {
    d.iter_mut()
        .map(|(_, v)| finite_only(v))
        .fold(0, u32::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::LoadOptions;

    fn load_strict(bytes: &[u8]) -> Document {
        let opts = LoadOptions {
            strict: true,
            max_decompressed_size: Some(1 << 20),
            ..LoadOptions::default()
        };
        Document::load_mem_with_options(bytes, opts).expect("strict load")
    }

    fn count(hay: &[u8], needle: &[u8]) -> usize {
        hay.windows(needle.len()).filter(|w| *w == needle).count()
    }

    /// The bytes from the last `trailer` keyword to the end.
    fn trailer_text(bytes: &[u8]) -> String {
        let at = bytes
            .windows(7)
            .rposition(|w| w == b"trailer")
            .expect("trailer keyword");
        String::from_utf8_lossy(&bytes[at..]).into_owned()
    }

    fn catalog(w: &mut Writer, id: u32, pages: u32) {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Catalog".to_vec()));
        d.set("Pages", Object::Reference((pages, 0)));
        w.add(id, Object::Dictionary(d));
    }

    fn pages(w: &mut Writer, id: u32) {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Pages".to_vec()));
        d.set("Kids", Object::Array(vec![]));
        d.set("Count", Object::Integer(0));
        w.add(id, Object::Dictionary(d));
    }

    fn minimal() -> Writer {
        let mut w = Writer::with_version("1.7");
        catalog(&mut w, 1, 2);
        pages(&mut w, 2);
        w.trailer((1, 0), [7; 32], None);
        w
    }

    #[test]
    fn header_is_the_version_then_four_high_bytes() {
        let out = minimal().finish().unwrap();
        assert!(out.starts_with(b"%PDF-1.7\n%"));
        assert!(out[10..14].iter().all(|&b| b >= 0x80), "{:?}", &out[10..14]);
        assert_eq!(out[14], b'\n');
    }

    #[test]
    fn classic_xref_table_and_no_object_or_xref_streams() {
        let out = minimal().finish().unwrap();
        assert_eq!(count(&out, b"\nxref\n"), 1);
        assert_eq!(count(&out, b"XRef"), 0);
        assert_eq!(count(&out, b"ObjStm"), 0);
        load_strict(&out);
    }

    #[test]
    fn gapped_numbers_give_size_from_the_highest_and_separate_subsections() {
        let mut w = Writer::with_version("1.7");
        catalog(&mut w, 5, 9);
        pages(&mut w, 9);
        w.add(20, Object::Integer(1));
        w.add(30, Object::Integer(2));
        w.trailer((5, 0), [1; 32], None);
        let out = w.finish().unwrap();
        assert!(
            trailer_text(&out).contains("/Size 31"),
            "{}",
            trailer_text(&out)
        );
        for sub in ["\n0 1\n", "\n5 1\n", "\n9 1\n", "\n20 1\n", "\n30 1\n"] {
            assert_eq!(count(&out, sub.as_bytes()), 1, "subsection {sub:?}");
        }
        let doc = load_strict(&out);
        assert_eq!(doc.get_object((30, 0)).unwrap(), &Object::Integer(2));
    }

    #[test]
    fn trailer_holds_only_root_id_and_size() {
        let out = minimal().finish().unwrap();
        let doc = load_strict(&out);
        let keys: Vec<&[u8]> = doc.trailer.iter().map(|(k, _)| k.as_slice()).collect();
        assert_eq!(keys, [b"Root".as_slice(), b"ID", b"Size"]);
        let id = Object::String(vec![7; 16], StringFormat::Hexadecimal);
        assert_eq!(
            doc.trailer.get(b"ID").unwrap(),
            &Object::Array(vec![id.clone(), id])
        );
    }

    #[test]
    fn info_is_written_only_when_given() {
        let mut w = minimal();
        let mut info = Dictionary::new();
        info.set("Title", Object::string_literal("t"));
        w.add(3, Object::Dictionary(info));
        w.trailer((1, 0), [7; 32], Some((3, 0)));
        let doc = load_strict(&w.finish().unwrap());
        assert_eq!(
            doc.trailer.get(b"Info").unwrap(),
            &Object::Reference((3, 0))
        );
    }

    #[test]
    fn raw_stream_bytes_and_filter_are_kept() {
        let mut w = minimal();
        let mut d = Dictionary::new();
        d.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        let bytes = vec![0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
        w.add_stream_raw(3, d, bytes.clone());
        let out = w.finish().unwrap();
        let doc = load_strict(&out);
        let s = doc.get_object((3, 0)).unwrap().as_stream().unwrap();
        assert_eq!(s.content, bytes);
        assert_eq!(s.dict.get(b"Length").unwrap(), &Object::Integer(8));
        assert_eq!(
            s.dict.get(b"Filter").unwrap(),
            &Object::Name(b"FlateDecode".to_vec())
        );
    }

    #[test]
    fn finish_without_a_trailer_fails() {
        let mut w = Writer::with_version("1.7");
        catalog(&mut w, 1, 2);
        assert_eq!(w.finish(), Err(EmitError::NoTrailer));
    }

    #[test]
    fn same_objects_give_the_same_bytes() {
        assert_eq!(minimal().finish().unwrap(), minimal().finish().unwrap());
    }

    #[test]
    fn rule_1_the_version_is_the_header_and_the_xref_is_a_table() {
        let out = minimal().finish().unwrap();
        assert!(out.starts_with(b"%PDF-1.7\n"));
        let doc = load_strict(&out);
        assert_eq!(doc.version, "1.7");
        assert!(matches!(
            doc.reference_table.cross_reference_type,
            XrefType::CrossReferenceTable
        ));
    }

    #[test]
    fn rule_4_an_uncompressed_stream_has_no_filter() {
        let mut w = minimal();
        let mut d = Dictionary::new();
        d.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        d.set("DecodeParms", Object::Dictionary(Dictionary::new()));
        d.set("Subtype", Object::Name(b"Form".to_vec()));
        w.add_stream_uncompressed(3, d, b"q Q".to_vec());
        let doc = load_strict(&w.finish().unwrap());
        let s = doc.get_object((3, 0)).unwrap().as_stream().unwrap();
        assert_eq!(s.content, b"q Q");
        assert!(!s.dict.has(b"Filter"));
        assert!(!s.dict.has(b"DecodeParms"));
        assert_eq!(s.dict.get(b"Length").unwrap(), &Object::Integer(3));
        assert_eq!(
            s.dict.get(b"Subtype").unwrap(),
            &Object::Name(b"Form".to_vec())
        );
    }

    #[test]
    fn rule_6_non_finite_reals_are_written_as_zero_and_counted() {
        let mut w = minimal();
        let mut d = Dictionary::new();
        d.set(
            "BBox",
            Object::Array(vec![
                Object::Real(f32::NAN),
                Object::Real(0.5),
                Object::Real(f32::INFINITY),
                Object::Real(f32::NEG_INFINITY),
            ]),
        );
        w.add_stream_raw(3, d, Vec::new());
        w.add(4, Object::Real(1.25));
        assert_eq!(w.non_finite_reals(), 3);
        let out = w.finish().unwrap();
        assert_eq!(count(&out, b"NaN"), 0);
        assert_eq!(count(&out, b"inf"), 0);
        let doc = load_strict(&out);
        let s = doc.get_object((3, 0)).unwrap().as_stream().unwrap();
        assert_eq!(
            s.dict.get(b"BBox").unwrap(),
            &Object::Array(vec![
                Object::Integer(0),
                Object::Real(0.5),
                Object::Integer(0),
                Object::Integer(0),
            ])
        );
        assert_eq!(doc.get_object((4, 0)).unwrap(), &Object::Real(1.25));
    }

    #[test]
    fn rule_7_the_limit_is_four_gib() {
        assert_eq!(MAX_OUTPUT_BYTES, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn rule_7_stream_data_above_the_limit_is_refused_before_writing() {
        let mut w = minimal().with_limit(1000);
        w.add_stream_raw(3, Dictionary::new(), vec![b' '; 1001]);
        assert_eq!(w.finish(), Err(EmitError::TooLarge { bytes: 1001 }));
    }

    #[test]
    fn rule_7_an_output_above_the_limit_is_refused() {
        let out = minimal().finish().unwrap();
        let at = out.len() as u64;
        assert_eq!(
            minimal().with_limit(at - 1).finish(),
            Err(EmitError::TooLarge { bytes: at })
        );
        assert_eq!(minimal().with_limit(at).finish(), Ok(out));
    }

    /// What rules 3 and 5 ban: an object or xref stream, a compression pass,
    /// or a loaded document to re-save.
    const BANNED: [&str; 8] = [
        ".compress(",
        "save_modern",
        "save_with_options",
        "use_object_streams",
        "use_xref_streams",
        "XrefType::CrossReferenceStream",
        "Document::load",
        "Document::new(",
    ];

    /// The [`BANNED`] names that `src`'s code uses outside its tests module
    /// and outside `//` comment lines.
    fn banned_uses(src: &str) -> Vec<&'static str> {
        let code: String = crate::line_endings::lf(src)
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect();
        BANNED.into_iter().filter(|b| code.contains(b)).collect()
    }

    /// Rules 3 and 5: nothing outside the tests of this module or of `emit`
    /// asks lopdf for an object or xref stream, a compression pass, or a
    /// loaded document to re-save (a cheap tripwire; the review is the
    /// guard).
    #[test]
    fn rules_3_and_5_no_compression_no_streams_no_resave() {
        for (name, src) in [
            ("write.rs", include_str!("write.rs")),
            ("emit.rs", include_str!("emit.rs")),
        ] {
            let hits = banned_uses(src);
            assert!(hits.is_empty(), "{name} uses {hits:?}");
        }
    }

    /// A CRLF checkout scans the same (CI-01): the tests module is still cut
    /// off, so its own list of banned names is not a hit, and a banned call
    /// in the code still is.
    #[test]
    fn the_rules_3_and_5_scan_reads_a_crlf_checkout() {
        use crate::line_endings::crlf;
        let src = include_str!("write.rs");
        assert!(banned_uses(&crlf(src)).is_empty());
        let planted = crlf(&format!("fn f(d: &mut Doc) {{ d.compress(); }}\n{src}"));
        assert_eq!(banned_uses(&planted), [".compress("]);
    }
}
