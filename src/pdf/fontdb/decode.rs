//! Decode through the document's own fonts (T-27c, RR change #2, FR-03).
//!
//! [`decode_codes`] turns a font slot's character codes into text with only
//! what the document itself still carries. Each code climbs the ladder on its
//! own and the first rung that gives text wins:
//! 1. the font's `/ToUnicode` CMap;
//! 2. simple fonts: the `/Differences` name for the code, else (for a code
//!    `/Differences` does not cover; its name replaces the base one) the name
//!    in the base table (`/Encoding`'s name or `/BaseEncoding`;
//!    `StandardEncoding` when neither is given, the font is not symbolic and
//!    its program has no built-in encoding), through [`glyph_name_to_unicode`]
//!    (the ITC Zapf Dingbats list first for ZapfDingbats);
//! 3. the embedded program, read through `skrifa::raw` (D-035):
//!    - a TrueType program's glyph is found, for a simple font, by the
//!      `/Differences` name in `post`, else through its raw (3,0) subtable
//!      (code 0xF000, 0x0000, 0xF100 then 0xF200 plus the code), (1,0)
//!      subtable or, for a symbolic font, (3,1) subtable; for a CIDFontType2
//!      it is found through `/CIDToGIDMap` (a present but unreadable map
//!      skips this rung); the glyph is named by the Unicode `cmap` subtables read
//!      backwards (the lowest code point they map to it, the most
//!      comprehensive subtable winning where they disagree), else by its
//!      `post` name, else by the (1,0) subtable read backwards through
//!      `MacRomanEncoding`;
//!    - a bare CFF program's glyph is found through its built-in encoding for
//!      a simple font whose code `/Differences` does not cover (a covered
//!      code's glyph is named by the name that did not decode, so it stops
//!      here) and through its charset (CID → glyph) for a CID-keyed
//!      one, whose glyph must also lie in a subfont (`subfont_index`, the
//!      FDSelect); a name-keyed glyph is named by its charset string;
//! 4. composite fonts: the CID's character collection (`/CIDSystemInfo`, else
//!    the CFF program's ROS), through Adobe's UCS2 CMap for Japan1, GB1, CNS1
//!    or Korea1.
//!
//! A composite code whose glyph the embedded program lacks (glyph 0, past the
//! glyph count when `maxp` gives one, or a CID outside the charset) draws
//! `.notdef`, so it skips rung 4. A Type0 `/Encoding` CMap gets each code
//! without its byte length: a code up to 0xFF is looked up as two bytes,
//! then as one. A code no rung decodes gives an empty [`Decoded`]; a rung whose
//! text is only NUL or U+FFFD counts as a miss. Type 1 programs (`/FontFile`)
//! are not read: such a font gets the `StandardEncoding` default.
//!
//! `font` is the slot's font dictionary (for a composite font, the Type0 one)
//! with its indirect references resolved in place: `/ToUnicode`, the
//! descriptor's font file, `/CIDToGIDMap` and a Type0 `/Encoding` CMap are
//! `Object::Stream`s holding their raw, still-filtered data, which this module
//! decodes through [`decode_chain`]. A reference left in place counts as a
//! missing object, which is what a damaged one is, except that a
//! `/CIDToGIDMap` left as one is unknown rather than the default identity.

// T-28 (scorer) and T-30 (template assembly) are the first callers outside
// the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::BTreeMap;

use hayro_interpret::hayro_cmap::{BfString, CMap, CMapName, CidFamily, load_embedded};
use lopdf::{Dictionary, Object};
use skrifa::GlyphId;
use skrifa::raw::ps::cff::{CffFontRef, dict};
use skrifa::raw::ps::string::Sid;
use skrifa::raw::tables::cff::Cff;
use skrifa::raw::tables::cmap::{CmapSubtable, PlatformId};
use skrifa::raw::{FontData, FontRead, FontRef, TableProvider};

use super::agl::{Decoded, Prov, glyph_name_to_unicode, zapf_dingbats_glyph_name_to_unicode};
use super::build::unicode_cmap;
use crate::pdf::streams::{DEFAULT_CAP, decode_chain, filters_of};

/// The text of each code in `codes`, through `font`'s own resources (the
/// module comment has the ladder): one [`Decoded`] per code, empty where no
/// rung decodes it. A simple font's codes are single bytes, so past 255 only
/// `/ToUnicode` can decode one.
pub(crate) fn decode_codes(font: &Dictionary, codes: &[u16]) -> Vec<Decoded> {
    let streams = Streams::read(font);
    let ladder = Ladder::new(font, &streams);
    codes
        .iter()
        .map(|&code| ladder.decode(code).unwrap_or_default())
        .collect()
}

/// The advance width of every glyph of `font`'s embedded TrueType or
/// OpenType program, in glyph order, from `hmtx` scaled to 1000 units per em
/// by truncating division (as the `.gmap` widths are). Empty when the font
/// has no such program or it lacks `head`, `maxp` or `hmtx`; a glyph `hmtx`
/// gives no advance for is 0, and one too wide for 16 bits is `u16::MAX`.
pub(crate) fn width_fingerprint(font: &Dictionary) -> Vec<u16> {
    let Some((Format::TrueType, bytes)) = program(font) else {
        return Vec::new();
    };
    let Ok(program) = FontRef::new(&bytes) else {
        return Vec::new();
    };
    let (Ok(head), Ok(maxp), Ok(hmtx)) = (program.head(), program.maxp(), program.hmtx()) else {
        return Vec::new();
    };
    let upem = u32::from(head.units_per_em());
    if upem == 0 {
        return Vec::new();
    }
    (0..u32::from(maxp.num_glyphs()))
        .map(|gid| {
            hmtx.advance(GlyphId::new(gid)).map_or(0, |advance| {
                u16::try_from(u32::from(advance) * 1000 / upem).unwrap_or(u16::MAX)
            })
        })
        .collect()
}

/// How an embedded font program is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// `/FontFile2`, or `/FontFile3` with `/Subtype /OpenType`.
    TrueType,
    /// `/FontFile3` with `/Subtype /Type1C` or `/CIDFontType0C`: bare CFF.
    Cff,
    /// `/FontFile`: a Type 1 program, which this module does not read.
    Type1,
}

/// The decoded streams of a font dictionary.
struct Streams {
    tounicode: Option<Vec<u8>>,
    program: Option<(Format, Vec<u8>)>,
    /// A CIDFontType2's `/CIDToGIDMap`.
    cid_to_gid: CidToGid<Vec<u8>>,
    /// A Type0 font's embedded `/Encoding` CMap.
    encoding_cmap: Option<Vec<u8>>,
}

impl Streams {
    fn read(font: &Dictionary) -> Streams {
        let composite = is_composite(font);
        Streams {
            tounicode: stream_data(font.get(b"ToUnicode").ok()),
            program: program(font),
            cid_to_gid: descendant(font).map_or(CidToGid::Identity, cid_to_gid),
            encoding_cmap: if composite {
                stream_data(font.get(b"Encoding").ok())
            } else {
                None
            },
        }
    }
}

/// Everything the rungs read, parsed once per call.
struct Ladder<'a> {
    tounicode: Option<CMap>,
    kind: Kind<'a>,
    program: Program<'a>,
}

enum Kind<'a> {
    Simple {
        /// Code → glyph name, from `/Differences`.
        differences: BTreeMap<u8, String>,
        base: Option<Box<[&'static str; 256]>>,
        zapf: bool,
        /// The descriptor's Symbolic flag, or a standard symbolic font.
        symbolic: bool,
    },
    Composite {
        /// Code → CID; `None` when the Type0 `/Encoding` cannot be read.
        cids: Option<Cids>,
        /// The `/CIDToGIDMap`: CID → big-endian glyph id.
        cid_to_gid: CidToGid<&'a [u8]>,
        /// The character collection's UCS2 CMap: CID → text.
        collection: Option<Box<CMap>>,
    },
}

/// A CIDFontType2's CID → glyph map (ISO 32000-1 Table 117).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CidToGid<T> {
    /// Absent, null or `/Identity`: the glyph is the CID.
    Identity,
    /// A stream whose data decoded.
    Map(T),
    /// Present but unreadable (a stream whose filters fail, a reference left
    /// in place, any other object): the glyph is unknown, so the program
    /// rung is skipped.
    Unknown,
}

/// The `/CIDToGIDMap` of `cid_font`, its stream data decoded.
fn cid_to_gid(cid_font: &Dictionary) -> CidToGid<Vec<u8>> {
    match cid_font.get(b"CIDToGIDMap").ok() {
        None | Some(Object::Null) => CidToGid::Identity,
        Some(Object::Name(name)) if name == b"Identity" => CidToGid::Identity,
        Some(object @ Object::Stream(_)) => {
            stream_data(Some(object)).map_or(CidToGid::Unknown, CidToGid::Map)
        }
        Some(_) => CidToGid::Unknown,
    }
}

/// A Type0 font's code → CID map.
enum Cids {
    Identity,
    CMap(Box<CMap>),
}

enum Program<'a> {
    None,
    TrueType(Box<TrueType<'a>>),
    Cff(CffFontRef<'a>),
}

/// A TrueType or OpenType program and the lookups the rungs make in it.
struct TrueType<'a> {
    /// `maxp`'s glyph count; `None` when `maxp` is missing or unreadable.
    num_glyphs: Option<u32>,
    /// Glyph → the lowest code point the Unicode subtables map to it.
    unicode: BTreeMap<u16, char>,
    post: BTreeMap<u32, &'a str>,
    /// `post` name → the lowest glyph with that name.
    post_glyph: BTreeMap<&'a str, u16>,
    /// The (3,0) symbol subtable.
    symbol: Option<CmapSubtable<'a>>,
    /// The (1,0) Macintosh Roman subtable.
    mac: Option<CmapSubtable<'a>>,
    /// The (3,1) Unicode BMP subtable.
    windows_unicode: Option<CmapSubtable<'a>>,
    /// Glyph → the `MacRomanEncoding` name of its lowest code in the (1,0)
    /// subtable.
    mac_reverse: BTreeMap<u16, &'static str>,
}

impl<'a> Ladder<'a> {
    fn new(font: &Dictionary, streams: &'a Streams) -> Ladder<'a> {
        let tounicode = streams
            .tounicode
            .as_deref()
            .and_then(|data| CMap::parse(data, load_embedded));
        let program = match &streams.program {
            Some((Format::TrueType, bytes)) => {
                FontRef::new(bytes).ok().map_or(Program::None, |f| {
                    Program::TrueType(Box::new(TrueType::new(&f)))
                })
            }
            Some((Format::Cff, bytes)) => {
                CffFontRef::new(bytes, 0, None).map_or(Program::None, Program::Cff)
            }
            Some((Format::Type1, _)) | None => Program::None,
        };
        let kind = if is_composite(font) {
            let cid_font = descendant(font);
            let cids = match font.get(b"Encoding").ok() {
                Some(Object::Name(name)) if name == b"Identity-H" || name == b"Identity-V" => {
                    Some(Cids::Identity)
                }
                Some(Object::Name(name)) => load_embedded(CMapName::from_bytes(name))
                    .and_then(|data| CMap::parse(data, load_embedded))
                    .map(|cmap| Cids::CMap(Box::new(cmap))),
                _ => streams
                    .encoding_cmap
                    .as_deref()
                    .and_then(|data| CMap::parse(data, load_embedded))
                    .map(|cmap| Cids::CMap(Box::new(cmap))),
            };
            let pdf_family = cid_font
                .and_then(|d| d.get(b"CIDSystemInfo").ok())
                .and_then(|o| o.as_dict().ok())
                .and_then(|info| {
                    let registry = string_bytes(info.get(b"Registry").ok()?)?;
                    let ordering = string_bytes(info.get(b"Ordering").ok()?)?;
                    Some(CidFamily::from_registry_ordering(registry, ordering))
                });
            let cff_family = match &streams.program {
                Some((Format::Cff, bytes)) => cff_family(bytes),
                _ => None,
            };
            let collection = [pdf_family, cff_family]
                .into_iter()
                .flatten()
                .find_map(|family| {
                    let data = load_embedded(family.ucs2_cmap()?)?;
                    CMap::parse(data, load_embedded).map(Box::new)
                });
            Kind::Composite {
                cids,
                cid_to_gid: match &streams.cid_to_gid {
                    CidToGid::Identity => CidToGid::Identity,
                    CidToGid::Map(data) => CidToGid::Map(data.as_slice()),
                    CidToGid::Unknown => CidToGid::Unknown,
                },
                collection,
            }
        } else {
            let (base_name, differences) = match font.get(b"Encoding").ok() {
                Some(Object::Name(name)) => (Some(name.as_slice()), BTreeMap::new()),
                Some(Object::Dictionary(d)) => (
                    d.get(b"BaseEncoding").ok().and_then(|o| o.as_name().ok()),
                    d.get(b"Differences")
                        .ok()
                        .and_then(|o| o.as_array().ok())
                        .map_or_else(BTreeMap::new, |a| differences(a)),
                ),
                _ => (None, BTreeMap::new()),
            };
            let base_font = font
                .get(b"BaseFont")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(strip_subset_tag);
            let symbolic = base_font == Some(b"Symbol".as_slice())
                || base_font == Some(b"ZapfDingbats".as_slice())
                || descriptor(font)
                    .and_then(|d| d.get(b"Flags").ok())
                    .and_then(|o| o.as_i64().ok())
                    .is_some_and(|flags| flags & FLAG_SYMBOLIC != 0);
            let builtin = matches!(program, Program::Cff(_));
            let base = match base_name {
                Some(b"StandardEncoding") => Some(STANDARD),
                Some(b"WinAnsiEncoding") => Some(WIN_ANSI),
                Some(b"MacRomanEncoding") => Some(MAC_ROMAN),
                Some(b"MacExpertEncoding") => Some(MAC_EXPERT),
                _ if !symbolic && !builtin => Some(STANDARD),
                _ => None,
            };
            Kind::Simple {
                differences,
                base: base.map(|src| Box::new(table(src))),
                zapf: base_font == Some(b"ZapfDingbats".as_slice()),
                symbolic,
            }
        };
        Ladder {
            tounicode,
            kind,
            program,
        }
    }

    fn decode(&self, code: u16) -> Option<Decoded> {
        if let Some(text) = self.tounicode.as_ref().and_then(|cmap| {
            match cmap.lookup_bf_string(u32::from(code))? {
                BfString::Char(c) => Some(c.to_string()),
                BfString::String(s) => Some(s),
            }
        }) && let Some(d) = tagged(text, Prov::ToUnicode)
        {
            return Some(d);
        }
        match &self.kind {
            Kind::Simple {
                differences,
                base,
                zapf,
                symbolic,
            } => {
                let code = u8::try_from(code).ok()?;
                let by_name = |name: &str| {
                    let d = if *zapf {
                        zapf_dingbats_glyph_name_to_unicode(name)
                    } else {
                        glyph_name_to_unicode(name)
                    };
                    (!d.text.is_empty()).then_some(d)
                };
                match differences.get(&code) {
                    // A `/Differences` name replaces the base table's for its
                    // code (ISO 32000-1 9.6.6.1): the base name is not the
                    // glyph drawn, so it is never a fallback.
                    Some(name) => {
                        by_name(name).or_else(|| self.simple_program(code, Some(name), *symbolic))
                    }
                    None => base
                        .as_ref()
                        .and_then(|t| by_name(t[usize::from(code)]))
                        .or_else(|| self.simple_program(code, None, *symbolic)),
                }
            }
            Kind::Composite {
                cids,
                cid_to_gid,
                collection,
            } => {
                let cid = match cids.as_ref()? {
                    Cids::Identity => u32::from(code),
                    // A code comes without its byte length: one up to 0xFF
                    // is tried as two bytes, then as one (a one-byte
                    // codespace, or the single-byte half of a mixed one).
                    Cids::CMap(cmap) => {
                        let code = u32::from(code);
                        cmap.lookup_cid_code(code, 2).or_else(|| {
                            (code <= 0xFF)
                                .then(|| cmap.lookup_cid_code(code, 1))
                                .flatten()
                        })?
                    }
                };
                let by_collection = || {
                    let text = match collection.as_ref()?.lookup_bf_string(cid)? {
                        BfString::Char(c) => c.to_string(),
                        BfString::String(s) => s,
                    };
                    tagged(text, Prov::CidCollection)
                };
                match &self.program {
                    Program::None => by_collection(),
                    Program::TrueType(tt) => {
                        let gid = match cid_to_gid {
                            CidToGid::Identity => u16::try_from(cid).ok()?,
                            CidToGid::Map(map) => {
                                let at = usize::try_from(cid).ok()?.checked_mul(2)?;
                                u16::from_be_bytes([*map.get(at)?, *map.get(at + 1)?])
                            }
                            CidToGid::Unknown => return by_collection(),
                        };
                        // An unreadable `maxp` leaves the glyph count unknown,
                        // and then only glyph 0 is known to be `.notdef`.
                        if gid == 0 || tt.num_glyphs.is_some_and(|n| u32::from(gid) >= n) {
                            return None;
                        }
                        tt.glyph_text(gid).or_else(by_collection)
                    }
                    Program::Cff(cff) if cff.is_cid() => {
                        let cid = u16::try_from(cid).ok()?;
                        let gid = cff.charset()?.glyph_id(Sid::new(cid)).ok()?;
                        if gid.to_u32() == 0 || cff.subfont_index(gid).is_none() {
                            return None;
                        }
                        by_collection()
                    }
                    Program::Cff(cff) => {
                        if cid == 0 || cid >= cff.num_glyphs() {
                            return None;
                        }
                        cff_glyph_text(cff, GlyphId::new(cid)).or_else(by_collection)
                    }
                }
            }
        }
    }

    /// Rung 3 for a simple font: the code's glyph in the embedded program.
    /// `name` is the code's `/Differences` name, which did not decode.
    fn simple_program(&self, code: u8, name: Option<&str>, symbolic: bool) -> Option<Decoded> {
        match &self.program {
            Program::None => None,
            Program::TrueType(tt) => {
                // The glyph `post` gives that name, else the raw code's.
                let gid = match name.and_then(|n| tt.post_glyph.get(n)) {
                    Some(0) => return None,
                    Some(&gid) => gid,
                    None => tt.simple_glyph(code, symbolic)?,
                };
                tt.glyph_text(gid)
            }
            // The name picks the glyph through the charset, and a name-keyed
            // glyph's text is its charset name: the one that did not decode.
            Program::Cff(_) if name.is_some() => None,
            Program::Cff(cff) => {
                let gid = cff.encoding()?.map(code)?;
                if gid.to_u32() == 0 {
                    return None;
                }
                cff_glyph_text(cff, gid)
            }
        }
    }
}

impl<'a> TrueType<'a> {
    fn new(font: &FontRef<'a>) -> TrueType<'a> {
        let num_glyphs = font.maxp().ok().map(|m| u32::from(m.num_glyphs()));
        let mut unicode = BTreeMap::new();
        for (c, gid) in unicode_cmap(font).unwrap_or_default() {
            unicode.entry(gid).or_insert(c);
        }
        let post = font.post().map_or_else(
            |_| BTreeMap::new(),
            |post| {
                post.glyph_names()
                    .map(|(gid, name)| (gid.to_u32(), name))
                    .collect()
            },
        );
        let mut post_glyph = BTreeMap::new();
        for (&gid, &name) in &post {
            if let Ok(gid) = u16::try_from(gid) {
                post_glyph.entry(name).or_insert(gid);
            }
        }
        let (mut symbol, mut mac, mut windows_unicode) = (None, None, None);
        if let Ok(cmap) = font.cmap() {
            for record in cmap.encoding_records() {
                let slot = match (record.platform_id(), record.encoding_id()) {
                    (PlatformId::Windows, 0) => &mut symbol,
                    (PlatformId::Macintosh, 0) => &mut mac,
                    (PlatformId::Windows, 1) => &mut windows_unicode,
                    _ => continue,
                };
                if slot.is_none() {
                    *slot = record.subtable(cmap.offset_data()).ok();
                }
            }
        }
        let mut mac_reverse = BTreeMap::new();
        if let Some(sub) = &mac {
            let names = table(MAC_ROMAN);
            for code in 0..=255u8 {
                if let Some(gid) = sub.map_codepoint(code)
                    && let Ok(gid) = u16::try_from(gid.to_u32())
                    && gid != 0
                {
                    mac_reverse.entry(gid).or_insert(names[usize::from(code)]);
                }
            }
        }
        TrueType {
            num_glyphs,
            unicode,
            post,
            post_glyph,
            symbol,
            mac,
            windows_unicode,
            mac_reverse,
        }
    }

    /// A simple font's glyph for `code` through the (3,0), else the (1,0),
    /// subtable (ISO 32000-1 9.6.6.4); for a symbolic font, else the raw code
    /// in the (3,1) subtable, as subsetters that write only (3,1) expect.
    fn simple_glyph(&self, code: u8, symbolic: bool) -> Option<u16> {
        let code = u32::from(code);
        let found = |sub: &CmapSubtable, cp: u32| {
            sub.map_codepoint(cp)
                .and_then(|g| u16::try_from(g.to_u32()).ok())
                .filter(|&g| g != 0)
        };
        if let Some(sub) = &self.symbol
            && let Some(gid) = [0xF000, 0, 0xF100, 0xF200]
                .into_iter()
                .find_map(|high| found(sub, high | code))
        {
            return Some(gid);
        }
        if let Some(gid) = self.mac.as_ref().and_then(|sub| found(sub, code)) {
            return Some(gid);
        }
        found(self.windows_unicode.as_ref().filter(|_| symbolic)?, code)
    }

    /// The text a glyph stands for: Unicode `cmap`, `post` name, (1,0) code.
    fn glyph_text(&self, gid: u16) -> Option<Decoded> {
        if let Some(&c) = self.unicode.get(&gid) {
            return tagged(c.to_string(), Prov::Cmap);
        }
        if let Some(name) = self.post.get(&u32::from(gid)) {
            let d = glyph_name_to_unicode(name);
            if !d.text.is_empty() {
                return Some(d);
            }
        }
        let d = glyph_name_to_unicode(self.mac_reverse.get(&gid)?);
        (!d.text.is_empty()).then_some(d)
    }
}

/// A name-keyed CFF glyph's text, through its charset string.
fn cff_glyph_text(cff: &CffFontRef, gid: GlyphId) -> Option<Decoded> {
    let sid = cff.charset()?.string_id(gid).ok()?;
    let name = std::str::from_utf8(cff.string(sid)?).ok()?;
    let d = glyph_name_to_unicode(name);
    (!d.text.is_empty()).then_some(d)
}

/// The character collection a CID-keyed CFF program's ROS names.
fn cff_family(bytes: &[u8]) -> Option<CidFamily> {
    let cff = Cff::read(FontData::new(bytes)).ok()?;
    let top = cff.top_dicts().get(0).ok()?;
    dict::entries(top, None)
        .flatten()
        .find_map(|entry| match entry {
            dict::Entry::Ros {
                registry, ordering, ..
            } => Some(CidFamily::from_registry_ordering(
                cff.string(registry)?,
                cff.string(ordering)?,
            )),
            _ => None,
        })
}

/// `text` tagged `prov` per character, unless it is only NUL and U+FFFD.
fn tagged(text: String, prov: Prov) -> Option<Decoded> {
    if text
        .chars()
        .all(|c| c == '\0' || c == char::REPLACEMENT_CHARACTER)
    {
        return None;
    }
    let provenance = vec![prov; text.chars().count()];
    Some(Decoded { text, provenance })
}

/// ISO 32000-1 Table 123: the font uses glyphs outside the standard Latin set.
const FLAG_SYMBOLIC: i64 = 1 << 2;

fn is_composite(font: &Dictionary) -> bool {
    font.get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .is_some_and(|s| s == b"Type0")
}

/// A Type0 font's CIDFont: the first `/DescendantFonts` entry.
fn descendant(font: &Dictionary) -> Option<&Dictionary> {
    if !is_composite(font) {
        return None;
    }
    let fonts = font.get(b"DescendantFonts").ok()?.as_array().ok()?;
    fonts.first()?.as_dict().ok()
}

/// The descriptor of the font that holds the program (the CIDFont for a
/// Type0 font).
fn descriptor(font: &Dictionary) -> Option<&Dictionary> {
    let holder = if is_composite(font) {
        descendant(font)?
    } else {
        font
    };
    holder.get(b"FontDescriptor").ok()?.as_dict().ok()
}

/// The embedded font program, decoded, and how to read it.
fn program(font: &Dictionary) -> Option<(Format, Vec<u8>)> {
    let descriptor = descriptor(font)?;
    if let Some(bytes) = stream_data(descriptor.get(b"FontFile2").ok()) {
        return Some((Format::TrueType, bytes));
    }
    if let Ok(Object::Stream(s)) = descriptor.get(b"FontFile3") {
        let bytes = stream_data(descriptor.get(b"FontFile3").ok())?;
        let format = match s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) {
            Some(b"OpenType") => Format::TrueType,
            Some(b"Type1C" | b"CIDFontType0C") => Format::Cff,
            // CFF data starts with major version 1; anything else is sfnt.
            _ if bytes.first() == Some(&1) => Format::Cff,
            _ => Format::TrueType,
        };
        return Some((format, bytes));
    }
    stream_data(descriptor.get(b"FontFile").ok()).map(|bytes| (Format::Type1, bytes))
}

/// A stream's data through its filter chain; `None` for anything else, or
/// when the chain does not decode.
fn stream_data(object: Option<&Object>) -> Option<Vec<u8>> {
    let Some(Object::Stream(s)) = object else {
        return None;
    };
    decode_chain(&s.content, &filters_of(&s.dict), DEFAULT_CAP).ok()
}

fn string_bytes(object: &Object) -> Option<&[u8]> {
    match object {
        Object::String(bytes, _) => Some(bytes),
        _ => None,
    }
}

/// `name` without a subset tag (six uppercase letters and `+`).
fn strip_subset_tag(name: &[u8]) -> &[u8] {
    match name.split_at_checked(7) {
        Some((tag, rest)) if tag[6] == b'+' && tag[..6].iter().all(u8::is_ascii_uppercase) => rest,
        _ => name,
    }
}

/// `/Differences`: a code, then names for it and the codes after it, again
/// and again (ISO 32000-1 9.6.6.1). Codes past 255 are dropped.
fn differences(array: &[Object]) -> BTreeMap<u8, String> {
    let mut map = BTreeMap::new();
    let mut code: Option<i64> = None;
    for item in array {
        match item {
            Object::Integer(n) => code = Some(*n),
            Object::Name(name) => {
                let Some(c) = code else { continue };
                // A name that is not UTF-8 still replaces the base name; it
                // cannot decode, so lossy conversion loses nothing.
                if let Ok(c) = u8::try_from(c) {
                    map.insert(c, String::from_utf8_lossy(name).into_owned());
                }
                code = c.checked_add(1);
            }
            _ => {}
        }
    }
    map
}

/// A base table as an array: the names, `""` for an unused code.
fn table(src: &'static str) -> [&'static str; 256] {
    let mut out = [""; 256];
    for (slot, name) in out.iter_mut().zip(src.split_ascii_whitespace()) {
        if name != "-" {
            *slot = name;
        }
    }
    out
}

// The base encodings of ISO 32000-1 Annex D, codes 0 to 255, eight per line;
// `-` is an unused code. WinAnsiEncoding maps its unused codes above 40
// (octal) to `bullet` and repeats `space` and `hyphen` at 240 and 255 (octal),
// as the Annex's notes say.

/// StandardEncoding.
const STANDARD: &str = "\
- - - - - - - -
- - - - - - - -
- - - - - - - -
- - - - - - - -
space exclam quotedbl numbersign dollar percent ampersand quoteright
parenleft parenright asterisk plus comma hyphen period slash
zero one two three four five six seven
eight nine colon semicolon less equal greater question
at A B C D E F G
H I J K L M N O
P Q R S T U V W
X Y Z bracketleft backslash bracketright asciicircum underscore
quoteleft a b c d e f g
h i j k l m n o
p q r s t u v w
x y z braceleft bar braceright asciitilde -
- - - - - - - -
- - - - - - - -
- - - - - - - -
- - - - - - - -
- exclamdown cent sterling fraction yen florin section
currency quotesingle quotedblleft guillemotleft guilsinglleft guilsinglright fi fl
- endash dagger daggerdbl periodcentered - paragraph bullet
quotesinglbase quotedblbase quotedblright guillemotright ellipsis perthousand - questiondown
- grave acute circumflex tilde macron breve dotaccent
dieresis - ring cedilla - hungarumlaut ogonek caron
emdash - - - - - - -
- - - - - - - -
- AE - ordfeminine - - - -
Lslash Oslash OE ordmasculine - - - -
- ae - - - dotlessi - -
lslash oslash oe germandbls - - - -";

/// WinAnsiEncoding.
const WIN_ANSI: &str = "\
- - - - - - - -
- - - - - - - -
- - - - - - - -
- - - - - - - -
space exclam quotedbl numbersign dollar percent ampersand quotesingle
parenleft parenright asterisk plus comma hyphen period slash
zero one two three four five six seven
eight nine colon semicolon less equal greater question
at A B C D E F G
H I J K L M N O
P Q R S T U V W
X Y Z bracketleft backslash bracketright asciicircum underscore
grave a b c d e f g
h i j k l m n o
p q r s t u v w
x y z braceleft bar braceright asciitilde bullet
Euro bullet quotesinglbase florin quotedblbase ellipsis dagger daggerdbl
circumflex perthousand Scaron guilsinglleft OE bullet Zcaron bullet
bullet quoteleft quoteright quotedblleft quotedblright bullet endash emdash
tilde trademark scaron guilsinglright oe bullet zcaron Ydieresis
space exclamdown cent sterling currency yen brokenbar section
dieresis copyright ordfeminine guillemotleft logicalnot hyphen registered macron
degree plusminus twosuperior threesuperior acute mu paragraph periodcentered
cedilla onesuperior ordmasculine guillemotright onequarter onehalf threequarters questiondown
Agrave Aacute Acircumflex Atilde Adieresis Aring AE Ccedilla
Egrave Eacute Ecircumflex Edieresis Igrave Iacute Icircumflex Idieresis
Eth Ntilde Ograve Oacute Ocircumflex Otilde Odieresis multiply
Oslash Ugrave Uacute Ucircumflex Udieresis Yacute Thorn germandbls
agrave aacute acircumflex atilde adieresis aring ae ccedilla
egrave eacute ecircumflex edieresis igrave iacute icircumflex idieresis
eth ntilde ograve oacute ocircumflex otilde odieresis divide
oslash ugrave uacute ucircumflex udieresis yacute thorn ydieresis";

/// MacRomanEncoding.
const MAC_ROMAN: &str = "\
- - - - - - - -
- - - - - - - -
- - - - - - - -
- - - - - - - -
space exclam quotedbl numbersign dollar percent ampersand quotesingle
parenleft parenright asterisk plus comma hyphen period slash
zero one two three four five six seven
eight nine colon semicolon less equal greater question
at A B C D E F G
H I J K L M N O
P Q R S T U V W
X Y Z bracketleft backslash bracketright asciicircum underscore
grave a b c d e f g
h i j k l m n o
p q r s t u v w
x y z braceleft bar braceright asciitilde -
Adieresis Aring Ccedilla Eacute Ntilde Odieresis Udieresis aacute
agrave acircumflex adieresis atilde aring ccedilla eacute egrave
ecircumflex edieresis iacute igrave icircumflex idieresis ntilde oacute
ograve ocircumflex odieresis otilde uacute ugrave ucircumflex udieresis
dagger degree cent sterling section bullet paragraph germandbls
registered copyright trademark acute dieresis notequal AE Oslash
infinity plusminus lessequal greaterequal yen mu partialdiff summation
product pi integral ordfeminine ordmasculine Omega ae oslash
questiondown exclamdown logicalnot radical florin approxequal Delta guillemotleft
guillemotright ellipsis space Agrave Atilde Otilde OE oe
endash emdash quotedblleft quotedblright quoteleft quoteright divide lozenge
ydieresis Ydieresis fraction currency guilsinglleft guilsinglright fi fl
daggerdbl periodcentered quotesinglbase quotedblbase perthousand Acircumflex Ecircumflex Aacute
Edieresis Egrave Iacute Icircumflex Idieresis Igrave Oacute Ocircumflex
apple Ograve Uacute Ucircumflex Ugrave dotlessi circumflex tilde
macron breve dotaccent ring cedilla hungarumlaut ogonek caron";

/// MacExpertEncoding.
const MAC_EXPERT: &str = "\
- - - - - - - -
- - - - - - - -
- - - - - - - -
- - - - - - - -
space exclamsmall Hungarumlautsmall centoldstyle dollaroldstyle dollarsuperior ampersandsmall Acutesmall
parenleftsuperior parenrightsuperior twodotenleader onedotenleader comma hyphen period fraction
zerooldstyle oneoldstyle twooldstyle threeoldstyle fouroldstyle fiveoldstyle sixoldstyle sevenoldstyle
eightoldstyle nineoldstyle colon semicolon - threequartersemdash - questionsmall
- - - - Ethsmall - - onequarter
onehalf threequarters oneeighth threeeighths fiveeighths seveneighths onethird twothirds
- - - - - - ff fi
fl ffi ffl parenleftinferior - parenrightinferior Circumflexsmall hypheninferior
Gravesmall Asmall Bsmall Csmall Dsmall Esmall Fsmall Gsmall
Hsmall Ismall Jsmall Ksmall Lsmall Msmall Nsmall Osmall
Psmall Qsmall Rsmall Ssmall Tsmall Usmall Vsmall Wsmall
Xsmall Ysmall Zsmall colonmonetary onefitted rupiah Tildesmall -
- asuperior centsuperior - - - - Aacutesmall
Agravesmall Acircumflexsmall Adieresissmall Atildesmall Aringsmall Ccedillasmall Eacutesmall Egravesmall
Ecircumflexsmall Edieresissmall Iacutesmall Igravesmall Icircumflexsmall Idieresissmall Ntildesmall Oacutesmall
Ogravesmall Ocircumflexsmall Odieresissmall Otildesmall Uacutesmall Ugravesmall Ucircumflexsmall Udieresissmall
- eightsuperior fourinferior threeinferior sixinferior eightinferior seveninferior Scaronsmall
- centinferior twoinferior - Dieresissmall - Caronsmall osuperior
fiveinferior - commainferior periodinferior Yacutesmall - dollarinferior -
- Thornsmall - nineinferior zeroinferior Zcaronsmall AEsmall Oslashsmall
questiondownsmall oneinferior Lslashsmall - - - - -
- Cedillasmall - - - - - OEsmall
figuredash hyphensuperior - - - - exclamdownsmall -
Ydieresissmall - onesuperior twosuperior threesuperior foursuperior fivesuperior sixsuperior
sevensuperior ninesuperior zerosuperior - esuperior rsuperior tsuperior -
- isuperior ssuperior dsuperior - - - -
- lsuperior Ogoneksmall Brevesmall Macronsmall bsuperior nsuperior msuperior
commasuperior periodsuperior Dotaccentsmall Ringsmall - - - -";

#[cfg(test)]
mod tests {
    use super::*;

    use lopdf::{Document, Stream};
    use skrifa::MetadataProvider;

    use crate::pdf::fixtures::{GOLDEN_TEXT, TEST_FONT, golden_pdf};
    use crate::pdf::fontdb::build::{build_from_ttf, build_tounicode};
    use crate::pdf::fontdb::gmap::GmapTable;
    use crate::pdf::streams::content_ops;

    fn dict(entries: Vec<(&str, Object)>) -> Dictionary {
        let mut d = Dictionary::new();
        for (key, value) in entries {
            d.set(key, value);
        }
        d
    }

    fn name(n: &str) -> Object {
        Object::Name(n.as_bytes().to_vec())
    }

    fn string(s: &str) -> Object {
        Object::string_literal(s)
    }

    fn stream(entries: Vec<(&str, Object)>, data: Vec<u8>) -> Object {
        Object::Stream(Stream::new(dict(entries), data))
    }

    fn text(decoded: &[Decoded]) -> String {
        decoded.iter().map(|d| d.text.as_str()).collect()
    }

    fn provenance(decoded: &[Decoded]) -> Vec<Prov> {
        decoded
            .iter()
            .flat_map(|d| d.provenance.iter().copied())
            .collect()
    }

    // ---- the golden --------------------------------------------------------

    /// `object` with every reference resolved in place, which is how
    /// `decode_codes` takes a font.
    fn resolved(doc: &Document, object: &Object, depth: u8) -> Object {
        let resolve_dict = |d: &Dictionary| {
            let mut out = Dictionary::new();
            for (key, value) in d.iter() {
                out.set(key.clone(), resolved(doc, value, depth));
            }
            out
        };
        match object {
            Object::Reference(id) if depth > 0 => doc
                .get_object(*id)
                .map_or_else(|_| object.clone(), |o| resolved(doc, o, depth - 1)),
            Object::Array(items) => {
                Object::Array(items.iter().map(|o| resolved(doc, o, depth)).collect())
            }
            Object::Dictionary(d) => Object::Dictionary(resolve_dict(d)),
            Object::Stream(s) => {
                let mut s = s.clone();
                s.dict = resolve_dict(&s.dict);
                Object::Stream(s)
            }
            _ => object.clone(),
        }
    }

    /// The golden's Type0 font dictionary, resolved.
    fn golden_font() -> Dictionary {
        let doc = Document::load_mem(&golden_pdf()).unwrap();
        let type0 = doc
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .find(|d| d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Type0"))
            .expect("golden has a Type0 font");
        match resolved(&doc, &Object::Dictionary(type0.clone()), 8) {
            Object::Dictionary(d) => d,
            _ => unreachable!(),
        }
    }

    /// The codes of every `Tj` the golden draws, page by page, line by line.
    fn golden_codes() -> Vec<Vec<Vec<u16>>> {
        let doc = Document::load_mem(&golden_pdf()).unwrap();
        doc.get_pages()
            .values()
            .map(|&page| {
                let content = doc.get_page_content(page);
                content_ops(&content)
                    .filter(|op| op.op == b"Tj")
                    .map(|op| match &op.operands[..] {
                        [Object::String(bytes, _)] => bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&pair| u16::from_be_bytes(pair))
                            .collect(),
                        other => panic!("Tj operands {other:?}"),
                    })
                    .collect()
            })
            .collect()
    }

    /// The golden's descendant CIDFont inside `font`.
    fn cid_font(font: &mut Dictionary) -> &mut Dictionary {
        let Ok(Object::Array(fonts)) = font.get_mut(b"DescendantFonts") else {
            panic!("DescendantFonts");
        };
        let Object::Dictionary(cid) = &mut fonts[0] else {
            panic!("resolved CIDFont");
        };
        cid
    }

    /// The program inside the golden's descriptor replaced by `program`.
    fn with_program(mut font: Dictionary, program: Vec<u8>) -> Dictionary {
        let Ok(Object::Dictionary(descriptor)) = cid_font(&mut font).get_mut(b"FontDescriptor")
        else {
            panic!("resolved descriptor");
        };
        descriptor.set("FontFile2", stream(vec![], program));
        font
    }

    /// Decodes every golden line through `font` and checks it against
    /// `GOLDEN_TEXT`; returns every character's provenance.
    fn assert_decodes_golden(font: &Dictionary) -> Vec<Prov> {
        let mut all = Vec::new();
        for (page, lines) in golden_codes().iter().enumerate() {
            assert_eq!(lines.len(), GOLDEN_TEXT[page].len());
            for (codes, want) in lines.iter().zip(GOLDEN_TEXT[page]) {
                let decoded = decode_codes(font, codes);
                assert_eq!(decoded.len(), codes.len(), "one Decoded per code");
                assert_eq!(text(&decoded), *want);
                all.extend(provenance(&decoded));
            }
        }
        all
    }

    #[test]
    fn golden_decodes_through_its_tounicode() {
        let prov = assert_decodes_golden(&golden_font());
        assert!(prov.iter().all(|&p| p == Prov::ToUnicode));
    }

    #[test]
    fn without_tounicode_the_program_cmap_decodes_the_golden() {
        // The test font's (3,1) format 4 subtable, composite glyphs included
        // (é è ç ñ ü are composites in the subset).
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        let prov = assert_decodes_golden(&font);
        assert!(prov.iter().all(|&p| p == Prov::Cmap));
    }

    #[test]
    fn a_damaged_tounicode_falls_through_to_the_program() {
        let mut font = golden_font();
        // C8: the stream's data overwritten with spaces.
        let Ok(Object::Stream(s)) = font.get_mut(b"ToUnicode") else {
            panic!("resolved ToUnicode");
        };
        s.content = vec![b' '; s.content.len()];
        assert!(
            assert_decodes_golden(&font)
                .iter()
                .all(|&p| p == Prov::Cmap)
        );
        // A reference left in place is a missing object.
        font.set("ToUnicode", Object::Reference((999, 0)));
        assert!(
            assert_decodes_golden(&font)
                .iter()
                .all(|&p| p == Prov::Cmap)
        );
    }

    #[test]
    fn each_code_climbs_the_ladder_on_its_own() {
        // A ToUnicode covering only the glyphs of the first line: the rest
        // of the golden decodes through the program.
        let first_line = &golden_codes()[0][0];
        let charmap = FontRef::new(TEST_FONT).unwrap().charmap();
        let partial: BTreeMap<u16, char> = GOLDEN_TEXT[0][0]
            .chars()
            .map(|c| {
                let gid = u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap();
                (gid, c)
            })
            .collect();
        let mut font = golden_font();
        font.set("ToUnicode", stream(vec![], build_tounicode(&partial)));
        assert_decodes_golden(&font);
        let second = decode_codes(&font, &golden_codes()[0][1]);
        for (d, c) in second.iter().zip(GOLDEN_TEXT[0][1].chars()) {
            let gid = u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap();
            let want = if partial.contains_key(&gid) {
                Prov::ToUnicode
            } else {
                Prov::Cmap
            };
            assert_eq!(d.provenance, vec![want], "{c:?}");
        }
        assert!(first_line.iter().all(|g| partial.contains_key(g)));
    }

    #[test]
    fn without_a_cmap_the_post_names_decode_the_golden() {
        // `post` 2.0 names: `eacute`, `guillemotleft`, `quoteright`, …
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        let font = with_program(font, sfnt_with(TEST_FONT, &[(*b"cmap", None)]));
        let prov = assert_decodes_golden(&font);
        assert!(prov.iter().all(|&p| p == Prov::Agl));
    }

    #[test]
    fn a_glyph_no_rung_names_decodes_to_nothing() {
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        let font = with_program(
            font,
            sfnt_with(TEST_FONT, &[(*b"cmap", None), (*b"post", None)]),
        );
        let codes = &golden_codes()[0][0];
        let decoded = decode_codes(&font, codes);
        assert_eq!(decoded.len(), codes.len());
        assert!(decoded.iter().all(|d| *d == Decoded::default()));
    }

    #[test]
    fn a_composite_code_the_program_lacks_draws_notdef() {
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        // Glyph 0, and one past the subset's 109 glyphs.
        let decoded = decode_codes(&font, &[0, 109, 5000]);
        assert!(decoded.iter().all(|d| *d == Decoded::default()));
        // A CIDToGIDMap stream sends CID 1 to the glyph of `A` and CID 2
        // past its end.
        let gid_a = FontRef::new(TEST_FONT)
            .unwrap()
            .charmap()
            .map('A')
            .unwrap()
            .to_u32();
        let mut map = vec![0, 0];
        map.extend(u16::try_from(gid_a).unwrap().to_be_bytes());
        cid_font(&mut font).set("CIDToGIDMap", stream(vec![], map));
        let decoded = decode_codes(&font, &[1, 2]);
        assert_eq!(text(&decoded), "A");
        assert_eq!(decoded[1], Decoded::default());
    }

    #[test]
    fn an_unreadable_cidtogidmap_skips_the_program() {
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        let codes = &golden_codes()[0][0];
        // `/Identity` and an explicit null are the default identity map.
        for identity in [name("Identity"), Object::Null] {
            cid_font(&mut font).set("CIDToGIDMap", identity);
            assert_decodes_golden(&font);
        }
        // A stream whose filter fails, a reference left in place, or another
        // object leave the glyph unknown: the golden's Adobe-Identity
        // collection has no UCS2 CMap, so nothing decodes.
        for unknown in [
            stream(vec![("Filter", name("FlateDecode"))], b"not zlib".to_vec()),
            Object::Reference((999, 0)),
            name("Other"),
        ] {
            cid_font(&mut font).set("CIDToGIDMap", unknown.clone());
            let decoded = decode_codes(&font, codes);
            assert!(
                decoded.iter().all(|d| *d == Decoded::default()),
                "{unknown:?}"
            );
        }
    }

    #[test]
    fn an_unreadable_maxp_does_not_hide_the_glyphs() {
        let mut font = golden_font();
        font.remove(b"ToUnicode");
        let font = with_program(font, sfnt_with(TEST_FONT, &[(*b"maxp", None)]));
        let prov = assert_decodes_golden(&font);
        assert!(prov.iter().all(|&p| p == Prov::Cmap));
        // Glyph 0 is still `.notdef`.
        assert_eq!(decode_codes(&font, &[0])[0], Decoded::default());
    }

    #[test]
    fn width_fingerprint_matches_hmtx() {
        let program = FontRef::new(TEST_FONT).unwrap();
        let upem = u32::from(program.head().unwrap().units_per_em());
        let hmtx = program.hmtx().unwrap();
        let widths = width_fingerprint(&golden_font());
        assert_eq!(widths.len(), 109);
        for (gid, &width) in widths.iter().enumerate() {
            let advance = u32::from(hmtx.advance(GlyphId::new(gid as u32)).unwrap());
            assert_eq!(u32::from(width), advance * 1000 / upem, "glyph {gid}");
        }

        // The same widths the golden's /W array and the .gmap carry.
        let mut font = golden_font();
        let Ok(Object::Array(w)) = cid_font(&mut font).get(b"W") else {
            panic!("/W");
        };
        for pair in w.as_chunks::<2>().0 {
            let gid = usize::try_from(pair[0].as_i64().unwrap()).unwrap();
            let width = pair[1].as_array().unwrap()[0].as_i64().unwrap();
            assert_eq!(i64::from(widths[gid]), width, "glyph {gid}");
        }
        let (_, gmap) = build_from_ttf(TEST_FONT).unwrap();
        for record in GmapTable::new(&gmap).unwrap().records() {
            assert_eq!(widths[usize::from(record.gid())], record.width());
        }

        // No TrueType program, no fingerprint.
        let simple = cff_simple_font(name_keyed_cff());
        assert!(width_fingerprint(&simple).is_empty());
    }

    // ---- simple fonts ------------------------------------------------------

    /// A simple TrueType font embedding `program`.
    fn truetype_font(encoding: Option<Object>, flags: i64, program: Vec<u8>) -> Dictionary {
        let mut font = dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("TrueType")),
            ("BaseFont", name("ABCDEF+NotoSans-Regular")),
            (
                "FontDescriptor",
                Object::Dictionary(dict(vec![
                    ("Flags", Object::Integer(flags)),
                    ("FontFile2", stream(vec![], program)),
                ])),
            ),
        ]);
        if let Some(encoding) = encoding {
            font.set("Encoding", encoding);
        }
        font
    }

    const NONSYMBOLIC: i64 = 32;
    const SYMBOLIC: i64 = 4;

    #[test]
    fn differences_win_over_the_base_table() {
        let encoding = dict(vec![
            ("BaseEncoding", name("WinAnsiEncoding")),
            (
                "Differences",
                Object::Array(vec![
                    Object::Integer(65),
                    name("eacute"),
                    name("uni2019"),
                    Object::Integer(300),
                    name("A"),
                    Object::Integer(i64::MAX),
                    name("B"),
                    name("C"),
                ]),
            ),
        ]);
        let font = truetype_font(
            Some(Object::Dictionary(encoding)),
            NONSYMBOLIC,
            TEST_FONT.to_vec(),
        );
        let decoded = decode_codes(&font, &[65, 66, 67, 0x92, 0x80, 300]);
        let texts: Vec<&str> = decoded.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(texts, ["é", "’", "C", "’", "€", ""]);
        assert_eq!(decoded[0].provenance, [Prov::Agl]);
        assert_eq!(decoded[1].provenance, [Prov::UniRule]);
    }

    /// A `post` 2.0 table for `num_glyphs` glyphs, all `.notdef` but `gid`,
    /// named `custom`.
    fn post_naming(num_glyphs: u16, gid: u16, custom: &str) -> Vec<u8> {
        let mut out = 0x0002_0000u32.to_be_bytes().to_vec();
        out.extend([0; 28]);
        out.extend(num_glyphs.to_be_bytes());
        for g in 0..num_glyphs {
            out.extend(if g == gid { 258u16 } else { 0 }.to_be_bytes());
        }
        out.push(u8::try_from(custom.len()).unwrap());
        out.extend(custom.as_bytes());
        out
    }

    #[test]
    fn an_undecodable_difference_hides_the_base_name() {
        // /glyph65 replaces WinAnsi's `A` at 65: the glyph drawn is not `A`.
        let encoding = Object::Dictionary(dict(vec![
            ("BaseEncoding", name("WinAnsiEncoding")),
            (
                "Differences",
                Object::Array(vec![Object::Integer(65), name("glyph65")]),
            ),
        ]));
        let mut bare = truetype_font(Some(encoding.clone()), NONSYMBOLIC, Vec::new());
        bare.remove(b"FontDescriptor");
        let decoded = decode_codes(&bare, &[65, 66]);
        assert_eq!(decoded[0], Decoded::default());
        assert_eq!(text(&decoded), "B");

        let charmap = FontRef::new(TEST_FONT).unwrap().charmap();
        let gid = |c: char| u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap();
        // With a program, the code's glyph decodes: here a (1,0) subtable
        // sends 65 to the glyph of `é`, named `eacute` in `post`.
        let program = sfnt_with(
            TEST_FONT,
            &[(*b"cmap", Some(cmap_table(&[(1, 0, 65, &[gid('é')])])))],
        );
        let font = truetype_font(Some(encoding.clone()), NONSYMBOLIC, program);
        let decoded = decode_codes(&font, &[65]);
        assert_eq!(text(&decoded), "é");
        assert_eq!(decoded[0].provenance, [Prov::Agl]);

        // `post` naming a glyph /glyph65 finds it before any raw-code lookup.
        let program = sfnt_with(
            TEST_FONT,
            &[(*b"post", Some(post_naming(109, gid('ñ'), "glyph65")))],
        );
        let font = truetype_font(Some(encoding), NONSYMBOLIC, program);
        let decoded = decode_codes(&font, &[65, 66]);
        assert_eq!(text(&decoded), "ñB");
        assert_eq!(provenance(&decoded), [Prov::Cmap, Prov::Agl]);

        // A CFF program's built-in encoding does not stand in for the name.
        let mut cff = cff_simple_font(name_keyed_cff());
        cff.set(
            "Encoding",
            Object::Dictionary(dict(vec![(
                "Differences",
                Object::Array(vec![Object::Integer(0x41), name("glyph65")]),
            )])),
        );
        let decoded = decode_codes(&cff, &[0x41, 0x61]);
        assert_eq!(decoded[0], Decoded::default());
        assert_eq!(text(&decoded), "a");
    }

    #[test]
    fn a_simple_font_reads_its_one_byte_tounicode() {
        let tounicode = b"/CIDInit /ProcSet findresource begin\n\
            12 dict begin\nbegincmap\n\
            1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
            1 beginbfchar\n<41> <0078>\nendbfchar\n\
            endcmap\nend\nend\n";
        let mut font = truetype_font(Some(name("WinAnsiEncoding")), NONSYMBOLIC, Vec::new());
        font.remove(b"FontDescriptor");
        font.set("ToUnicode", stream(vec![], tounicode.to_vec()));
        let decoded = decode_codes(&font, &[0x41, 0x42]);
        assert_eq!(text(&decoded), "xB");
        assert_eq!(provenance(&decoded), [Prov::ToUnicode, Prov::Agl]);
    }

    #[test]
    fn a_symbolic_truetype_font_falls_back_to_its_3_1_subtable() {
        // The test font has only Unicode subtables; a symbolic font with no
        // /Encoding reads the raw code through (3,1).
        let font = truetype_font(None, SYMBOLIC, TEST_FONT.to_vec());
        let decoded = decode_codes(&font, &[0x41, 0x61]);
        assert_eq!(text(&decoded), "Aa");
        assert!(provenance(&decoded).iter().all(|&p| p == Prov::Cmap));
    }

    #[test]
    fn fontfile3_without_a_subtype_is_sniffed() {
        let unsubtyped = |mut font: Dictionary| {
            let Ok(Object::Dictionary(descriptor)) = font.get_mut(b"FontDescriptor") else {
                panic!();
            };
            let Ok(Object::Stream(s)) = descriptor.get_mut(b"FontFile3") else {
                panic!();
            };
            s.dict.remove(b"Subtype");
            font
        };
        // CFF data starts with major version 1.
        let cff = unsubtyped(cff_simple_font(name_keyed_cff()));
        assert_eq!(text(&decode_codes(&cff, &[0x41, 0x80])), "A中");
        // Anything else is sfnt, as `/Subtype /OpenType` is.
        let mut sfnt = truetype_font(None, SYMBOLIC, Vec::new());
        let Ok(Object::Dictionary(descriptor)) = sfnt.get_mut(b"FontDescriptor") else {
            panic!();
        };
        descriptor.remove(b"FontFile2");
        descriptor.set(
            "FontFile3",
            stream(vec![("Subtype", name("OpenType"))], TEST_FONT.to_vec()),
        );
        assert_eq!(text(&decode_codes(&sfnt, &[0x41])), "A");
        let sfnt = unsubtyped(sfnt);
        assert_eq!(text(&decode_codes(&sfnt, &[0x41])), "A");
    }

    #[test]
    fn a_nonsymbolic_font_without_an_encoding_reads_standard_encoding() {
        // No /Encoding: StandardEncoding, whose 0x27 and 0x60 are the curly
        // quotes and whose 0xE1 is AE.
        let font = truetype_font(None, NONSYMBOLIC, TEST_FONT.to_vec());
        assert_eq!(
            text(&decode_codes(&font, &[0x27, 0x60, 0xE1, 0x41])),
            "’‘ÆA"
        );
        // A Type 1 program is not read, so the default holds there too.
        let mut type1 = truetype_font(None, NONSYMBOLIC, Vec::new());
        type1.set("Subtype", name("Type1"));
        let Ok(Object::Dictionary(descriptor)) = type1.get_mut(b"FontDescriptor") else {
            panic!();
        };
        descriptor.remove(b"FontFile2");
        descriptor.set("FontFile", stream(vec![], b"%!PS-AdobeFont-1.0".to_vec()));
        assert_eq!(text(&decode_codes(&type1, &[0x27, 0x41])), "’A");
    }

    #[test]
    fn named_base_tables_map_their_codes() {
        let cases: [(&str, &[u8], &str); 4] = [
            ("StandardEncoding", &[0x27, 0xA4, 0xD0], "’⁄—"),
            ("WinAnsiEncoding", &[0x27, 0x80, 0x95, 0xA0, 0xE9], "'€• é"),
            ("MacRomanEncoding", &[0x27, 0x80, 0xCA, 0xDB], "'Ä ¤"),
            (
                "MacExpertEncoding",
                &[0x56, 0x59, 0xA2],
                "\u{FB00}\u{FB03}\u{2084}",
            ),
        ];
        for (base, codes, want) in cases {
            // No program, so nothing but the table can decode.
            let mut font = truetype_font(Some(name(base)), NONSYMBOLIC, Vec::new());
            font.remove(b"FontDescriptor");
            let codes: Vec<u16> = codes.iter().map(|&c| u16::from(c)).collect();
            assert_eq!(text(&decode_codes(&font, &codes)), want, "{base}");
        }
    }

    #[test]
    fn base_tables_agree_with_lopdf() {
        // lopdf's tables are an independent transcription of Annex D.
        for base in [
            "StandardEncoding",
            "WinAnsiEncoding",
            "MacRomanEncoding",
            "MacExpertEncoding",
        ] {
            let mut font = truetype_font(Some(name(base)), NONSYMBOLIC, Vec::new());
            font.remove(b"FontDescriptor");
            let ours = decode_codes(&font, &(0..=255).collect::<Vec<u16>>());
            let doc = Document::new();
            let lopdf::Encoding::OneByteEncoding(theirs) = font.get_font_encoding(&doc).unwrap()
            else {
                panic!("{base} is a one-byte encoding in lopdf");
            };
            let mut diffs = Vec::new();
            for (code, d) in ours.iter().enumerate() {
                let theirs = lopdf::Encoding::OneByteEncoding(theirs)
                    .bytes_to_string(&[u8::try_from(code).unwrap()])
                    .unwrap();
                if d.text != theirs {
                    diffs.push((code, d.text.clone(), theirs));
                }
            }
            assert!(diffs.is_empty(), "{base}: {diffs:?}");
        }
    }

    #[test]
    fn zapf_dingbats_names_read_the_itc_list() {
        let encoding = dict(vec![(
            "Differences",
            Object::Array(vec![Object::Integer(0x21), name("a1")]),
        )]);
        // Symbolic and without a program: only the name can decode.
        let mut font = dict(vec![
            ("Subtype", name("Type1")),
            ("BaseFont", name("ZapfDingbats")),
            ("Encoding", Object::Dictionary(encoding)),
            (
                "FontDescriptor",
                Object::Dictionary(dict(vec![("Flags", Object::Integer(SYMBOLIC))])),
            ),
        ]);
        assert_eq!(text(&decode_codes(&font, &[0x21])), "\u{2701}");
        // `a1` is not an AGL name.
        font.set("BaseFont", name("Other"));
        assert_eq!(decode_codes(&font, &[0x21])[0], Decoded::default());
        // A subset tag does not hide the name.
        font.set("BaseFont", name("ABCDEF+ZapfDingbats"));
        assert_eq!(text(&decode_codes(&font, &[0x21])), "\u{2701}");
    }

    #[test]
    fn a_symbolic_truetype_font_reads_its_3_0_then_1_0_subtables() {
        let font = FontRef::new(TEST_FONT).unwrap();
        let charmap = font.charmap();
        let gid = |c: char| u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap();
        // (3,0) maps 0xF041… to the capitals and (1,0) maps 0x61… to the
        // small letters: each code is found through one subtable only.
        let symbol: Vec<u16> = ('A'..='Z').map(gid).collect();
        let mac: Vec<u16> = ('a'..='z').map(gid).collect();
        let cmap = cmap_table(&[(1, 0, 0x61, &mac), (3, 0, 0xF041, &symbol)]);

        // The glyphs are named by their `post` names.
        let program = sfnt_with(TEST_FONT, &[(*b"cmap", Some(cmap.clone()))]);
        let pdf_font = truetype_font(None, SYMBOLIC, program);
        let decoded = decode_codes(&pdf_font, &[0x41, 0x5A, 0x61, 0x7A, 0x20]);
        assert_eq!(text(&decoded), "AZaz");
        assert_eq!(decoded[4], Decoded::default());
        assert!(provenance(&decoded).iter().all(|&p| p == Prov::Agl));

        // Without `post`, a glyph is named by its (1,0) code through
        // MacRomanEncoding; a capital's glyph has none.
        let program = sfnt_with(TEST_FONT, &[(*b"cmap", Some(cmap)), (*b"post", None)]);
        let pdf_font = truetype_font(None, SYMBOLIC, program);
        let decoded = decode_codes(&pdf_font, &[0x41, 0x61, 0x7A]);
        assert_eq!(decoded[0], Decoded::default());
        assert_eq!(text(&decoded), "az");
    }

    // ---- CFF ---------------------------------------------------------------

    /// A five-byte CFF DICT integer operand.
    fn op_int(v: usize) -> Vec<u8> {
        let mut out = vec![29];
        out.extend(i32::try_from(v).unwrap().to_be_bytes());
        out
    }

    /// A CFF INDEX with four-byte offsets.
    fn cff_index(items: &[&[u8]]) -> Vec<u8> {
        if items.is_empty() {
            return vec![0, 0];
        }
        let mut out = u16::try_from(items.len()).unwrap().to_be_bytes().to_vec();
        out.push(4);
        let mut offset = 1u32;
        out.extend(offset.to_be_bytes());
        for item in items {
            offset += u32::try_from(item.len()).unwrap();
            out.extend(offset.to_be_bytes());
        }
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }

    /// A CFF blob: `top(offsets)` builds the Top DICT from the absolute
    /// offsets of `tail`'s parts, which follow the global subroutine INDEX.
    fn cff(strings: &[&[u8]], top: impl Fn(&[usize]) -> Vec<u8>, tail: &[Vec<u8>]) -> Vec<u8> {
        let head = |top_dict: &[u8]| {
            let mut out = vec![1, 0, 4, 4];
            out.extend(cff_index(&[b"Test"]));
            out.extend(cff_index(&[top_dict]));
            out.extend(cff_index(strings));
            out.extend(cff_index(&[]));
            out
        };
        // Every operand is five bytes, so the Top DICT's size does not
        // depend on the offsets.
        let base = head(&top(&vec![0; tail.len()])).len();
        let mut offsets = Vec::new();
        let mut at = base;
        for part in tail {
            offsets.push(at);
            at += part.len();
        }
        let mut out = head(&top(&offsets));
        assert_eq!(out.len(), base);
        for part in tail {
            out.extend(part);
        }
        out
    }

    /// `endchar` for each of `n` glyphs.
    fn charstrings(n: usize) -> Vec<u8> {
        cff_index(&vec![&[14u8][..]; n])
    }

    /// A CID-keyed CFF (ROS Adobe-Japan1-6): glyphs 1 to 4 are CIDs 34 to 37
    /// (`A` to `D` in Adobe-Japan1), and the FDSelect gives glyph 4 no
    /// subfont.
    fn cid_keyed_cff() -> Vec<u8> {
        let mut charset = vec![0];
        for cid in 34u16..=37 {
            charset.extend(cid.to_be_bytes());
        }
        // Format 3: one range from glyph 0 in Font DICT 0, sentinel 4.
        let fd_select = vec![3, 0, 1, 0, 0, 0, 0, 4];
        let fd_array = cff_index(&[&[]]);
        cff(
            &[b"Adobe", b"Japan1"],
            |at| {
                let mut top = Vec::new();
                top.extend(op_int(391));
                top.extend(op_int(392));
                top.extend(op_int(6));
                top.extend([12, 30]); // ROS
                top.extend(op_int(at[0]));
                top.push(15); // charset
                top.extend(op_int(at[1]));
                top.extend([12, 37]); // FDSelect
                top.extend(op_int(at[2]));
                top.push(17); // CharStrings
                top.extend(op_int(at[3]));
                top.extend([12, 36]); // FDArray
                top
            },
            &[charset, fd_select, charstrings(5), fd_array],
        )
    }

    /// A name-keyed CFF: glyphs `A`, `a` and `uni4E2D`, with a custom
    /// encoding giving them codes 0x41, 0x61 and 0x80.
    fn name_keyed_cff() -> Vec<u8> {
        let mut charset = vec![0];
        for sid in [34u16, 66, 391] {
            charset.extend(sid.to_be_bytes());
        }
        let encoding = vec![0, 3, 0x41, 0x61, 0x80];
        cff(
            &[b"uni4E2D"],
            |at| {
                let mut top = Vec::new();
                top.extend(op_int(at[0]));
                top.push(15); // charset
                top.extend(op_int(at[1]));
                top.push(16); // Encoding
                top.extend(op_int(at[2]));
                top.push(17); // CharStrings
                top.extend(op_int(0));
                top.extend(op_int(at[2]));
                top.push(18); // Private, empty
                top
            },
            &[charset, encoding, charstrings(4)],
        )
    }

    /// A Type0/Identity-H font over a CIDFontType0 embedding `program` as
    /// `/FontFile3 /CIDFontType0C`.
    fn cid_cff_font(program: Vec<u8>, system_info: Option<(&str, &str)>) -> Dictionary {
        let mut cid = dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("CIDFontType0")),
            ("BaseFont", name("Test")),
            (
                "FontDescriptor",
                Object::Dictionary(dict(vec![
                    ("Flags", Object::Integer(SYMBOLIC)),
                    (
                        "FontFile3",
                        stream(vec![("Subtype", name("CIDFontType0C"))], program),
                    ),
                ])),
            ),
        ]);
        if let Some((registry, ordering)) = system_info {
            cid.set(
                "CIDSystemInfo",
                Object::Dictionary(dict(vec![
                    ("Registry", string(registry)),
                    ("Ordering", string(ordering)),
                    ("Supplement", Object::Integer(6)),
                ])),
            );
        }
        dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("Type0")),
            ("BaseFont", name("Test")),
            ("Encoding", name("Identity-H")),
            (
                "DescendantFonts",
                Object::Array(vec![Object::Dictionary(cid)]),
            ),
        ])
    }

    /// A simple Type1 font embedding `program` as `/FontFile3 /Type1C`.
    fn cff_simple_font(program: Vec<u8>) -> Dictionary {
        dict(vec![
            ("Type", name("Font")),
            ("Subtype", name("Type1")),
            ("BaseFont", name("ABCDEF+Test")),
            (
                "FontDescriptor",
                Object::Dictionary(dict(vec![
                    ("Flags", Object::Integer(SYMBOLIC)),
                    (
                        "FontFile3",
                        stream(vec![("Subtype", name("Type1C"))], program),
                    ),
                ])),
            ),
        ])
    }

    #[test]
    fn the_synthetic_cid_blob_is_what_it_claims() {
        let bytes = cid_keyed_cff();
        let cff = CffFontRef::new(&bytes, 0, None).unwrap();
        assert!(cff.is_cid());
        assert_eq!(cff.num_glyphs(), 5);
        let charset = cff.charset().unwrap();
        assert_eq!(charset.glyph_id(Sid::new(36)).unwrap().to_u32(), 3);
        assert_eq!(cff.subfont_index(GlyphId::new(3)), Some(0));
        assert_eq!(cff.subfont_index(GlyphId::new(4)), None);
        assert_eq!(cff_family(&bytes), Some(CidFamily::AdobeJapan1));
    }

    #[test]
    fn a_cid_keyed_cff_decodes_through_its_charset_and_collection() {
        let font = cid_cff_font(cid_keyed_cff(), Some(("Adobe", "Japan1")));
        // CIDs 34–36 are in the charset and a subfont; 37's glyph has no
        // subfont; 38 is not in the charset; 0 is .notdef.
        let decoded = decode_codes(&font, &[34, 35, 36, 37, 38, 0]);
        assert_eq!(text(&decoded), "ABC");
        assert!(decoded[3..].iter().all(|d| *d == Decoded::default()));
        assert!(
            provenance(&decoded)
                .iter()
                .all(|&p| p == Prov::CidCollection)
        );
    }

    #[test]
    fn a_cid_keyed_cff_falls_back_to_its_own_ros() {
        // An Identity or missing /CIDSystemInfo has no UCS2 CMap: the
        // program's ROS names the collection instead.
        for info in [Some(("Adobe", "Identity")), None] {
            let font = cid_cff_font(cid_keyed_cff(), info);
            assert_eq!(text(&decode_codes(&font, &[34, 35, 36])), "ABC", "{info:?}");
        }
    }

    #[test]
    fn a_composite_font_without_its_program_reads_its_collection() {
        // C7/C8: the program is lost; /CIDSystemInfo still names Japan1.
        let mut font = cid_cff_font(cid_keyed_cff(), Some(("Adobe", "Japan1")));
        let Some(Object::Dictionary(descriptor)) = font
            .get_mut(b"DescendantFonts")
            .ok()
            .and_then(|o| o.as_array_mut().ok())
            .and_then(|a| a.first_mut())
            .and_then(|cid| cid.as_dict_mut().ok())
            .and_then(|cid| cid.get_mut(b"FontDescriptor").ok())
        else {
            panic!("descriptor");
        };
        descriptor.remove(b"FontFile3");
        // Without a program nothing says CID 37 or 38 is `.notdef`.
        let decoded = decode_codes(&font, &[34, 35, 37, 38]);
        assert_eq!(text(&decoded), "ABDE");
        assert!(
            provenance(&decoded)
                .iter()
                .all(|&p| p == Prov::CidCollection)
        );
    }

    #[test]
    fn a_predefined_encoding_cmap_maps_codes_to_cids() {
        // UniJIS-UCS2-H sends U+0041… to CIDs 34….
        let mut font = cid_cff_font(cid_keyed_cff(), Some(("Adobe", "Japan1")));
        font.set("Encoding", name("UniJIS-UCS2-H"));
        assert_eq!(text(&decode_codes(&font, &[0x41, 0x42, 0x43])), "ABC");
        // A CMap name nothing knows decodes nothing.
        font.set("Encoding", name("No-Such-CMap"));
        let decoded = decode_codes(&font, &[0x41]);
        assert_eq!(decoded[0], Decoded::default());
    }

    #[test]
    fn an_embedded_one_byte_encoding_cmap_maps_codes_to_cids() {
        let cmap = b"/CIDInit /ProcSet findresource begin\n\
            12 dict begin\nbegincmap\n/CMapName /Test-H def\n\
            1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
            1 begincidrange\n<41> <43> 34\nendcidrange\n\
            endcmap\nend\nend\n";
        let mut font = cid_cff_font(cid_keyed_cff(), Some(("Adobe", "Japan1")));
        font.set("Encoding", stream(vec![], cmap.to_vec()));
        let decoded = decode_codes(&font, &[0x41, 0x42, 0x43, 0x44]);
        assert_eq!(text(&decoded), "ABC");
        assert_eq!(decoded[3], Decoded::default());
    }

    #[test]
    fn a_tounicode_wins_over_the_cff_rungs() {
        let mut font = cid_cff_font(cid_keyed_cff(), Some(("Adobe", "Japan1")));
        font.set(
            "ToUnicode",
            stream(vec![], build_tounicode(&BTreeMap::from([(34, 'x')]))),
        );
        let decoded = decode_codes(&font, &[34, 35]);
        assert_eq!(text(&decoded), "xB");
        assert_eq!(provenance(&decoded), [Prov::ToUnicode, Prov::CidCollection]);
    }

    #[test]
    fn a_name_keyed_cff_decodes_through_its_encoding_and_charset() {
        let font = cff_simple_font(name_keyed_cff());
        let decoded = decode_codes(&font, &[0x41, 0x61, 0x80, 0x42, 300]);
        assert_eq!(text(&decoded), "Aa中");
        assert_eq!(provenance(&decoded), [Prov::Agl, Prov::Agl, Prov::UniRule]);
        assert!(decoded[3..].iter().all(|d| *d == Decoded::default()));
    }

    // ---- sfnt editing --------------------------------------------------------

    /// `font` with each `(tag, table)` replaced, added, or (for `None`)
    /// dropped, laid out afresh (checksums are left zero; nothing here reads
    /// them).
    fn sfnt_with(font: &[u8], edits: &[([u8; 4], Option<Vec<u8>>)]) -> Vec<u8> {
        let be16 = |at: usize| usize::from(u16::from_be_bytes([font[at], font[at + 1]]));
        let be32 = |at: usize| {
            usize::try_from(u32::from_be_bytes(font[at..at + 4].try_into().unwrap())).unwrap()
        };
        let mut tables = BTreeMap::new();
        for i in 0..be16(4) {
            let record = 12 + 16 * i;
            let tag: [u8; 4] = font[record..record + 4].try_into().unwrap();
            let (offset, len) = (be32(record + 8), be32(record + 12));
            tables.insert(tag, font[offset..offset + len].to_vec());
        }
        for (tag, table) in edits {
            match table {
                Some(data) => tables.insert(*tag, data.clone()),
                None => tables.remove(tag),
            };
        }
        let count = u16::try_from(tables.len()).unwrap();
        let mut out = font[..4].to_vec();
        out.extend(count.to_be_bytes());
        out.extend([0; 6]);
        let mut offset = 12 + 16 * tables.len();
        let mut data = Vec::new();
        for (tag, table) in &tables {
            out.extend(tag);
            out.extend([0; 4]);
            out.extend(u32::try_from(offset).unwrap().to_be_bytes());
            out.extend(u32::try_from(table.len()).unwrap().to_be_bytes());
            data.extend(table);
            while data.len() % 4 != 0 {
                data.push(0);
            }
            offset = 12 + 16 * tables.len() + data.len();
        }
        out.extend(data);
        out
    }

    /// A `cmap` table of format 6 subtables `(platform, encoding, first code,
    /// glyphs)`, in the order given.
    fn cmap_table(subtables: &[(u16, u16, u16, &[u16])]) -> Vec<u8> {
        let count = u16::try_from(subtables.len()).unwrap();
        let mut records = Vec::new();
        let mut bodies = Vec::new();
        let start = 4 + 8 * subtables.len();
        for &(platform, encoding, first, glyphs) in subtables {
            records.extend(platform.to_be_bytes());
            records.extend(encoding.to_be_bytes());
            records.extend(u32::try_from(start + bodies.len()).unwrap().to_be_bytes());
            let len = u16::try_from(10 + 2 * glyphs.len()).unwrap();
            for v in [6, len, 0, first, u16::try_from(glyphs.len()).unwrap()] {
                bodies.extend(v.to_be_bytes());
            }
            for g in glyphs {
                bodies.extend(g.to_be_bytes());
            }
        }
        let mut out = vec![0, 0];
        out.extend(count.to_be_bytes());
        out.extend(records);
        out.extend(bodies);
        out
    }

    #[test]
    fn sfnt_with_no_edits_keeps_every_table() {
        let rebuilt = sfnt_with(TEST_FONT, &[]);
        let (a, b) = (
            FontRef::new(TEST_FONT).unwrap(),
            FontRef::new(&rebuilt).unwrap(),
        );
        assert_eq!(
            a.table_directory().num_tables(),
            b.table_directory().num_tables()
        );
        for record in a.table_directory().table_records() {
            let tag = record.tag();
            assert_eq!(
                a.table_data(tag).unwrap().as_bytes(),
                b.table_data(tag).unwrap().as_bytes()
            );
        }
    }
}
