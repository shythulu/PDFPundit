//! The gap sweep (T-07 rule 6; TD §14.5): what lies between the objects.

use std::ops::Range;

use lopdf::{Dictionary, Object};

use super::{
    Cancelled, Cap, CarveNote, CarveReport, Classifier, Ladder, LengthSource, Orphan, Poll,
    dict_kind, range, region_end, span,
};
use crate::pdf::lexer::{self, Lexer, Tok, is_reg, is_ws};

/// A gap is read only when it holds at least this many bytes that are
/// neither whitespace nor comments.
const MIN_SOLID: usize = 16;

/// Finds the orphans and unexplained spans in every gap of the carve: the
/// runs outside objects, xref tables, trailers and `startxref` values.
/// Orphans count toward `max_objects` with the objects already carved;
/// the sweep stops at the cap with [`CarveNote::CapHit`]. `poll` ticks once
/// per gap and once per orphan tried, so a gap of many orphans still sees a
/// cancel.
pub(super) fn sweep(
    ladder: &mut Ladder<'_>,
    classify: &mut Classifier,
    max_objects: usize,
    poll: &mut Poll<'_>,
    report: &mut CarveReport,
) -> Result<(), Cancelled> {
    let buf = ladder.buf;
    let mut room = max_objects.saturating_sub(report.objects.len());
    for gap in gaps(ladder, report) {
        poll.tick()?;
        let mut pos = gap.start;
        loop {
            pos = skip_blank(buf, pos, gap.end);
            if !solid_at_least(buf, pos, gap.end, MIN_SOLID) {
                break;
            }
            poll.tick()?;
            let rungs = ladder.rungs;
            match orphan_at(ladder, classify, pos, gap.end) {
                Some(_) if room == 0 => {
                    ladder.rungs = rungs;
                    let note = CarveNote::CapHit(Cap::Objects);
                    if !report.notes.contains(&note) {
                        report.notes.push(note);
                    }
                    return Ok(());
                }
                Some(orphan) => {
                    room -= 1;
                    pos = orphan.span().end as usize;
                    report.orphans.push(orphan);
                }
                None => {
                    let span = span(pos..gap.end);
                    report.notes.push(CarveNote::UnexplainedSpan { span });
                    break;
                }
            }
        }
    }
    Ok(())
}

/// The maximal runs of the file no object, xref table, trailer or
/// `startxref` value covers, in byte order.
fn gaps(ladder: &Ladder<'_>, report: &CarveReport) -> Vec<Range<usize>> {
    let (buf, lm) = (ladder.buf, ladder.lm);
    let mut covered: Vec<Range<usize>> = report
        .objects
        .iter()
        .map(|o| range(o.span))
        .chain(report.xref_spans.iter().map(|s| range(*s)))
        .chain(report.trailer_spans.iter().map(|s| range(*s)))
        .chain(report.startxref.iter().map(|&(at, _)| {
            let at = at as usize;
            let window = &buf[..region_end(buf, lm, at)];
            let mut lx = Lexer::new(window, at + b"startxref".len());
            match lx.next() {
                Tok::Int(_) => at..lx.pos,
                _ => at..at + b"startxref".len(),
            }
        }))
        .collect();
    covered.sort_unstable_by_key(|r| (r.start, r.end));
    let mut out = Vec::new();
    let mut pos = 0;
    for r in covered {
        if r.start > pos {
            out.push(pos..r.start);
        }
        pos = pos.max(r.end);
    }
    if pos < buf.len() {
        out.push(pos..buf.len());
    }
    out
}

/// `pos` moved past whitespace and comments, never past `end`.
fn skip_blank(buf: &[u8], mut pos: usize, end: usize) -> usize {
    while pos < end {
        match buf[pos] {
            b'%' => {
                while pos < end && !matches!(buf[pos], b'\r' | b'\n') {
                    pos += 1;
                }
            }
            b if is_ws(b) => pos += 1,
            _ => break,
        }
    }
    pos
}

/// Whether `buf[pos..end]` holds `n` bytes that are neither whitespace nor
/// comments. Stops counting at `n`, so the sweep stays linear.
fn solid_at_least(buf: &[u8], mut pos: usize, end: usize, n: usize) -> bool {
    let mut seen = 0;
    loop {
        pos = skip_blank(buf, pos, end);
        if pos >= end {
            return false;
        }
        seen += 1;
        if seen == n {
            return true;
        }
        pos += 1;
    }
}

/// The orphan starting at `pos` (its first solid byte), read no further
/// than `end`: a dictionary, a dictionary with a stream, or a dictionary-less
/// stream whose data ends at `endstream` or a complete inflate.
fn orphan_at(
    ladder: &mut Ladder<'_>,
    classify: &mut Classifier,
    pos: usize,
    end: usize,
) -> Option<Orphan> {
    let buf = ladder.buf;
    let window = &buf[..end];
    // A dictionary does not run past `endstream` or `endobj`: one that
    // never closes leaves the orphans after it alone.
    let dict_window = &buf[..dict_end(ladder, pos, end)];
    if window[pos..].starts_with(b"<<")
        && let Ok(p) = lexer::parse_value(dict_window, pos, 0)
        && let Object::Dictionary(dict) = p.value
    {
        let mut lx = Lexer::new(dict_window, p.end);
        if lx.next() == Tok::Kw(&b"stream"[..]) {
            return Some(stream_orphan(ladder, classify, pos, dict, lx.pos, end));
        }
        let kind = dict_kind(&dict);
        return Some(Orphan::Dict {
            span: span(pos..past_endobj(buf, p.end, end)),
            dict,
            kind,
        });
    }
    let kw = stream_keyword(&window[pos..])? + pos;
    // Only a stream that ends is an orphan; a rejected try counts no rung.
    let rungs = ladder.rungs;
    let orphan = stream_orphan(
        ladder,
        classify,
        pos,
        Dictionary::new(),
        kw + b"stream".len(),
        end,
    );
    match &orphan {
        Orphan::Stream {
            length_source: LengthSource::ScannedEndstream | LengthSource::InflateProbe,
            ..
        } => Some(orphan),
        _ => {
            ladder.rungs = rungs;
            None
        }
    }
}

/// Where a dictionary from `pos` must end: the first `endstream` or
/// `endobj` from there, or `end`.
fn dict_end(ladder: &Ladder<'_>, pos: usize, end: usize) -> usize {
    let first = |list: &[usize]| list.get(list.partition_point(|&at| at < pos)).copied();
    [first(&ladder.lm.endstreams), first(&ladder.lm.endobjs)]
        .into_iter()
        .flatten()
        .fold(end, usize::min)
}

/// The stream orphan from `pos` whose `stream` keyword ends at `kw_end`.
fn stream_orphan(
    ladder: &mut Ladder<'_>,
    classify: &mut Classifier,
    pos: usize,
    dict: Dictionary,
    kw_end: usize,
    end: usize,
) -> Orphan {
    let buf = ladder.buf;
    let ext = ladder.extent(&dict, kw_end, end, &mut Vec::new());
    let kind = classify.kind(&dict, &buf[ext.data.clone()]);
    Orphan::Stream {
        span: span(pos..past_endobj(buf, ext.resume, end)),
        dict,
        data: span(ext.data),
        length_source: ext.source,
        kind,
    }
}

/// Just past an `endobj` that follows `from` after optional whitespace,
/// within `end`; else `from`.
fn past_endobj(buf: &[u8], from: usize, end: usize) -> usize {
    let from = from.min(end);
    let ws = buf[from..end].iter().take_while(|&&b| is_ws(b)).count();
    if buf[from + ws..end].starts_with(b"endobj") {
        from + ws + b"endobj".len()
    } else {
        from
    }
}

/// Where the first `stream` keyword in `bytes` starts: a token on its own,
/// so never the tail of `endstream`.
fn stream_keyword(bytes: &[u8]) -> Option<usize> {
    memchr::memmem::find_iter(bytes, b"stream").find(|&at| {
        let starts = at == 0 || !is_reg(bytes[at - 1]);
        let ends = bytes.get(at + 6).is_none_or(|&b| !is_reg(b));
        starts && ends
    })
}
