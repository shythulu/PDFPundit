//! Phase A of the carve (TD §14.1): one pass over the file that finds every
//! keyword the assembly needs, in byte order.

use memchr::memmem;

use super::{Cancelled, Poll};
use crate::pdf::lexer::{is_reg, is_ws};

/// How far back from `obj` a header may start (rule 1). ISO 32000-1 Annex C
/// caps object numbers at 8,388,607, so a legal header is at most 17 bytes;
/// 24 is slack.
pub(super) const HEADER_WALK_BACK: usize = 24;

/// One `num gen obj` header. `at` is the first digit of `num`; `obj_end` is
/// just past the `obj` keyword, where the value starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Header {
    pub(super) at: usize,
    pub(super) obj_end: usize,
    pub(super) num: u32,
    pub(super) gen_nr: u16,
}

/// Every landmark, one sorted list per kind. Offsets are where the keyword
/// starts (for a header, its first digit).
#[derive(Debug, Default)]
pub(super) struct Landmarks {
    pub(super) headers: Vec<Header>,
    /// Headers whose numbers overflow `u32` or whose generation exceeds 65,535.
    pub(super) bad_headers: Vec<usize>,
    pub(super) endobjs: Vec<usize>,
    pub(super) endstreams: Vec<usize>,
    pub(super) xrefs: Vec<usize>,
    pub(super) startxrefs: Vec<usize>,
    pub(super) trailers: Vec<usize>,
    pub(super) eofs: Vec<usize>,
    /// Landmarks collected, over every kind.
    pub(super) count: usize,
    /// More landmarks followed the last one kept.
    pub(super) capped: bool,
}

/// What one keyword hit turned out to be.
enum Hit {
    Header(Header),
    BadHeader(usize),
    EndObj(usize),
    EndStream(usize),
    Xref(usize),
    StartXref(usize),
    Trailer(usize),
    Eof(usize),
}

/// The keywords searched for, one finder each. `endobj` is found through
/// `obj` (its tail) and `startxref` through `xref`.
const KEYWORDS: [&[u8]; 5] = [b"obj", b"endstream", b"xref", b"trailer", b"%%EOF"];

/// Scans `buf` for landmarks, keeping at most `cap`. Every keyword hit is a
/// step of `poll`, so cancellation is seen at least every 256 landmarks.
pub(super) fn scan(buf: &[u8], cap: usize, poll: &mut Poll<'_>) -> Result<Landmarks, Cancelled> {
    let mut finders: Vec<_> = KEYWORDS
        .iter()
        .map(|kw| memmem::find_iter(buf, kw).peekable())
        .collect();
    let mut out = Landmarks::default();
    loop {
        // The earliest pending hit over all finders: hits come in byte order,
        // and no landmark's bytes hold another keyword, so landmarks do too.
        let mut next: Option<(usize, usize)> = None;
        for (k, f) in finders.iter_mut().enumerate() {
            if let Some(&p) = f.peek()
                && next.is_none_or(|(_, q)| p < q)
            {
                next = Some((k, p));
            }
        }
        let Some((k, p)) = next else { break };
        finders[k].next();
        poll.tick()?;
        let Some(hit) = classify(buf, k, p) else {
            continue;
        };
        if out.count == cap {
            out.capped = true;
            break;
        }
        out.count += 1;
        match hit {
            Hit::Header(h) => out.headers.push(h),
            Hit::BadHeader(at) => out.bad_headers.push(at),
            Hit::EndObj(at) => out.endobjs.push(at),
            Hit::EndStream(at) => out.endstreams.push(at),
            Hit::Xref(at) => out.xrefs.push(at),
            Hit::StartXref(at) => out.startxrefs.push(at),
            Hit::Trailer(at) => out.trailers.push(at),
            Hit::Eof(at) => out.eofs.push(at),
        }
    }
    Ok(out)
}

/// What the hit of `KEYWORDS[k]` at `p` is, if it is a landmark at all.
fn classify(buf: &[u8], k: usize, p: usize) -> Option<Hit> {
    let ends_token = |end: usize| buf.get(end).is_none_or(|&b| !is_reg(b));
    let starts_token = |at: usize| at == 0 || !is_reg(buf[at - 1]);
    match k {
        0 => {
            if !ends_token(p + 3) {
                return None;
            }
            if p >= 3 && &buf[p - 3..p] == b"end" {
                return Some(Hit::EndObj(p - 3));
            }
            header(buf, p)
        }
        // `endstream` is matched loosely: the extent ladder checks how it is
        // framed.
        1 => Some(Hit::EndStream(p)),
        2 => {
            if !ends_token(p + 4) {
                return None;
            }
            if p >= 5 && &buf[p - 5..p] == b"start" && starts_token(p - 5) {
                Some(Hit::StartXref(p - 5))
            } else if starts_token(p) {
                Some(Hit::Xref(p))
            } else {
                None
            }
        }
        3 => (starts_token(p) && ends_token(p + 7)).then_some(Hit::Trailer(p)),
        _ => Some(Hit::Eof(p)),
    }
}

/// Rule 1: `num gen obj` with `num` and `gen` decimal digit runs, one
/// whitespace byte between `num` and `gen` and between `gen` and `obj`, the
/// header at the start of the buffer or after whitespace, walking back at
/// most [`HEADER_WALK_BACK`] bytes from `obj` (at `p`).
fn header(buf: &[u8], p: usize) -> Option<Hit> {
    let lo = p.saturating_sub(HEADER_WALK_BACK);
    let digits_back = |end: usize| {
        let mut i = end;
        while i > lo && buf[i - 1].is_ascii_digit() {
            i -= 1;
        }
        i
    };
    // The digit run reaches the window's edge and goes on past it: the
    // number cannot be assembled in 24 bytes, so it is out of range.
    let cut = |start: usize| start == lo && start > 0 && buf[start - 1].is_ascii_digit();
    if p == lo || !is_ws(buf[p - 1]) {
        return None;
    }
    let gen_end = p - 1;
    let gen_start = digits_back(gen_end);
    if gen_start == gen_end {
        return None;
    }
    if cut(gen_start) {
        return Some(Hit::BadHeader(lo));
    }
    if gen_start == lo || !is_ws(buf[gen_start - 1]) {
        return None;
    }
    let num_end = gen_start - 1;
    let num_start = digits_back(num_end);
    if num_start == num_end {
        return None;
    }
    if cut(num_start) {
        return Some(Hit::BadHeader(lo));
    }
    if num_start > 0 && !is_ws(buf[num_start - 1]) {
        return None;
    }
    let num = decimal(&buf[num_start..num_end]).and_then(|n| u32::try_from(n).ok());
    let gen_nr = decimal(&buf[gen_start..gen_end]).and_then(|g| u16::try_from(g).ok());
    match (num, gen_nr) {
        (Some(num), Some(gen_nr)) => Some(Hit::Header(Header {
            at: num_start,
            obj_end: p + 3,
            num,
            gen_nr,
        })),
        _ => Some(Hit::BadHeader(num_start)),
    }
}

/// The value of a run of ASCII digits; `None` past `u64`.
fn decimal(digits: &[u8]) -> Option<u64> {
    digits.iter().try_fold(0u64, |acc, &d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })
}
