//! The font-DB builder (T-27b, TD §5.1, §18.1, §18.3, FR-03): a TrueType font's
//! `.gmap` and index entry, and the `/ToUnicode` CMap builder.
//!
//! [`build_from_ttf`] is what `tools/build-templates` calls to write
//! `assets/gmaps/*.gmap` and `assets/fontindex.json`, so the tool and the
//! runtime read the same records by construction. Everything is integer: widths
//! and descriptor metrics are scaled to 1000 units per em by truncating
//! division, exactly as the fixtures scale the test font.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use harfrust::{ShapeOptions, ShaperData, UnicodeBuffer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skrifa::raw::tables::cmap::{CmapIterLimits, PlatformId};
use skrifa::raw::types::Tag;
use skrifa::raw::{FontRef, TableProvider};
use skrifa::{GlyphId, MetadataProvider, string::StringId};

use super::gmap::{self, GmapRecord, Source};

/// One font of the bundled database: a `fontindex.json` entry (TD §5.1).
///
/// [`build_from_ttf`] fills every field but `base_font_aliases` and
/// `drawn_with`, which are curated rather than read from the font:
/// `tools/build-templates` sets them. Template PDFs are built at run time
/// (D-021), so no template path is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// The database key, the font's PostScript name.
    pub id: String,
    /// The typographic family name (name ID 16), else the family (name ID 1).
    pub family: String,
    /// The PostScript name (name ID 6).
    pub postscript_name: String,
    /// `/BaseFont` names this font stands in for (e.g. `Helvetica`).
    pub base_font_aliases: Vec<String>,
    /// For a font the database holds only the `.gmap` of (D-010 (b)): the id
    /// of the database font whose program draws its glyphs in an output.
    /// `None` (left out of the JSON) for a font whose program is in the
    /// database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn_with: Option<String>,
    /// ISO 15924 codes of the scripts the font covers, sorted.
    pub scripts: Vec<String>,
    /// Labels (`en`, `fr`, `es`, `ar`, `hi`, `zh`) of the languages whose
    /// letters the font maps, in that order.
    pub languages: Vec<String>,
    /// Lowercase hex SHA-256 of the font file. For a font indexed with
    /// `drawn_with`, the file its `.gmap` was built from, which the database
    /// does not hold.
    pub sha256: String,
    /// Lowercase hex SHA-256 of the `.gmap` built with this entry.
    pub gmap_sha256: String,
    /// `/FontDescriptor /Flags`: FixedPitch, Nonsymbolic or Symbolic, Italic.
    pub flags: u32,
    /// `head`'s glyph bounding box, 1000 units per em.
    pub bbox: [i32; 4],
    /// `post`'s italic angle in whole degrees, truncated toward zero.
    pub italic_angle: i32,
    /// `hhea` ascender, 1000 units per em.
    pub ascent: i32,
    /// `hhea` descender, 1000 units per em.
    pub descent: i32,
    /// `OS/2` cap height (0 when absent), 1000 units per em.
    pub cap_height: i32,
}

/// Why a font cannot go into the database.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    #[error("font does not parse: {0}")]
    Parse(String),
    #[error("font has no TrueType outlines (no glyf table)")]
    NotTrueType,
    #[error("font has no {0} table")]
    MissingTable(&'static str),
    #[error("font has no name ID {0}")]
    MissingName(u16),
    #[error("font has units_per_em 0")]
    ZeroUnitsPerEm,
    #[error("font maps no Unicode code point")]
    NoUnicodeCmap,
    #[error("glyph {0} does not fit a 16-bit glyph id")]
    GlyphIdTooLarge(u32),
    #[error("glyph {0} has no advance width that fits 16 bits at 1000 units per em")]
    BadWidth(u16),
}

/// The `.gmap` and index entry of a TrueType font.
///
/// The records are every Unicode `cmap` entry (each subtable read through its
/// raw encoding record and confirmed with `map_codepoint`) plus the shaped
/// Latin ligatures (`ff`, `fi`, `fl`, `ffi`, `ffl`, `ſt`, `st` under the default
/// features) that the `cmap` does not already pair with their presentation
/// form, each with its `hmtx` advance scaled to 1000 units per em. The output
/// depends on `bytes` alone.
pub fn build_from_ttf(bytes: &[u8]) -> Result<(IndexEntry, Vec<u8>), BuildError> {
    let font = FontRef::new(bytes).map_err(|e| BuildError::Parse(e.to_string()))?;
    if font.table_data(Tag::new(b"glyf")).is_none() {
        return Err(BuildError::NotTrueType);
    }
    let head = font.head().map_err(|_| BuildError::MissingTable("head"))?;
    let hhea = font.hhea().map_err(|_| BuildError::MissingTable("hhea"))?;
    let hmtx = font.hmtx().map_err(|_| BuildError::MissingTable("hmtx"))?;
    let upem = i64::from(head.units_per_em());
    if upem == 0 {
        return Err(BuildError::ZeroUnitsPerEm);
    }
    let scale = |v: i64| i32::try_from(v * 1000 / upem).unwrap_or(i32::MAX);

    let cmap = unicode_cmap(&font)?;
    let width = |gid: u16| -> Result<u16, BuildError> {
        let advance = hmtx.advance(GlyphId::new(u32::from(gid)));
        advance
            .and_then(|a| u16::try_from(i64::from(a) * 1000 / upem).ok())
            .ok_or(BuildError::BadWidth(gid))
    };
    let mut records = Vec::with_capacity(cmap.len());
    for (&c, &gid) in &cmap {
        records.push(GmapRecord::new(gid, c, width(gid)?, Source::Cmap));
    }
    for (lig, gid) in shaped_ligatures(&font, &cmap) {
        // Fonts usually map the presentation form to the same glyph already.
        if cmap.get(&lig) == Some(&gid) {
            continue;
        }
        records.push(GmapRecord::new(gid, lig, width(gid)?, Source::Shaped));
    }
    let gmap = gmap::encode(records);

    let postscript_name = name(&font, StringId::POSTSCRIPT_NAME)?;
    let family = name(&font, StringId::TYPOGRAPHIC_FAMILY_NAME)
        .or_else(|_| name(&font, StringId::FAMILY_NAME))?;
    let languages: Vec<&Language> = LANGUAGES
        .iter()
        .filter(|l| l.required.chars().all(|c| cmap.contains_key(&c)))
        .collect();
    let mut scripts: Vec<String> = languages.iter().map(|l| l.script.to_owned()).collect();
    scripts.sort();
    scripts.dedup();

    let post = font.post().ok();
    let italic_angle = post
        .as_ref()
        .map_or(0, |p| p.italic_angle().to_bits() / 65536);
    let mut flags = 0;
    if post.as_ref().is_some_and(|p| p.is_fixed_pitch() != 0) {
        flags |= FLAG_FIXED_PITCH;
    }
    flags |= if languages.iter().any(|l| l.script == "Latn") {
        FLAG_NONSYMBOLIC
    } else {
        FLAG_SYMBOLIC
    };
    if italic_angle != 0 {
        flags |= FLAG_ITALIC;
    }
    let cap_height = font
        .os2()
        .ok()
        .and_then(|os2| os2.s_cap_height())
        .unwrap_or(0);

    let entry = IndexEntry {
        id: postscript_name.clone(),
        family,
        postscript_name,
        base_font_aliases: Vec::new(),
        drawn_with: None,
        scripts,
        languages: languages.iter().map(|l| l.label.to_owned()).collect(),
        sha256: sha256_hex(bytes),
        gmap_sha256: sha256_hex(&gmap),
        flags,
        bbox: [
            scale(i64::from(head.x_min())),
            scale(i64::from(head.y_min())),
            scale(i64::from(head.x_max())),
            scale(i64::from(head.y_max())),
        ],
        italic_angle,
        ascent: scale(i64::from(hhea.ascender().to_i16())),
        descent: scale(i64::from(hhea.descender().to_i16())),
        cap_height: scale(i64::from(cap_height)),
    };
    Ok((entry, gmap))
}

const FLAG_FIXED_PITCH: u32 = 1;
const FLAG_SYMBOLIC: u32 = 1 << 2;
const FLAG_NONSYMBOLIC: u32 = 1 << 5;
const FLAG_ITALIC: u32 = 1 << 6;

/// A language the index can list, and the letters a font must map for it.
struct Language {
    label: &'static str,
    script: &'static str,
    required: &'static str,
}

const LATIN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

const LANGUAGES: [Language; 6] = [
    Language {
        label: "en",
        script: "Latn",
        required: LATIN,
    },
    Language {
        label: "fr",
        script: "Latn",
        required: concat!(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
            "àâæçéèêëîïôœùûüÿÀÂÆÇÉÈÊËÎÏÔŒÙÛÜŸ«»"
        ),
    },
    Language {
        label: "es",
        script: "Latn",
        required: concat!(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz",
            "áéíñóúüÁÉÍÑÓÚÜ¡¿"
        ),
    },
    Language {
        label: "ar",
        script: "Arab",
        // U+0621..U+063A and U+0641..U+064A: the Arabic letters.
        required: "ءآأؤإئابةتثجحخدذرزسشصضطظعغفقكلمنهوىي",
    },
    Language {
        label: "hi",
        script: "Deva",
        // The independent vowels, the consonants, the vowel signs and virama.
        required: "अआइईउऊऋएऐओऔकखगघङचछजझञटठडढणतथदधनपफबभमयरलवशषसहािीुूृेैोौ्",
    },
    Language {
        label: "zh",
        script: "Hani",
        // Twenty of the commonest Han characters.
        required: "的一是不了人我在有他这中大来上国个到说们",
    },
];

/// The Unicode subtables, most comprehensive first (the order of read-fonts'
/// `best_subtable`, without the symbol and Mac Roman subtables, whose codes
/// are not Unicode).
const UNICODE_SUBTABLES: [(PlatformId, u16); 8] = [
    (PlatformId::Windows, 10),
    (PlatformId::Unicode, 6),
    (PlatformId::Unicode, 4),
    (PlatformId::Windows, 1),
    (PlatformId::Unicode, 3),
    (PlatformId::Unicode, 2),
    (PlatformId::Unicode, 1),
    (PlatformId::Unicode, 0),
];

/// Every code point a Unicode subtable maps to a glyph other than `.notdef`;
/// where subtables disagree, the most comprehensive one wins.
pub(super) fn unicode_cmap(font: &FontRef) -> Result<BTreeMap<char, u16>, BuildError> {
    let cmap = font.cmap().map_err(|_| BuildError::MissingTable("cmap"))?;
    let limits = CmapIterLimits::default_for_font(font);
    let mut map = BTreeMap::new();
    for (platform, encoding) in UNICODE_SUBTABLES {
        for record in cmap.encoding_records() {
            if record.platform_id() != platform || record.encoding_id() != encoding {
                continue;
            }
            let Ok(subtable) = record.subtable(cmap.offset_data()) else {
                continue;
            };
            for (cp, _) in subtable.iter_with_limits(limits) {
                let (Some(c), Some(gid)) = (char::from_u32(cp), subtable.map_codepoint(cp)) else {
                    continue;
                };
                if gid.to_u32() == 0 {
                    continue;
                }
                let gid = u16::try_from(gid.to_u32())
                    .map_err(|_| BuildError::GlyphIdTooLarge(gid.to_u32()))?;
                map.entry(c).or_insert(gid);
            }
        }
    }
    if map.is_empty() {
        return Err(BuildError::NoUnicodeCmap);
    }
    Ok(map)
}

/// The Latin ligatures with a Unicode presentation form, as `(text, form)`.
const LIGATURES: [(&str, char); 7] = [
    ("ff", '\u{fb00}'),
    ("fi", '\u{fb01}'),
    ("fl", '\u{fb02}'),
    ("ffi", '\u{fb03}'),
    ("ffl", '\u{fb04}'),
    ("\u{17f}t", '\u{fb05}'),
    ("st", '\u{fb06}'),
];

/// `(presentation form, glyph)` for each ligature that shaping its letters
/// with the default features turns into one glyph of its own.
fn shaped_ligatures(font: &FontRef, cmap: &BTreeMap<char, u16>) -> Vec<(char, u16)> {
    let data = ShaperData::new(font);
    let shaper = data.shaper(font).build();
    let mut out = Vec::new();
    for (text, form) in LIGATURES {
        let letters: Option<Vec<u16>> = text.chars().map(|c| cmap.get(&c).copied()).collect();
        let Some(letters) = letters else {
            continue;
        };
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, ShapeOptions::new());
        if let [glyph] = shaped.glyph_infos()
            && let Ok(gid) = u16::try_from(glyph.glyph_id)
            && gid != 0
            && !letters.contains(&gid)
        {
            out.push((form, gid));
        }
    }
    out
}

/// The English (else first) string of a name ID.
fn name(font: &FontRef, id: StringId) -> Result<String, BuildError> {
    font.localized_strings(id)
        .english_or_first()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or(BuildError::MissingName(id.to_u16()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The most entries one `bfchar` or `bfrange` block may hold (the limit of
/// Adobe's CMap specification, TD §18.3).
const BLOCK_LIMIT: usize = 100;

/// A `/ToUnicode` CMap for `map` (code → character), uncompressed (TD §18.3).
///
/// Two-byte codes over `<0000> <FFFF>`. A run of consecutive codes mapping to
/// consecutive BMP characters, with neither the code nor the character
/// crossing a 256 boundary (only the last byte may vary, ISO 32000-1 §9.10.3),
/// becomes one `bfrange` entry; every other code is a `bfchar` singleton, a
/// supplementary-plane character written as its UTF-16 surrogate pair. Blocks
/// hold at most 100 entries; all `bfchar` blocks come before all `bfrange`
/// blocks. The callers pass only the codes a document uses.
// T-30 (template assembly) is the first caller outside the tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_tounicode(map: &BTreeMap<u16, char>) -> Vec<u8> {
    let text: BTreeMap<u16, String> = map.iter().map(|(&k, &c)| (k, c.to_string())).collect();
    build_tounicode_text(&text)
}

/// [`build_tounicode`] with each code's text a string: a code whose text is
/// several characters (a ligature, a conjunct, a combining sequence) is a
/// `bfchar` singleton mapping to all of them, as a C7 file's surviving
/// `/ToUnicode` maps it (C8-01). One-character texts are written as
/// `build_tounicode` writes them; an empty text is left out.
pub(crate) fn build_tounicode_text(map: &BTreeMap<u16, String>) -> Vec<u8> {
    let mut singles: Vec<(u16, &str)> = Vec::new();
    let mut ranges: Vec<(u16, u16, char)> = Vec::new();
    // `(code, the text, its one character when it has exactly one)`.
    let one = |t: &str| {
        let mut chars = t.chars();
        chars.next().filter(|_| chars.next().is_none())
    };
    let entries: Vec<(u16, &str, Option<char>)> = map
        .iter()
        .filter(|(_, t)| !t.is_empty())
        .map(|(&k, t)| (k, t.as_str(), one(t)))
        .collect();
    let mut i = 0;
    while i < entries.len() {
        let (lo, text, first) = entries[i];
        let mut j = i;
        while let (Some(&(code, _, Some(c))), (prev_code, _, Some(prev_c))) =
            (entries.get(j + 1), entries[j])
        {
            let continues = u32::from(code) == u32::from(prev_code) + 1
                && u32::from(c) == u32::from(prev_c) + 1
                && code & 0xff != 0
                && u32::from(c) & 0xff != 0
                && u32::from(c) <= 0xffff;
            if !continues {
                break;
            }
            j += 1;
        }
        match first {
            Some(c) if j > i => ranges.push((lo, entries[j].0, c)),
            _ => singles.push((lo, text)),
        }
        i = j + 1;
    }

    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for block in singles.chunks(BLOCK_LIMIT) {
        let _ = writeln!(s, "{} beginbfchar", block.len());
        for &(code, text) in block {
            let hex: String = text.chars().map(utf16_hex).collect();
            let _ = writeln!(s, "<{code:04X}> <{hex}>");
        }
        s.push_str("endbfchar\n");
    }
    for block in ranges.chunks(BLOCK_LIMIT) {
        let _ = writeln!(s, "{} beginbfrange", block.len());
        for &(lo, hi, c) in block {
            let _ = writeln!(s, "<{lo:04X}> <{hi:04X}> <{}>", utf16_hex(c));
        }
        s.push_str("endbfrange\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// `c` as UTF-16BE hex, four digits per code unit.
fn utf16_hex(c: char) -> String {
    let mut units = [0u16; 2];
    c.encode_utf16(&mut units)
        .iter()
        .fold(String::new(), |mut s, u| {
            let _ = write!(s, "{u:04X}");
            s
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use crate::pdf::fixtures::{GOLDEN_TEXT, TEST_FONT, golden_pdf};
    use crate::pdf::fontdb::gmap::GmapTable;
    use lopdf::{Document, Encoding, Object};

    /// `tests/data/README.md`'s hash of the test font.
    const TEST_FONT_SHA256: &str =
        "6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383";

    #[test]
    fn test_font_gives_104_cmap_records() {
        let (entry, gmap) = build_from_ttf(TEST_FONT).expect("test font builds");
        let table = GmapTable::new(&gmap).expect("valid gmap");
        assert_eq!(table.len(), 104);
        assert_eq!(gmap.len(), 104 * gmap::RECORD_LEN);
        // GSUB was dropped from the subset: nothing is shaped.
        assert!(table.records().iter().all(|r| r.source() == Source::Cmap));
        assert_eq!(entry.sha256, TEST_FONT_SHA256);
        assert_eq!(entry.gmap_sha256, sha256_hex(&gmap));
    }

    #[test]
    fn build_from_ttf_is_deterministic() {
        let a = build_from_ttf(TEST_FONT).unwrap();
        let b = build_from_ttf(TEST_FONT).unwrap();
        assert_eq!(a, b);
        let json_a = serde_json::to_string(&a.0).unwrap();
        assert_eq!(json_a, serde_json::to_string(&b.0).unwrap());
        // The entry survives the trip through fontindex.json.
        let back: IndexEntry = serde_json::from_str(&json_a).unwrap();
        assert_eq!(back, a.0);
    }

    #[test]
    fn records_are_the_fonts_charmap_with_scaled_hmtx_widths() {
        let font = FontRef::new(TEST_FONT).unwrap();
        let upem = u32::from(font.head().unwrap().units_per_em());
        let hmtx = font.hmtx().unwrap();
        let expected: BTreeMap<(u16, u32), u16> = font
            .charmap()
            .mappings()
            .map(|(cp, gid)| {
                let gid16 = u16::try_from(gid.to_u32()).unwrap();
                let width = u32::from(hmtx.advance(gid).unwrap()) * 1000 / upem;
                ((gid16, cp), u16::try_from(width).unwrap())
            })
            .collect();
        let (_, gmap) = build_from_ttf(TEST_FONT).unwrap();
        let table = GmapTable::new(&gmap).unwrap();
        let got: BTreeMap<(u16, u32), u16> = table
            .records()
            .iter()
            .map(|r| ((r.gid(), u32::from(r.unicode())), r.width()))
            .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn index_entry_describes_the_test_font() {
        let (entry, _) = build_from_ttf(TEST_FONT).unwrap();
        assert_eq!(entry.id, "NotoSans-Regular");
        assert_eq!(entry.postscript_name, "NotoSans-Regular");
        assert_eq!(entry.family, "Noto Sans");
        assert!(entry.base_font_aliases.is_empty());
        // The subset has é è ç ñ ü œ but not à, nor á í ó ú.
        assert_eq!(entry.languages, ["en"]);
        assert_eq!(entry.scripts, ["Latn"]);
        assert_eq!(entry.flags, FLAG_NONSYMBOLIC);
        assert_eq!(entry.italic_angle, 0);

        // The same metrics the golden's FontDescriptor carries.
        let doc = Document::load_mem(&golden_pdf()).unwrap();
        let descriptor = doc
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .find(|d| d.has_type(b"FontDescriptor"))
            .expect("golden has a descriptor");
        let int = |k: &[u8]| descriptor.get(k).unwrap().as_i64().unwrap();
        let bbox: Vec<i64> = descriptor
            .get(b"FontBBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_i64().unwrap())
            .collect();
        let entry_bbox: Vec<i64> = entry.bbox.iter().map(|&v| i64::from(v)).collect();
        assert_eq!(entry_bbox, bbox);
        assert_eq!(i64::from(entry.ascent), int(b"Ascent"));
        assert_eq!(i64::from(entry.descent), int(b"Descent"));
        assert_eq!(i64::from(entry.cap_height), int(b"CapHeight"));
        assert_eq!(i64::from(entry.flags), int(b"Flags"));
    }

    /// The test font with one table tag in its directory overwritten.
    fn without_table(tag: &[u8; 4]) -> Vec<u8> {
        let mut bytes = TEST_FONT.to_vec();
        let num_tables = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let at = (0..num_tables)
            .map(|i| 12 + 16 * i)
            .find(|&at| &bytes[at..at + 4] == tag)
            .expect("table present");
        bytes[at..at + 4].copy_from_slice(b"zzzz");
        bytes
    }

    #[test]
    fn unusable_fonts_are_refused() {
        assert!(matches!(
            build_from_ttf(b"not a font"),
            Err(BuildError::Parse(_))
        ));
        assert_eq!(
            build_from_ttf(&without_table(b"glyf")),
            Err(BuildError::NotTrueType)
        );
        assert_eq!(
            build_from_ttf(&without_table(b"cmap")),
            Err(BuildError::MissingTable("cmap"))
        );
        assert_eq!(
            build_from_ttf(&without_table(b"hmtx")),
            Err(BuildError::MissingTable("hmtx"))
        );
    }

    /// The test font's tables plus `extra`, laid out afresh (table checksums
    /// are left 0: nothing here checks them).
    fn with_table(extra_tag: &[u8; 4], extra: &[u8]) -> Vec<u8> {
        let src = TEST_FONT;
        let be16 = |at: usize| usize::from(u16::from_be_bytes([src[at], src[at + 1]]));
        let be32 = |at: usize| {
            usize::try_from(u32::from_be_bytes(src[at..at + 4].try_into().unwrap())).unwrap()
        };
        let mut tables: Vec<([u8; 4], &[u8])> = (0..be16(4))
            .map(|i| {
                let rec = 12 + 16 * i;
                let (offset, len) = (be32(rec + 8), be32(rec + 12));
                (
                    src[rec..rec + 4].try_into().unwrap(),
                    &src[offset..offset + len],
                )
            })
            .collect();
        tables.push((*extra_tag, extra));
        tables.sort_by_key(|(tag, _)| *tag);

        let n = u16::try_from(tables.len()).unwrap();
        let mut out = src[..4].to_vec();
        for v in [n, 0, 0, 0] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend_from_slice(tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            body.extend_from_slice(data);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            offset = 12 + 16 * tables.len() + body.len();
        }
        out.extend_from_slice(&body);
        out
    }

    /// A GSUB table whose one `liga` lookup (DFLT and latn) turns
    /// `first second` into `ligature`.
    fn liga_gsub(first: u16, second: u16, ligature: u16) -> Vec<u8> {
        let words: Vec<u16> = vec![
            // Header: version 1.0, ScriptList at 10, FeatureList at 46,
            // LookupList at 60.
            1, 0, 10, 46, 60, // ScriptList (10): DFLT and latn, both to the Script at 24.
            2, 0x4446, 0x4c54, 14, 0x6c61, 0x746e, 14,
            // Script (24): default LangSys at 4, no others.
            4, 0, // LangSys (28): no reorder, no required feature, feature 0.
            0, 0xffff, 1, 0, // Padding to 46.
            0, 0, 0, 0, 0, // FeatureList (46): liga at 8.
            1, 0x6c69, 0x6761, 8, // Feature (54): no params, lookup 0.
            0, 1, 0, // LookupList (60): one lookup at 4.
            1, 4, // Lookup (64): type 4, no flags, one subtable at 8.
            4, 0, 1, 8, // LigatureSubstFormat1 (72): coverage at 8, one set at 14.
            1, 8, 1, 14, // Coverage (80): format 1, `first`.
            1, 1, first, // LigatureSet (86): one ligature at 4.
            1, 4, // Ligature (90): `ligature` from two components, then `second`.
            ligature, 2, second,
        ];
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    #[test]
    fn shaped_ligatures_are_recorded_with_their_presentation_form() {
        let font = FontRef::new(TEST_FONT).unwrap();
        let charmap = font.charmap();
        let gid = |c: char| u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap();
        let mapped: BTreeSet<u16> = charmap
            .mappings()
            .map(|(_, g)| u16::try_from(g.to_u32()).unwrap())
            .collect();
        let num_glyphs = font.maxp().unwrap().num_glyphs();
        let spare = (1..num_glyphs)
            .find(|g| !mapped.contains(g))
            .expect("the subset has a glyph no code point maps");

        let bytes = with_table(b"GSUB", &liga_gsub(gid('f'), gid('i'), spare));
        let (entry, gmap) = build_from_ttf(&bytes).expect("font with GSUB builds");
        let table = GmapTable::new(&gmap).unwrap();
        assert_eq!(table.len(), 105);
        let shaped: Vec<(u16, char, Source)> = table
            .records()
            .iter()
            .filter(|r| r.source() == Source::Shaped)
            .map(|r| (r.gid(), r.unicode(), r.source()))
            .collect();
        assert_eq!(shaped, [(spare, '\u{fb01}', Source::Shaped)]);
        let advance = font.hmtx().unwrap().advance(GlyphId::new(u32::from(spare)));
        let upem = u32::from(font.head().unwrap().units_per_em());
        let width = u32::from(advance.unwrap()) * 1000 / upem;
        assert_eq!(u32::from(table.width(spare).unwrap()), width);
        assert_eq!(entry.gmap_sha256, sha256_hex(&gmap));
        assert_eq!(build_from_ttf(&bytes).unwrap().1, gmap);

        // A ligature onto a glyph the cmap already gives the presentation
        // form adds no record; one onto a letter's own glyph is no ligature.
        let same_as_letter = with_table(b"GSUB", &liga_gsub(gid('f'), gid('i'), gid('f')));
        assert_eq!(
            GmapTable::new(&build_from_ttf(&same_as_letter).unwrap().1)
                .unwrap()
                .len(),
            104
        );
        // Without the GSUB the rebuilt font gives the plain 104 records.
        let plain = with_table(b"zzzz", &[]);
        assert_eq!(
            build_from_ttf(&plain).unwrap().1,
            build_from_ttf(TEST_FONT).unwrap().1
        );
    }

    /// code → character for every glyph the goldens draw, read from the test
    /// font's `cmap` independently of the fixtures.
    fn golden_map() -> BTreeMap<u16, char> {
        let font = FontRef::new(TEST_FONT).unwrap();
        let charmap = font.charmap();
        GOLDEN_TEXT
            .iter()
            .flat_map(|page| page.iter())
            .flat_map(|line| line.chars())
            .map(|c| {
                let gid = charmap.map(c).expect("test font maps golden text");
                (u16::try_from(gid.to_u32()).unwrap(), c)
            })
            .collect()
    }

    /// The decoded `/ToUnicode` stream of the golden's Type0 font.
    fn golden_tounicode() -> Vec<u8> {
        let doc = Document::load_mem(&golden_pdf()).unwrap();
        let type0 = doc
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .find(|d| d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Type0"))
            .expect("golden has a Type0 font");
        let id = type0.get(b"ToUnicode").unwrap().as_reference().unwrap();
        let Object::Stream(stream) = doc.get_object(id).unwrap() else {
            panic!("ToUnicode is a stream");
        };
        stream.get_plain_content().unwrap()
    }

    /// T-03a's hand-built CMap is all `bfchar`; the golden's map has runs
    /// (`D`–`F`, `a`–`z`, `ç`–`é`), which the builder writes as `bfrange`. So
    /// the two are compared as what they mean, plus every byte the two forms
    /// share: the prelude, the epilogue and each singleton's line.
    #[test]
    fn golden_map_gives_the_hand_built_cmap() {
        let built = String::from_utf8(build_tounicode(&golden_map())).unwrap();
        let golden = String::from_utf8(golden_tounicode()).unwrap();
        assert_eq!(
            reload(built.clone().into_bytes()),
            reload(golden.clone().into_bytes())
        );

        let prelude_end = |s: &str| s.find("endcodespacerange\n").unwrap() + 18;
        assert_eq!(built[..prelude_end(&built)], golden[..prelude_end(&golden)]);
        let epilogue = "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
        assert!(built.ends_with(epilogue) && golden.ends_with(epilogue));

        let golden_lines: BTreeSet<&str> = golden.lines().collect();
        let singles: Vec<&str> = built
            .lines()
            .skip_while(|l| !l.ends_with("beginbfchar"))
            .skip(1)
            .take_while(|l| *l != "endbfchar")
            .collect();
        assert_eq!(singles.len(), 12);
        for line in singles {
            assert!(golden_lines.contains(line), "{line} is in the golden CMap");
        }
    }

    /// `cmap` loaded by lopdf as a font's `/ToUnicode`, then every two-byte
    /// code looked up.
    fn reload(cmap: Vec<u8>) -> BTreeMap<u16, Vec<u16>> {
        let mut doc = Document::with_version("1.7");
        let stream = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), cmap));
        let mut font = lopdf::Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(b"Type0".to_vec()));
        font.set("ToUnicode", Object::Reference(stream));
        let Ok(Encoding::UnicodeMapEncoding(map)) = font.get_font_encoding(&doc) else {
            panic!("lopdf reads the CMap");
        };
        (0..=u16::MAX)
            .filter_map(|code| map.get(u32::from(code), 2).map(|u| (code, u)))
            .collect()
    }

    fn utf16(c: char) -> Vec<u16> {
        let mut units = [0u16; 2];
        c.encode_utf16(&mut units).to_vec()
    }

    /// A map with runs, singletons, runs over 100 long, 256-boundary
    /// crossings in the code and in the character, and supplementary-plane
    /// characters.
    fn mixed_map() -> BTreeMap<u16, char> {
        let mut map = BTreeMap::new();
        // 300 isolated codes.
        for i in 0..300u16 {
            map.insert(i * 2, char::from_u32(0x4e00 + u32::from(i) * 3).unwrap());
        }
        // 150 runs of three.
        for i in 0..150u16 {
            for k in 0..3u16 {
                map.insert(
                    0x1000 + i * 8 + k,
                    char::from_u32(0x41 + u32::from(k)).unwrap(),
                );
            }
        }
        // One long run crossing code and character 256 boundaries.
        for k in 0..600u16 {
            map.insert(0x3010 + k, char::from_u32(0x0480 + u32::from(k)).unwrap());
        }
        // Supplementary plane, consecutive codes and characters.
        for k in 0..5u16 {
            map.insert(0x8000 + k, char::from_u32(0x1f600 + u32::from(k)).unwrap());
        }
        map
    }

    #[test]
    fn tounicode_reloads_in_lopdf() {
        for map in [golden_map(), mixed_map()] {
            let expected: BTreeMap<u16, Vec<u16>> =
                map.iter().map(|(&code, &c)| (code, utf16(c))).collect();
            assert_eq!(reload(build_tounicode(&map)), expected);
        }
    }

    #[test]
    fn blocks_never_exceed_100_entries() {
        let cmap = String::from_utf8(build_tounicode(&mixed_map())).unwrap();
        let (mut blocks, mut singles, mut ranges) = (0, 0, 0);
        let mut lines = cmap.lines();
        while let Some(line) = lines.next() {
            let Some((n, kind)) = line.split_once(' ') else {
                continue;
            };
            if kind != "beginbfchar" && kind != "beginbfrange" {
                continue;
            }
            let n: usize = n.parse().unwrap();
            assert!((1..=100).contains(&n), "{line}");
            let end = if kind == "beginbfchar" {
                "endbfchar"
            } else {
                "endbfrange"
            };
            let body: Vec<&str> = lines.by_ref().take_while(|l| *l != end).collect();
            assert_eq!(body.len(), n, "{line}: count matches the entries");
            if kind == "beginbfchar" {
                singles += n;
            } else {
                ranges += n;
                for entry in body {
                    let hex: Vec<u32> = entry
                        .split(' ')
                        .map(|h| u32::from_str_radix(h.trim_matches(['<', '>']), 16).unwrap())
                        .collect();
                    let [lo, hi, dst] = hex[..] else {
                        panic!("{entry}");
                    };
                    // Only the last byte varies, in the code and in the character.
                    assert_eq!(lo >> 8, hi >> 8, "{entry}");
                    assert!((dst & 0xff) + (hi - lo) <= 0xff, "{entry}");
                }
            }
            blocks += 1;
        }
        // 305 singletons in four bfchar blocks; 150 runs of three plus the long
        // run cut at codes 0x3100, 0x3200 and characters U+0500, U+0600 make
        // 155 ranges in two bfrange blocks.
        assert_eq!((blocks, singles, ranges), (6, 305, 155));
    }

    #[test]
    fn several_characters_are_one_bfchar_entry_and_one_character_is_as_before() {
        // A Devanagari conjunct, a lam-alef and a combining sequence keep all
        // their characters; one-character texts give `build_tounicode`'s
        // bytes.
        let mut text: BTreeMap<u16, String> = mixed_map()
            .into_iter()
            .map(|(code, c)| (code, c.to_string()))
            .collect();
        assert_eq!(build_tounicode_text(&text), build_tounicode(&mixed_map()));
        text.insert(0x9000, "\u{915}\u{94D}\u{937}".to_owned());
        text.insert(0x9001, "\u{644}\u{627}".to_owned());
        text.insert(0x9002, "e\u{301}".to_owned());
        let expected: BTreeMap<u16, Vec<u16>> = text
            .iter()
            .map(|(&code, t)| (code, t.encode_utf16().collect()))
            .collect();
        assert_eq!(reload(build_tounicode_text(&text)), expected);
    }

    #[test]
    fn empty_map_gives_a_cmap_with_no_blocks() {
        let cmap = String::from_utf8(build_tounicode(&BTreeMap::new())).unwrap();
        assert!(!cmap.contains("beginbf"));
        assert!(reload(cmap.into_bytes()).is_empty());
    }
}
