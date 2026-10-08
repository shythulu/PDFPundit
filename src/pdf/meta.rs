//! File metadata (T-11a): what the file says about itself.
//!
//! - **Version**: the `x.y` after the `%PDF-` the carve found, wherever it
//!   is; `None` when there is none or it is not `\d\.\d`.
//! - **Pages and sizes**: the pages of T-10's flat page tree, in its
//!   document order, each with its MediaBox (its own, an ancestor's, the
//!   other pages' modal box, then A4, the shipped `default_page_size`) as
//!   `(|x1 - x0|, |y1 - y0|)` rounded to whole points.
//! - **Title**: `/Title` of the document information dictionary. That is the
//!   `/Info` of the last xref stream that names one; a classic trailer is not
//!   in the carve's objects, so otherwise it is the last dictionary in byte
//!   order that nothing references, has no `/Type`, and carries a `/Title`.
//!   Text strings decode as UTF-16BE or UTF-8 by their byte-order mark, else
//!   as PDFDocEncoding (lopdf's `decode_text_string`).
#![cfg_attr(not(test), allow(dead_code))]

use lopdf::{Dictionary, Object};

use crate::engine::PageSize;
use crate::pdf::carver::{Body, CarveReport};
use crate::pdf::graph::{ObjectGraph, winning_copies};
use crate::pdf::model::{FileMeta, ObjId};
use crate::pdf::rebuild::{plan_ids, rebuild_page_tree};

/// The file's version, pages, page sizes and title (module docs). `graph`
/// must be built from `carve`.
pub(crate) fn file_meta(carve: &CarveReport, graph: &ObjectGraph) -> FileMeta {
    let remap = plan_ids(carve, graph);
    let tree = rebuild_page_tree(carve, graph, &remap, PageSize::A4);
    FileMeta {
        version: version(carve),
        pages: u32::try_from(tree.pages.len()).unwrap_or(u32::MAX),
        title: title(carve, graph),
        page_sizes: tree.pages.iter().map(|p| size(&p.mediabox)).collect(),
    }
}

fn version(carve: &CarveReport) -> Option<String> {
    let v = carve.header.as_ref()?.version.as_deref()?;
    match v.as_bytes() {
        [a, b'.', b, ..] if a.is_ascii_digit() && b.is_ascii_digit() => Some(v[..3].to_owned()),
        _ => None,
    }
}

/// A box's width and height in whole points, halves rounded up.
fn size(mediabox: &[Object; 4]) -> (u32, u32) {
    let n: [f64; 4] = mediabox.each_ref().map(|o| match *o {
        Object::Integer(i) => i as f64,
        Object::Real(r) => f64::from(r),
        _ => 0.0,
    });
    let whole = |a: f64, b: f64| {
        // `as` saturates, and maps NaN to 0.
        ((a - b).abs() + 0.5) as u32
    };
    (whole(n[2], n[0]), whole(n[3], n[1]))
}

fn title(carve: &CarveReport, graph: &ObjectGraph) -> Option<String> {
    let winners = winning_copies(carve);
    let dict_of = |id: ObjId| match &carve.objects.get(*winners.get(&id)?)?.body {
        Body::Dict(d) => Some(d),
        _ => None,
    };
    let declared = carve
        .xref_streams
        .iter()
        .rev()
        .find_map(|x| x.trailer.info.as_ref()?.as_reference().ok())
        .and_then(dict_of);
    let info: &Dictionary = match declared {
        Some(d) => d,
        None => {
            winners
                .iter()
                .filter_map(|(&id, &at)| {
                    let Body::Dict(d) = &carve.objects[at].body else {
                        return None;
                    };
                    let info = !d.has(b"Type") && d.has(b"Title") && graph.referrers(id).is_empty();
                    info.then_some((at, d))
                })
                .max_by_key(|&(at, _)| at)?
                .1
        }
    };
    let value = match info.get(b"Title").ok()? {
        Object::Reference(id) => match &carve.objects.get(*winners.get(id)?)?.body {
            Body::Primitive(v) => v,
            _ => return None,
        },
        v => v,
    };
    let text = lopdf::decode_text_string(value).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    (!text.is_empty()).then(|| text.to_owned())
}
