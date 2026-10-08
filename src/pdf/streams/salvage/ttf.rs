//! The TrueType-checksum localizer for C9 font streams (T-08b; D-056,
//! FR-r1-e Part 2).
//!
//! An embedded font whose stream decodes to its end with a bad Adler-32 still
//! carries its sfnt table directory, and every table there has a checksum.
//! The first byte the damage changed lies before the end of the failing table
//! with the lowest offset (T1) and inside no table that verified, so the
//! search window is output `[0, end(T1))` minus every verified table, mapped
//! to input positions through the byte-wise trace (the complete window; with
//! no failing table, the gaps between tables). A candidate past the directory
//! is decoded to `end(T1)` first and rejected when T1's checksum still fails
//! (verifier B), summing only the words past the candidate's checkpoint
//! against the damaged output's running sums. A font's own checksums can be
//! wrong, so when the window gives no accept the ladder widens the search
//! under the Adler-32 alone: the positions B judged, then the rest of its
//! own window.
//!
//! The directory format is ISO/IEC 14496-22 §4.5; the checksum is the
//! published sum of big-endian 32-bit words, zero-padded, with `head`'s
//! `checkSumAdjustment` counted as zero. No glyph data is read.

use std::ops::Range;
use std::sync::Arc;

use super::{ADLER_FROM, Check, InputTrace, Localizer, Window};

#[cfg(test)]
mod tests;

/// The Adler-32 trailer's length: the ladder's trailer path covers it.
const TRAILER: usize = 4;
/// The table directory starts after the 12-byte offset subtable ...
const DIR_HEAD: usize = 12;
/// ... and has one 16-byte record per table.
const RECORD: usize = 16;

/// The C9 localizer for sfnt font streams (TrueType or CFF-flavoured).
pub(crate) struct TtfLocalizer;

impl Localizer for TtfLocalizer {
    /// Applies when the damage shows only in the Adler-32 and the damaged
    /// output begins with an sfnt directory that parses.
    fn window(&self, baseline_out: &[u8], trace: &InputTrace) -> Option<Window> {
        if !trace.decoded_to_end {
            return None;
        }
        let dir = Directory::parse(baseline_out)?;
        let (verified, failing): (Vec<&Table>, Vec<&Table>) = dir
            .tables
            .iter()
            .partition(|t| t.verifies(baseline_out, dir.end));
        // The first of the lowest offset, so a tie keeps directory order.
        let t1 = failing.iter().min_by_key(|t| t.offset);
        let end = t1.map_or(baseline_out.len(), |t| t.end().min(baseline_out.len()));
        let mut skip: Vec<Range<usize>> = verified.iter().map(|t| t.offset..t.end()).collect();
        skip.sort_by_key(|r| (r.start, r.end));
        let mut gaps = Vec::new();
        let mut at = 0;
        for r in skip {
            if r.start >= end {
                break;
            }
            if r.start > at {
                gaps.push(at..r.start);
            }
            at = at.max(r.end);
        }
        if at < end {
            gaps.push(at..end);
        }
        let map = Map(trace.out_after());
        let ranges = map.inputs(&gaps);
        if ranges.is_empty() {
            return None;
        }
        // B needs T1 whole in the output and past the directory; positions
        // whose decode can still change the directory are judged by A.
        let check = t1
            .filter(|t| t.len > 0 && t.offset >= dir.end && t.end() <= baseline_out.len())
            .map(|t| Check {
                at_out: t.end(),
                from: map.producer(dir.end) + 1,
                memo: Arc::new(T1Sums::of(**t, baseline_out)),
            });
        // Every failing table assumes the original's checksums were right.
        Some(Window {
            ranges,
            check,
            widen: t1.is_some(),
        })
    }

    /// Rejects the candidate when T1 fails its checksum. Only the words at
    /// or past `same_to` are summed: the ones before are the damaged
    /// output's, whose running sums the check carries. A candidate is judged
    /// only past the directory, so T1's record is the damaged output's.
    fn early_reject(
        &self,
        check: &Check,
        candidate_out_prefix: &[u8],
        same_to: usize,
    ) -> Option<bool> {
        let m = check.memo.downcast_ref::<T1Sums>()?;
        let t = m.table;
        let bytes = candidate_out_prefix.get(t.offset..t.end())?;
        let first = (same_to.saturating_sub(t.offset) / 4).min(m.sums.len() - 1);
        let sum = m.sums[first].wrapping_add(word_sum(&t.tag, bytes, first));
        Some(sum != t.checksum)
    }
}

/// T1 and the damaged output's running sums over it: `sums[i]` is the
/// checksum of T1's first `i` whole words.
#[derive(Debug)]
pub(super) struct T1Sums {
    pub table: Table,
    pub sums: Vec<u32>,
}

impl T1Sums {
    /// `font` holds `table` whole.
    pub(super) fn of(table: Table, font: &[u8]) -> T1Sums {
        let (words, _) = font[table.offset..table.end()].as_chunks::<4>();
        let mut sums = Vec::with_capacity(words.len() + 1);
        let mut sum = 0u32;
        sums.push(sum);
        for (i, w) in words.iter().enumerate() {
            if !is_adjustment(&table.tag, i) {
                sum = sum.wrapping_add(u32::from_be_bytes(*w));
            }
            sums.push(sum);
        }
        T1Sums { table, sums }
    }
}

/// One table record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Table {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub offset: usize,
    pub len: usize,
}

impl Table {
    fn end(&self) -> usize {
        self.offset.saturating_add(self.len)
    }

    /// The table lies past the directory, wholly inside `font`, and sums to
    /// its recorded checksum.
    pub(super) fn verifies(&self, font: &[u8], dir_end: usize) -> bool {
        self.offset >= dir_end
            && font
                .get(self.offset..self.end())
                .is_some_and(|b| checksum(&self.tag, b) == self.checksum)
    }
}

/// An sfnt table directory: its records and where it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Directory {
    pub tables: Vec<Table>,
    pub end: usize,
}

impl Directory {
    /// The directory at the start of `font`: `None` unless the sfnt version
    /// is `00 01 00 00`, `true` or `OTTO`, there is at least one table, and
    /// every record is present.
    pub(super) fn parse(font: &[u8]) -> Option<Directory> {
        if !matches!(font.get(..4)?, [0, 1, 0, 0] | b"true" | b"OTTO") {
            return None;
        }
        let n = usize::from(u16::from_be_bytes([*font.get(4)?, *font.get(5)?]));
        let end = DIR_HEAD + RECORD * n;
        let records = font.get(DIR_HEAD..end).filter(|_| n > 0)?;
        let tables = records
            .as_chunks::<RECORD>()
            .0
            .iter()
            .map(|r| Table {
                tag: [r[0], r[1], r[2], r[3]],
                checksum: be32(&r[4..8]),
                offset: be32(&r[8..12]) as usize,
                len: be32(&r[12..16]) as usize,
            })
            .collect();
        Some(Directory { tables, end })
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// A table's checksum: the wrapping sum of its big-endian 32-bit words, the
/// last one zero-padded. In `head` the third word (`checkSumAdjustment`)
/// counts as zero.
pub(super) fn checksum(tag: &[u8; 4], table: &[u8]) -> u32 {
    word_sum(tag, table, 0)
}

/// [`checksum`]'s sum over the words from whole word `first` on (the
/// zero-padded last word included).
fn word_sum(tag: &[u8; 4], table: &[u8], first: usize) -> u32 {
    let (words, rest) = table.as_chunks::<4>();
    let mut sum = 0u32;
    for (i, w) in words.iter().enumerate().skip(first) {
        if !is_adjustment(tag, i) {
            sum = sum.wrapping_add(u32::from_be_bytes(*w));
        }
    }
    let mut last = [0u8; 4];
    last[..rest.len()].copy_from_slice(rest);
    sum.wrapping_add(u32::from_be_bytes(last))
}

/// Whole word `i` of a `tag` table is `head`'s `checkSumAdjustment`.
fn is_adjustment(tag: &[u8; 4], i: usize) -> bool {
    i == 2 && tag == b"head"
}

/// Output offsets to input positions, through the byte-wise trace.
struct Map<'a>(&'a [usize]);

impl Map<'_> {
    /// The last input byte after which at most `x` output bytes exist (0 for
    /// `x = 0`): the earliest byte whose change can reach output `x`.
    fn earliest(&self, x: usize) -> usize {
        if x == 0 {
            return 0;
        }
        self.0.partition_point(|&o| o <= x).saturating_sub(1)
    }

    /// The input byte whose consumption first gives `y` output bytes.
    fn producer(&self, y: usize) -> usize {
        self.0
            .partition_point(|&o| o < y)
            .min(self.0.len().saturating_sub(1))
    }

    /// The input positions of output intervals, margin 0, without the zlib
    /// header and the Adler-32 trailer: disjoint ranges, highest first, so
    /// the search runs from the top of the window down.
    fn inputs(&self, outs: &[Range<usize>]) -> Vec<Range<usize>> {
        let (lo, hi) = (ADLER_FROM, self.0.len().saturating_sub(TRAILER));
        let mut ins: Vec<Range<usize>> = outs
            .iter()
            .filter(|r| r.end > r.start)
            .map(|r| self.earliest(r.start).max(lo)..(self.producer(r.end) + 1).min(hi))
            .filter(|r| r.end > r.start)
            .collect();
        ins.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in ins {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        merged.reverse();
        merged
    }
}
