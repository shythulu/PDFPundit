//! Tolerant lexer (T-04, TD §13): a byte-slice tokenizer and object-value
//! parser for damaged PDFs. It never panics, and every token other than
//! [`Tok::Eof`] moves `pos` forward, so a caller looping on [`Lexer::next`]
//! always terminates.
// The carver (T-05) is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::borrow::Cow;

use lopdf::Object;

/// Deepest container nesting [`parse_value`] accepts (TD §13.3).
pub const MAX_DEPTH: u8 = 100;

/// Most elements read per array, or entries per dictionary: the figure qpdf's
/// 12.2.0 recovery adopts. Elements and entries that were dropped count too.
/// The rest are skipped with [`LexNote::ContainerTruncated`].
pub const MAX_ELEMENTS: usize = 5_000;

/// One token. `Kw` is any bareword, keyword or not (content operators too).
#[derive(Debug, Clone, PartialEq)]
pub enum Tok<'a> {
    Int(i64),
    Real(f64),
    /// `/` consumed; `#xx` unescaped (borrowed when there was none).
    Name(Cow<'a, [u8]>),
    /// `( … )` with escapes and balanced parentheses resolved.
    LitStr(Vec<u8>),
    /// `< … >`; an odd final nibble is padded with 0.
    HexStr(Vec<u8>),
    ArrOpen,
    ArrClose,
    DictOpen,
    DictClose,
    Kw(&'a [u8]),
    Eof,
}

/// Something the lexer or parser tolerated. `at` is a byte offset into the
/// buffer the lexer was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexNote {
    /// A `(` with no balancing `)`: it ends at EOF or, with the keyword
    /// cutoff on, before an `endobj` that the next object's header, `xref`,
    /// `trailer`, `startxref`, `%%EOF` or EOF follows.
    UnterminatedString { at: u64 },
    /// A `<` with no closing `>` before EOF, another delimiter or a structural
    /// keyword (`endobj`, `obj`, `stream`, `xref`, ...).
    UnterminatedHex { at: u64 },
    /// A hex string held a byte that is not a hex digit; decoding stopped there.
    BadHexByte { at: u64 },
    /// Bytes that cannot start a token were skipped.
    Skipped { at: u64, len: u64 },
    /// Near-miss mode: a bareword one byte away from `obj`, `endobj`, `stream`
    /// or `endstream` was read as that keyword.
    NearMissKeyword { at: u64 },
    /// Near-miss mode: a bareword of digits with exactly one other byte (the
    /// `115 0 obj` → `11l 0 obj` damage). The token stays a `Kw`: the digit
    /// that was lost cannot be known.
    NearMissNumber { at: u64 },
    /// An array or dictionary (starting at `at`) held more than
    /// [`MAX_ELEMENTS`]; the rest were skipped.
    ContainerTruncated { at: u64 },
    /// A real does not round-trip through `f32`, which is what
    /// `lopdf::Object::Real` stores (D-075). The tokenizer adds it too for an
    /// integer past the `i64` range (read as a real) and for a number beyond
    /// the `f32` range (clamped to `±f32::MAX`). One note per token.
    RealNarrowed { at: u64 },
    /// A dictionary key or value that did not parse was dropped.
    DictRecovered { at: u64 },
    /// An array element that is not a value was dropped.
    ElementSkipped { at: u64 },
    /// A container (starting at `at`) ended at EOF or a structural keyword
    /// before its closing bracket.
    UnterminatedContainer { at: u64 },
}

/// Why [`parse_value`] produced nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LexErr {
    /// More than [`MAX_DEPTH`] nested containers (a nesting bomb).
    #[error("containers nested deeper than {MAX_DEPTH}")]
    TooDeep,
    /// The buffer ended where a value was expected.
    #[error("end of input where a value was expected")]
    UnexpectedEof,
    /// The token at `at` is not the start of a value.
    #[error("no value at byte {at}")]
    NotAValue { at: u64 },
}

/// One parsed value, the offset just past it, and what was tolerated.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedObj {
    pub value: Object,
    pub end: usize,
    pub notes: Vec<LexNote>,
}

/// The tokenizer. `pos` is the next byte to read.
pub struct Lexer<'a> {
    buf: &'a [u8],
    pub pos: usize,
    near_miss: bool,
    keyword_cutoff: bool,
    notes: Vec<LexNote>,
}

impl<'a> Lexer<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self {
        Lexer {
            buf,
            pos,
            near_miss: false,
            keyword_cutoff: true,
            notes: Vec::new(),
        }
    }

    /// Near-miss mode, for the carver: see [`LexNote::NearMissKeyword`] and
    /// [`LexNote::NearMissNumber`].
    pub fn with_near_miss(mut self, on: bool) -> Self {
        self.near_miss = on;
        self
    }

    /// On by default: an unterminated string stops before the object-structure
    /// keywords that follow it (see [`LexNote::UnterminatedString`] and
    /// [`LexNote::UnterminatedHex`]). Turn it off for content streams, which
    /// have no object structure: there a string runs to its terminator or EOF.
    pub fn with_keyword_cutoff(mut self, on: bool) -> Self {
        self.keyword_cutoff = on;
        self
    }

    /// Parses one value at `pos` with this lexer's settings, as
    /// [`parse_value`] does; notes collect on the lexer. For callers such as
    /// the content-operator iterator that read values and operators in turn.
    pub fn read_value(&mut self, depth: u8) -> Result<Object, LexErr> {
        let (at, tok) = self.next_at();
        value(self, at, tok, depth)
    }

    /// The next token; skips whitespace and comments first. Never panics.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Tok<'a> {
        self.next_at().1
    }

    /// [`Self::next`] without advancing (and without adding notes).
    pub fn peek(&mut self) -> Tok<'a> {
        let mark = self.mark();
        let tok = self.next();
        self.reset(mark);
        tok
    }

    /// The notes gathered so far, oldest first; the lexer's list is emptied.
    pub fn take_notes(&mut self) -> Vec<LexNote> {
        std::mem::take(&mut self.notes)
    }

    fn mark(&self) -> (usize, usize) {
        (self.pos, self.notes.len())
    }

    fn reset(&mut self, (pos, notes): (usize, usize)) {
        self.pos = pos;
        self.notes.truncate(notes);
    }

    fn note(&mut self, note: LexNote) {
        self.notes.push(note);
    }

    /// The next token and the offset it starts at. Bytes that cannot start a
    /// token are skipped; each contiguous run gets one `Skipped` note.
    fn next_at(&mut self) -> (usize, Tok<'a>) {
        let mut skipped: Option<(usize, usize)> = None;
        loop {
            self.skip_ws_and_comments();
            let start = self.pos;
            let Some(&b) = self.buf.get(start) else {
                self.flush_skipped(skipped);
                return (start, Tok::Eof);
            };
            let after = self.buf.get(start + 1).copied();
            // `)`, `{`, `}` and a lone `>` cannot start a token: skip them.
            if matches!(b, b')' | b'{' | b'}') || (b == b'>' && after != Some(b'>')) {
                self.pos = start + 1;
                skipped = match skipped {
                    Some((at, len)) if at + len == start => Some((at, len + 1)),
                    run => {
                        self.flush_skipped(run);
                        Some((start, 1))
                    }
                };
                continue;
            }
            self.flush_skipped(skipped);
            let tok = match b {
                b'[' => {
                    self.pos = start + 1;
                    Tok::ArrOpen
                }
                b']' => {
                    self.pos = start + 1;
                    Tok::ArrClose
                }
                b'<' if after == Some(b'<') => {
                    self.pos = start + 2;
                    Tok::DictOpen
                }
                b'>' => {
                    self.pos = start + 2;
                    Tok::DictClose
                }
                b'<' => self.read_hex_string(start),
                b'(' => self.read_lit_string(start),
                b'/' => self.read_name(start),
                b'0'..=b'9' | b'+' | b'-' | b'.' => self.read_number(start),
                // Every other byte here is regular (whitespace, `%` and the
                // unknown delimiters are consumed above), so the run is non-empty.
                _ => self.read_bareword(start),
            };
            return (start, tok);
        }
    }

    fn skip_ws_and_comments(&mut self) {
        while let Some(&b) = self.buf.get(self.pos) {
            if is_ws(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self
                    .buf
                    .get(self.pos)
                    .is_some_and(|&c| c != b'\r' && c != b'\n')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn flush_skipped(&mut self, run: Option<(usize, usize)>) {
        if let Some((at, len)) = run {
            self.note(LexNote::Skipped {
                at: at as u64,
                len: len as u64,
            });
        }
    }

    /// Signs, digits and at most one point. A second sign or point ends the
    /// number; extra leading signs (`--5`) are dropped. A run that continues
    /// with any other regular byte is a bareword, so `11l` never reads as 11.
    fn read_number(&mut self, start: usize) -> Tok<'a> {
        let buf = self.buf;
        let mut i = start;
        let negative = buf[i] == b'-';
        while buf.get(i).is_some_and(|&b| b == b'+' || b == b'-') {
            i += 1;
        }
        let body = i;
        let mut point = false;
        let mut digits = 0usize;
        let mut anomaly = false;
        while let Some(&b) = buf.get(i) {
            match b {
                b'0'..=b'9' => digits += 1,
                b'.' if !point => point = true,
                b'.' | b'+' | b'-' => {
                    anomaly = true;
                    break;
                }
                _ => break,
            }
            i += 1;
        }
        if !anomaly && buf.get(i).is_some_and(|&b| is_reg(b)) {
            return self.read_bareword(start);
        }
        self.pos = i;
        let text = &buf[body..i];
        if digits == 0 {
            return if point { Tok::Real(0.0) } else { Tok::Int(0) };
        }
        if !point {
            // Accumulated with its sign, so i64::MIN stays an integer.
            let int = text.iter().try_fold(0i64, |acc, &d| {
                let d = i64::from(d - b'0');
                let acc = acc.checked_mul(10)?;
                if negative {
                    acc.checked_sub(d)
                } else {
                    acc.checked_add(d)
                }
            });
            if let Some(v) = int {
                return Tok::Int(v);
            }
            // Past the i64 range: a real, which changes its type and may
            // round its value.
            self.note(LexNote::RealNarrowed { at: start as u64 });
        }
        // ASCII digits and one point: always valid UTF-8 and a valid float.
        let v = std::str::from_utf8(text)
            .ok()
            .and_then(|t| t.parse::<f64>().ok())
            .unwrap_or(0.0);
        let v = if negative { -v } else { v };
        // A run too long for f64 parses to infinity, which no PDF writer can
        // emit: clamp it, and anything past f32, to the largest f32.
        let limit = f64::from(f32::MAX);
        if v.abs() > limit {
            if point {
                self.note(LexNote::RealNarrowed { at: start as u64 });
            }
            return Tok::Real(if v < 0.0 { -limit } else { limit });
        }
        Tok::Real(v)
    }

    /// A run of regular bytes. In near-miss mode a run one byte away from a
    /// carver keyword reads as that keyword. A run that is the keyword less one
    /// byte, followed by `<`, `>`, `(`, `[`, `]`, `/` or `%` (`ob<<`,
    /// `endob>>`), reads as a deletion and leaves the delimiter unread. Otherwise the window may
    /// substitute one byte, which may be a delimiter or whitespace that split
    /// the run (`endob)`).
    fn read_bareword(&mut self, start: usize) -> Tok<'a> {
        let buf = self.buf;
        let end = start + buf[start..].iter().take_while(|&&b| is_reg(b)).count();
        let run = &buf[start..end];
        self.pos = end;
        if !self.near_miss || NEAR_MISS_KEYWORDS.contains(&run) {
            return Tok::Kw(run);
        }
        if buf
            .get(end)
            .is_some_and(|b| matches!(b, b'<' | b'>' | b'(' | b'[' | b']' | b'/' | b'%'))
            && let Some(kw) = NEAR_MISS_KEYWORDS
                .into_iter()
                .find(|kw| is_one_deletion(run, kw))
        {
            self.note(LexNote::NearMissKeyword { at: start as u64 });
            return Tok::Kw(kw);
        }
        for kw in NEAR_MISS_KEYWORDS {
            let stop = start + kw.len();
            let Some(window) = buf.get(start..stop) else {
                continue;
            };
            let diffs = window.iter().zip(kw).filter(|(a, b)| a != b).count();
            if diffs == 1 && buf.get(stop).is_none_or(|&b| !is_reg(b)) {
                self.pos = stop;
                self.note(LexNote::NearMissKeyword { at: start as u64 });
                return Tok::Kw(kw);
            }
        }
        if run.len() >= 2 && run.iter().filter(|b| !b.is_ascii_digit()).count() == 1 {
            self.note(LexNote::NearMissNumber { at: start as u64 });
        }
        Tok::Kw(run)
    }

    fn read_name(&mut self, start: usize) -> Tok<'a> {
        let buf = self.buf;
        let from = start + 1;
        let end = from + buf[from..].iter().take_while(|&&b| is_reg(b)).count();
        self.pos = end;
        let raw = &buf[from..end];
        if !raw.contains(&b'#') {
            return Tok::Name(Cow::Borrowed(raw));
        }
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'#'
                && let (Some(h), Some(l)) = (
                    raw.get(i + 1).and_then(|&b| hex_val(b)),
                    raw.get(i + 2).and_then(|&b| hex_val(b)),
                )
            {
                out.push(h << 4 | l);
                i += 3;
            } else {
                out.push(raw[i]);
                i += 1;
            }
        }
        Tok::Name(Cow::Owned(out))
    }

    /// A string runs to its balancing `)`. With the keyword cutoff on, one
    /// whose `)` is missing ends before an `endobj` that the file structure
    /// follows: see [`scan_lit`].
    fn read_lit_string(&mut self, start: usize) -> Tok<'a> {
        let from = start + 1;
        match scan_lit(self.buf, from, self.keyword_cutoff) {
            Ok(close) => {
                self.pos = close + 1;
                Tok::LitStr(decode_lit(&self.buf[from..close]))
            }
            Err(stop) => {
                self.pos = stop;
                self.note(LexNote::UnterminatedString { at: start as u64 });
                Tok::LitStr(decode_lit(&self.buf[from..stop]))
            }
        }
    }

    /// Hex digits to `>`, ignoring whitespace. Another delimiter, EOF or (with
    /// the keyword cutoff on) a standalone structural keyword ends an
    /// unterminated string, left unread. A regular non-hex byte stops
    /// decoding, and the string still runs to one of those terminators.
    fn read_hex_string(&mut self, start: usize) -> Tok<'a> {
        let buf = self.buf;
        let mut i = start + 1;
        let mut out = Vec::new();
        let mut high: Option<u8> = None;
        let mut bad = false;
        loop {
            match buf.get(i) {
                Some(b'>') => {
                    i += 1;
                    break;
                }
                Some(&b) if is_ws(b) => {}
                Some(&b) if is_delim(b) => {
                    self.note(LexNote::UnterminatedHex { at: start as u64 });
                    break;
                }
                None => {
                    self.note(LexNote::UnterminatedHex { at: start as u64 });
                    break;
                }
                Some(_)
                    if self.keyword_cutoff && STRUCTURAL.iter().any(|kw| is_kw_at(buf, i, kw)) =>
                {
                    self.note(LexNote::UnterminatedHex { at: start as u64 });
                    break;
                }
                Some(&b) => match hex_val(b) {
                    Some(v) if !bad => match high.take() {
                        Some(h) => out.push(h << 4 | v),
                        None => high = Some(v),
                    },
                    Some(_) => {}
                    None => {
                        if !bad {
                            bad = true;
                            self.note(LexNote::BadHexByte { at: i as u64 });
                        }
                    }
                },
            }
            i += 1;
        }
        if let Some(h) = high {
            out.push(h << 4);
        }
        self.pos = i;
        Tok::HexStr(out)
    }
}

/// Keywords the carver matches with one byte of tolerance in near-miss mode.
const NEAR_MISS_KEYWORDS: [&[u8]; 4] = [b"obj", b"endobj", b"stream", b"endstream"];

/// Barewords that end any open container: they belong to the file structure,
/// never to a value.
const STRUCTURAL: [&[u8]; 7] = [
    b"obj",
    b"endobj",
    b"stream",
    b"endstream",
    b"xref",
    b"trailer",
    b"startxref",
];

#[inline]
fn is_ws(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

#[inline]
fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[inline]
fn is_reg(b: u8) -> bool {
    !is_ws(b) && !is_delim(b)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Where a literal string whose body starts at `from` ends: `Ok` at the `)`
/// that balances it (escaped bytes skipped), else `Err` where it is cut off.
/// A standalone `endobj` inside a string ends it only when, with `cutoff`
/// on, [`ends_object`] finds the file structure going on right after it; so
/// `(endobj) Tj` and `(About endobj syntax)` stay whole. Without such an
/// `endobj` the string runs to EOF. The decision looks at most
/// [`BOUNDARY_LOOKAHEAD`] bytes past each `endobj` and the lexer resumes at
/// the cut, so bytes scanned stay within that distance of bytes consumed and
/// tokenizing any buffer is linear.
fn scan_lit(buf: &[u8], from: usize, cutoff: bool) -> Result<usize, usize> {
    let mut depth = 1usize;
    let mut i = from;
    let mut looked = from;
    let result = loop {
        let Some(&b) = buf.get(i) else {
            break Err(buf.len());
        };
        match b {
            b'\\' => i += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break Ok(i);
                }
            }
            b'e' if cutoff && is_kw_at(buf, i, b"endobj") => {
                let (ends, seen) = ends_object(buf, i + 6);
                looked = looked.max(seen);
                if ends {
                    break Err(i);
                }
                // `endobj` holds no `(`, `)` or backslash.
                i += 5;
            }
            _ => {}
        }
        i += 1;
    };
    #[cfg(test)]
    LIT_SCANNED.with(|c| c.set(c.get() + looked.max(i).min(buf.len()) - from));
    #[cfg(not(test))]
    let _ = looked;
    result
}

/// Bytes [`ends_object`] reads past an `endobj` at most. The bound keeps a
/// scan linear even when many `endobj`s share one long comment line.
const BOUNDARY_LOOKAHEAD: usize = 256;

/// Whether the bytes from `i` (just past an `endobj`) are where the file
/// structure goes on: after whitespace and comments, EOF, `%%EOF`, `xref`,
/// `trailer`, `startxref` or an `N G obj` header. Reads at most
/// [`BOUNDARY_LOOKAHEAD`] bytes (plus a keyword's length); a window that runs
/// out first is not a boundary. Also returns one past the last byte read.
fn ends_object(buf: &[u8], i: usize) -> (bool, usize) {
    let limit = i.saturating_add(BOUNDARY_LOOKAHEAD).min(buf.len());
    let mut j = i;
    loop {
        if j >= limit {
            return (j == buf.len(), j);
        }
        match buf[j] {
            b if is_ws(b) => j += 1,
            b'%' => {
                if buf[j..].starts_with(b"%%EOF") {
                    return (true, j + 5);
                }
                while j < limit && buf[j] != b'\r' && buf[j] != b'\n' {
                    j += 1;
                }
            }
            _ => break,
        }
    }
    if let Some(kw) = [&b"xref"[..], b"trailer", b"startxref"]
        .into_iter()
        .find(|kw| is_kw_at(buf, j, kw))
    {
        return (true, j + kw.len() + 1);
    }
    let run = |from: usize, f: fn(u8) -> bool| {
        from + buf[from..limit].iter().take_while(|&&b| f(b)).count()
    };
    let mut at = j;
    for f in [is_digit, is_ws, is_digit, is_ws] {
        let next = run(at, f);
        if next == at {
            return (false, next + 1);
        }
        at = next;
    }
    (is_kw_at(buf, at, b"obj"), at + 4)
}

fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

#[cfg(test)]
thread_local! {
    /// Bytes [`scan_lit`] has read on this thread, lookahead included.
    static LIT_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `kw` standing as its own token at `i`.
fn is_kw_at(buf: &[u8], i: usize, kw: &[u8]) -> bool {
    buf.get(i..).is_some_and(|rest| rest.starts_with(kw))
        && (i == 0 || !is_reg(buf[i - 1]))
        && buf.get(i + kw.len()).is_none_or(|&b| !is_reg(b))
}

/// `run` is `kw` with exactly one byte deleted.
fn is_one_deletion(run: &[u8], kw: &[u8]) -> bool {
    if run.len() + 1 != kw.len() {
        return false;
    }
    let same = run.iter().zip(kw).take_while(|(a, b)| a == b).count();
    run[same..] == kw[same + 1..]
}

/// Resolves escapes in a literal string body (ISO 32000-1 7.3.4.2): an
/// unescaped EOL reads as LF, `\<EOL>` continues the line, `\ddd` keeps the
/// low eight bits, and an unknown escape drops the backslash.
fn decode_lit(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while let Some(&b) = body.get(i) {
        i += 1;
        match b {
            b'\\' => {
                let Some(&e) = body.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'0'..=b'7' => {
                        let mut v = u32::from(e - b'0');
                        for _ in 0..2 {
                            match body.get(i) {
                                Some(&d @ b'0'..=b'7') => {
                                    v = v * 8 + u32::from(d - b'0');
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push((v & 0xff) as u8);
                    }
                    b'\r' => {
                        if body.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'\n' => {}
                    other => out.push(other),
                }
            }
            b'\r' => {
                if body.get(i) == Some(&b'\n') {
                    i += 1;
                }
                out.push(b'\n');
            }
            other => out.push(other),
        }
    }
    out
}

/// Parses one object value starting at `pos` (after `N G obj` or inside a
/// container). Builds arrays and dictionaries recursively and reads
/// `N G R` as a reference. `depth` is the caller's nesting level, 0 at the top.
pub fn parse_value(buf: &[u8], pos: usize, depth: u8) -> Result<ParsedObj, LexErr> {
    let mut lx = Lexer::new(buf, pos);
    let value = lx.read_value(depth)?;
    Ok(ParsedObj {
        value,
        end: lx.pos,
        notes: lx.take_notes(),
    })
}

/// The value that starts with `tok` (already read, at `at`).
fn value(lx: &mut Lexer<'_>, at: usize, tok: Tok<'_>, depth: u8) -> Result<Object, LexErr> {
    if depth > MAX_DEPTH {
        return Err(LexErr::TooDeep);
    }
    Ok(match tok {
        Tok::Int(a) => int_or_ref(lx, a),
        Tok::Real(v) => {
            let narrow = v as f32;
            let note = LexNote::RealNarrowed { at: at as u64 };
            // The tokenizer may have noted this token already.
            if f64::from(narrow).to_bits() != v.to_bits() && lx.notes.last() != Some(&note) {
                lx.note(note);
            }
            Object::Real(narrow)
        }
        Tok::Name(n) => Object::Name(n.into_owned()),
        Tok::LitStr(s) => Object::String(s, lopdf::StringFormat::Literal),
        Tok::HexStr(s) => Object::String(s, lopdf::StringFormat::Hexadecimal),
        Tok::ArrOpen | Tok::DictOpen if depth >= MAX_DEPTH => return Err(LexErr::TooDeep),
        Tok::ArrOpen => array(lx, at, depth)?,
        Tok::DictOpen => dict(lx, at, depth)?,
        Tok::Kw(b"true") => Object::Boolean(true),
        Tok::Kw(b"false") => Object::Boolean(false),
        Tok::Kw(b"null") => Object::Null,
        Tok::Eof => return Err(LexErr::UnexpectedEof),
        Tok::Kw(_) | Tok::ArrClose | Tok::DictClose => {
            return Err(LexErr::NotAValue { at: at as u64 });
        }
    })
}

/// `a` alone, or `a b R` when the next two tokens are an in-range generation
/// and `R`; otherwise the lexer is put back after `a`.
fn int_or_ref(lx: &mut Lexer<'_>, a: i64) -> Object {
    if let Ok(num) = u32::try_from(a) {
        let mark = lx.mark();
        if let Tok::Int(b) = lx.next()
            && let Ok(generation) = u16::try_from(b)
            && lx.next() == Tok::Kw(b"R")
        {
            return Object::Reference((num, generation));
        }
        lx.reset(mark);
    }
    Object::Integer(a)
}

fn is_structural(tok: &Tok<'_>) -> bool {
    matches!(tok, Tok::Kw(k) if STRUCTURAL.contains(k))
}

/// Elements to `]`. A non-value element is dropped; `>>`, EOF or a structural
/// keyword ends the array unterminated, without consuming it. Dropped
/// elements count toward [`MAX_ELEMENTS`].
fn array(lx: &mut Lexer<'_>, open: usize, depth: u8) -> Result<Object, LexErr> {
    let mut items = Vec::new();
    let mut seen = 0usize;
    loop {
        let mark = lx.mark();
        let (at, tok) = lx.next_at();
        match tok {
            Tok::ArrClose => break,
            Tok::Eof | Tok::DictClose => {
                lx.reset(mark);
                lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                break;
            }
            t if is_structural(&t) => {
                lx.reset(mark);
                lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                break;
            }
            t if seen == MAX_ELEMENTS => {
                lx.note(LexNote::ContainerTruncated { at: open as u64 });
                skip_rest(lx, &t, open);
                break;
            }
            t => {
                seen += 1;
                match value(lx, at, t, depth + 1) {
                    Ok(v) => items.push(v),
                    Err(LexErr::TooDeep) => return Err(LexErr::TooDeep),
                    Err(_) => lx.note(LexNote::ElementSkipped { at: at as u64 }),
                }
            }
        }
    }
    Ok(Object::Array(items))
}

/// `key value` pairs to `>>`. A key whose value does not parse is dropped and
/// the tokens up to the next key or `>>` are skipped (`DictRecovered`); a
/// literal string is one token, so a `>>` inside it never closes the dict.
/// Every entry read, kept or recovered, counts toward [`MAX_ELEMENTS`].
fn dict(lx: &mut Lexer<'_>, open: usize, depth: u8) -> Result<Object, LexErr> {
    let mut dict = lopdf::Dictionary::new();
    let mut seen = 0usize;
    loop {
        let mark = lx.mark();
        let (at, tok) = lx.next_at();
        match tok {
            Tok::DictClose => break,
            t if t == Tok::Eof || is_structural(&t) => {
                lx.reset(mark);
                lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                break;
            }
            t if seen == MAX_ELEMENTS => {
                lx.note(LexNote::ContainerTruncated { at: open as u64 });
                skip_rest(lx, &t, open);
                break;
            }
            Tok::Name(key) => {
                seen += 1;
                let vmark = lx.mark();
                let (vat, vtok) = lx.next_at();
                match vtok {
                    Tok::DictClose => {
                        lx.note(LexNote::DictRecovered { at: at as u64 });
                        break;
                    }
                    t if t == Tok::Eof || is_structural(&t) => {
                        lx.reset(vmark);
                        lx.note(LexNote::DictRecovered { at: at as u64 });
                        lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                        break;
                    }
                    t => match value(lx, vat, t, depth + 1) {
                        Ok(v) => dict.set(key.into_owned(), v),
                        Err(LexErr::TooDeep) => return Err(LexErr::TooDeep),
                        Err(_) => {
                            skip_to_key(lx);
                            lx.note(LexNote::DictRecovered { at: at as u64 });
                        }
                    },
                }
            }
            t => {
                seen += 1;
                let opens = usize::from(matches!(t, Tok::ArrOpen | Tok::DictOpen));
                skip_to_key_from(lx, opens);
                lx.note(LexNote::DictRecovered { at: at as u64 });
            }
        }
    }
    Ok(Object::Dictionary(dict))
}

fn skip_to_key(lx: &mut Lexer<'_>) {
    skip_to_key_from(lx, 0);
}

/// Skips tokens, keeping `[`/`<<` against `]`/`>>` balanced, until a name or
/// `>>` at balance zero (left unread), EOF or a structural keyword.
fn skip_to_key_from(lx: &mut Lexer<'_>, mut balance: usize) {
    loop {
        let mark = lx.mark();
        match lx.next() {
            Tok::Name(_) | Tok::DictClose if balance == 0 => {
                lx.reset(mark);
                return;
            }
            t if t == Tok::Eof || is_structural(&t) => {
                lx.reset(mark);
                return;
            }
            Tok::ArrOpen | Tok::DictOpen => balance += 1,
            Tok::ArrClose | Tok::DictClose => balance = balance.saturating_sub(1),
            _ => {}
        }
    }
}

/// After the element cap: skips `first` and every token up to and including
/// the container's own closer (`open` names which one), without building
/// values or recursing. EOF or a structural keyword stops it unread.
fn skip_rest(lx: &mut Lexer<'_>, first: &Tok<'_>, open: usize) {
    let is_array = lx.buf.get(open) == Some(&b'[');
    let mut balance = usize::from(matches!(first, Tok::ArrOpen | Tok::DictOpen));
    loop {
        let mark = lx.mark();
        match lx.next() {
            t if t == Tok::Eof || is_structural(&t) => {
                lx.reset(mark);
                lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                return;
            }
            Tok::ArrOpen | Tok::DictOpen => balance += 1,
            Tok::ArrClose if balance == 0 && is_array => return,
            Tok::DictClose if balance == 0 && !is_array => return,
            Tok::DictClose if balance == 0 => {
                lx.reset(mark);
                lx.note(LexNote::UnterminatedContainer { at: open as u64 });
                return;
            }
            Tok::ArrClose | Tok::DictClose => balance = balance.saturating_sub(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::StringFormat;

    fn toks(src: &[u8], near_miss: bool) -> (Vec<Tok<'_>>, Vec<LexNote>) {
        let mut lx = Lexer::new(src, 0).with_near_miss(near_miss);
        let mut out = Vec::new();
        loop {
            match lx.next() {
                Tok::Eof => break,
                t => out.push(t),
            }
        }
        (out, lx.take_notes())
    }

    fn parse(src: &[u8]) -> ParsedObj {
        match parse_value(src, 0, 0) {
            Ok(p) => p,
            Err(e) => panic!("parse_value failed: {e}"),
        }
    }

    fn arr(o: &Object) -> &[Object] {
        match o {
            Object::Array(a) => a,
            _ => panic!("not an array"),
        }
    }

    fn dict(o: &Object) -> &lopdf::Dictionary {
        match o {
            Object::Dictionary(d) => d,
            _ => panic!("not a dictionary"),
        }
    }

    fn get<'d>(d: &'d lopdf::Dictionary, k: &[u8]) -> Option<&'d Object> {
        d.get(k).ok()
    }

    fn lit(s: &[u8]) -> Object {
        Object::String(s.to_vec(), StringFormat::Literal)
    }

    // Robustness rule: unterminated `(` runs to EOF or the next `endobj`.
    #[test]
    fn unterminated_string_runs_to_eof() {
        let (t, n) = toks(b"(abc", false);
        assert_eq!(t, vec![Tok::LitStr(b"abc".to_vec())]);
        assert_eq!(n, vec![LexNote::UnterminatedString { at: 0 }]);
    }

    #[test]
    fn unterminated_string_stops_before_next_endobj() {
        let src = b"(abc\nendobj\n5 0 obj\n(x)) endobj";
        let (t, n) = toks(src, false);
        assert_eq!(t[0], Tok::LitStr(b"abc\n".to_vec()));
        assert_eq!(t[1], Tok::Kw(b"endobj"));
        assert_eq!(t[2], Tok::Int(5));
        assert_eq!(n[0], LexNote::UnterminatedString { at: 0 });
    }

    // A terminated string that holds a standalone `endobj` keeps it.
    #[test]
    fn endobj_inside_a_terminated_string() {
        let (t, n) = toks(b"(endobj) Tj", false);
        assert_eq!(t, vec![Tok::LitStr(b"endobj".to_vec()), Tok::Kw(b"Tj")]);
        assert!(n.is_empty());

        let p = parse(b"<< /Title (About endobj syntax) /Author (x) /N 3 >>");
        let d = dict(&p.value);
        assert_eq!(get(d, b"Title"), Some(&lit(b"About endobj syntax")));
        assert_eq!(get(d, b"Author"), Some(&lit(b"x")));
        assert_eq!(get(d, b"N"), Some(&Object::Integer(3)));
        assert!(p.notes.is_empty());
    }

    #[test]
    fn unterminated_string_cut_at_endobj_before_eof() {
        let src = b"(abc endobj\n";
        let (t, n) = toks(src, false);
        assert_eq!(t, vec![Tok::LitStr(b"abc ".to_vec()), Tok::Kw(b"endobj")]);
        assert_eq!(n, vec![LexNote::UnterminatedString { at: 0 }]);
    }

    // The cut needs the file structure right after `endobj`; anything else
    // keeps it inside the string.
    #[test]
    fn unterminated_string_endobj_boundaries() {
        for tail in [
            &b"xref\n0 1"[..],
            b"trailer <<>>",
            b"startxref 9",
            b"%%EOF\n",
            b"% note\n12 0 obj",
            b"\r\n  7 0\tobj<<>>",
        ] {
            let src = [&b"(abc endobj\n"[..], tail].concat();
            let (t, n) = toks(&src, false);
            assert_eq!(t[0], Tok::LitStr(b"abc ".to_vec()), "{tail:?}");
            assert_eq!(t[1], Tok::Kw(b"endobj"), "{tail:?}");
            assert_eq!(n[0], LexNote::UnterminatedString { at: 0 });
        }
        for src in [
            &b"(a endobj b"[..],
            b"(a endobj (b",
            b"(a endobj 5 0 R b",
            b"(a endobj 5 objx",
        ] {
            let (t, n) = toks(src, false);
            assert_eq!(t, vec![Tok::LitStr(src[1..].to_vec())]);
            assert_eq!(n, vec![LexNote::UnterminatedString { at: 0 }]);
        }
        // Past the lookahead window an `endobj` does not cut.
        let src = [&b"(a endobj"[..], &[b' '; 300], b"5 0 obj"].concat();
        let (t, _) = toks(&src, false);
        assert_eq!(t, vec![Tok::LitStr(src[1..].to_vec())]);
    }

    // Tokenizing never re-reads a long tail: the bytes the string scanner
    // reads stay within a constant factor of the input, however the
    // `endobj`s fall.
    #[test]
    fn string_scanning_is_linear() {
        let scanned = || LIT_SCANNED.with(std::cell::Cell::get);
        let comment_line = [&b"(a endobj %"[..], &b" endobj".repeat(20_000)].concat();
        let cases = [
            b"(a endobj ".repeat(20_000),
            b"(a endobj x ".repeat(20_000),
            b"(a endobj 1 0 obj ".repeat(20_000),
            b"(endobj) ".repeat(20_000),
            comment_line,
        ];
        for (k, src) in cases.iter().enumerate() {
            for cutoff in [true, false] {
                let before = scanned();
                let mut lx = Lexer::new(src, 0).with_keyword_cutoff(cutoff);
                while lx.next() != Tok::Eof {}
                let work = scanned() - before;
                assert!(work <= 2 * src.len(), "case {k}: {work} for {}", src.len());

                let before = scanned();
                let mut pos = 0;
                while pos < src.len() {
                    let mut lx = Lexer::new(src, pos).with_keyword_cutoff(cutoff);
                    match lx.read_value(0) {
                        Err(LexErr::UnexpectedEof) => break,
                        _ => pos = lx.pos,
                    }
                }
                let work = scanned() - before;
                assert!(work <= 2 * src.len(), "case {k}: {work} for {}", src.len());
            }
        }
    }

    // Content streams have no object structure: no cutoff at `endobj`.
    #[test]
    fn keyword_cutoff_off_for_content() {
        let src = b"(a endobj 5 0 obj b";
        let mut lx = Lexer::new(src, 0).with_keyword_cutoff(false);
        assert_eq!(lx.next(), Tok::LitStr(b"a endobj 5 0 obj b".to_vec()));
        assert_eq!(lx.take_notes(), vec![LexNote::UnterminatedString { at: 0 }]);

        let mut lx = Lexer::new(b"<41 endobj>", 0).with_keyword_cutoff(false);
        // `e` is a hex digit; `n` stops decoding but not the string.
        assert_eq!(lx.next(), Tok::HexStr(vec![0x41, 0xe0]));
        assert_eq!(lx.pos, 11);

        let mut lx = Lexer::new(b"[(a) 1] TJ", 0).with_keyword_cutoff(false);
        assert_eq!(
            lx.read_value(0),
            Ok(Object::Array(vec![lit(b"a"), Object::Integer(1)]))
        );
        assert_eq!(lx.next(), Tok::Kw(b"TJ"));
    }

    #[test]
    fn literal_string_escapes_and_balanced_parens() {
        let src = b"(a\\n\\(b\\)\\\\ (c) \\101\\7x\\\r\nd\re\\q)";
        let (t, n) = toks(src, false);
        assert_eq!(t, vec![Tok::LitStr(b"a\n(b)\\ (c) A\x07xd\neq".to_vec())]);
        assert!(n.is_empty());
    }

    // Robustness rule: unterminated `<` (not `<<`) is a hex string to the next
    // `>` or delimiter.
    #[test]
    fn unterminated_hex_stops_at_delimiter() {
        let (t, n) = toks(b"<48 65\n6C /Next", false);
        assert_eq!(
            t,
            vec![
                Tok::HexStr(b"Hel".to_vec()),
                Tok::Name(Cow::Borrowed(b"Next"))
            ]
        );
        assert_eq!(n, vec![LexNote::UnterminatedHex { at: 0 }]);
    }

    #[test]
    fn unterminated_hex_stops_at_eof() {
        let (t, n) = toks(b"<4", false);
        assert_eq!(t, vec![Tok::HexStr(vec![0x40])]);
        assert_eq!(n, vec![LexNote::UnterminatedHex { at: 0 }]);
    }

    #[test]
    fn unterminated_hex_stops_before_endobj() {
        let src = b"<ABCD\nendobj\n5 0 obj\n<< /A 1 >>";
        let (t, n) = toks(src, false);
        assert_eq!(t[0], Tok::HexStr(vec![0xab, 0xcd]));
        assert_eq!(t[1], Tok::Kw(b"endobj"));
        assert_eq!(t[2], Tok::Int(5));
        assert_eq!(n, vec![LexNote::UnterminatedHex { at: 0 }]);

        let src = b"<< /A <ABCD\nendobj\n5 0 obj\n<< /B 1 >>";
        let p = parse(src);
        assert_eq!(&src[p.end..].trim_ascii_start()[..6], b"endobj");
        let d = dict(&p.value);
        assert_eq!(
            get(d, b"A"),
            Some(&Object::String(vec![0xab, 0xcd], StringFormat::Hexadecimal))
        );
        assert!(p.notes.contains(&LexNote::UnterminatedContainer { at: 0 }));
    }

    #[test]
    fn hex_string_stops_decoding_at_a_bad_byte() {
        let (t, n) = toks(b"<4142xz43> 7", false);
        assert_eq!(t, vec![Tok::HexStr(b"AB".to_vec()), Tok::Int(7)]);
        assert_eq!(n, vec![LexNote::BadHexByte { at: 5 }]);
    }

    // Robustness rule: a number with a second sign or point ends at the anomaly;
    // a leading "--" recovers.
    #[test]
    fn double_sign_and_point() {
        let (t, _) = toks(b"--5 1-2 1.2.3 . - +.5", false);
        assert_eq!(
            t,
            vec![
                Tok::Int(-5),
                Tok::Int(1),
                Tok::Int(-2),
                Tok::Real(1.2),
                Tok::Real(0.3),
                Tok::Real(0.0),
                Tok::Int(0),
                Tok::Real(0.5),
            ]
        );
    }

    // Robustness rule: an unknown byte where a token is expected is skipped and
    // `pos` strictly advances.
    #[test]
    fn unknown_bytes_are_skipped_with_progress() {
        let src = b") }{> 5";
        let mut lx = Lexer::new(src, 0);
        assert_eq!(lx.next(), Tok::Int(5));
        assert_eq!(lx.pos, src.len());
        assert_eq!(
            lx.take_notes(),
            vec![
                LexNote::Skipped { at: 0, len: 1 },
                LexNote::Skipped { at: 2, len: 3 },
            ]
        );
        assert_eq!(lx.next(), Tok::Eof);
    }

    #[test]
    fn peek_over_skipped_bytes_leaves_notes_alone() {
        let mut lx = Lexer::new(b") ) 5", 0);
        assert_eq!(lx.peek(), Tok::Int(5));
        assert_eq!(lx.next(), Tok::Int(5));
        assert_eq!(
            lx.take_notes(),
            vec![
                LexNote::Skipped { at: 0, len: 1 },
                LexNote::Skipped { at: 2, len: 1 },
            ]
        );
        let mut lx = Lexer::new(b")(", 0);
        assert_eq!(lx.next(), Tok::LitStr(Vec::new()));
        assert_eq!(
            lx.take_notes(),
            vec![
                LexNote::Skipped { at: 0, len: 1 },
                LexNote::UnterminatedString { at: 1 },
            ]
        );
    }

    #[test]
    fn names_comments_and_delimiters() {
        let (t, n) = toks(b"% c\r/A#20b/ [<<>>]%x\n/C#zz", false);
        assert_eq!(
            t,
            vec![
                Tok::Name(Cow::Owned(b"A b".to_vec())),
                Tok::Name(Cow::Borrowed(b"")),
                Tok::ArrOpen,
                Tok::DictOpen,
                Tok::DictClose,
                Tok::ArrClose,
                Tok::Name(Cow::Borrowed(b"C#zz")),
            ]
        );
        assert!(n.is_empty());
    }

    #[test]
    fn peek_does_not_advance_or_note() {
        let mut lx = Lexer::new(b" (x", 0);
        assert_eq!(lx.peek(), Tok::LitStr(b"x".to_vec()));
        assert_eq!(lx.pos, 0);
        assert!(lx.take_notes().is_empty());
        assert_eq!(lx.next(), Tok::LitStr(b"x".to_vec()));
        assert_eq!(lx.take_notes().len(), 1);
    }

    // Depth guard: more than 100 nested containers is `TooDeep`.
    #[test]
    fn depth_101_is_too_deep() {
        let nest = |n: usize| [vec![b'['; n], vec![b']'; n]].concat();
        assert!(matches!(
            parse_value(&nest(101), 0, 0),
            Err(LexErr::TooDeep)
        ));
        let ok = parse(&nest(100));
        assert_eq!(ok.end, 200);
        let dicts = [b"<</A ".repeat(101), b">>".repeat(101)].concat();
        assert!(matches!(parse_value(&dicts, 0, 0), Err(LexErr::TooDeep)));
        assert!(matches!(parse_value(b"1", 0, 101), Err(LexErr::TooDeep)));
    }

    // Dict recovery: a stray `>>` inside a literal never closes the dict, and a
    // value that fails to parse is skipped to the next key.
    #[test]
    fn stray_dict_close_in_literal() {
        let p = parse(b"<< /A (x >> y) /B 2 >> 9");
        let d = dict(&p.value);
        assert_eq!(get(d, b"A"), Some(&lit(b"x >> y")));
        assert_eq!(get(d, b"B"), Some(&Object::Integer(2)));
        assert_eq!(p.end, 22);
        assert!(p.notes.is_empty());

        let p = parse(b"<< /A foo (z >>) <</Q 1>> /B 2 >>");
        let d = dict(&p.value);
        assert_eq!(get(d, b"A"), None);
        assert_eq!(get(d, b"B"), Some(&Object::Integer(2)));
        assert_eq!(d.len(), 1);
        assert_eq!(p.notes, vec![LexNote::DictRecovered { at: 3 }]);
    }

    #[test]
    fn dict_key_without_value_is_dropped() {
        let p = parse(b"<< /A 1 /B >>");
        let d = dict(&p.value);
        assert_eq!(get(d, b"A"), Some(&Object::Integer(1)));
        assert_eq!(d.len(), 1);
        assert_eq!(p.notes, vec![LexNote::DictRecovered { at: 8 }]);
    }

    #[test]
    fn unterminated_dict_ends_before_endobj() {
        let src = b"<< /A [1 2 /B 3\nendobj";
        let p = parse(src);
        let d = dict(&p.value);
        assert_eq!(arr(get(d, b"A").unwrap()).len(), 4);
        assert_eq!(&src[p.end..].trim_ascii_start()[..6], b"endobj");
        assert!(p.notes.contains(&LexNote::UnterminatedContainer { at: 0 }));
        assert!(p.notes.contains(&LexNote::UnterminatedContainer { at: 6 }));
    }

    // Ref detection: `Int Int R` is a reference, anything else restores.
    #[test]
    fn ref_detection_restores() {
        let p = parse(b"[1 0 R 2 3 4 0 R 5 6 /N 7 -1 R 8]");
        assert_eq!(
            arr(&p.value),
            &[
                Object::Reference((1, 0)),
                Object::Integer(2),
                Object::Integer(3),
                Object::Reference((4, 0)),
                Object::Integer(5),
                Object::Integer(6),
                Object::Name(b"N".to_vec()),
                Object::Integer(7),
                Object::Integer(-1),
                Object::Integer(8),
            ]
        );
        // `R` after a restore is not a value: dropped with a note.
        assert_eq!(p.notes, vec![LexNote::ElementSkipped { at: 29 }]);
        let p = parse(b"12 0");
        assert_eq!(p.value, Object::Integer(12));
        assert_eq!(p.end, 2);
        let p = parse(b"12 0 R");
        assert_eq!(p.value, Object::Reference((12, 0)));
        assert_eq!(p.end, 6);
    }

    // Near-miss: a deleted `j` in a compact `obj<<` leaves the dict intact.
    #[test]
    fn near_miss_deletion_before_a_delimiter() {
        let (t, n) = toks(b"1 0 ob<</A 1>>", true);
        assert_eq!(
            t,
            vec![
                Tok::Int(1),
                Tok::Int(0),
                Tok::Kw(b"obj"),
                Tok::DictOpen,
                Tok::Name(Cow::Borrowed(b"A")),
                Tok::Int(1),
                Tok::DictClose,
            ]
        );
        assert_eq!(n, vec![LexNote::NearMissKeyword { at: 4 }]);
        let (t, _) = toks(b"endobj 2 0 obj(x)endstrem[", true);
        assert_eq!(t[3], Tok::Kw(b"obj"));
        assert_eq!(t[4], Tok::LitStr(b"x".to_vec()));
        assert_eq!(t[5], Tok::Kw(b"endstream"));
        assert_eq!(t[6], Tok::ArrOpen);
        // `>`, `]` and `%` leave the delimiter unread too.
        let (t, n) = toks(b"<</A 1 endob>>ob]endob%c", true);
        assert_eq!(
            t[3..],
            [
                Tok::Kw(b"endobj"),
                Tok::DictClose,
                Tok::Kw(b"obj"),
                Tok::ArrClose,
                Tok::Kw(b"endobj"),
            ]
        );
        assert_eq!(
            n,
            vec![
                LexNote::NearMissKeyword { at: 7 },
                LexNote::NearMissKeyword { at: 14 },
                LexNote::NearMissKeyword { at: 17 },
            ]
        );
    }

    // Near-miss: `115 0 obj` damaged to `11l 0 obj` (RR). The number token is
    // never shortened to 11.
    #[test]
    fn near_miss_number_11l_0_obj() {
        let expect = vec![Tok::Kw(b"11l"), Tok::Int(0), Tok::Kw(b"obj")];
        let (t, n) = toks(b"11l 0 obj", true);
        assert_eq!(t, expect);
        assert_eq!(n, vec![LexNote::NearMissNumber { at: 0 }]);
        let (t, n) = toks(b"11l 0 obj", false);
        assert_eq!(t, expect);
        assert!(n.is_empty());
    }

    #[test]
    fn near_miss_keywords() {
        let (t, n) = toks(b"7 0 obk endob) strean\nendstrXam objects", true);
        assert_eq!(
            t,
            vec![
                Tok::Int(7),
                Tok::Int(0),
                Tok::Kw(b"obj"),
                Tok::Kw(b"endobj"),
                Tok::Kw(b"stream"),
                Tok::Kw(b"endstream"),
                Tok::Kw(b"objects"),
            ]
        );
        assert_eq!(
            n,
            vec![
                LexNote::NearMissKeyword { at: 4 },
                LexNote::NearMissKeyword { at: 8 },
                LexNote::NearMissKeyword { at: 15 },
                LexNote::NearMissKeyword { at: 22 },
            ]
        );
        let (t, n) = toks(b"obk endob) obj", false);
        assert_eq!(t[0], Tok::Kw(b"obk"));
        assert_eq!(t[1], Tok::Kw(b"endob"));
        assert_eq!(t[2], Tok::Kw(b"obj"));
        assert_eq!(n, vec![LexNote::Skipped { at: 9, len: 1 }]);
    }

    // Element cap: 5,000 per array or dictionary.
    #[test]
    fn element_cap() {
        let mut src = b"[".to_vec();
        for i in 0..5_001 {
            src.extend_from_slice(format!("{i} [{i}] ").as_bytes());
        }
        src.extend_from_slice(b"] 42");
        let p = parse(&src);
        assert_eq!(arr(&p.value).len(), 5_000);
        assert_eq!(p.notes, vec![LexNote::ContainerTruncated { at: 0 }]);
        assert_eq!(&src[p.end..], b" 42");

        let mut src = b"<<".to_vec();
        for i in 0..5_001 {
            src.extend_from_slice(format!("/K{i} {i} ").as_bytes());
        }
        src.extend_from_slice(b">>");
        let p = parse(&src);
        assert_eq!(dict(&p.value).len(), 5_000);
        assert_eq!(p.notes, vec![LexNote::ContainerTruncated { at: 0 }]);
        assert_eq!(p.end, src.len());

        let src = [b"[".to_vec(), b"0 ".repeat(5_000), b"]".to_vec()].concat();
        let p = parse(&src);
        assert_eq!(arr(&p.value).len(), 5_000);
        assert!(p.notes.is_empty());
    }

    // Dropped elements and recovered entries count toward the cap, so junk
    // cannot produce more than 5,000 notes per container.
    #[test]
    fn element_cap_counts_dropped_elements() {
        let src = [b"[".to_vec(), b"a ".repeat(20_000), b"] 7".to_vec()].concat();
        let p = parse(&src);
        assert!(arr(&p.value).is_empty());
        assert_eq!(p.notes.len(), 5_001);
        assert_eq!(p.notes[5_000], LexNote::ContainerTruncated { at: 0 });
        assert_eq!(&src[p.end..], b" 7");

        let src = [b"<<".to_vec(), b"/a b ".repeat(20_000), b">> 7".to_vec()].concat();
        let p = parse(&src);
        assert!(dict(&p.value).is_empty());
        assert_eq!(p.notes.len(), 5_001);
        assert_eq!(p.notes[5_000], LexNote::ContainerTruncated { at: 0 });
        assert_eq!(&src[p.end..], b" 7");
    }

    // D-075: a real that does not round-trip through f32 is noted once.
    #[test]
    #[allow(clippy::approx_constant)] // the ticket's figure, not 1/sqrt(2)
    fn real_narrowed_once() {
        let p = parse(b"[0.70710678 0.5]");
        assert_eq!(p.notes, vec![LexNote::RealNarrowed { at: 1 }]);
        assert_eq!(
            arr(&p.value),
            &[Object::Real(0.70710678_f32), Object::Real(0.5)]
        );
    }

    #[test]
    fn scalars() {
        let p = parse(b"  true");
        assert_eq!(p.value, Object::Boolean(true));
        assert_eq!(p.end, 6);
        assert_eq!(parse(b"null").value, Object::Null);
        assert_eq!(
            parse(b"<414>").value,
            Object::String(b"A@".to_vec(), StringFormat::Hexadecimal)
        );
        assert_eq!(
            parse(b"-9223372036854775808").value,
            Object::Integer(i64::MIN)
        );
        assert!(parse(b"-9223372036854775808").notes.is_empty());
        // An integer past i64 becomes a real, noted once even when f32 then
        // rounds it as well.
        for (src, v) in [
            (&b"99999999999999999999"[..], 1e20),
            (b"-9223372036854775809", -9.223_372e18),
            (b"9223372036854775809", 9.223_372e18),
        ] {
            let p = parse(src);
            assert_eq!(p.value, Object::Real(v));
            assert_eq!(p.notes, vec![LexNote::RealNarrowed { at: 0 }]);
        }
        assert!(matches!(
            parse_value(b"  ", 0, 0),
            Err(LexErr::UnexpectedEof)
        ));
        assert!(matches!(
            parse_value(b" endobj", 0, 0),
            Err(LexErr::NotAValue { at: 1 })
        ));
        assert!(matches!(
            parse_value(b"1", 5, 0),
            Err(LexErr::UnexpectedEof)
        ));
    }

    // A number too large for f64 (or f32) clamps to f32::MAX with one note.
    #[test]
    fn huge_numbers_clamp_to_f32_max() {
        let src = vec![b'9'; 400];
        let p = parse(&src);
        assert_eq!(p.value, Object::Real(f32::MAX));
        assert_eq!(p.notes, vec![LexNote::RealNarrowed { at: 0 }]);
        let src = format!("[-1{} 4{}]", "0".repeat(39), "0".repeat(38));
        let p = parse(src.as_bytes());
        assert_eq!(
            arr(&p.value),
            &[Object::Real(-f32::MAX), Object::Real(f32::MAX)]
        );
        assert_eq!(
            p.notes,
            vec![
                LexNote::RealNarrowed { at: 1 },
                LexNote::RealNarrowed { at: 43 }
            ]
        );
    }

    /// xorshift64*: a fixed, platform-independent byte source for the fuzz test.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
    }

    // Property: `pos` strictly advances on every non-Eof token, on any slice.
    #[test]
    fn fuzz_pos_strictly_advances() {
        const ALPHABET: &[u8] = b"()<>[]{}/%\\#+-.0123456789 \n\r\tobjendstreamRtruefalsenull";
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for case in 0..10_000 {
            let len = (rng.next() % 200) as usize;
            let buf: Vec<u8> = (0..len)
                .map(|_| {
                    let r = rng.next();
                    if r.is_multiple_of(4) {
                        (r >> 8) as u8
                    } else {
                        ALPHABET[((r >> 8) as usize) % ALPHABET.len()]
                    }
                })
                .collect();
            for (near_miss, cutoff) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let lexer = || {
                    Lexer::new(&buf, 0)
                        .with_near_miss(near_miss)
                        .with_keyword_cutoff(cutoff)
                };
                let mut lx = lexer();
                let mut steps = 0;
                loop {
                    let before = lx.pos;
                    if lx.peek() == Tok::Eof {
                        assert_eq!(lx.next(), Tok::Eof);
                        break;
                    }
                    lx.next();
                    assert!(lx.pos > before, "case {case}: no progress at {before}");
                    assert!(lx.pos <= buf.len());
                    steps += 1;
                    assert!(steps <= len, "case {case}: too many tokens");
                }
                let mut lx = lexer();
                let mut steps = 0;
                loop {
                    let before = lx.pos;
                    if lx.read_value(0) == Err(LexErr::UnexpectedEof) {
                        break;
                    }
                    assert!(
                        lx.pos > before,
                        "case {case}: no value progress at {before}"
                    );
                    assert!(lx.pos <= buf.len());
                    steps += 1;
                    assert!(steps <= len, "case {case}: too many values");
                }
            }
            let mut pos = 0;
            while pos < buf.len() {
                if let Ok(p) = parse_value(&buf, pos, 0) {
                    assert!(p.end > pos && p.end <= buf.len(), "case {case}");
                }
                pos += 1 + (rng.next() % 16) as usize;
            }
        }
    }
}
