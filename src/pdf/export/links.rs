//! Link annotations with a URI action, per page, for the layout's link pass
//! (T-32a pass 10, T-32b).
//!
//! A page's `/Annots` entries whose `/A` action is `/S /URI` give a
//! [`UriAnnot`]: the URI and the annotation's `/Rect` mapped into the space
//! [`crate::pdf::text`] gives glyphs in (crop box and `/Rotate` applied,
//! origin bottom left, y up), in milli-points. The mapping is hayro's own
//! page transform, the one the text extractor uses, so a link's rect and the
//! glyphs under it always agree. Nothing else about an annotation is read.

use std::panic::{AssertUnwindSafe, catch_unwind};

use hayro::kurbo::Point;
use hayro_interpret::TransformExt;
use hayro_syntax::Pdf;
use hayro_syntax::object::dict::keys::{A, ANNOTS, RECT, S, URI};
use hayro_syntax::object::{Array, Dict, Name, Rect};

use super::layout::UriAnnot;

/// URIs longer than this are not links: no real target needs 8 KiB.
const MAX_URI_BYTES: usize = 8 << 10;
/// Annotations read per page; more than this on one page is not a document.
const MAX_ANNOTS: usize = 10_000;
/// Rect corners beyond ±10⁶ pt are clamped (the `i32` holds ±2.1 × 10⁶ pt).
const COORD_LIMIT_MILLI: f64 = 1.0e9;

/// The URI links of every page of `bytes`, in hayro's page order (the order
/// [`crate::pdf::text::extract_text`] uses), each page's in `/Annots` order.
/// A file hayro cannot load, or a panic inside it, gives no links.
pub(crate) fn uri_annots(bytes: &[u8]) -> Vec<Vec<UriAnnot>> {
    catch_unwind(AssertUnwindSafe(|| read(bytes))).unwrap_or_default()
}

fn read(bytes: &[u8]) -> Vec<Vec<UriAnnot>> {
    let Ok(pdf) = Pdf::new(bytes.to_vec()) else {
        return Vec::new();
    };
    pdf.pages()
        .iter()
        .map(|page| {
            let Some(annots) = page.raw().get::<Array<'_>>(ANNOTS) else {
                return Vec::new();
            };
            let (_, height) = page.render_dimensions();
            let to_page = page.initial_transform(true).to_kurbo();
            let height = f64::from(height);
            annots
                .iter::<Dict<'_>>()
                .take(MAX_ANNOTS)
                .filter_map(|annot| {
                    let uri = uri_of(&annot)?;
                    let rect = annot.get::<Rect>(RECT)?;
                    // The transform is y down; glyphs are y up from the
                    // displayed page's bottom edge.
                    let corner = |x: f64, y: f64| {
                        let p = to_page * Point::new(x, y);
                        [milli(p.x), milli(height - p.y)]
                    };
                    let [x0, y0] = corner(rect.x0, rect.y0);
                    let [x1, y1] = corner(rect.x1, rect.y1);
                    Some(UriAnnot {
                        rect: [x0, y0, x1, y1],
                        uri,
                    })
                })
                .collect()
        })
        .collect()
}

/// The URI of an annotation whose action is `/S /URI`, if it is printable
/// text of a sane length.
fn uri_of(annot: &Dict<'_>) -> Option<String> {
    let action = annot.get::<Dict<'_>>(A)?;
    if action.get::<Name<'_>>(S)?.as_ref() != b"URI" {
        return None;
    }
    let raw = action.get::<hayro_syntax::object::String<'_>>(URI)?;
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_URI_BYTES {
        return None;
    }
    // ISO 32000 makes it 7-bit ASCII; UTF-8 is taken too, nothing else.
    let uri = std::str::from_utf8(bytes).ok()?.trim();
    (!uri.is_empty() && !uri.chars().any(char::is_control)).then(|| uri.to_owned())
}

/// Points to milli-points, ties to even, clamped (NaN becomes 0).
fn milli(v: f64) -> i32 {
    (v * 1000.0)
        .clamp(-COORD_LIMIT_MILLI, COORD_LIMIT_MILLI)
        .round_ties_even() as i32
}

#[cfg(test)]
pub(crate) use tests::with_link;

#[cfg(test)]
mod tests {
    use lopdf::{Dictionary, Document, Object, StringFormat};

    use super::*;
    use crate::pdf::fixtures::golden_pdf;

    /// `pdf` with a link annotation on page `page` (0-based) over `rect`.
    pub(crate) fn with_link(pdf: &[u8], page: usize, rect: [f32; 4], uri: &str) -> Vec<u8> {
        let mut doc = Document::load_mem(pdf).expect("fixture loads");
        let page_id = *doc.get_pages().values().nth(page).expect("page");
        let mut action = Dictionary::new();
        action.set("S", Object::Name(b"URI".to_vec()));
        action.set(
            "URI",
            Object::String(uri.as_bytes().to_vec(), StringFormat::Literal),
        );
        let mut annot = Dictionary::new();
        annot.set("Type", Object::Name(b"Annot".to_vec()));
        annot.set("Subtype", Object::Name(b"Link".to_vec()));
        annot.set(
            "Rect",
            Object::Array(rect.iter().map(|&v| Object::Real(v)).collect()),
        );
        annot.set("A", Object::Dictionary(action));
        let annot_id = doc.add_object(annot);
        let page = doc
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page dict");
        page.set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("saves");
        out
    }

    #[test]
    fn a_uri_link_is_read_in_glyph_space() {
        let pdf = with_link(
            &golden_pdf(),
            1,
            [70.0, 690.0, 300.0, 712.5],
            "https://example.org/a",
        );
        let links = uri_annots(&pdf);
        assert_eq!(links.len(), 2);
        assert!(links[0].is_empty());
        assert_eq!(
            links[1],
            vec![UriAnnot {
                rect: [70_000, 690_000, 300_000, 712_500],
                uri: "https://example.org/a".into(),
            }]
        );
    }

    #[test]
    fn other_actions_and_unloadable_files_give_nothing() {
        assert!(uri_annots(b"not a pdf").is_empty());
        let mut doc = Document::load_mem(&golden_pdf()).expect("loads");
        let page_id = *doc.get_pages().values().next().expect("page");
        let mut action = Dictionary::new();
        action.set("S", Object::Name(b"JavaScript".to_vec()));
        let mut annot = Dictionary::new();
        annot.set("Subtype", Object::Name(b"Link".to_vec()));
        annot.set(
            "Rect",
            Object::Array(vec![0.into(), 0.into(), 10.into(), 10.into()]),
        );
        annot.set("A", Object::Dictionary(action));
        let id = doc.add_object(annot);
        doc.get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .expect("page")
            .set("Annots", Object::Array(vec![Object::Reference(id)]));
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("saves");
        assert!(uri_annots(&out).iter().all(Vec::is_empty));
    }
}
