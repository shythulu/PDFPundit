//! The runtime template builder (T-29, TD §5.1, D-021, D-027).
//!
//! A template is the one-page carrier PDF of one database font: a `/Type0`
//! `Identity-H` font whose `CIDFontType2` descendant has `/CIDToGIDMap
//! /Identity`, a `/W` array from the font's `.gmap`, the full, non-subset
//! program as `/FontFile2`, and a `/ToUnicode` over every glyph the `.gmap`
//! names. Template assembly (T-30) harvests that subtree from object
//! [`TYPE0_FONT`] down.
//!
//! Templates are never files (D-021): [`build`] makes one in memory through
//! [`crate::pdf::write`], and [`bundled`] builds each bundled font's template
//! on first use and keeps it for the life of the process. Every byte depends on
//! the font and its `.gmap` alone, so a template is the same on every run.

// T-28 (FontDb) and T-30 (template assembly) are the first callers outside the
// tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

use lopdf::{Dictionary, Object};
use sha2::{Digest, Sha256};

use super::build::{BuildError, build_from_ttf, build_tounicode};
use super::gmap::{GmapError, GmapTable, Source};
use crate::pdf::write::{EmitError, Writer};

const CATALOG: u32 = 1;
const PAGES: u32 = 2;
const PAGE: u32 = 3;
const CONTENTS: u32 = 4;
/// The template's `/Type0` font: the root of the subtree T-30 harvests.
pub(crate) const TYPE0_FONT: u32 = 5;
const CIDFONT: u32 = 6;
const DESCRIPTOR: u32 = 7;
const FONTFILE2: u32 = 8;
const TOUNICODE: u32 = 9;

/// The page's font resource name.
pub(crate) const FONT_RESOURCE: &[u8] = b"F1";

/// The blocks a bundled font must map in full (the tool refuses a font that
/// does not): Basic Latin, the Latin-1 Supplement and Latin Extended-A,
/// without their control characters. The template page draws them.
pub(crate) const LATIN_COVERAGE: [(char, char); 3] = [
    ('\u{20}', '\u{7e}'),
    ('\u{a0}', '\u{ff}'),
    ('\u{100}', '\u{17f}'),
];

/// Why a template could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TemplateError {
    /// The program is not a font [`build_from_ttf`] accepts.
    #[error(transparent)]
    Font(#[from] BuildError),
    /// The writer refused the document.
    #[error("template not written: {0}")]
    Write(#[from] EmitError),
}

/// Characters per line on the template page.
const LINE_LEN: usize = 32;

/// The template of `font_bytes`, a TrueType program, with widths and
/// `/ToUnicode` from `gmap`, the font's `.gmap`.
///
/// The descriptor metrics, flags and `/BaseFont` are the ones
/// [`build_from_ttf`] writes into the font's index entry, so a template and
/// its `fontindex.json` entry always agree. The page draws every character of
/// [`LATIN_COVERAGE`] the `.gmap` maps, [`LINE_LEN`] to a line.
pub(crate) fn build(font_bytes: &[u8], gmap: &GmapTable) -> Result<Vec<u8>, TemplateError> {
    let (entry, _) = build_from_ttf(font_bytes)?;
    let base_font = || Object::Name(entry.postscript_name.clone().into_bytes());

    let mut w = Writer::with_version("1.7");
    w.add(
        CATALOG,
        dict(vec![("Type", name(b"Catalog")), ("Pages", r(PAGES))]),
    );
    w.add(
        PAGES,
        dict(vec![
            ("Type", name(b"Pages")),
            ("Kids", Object::Array(vec![r(PAGE)])),
            ("Count", Object::Integer(1)),
        ]),
    );
    let font_resources = dict(vec![("F1", r(TYPE0_FONT))]);
    w.add(
        PAGE,
        dict(vec![
            ("Type", name(b"Page")),
            ("Parent", r(PAGES)),
            ("MediaBox", ints(&[0, 0, 612, 792])),
            ("Resources", dict(vec![("Font", font_resources)])),
            ("Contents", r(CONTENTS)),
        ]),
    );
    w.add_stream_raw(CONTENTS, flate_dict(vec![]), flate(&content(gmap)));

    w.add(
        TYPE0_FONT,
        dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"Type0")),
            ("BaseFont", base_font()),
            ("Encoding", name(b"Identity-H")),
            ("DescendantFonts", Object::Array(vec![r(CIDFONT)])),
            ("ToUnicode", r(TOUNICODE)),
        ]),
    );
    let system_info = dict(vec![
        ("Registry", Object::string_literal("Adobe")),
        ("Ordering", Object::string_literal("Identity")),
        ("Supplement", Object::Integer(0)),
    ]);
    w.add(
        CIDFONT,
        dict(vec![
            ("Type", name(b"Font")),
            ("Subtype", name(b"CIDFontType2")),
            ("BaseFont", base_font()),
            ("CIDSystemInfo", system_info),
            ("FontDescriptor", r(DESCRIPTOR)),
            ("W", widths(gmap)),
            ("CIDToGIDMap", name(b"Identity")),
        ]),
    );
    w.add(
        DESCRIPTOR,
        dict(vec![
            ("Type", name(b"FontDescriptor")),
            ("FontName", base_font()),
            ("Flags", Object::Integer(i64::from(entry.flags))),
            ("FontBBox", ints(&entry.bbox.map(i64::from))),
            (
                "ItalicAngle",
                Object::Integer(i64::from(entry.italic_angle)),
            ),
            ("Ascent", Object::Integer(i64::from(entry.ascent))),
            ("Descent", Object::Integer(i64::from(entry.descent))),
            ("CapHeight", Object::Integer(i64::from(entry.cap_height))),
            // No program-independent stem width exists for a TrueType font;
            // 80 is the value the goldens carry.
            ("StemV", Object::Integer(80)),
            ("FontFile2", r(FONTFILE2)),
        ]),
    );
    let length1 = i64::try_from(font_bytes.len()).unwrap_or(i64::MAX);
    w.add_stream_raw(
        FONTFILE2,
        flate_dict(vec![("Length1", Object::Integer(length1))]),
        flate(font_bytes),
    );
    w.add_stream_raw(TOUNICODE, flate_dict(vec![]), flate(&tounicode(gmap)));

    let mut seed = Sha256::new();
    seed.update(b"pdfpundit template");
    seed.update(font_bytes);
    seed.update(gmap_bytes(gmap));
    w.trailer((CATALOG, 0), seed.finalize().into(), None);
    // The writer refuses only a missing trailer, set above, and an output
    // above 4 GiB, which no font program reaches.
    Ok(w.finish()?)
}

/// `/W`: one `c [w1 w2 …]` entry per run of consecutive glyph ids, each
/// glyph's width its first `.gmap` record's.
fn widths(gmap: &GmapTable) -> Object {
    let mut by_gid: BTreeMap<u16, u16> = BTreeMap::new();
    for record in gmap.records() {
        by_gid.entry(record.gid()).or_insert(record.width());
    }
    let mut out = Vec::new();
    let mut run: Vec<Object> = Vec::new();
    let mut next = None;
    for (&gid, &width) in &by_gid {
        if next != Some(u32::from(gid)) {
            if !run.is_empty() {
                out.push(Object::Array(std::mem::take(&mut run)));
            }
            out.push(Object::Integer(i64::from(gid)));
        }
        run.push(Object::Integer(i64::from(width)));
        next = Some(u32::from(gid) + 1);
    }
    if !run.is_empty() {
        out.push(Object::Array(run));
    }
    Object::Array(out)
}

/// The full-coverage `/ToUnicode`: every glyph the `.gmap` names, to the code
/// point [`GmapTable::unicode`] gives it.
fn tounicode(gmap: &GmapTable) -> Vec<u8> {
    let mut map: BTreeMap<u16, char> = BTreeMap::new();
    for record in gmap.records() {
        map.entry(record.gid()).or_insert(record.unicode());
    }
    build_tounicode(&map)
}

/// The page: [`LATIN_COVERAGE`] in 12-point lines, one `Tj` each.
fn content(gmap: &GmapTable) -> Vec<u8> {
    let gid_of = cmap_gids(gmap);
    let gids: Vec<u16> = LATIN_COVERAGE
        .iter()
        .flat_map(|&(lo, hi)| lo..=hi)
        .filter_map(|c| gid_of.get(&c).copied())
        .collect();
    let mut s = String::from("BT\n/F1 12 Tf\n16 TL\n72 720 Td\n");
    for line in gids.chunks(LINE_LEN) {
        s.push('<');
        for gid in line {
            let _ = write!(s, "{gid:04X}");
        }
        s.push_str("> Tj\nT*\n");
    }
    s.push_str("ET\n");
    s.into_bytes()
}

/// code point → glyph over the `.gmap`'s `cmap` records, the lowest glyph
/// first where several glyphs claim one code point.
fn cmap_gids(gmap: &GmapTable) -> BTreeMap<char, u16> {
    let mut out = BTreeMap::new();
    for record in gmap.records() {
        if record.source() == Source::Cmap {
            out.entry(record.unicode()).or_insert(record.gid());
        }
    }
    out
}

fn gmap_bytes<'a>(gmap: &GmapTable<'a>) -> &'a [u8] {
    bytemuck::cast_slice(gmap.records())
}

fn name(s: &[u8]) -> Object {
    Object::Name(s.to_vec())
}

fn r(id: u32) -> Object {
    Object::Reference((id, 0))
}

fn ints(values: &[i64]) -> Object {
    Object::Array(values.iter().map(|&v| Object::Integer(v)).collect())
}

fn dict(entries: Vec<(&str, Object)>) -> Object {
    Object::Dictionary(dict_of(entries))
}

fn dict_of(entries: Vec<(&str, Object)>) -> Dictionary {
    let mut d = Dictionary::new();
    for (k, v) in entries {
        d.set(k, v);
    }
    d
}

fn flate_dict(mut entries: Vec<(&str, Object)>) -> Dictionary {
    entries.push(("Filter", name(b"FlateDecode")));
    dict_of(entries)
}

/// zlib at level 6: the same bytes on every platform and run.
fn flate(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6)
}

// ── the bundled fonts ────────────────────────────────────────────────────

/// One font the binary carries (D-021): its index id, program and `.gmap`.
pub(crate) struct BundledFont {
    pub(crate) id: &'static str,
    pub(crate) ttf: &'static [u8],
    pub(crate) gmap: &'static [u8],
}

/// `assets/fontindex.json`, written by `tools/build-templates`.
pub(crate) const BUNDLED_INDEX: &[u8] = include_bytes!("../../../assets/fontindex.json");

/// The bundled fonts in `fontindex.json` order (a test checks the order, the
/// ids and the hashes against the index).
pub(crate) const BUNDLED: [BundledFont; 2] = [
    BundledFont {
        id: "NotoSans-Regular",
        ttf: include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf"),
        gmap: include_bytes!("../../../assets/gmaps/NotoSans-Regular.gmap"),
    },
    BundledFont {
        id: "NotoSerif-Regular",
        ttf: include_bytes!("../../../assets/fonts/NotoSerif-Regular.ttf"),
        gmap: include_bytes!("../../../assets/gmaps/NotoSerif-Regular.gmap"),
    },
];

/// Why a bundled template could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BundledError {
    #[error("no bundled font {0}")]
    Unknown(String),
    #[error("bundled gmap of {id}: {err}")]
    Gmap { id: String, err: GmapError },
    #[error("bundled font {id}: {err}")]
    Build { id: String, err: TemplateError },
}

/// The template of bundled font `id`, built on first use and kept for the
/// life of the process.
pub(crate) fn bundled(id: &str) -> Result<&'static [u8], BundledError> {
    static CACHE: [OnceLock<Result<Vec<u8>, BundledError>>; BUNDLED.len()] =
        [const { OnceLock::new() }; BUNDLED.len()];
    let at = BUNDLED
        .iter()
        .position(|f| f.id == id)
        .ok_or_else(|| BundledError::Unknown(id.to_owned()))?;
    let font = &BUNDLED[at];
    CACHE[at]
        .get_or_init(|| {
            let gmap = GmapTable::new(font.gmap).map_err(|err| BundledError::Gmap {
                id: font.id.to_owned(),
                err,
            })?;
            build(font.ttf, &gmap).map_err(|err| BundledError::Build {
                id: font.id.to_owned(),
                err,
            })
        })
        .as_ref()
        .map(Vec::as_slice)
        .map_err(Clone::clone)
}

/// The bundled database's hash, what `RepairReport.font_db_sha256` records:
/// SHA-256 over `fontindex.json` and then every `.gmap` in index order.
pub(crate) fn bundled_sha256() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(BUNDLED_INDEX);
    for font in &BUNDLED {
        h.update(font.gmap);
    }
    h.finalize().into()
}

#[cfg(test)]
mod tests;
