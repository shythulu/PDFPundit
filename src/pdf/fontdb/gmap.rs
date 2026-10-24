//! Glyph maps (T-27b, TD §18.1): the `.gmap` sidecar of a bundled font.
//!
//! A `.gmap` is a flat array of 9-byte little-endian records
//! `gid:u16, unicode:u32, width:u16, source:u8`, sorted by
//! `(gid, source, unicode)` with no repeated record. `width` is the glyph's
//! advance in PDF glyph space (1000 units per em) and `source` says where the
//! pairing came from ([`Source`]). Every field is a byte array, so the record
//! has alignment 1, [`GmapTable`] views the file in place through `bytemuck`,
//! and the layout is the same on every platform.
//!
//! The sort order puts a glyph's `cmap` entries before its shaping-derived ones
//! and its lowest code point first, so [`GmapTable::unicode`] has one
//! deterministic answer for a glyph that several code points share (space and
//! no-break space, say).

// T-28 (scorer) and T-29 (templates) are the first callers outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use bytemuck::{Pod, Zeroable};

/// The size of one record in bytes.
pub(crate) const RECORD_LEN: usize = 9;

/// Where a record's glyph ↔ code point pairing came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    /// A `cmap` subtable maps the code point to the glyph.
    Cmap = 0,
    /// Shaping produced the glyph (a ligature): the code point is the
    /// ligature's Unicode presentation form.
    Shaped = 1,
}

impl Source {
    fn from_byte(b: u8) -> Option<Source> {
        match b {
            0 => Some(Source::Cmap),
            1 => Some(Source::Shaped),
            _ => None,
        }
    }
}

/// One `.gmap` record, as it lies in the file.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct GmapRecord {
    gid: [u8; 2],
    unicode: [u8; 4],
    width: [u8; 2],
    source: u8,
}

const _: () = assert!(size_of::<GmapRecord>() == RECORD_LEN);

impl GmapRecord {
    pub(crate) fn new(gid: u16, unicode: char, width: u16, source: Source) -> GmapRecord {
        GmapRecord {
            gid: gid.to_le_bytes(),
            unicode: u32::from(unicode).to_le_bytes(),
            width: width.to_le_bytes(),
            source: source as u8,
        }
    }

    pub(crate) fn gid(&self) -> u16 {
        u16::from_le_bytes(self.gid)
    }

    /// The code point. [`GmapTable::new`] has checked it is a scalar value.
    pub(crate) fn unicode(&self) -> char {
        char::from_u32(u32::from_le_bytes(self.unicode)).unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// The advance width, 1000 units per em.
    pub(crate) fn width(&self) -> u16 {
        u16::from_le_bytes(self.width)
    }

    /// [`GmapTable::new`] has checked the byte is a known source.
    pub(crate) fn source(&self) -> Source {
        Source::from_byte(self.source).unwrap_or(Source::Shaped)
    }

    /// The order a `.gmap` keeps its records in.
    pub(crate) fn sort_key(&self) -> (u16, u8, u32) {
        (self.gid(), self.source, u32::from_le_bytes(self.unicode))
    }
}

impl std::fmt::Debug for GmapRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GmapRecord")
            .field("gid", &self.gid())
            .field("unicode", &u32::from_le_bytes(self.unicode))
            .field("width", &self.width())
            .field("source", &self.source)
            .finish()
    }
}

/// Why bytes are not a `.gmap`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum GmapError {
    #[error("gmap length {0} is not a multiple of {RECORD_LEN}")]
    BadLength(usize),
    #[error("gmap record {0} has a code point that is not a Unicode scalar value")]
    BadUnicode(usize),
    #[error("gmap record {0} has an unknown source byte")]
    BadSource(usize),
    #[error("gmap record {0} is out of order or repeats the one before it")]
    Unsorted(usize),
}

/// A checked `.gmap`, viewed in place.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GmapTable<'a> {
    records: &'a [GmapRecord],
}

impl<'a> GmapTable<'a> {
    /// Views `bytes` as records after checking the length, every code point,
    /// every source byte and the order.
    pub(crate) fn new(bytes: &'a [u8]) -> Result<GmapTable<'a>, GmapError> {
        let records: &[GmapRecord] =
            bytemuck::try_cast_slice(bytes).map_err(|_| GmapError::BadLength(bytes.len()))?;
        for (i, r) in records.iter().enumerate() {
            if char::from_u32(u32::from_le_bytes(r.unicode)).is_none() {
                return Err(GmapError::BadUnicode(i));
            }
            if Source::from_byte(r.source).is_none() {
                return Err(GmapError::BadSource(i));
            }
            if i > 0 && records[i - 1].sort_key() >= r.sort_key() {
                return Err(GmapError::Unsorted(i));
            }
        }
        Ok(GmapTable { records })
    }

    pub(crate) fn records(&self) -> &'a [GmapRecord] {
        self.records
    }

    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Every record for `gid`, in file order.
    pub(crate) fn records_of(&self, gid: u16) -> &'a [GmapRecord] {
        let start = self.records.partition_point(|r| r.gid() < gid);
        let len = self.records[start..].partition_point(|r| r.gid() == gid);
        &self.records[start..start + len]
    }

    /// The code point `gid` stands for: its first record's, so a `cmap`
    /// pairing before a shaped one and the lowest code point first.
    pub(crate) fn unicode(&self, gid: u16) -> Option<char> {
        self.records_of(gid).first().map(GmapRecord::unicode)
    }

    /// `gid`'s advance width, 1000 units per em.
    pub(crate) fn width(&self, gid: u16) -> Option<u16> {
        self.records_of(gid).first().map(GmapRecord::width)
    }
}

/// The `.gmap` bytes of `records`, sorted and with repeats dropped.
pub(crate) fn encode(mut records: Vec<GmapRecord>) -> Vec<u8> {
    records.sort_by_key(GmapRecord::sort_key);
    records.dedup_by_key(|r| r.sort_key());
    bytemuck::cast_slice(&records).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<GmapRecord> {
        vec![
            GmapRecord::new(3, ' ', 260, Source::Cmap),
            GmapRecord::new(3, '\u{a0}', 260, Source::Cmap),
            GmapRecord::new(36, 'A', 639, Source::Cmap),
            GmapRecord::new(400, '\u{fb01}', 557, Source::Shaped),
            GmapRecord::new(400, '\u{fb01}', 557, Source::Cmap),
            GmapRecord::new(65535, '\u{10ffff}', 65535, Source::Cmap),
        ]
    }

    #[test]
    fn records_are_nine_little_endian_bytes() {
        let bytes = encode(vec![GmapRecord::new(
            0x0102,
            '\u{1f600}',
            0x0304,
            Source::Shaped,
        )]);
        assert_eq!(
            bytes,
            [0x02, 0x01, 0x00, 0xf6, 0x01, 0x00, 0x04, 0x03, 0x01]
        );
    }

    #[test]
    fn gmap_round_trips() {
        let bytes = encode(sample());
        assert_eq!(bytes.len(), 6 * RECORD_LEN);
        let table = GmapTable::new(&bytes).expect("valid gmap");
        assert_eq!(table.len(), 6);
        let back: Vec<(u16, char, u16, Source)> = table
            .records()
            .iter()
            .map(|r| (r.gid(), r.unicode(), r.width(), r.source()))
            .collect();
        assert_eq!(
            back,
            [
                (3, ' ', 260, Source::Cmap),
                (3, '\u{a0}', 260, Source::Cmap),
                (36, 'A', 639, Source::Cmap),
                (400, '\u{fb01}', 557, Source::Cmap),
                (400, '\u{fb01}', 557, Source::Shaped),
                (65535, '\u{10ffff}', 65535, Source::Cmap),
            ]
        );
        // Re-encoding the loaded records gives the same bytes.
        assert_eq!(encode(table.records().to_vec()), bytes);
        // Zero-copy: the table points into the input.
        assert_eq!(table.records().as_ptr().cast::<u8>(), bytes.as_ptr());
    }

    #[test]
    fn lookups_take_the_first_record_of_a_glyph() {
        let bytes = encode(sample());
        let table = GmapTable::new(&bytes).unwrap();
        assert_eq!(table.unicode(3), Some(' '));
        assert_eq!(table.records_of(3).len(), 2);
        assert_eq!(table.unicode(400), Some('\u{fb01}'));
        assert_eq!(table.records_of(400)[0].source(), Source::Cmap);
        assert_eq!(table.width(36), Some(639));
        assert_eq!(table.unicode(65535), Some('\u{10ffff}'));
        assert_eq!(table.unicode(4), None);
        assert_eq!(table.width(0), None);
        assert!(table.records_of(37).is_empty());
    }

    #[test]
    fn empty_gmap_is_valid() {
        let table = GmapTable::new(&[]).unwrap();
        assert!(table.is_empty());
        assert_eq!(table.unicode(0), None);
    }

    #[test]
    fn loader_rejects_bad_bytes() {
        let bytes = encode(sample());
        assert_eq!(
            GmapTable::new(&bytes[..10]).unwrap_err(),
            GmapError::BadLength(10)
        );

        let mut surrogate = bytes.clone();
        surrogate[RECORD_LEN + 2..RECORD_LEN + 6].copy_from_slice(&0xd800u32.to_le_bytes());
        assert_eq!(
            GmapTable::new(&surrogate).unwrap_err(),
            GmapError::BadUnicode(1)
        );

        let mut source = bytes.clone();
        source[2 * RECORD_LEN + 8] = 2;
        assert_eq!(
            GmapTable::new(&source).unwrap_err(),
            GmapError::BadSource(2)
        );

        // Swap the first two records.
        let mut swapped = bytes.clone();
        let (a, b) = swapped.split_at_mut(RECORD_LEN);
        a.swap_with_slice(&mut b[..RECORD_LEN]);
        assert_eq!(
            GmapTable::new(&swapped).unwrap_err(),
            GmapError::Unsorted(1)
        );

        // A repeated record.
        let mut repeated = bytes[..RECORD_LEN].to_vec();
        repeated.extend_from_slice(&bytes[..RECORD_LEN]);
        assert_eq!(
            GmapTable::new(&repeated).unwrap_err(),
            GmapError::Unsorted(1)
        );
    }
}
