//! Filter chains, inflate, stream classification and the content-operator
//! walk (T-06, TD §15, §16 step 1); C9 salvage (T-08, T-08b).
// T-07, T-09 and T-11b are the first callers outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod inflate;
pub(crate) mod salvage;

use lopdf::{Dictionary, Object, StringFormat};

use crate::pdf::lexer::{Lexer, MAX_DEPTH, MAX_ELEMENTS, Tok};

pub(crate) use inflate::{InflateStatus, RAW_END_SLACK, inflate};

/// The inflate ceiling `decode_chain` callers pass by default: 256 MiB.
pub(crate) const DEFAULT_CAP: usize = 256 << 20;

/// One `/Filter` entry. Abbreviated inline-image names map to the same
/// variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Filter {
    Flate,
    Ascii85,
    AsciiHex,
    Lzw,
    RunLength,
    Dct,
    Jpx,
    Ccitt,
    Jbig2,
    /// Any other name, as written (lossy UTF-8); a non-name entry is its
    /// object type in angle brackets, such as `<Reference>`.
    Unknown(String),
}

/// The image codec a stream's data is in, for image extraction (TD §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImgCodec {
    Jpeg,
    Jp2,
    Png,
    Ccitt,
    Jbig2,
    /// Decoded samples (Flate, LZW or no filter).
    Raw,
}

/// What a stream holds (TD §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamClass {
    Content,
    Image { codec: ImgCodec },
    Form,
    FontFile,
    CMap,
    Metadata,
    ObjStm,
    XRef,
    Other,
}

/// Why `decode_chain` produced nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DecodeError {
    /// A stage's output reached the cap.
    #[error("decoded output reached the cap")]
    CapHit,
    /// The Flate stage did not end `Done` (miniz_oxide's own status).
    #[error("inflate ended {0:?}")]
    Inflate(InflateStatus),
    /// A stage met bad data at byte `at` of its own input.
    #[error("{filter:?} data is corrupt at byte {at}")]
    Corrupt { filter: Filter, at: usize },
    /// `/DecodeParms` this decoder cannot apply (an unknown predictor, a
    /// bit depth or colour count outside ISO 32000's ranges, or an entry
    /// [`filters_of`] could not read: [`UNRESOLVED_PARMS`]).
    #[error("{0:?} has decode parameters it cannot apply")]
    BadParms(Filter),
    /// A filter we do not decode, or an image codec that is not last.
    #[error("{0:?} is not decoded here")]
    Unsupported(Filter),
}

/// One stage of a chain: a filter and its `/DecodeParms`. `decode_chain`
/// takes either bare filters (the `&[Filter]` of the T-06 interface) or what
/// [`filters_of`] returns, since predictors need the parameters.
pub(crate) trait Stage {
    fn filter(&self) -> &Filter;
    fn parms(&self) -> Option<&Dictionary>;
}

impl Stage for Filter {
    fn filter(&self) -> &Filter {
        self
    }
    fn parms(&self) -> Option<&Dictionary> {
        None
    }
}

impl Stage for (Filter, Option<Dictionary>) {
    fn filter(&self) -> &Filter {
        &self.0
    }
    fn parms(&self) -> Option<&Dictionary> {
        self.1.as_ref()
    }
}

impl Filter {
    /// The filter a `/Filter` name stands for, full or abbreviated.
    pub(crate) fn from_name(name: &[u8]) -> Filter {
        match name {
            b"FlateDecode" | b"Fl" => Filter::Flate,
            b"ASCII85Decode" | b"A85" => Filter::Ascii85,
            b"ASCIIHexDecode" | b"AHx" => Filter::AsciiHex,
            b"LZWDecode" | b"LZW" => Filter::Lzw,
            b"RunLengthDecode" | b"RL" => Filter::RunLength,
            b"DCTDecode" | b"DCT" => Filter::Dct,
            b"JPXDecode" => Filter::Jpx,
            b"CCITTFaxDecode" | b"CCF" => Filter::Ccitt,
            b"JBIG2Decode" => Filter::Jbig2,
            other => Filter::Unknown(String::from_utf8_lossy(other).into_owned()),
        }
    }

    /// The image codec this filter leaves its data in, if it is one.
    fn codec(&self) -> Option<ImgCodec> {
        match self {
            Filter::Dct => Some(ImgCodec::Jpeg),
            Filter::Jpx => Some(ImgCodec::Jp2),
            Filter::Ccitt => Some(ImgCodec::Ccitt),
            Filter::Jbig2 => Some(ImgCodec::Jbig2),
            _ => None,
        }
    }

    /// Filters that read `/DecodeParms`.
    fn takes_parms(&self) -> bool {
        matches!(
            self,
            Filter::Flate | Filter::Lzw | Filter::Dct | Filter::Ccitt | Filter::Jbig2
        )
    }
}

/// The key of the stand-in dictionary [`filters_of`] gives a stage whose
/// `/DecodeParms` entry is neither a dictionary nor null: an indirect
/// reference it cannot resolve, or damage. The value is the entry as written.
/// [`decode_chain`] refuses a Flate or LZW stage carrying it with
/// `BadParms`, so predicted bytes are never passed off as decoded ones.
pub(crate) const UNRESOLVED_PARMS: &[u8] = b"PDFPunditUnresolvedParms";

/// The stream's filter chain, in decoding order, each with its
/// `/DecodeParms` entry. Reads `/Filter` and `/DecodeParms`, or an inline
/// image's `/F` and `/DP` when `/Filter` is absent (a stream's `/F` is a file
/// specification, never a name or array of names). A null `/Filter`, or a
/// null element of a `/Filter` array, counts as absent (ISO 32000-1 7.3.9).
/// A parameters dictionary given with several filters goes to the first one
/// that reads parameters. A `/Filter` this cannot read (an indirect
/// reference) gives one `Unknown` stage, so the stream is never mistaken for
/// unfiltered; a `/DecodeParms` entry it cannot read gives the
/// [`UNRESOLVED_PARMS`] stand-in. Callers holding the object table resolve
/// both before calling this.
pub(crate) fn filters_of(dict: &Dictionary) -> Vec<(Filter, Option<Dictionary>)> {
    let (filter, parms) = match dict.get(b"Filter") {
        Ok(Object::Null) | Err(_) => match dict.get(b"F") {
            Ok(f @ (Object::Name(_) | Object::Array(_))) => (f, dict.get(b"DP").ok()),
            _ => return Vec::new(),
        },
        Ok(f) => (f, dict.get(b"DecodeParms").ok()),
    };
    let one = |o: &Object| match o {
        Object::Name(n) => Filter::from_name(n),
        other => Filter::Unknown(format!("<{}>", other.enum_variant())),
    };
    let parm = |o: &Object| match o {
        Object::Null => None,
        Object::Dictionary(d) => Some(d.clone()),
        other => {
            let mut d = Dictionary::new();
            d.set(UNRESOLVED_PARMS, other.clone());
            Some(d)
        }
    };
    let items = match filter {
        Object::Array(items) => items.as_slice(),
        other => std::slice::from_ref(other),
    };
    let per_item: Vec<Option<Dictionary>> = match parms {
        Some(Object::Array(ps)) => (0..items.len()).map(|i| ps.get(i).and_then(parm)).collect(),
        _ => vec![None; items.len()],
    };
    let mut stages: Vec<(Filter, Option<Dictionary>)> = items
        .iter()
        .zip(per_item)
        .filter(|(o, _)| !matches!(o, Object::Null))
        .map(|(o, p)| (one(o), p))
        .collect();
    // One entry that is not an array: a dictionary, or one we cannot read.
    if let Some(p) = parms.filter(|p| !matches!(p, Object::Array(_) | Object::Null)) {
        let target = if stages.len() == 1 {
            Some(0)
        } else {
            stages.iter().position(|(f, _)| f.takes_parms())
        };
        if let Some(i) = target {
            stages[i].1 = parm(p);
        }
    }
    stages
}

/// Decodes `raw` through `chain`, left to right. Flate goes through
/// [`inflate`] and must end `Done` (an Adler-32 mismatch is an error here;
/// the C9 salvage decides what to keep). Flate and LZW undo their PNG or
/// TIFF predictor. An image codec (DCT, JPX, CCITT, JBIG2) is not decoded:
/// it must be the last stage, and its data is returned still encoded. Every
/// stage's output is capped at `cap` bytes ([`DEFAULT_CAP`] for callers
/// without their own ceiling).
pub(crate) fn decode_chain<S: Stage>(
    raw: &[u8],
    chain: &[S],
    cap: usize,
) -> Result<Vec<u8>, DecodeError> {
    let mut data = raw.to_vec();
    for (i, stage) in chain.iter().enumerate() {
        let filter = stage.filter();
        data = match filter {
            Filter::Flate => {
                let parms = parms_of(stage)?;
                let r = inflate(&data, cap);
                match r.status {
                    InflateStatus::Done => unpredict(r.out, parms, filter)?,
                    InflateStatus::CapHit => return Err(DecodeError::CapHit),
                    status => return Err(DecodeError::Inflate(status)),
                }
            }
            Filter::Lzw => {
                let parms = parms_of(stage)?;
                let early = int_parm(parms, b"EarlyChange", 1) != 0;
                unpredict(lzw(&data, early, cap)?, parms, filter)?
            }
            Filter::Ascii85 => ascii85(&data, cap)?,
            Filter::AsciiHex => ascii_hex(&data, cap)?,
            Filter::RunLength => run_length(&data, cap)?,
            Filter::Dct | Filter::Jpx | Filter::Ccitt | Filter::Jbig2 if i + 1 == chain.len() => {
                return Ok(data);
            }
            other => return Err(DecodeError::Unsupported(other.clone())),
        };
    }
    Ok(data)
}

/// The Flate stage's `/Predictor` step on its own: what [`decode_chain`] does
/// to a Flate stage's inflated bytes. The C9 salvage (T-08) keeps its
/// partial and repaired outputs in this domain.
pub(crate) fn unpredict_flate(
    data: Vec<u8>,
    parms: Option<&Dictionary>,
) -> Result<Vec<u8>, DecodeError> {
    if parms.is_some_and(|d| d.has(UNRESOLVED_PARMS)) {
        return Err(DecodeError::BadParms(Filter::Flate));
    }
    unpredict(data, parms, &Filter::Flate)
}

/// [`unpredict_flate`] for unverified salvage output, which may hold garbage
/// rows: PNG rows are undone up to the first row whose filter type is above
/// 4, and that row and everything after it are dropped. Returns the bytes
/// undone and, when it stopped early, the offset in `data` of the row it
/// stopped at. Parameters it cannot apply are still an error.
pub(crate) fn unpredict_flate_lenient(
    data: Vec<u8>,
    parms: Option<&Dictionary>,
) -> Result<(Vec<u8>, Option<usize>), DecodeError> {
    if parms.is_some_and(|d| d.has(UNRESOLVED_PARMS)) {
        return Err(DecodeError::BadParms(Filter::Flate));
    }
    unpredict_rows(data, parms, &Filter::Flate)
}

/// The stage's parameters, refusing the [`UNRESOLVED_PARMS`] stand-in.
fn parms_of<S: Stage>(stage: &S) -> Result<Option<&Dictionary>, DecodeError> {
    match stage.parms() {
        Some(d) if d.has(UNRESOLVED_PARMS) => Err(DecodeError::BadParms(stage.filter().clone())),
        p => Ok(p),
    }
}

fn int_parm(parms: Option<&Dictionary>, key: &[u8], default: i64) -> i64 {
    parms
        .and_then(|d| d.get(key).ok())
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(default)
}

fn corrupt(filter: Filter, at: usize) -> DecodeError {
    DecodeError::Corrupt { filter, at }
}

fn is_ws(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// ISO 32000-1 7.4.3. White space is skipped; `~` ends the data (with or
/// without its `>`), as does the end of input; `<~` before the data is
/// skipped. A final group of n ≥ 2 digits gives n − 1 bytes; a lone final
/// digit is ignored.
fn ascii85(data: &[u8], cap: usize) -> Result<Vec<u8>, DecodeError> {
    let body = data
        .iter()
        .position(|&b| !is_ws(b))
        .filter(|&i| data[i..].starts_with(b"<~"))
        .map_or(data, |i| &data[i + 2..]);
    let skipped = data.len() - body.len();
    let mut out = Vec::with_capacity(body.len() / 5 * 4 + 4);
    let mut group = [0u8; 5];
    let mut n = 0usize;
    let mut group_at = 0usize;
    let push = |out: &mut Vec<u8>, group: &[u8; 5], n: usize, at: usize| {
        let v = group.iter().fold(0u64, |acc, &d| acc * 85 + u64::from(d));
        let v = u32::try_from(v).map_err(|_| corrupt(Filter::Ascii85, at))?;
        out.extend_from_slice(&v.to_be_bytes()[..n - 1]);
        if out.len() > cap {
            return Err(DecodeError::CapHit);
        }
        Ok(())
    };
    for (i, &b) in body.iter().enumerate() {
        let at = skipped + i;
        match b {
            b'~' => break,
            b if is_ws(b) => {}
            b'z' if n == 0 => {
                out.extend_from_slice(&[0; 4]);
                if out.len() > cap {
                    return Err(DecodeError::CapHit);
                }
            }
            b'!'..=b'u' => {
                if n == 0 {
                    group_at = at;
                }
                group[n] = b - b'!';
                n += 1;
                if n == 5 {
                    push(&mut out, &group, 5, group_at)?;
                    n = 0;
                }
            }
            _ => return Err(corrupt(Filter::Ascii85, at)),
        }
    }
    if n >= 2 {
        group[n..].fill(84);
        push(&mut out, &group, n, group_at)?;
    }
    Ok(out)
}

/// ISO 32000-1 7.4.2. White space is skipped; `>` or the end of input ends
/// the data; an odd final digit is followed by an implied 0.
fn ascii_hex(data: &[u8], cap: usize) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for (at, &b) in data.iter().enumerate() {
        let v = match b {
            b'>' => break,
            b if is_ws(b) => continue,
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(corrupt(Filter::AsciiHex, at)),
        };
        match high.take() {
            Some(h) => out.push(h << 4 | v),
            None => high = Some(v),
        }
    }
    if let Some(h) = high {
        out.push(h << 4);
    }
    if out.len() > cap {
        return Err(DecodeError::CapHit);
    }
    Ok(out)
}

/// ISO 32000-1 7.4.5. A run cut short by the end of input keeps what is
/// there.
fn run_length(data: &[u8], cap: usize) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(&len) = data.get(i) {
        match len {
            128 => break,
            0..=127 => {
                let end = (i + 2 + usize::from(len)).min(data.len());
                out.extend_from_slice(&data[i + 1..end]);
                i = end;
            }
            _ => {
                let Some(&b) = data.get(i + 1) else { break };
                out.resize(out.len() + 257 - usize::from(len), b);
                i += 2;
            }
        }
        if out.len() > cap {
            return Err(DecodeError::CapHit);
        }
    }
    Ok(out)
}

/// ISO 32000-1 7.4.4: 9- to 12-bit codes, most significant bit first; 256
/// clears the table, 257 ends the data. With `early` the code width grows
/// one code sooner. A full table stops growing until the next clear. The end
/// of input ends the data; a code beyond the table is corrupt.
fn lzw(data: &[u8], early: bool, cap: usize) -> Result<Vec<u8>, DecodeError> {
    const CLEAR: usize = 256;
    const EOD: usize = 257;
    const MAX: usize = 4096;
    // Entry `c` (≥ 258) is `out[start..start + len]`: a string is always the
    // previous string followed by the next one's first byte, and those bytes
    // sit next to each other in the output.
    let mut table: Vec<(usize, usize)> = Vec::with_capacity(MAX);
    let mut out = Vec::new();
    let mut width = 9u32;
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut prev: Option<(usize, usize)> = None;
    let early = usize::from(early);
    for (at, &b) in data.iter().enumerate() {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        if bits < width {
            continue;
        }
        bits -= width;
        let code = ((acc >> bits) & ((1 << width) - 1)) as usize;
        acc &= (1 << bits) - 1;
        if code == CLEAR {
            table.clear();
            width = 9;
            prev = None;
            continue;
        }
        if code == EOD {
            break;
        }
        let next = 258 + table.len();
        let start = out.len();
        let len = if code < 256 {
            out.push(code as u8);
            1
        } else if code < next {
            let (s, l) = table[code - 258];
            out.extend_from_within(s..s + l);
            l
        } else if code == next
            && let Some((s, l)) = prev
        {
            out.extend_from_within(s..s + l);
            out.push(out[s]);
            l + 1
        } else {
            return Err(corrupt(Filter::Lzw, at));
        };
        if let Some((s, l)) = prev
            && next < MAX
        {
            table.push((s, l + 1));
            width = match next + 1 + early {
                0..512 => 9,
                512..1024 => 10,
                1024..2048 => 11,
                _ => 12,
            };
        }
        prev = Some((start, len));
        if out.len() > cap {
            return Err(DecodeError::CapHit);
        }
    }
    Ok(out)
}

/// Undoes `/Predictor` (ISO 32000-1 7.4.4.4): 1 (or less, as pdf.js reads
/// it) none, 2 TIFF, 10–15 PNG (each row says its own type). A final short
/// row is decoded as far as it goes. `/Columns` is untrusted and may be
/// huge: nothing here allocates by the row length.
fn unpredict(
    data: Vec<u8>,
    parms: Option<&Dictionary>,
    filter: &Filter,
) -> Result<Vec<u8>, DecodeError> {
    match unpredict_rows(data, parms, filter)? {
        (out, None) => Ok(out),
        (_, Some(at)) => Err(corrupt(filter.clone(), at)),
    }
}

/// [`unpredict`], reporting a PNG row with an invalid filter type as the
/// offset it starts at (the rows before it undone) instead of failing.
fn unpredict_rows(
    data: Vec<u8>,
    parms: Option<&Dictionary>,
    filter: &Filter,
) -> Result<(Vec<u8>, Option<usize>), DecodeError> {
    let predictor = int_parm(parms, b"Predictor", 1);
    if predictor <= 1 {
        return Ok((data, None));
    }
    let bad = || DecodeError::BadParms(filter.clone());
    let colors = int_parm(parms, b"Colors", 1);
    let bpc = int_parm(parms, b"BitsPerComponent", 8);
    let columns = int_parm(parms, b"Columns", 1);
    if !(1..=32).contains(&colors) || ![1, 2, 4, 8, 16].contains(&bpc) || columns < 1 {
        return Err(bad());
    }
    let (colors, bpc) = (colors as usize, bpc as usize);
    let columns = usize::try_from(columns).map_err(|_| bad())?;
    let row = colors
        .checked_mul(bpc)
        .and_then(|b| b.checked_mul(columns))
        .map(|bits| bits.div_ceil(8))
        .ok_or_else(bad)?;
    match predictor {
        2 => Ok((tiff(data, row, colors, bpc, columns), None)),
        10..=15 => Ok(png(&data, row, (colors * bpc).div_ceil(8))),
        _ => Err(bad()),
    }
}

/// Undoes PNG predictors row by row. A row whose filter type is above 4
/// stops it: the rows before it are returned with that row's offset.
fn png(data: &[u8], row: usize, bpp: usize) -> (Vec<u8>, Option<usize>) {
    // PNG-predicted output is never longer than its input; `row` comes from
    // `/Columns` and must not size an allocation.
    let mut out = Vec::with_capacity(data.len());
    let mut prev_start: Option<usize> = None;
    for (r, chunk) in data.chunks(row + 1).enumerate() {
        let start = out.len();
        let cur = &chunk[1..];
        for (i, &x) in cur.iter().enumerate() {
            let a = if i >= bpp { out[start + i - bpp] } else { 0 };
            let b = prev_start.map_or(0, |p| out[p + i]);
            let c = match prev_start {
                Some(p) if i >= bpp => out[p + i - bpp],
                _ => 0,
            };
            let pred = match chunk[0] {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                _ => {
                    out.truncate(start);
                    return (out, Some(r * (row + 1)));
                }
            };
            out.push(x.wrapping_add(pred));
        }
        prev_start = Some(start);
        if cur.len() < row {
            break;
        }
    }
    (out, None)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
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

/// TIFF predictor 2: each sample is the difference from the same colour
/// component of the pixel to its left, modulo 2^bpc. Row padding bits are
/// left as they are.
fn tiff(mut data: Vec<u8>, row: usize, colors: usize, bpc: usize, columns: usize) -> Vec<u8> {
    for cur in data.chunks_mut(row) {
        let samples = (cur.len() * 8 / bpc).min(colors * columns);
        match bpc {
            8 => {
                for i in colors..samples {
                    cur[i] = cur[i].wrapping_add(cur[i - colors]);
                }
            }
            16 => {
                for i in colors..samples {
                    let left =
                        u16::from_be_bytes([cur[2 * (i - colors)], cur[2 * (i - colors) + 1]]);
                    let v = u16::from_be_bytes([cur[2 * i], cur[2 * i + 1]]).wrapping_add(left);
                    cur[2 * i..2 * i + 2].copy_from_slice(&v.to_be_bytes());
                }
            }
            _ => {
                let mask = (1u8 << bpc) - 1;
                let shift = |i: usize| 8 - bpc - (i * bpc) % 8;
                for i in colors..samples {
                    let left = (cur[(i - colors) * bpc / 8] >> shift(i - colors)) & mask;
                    let byte = i * bpc / 8;
                    let v = ((cur[byte] >> shift(i)) & mask).wrapping_add(left) & mask;
                    cur[byte] = (cur[byte] & !(mask << shift(i))) | (v << shift(i));
                }
            }
        }
    }
    data
}

/// What `dict` and, failing that, the bytes say the stream is (TD §15).
/// `inflated` is the stream decoded through [`decode_chain`] (so an image
/// codec's data is still encoded). An explicit, recognised `/Type` or
/// `/Subtype` wins; then image magic, font magic, XMP, CMap markers, and the
/// content grammar: at least 90% of operator tokens are ISO 32000 operators
/// whose operand counts fit, with at least one text or path operator (an
/// outline-only page is content); a stream that passes the grammar with
/// `Do` but no text or path operator is a `Form`. An image's codec comes
/// from its filter, else from its magic, else it is raw samples.
pub(crate) fn classify(dict: &Dictionary, inflated: &[u8]) -> StreamClass {
    let name = |key: &[u8]| dict.get(key).ok().and_then(|o| o.as_name().ok());
    let image = || {
        let codec = filters_of(dict)
            .iter()
            .rev()
            .find_map(|(f, _)| f.codec())
            .or_else(|| image_magic(inflated))
            .unwrap_or(ImgCodec::Raw);
        StreamClass::Image { codec }
    };
    match (name(b"Type"), name(b"Subtype")) {
        (Some(b"ObjStm"), _) => return StreamClass::ObjStm,
        (Some(b"XRef"), _) => return StreamClass::XRef,
        (Some(b"Metadata"), _) | (_, Some(b"XML")) => return StreamClass::Metadata,
        (Some(b"CMap"), _) => return StreamClass::CMap,
        (_, Some(b"Image")) => return image(),
        (_, Some(b"Form")) => return StreamClass::Form,
        (_, Some(b"Type1C" | b"CIDFontType0C" | b"OpenType")) => return StreamClass::FontFile,
        _ => {}
    }
    if [&b"Length1"[..], b"Length2", b"Length3"]
        .iter()
        .any(|k| dict.has(k))
    {
        return StreamClass::FontFile;
    }
    if image_magic(inflated).is_some() || filters_of(dict).iter().any(|(f, _)| f.codec().is_some())
    {
        return image();
    }
    if [
        &b"\x00\x01\x00\x00"[..],
        b"OTTO",
        b"true",
        b"ttcf",
        b"%!PS-AdobeFont",
        b"%!FontType1",
    ]
    .iter()
    .any(|m| inflated.starts_with(m))
    {
        return StreamClass::FontFile;
    }
    let text = inflated
        .iter()
        .position(|&b| !is_ws(b))
        .map_or(&[][..], |i| &inflated[i..]);
    if text.starts_with(b"<?xpacket") || text.starts_with(b"<x:xmpmeta") {
        return StreamClass::Metadata;
    }
    if inflated.starts_with(b"%!PS")
        || contains(inflated, b"begincmap")
        || contains(inflated, b"endcmap")
    {
        return StreamClass::CMap;
    }
    content_grammar(inflated)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

fn image_magic(bytes: &[u8]) -> Option<ImgCodec> {
    if bytes.starts_with(b"\xff\xd8\xff") {
        Some(ImgCodec::Jpeg)
    } else if bytes.starts_with(b"\x89PNG") {
        Some(ImgCodec::Png)
    } else if bytes.starts_with(b"\x00\x00\x00\x0cjP") {
        Some(ImgCodec::Jp2)
    } else {
        None
    }
}

/// The content-grammar rule of [`classify`].
fn content_grammar(bytes: &[u8]) -> StreamClass {
    let (mut ops, mut good) = (0usize, 0usize);
    let (mut marks, mut xobject) = (false, false);
    for op in content_ops(bytes) {
        ops += 1;
        let Some(kind) = operator(op.op) else {
            continue;
        };
        if !kind.fits(op.operands.len()) {
            continue;
        }
        good += 1;
        marks |= matches!(kind.group, Group::Text | Group::Path);
        xobject |= op.op == b"Do";
    }
    if ops == 0 || good * 10 < ops * 9 {
        StreamClass::Other
    } else if marks {
        StreamClass::Content
    } else if xobject {
        StreamClass::Form
    } else {
        StreamClass::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Text,
    /// Path construction and painting, clipping, shading and inline images:
    /// what marks a page without text.
    Path,
    /// Graphics state, colour, XObjects, marked content, compatibility.
    Other,
}

/// An operator's operand count range and group.
#[derive(Debug, Clone, Copy)]
struct Kind {
    min: u8,
    max: u8,
    group: Group,
}

impl Kind {
    fn fits(self, n: usize) -> bool {
        (usize::from(self.min)..=usize::from(self.max)).contains(&n)
    }
}

/// The ISO 32000-1 operator set (Annex A, Table A.1) with operand counts.
/// `SC`/`sc` take 1–4 colour components, `SCN`/`scn` up to 32 and a
/// pattern name; `BI` carries the inline image's dictionary ([`Op`]).
fn operator(op: &[u8]) -> Option<Kind> {
    use Group::{Other, Path, Text};
    let (min, max, group) = match op {
        b"BT" | b"ET" | b"T*" => (0, 0, Text),
        b"Tc" | b"Tw" | b"Tz" | b"TL" | b"Tr" | b"Ts" | b"Tj" | b"TJ" | b"'" => (1, 1, Text),
        b"Tf" | b"Td" | b"TD" => (2, 2, Text),
        b"\"" => (3, 3, Text),
        b"Tm" => (6, 6, Text),
        b"h" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" | b"W"
        | b"W*" => (0, 0, Path),
        b"m" | b"l" => (2, 2, Path),
        b"v" | b"y" | b"re" => (4, 4, Path),
        b"c" => (6, 6, Path),
        b"sh" => (1, 1, Path),
        b"BI" => (0, 1, Path),
        b"q" | b"Q" | b"EMC" | b"BX" | b"EX" | b"ID" | b"EI" => (0, 0, Other),
        b"w" | b"J" | b"j" | b"M" | b"ri" | b"i" | b"gs" | b"CS" | b"cs" | b"G" | b"g" | b"Do"
        | b"MP" | b"BMC" => (1, 1, Other),
        b"d" | b"d0" | b"DP" | b"BDC" => (2, 2, Other),
        b"RG" | b"rg" => (3, 3, Other),
        b"K" | b"k" => (4, 4, Other),
        b"cm" | b"d1" => (6, 6, Other),
        b"SC" | b"sc" => (1, 4, Other),
        b"SCN" | b"scn" => (1, 33, Other),
        _ => return None,
    };
    Some(Kind { min, max, group })
}

/// One content-stream operator with the operands before it. `at` is the
/// operator's byte offset. An inline image is one `BI` op whose single
/// operand is its parameter dictionary; its data is skipped.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Op<'a> {
    pub op: &'a [u8],
    pub operands: Vec<Object>,
    pub at: usize,
}

/// More operands than any operator takes: past this the list is malformed.
const MAX_OPERANDS: usize = 64;

/// Walks a decoded content stream operator by operator on T-04's lexer.
/// Tolerant: an unknown operator is yielded as-is; a malformed operand (a
/// stray `]` or `>>`, a container cut short by an operator, nesting past
/// [`MAX_DEPTH`], more than [`MAX_OPERANDS`]) ends the current operand list,
/// never the walk; operands left at the end of input are dropped. Every
/// yielded op consumes at least its own bytes, so the walk ends.
pub(crate) fn content_ops(bytes: &[u8]) -> impl Iterator<Item = Op<'_>> {
    ContentOps {
        buf: bytes,
        lx: Lexer::new(bytes, 0).with_keyword_cutoff(false),
    }
}

struct ContentOps<'a> {
    buf: &'a [u8],
    lx: Lexer<'a>,
}

impl<'a> Iterator for ContentOps<'a> {
    type Item = Op<'a>;

    fn next(&mut self) -> Option<Op<'a>> {
        let mut operands = Vec::new();
        loop {
            let tok = self.lx.next();
            self.lx.take_notes();
            match tok {
                Tok::Eof => return None,
                Tok::Kw(op) if is_operator(op) => {
                    // A bareword ends where the lexer stopped.
                    let at = self.lx.pos - op.len();
                    if op == b"BI" {
                        return Some(self.inline_image(op, at));
                    }
                    return Some(Op { op, operands, at });
                }
                t => match operand(&mut self.lx, t, 0) {
                    Some(v) if operands.len() < MAX_OPERANDS => operands.push(v),
                    _ => operands.clear(),
                },
            }
        }
    }
}

impl<'a> ContentOps<'a> {
    /// After `BI`: the parameters to `ID`, then the data, skipped to `EI` by
    /// ISO 32000-1 8.9.7: one white-space byte after `ID`, then for
    /// unfiltered data (or a PDF 2.0 `/L`) exactly the image's byte count
    /// when `EI` follows it; otherwise up to the first `EI` with white space
    /// before it and white space, a delimiter or the end after it; failing
    /// that, the end of the stream.
    fn inline_image(&mut self, op: &'a [u8], at: usize) -> Op<'a> {
        let mut params = Dictionary::new();
        loop {
            let tok = self.lx.next();
            self.lx.take_notes();
            match tok {
                Tok::Eof | Tok::Kw(b"EI") => {
                    return Op {
                        op,
                        operands: vec![Object::Dictionary(params)],
                        at,
                    };
                }
                Tok::Kw(b"ID") => break,
                Tok::Name(key) => {
                    let mark = self.lx.pos;
                    let t = self.lx.next();
                    if matches!(t, Tok::Eof | Tok::Kw(b"ID" | b"EI")) {
                        self.lx.pos = mark;
                    } else if let Some(v) = operand(&mut self.lx, t, 1) {
                        params.set(key.into_owned(), v);
                    }
                }
                _ => {}
            }
        }
        let buf = self.buf;
        let mut start = self.lx.pos;
        if buf.get(start).is_some_and(|&b| is_ws(b)) {
            start += 1;
        }
        let ei_at = |i: usize| {
            buf[i..].starts_with(b"EI") && buf.get(i + 2).is_none_or(|&b| is_ws(b) || is_delim(b))
        };
        let by_length = inline_image_len(&params).and_then(|len| {
            let end = start.checked_add(len)?;
            let ei = end + buf.get(end..)?.iter().take_while(|&&b| is_ws(b)).count();
            ei_at(ei).then_some(ei)
        });
        let by_scan = || {
            memchr::memmem::find_iter(buf.get(start..).unwrap_or_default(), b"EI")
                .map(|i| start + i)
                .find(|&i| (i == start || is_ws(buf[i - 1])) && ei_at(i))
        };
        self.lx.pos = by_length.or_else(by_scan).map_or(buf.len(), |ei| ei + 2);
        Op {
            op,
            operands: vec![Object::Dictionary(params)],
            at,
        }
    }
}

/// The inline image's data length, when it can be known before reading it:
/// `/L` (`/Length`), or for unfiltered data rows of
/// `ceil(W × components × BPC / 8)` bytes times `H`.
fn inline_image_len(p: &Dictionary) -> Option<usize> {
    let get = |short: &[u8], long: &[u8]| p.get(short).or_else(|_| p.get(long)).ok();
    let int = |short: &[u8], long: &[u8]| {
        get(short, long)
            .and_then(|o| o.as_i64().ok())
            .and_then(|v| usize::try_from(v).ok())
    };
    if let Some(len) = int(b"L", b"Length") {
        return Some(len);
    }
    if get(b"F", b"Filter").is_some() {
        return None;
    }
    let mask = matches!(get(b"IM", b"ImageMask"), Some(Object::Boolean(true)));
    let (components, bpc) = if mask {
        (1, 1)
    } else {
        let components = match get(b"CS", b"ColorSpace")? {
            Object::Name(n) => match n.as_slice() {
                b"G" | b"DeviceGray" | b"I" | b"Indexed" => 1,
                b"RGB" | b"DeviceRGB" => 3,
                b"CMYK" | b"DeviceCMYK" => 4,
                _ => return None,
            },
            Object::Array(a) if matches!(a.first(), Some(Object::Name(n)) if n == b"I" || n == b"Indexed") => {
                1
            }
            _ => return None,
        };
        (components, int(b"BPC", b"BitsPerComponent")?)
    };
    let w = int(b"W", b"Width")?;
    let h = int(b"H", b"Height")?;
    w.checked_mul(components)?
        .checked_mul(bpc)?
        .div_ceil(8)
        .checked_mul(h)
}

/// A bareword that is not a value keyword.
fn is_operator(word: &[u8]) -> bool {
    !matches!(word, b"true" | b"false" | b"null")
}

/// The operand `tok` starts. `None` if it is not a well-formed operand;
/// when an operator cut a container short, the lexer is put back on it.
fn operand<'a>(lx: &mut Lexer<'a>, tok: Tok<'a>, depth: u8) -> Option<Object> {
    Some(match tok {
        Tok::Int(v) => Object::Integer(v),
        Tok::Real(v) => Object::Real(v as f32),
        Tok::Name(n) => Object::Name(n.into_owned()),
        Tok::LitStr(s) => Object::String(s, StringFormat::Literal),
        Tok::HexStr(s) => Object::String(s, StringFormat::Hexadecimal),
        Tok::Kw(b"true") => Object::Boolean(true),
        Tok::Kw(b"false") => Object::Boolean(false),
        Tok::Kw(b"null") => Object::Null,
        Tok::ArrOpen | Tok::DictOpen if depth >= MAX_DEPTH => return None,
        Tok::ArrOpen => {
            let mut items = Vec::new();
            loop {
                let mark = lx.pos;
                match lx.next() {
                    Tok::ArrClose => break,
                    Tok::Eof => return None,
                    Tok::Kw(k) if is_operator(k) => {
                        lx.pos = mark;
                        return None;
                    }
                    t => {
                        let v = operand(lx, t, depth + 1)?;
                        if items.len() < MAX_ELEMENTS {
                            items.push(v);
                        }
                    }
                }
            }
            Object::Array(items)
        }
        Tok::DictOpen => {
            let mut dict = Dictionary::new();
            loop {
                let mark = lx.pos;
                match lx.next() {
                    Tok::DictClose => break,
                    Tok::Name(key) => {
                        let vmark = lx.pos;
                        let t = lx.next();
                        if matches!(t, Tok::Kw(k) if is_operator(k)) {
                            lx.pos = vmark;
                            return None;
                        }
                        let v = operand(lx, t, depth + 1)?;
                        if dict.len() < MAX_ELEMENTS {
                            dict.set(key.into_owned(), v);
                        }
                    }
                    Tok::Kw(k) if is_operator(k) => {
                        lx.pos = mark;
                        return None;
                    }
                    _ => return None,
                }
            }
            Object::Dictionary(dict)
        }
        Tok::Kw(_) | Tok::ArrClose | Tok::DictClose | Tok::Eof => return None,
    })
}

#[cfg(test)]
mod tests;
