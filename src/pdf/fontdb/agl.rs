//! Adobe Glyph List and the glyph-name ladder (T-27a, TD §18.3, FR-01, D-040).
//!
//! [`glyph_name_to_unicode`] is the "Mapping glyph names to Unicode" algorithm of
//! the Adobe Glyph List Specification: drop everything from the first `.`, split
//! the rest on `_`, map each component through the AGL, then the `uniXXXX…` rule,
//! then the `uXXXX[XX]` rule, and concatenate. A component none of those map is a
//! miss, and only a miss reaches the heuristic layer (the pdf.js names and the
//! `GNN`/`gNN`/`cNN` forms), whose output is tagged [`Prov::Heuristic`] with a
//! versioned rule name, so an audit can tell a spec mapping from a guess.
//!
//! pdf.js also maps `f_h`, `f_t` and `T_h` by dropping the underscores. The
//! specification's `_` split already gives the same text, through the AGL, so
//! those names never miss and need no heuristic rule here.
//!
//! The tables live in the generated `agl_table.rs`; the test
//! `agl_table_is_generated_from_the_assets` below is the generator.

// T-27c (decode ladder) is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use super::agl_table::{AGL, PDFJS_EXTRA, ZAPF_DINGBATS};

/// The heuristic layer's version, carried in every [`Prov::Heuristic`] tag.
/// Bump it (and the tags) when a rule changes, so old reports stay readable.
pub(crate) const HEURISTICS_VERSION: &str = "v1";

/// Where one character of a [`Decoded`] text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prov {
    /// A list of the specification: the AGL, or the ITC Zapf Dingbats list
    /// when decoding for that font.
    Agl,
    /// The specification's `uni` + groups of four uppercase hex digits rule.
    UniRule,
    /// The specification's `u` + four to six uppercase hex digits rule.
    URule,
    /// A non-standard rule, named `"<version>:<rule>"`, e.g. `"v1:gNN"`.
    Heuristic(&'static str),
    /// The font's own `/ToUnicode` CMap (the decode ladder, T-27c).
    ToUnicode,
    /// The embedded program's Unicode `cmap`, read from glyph to code point
    /// (the decode ladder, T-27c).
    Cmap,
    /// The CID's character collection, through Adobe's UCS2 CMap for it
    /// (the decode ladder, T-27c).
    CidCollection,
}

/// The text a glyph name maps to, with one [`Prov`] per character of `text`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub(crate) text: String,
    pub(crate) provenance: Vec<Prov>,
}

impl Decoded {
    fn push(&mut self, c: char, prov: Prov) {
        self.text.push(c);
        self.provenance.push(prov);
    }

    fn push_str(&mut self, s: &str, prov: Prov) {
        for c in s.chars() {
            self.push(c, prov);
        }
    }
}

/// Maps a glyph name to text for any font but ZapfDingbats. An unmappable name
/// gives an empty [`Decoded`].
pub(crate) fn glyph_name_to_unicode(name: &str) -> Decoded {
    decode(name, false)
}

/// The same for a font whose PostScript name is ZapfDingbats: the specification
/// looks each component up in the ITC Zapf Dingbats list before the AGL.
pub(crate) fn zapf_dingbats_glyph_name_to_unicode(name: &str) -> Decoded {
    decode(name, true)
}

fn decode(name: &str, zapf: bool) -> Decoded {
    let base = name.split_once('.').map_or(name, |(head, _)| head);
    let mut out = Decoded::default();
    for component in base.split('_') {
        let before = out.text.len();
        spec_component(component, zapf, &mut out);
        if out.text.len() == before {
            heuristic_component(component, &mut out);
        }
    }
    out
}

/// One component through the specification's rules; appends nothing on a miss.
fn spec_component(component: &str, zapf: bool, out: &mut Decoded) {
    if zapf && let Some(text) = lookup(&ZAPF_DINGBATS, component) {
        return out.push_str(text, Prov::Agl);
    }
    if let Some(text) = lookup(&AGL, component) {
        return out.push_str(text, Prov::Agl);
    }
    if let Some(digits) = component.strip_prefix("uni") {
        if digits.len() % 4 == 0 && is_upper_hex(digits) {
            let chars: Option<Vec<char>> = digits
                .as_bytes()
                .chunks(4)
                .map(|group| scalar(group, 16))
                .collect();
            if let Some(chars) = chars {
                for c in chars {
                    out.push(c, Prov::UniRule);
                }
            }
        }
        return;
    }
    if let Some(digits) = component.strip_prefix('u')
        && (4..=6).contains(&digits.len())
        && is_upper_hex(digits)
        && let Some(c) = scalar(digits.as_bytes(), 16)
    {
        out.push(c, Prov::URule);
    }
}

/// The versioned heuristic layer, for a component the specification missed.
fn heuristic_component(component: &str, out: &mut Decoded) {
    if let Some(text) = lookup(&PDFJS_EXTRA, component) {
        // pdf.js maps `controlNULL` to U+0000; a NUL is never useful text.
        if text != "\0" {
            out.push_str(text, Prov::Heuristic("v1:pdfjs"));
        }
        return;
    }
    let bytes = component.as_bytes();
    let Some((&first, digits)) = bytes.split_first() else {
        return;
    };
    let hex = |d: &[u8]| d.iter().all(u8::is_ascii_hexdigit);
    let dec = |d: &[u8]| d.iter().all(u8::is_ascii_digit);
    let hit = match (first, digits.len()) {
        (b'G', 2) if hex(digits) => scalar(digits, 16).map(|c| (c, "v1:GNN")),
        (b'g', 2..=4) if hex(digits) => scalar(digits, 16).map(|c| (c, "v1:gNN")),
        (b'c' | b'C', 2..=3) if dec(digits) => scalar(digits, 10).map(|c| (c, "v1:cNN")),
        // Some producers write hex in the cNN form (pdf.js falls back the same way).
        (b'c' | b'C', 2..=3) if hex(digits) => scalar(digits, 16).map(|c| (c, "v1:cXX")),
        _ => None,
    };
    if let Some((c, rule)) = hit
        && c != '\0'
    {
        out.push(c, Prov::Heuristic(rule));
    }
}

fn lookup(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| table[i].1)
}

fn is_upper_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))
}

/// The Unicode scalar value spelled by ASCII `digits` in `radix`, if any:
/// surrogates and values past U+10FFFF are not scalars.
fn scalar(digits: &[u8], radix: u32) -> Option<char> {
    let s = std::str::from_utf8(digits).ok()?;
    char::from_u32(u32::from_str_radix(s, radix).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    const TABLE_PATH: &str = "src/pdf/fontdb/agl_table.rs";
    const REGEN_VAR: &str = "PDFPUNDIT_REGEN_AGL";

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// A text file of the repo with CRLF folded to LF, so a Windows checkout
    /// hashes and compares the same as the committed bytes.
    fn read_lf(rel: &str) -> Vec<u8> {
        let bytes = std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let mut lf = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                lf.push(b);
            }
        }
        lf
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut s, b| {
                let _ = write!(s, "{b:02x}");
                s
            })
    }

    fn cps(s: &str) -> Vec<u32> {
        s.chars().map(u32::from).collect()
    }

    fn agl(name: &str) -> (Vec<u32>, Vec<Prov>) {
        let d = glyph_name_to_unicode(name);
        (cps(&d.text), d.provenance)
    }

    // ---- the specification's examples (AGL Specification, "Examples") ----

    #[test]
    fn spec_examples_decode() {
        use Prov::*;
        assert_eq!(agl("Lcommaaccent"), (vec![0x013B], vec![Agl]));
        assert_eq!(
            agl("uni20AC0308"),
            (vec![0x20AC, 0x0308], vec![UniRule, UniRule])
        );
        assert_eq!(agl("u1040C"), (vec![0x1040C], vec![URule]));
        assert_eq!(agl("uniD801DC0C"), (vec![], vec![]), "surrogates");
        assert_eq!(agl("uni20ac"), (vec![], vec![]), "lowercase hex");
        assert_eq!(
            agl("Lcommaaccent_uni20AC0308_u1040C.alternate"),
            (
                vec![0x013B, 0x20AC, 0x0308, 0x1040C],
                vec![Agl, UniRule, UniRule, URule]
            )
        );
        assert_eq!(agl("uni013B"), (vec![0x013B], vec![UniRule]));
        assert_eq!(agl("u013B"), (vec![0x013B], vec![URule]));
        assert_eq!(agl("foo"), (vec![], vec![]));
        assert_eq!(agl(".notdef"), (vec![], vec![]));
        assert_eq!(agl("Ogoneksmall"), (vec![0xF6FB], vec![Agl]));
    }

    #[test]
    fn multi_code_point_entries_and_suffixes() {
        use Prov::*;
        assert_eq!(
            agl("dalethatafpatah"),
            (vec![0x05D3, 0x05B2], vec![Agl, Agl])
        );
        assert_eq!(agl("A.sc"), (vec![0x41], vec![Agl]));
        assert_eq!(agl("A.sc.alt"), (vec![0x41], vec![Agl]), "first period");
        assert_eq!(agl("f_f_i"), (vec![0x66, 0x66, 0x69], vec![Agl; 3]));
        assert_eq!(agl("a_.b"), (vec![0x61], vec![Agl]), "empty component");
        assert_eq!(agl(""), (vec![], vec![]));
        assert_eq!(
            agl("foo_A"),
            (vec![0x41], vec![Agl]),
            "a miss maps to empty"
        );
    }

    #[test]
    fn uni_and_u_rule_bounds() {
        use Prov::*;
        // uni: groups of exactly four; BMP minus surrogates; all or nothing.
        assert_eq!(agl("uni0041"), (vec![0x41], vec![UniRule]));
        assert_eq!(agl("uniFFFF"), (vec![0xFFFF], vec![UniRule]));
        assert_eq!(agl("uniE000"), (vec![0xE000], vec![UniRule]));
        assert_eq!(agl("uniD7FF"), (vec![0xD7FF], vec![UniRule]));
        assert_eq!(agl("uni004"), (vec![], vec![]), "not a multiple of four");
        assert_eq!(agl("uni00410042D800"), (vec![], vec![]), "one bad group");
        assert_eq!(agl("uni1040C"), (vec![], vec![]));
        assert_eq!(agl("uni"), (vec![], vec![]));
        // u: four to six digits, up to U+10FFFF, no surrogates.
        assert_eq!(agl("u0041"), (vec![0x41], vec![URule]));
        assert_eq!(agl("u10FFFF"), (vec![0x10FFFF], vec![URule]));
        assert_eq!(agl("u110000"), (vec![], vec![]));
        assert_eq!(agl("uD800"), (vec![], vec![]));
        assert_eq!(agl("u041"), (vec![], vec![]), "three digits");
        assert_eq!(agl("u0010FFFF"), (vec![], vec![]), "eight digits");
        assert_eq!(agl("u00e9"), (vec![], vec![]), "lowercase hex");
    }

    #[test]
    fn provenance_has_one_entry_per_character() {
        for table in [&AGL[..], &ZAPF_DINGBATS[..], &PDFJS_EXTRA[..]] {
            for (name, _) in table {
                for d in [
                    glyph_name_to_unicode(name),
                    zapf_dingbats_glyph_name_to_unicode(name),
                ] {
                    assert_eq!(d.text.chars().count(), d.provenance.len(), "{name}");
                }
            }
        }
        for (name, text) in AGL.iter() {
            let d = glyph_name_to_unicode(name);
            assert_eq!(d.text, *text, "{name}");
            assert!(d.provenance.iter().all(|p| *p == Prov::Agl), "{name}");
        }
    }

    #[test]
    fn pdfjs_ligature_names_decode_through_the_spec() {
        use Prov::*;
        assert_eq!(agl("f_h"), (vec![0x66, 0x68], vec![Agl, Agl]));
        assert_eq!(agl("f_t"), (vec![0x66, 0x74], vec![Agl, Agl]));
        assert_eq!(agl("T_h"), (vec![0x54, 0x68], vec![Agl, Agl]));
    }

    // ---- ZapfDingbats ----

    #[test]
    fn zapf_dingbats_list_applies_only_to_that_font() {
        let z = zapf_dingbats_glyph_name_to_unicode("a1");
        assert_eq!(
            (cps(&z.text), z.provenance),
            (vec![0x2701], vec![Prov::Agl])
        );
        assert_eq!(agl("a1"), (vec![], vec![]));
        // The AGL still applies inside a ZapfDingbats font.
        let s = zapf_dingbats_glyph_name_to_unicode("space");
        assert_eq!(cps(&s.text), vec![0x20]);
    }

    // ---- the heuristic layer, isolated ----

    /// The specification's rules alone, for every component.
    fn spec_only(name: &str) -> Decoded {
        let base = name.split_once('.').map_or(name, |(head, _)| head);
        let mut out = Decoded::default();
        for component in base.split('_') {
            spec_component(component, false, &mut out);
        }
        out
    }

    #[test]
    fn g36_decodes_only_via_heuristic() {
        assert_eq!(spec_only("g36"), Decoded::default());
        let d = glyph_name_to_unicode("g36");
        assert_eq!(d.text, "6");
        assert_eq!(d.provenance, vec![Prov::Heuristic("v1:gNN")]);
    }

    #[test]
    fn heuristic_rules_are_tagged_and_versioned() {
        let h = |name: &str| {
            let d = glyph_name_to_unicode(name);
            assert_eq!(spec_only(name), Decoded::default(), "{name} is a spec miss");
            (d.text, d.provenance)
        };
        let tag = |t: &'static str| vec![Prov::Heuristic(t)];
        assert_eq!(h("G41"), ("A".into(), tag("v1:GNN")));
        assert_eq!(h("g0041"), ("A".into(), tag("v1:gNN")));
        assert_eq!(h("g41.sc"), ("A".into(), tag("v1:gNN")));
        assert_eq!(h("c65"), ("A".into(), tag("v1:cNN")));
        assert_eq!(h("C097"), ("a".into(), tag("v1:cNN")));
        assert_eq!(h("c4F"), ("O".into(), tag("v1:cXX")));
        assert_eq!(h("braceleftBig"), ("{".into(), tag("v1:pdfjs")));
        assert_eq!(h("arrowhookleft"), ("\u{21AA}".into(), tag("v1:pdfjs")));
        // Out of the forms, or a NUL: nothing.
        for miss in [
            "G4",
            "G041",
            "g4",
            "g00041",
            "c6",
            "c6500",
            "cXY",
            "g00",
            "c00",
            "controlNULL",
        ] {
            assert_eq!(h(miss), (String::new(), vec![]), "{miss}");
        }
        for rule in ["v1:GNN", "v1:gNN", "v1:cNN", "v1:cXX", "v1:pdfjs"] {
            assert!(rule.starts_with(&format!("{HEURISTICS_VERSION}:")));
        }
    }

    #[test]
    fn heuristics_run_per_missed_component_only() {
        let d = glyph_name_to_unicode("A_g36_uni0042");
        assert_eq!(d.text, "A6B");
        assert_eq!(
            d.provenance,
            vec![Prov::Agl, Prov::Heuristic("v1:gNN"), Prov::UniRule]
        );
    }

    // ---- the tables and the files they come from ----

    #[test]
    fn asset_files_are_the_pinned_ones() {
        for (rel, len, sha) in [
            (
                "assets/agl/glyphlist.txt",
                78_060,
                "a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789",
            ),
            (
                "assets/agl/zapfdingbats.txt",
                3_879,
                "f6394e3cb8a447e84a1dad75d4baaf2aa7f45dc104faf369f4720e1a774ef2dc",
            ),
            (
                "assets/agl/LICENSE.md",
                1_404,
                "58147d341e7a34aa2196862395a34d2fd95716c41d5ed26efb59ab0e12f92089",
            ),
            (
                "assets/agl/pdfjs-extra.txt",
                3_232,
                "aedabd5fea5c2e8a0e90e21211b1e06344cbfe5ee7cc8ef5bcc6cf23dea86e45",
            ),
            (
                "assets/licenses/pdfjs-Apache-2.0.txt",
                10_174,
                "0d542e0c8804e39aa7f37eb00da5a762149dc682d7829451287e11b938e94594",
            ),
        ] {
            let bytes = read_lf(rel);
            assert_eq!(bytes.len(), len, "{rel} size");
            assert_eq!(sha256_hex(&bytes), sha, "{rel} sha256");
        }
    }

    #[test]
    fn tables_have_the_documented_shape() {
        assert_eq!(AGL.len(), 4_281);
        assert_eq!(
            AGL.iter().filter(|(_, t)| t.chars().count() > 1).count(),
            81
        );
        assert_eq!(ZAPF_DINGBATS.len(), 201);
        assert_eq!(PDFJS_EXTRA.len(), 127);
        for table in [&AGL[..], &ZAPF_DINGBATS[..], &PDFJS_EXTRA[..]] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0), "sorted, unique");
        }
        for (name, _) in PDFJS_EXTRA.iter() {
            assert!(lookup(&AGL, name).is_none(), "{name} is an AGL name");
        }
    }

    // ---- the generator ----

    /// `name;HEX[ HEX…]` lines of an AGL-format file, sorted by name bytes.
    fn parse(rel: &str) -> Vec<(String, String)> {
        let text = String::from_utf8(read_lf(rel)).expect("ASCII");
        let mut rows: Vec<(String, String)> = text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let (name, hex) = l.split_once(';').unwrap_or_else(|| panic!("{rel}: {l}"));
                let value = hex
                    .split(' ')
                    .map(|h| {
                        u32::from_str_radix(h, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .unwrap_or_else(|| panic!("{rel}: {l}"))
                    })
                    .collect();
                (name.to_owned(), value)
            })
            .collect();
        rows.sort();
        let before = rows.len();
        rows.dedup_by(|a, b| a.0 == b.0);
        assert_eq!(rows.len(), before, "{rel}: duplicate names");
        rows
    }

    /// The leading comment block of an AGL-format file, as `//` lines.
    fn header(rel: &str) -> String {
        let text = String::from_utf8(read_lf(rel)).expect("ASCII");
        text.lines()
            .take_while(|l| l.starts_with('#'))
            .map(|l| format!("//{}\n", &l[1..]))
            .collect()
    }

    fn table(out: &mut String, doc: &str, ident: &str, rows: &[(String, String)]) {
        let _ = writeln!(out, "/// {doc}");
        let _ = writeln!(
            out,
            "pub(super) static {ident}: [(&str, &str); {}] = [",
            rows.len()
        );
        for (name, value) in rows {
            let escaped: String = value
                .chars()
                .map(|c| format!("\\u{{{:04X}}}", u32::from(c)))
                .collect();
            let _ = writeln!(out, "    ({name:?}, \"{escaped}\"),");
        }
        out.push_str("];\n");
    }

    fn generate() -> String {
        let mut out = String::new();
        out.push_str(
            "// @generated by the `agl_table_is_generated_from_the_assets` test in agl.rs.\n\
             // Do not edit: run `PDFPUNDIT_REGEN_AGL=1 cargo test agl_table` instead.\n\
             //\n\
             // Sources: assets/agl/glyphlist.txt and assets/agl/zapfdingbats.txt (Adobe,\n\
             // BSD-3-Clause; their notice follows) and assets/agl/pdfjs-extra.txt (Mozilla\n\
             // Foundation, Apache-2.0; its notice follows that).\n\
             //\n",
        );
        out.push_str(&header("assets/agl/glyphlist.txt"));
        out.push_str(&header("assets/agl/pdfjs-extra.txt"));
        out.push('\n');
        table(
            &mut out,
            "The Adobe Glyph List 2.0: glyph name and its text, sorted by name bytes.",
            "AGL",
            &parse("assets/agl/glyphlist.txt"),
        );
        out.push('\n');
        table(
            &mut out,
            "The ITC Zapf Dingbats Glyph List 2.0, sorted by name bytes.",
            "ZAPF_DINGBATS",
            &parse("assets/agl/zapfdingbats.txt"),
        );
        out.push('\n');
        table(
            &mut out,
            "pdf.js names missing from the AGL (heuristic layer only), sorted by name bytes.",
            "PDFJS_EXTRA",
            &parse("assets/agl/pdfjs-extra.txt"),
        );
        out
    }

    #[test]
    fn agl_table_is_generated_from_the_assets() {
        let want = generate();
        let path: &Path = &root().join(TABLE_PATH);
        if std::env::var_os(REGEN_VAR).is_some() {
            std::fs::write(path, &want).expect(TABLE_PATH);
        }
        let have = String::from_utf8(read_lf(TABLE_PATH)).expect("UTF-8");
        assert!(
            have == want,
            "{TABLE_PATH} is stale: rerun with {REGEN_VAR}=1"
        );
        // Generating twice gives the same bytes.
        assert_eq!(generate(), want);
    }
}
