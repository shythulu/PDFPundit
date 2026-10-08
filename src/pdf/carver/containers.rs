//! Object streams and cross-reference streams (T-07 rules 3 and 5; TD §14.4).

use std::borrow::Cow;
use std::collections::BTreeSet;

use lopdf::{Dictionary, Object};

use super::{
    Body, Cancelled, Cap, CarveNote, CarveReport, CarvedObject, Decoding, EntryFault,
    MAX_OBJSTM_ENTRIES, MAX_XREF_ROWS, ObjStmFault, Origin, Poll, TrailerKeys, XrefRow, XrefStream,
    dict_kind, name, range,
};
use crate::pdf::lexer::{self, Lexer, Tok};
use crate::pdf::model::ObjectKind;
use crate::pdf::streams::{self, Filter, InflateStatus};

/// Rule 3: every top-level object stream's entries, each inserted right
/// after its container, so `objects` stays in container-offset order. The
/// whole list, packed objects included, stays within `max_objects`.
pub(super) fn expand_objstms(
    buf: &[u8],
    max_objects: usize,
    decoding: &mut Decoding,
    poll: &mut Poll<'_>,
    report: &mut CarveReport,
) -> Result<(), Cancelled> {
    let top = std::mem::take(&mut report.objects);
    let mut room = max_objects.saturating_sub(top.len());
    let mut capped = false;
    // Packed values are never streams and `/Extends` is not followed, so
    // nothing here recurses; the set makes sure each container, keyed by
    // where it starts, is expanded once.
    let mut visited = BTreeSet::new();
    let mut out = Vec::with_capacity(top.len());
    for mut o in top {
        let members = if is_container(&o) && visited.insert(o.span.start) {
            expand(buf, &mut o, decoding, poll, &mut room, &mut capped)?
        } else {
            Vec::new()
        };
        out.push(o);
        out.extend(members);
    }
    report.objects = out;
    let note = CarveNote::CapHit(Cap::Objects);
    if capped && !report.notes.contains(&note) {
        report.notes.push(note);
    }
    Ok(())
}

fn is_container(o: &CarvedObject) -> bool {
    o.origin == Origin::TopLevel
        && matches!(&o.body, Body::Stream { dict, .. } if name(dict, b"Type") == Some(b"ObjStm"))
}

/// The packed objects of `container`, with its notes added to it. At most
/// `room` are returned; `capped` is set when more were left.
fn expand(
    buf: &[u8],
    container: &mut CarvedObject,
    decoding: &mut Decoding,
    poll: &mut Poll<'_>,
    room: &mut usize,
    capped: &mut bool,
) -> Result<Vec<CarvedObject>, Cancelled> {
    let Body::Stream { dict, data, .. } = &container.body else {
        return Ok(Vec::new());
    };
    let notes = &mut container.notes;
    let n = match dict.get(b"N") {
        Ok(&Object::Integer(n)) if (0..=MAX_OBJSTM_ENTRIES).contains(&n) => n as usize,
        _ => {
            notes.push(CarveNote::ObjStmRejected(ObjStmFault::BadCount));
            return Ok(Vec::new());
        }
    };
    let first = match dict.get(b"First") {
        Ok(&Object::Integer(f)) if f >= 0 => usize::try_from(f).unwrap_or(usize::MAX),
        _ => {
            notes.push(CarveNote::ObjStmRejected(ObjStmFault::BadFirst));
            return Ok(Vec::new());
        }
    };
    let Some((decoded, partial)) = decode_objstm(dict, &buf[range(*data)], decoding) else {
        notes.push(CarveNote::ObjStmRejected(ObjStmFault::Undecodable));
        return Ok(Vec::new());
    };
    if first > decoded.len() {
        notes.push(CarveNote::ObjStmRejected(ObjStmFault::BadFirst));
        return Ok(Vec::new());
    }
    if partial {
        notes.push(CarveNote::ObjStmPartial);
    }
    let pairs = index(&decoded[..first], n);
    let faults = faults(&pairs, decoded.len() - first, container.declared_id.0);
    let good: Vec<usize> = (0..pairs.len()).filter(|&k| faults[k].is_none()).collect();

    let mut members = Vec::new();
    let mut next_good = good.iter().copied().skip(1);
    for (k, fault) in faults.iter().enumerate() {
        poll.tick()?;
        if let Some(fault) = fault {
            notes.push(entry_bad(k, *fault));
            continue;
        }
        let end = next_good
            .next()
            .map_or(decoded.len(), |j| first + pairs[j].1 as usize);
        let start = first + pairs[k].1 as usize;
        let Ok(p) = lexer::parse_value(&decoded[..end], start, 0) else {
            notes.push(entry_bad(k, EntryFault::Unparsed));
            continue;
        };
        if *room == 0 {
            *capped = true;
            break;
        }
        *room -= 1;
        let (body, kind) = match p.value {
            Object::Dictionary(d) => {
                let kind = dict_kind(&d);
                (Body::Dict(d), kind)
            }
            o => (Body::Primitive(o), ObjectKind::Other(String::new())),
        };
        members.push(CarvedObject {
            declared_id: (pairs[k].0 as u32, 0),
            span: container.span,
            body,
            kind,
            origin: Origin::Compressed(container.declared_id),
            notes: p.notes.into_iter().map(CarveNote::Lex).collect(),
        });
    }
    if pairs.len() < n {
        notes.push(entry_bad(pairs.len(), EntryFault::Missing));
    }
    Ok(members)
}

fn entry_bad(index: usize, fault: EntryFault) -> CarveNote {
    CarveNote::ObjStmEntryBad {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        fault,
    }
}

/// Up to `n` `(objnum, offset)` pairs from the index before `/First`; fewer
/// when a token there is not an integer.
fn index(header: &[u8], n: usize) -> Vec<(i64, i64)> {
    let mut lx = Lexer::new(header, 0);
    let mut out = Vec::new();
    while out.len() < n {
        let (Tok::Int(num), Tok::Int(off)) = (lx.next(), lx.next()) else {
            break;
        };
        out.push((num, off));
    }
    out
}

/// Each entry's fault, if any. Out of range first; then, among the in-range
/// offsets in index order, any not strictly between its neighbours; then an
/// entry naming its own container.
fn faults(pairs: &[(i64, i64)], body_len: usize, own: u32) -> Vec<Option<EntryFault>> {
    let mut out: Vec<Option<EntryFault>> = pairs
        .iter()
        .map(|&(num, off)| {
            let ok =
                u32::try_from(num).is_ok() && usize::try_from(off).is_ok_and(|off| off < body_len);
            (!ok).then_some(EntryFault::OutOfRange)
        })
        .collect();
    let in_range: Vec<usize> = (0..pairs.len()).filter(|&k| out[k].is_none()).collect();
    for (i, &k) in in_range.iter().enumerate() {
        let off = pairs[k].1;
        let after_prev = i == 0 || off > pairs[in_range[i - 1]].1;
        let before_next = in_range.get(i + 1).is_none_or(|&j| off < pairs[j].1);
        if !(after_prev && before_next) {
            out[k] = Some(EntryFault::NonMonotonic);
        }
    }
    for (k, &(num, _)) in pairs.iter().enumerate() {
        if out[k].is_none() && num == i64::from(own) {
            out[k] = Some(EntryFault::SelfReference);
        }
    }
    out
}

/// An object stream's decoded data and whether it decoded only in part. A
/// chain ending in a Flate stage without parameters keeps what inflates
/// (a C9 or truncated container still gives the entries before the damage);
/// any other chain must decode whole. `None` when nothing decodes.
fn decode_objstm(
    dict: &Dictionary,
    raw: &[u8],
    decoding: &mut Decoding,
) -> Option<(Vec<u8>, bool)> {
    let chain = streams::filters_of(dict);
    let cap = decoding.cap();
    let out = match chain.split_last() {
        None => (raw.to_vec(), false),
        Some(((Filter::Flate, None), earlier)) => {
            let pre = if earlier.is_empty() {
                Cow::Borrowed(raw)
            } else {
                let pre = streams::decode_chain(raw, earlier, cap).ok()?;
                decoding.spend(pre.len());
                Cow::Owned(pre)
            };
            let r = streams::inflate(&pre, decoding.cap());
            decoding.spend(r.out.len());
            if r.out.is_empty() && r.status != InflateStatus::Done {
                return None;
            }
            (r.out, r.status != InflateStatus::Done)
        }
        Some(_) => {
            let out = streams::decode_chain(raw, &chain, cap).ok()?;
            decoding.spend(out.len());
            (out, false)
        }
    };
    Some(out)
}

/// Rule 5: every top-level `/Type /XRef` stream, its rows and its trailer
/// keys. Undecodable data still gives the keys, and a note on the object.
pub(super) fn xref_streams(buf: &[u8], decoding: &mut Decoding, report: &mut CarveReport) {
    let mut rows_left = MAX_XREF_ROWS;
    let mut capped = false;
    let mut found = Vec::new();
    for o in report.objects.iter_mut() {
        let Body::Stream { dict, data, .. } = &o.body else {
            continue;
        };
        if o.origin != Origin::TopLevel || name(dict, b"Type") != Some(b"XRef") {
            continue;
        }
        let key = |k: &[u8]| dict.get(k).ok().cloned();
        let trailer = TrailerKeys {
            root: key(b"Root"),
            info: key(b"Info"),
            id: key(b"ID"),
            encrypt: key(b"Encrypt"),
        };
        let chain = streams::filters_of(dict);
        let decoded = streams::decode_chain(&buf[range(*data)], &chain, decoding.cap()).ok();
        let rows = match decoded.and_then(|d| {
            decoding.spend(d.len());
            rows(dict, &d, &mut rows_left, &mut capped)
        }) {
            Some(rows) => rows,
            None => {
                o.notes.push(CarveNote::XrefStreamUndecodable);
                Vec::new()
            }
        };
        found.push(XrefStream {
            id: o.declared_id,
            span: o.span,
            rows,
            trailer,
        });
    }
    report.xref_streams = found;
    if capped {
        report.notes.push(CarveNote::CapHit(Cap::XrefRows));
    }
}

/// The rows of decoded xref-stream `data` by `/W` and `/Index` (default
/// `[0 /Size]`). `None` when `/W` is not three integers from 0 to 8 that
/// add up to at least 1. Rows stop at the end of the data or when
/// `rows_left` runs out (`capped`).
fn rows(
    dict: &Dictionary,
    data: &[u8],
    rows_left: &mut usize,
    capped: &mut bool,
) -> Option<Vec<XrefRow>> {
    let ints = |key: &[u8]| -> Option<Vec<i64>> {
        let arr = dict.get(key).ok()?.as_array().ok()?;
        arr.iter().map(|o| o.as_i64().ok()).collect()
    };
    let w = ints(b"W")?;
    let w: [usize; 3] = match w.as_slice() {
        &[a, b, c] if [a, b, c].iter().all(|v| (0..=8).contains(v)) => {
            [a as usize, b as usize, c as usize]
        }
        _ => return None,
    };
    let width: usize = w.iter().sum();
    if width == 0 {
        return None;
    }
    let size = dict
        .get(b"Size")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(u64::MAX);
    let sections: Vec<(u64, u64)> = match ints(b"Index") {
        Some(v) if v.len() % 2 == 0 && v.iter().all(|&x| x >= 0) => {
            v.chunks(2).map(|p| (p[0] as u64, p[1] as u64)).collect()
        }
        _ => vec![(0, size)],
    };
    let mut out = Vec::new();
    let mut chunks = data.chunks_exact(width);
    'sections: for (start, count) in sections {
        for i in 0..count {
            let Some(row) = chunks.next() else {
                break 'sections;
            };
            if *rows_left == 0 {
                *capped = true;
                break 'sections;
            }
            *rows_left -= 1;
            let mut fields = [0u64; 3];
            let mut at = 0;
            for (f, &len) in w.iter().enumerate() {
                fields[f] = row[at..at + len]
                    .iter()
                    .fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
                at += len;
            }
            if w[0] == 0 {
                fields[0] = 1;
            }
            out.push(XrefRow {
                num: start.saturating_add(i),
                fields,
            });
        }
    }
    Some(out)
}
