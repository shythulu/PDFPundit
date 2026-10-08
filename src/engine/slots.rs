//! `AnalysisResult.font_slots` (T-14): every font resource a page's text
//! selects, as the analysis found it.
//!
//! - **Which slots**: per page in document order, every name a `Tf` selects
//!   in the page's content or a Form XObject it draws, in name order. A slot
//!   is listed once per page.
//! - **Its font**: the slot in the `/Font` of the resources in force for the
//!   stream that selects it, else in the page's own. A slot no resources map
//!   (C6) is listed with no name, no subtype, not embedded and no
//!   `/ToUnicode`.
//! - **`base_font`, `subtype`**: the names as written, with their `/`.
//! - **`embedded`**: a Type 3 font always is; otherwise the descriptor (a
//!   Type 0 font's is its first descendant's) names a `/FontFile*` stream
//!   that holds something other than spaces.
//! - **`tounicode`**: `Present` when `/ToUnicode` names a stream that decodes
//!   to a CMap with `bfchar` or `bfrange`, `Missing` when there is none or
//!   it names no stream, `Unparsable` otherwise.
//! - **`glyph_count`**: the codes the page shows through the slot: the bytes
//!   of the strings shown while it is selected, two to a code for a Type 0
//!   font.

use std::borrow::Cow;
use std::collections::BTreeMap;

use lopdf::{Dictionary, Object};

use super::{FontSlot, ToUnicodeState};
use crate::pdf::carver::{Body, CarveReport};
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::model::{ByteSpan, ObjId};
use crate::pdf::streams::salvage::{CarveSource, SalvageIndex};
use crate::pdf::streams::{DEFAULT_CAP, content_ops};

/// The font slots of `bytes` (module docs). `carve`, `graph` and `salvage`
/// must come from `bytes`.
pub(super) fn font_slots(
    bytes: &[u8],
    carve: &CarveReport,
    graph: &ObjectGraph,
    salvage: &SalvageIndex,
) -> Vec<FontSlot> {
    let file = File {
        bytes,
        carve,
        winners: winning_copies(carve),
        source: CarveSource::new(carve, bytes),
        salvage,
    };
    let mut out = Vec::new();
    for (index, page) in graph.pages_in_doc_order().into_iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let pieces = graph.page_content(carve, page, |id| file.decoded(id));
        let own = pieces
            .iter()
            .find(|p| p.via.is_empty())
            .map(|p| &p.resources.dict);
        // Per slot: its font dictionary (if mapped) and the bytes shown.
        let mut slots: BTreeMap<Vec<u8>, (Option<Dictionary>, u64)> = BTreeMap::new();
        for piece in &pieces {
            let Some(content) = file.decoded(piece.stream) else {
                continue;
            };
            let mut current: Option<Vec<u8>> = None;
            for op in content_ops(&content) {
                let shown = match op.op {
                    b"Tf" => {
                        if let [.., Object::Name(name), _] = op.operands.as_slice() {
                            slots.entry(name.clone()).or_insert_with(|| {
                                let font = [Some(&piece.resources.dict), own]
                                    .into_iter()
                                    .flatten()
                                    .find_map(|r| file.font_in(r, name));
                                (font, 0)
                            });
                            current = Some(name.clone());
                        }
                        continue;
                    }
                    b"Tj" | b"'" | b"\"" => match op.operands.last() {
                        Some(Object::String(s, _)) => s.len() as u64,
                        _ => 0,
                    },
                    b"TJ" => match op.operands.last() {
                        Some(Object::Array(items)) => items
                            .iter()
                            .map(|o| match o {
                                Object::String(s, _) => s.len() as u64,
                                _ => 0,
                            })
                            .sum(),
                        _ => 0,
                    },
                    _ => continue,
                };
                if let Some(entry) = current.as_ref().and_then(|c| slots.get_mut(c)) {
                    entry.1 = entry.1.saturating_add(shown);
                }
            }
        }
        out.extend(
            slots
                .into_iter()
                .map(|(name, (font, shown))| file.slot(index, &name, font.as_ref(), shown)),
        );
    }
    out
}

/// The input and what analysis built from it.
struct File<'a> {
    bytes: &'a [u8],
    carve: &'a CarveReport,
    winners: BTreeMap<ObjId, usize>,
    source: CarveSource<'a>,
    salvage: &'a SalvageIndex,
}

impl<'a> File<'a> {
    fn decoded(&self, id: ObjId) -> Option<Cow<'a, [u8]>> {
        self.salvage.decoded(&self.source, id, DEFAULT_CAP).ok()
    }

    fn body(&self, id: ObjId) -> Option<&'a Body> {
        let at = *self.winners.get(&id)?;
        self.carve.objects.get(at).map(|o| &o.body)
    }

    /// `v` as a dictionary: itself, or the dictionary or stream dictionary
    /// it references.
    fn dict(&self, v: &Object) -> Option<Dictionary> {
        match v {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(id) => match self.body(*id)? {
                Body::Dict(d) | Body::Stream { dict: d, .. } => Some(d.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The font `resources` maps `slot` to.
    fn font_in(&self, resources: &Dictionary, slot: &[u8]) -> Option<Dictionary> {
        let fonts = self.dict(resources.get(b"Font").ok()?)?;
        self.dict(fonts.get(slot).ok()?)
    }

    /// The raw data of stream `id`, if it is one.
    fn raw(&self, id: ObjId) -> Option<&'a [u8]> {
        match self.body(id)? {
            Body::Stream { data, .. } => Some(slice(self.bytes, *data)),
            _ => None,
        }
    }

    fn slot(&self, page: u32, name: &[u8], font: Option<&Dictionary>, shown: u64) -> FontSlot {
        let name_of = |key: &[u8]| {
            let n = font?.get(key).ok()?.as_name().ok()?;
            Some(format!("/{}", String::from_utf8_lossy(n)))
        };
        let subtype = name_of(b"Subtype");
        let width = if subtype.as_deref() == Some("/Type0") {
            2
        } else {
            1
        };
        FontSlot {
            page,
            slot: String::from_utf8_lossy(name).into_owned(),
            base_font: name_of(b"BaseFont"),
            embedded: font.is_some_and(|f| self.embedded(f, subtype.as_deref())),
            tounicode: font.map_or(ToUnicodeState::Missing, |f| self.tounicode(f)),
            subtype,
            glyph_count: u32::try_from(shown / width).unwrap_or(u32::MAX),
            resolution: None,
        }
    }

    fn embedded(&self, font: &Dictionary, subtype: Option<&str>) -> bool {
        let descriptor_of = |f: &Dictionary| self.dict(f.get(b"FontDescriptor").ok()?);
        let descriptor = match subtype {
            Some("/Type3") => return true,
            Some("/Type0") => font
                .get(b"DescendantFonts")
                .ok()
                .and_then(|v| match v {
                    Object::Array(a) => a.first().cloned(),
                    Object::Reference(id) => match self.body(*id)? {
                        Body::Primitive(Object::Array(a)) => a.first().cloned(),
                        _ => None,
                    },
                    _ => None,
                })
                .and_then(|d| self.dict(&d))
                .and_then(|d| descriptor_of(&d)),
            _ => descriptor_of(font),
        };
        let Some(descriptor) = descriptor else {
            return false;
        };
        [&b"FontFile"[..], b"FontFile2", b"FontFile3"]
            .iter()
            .filter_map(|k| descriptor.get(k).ok()?.as_reference().ok())
            .filter_map(|id| self.raw(id))
            .any(|raw| raw.iter().any(|&b| b != b' '))
    }

    fn tounicode(&self, font: &Dictionary) -> ToUnicodeState {
        let id = match font.get(b"ToUnicode") {
            Err(_) => return ToUnicodeState::Missing,
            Ok(Object::Reference(id)) => *id,
            Ok(_) => return ToUnicodeState::Unparsable,
        };
        if self.raw(id).is_none() {
            return ToUnicodeState::Missing;
        }
        let cmap = self.decoded(id).is_some_and(|b| {
            [&b"beginbfchar"[..], b"beginbfrange"]
                .iter()
                .any(|k| memchr::memmem::find(&b, k).is_some())
        });
        if cmap {
            ToUnicodeState::Present
        } else {
            ToUnicodeState::Unparsable
        }
    }
}

fn slice(bytes: &[u8], span: ByteSpan) -> &[u8] {
    let range = usize::try_from(span.start)
        .ok()
        .zip(usize::try_from(span.end).ok());
    range.and_then(|(s, e)| bytes.get(s..e)).unwrap_or(&[])
}
