//! T-06 acceptance: filters, decode chains, predictors, classification and
//! the content-operator walk. The inflate statuses are tested beside the
//! inflater (`inflate.rs`).

use super::*;

use lopdf::content::Content;
use lopdf::{Document, StringFormat};

use crate::pdf::fixtures::{GOLDEN_TEXT, TEST_FONT, TINY_JPEG, golden_pdf};

fn name(s: &[u8]) -> Object {
    Object::Name(s.to_vec())
}

fn dict(entries: Vec<(&str, Object)>) -> Dictionary {
    let mut d = Dictionary::new();
    for (k, v) in entries {
        d.set(k, v);
    }
    d
}

fn hexs(s: &[u8]) -> Object {
    Object::String(s.to_vec(), StringFormat::Hexadecimal)
}

fn lit(s: &[u8]) -> Object {
    Object::String(s.to_vec(), StringFormat::Literal)
}

fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// xorshift64: the tests' deterministic generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn noise(seed: u64, len: usize, alphabet: u8) -> Vec<u8> {
    let mut rng = Rng(seed);
    (0..len)
        .map(|_| b'a' + (rng.next() % u64::from(alphabet)) as u8)
        .collect()
}

// ---- encoders, written from ISO 32000-1 7.4, for round trips ----

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(word);
        if chunk.len() == 4 && v == 0 {
            out.push(b'z');
            continue;
        }
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = b'!' + (v % 85) as u8;
            v /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
        if out.len() % 60 < 5 {
            out.push(b'\n');
        }
    }
    out.extend_from_slice(b"~>");
    out
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = data
        .iter()
        .flat_map(|b| format!("{b:02x} ").into_bytes())
        .collect();
    out.push(b'>');
    out
}

/// PNG-predicts `data` (rows of `row` bytes, `bpp` bytes per pixel), using
/// each row's type in turn from `types`.
fn png_predict(data: &[u8], row: usize, bpp: usize, types: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; row];
    for (r, cur) in data.chunks(row).enumerate() {
        let t = types[r % types.len()];
        out.push(t);
        for i in 0..cur.len() {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let pred = match t {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                _ => {
                    let p = i16::from(a) + i16::from(b) - i16::from(c);
                    let (pa, pb, pc) = (
                        (p - i16::from(a)).abs(),
                        (p - i16::from(b)).abs(),
                        (p - i16::from(c)).abs(),
                    );
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
            };
            out.push(cur[i].wrapping_sub(pred));
        }
        prev[..cur.len()].copy_from_slice(cur);
    }
    out
}

/// TIFF predictor 2 for `bpc`-bit samples, `colors` per pixel.
fn tiff_predict(data: &[u8], row: usize, colors: usize, bpc: usize, columns: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for cur in data.chunks(row) {
        let samples = colors * columns;
        let get = |i: usize| -> u32 {
            match bpc {
                16 => u32::from(u16::from_be_bytes([cur[2 * i], cur[2 * i + 1]])),
                _ => {
                    let bit = i * bpc;
                    u32::from(cur[bit / 8] >> (8 - bpc - bit % 8)) & ((1 << bpc) - 1)
                }
            }
        };
        let mask = (1u32 << bpc) - 1;
        let diffs: Vec<u32> = (0..samples)
            .map(|i| {
                let left = if i >= colors { get(i - colors) } else { 0 };
                get(i).wrapping_sub(left) & mask
            })
            .collect();
        let mut row_out = vec![0u8; cur.len()];
        for (i, d) in diffs.iter().enumerate() {
            match bpc {
                16 => row_out[2 * i..2 * i + 2].copy_from_slice(&(*d as u16).to_be_bytes()),
                _ => {
                    let bit = i * bpc;
                    row_out[bit / 8] |= (*d as u8) << (8 - bpc - bit % 8);
                }
            }
        }
        out.extend_from_slice(&row_out);
    }
    out
}

/// PDF LZW with `/EarlyChange` `early`; emits a clear code when the table fills.
fn lzw(data: &[u8], early: u32) -> Vec<u8> {
    lzw_with(data, early, true)
}

/// PDF LZW; with `clear` false a full table is kept (no new entries, 12-bit
/// codes) until the end of the data, as a decoder must accept.
fn lzw_with(data: &[u8], early: u32, clear: bool) -> Vec<u8> {
    fn width(next: u32, early: u32) -> u32 {
        match next + early {
            0..512 => 9,
            512..1024 => 10,
            1024..2048 => 11,
            _ => 12,
        }
    }
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u64, 0u32);
    let mut emit = |code: u32, w: u32, out: &mut Vec<u8>| {
        acc = (acc << w) | u64::from(code);
        bits += w;
        while bits >= 8 {
            out.push((acc >> (bits - 8)) as u8);
            bits -= 8;
        }
    };
    let mut table: std::collections::BTreeMap<Vec<u8>, u32> = std::collections::BTreeMap::new();
    // `dec_next` is the decoder's next code when it reads what we emit.
    let (mut enc_next, mut dec_next, mut first) = (258u32, 258u32, true);
    emit(256, 9, &mut out);
    let mut w: Vec<u8> = Vec::new();
    for &c in data {
        let mut wc = w.clone();
        wc.push(c);
        if w.is_empty() || wc.len() == 1 || table.contains_key(&wc) {
            w = wc;
            continue;
        }
        let code = if w.len() == 1 {
            u32::from(w[0])
        } else {
            table[&w]
        };
        emit(code, width(dec_next, early), &mut out);
        if !first && dec_next < 4096 {
            dec_next += 1;
        }
        first = false;
        if enc_next < 4096 {
            table.insert(wc, enc_next);
            enc_next += 1;
        }
        w = vec![c];
        if clear && enc_next == 4096 - early {
            emit(256, width(dec_next, early), &mut out);
            table.clear();
            enc_next = 258;
            dec_next = 258;
            first = true;
        }
    }
    if !w.is_empty() {
        let code = if w.len() == 1 {
            u32::from(w[0])
        } else {
            table[&w]
        };
        emit(code, width(dec_next, early), &mut out);
        if !first && dec_next < 4096 {
            dec_next += 1;
        }
    }
    emit(257, width(dec_next, early), &mut out);
    if bits > 0 {
        out.push((acc << (8 - bits)) as u8);
    }
    out
}

// ---- filters_of ----

#[test]
fn filters_of_reads_names_arrays_and_abbreviations() {
    let d = dict(vec![("Filter", name(b"FlateDecode"))]);
    assert_eq!(filters_of(&d), vec![(Filter::Flate, None)]);

    let parms = dict(vec![("Predictor", Object::Integer(12))]);
    let d = dict(vec![
        (
            "Filter",
            Object::Array(vec![name(b"ASCII85Decode"), name(b"FlateDecode")]),
        ),
        (
            "DecodeParms",
            Object::Array(vec![Object::Null, Object::Dictionary(parms.clone())]),
        ),
    ]);
    assert_eq!(
        filters_of(&d),
        vec![
            (Filter::Ascii85, None),
            (Filter::Flate, Some(parms.clone()))
        ]
    );

    let all = [
        (&b"AHx"[..], Filter::AsciiHex),
        (b"A85", Filter::Ascii85),
        (b"LZW", Filter::Lzw),
        (b"Fl", Filter::Flate),
        (b"RL", Filter::RunLength),
        (b"CCF", Filter::Ccitt),
        (b"DCT", Filter::Dct),
        (b"ASCIIHexDecode", Filter::AsciiHex),
        (b"LZWDecode", Filter::Lzw),
        (b"RunLengthDecode", Filter::RunLength),
        (b"CCITTFaxDecode", Filter::Ccitt),
        (b"DCTDecode", Filter::Dct),
        (b"JPXDecode", Filter::Jpx),
        (b"JBIG2Decode", Filter::Jbig2),
        (b"Crypt", Filter::Unknown("Crypt".into())),
    ];
    for (n, f) in all {
        assert_eq!(
            filters_of(&dict(vec![("Filter", name(n))])),
            vec![(f, None)]
        );
    }

    // A dictionary of parameters with one filter belongs to it.
    let d = dict(vec![
        ("Filter", name(b"LZWDecode")),
        ("DecodeParms", Object::Dictionary(parms.clone())),
    ]);
    assert_eq!(filters_of(&d), vec![(Filter::Lzw, Some(parms.clone()))]);

    // Inline-image keys.
    let d = dict(vec![("F", name(b"AHx"))]);
    assert_eq!(filters_of(&d), vec![(Filter::AsciiHex, None)]);

    assert_eq!(filters_of(&Dictionary::new()), vec![]);
    // An indirect /Filter cannot be resolved here: not "unfiltered".
    let d = dict(vec![("Filter", Object::Reference((4, 0)))]);
    assert_eq!(
        filters_of(&d),
        vec![(Filter::Unknown("<Reference>".into()), None)]
    );

    // A null /Filter, or a null array element, is absent; parameters stay
    // with their filters.
    let d = dict(vec![("Filter", Object::Null)]);
    assert_eq!(filters_of(&d), vec![]);
    let d = dict(vec![
        (
            "Filter",
            Object::Array(vec![name(b"A85"), Object::Null, name(b"Fl")]),
        ),
        (
            "DecodeParms",
            Object::Array(vec![
                Object::Null,
                Object::Null,
                Object::Dictionary(parms.clone()),
            ]),
        ),
    ]);
    assert_eq!(
        filters_of(&d),
        vec![
            (Filter::Ascii85, None),
            (Filter::Flate, Some(parms.clone()))
        ]
    );
}

#[test]
fn unreadable_decode_parms_are_marked_and_refused() {
    let data = noise(11, 30, 26);
    let predicted = png_predict(&data, 3, 1, &[2]);
    let mut stand_in = Dictionary::new();
    stand_in.set(UNRESOLVED_PARMS, Object::Reference((5, 0)));
    // `/DecodeParms 5 0 R` on one filter.
    let d = dict(vec![
        ("Filter", name(b"FlateDecode")),
        ("DecodeParms", Object::Reference((5, 0))),
    ]);
    let chain = filters_of(&d);
    assert_eq!(chain, vec![(Filter::Flate, Some(stand_in.clone()))]);
    assert_eq!(
        decode_chain(&zlib(&predicted), &chain, DEFAULT_CAP),
        Err(DecodeError::BadParms(Filter::Flate))
    );
    // `[null 7 0 R]` on `[/A85 /LZW]`.
    let d = dict(vec![
        (
            "Filter",
            Object::Array(vec![name(b"ASCII85Decode"), name(b"LZWDecode")]),
        ),
        (
            "DecodeParms",
            Object::Array(vec![Object::Null, Object::Reference((7, 0))]),
        ),
    ]);
    let chain = filters_of(&d);
    stand_in.set(UNRESOLVED_PARMS, Object::Reference((7, 0)));
    assert_eq!(
        chain,
        vec![(Filter::Ascii85, None), (Filter::Lzw, Some(stand_in))]
    );
    assert_eq!(
        decode_chain(&ascii85(&lzw(&predicted, 1)), &chain, DEFAULT_CAP),
        Err(DecodeError::BadParms(Filter::Lzw))
    );
    // Resolved by the caller, the same stream decodes.
    let parms = dict(vec![
        ("Predictor", Object::Integer(12)),
        ("Columns", Object::Integer(3)),
    ]);
    let d = dict(vec![
        ("Filter", name(b"FlateDecode")),
        ("DecodeParms", Object::Dictionary(parms)),
    ]);
    assert_eq!(
        decode_chain(&zlib(&predicted), &filters_of(&d), DEFAULT_CAP),
        Ok(data)
    );
}

#[test]
fn one_parms_dictionary_goes_to_the_filter_that_reads_it() {
    let data = noise(12, 60, 26);
    let predicted = png_predict(&data, 3, 1, &[1, 2, 4, 3]);
    let parms = dict(vec![
        ("Predictor", Object::Integer(12)),
        ("Columns", Object::Integer(3)),
    ]);
    let d = dict(vec![
        ("Filter", Object::Array(vec![name(b"A85"), name(b"Fl")])),
        ("DecodeParms", Object::Dictionary(parms.clone())),
    ]);
    let chain = filters_of(&d);
    assert_eq!(
        chain,
        vec![(Filter::Ascii85, None), (Filter::Flate, Some(parms))]
    );
    assert_eq!(
        decode_chain(&ascii85(&zlib(&predicted)), &chain, DEFAULT_CAP),
        Ok(data)
    );
}

// ---- decode_chain ----

#[test]
fn ascii85_then_flate_decodes_left_to_right() {
    let data = noise(1, 10_000, 7);
    let raw = ascii85(&zlib(&data));
    let d = dict(vec![(
        "Filter",
        Object::Array(vec![name(b"ASCII85Decode"), name(b"FlateDecode")]),
    )]);
    assert_eq!(
        decode_chain(&raw, &filters_of(&d), DEFAULT_CAP),
        Ok(data.clone())
    );
    // Bare filters work too (T-08 decodes the stages before Flate this way).
    assert_eq!(
        decode_chain(&raw, &[Filter::Ascii85], DEFAULT_CAP),
        Ok(zlib(&data))
    );
    let hex = ascii_hex(&zlib(&data));
    assert_eq!(
        decode_chain(&hex, &[Filter::AsciiHex, Filter::Flate], DEFAULT_CAP),
        Ok(data)
    );
}

#[test]
fn ascii_filters_are_tolerant_where_readers_agree() {
    // `<~` prefix, `z`, whitespace, a final partial group, missing `~>`.
    let mut raw = b"<~z \n".to_vec();
    let hello = ascii85(b"Hello, World!");
    raw.extend_from_slice(&hello[..hello.len() - 2]);
    assert_eq!(
        decode_chain(&raw, &[Filter::Ascii85], DEFAULT_CAP),
        Ok(b"\0\0\0\0Hello, World!".to_vec())
    );
    // An odd final digit is padded with 0; text after `>` is ignored.
    assert_eq!(
        decode_chain(b"48 65\n6c7>junk", &[Filter::AsciiHex], DEFAULT_CAP),
        Ok(b"Hel\x70".to_vec())
    );
    assert_eq!(
        decode_chain(b"48 6x", &[Filter::AsciiHex], DEFAULT_CAP),
        Err(DecodeError::Corrupt {
            filter: Filter::AsciiHex,
            at: 4
        })
    );
    assert_eq!(
        decode_chain(b"87cU{", &[Filter::Ascii85], DEFAULT_CAP),
        Err(DecodeError::Corrupt {
            filter: Filter::Ascii85,
            at: 4
        })
    );
}

#[test]
fn run_length_decodes_literal_and_repeat_runs() {
    // 3 literal bytes, `x` repeated 5 times, EOD, then junk.
    let raw = b"\x02abc\xfcx\x80zz";
    assert_eq!(
        decode_chain(raw, &[Filter::RunLength], DEFAULT_CAP),
        Ok(b"abcxxxxx".to_vec())
    );
}

#[test]
fn lzw_decodes_the_iso_example_and_round_trips() {
    // ISO 32000-1 7.4.4.2, Example 2.
    let raw = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
    assert_eq!(
        decode_chain(&raw, &[Filter::Lzw], DEFAULT_CAP),
        Ok(b"-----A---B".to_vec())
    );
    // Enough data to cross the 10-, 11- and 12-bit widths and a clear code.
    let data: Vec<u8> = noise(7, 40_000, 20);
    for early in [1, 0] {
        let parms = dict(vec![("EarlyChange", Object::Integer(i64::from(early)))]);
        let chain = [(Filter::Lzw, Some(parms))];
        assert_eq!(
            decode_chain(&lzw(&data, early), &chain, DEFAULT_CAP),
            Ok(data.clone()),
            "EarlyChange {early}"
        );
    }
}

#[test]
fn lzw_keeps_decoding_with_a_full_table() {
    // No clear code: past 4096 codes the table stops growing at 12 bits.
    let data: Vec<u8> = noise(8, 60_000, 20);
    for early in [1, 0] {
        let full = lzw_with(&data, early, false);
        assert_ne!(full, lzw(&data, early), "the encoder never cleared");
        let parms = dict(vec![("EarlyChange", Object::Integer(i64::from(early)))]);
        assert_eq!(
            decode_chain(&full, &[(Filter::Lzw, Some(parms))], DEFAULT_CAP),
            Ok(data.clone()),
            "EarlyChange {early}"
        );
    }
}

#[test]
fn png_predictors_round_trip() {
    for (colors, bpc, columns) in [
        (3usize, 8usize, 5usize),
        (1, 8, 17),
        (2, 16, 7),
        (1, 1, 13),
        (3, 4, 9),
    ] {
        let row = (colors * bpc * columns).div_ceil(8);
        let bpp = (colors * bpc).div_ceil(8);
        let data = noise(colors as u64 * 31 + bpc as u64, row * 12, 26);
        let predicted = png_predict(&data, row, bpp, &[0, 1, 2, 3, 4, 4, 2]);
        let parms = dict(vec![
            ("Predictor", Object::Integer(15)),
            ("Colors", Object::Integer(colors as i64)),
            ("BitsPerComponent", Object::Integer(bpc as i64)),
            ("Columns", Object::Integer(columns as i64)),
        ]);
        let chain = [(Filter::Flate, Some(parms))];
        assert_eq!(
            decode_chain(&zlib(&predicted), &chain, DEFAULT_CAP),
            Ok(data),
            "colors {colors} bpc {bpc} columns {columns}"
        );
    }
}

#[test]
fn tiff_predictor_round_trips() {
    for (colors, bpc, columns) in [
        (3usize, 8usize, 5usize),
        (1, 16, 9),
        (1, 1, 13),
        (2, 4, 7),
        (4, 2, 3),
    ] {
        let row = (colors * bpc * columns).div_ceil(8);
        let mut data = noise(colors as u64 * 7 + bpc as u64, row * 10, 26);
        // Padding bits at the end of each row are not samples: keep them 0.
        let pad = row * 8 - colors * bpc * columns;
        for r in data.chunks_mut(row) {
            r[row - 1] &= (0xffu16 << pad) as u8;
        }
        let parms = dict(vec![
            ("Predictor", Object::Integer(2)),
            ("Colors", Object::Integer(colors as i64)),
            ("BitsPerComponent", Object::Integer(bpc as i64)),
            ("Columns", Object::Integer(columns as i64)),
        ]);
        let predicted = tiff_predict(&data, row, colors, bpc, columns);
        let chain = [(Filter::Lzw, Some(parms))];
        assert_eq!(
            decode_chain(&lzw(&predicted, 1), &chain, DEFAULT_CAP),
            Ok(data),
            "colors {colors} bpc {bpc} columns {columns}"
        );
    }
}

#[test]
fn bad_predictor_parameters_are_refused() {
    let z = zlib(b"\x00abc");
    for parms in [
        dict(vec![("Predictor", Object::Integer(7))]),
        dict(vec![
            ("Predictor", Object::Integer(12)),
            ("BitsPerComponent", Object::Integer(3)),
        ]),
        dict(vec![
            ("Predictor", Object::Integer(12)),
            ("Columns", Object::Integer(0)),
        ]),
    ] {
        assert_eq!(
            decode_chain(&z, &[(Filter::Flate, Some(parms))], DEFAULT_CAP),
            Err(DecodeError::BadParms(Filter::Flate))
        );
    }
    let parms = dict(vec![
        ("Predictor", Object::Integer(12)),
        ("Columns", Object::Integer(3)),
    ]);
    assert_eq!(
        decode_chain(
            &zlib(b"\x07abc"),
            &[(Filter::Flate, Some(parms))],
            DEFAULT_CAP
        ),
        Err(DecodeError::Corrupt {
            filter: Filter::Flate,
            at: 0
        })
    );
}

#[test]
fn huge_columns_never_size_an_allocation() {
    // Rows far longer than the data: one short row, decoded as far as it
    // goes, with nothing reserved by the row length.
    let data = noise(13, 320, 26); // whole 64-byte pixels at 32 × 16 bits
    let max = (usize::MAX / 512) as i64;
    for (colors, bpc, columns) in [(1i64, 8i64, 1i64 << 55), (32, 16, max), (1, 8, max)] {
        let parms = |predictor: i64| {
            dict(vec![
                ("Predictor", Object::Integer(predictor)),
                ("Colors", Object::Integer(colors)),
                ("BitsPerComponent", Object::Integer(bpc)),
                ("Columns", Object::Integer(columns)),
            ])
        };
        let mut png = vec![0u8];
        png.extend_from_slice(&data);
        assert_eq!(
            decode_chain(
                &zlib(&png),
                &[(Filter::Flate, Some(parms(12)))],
                DEFAULT_CAP
            ),
            Ok(data.clone()),
            "PNG colors {colors} bpc {bpc}"
        );
        let (c, b) = (colors as usize, bpc as usize);
        let cols = data.len() * 8 / (c * b);
        let tiff = tiff_predict(&data, data.len(), c, b, cols);
        assert_eq!(
            decode_chain(
                &lzw(&tiff, 1),
                &[(Filter::Lzw, Some(parms(2)))],
                DEFAULT_CAP
            ),
            Ok(data.clone()),
            "TIFF colors {colors} bpc {bpc}"
        );
    }
}

#[test]
fn a_predictor_of_one_or_less_is_none() {
    let data = noise(14, 50, 26);
    for p in [1, 0, -3] {
        let parms = dict(vec![
            ("Predictor", Object::Integer(p)),
            ("Columns", Object::Integer(7)),
        ]);
        assert_eq!(
            decode_chain(&zlib(&data), &[(Filter::Flate, Some(parms))], DEFAULT_CAP),
            Ok(data.clone()),
            "Predictor {p}"
        );
    }
}

#[test]
fn every_stage_stops_at_the_cap() {
    let data = vec![b'x'; 1 << 20];
    let z = zlib(&data);
    assert_eq!(
        decode_chain(&z, &[Filter::Flate], 64 << 10),
        Err(DecodeError::CapHit)
    );
    assert_eq!(
        decode_chain(&z, &[Filter::Flate], 1 << 20),
        Ok(data.clone())
    );
    assert_eq!(
        decode_chain(&lzw(&data, 1), &[Filter::Lzw], 64 << 10),
        Err(DecodeError::CapHit)
    );
    // 128 repeat runs of 128 bytes each.
    let rl: Vec<u8> = (0..128).flat_map(|_| [0x81u8, b'x']).collect();
    assert_eq!(
        decode_chain(&rl, &[Filter::RunLength], 1000),
        Err(DecodeError::CapHit)
    );
}

#[test]
fn flate_errors_carry_the_miniz_status() {
    let mut z = zlib(&noise(3, 5_000, 9));
    let last = z.len() - 1;
    z[last] ^= 1;
    assert_eq!(
        decode_chain(&z, &[Filter::Flate], DEFAULT_CAP),
        Err(DecodeError::Inflate(InflateStatus::AdlerMismatch))
    );
    assert_eq!(
        decode_chain(&z[..z.len() / 2], &[Filter::Flate], DEFAULT_CAP),
        Err(DecodeError::Inflate(InflateStatus::NeedsMoreInput))
    );
}

#[test]
fn image_codecs_are_left_encoded_and_must_be_last() {
    assert_eq!(
        decode_chain(TINY_JPEG, &[Filter::Dct], DEFAULT_CAP),
        Ok(TINY_JPEG.to_vec())
    );
    assert_eq!(
        decode_chain(
            &ascii_hex(TINY_JPEG),
            &[Filter::AsciiHex, Filter::Dct],
            DEFAULT_CAP
        ),
        Ok(TINY_JPEG.to_vec())
    );
    assert_eq!(
        decode_chain(TINY_JPEG, &[Filter::Dct, Filter::Flate], DEFAULT_CAP),
        Err(DecodeError::Unsupported(Filter::Dct))
    );
    let crypt = Filter::Unknown("Crypt".into());
    assert_eq!(
        decode_chain(b"x", std::slice::from_ref(&crypt), DEFAULT_CAP),
        Err(DecodeError::Unsupported(crypt))
    );
    assert_eq!(
        decode_chain::<Filter>(b"as is", &[], DEFAULT_CAP),
        Ok(b"as is".to_vec())
    );
}

// ---- classify ----

const OUTLINES_ONLY: &[u8] = b"0.2 0.3 0.4 rg\n10 10 m\n20 10 l\n20 20 15 25 10 20 c\nh f\n\
30 30 50 50 re S\n0 0 1 RG 2 w 1 0 0 1 5 5 cm 40 40 m 60 60 l S\n";

fn sniff(bytes: &[u8]) -> StreamClass {
    classify(&Dictionary::new(), bytes)
}

#[test]
fn outline_only_content_is_content() {
    assert_eq!(sniff(OUTLINES_ONLY), StreamClass::Content);
    assert_eq!(
        sniff(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET"),
        StreamClass::Content
    );
}

#[test]
fn xobject_only_content_is_a_form() {
    assert_eq!(sniff(b"q 100 0 0 100 0 0 cm /Im0 Do Q"), StreamClass::Form);
}

#[test]
fn content_needs_ninety_percent_known_operators_with_fitting_arities() {
    // 9 known of 10 passes; 8 of 10 does not.
    let nine = b"0 0 m 1 1 l 2 2 l 3 3 l 4 4 l 5 5 l 6 6 l 7 7 l S zork";
    assert_eq!(sniff(nine), StreamClass::Content);
    let eight = b"0 0 m 1 1 l 2 2 l 3 3 l 4 4 l 5 5 l 6 6 l S zork 1 2 3 h";
    assert_eq!(sniff(eight), StreamClass::Other);
    // `l` with three operands does not fit its arity.
    let arity = b"0 0 m 1 1 1 l 2 2 2 l 3 3 l S";
    assert_eq!(sniff(arity), StreamClass::Other);
}

#[test]
fn image_magic_is_an_image() {
    assert_eq!(
        sniff(TINY_JPEG),
        StreamClass::Image {
            codec: ImgCodec::Jpeg
        }
    );
    assert_eq!(
        sniff(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
        StreamClass::Image {
            codec: ImgCodec::Png
        }
    );
    assert_eq!(
        sniff(b"\0\0\0\x0cjP  \r\n\x87\n"),
        StreamClass::Image {
            codec: ImgCodec::Jp2
        }
    );
}

#[test]
fn image_codec_comes_from_the_filter() {
    let d = dict(vec![
        ("Type", name(b"XObject")),
        ("Subtype", name(b"Image")),
        ("Filter", name(b"DCTDecode")),
    ]);
    assert_eq!(
        classify(&d, TINY_JPEG),
        StreamClass::Image {
            codec: ImgCodec::Jpeg
        }
    );
    let d = dict(vec![
        ("Subtype", name(b"Image")),
        ("Filter", name(b"FlateDecode")),
    ]);
    assert_eq!(
        classify(&d, &[0x7f; 64]),
        StreamClass::Image {
            codec: ImgCodec::Raw
        }
    );
    let d = dict(vec![
        ("Subtype", name(b"Image")),
        ("Filter", name(b"JPXDecode")),
    ]);
    assert_eq!(
        classify(&d, b"\xff\x4f\xff\x51"),
        StreamClass::Image {
            codec: ImgCodec::Jp2
        }
    );
    let d = dict(vec![
        ("Subtype", name(b"Image")),
        ("Filter", name(b"CCITTFaxDecode")),
    ]);
    assert_eq!(
        classify(&d, b"\0\x01"),
        StreamClass::Image {
            codec: ImgCodec::Ccitt
        }
    );
    let d = dict(vec![
        ("Subtype", name(b"Image")),
        ("Filter", name(b"JBIG2Decode")),
    ]);
    assert_eq!(
        classify(&d, b"\0\x01"),
        StreamClass::Image {
            codec: ImgCodec::Jbig2
        }
    );
    // No /Subtype (a damaged dictionary): the filter still names the codec.
    let d = dict(vec![("Filter", name(b"DCTDecode"))]);
    assert_eq!(
        classify(&d, TINY_JPEG),
        StreamClass::Image {
            codec: ImgCodec::Jpeg
        }
    );
}

#[test]
fn font_magic_is_a_font_file() {
    for magic in [&b"\x00\x01\x00\x00"[..], b"OTTO", b"true", b"ttcf"] {
        let mut bytes = magic.to_vec();
        bytes.extend_from_slice(&[0, 9, 0, 0x80, 0, 3, 0, 0x10]);
        assert_eq!(sniff(&bytes), StreamClass::FontFile, "{magic:?}");
    }
    assert_eq!(sniff(TEST_FONT), StreamClass::FontFile);
    assert_eq!(
        sniff(b"%!PS-AdobeFont-1.0: Foo 001.000\n"),
        StreamClass::FontFile
    );
    let d = dict(vec![("Length1", Object::Integer(9672))]);
    assert_eq!(classify(&d, b"\0\0"), StreamClass::FontFile);
    let d = dict(vec![("Subtype", name(b"Type1C"))]);
    assert_eq!(classify(&d, b"\x01\0\x04\x02"), StreamClass::FontFile);
}

#[test]
fn cmaps_are_cmaps() {
    let tounicode = b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfchar\n<0003> <0020>\n\
endbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
    assert_eq!(sniff(tounicode), StreamClass::CMap);
    assert_eq!(
        sniff(b"%!PS-Adobe-3.0 Resource-CMap\n%%DocumentNeededResources: ProcSet (CIDInit)\n"),
        StreamClass::CMap
    );
    let d = dict(vec![("Type", name(b"CMap")), ("CMapName", name(b"X"))]);
    assert_eq!(classify(&d, b"garbage"), StreamClass::CMap);
}

#[test]
fn xmp_is_metadata() {
    let xmp = b"<?xpacket begin=\"\xef\xbb\xbf\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>\n<?xpacket end=\"w\"?>";
    assert_eq!(sniff(xmp), StreamClass::Metadata);
    assert_eq!(
        sniff(b"\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">"),
        StreamClass::Metadata
    );
    let d = dict(vec![("Type", name(b"Metadata")), ("Subtype", name(b"XML"))]);
    assert_eq!(classify(&d, b""), StreamClass::Metadata);
}

#[test]
fn object_and_xref_streams_come_from_the_dictionary() {
    let d = dict(vec![
        ("Type", name(b"ObjStm")),
        ("N", Object::Integer(2)),
        ("First", Object::Integer(9)),
    ]);
    assert_eq!(classify(&d, b"1 0 2 5 <<>> <<>>"), StreamClass::ObjStm);
    let d = dict(vec![("Type", name(b"XRef")), ("Size", Object::Integer(3))]);
    assert_eq!(classify(&d, &[1, 0, 0, 1, 0, 9]), StreamClass::XRef);
    let d = dict(vec![("Type", name(b"XObject")), ("Subtype", name(b"Form"))]);
    assert_eq!(classify(&d, OUTLINES_ONLY), StreamClass::Form);
}

#[test]
fn an_unknown_type_falls_back_to_the_sniff() {
    let d = dict(vec![("Type", name(b"Wombat"))]);
    assert_eq!(classify(&d, OUTLINES_ONLY), StreamClass::Content);
}

#[test]
fn noise_is_other() {
    assert_eq!(sniff(b""), StreamClass::Other);
    assert_eq!(sniff(b"   \n"), StreamClass::Other);
    let mut rng = Rng(99);
    let bin: Vec<u8> = (0..4096).map(|_| rng.next() as u8).collect();
    assert_eq!(sniff(&bin), StreamClass::Other);
    assert_eq!(sniff(&noise(5, 4096, 26)), StreamClass::Other);
}

// ---- content_ops ----

/// Page `n`'s content stream from the golden, decoded through our own chain.
fn golden_content(page: u32) -> Vec<u8> {
    let doc = Document::load_mem(&golden_pdf()).expect("golden loads");
    let page_id = doc.get_pages()[&page];
    let contents = doc
        .get_dictionary(page_id)
        .and_then(|p| p.get(b"Contents"))
        .and_then(Object::as_reference)
        .expect("one content stream");
    let stream = doc
        .get_object(contents)
        .and_then(Object::as_stream)
        .expect("stream");
    decode_chain(&stream.content, &filters_of(&stream.dict), DEFAULT_CAP).expect("decodes")
}

fn names<'a>(ops: &[Op<'a>]) -> Vec<&'a str> {
    ops.iter()
        .map(|o| std::str::from_utf8(o.op).unwrap())
        .collect()
}

#[test]
fn golden_content_walks_to_the_known_operator_sequence() {
    for page in [1u32, 2] {
        let bytes = golden_content(page);
        let ops: Vec<Op<'_>> = content_ops(&bytes).collect();
        let mut expected = vec!["BT", "Tf", "Td", "Tj", "Td", "Tj", "ET"];
        if page == 1 {
            expected.extend(["q", "cm", "Do", "Q"]);
        }
        assert_eq!(names(&ops), expected, "page {page}");
        assert_eq!(ops[1].operands, vec![name(b"F1"), Object::Integer(16)]);
        let tj = &ops[3].operands;
        assert_eq!(tj.len(), 1);
        let Object::String(glyphs, StringFormat::Hexadecimal) = &tj[0] else {
            panic!("Tj operand {tj:?}");
        };
        assert_eq!(
            glyphs.len(),
            2 * GOLDEN_TEXT[page as usize - 1][0].chars().count()
        );
        // Every operator and operand agrees with lopdf's strict decoder.
        let strict = Content::decode(&bytes).expect("lopdf decodes the golden");
        assert_eq!(strict.operations.len(), ops.len());
        for (ours, theirs) in ops.iter().zip(&strict.operations) {
            assert_eq!(ours.op, theirs.operator.as_bytes());
            assert_eq!(ours.operands, theirs.operands);
            assert!(bytes[ours.at..].starts_with(ours.op));
        }
    }
}

#[test]
fn text_operators_keep_their_operands() {
    let src = b"BT /F2 9.5 Tf [(Hel) -120 (lo) 50.5 <0041>] TJ (x) ' 1 2 (y) \" ET";
    let ops: Vec<Op<'_>> = content_ops(src).collect();
    assert_eq!(names(&ops), ["BT", "Tf", "TJ", "'", "\"", "ET"]);
    assert_eq!(ops[1].operands, vec![name(b"F2"), Object::Real(9.5)]);
    assert_eq!(
        ops[2].operands,
        vec![Object::Array(vec![
            lit(b"Hel"),
            Object::Integer(-120),
            lit(b"lo"),
            Object::Real(50.5),
            hexs(b"\x00\x41"),
        ])]
    );
    assert_eq!(ops[3].operands, vec![lit(b"x")]);
    assert_eq!(
        ops[4].operands,
        vec![Object::Integer(1), Object::Integer(2), lit(b"y")]
    );
    assert_eq!(ops[2].at, src.windows(2).position(|w| w == b"TJ").unwrap());
}

#[test]
fn marked_content_dictionaries_are_operands() {
    let ops: Vec<Op<'_>> = content_ops(b"/Span <</MCID 3 /Alt (a)>> BDC EMC").collect();
    assert_eq!(names(&ops), ["BDC", "EMC"]);
    assert_eq!(
        ops[0].operands,
        vec![
            name(b"Span"),
            Object::Dictionary(dict(vec![("MCID", Object::Integer(3)), ("Alt", lit(b"a"))])),
        ]
    );
}

#[test]
fn an_inline_image_is_one_bi_op() {
    // Unfiltered 4×2 gray: the data contains ` EI ` but its length is known.
    let mut src = b"q BI /W 4 /H 2 /BPC 8 /CS /G ID ".to_vec();
    src.extend_from_slice(b" EI \xff\x00\x01\x02");
    src.extend_from_slice(b"\nEI Q 1 0 0 1 0 0 cm");
    let ops: Vec<Op<'_>> = content_ops(&src).collect();
    assert_eq!(names(&ops), ["q", "BI", "Q", "cm"]);
    assert_eq!(ops[1].at, 2);
    let Object::Dictionary(d) = &ops[1].operands[0] else {
        panic!("BI operand {:?}", ops[1].operands);
    };
    assert_eq!(d.get(b"W").unwrap(), &Object::Integer(4));
    assert_eq!(d.get(b"CS").unwrap(), &name(b"G"));

    // Filtered: the data runs to the first `EI` with white space both sides.
    let src = b"BI /W 2 /H 2 /BPC 8 /CS /G /F /AHx ID 00ffEI00 ff>\nEI\nS";
    let ops: Vec<Op<'_>> = content_ops(src).collect();
    assert_eq!(names(&ops), ["BI", "S"]);

    // Binary data that lexes as anything at all is skipped.
    let mut src = b"BI /W 3 /H 1 /BPC 8 /CS /RGB /F /Fl ID ".to_vec();
    src.extend_from_slice(&zlib(b"(((]]>> BT /x Tj [<"));
    src.extend_from_slice(b" EI Q");
    let ops: Vec<Op<'_>> = content_ops(&src).collect();
    assert_eq!(names(&ops), ["BI", "Q"]);

    // `/L` gives the length of filtered data, which here holds ` EI `.
    for key in ["L", "Length"] {
        let src = format!("BI /W 2 /H 1 /BPC 8 /CS /G /F /Fl /{key} 6 ID a EI b EI Q");
        let ops: Vec<Op<'_>> = content_ops(src.as_bytes()).collect();
        assert_eq!(names(&ops), ["BI", "Q"], "/{key}");
    }
    // An image mask is 1 bit per pixel with no /CS or /BPC: 32 × 1 is 4
    // bytes, here ` EI `.
    for key in ["IM", "ImageMask"] {
        let src = format!("BI /W 32 /H 1 /{key} true ID  EI \nEI Q");
        let ops: Vec<Op<'_>> = content_ops(src.as_bytes()).collect();
        assert_eq!(names(&ops), ["BI", "Q"], "/{key}");
    }

    // No `EI`: the image runs to the end.
    let ops: Vec<Op<'_>> = content_ops(b"q BI /W 9 ID abc Q").collect();
    assert_eq!(names(&ops), ["q", "BI"]);
}

#[test]
fn the_walk_is_tolerant() {
    // An unknown operator is yielded as-is.
    let ops: Vec<Op<'_>> = content_ops(b"1 2 zork 3 0 Td").collect();
    assert_eq!(names(&ops), ["zork", "Td"]);
    assert_eq!(
        ops[0].operands,
        vec![Object::Integer(1), Object::Integer(2)]
    );
    // A stray closer ends the operand list, not the walk.
    let ops: Vec<Op<'_>> = content_ops(b"1 2 ] 3 4 m 5 >> 6 7 l").collect();
    assert_eq!(
        ops[0].operands,
        vec![Object::Integer(3), Object::Integer(4)]
    );
    assert_eq!(
        ops[1].operands,
        vec![Object::Integer(6), Object::Integer(7)]
    );
    // An unterminated array ends at the next operator, which is still read.
    let ops: Vec<Op<'_>> = content_ops(b"[(a) 3 Tj (b) Tj").collect();
    assert_eq!(names(&ops), ["Tj", "Tj"]);
    assert_eq!(ops[0].operands, vec![]);
    assert_eq!(ops[1].operands, vec![lit(b"b")]);
    // Operands with no operator before EOF are dropped.
    let ops: Vec<Op<'_>> = content_ops(b"q 1 2 3").collect();
    assert_eq!(names(&ops), ["q"]);
    // Comments are skipped; an unterminated string runs to EOF.
    let ops: Vec<Op<'_>> = content_ops(b"% c\nq (abc Tj Q").collect();
    assert_eq!(names(&ops), ["q"]);
}

#[test]
fn ten_thousand_fuzzed_slices_terminate() {
    let mut tj = b"BT /F2 9.5 Tf [(Hel) -120 (lo) 50.5 <0041>] TJ (x) ' ET\n".to_vec();
    tj.extend_from_slice(b"q BI /W 4 /H 2 /BPC 8 /CS /G ID  EI \xff\x00\x01\nEI Q ");
    tj.extend_from_slice(b"/Span <</MCID 3>> BDC EMC [[[[<<<<(( ))]]]]>>");
    let mut sources = vec![
        golden_content(1),
        golden_content(2),
        tj,
        OUTLINES_ONLY.to_vec(),
    ];
    let mut rng = Rng(0x5eed);
    sources.push((0..2048).map(|_| rng.next() as u8).collect());
    const SPICE: &[u8] = b"()<>[]{}/%\\BIDE \n\r\t0123456789.-+#";
    for _ in 0..10_000 {
        let src = &sources[rng.below(sources.len())];
        let start = rng.below(src.len());
        let end = start + rng.below(src.len() - start + 1);
        let mut slice = src[start..end].to_vec();
        for _ in 0..rng.below(5) {
            if slice.is_empty() {
                break;
            }
            let at = rng.below(slice.len());
            slice[at] = if rng.next().is_multiple_of(2) {
                SPICE[rng.below(SPICE.len())]
            } else {
                rng.next() as u8
            };
        }
        let mut n = 0usize;
        for op in content_ops(&slice) {
            assert!(op.at < slice.len() && slice[op.at..].starts_with(op.op));
            n += 1;
            assert!(n <= slice.len(), "more operators than bytes");
        }
        let _ = classify(&Dictionary::new(), &slice);
        for f in [
            Filter::Ascii85,
            Filter::AsciiHex,
            Filter::Lzw,
            Filter::RunLength,
            Filter::Flate,
        ] {
            let _ = decode_chain(&slice, &[f], 1 << 16);
        }
        // Valid Flate and LZW bodies under random predictor parameters,
        // including huge and out-of-range ones.
        let pick = |rng: &mut Rng, xs: &[i64]| xs[rng.below(xs.len())];
        let parms = dict(vec![
            (
                "Predictor",
                Object::Integer(pick(&mut rng, &[-1, 0, 1, 2, 3, 10, 12, 15, 16])),
            ),
            (
                "Colors",
                Object::Integer(pick(&mut rng, &[0, 1, 3, 4, 32, 33, i64::MAX])),
            ),
            (
                "BitsPerComponent",
                Object::Integer(pick(&mut rng, &[1, 2, 3, 4, 8, 16, 32])),
            ),
            (
                "Columns",
                Object::Integer(match rng.below(4) {
                    0 => rng.below(64) as i64,
                    1 => (rng.next() >> 1) as i64,
                    2 => 1i64 << rng.below(63),
                    _ => i64::MAX,
                }),
            ),
        ]);
        let _ = decode_chain(
            &zlib(&slice),
            &[(Filter::Flate, Some(parms.clone()))],
            1 << 16,
        );
        let _ = decode_chain(&lzw(&slice, 1), &[(Filter::Lzw, Some(parms))], 1 << 16);
    }
}
