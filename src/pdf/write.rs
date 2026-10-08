//! The lopdf writer: the only place output and template documents are built
//! through lopdf's `Document` (plan §3.1). This is the first cut (T-03a); T-12a
//! finishes it with the remaining rules.
//!
//! Rules held here, each with a test below:
//! 1. a fresh `Document` for every write, never a re-saved loaded one, with a
//!    classic cross-reference **table** (lopdf defaults to an xref stream);
//! 2. `max_id` is set to the highest object number by hand (lopdf's `/Size` is
//!    `max_id + 1` and `set_object` never moves it);
//! 3. no object streams and no xref streams: nothing here calls `compress`,
//!    `save_modern`, `save_with_options` or `use_object_streams`;
//! 4. stream bytes are written exactly as given, with the caller's `/Filter`;
//! 5. the trailer holds only `/Root`, `/ID`, `/Info` (when given) and `/Size`.

use std::collections::BTreeMap;

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, Stream, StringFormat};

use crate::pdf::model::ObjId;

/// Why a write failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmitError {
    /// `finish` was called before `trailer`.
    #[error("no trailer was set")]
    NoTrailer,
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
        }
    }

    /// Object `id 0 obj`. A second `add` with the same `id` replaces the first.
    pub fn add(&mut self, id: u32, object: Object) {
        self.objects.insert(id, object);
    }

    /// A stream whose bytes are written exactly as given; `dict` carries the
    /// `/Filter` those bytes are already encoded with. `/Length` is set here.
    pub fn add_stream_raw(&mut self, id: u32, dict: Dictionary, bytes: Vec<u8>) {
        self.add(id, Object::Stream(Stream::new(dict, bytes)));
    }

    /// The trailer: `/Root root`, `/ID` = the first 16 bytes of `id_seed`
    /// twice, and `/Info` only when `info` is given.
    pub fn trailer(&mut self, root: ObjId, id_seed: [u8; 32], info: Option<ObjId>) {
        let mut id = [0u8; 16];
        id.copy_from_slice(&id_seed[..16]);
        self.trailer = Some(Trailer { root, id, info });
    }

    /// The file's bytes. The same objects give the same bytes on every run.
    pub fn finish(self) -> Result<Vec<u8>, EmitError> {
        let trailer = self.trailer.ok_or(EmitError::NoTrailer)?;
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
        Ok(out)
    }
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
}
