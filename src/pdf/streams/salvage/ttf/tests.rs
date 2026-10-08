//! T-08b acceptance: the sfnt parser and checksum, the complete and the
//! gaps-only window, verifier B, the Adler-only re-run guard, determinism
//! under budget and the reject counts.

use super::super::*;
use super::*;

use lopdf::{Document, Object};

use crate::pdf::fixtures::{TEST_FONT, golden_pdf};

// ── helpers ──────────────────────────────────────────────────────────────

/// The golden's `/FontFile2` stream: [`TEST_FONT`] deflated.
fn font_stream() -> Vec<u8> {
    let doc = Document::load_mem(&golden_pdf()).expect("the golden loads");
    let streams: Vec<Vec<u8>> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            Object::Stream(s) if s.dict.get(b"Length1").is_ok() => Some(s.content.clone()),
            _ => None,
        })
        .collect();
    let [z] = &streams[..] else {
        panic!("one font stream, found {}", streams.len());
    };
    assert_eq!(inflate(z, DEFAULT_CAP).out, TEST_FONT);
    z.clone()
}

fn table(font: &[u8], tag: &[u8; 4]) -> Table {
    let dir = Directory::parse(font).unwrap();
    *dir.tables.iter().find(|t| &t.tag == tag).unwrap()
}

fn failing(font: &[u8]) -> Vec<[u8; 4]> {
    let dir = Directory::parse(font).unwrap();
    dir.tables
        .iter()
        .filter(|t| !t.verifies(font, dir.end))
        .map(|t| t.tag)
        .collect()
}

fn trace_of(input: &[u8]) -> Vec<usize> {
    InputTrace::of(input, Mode::Zlib, true).out_after().to_vec()
}

/// The first one-byte change, over `positions` in order and values from 0
/// up, whose decode ends `AdlerMismatch` with output satisfying `want`.
fn seeded(
    z: &[u8],
    positions: impl IntoIterator<Item = usize>,
    want: impl Fn(&[u8]) -> bool,
) -> (Vec<u8>, usize) {
    for pos in positions {
        for v in 0..=255u8 {
            if v == z[pos] {
                continue;
            }
            let mut m = z.to_vec();
            m[pos] = v;
            let r = inflate(&m, DEFAULT_CAP);
            if r.status == InflateStatus::AdlerMismatch && want(&r.out) {
                return (m, pos);
            }
        }
    }
    panic!("no change gives the wanted damage");
}

/// A change near the top of `glyf`'s input span that makes `glyf`, and only
/// `glyf`, fail.
fn glyf_damage(z: &[u8]) -> (Vec<u8>, usize) {
    let glyf = table(TEST_FONT, b"glyf");
    let top = Map(&trace_of(z)).producer(glyf.end());
    seeded(z, (top - 40..top - 20).rev(), |out| {
        failing(out) == [*b"glyf"]
    })
}

fn under_work() -> SalvageBudget {
    SalvageBudget {
        deep_pool: 0,
        ..SalvageBudget::default()
    }
}

fn with_work(work: u64) -> SalvageBudget {
    SalvageBudget {
        work,
        deep_pool: 0,
        ..SalvageBudget::default()
    }
}

fn patched(input: &[u8], edits: &[Edit]) -> Vec<u8> {
    let mut m = input.to_vec();
    for &(at, old, new) in edits {
        assert_eq!(m[at], old);
        m[at] = new;
    }
    m
}

/// A small sfnt of `tables` (4-byte aligned, zero-padded) with correct
/// checksums, except `wrong`'s, which is off by one.
fn sfnt(tables: &[([u8; 4], &[u8])], wrong: Option<&[u8; 4]>) -> Vec<u8> {
    let n = tables.len();
    let mut font = vec![0, 1, 0, 0];
    font.extend_from_slice(&(n as u16).to_be_bytes());
    font.extend_from_slice(&[0; 6]);
    let mut offset = DIR_HEAD + RECORD * n;
    let mut body = Vec::new();
    for (tag, data) in tables {
        let sum = checksum(tag, data).wrapping_add(u32::from(wrong == Some(tag)));
        font.extend_from_slice(tag);
        font.extend_from_slice(&sum.to_be_bytes());
        font.extend_from_slice(&(offset as u32).to_be_bytes());
        font.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        body.resize(body.len().next_multiple_of(4), 0);
        offset = DIR_HEAD + RECORD * n + body.len();
    }
    font.extend_from_slice(&body);
    font
}

/// A 1.4 KB font made of the test font's first 900 `glyf` bytes, `head`
/// and `post`.
fn small_font(wrong: Option<&[u8; 4]>) -> Vec<u8> {
    let part = |tag: &[u8; 4], len: usize| {
        let t = table(TEST_FONT, tag);
        &TEST_FONT[t.offset..t.offset + t.len.min(len)]
    };
    sfnt(
        &[
            (*b"glyf", part(b"glyf", 900)),
            (*b"head", part(b"head", usize::MAX)),
            (*b"post", part(b"post", usize::MAX)),
        ],
        wrong,
    )
}

fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// A localizer with [`TtfLocalizer`]'s check (or none) over fixed ranges.
struct Narrow {
    ranges: Vec<Range<usize>>,
    check: bool,
}

impl Localizer for Narrow {
    fn window(&self, baseline_out: &[u8], trace: &InputTrace) -> Option<Window> {
        let w = TtfLocalizer.window(baseline_out, trace)?;
        Some(Window {
            ranges: self.ranges.clone(),
            check: w.check.filter(|_| self.check),
        })
    }
    fn early_reject(&self, candidate_out_prefix: &[u8]) -> Option<bool> {
        TtfLocalizer.early_reject(candidate_out_prefix)
    }
}

/// The search of `input`'s localized window under `limit`, as the ladder
/// runs it, with its counters.
fn hunt(input: &[u8], l: &dyn Localizer, limit: u64) -> Pending {
    let Read::Damaged { mode, damage, out } = read(input) else {
        panic!("searchable damage");
    };
    let (windows, localizer, check) = windows_for(input, mode, damage, &out, &[l]);
    assert_eq!(localizer, Some(0));
    let mut p = Pending {
        mode,
        damage,
        fallback: out,
        in_total: input.len(),
        windows,
        localizer,
        check,
        rerun: false,
        raw_next: false,
        hunt: Hunt::default(),
    };
    search(input, &mut p, 0, limit, &[l], &Ctx::alone()).expect("never cancelled");
    p
}

// ── the sfnt parser and checksum ─────────────────────────────────────────

/// Every table of the test font verifies; `head` only with its
/// `checkSumAdjustment` counted as zero, and that adjustment is the
/// published `0xB1B0AFBA` minus the whole font's sum.
#[test]
fn every_test_font_table_verifies() {
    let dir = Directory::parse(TEST_FONT).expect("the directory parses");
    assert_eq!(dir.tables.len(), 10);
    assert_eq!(dir.end, 12 + 16 * 10);
    for t in &dir.tables {
        assert!(t.verifies(TEST_FONT, dir.end), "{:?}", t.tag);
    }
    assert!(failing(TEST_FONT).is_empty());

    let head = table(TEST_FONT, b"head");
    let bytes = &TEST_FONT[head.offset..head.offset + head.len];
    let adjustment = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    assert_ne!(adjustment, 0);
    assert_ne!(checksum(b"xxxx", bytes), head.checksum, "counted as is");
    assert_eq!(checksum(b"head", bytes), head.checksum);
    let whole = checksum(b"xxxx", TEST_FONT).wrapping_sub(adjustment);
    assert_eq!(0xB1B0_AFBAu32.wrapping_sub(whole), adjustment);
}

#[test]
fn the_parser_takes_sfnt_versions_only() {
    for version in [b"true", b"OTTO"] {
        let mut font = TEST_FONT.to_vec();
        font[..4].copy_from_slice(version);
        assert_eq!(Directory::parse(&font).unwrap().tables.len(), 10);
    }
    let mut font = TEST_FONT.to_vec();
    font[..4].copy_from_slice(b"wOFF");
    assert_eq!(Directory::parse(&font), None);
    assert_eq!(
        Directory::parse(&TEST_FONT[..12 + 16 * 9]),
        None,
        "cut short"
    );
    assert_eq!(
        Directory::parse(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        None
    );
    assert_eq!(Directory::parse(b"BT /F1 12 Tf ET"), None);
}

/// The localizer applies to Adler-only damage of an sfnt stream only.
#[test]
fn the_localizer_applies_to_adler_only_font_streams() {
    let z = font_stream();
    let (damaged, _) = glyf_damage(&z);
    let out = inflate(&damaged, DEFAULT_CAP).out;
    assert!(
        TtfLocalizer
            .window(&out, &InputTrace::of(&damaged, Mode::Zlib, true))
            .is_some()
    );
    assert_eq!(
        TtfLocalizer.window(&out, &InputTrace::of(&damaged, Mode::Zlib, false)),
        None,
        "error damage keeps the ladder's own window"
    );
    let text = zlib(b"BT /F1 12 Tf (not a font) Tj ET");
    let t = InputTrace::of(&text, Mode::Zlib, true);
    assert_eq!(
        TtfLocalizer.window(&inflate(&text, DEFAULT_CAP).out, &t),
        None
    );
}

/// Verifier B judges the table that ends where the prefix ends.
#[test]
fn early_reject_checks_the_table_ending_at_the_prefix() {
    let glyf = table(TEST_FONT, b"glyf");
    let prefix = &TEST_FONT[..glyf.end()];
    assert_eq!(TtfLocalizer.early_reject(prefix), Some(false));
    let mut bad = prefix.to_vec();
    bad[glyf.offset + 100] ^= 1;
    assert_eq!(TtfLocalizer.early_reject(&bad), Some(true));
    // No table ends here: nothing to judge.
    let mut off_end = TEST_FONT[..glyf.end() - 4].to_vec();
    off_end[glyf.offset + 100] ^= 1;
    assert_eq!(TtfLocalizer.early_reject(&off_end), Some(false));
    assert_eq!(TtfLocalizer.early_reject(b"not a font"), Some(false));
}

// ── the window ───────────────────────────────────────────────────────────

/// A seeded change inside `glyf` makes exactly `glyf` fail. `glyf` is the
/// first table after the directory, so the complete window is the
/// directory's and `glyf`'s input span and nothing past it, and B applies
/// from the first byte past the directory, at `end(glyf)`.
#[test]
fn glyf_damage_gives_a_window_inside_glyf() {
    let z = font_stream();
    let (damaged, pos) = glyf_damage(&z);
    let out = inflate(&damaged, DEFAULT_CAP).out;
    assert_eq!(failing(&out), [*b"glyf"]);

    let trace = trace_of(&damaged);
    let map = Map(&trace);
    let glyf = table(TEST_FONT, b"glyf");
    assert_eq!(glyf.offset, 12 + 16 * 10, "glyf follows the directory");
    let w = TtfLocalizer
        .window(&out, &InputTrace::of(&damaged, Mode::Zlib, true))
        .unwrap();
    let top = map.producer(glyf.end()) + 1;
    assert_eq!(
        w.ranges,
        std::iter::once(ADLER_FROM..top).collect::<Vec<_>>()
    );
    assert!(w.ranges[0].contains(&pos));
    assert!(top < damaged.len() - 4 - 500, "later tables are left out");
    let from = map.producer(glyf.offset) + 1;
    assert_eq!(
        w.check,
        Some(Check {
            at_out: glyf.end(),
            from,
        })
    );
    // Past the directory, the window is glyf's input span.
    assert!(trace[from - 1] >= glyf.offset && trace[top - 2] < glyf.end());
}

/// A change inside the directory that makes no table fail (the damage only
/// the Adler-32 sees) gives the gaps-only window: the directory and the
/// padding between tables, with no check.
#[test]
fn directory_damage_gives_the_gaps_only_window() {
    let z = font_stream();
    let dir_end = Directory::parse(TEST_FONT).unwrap().end;
    let span = Map(&trace_of(&z)).producer(dir_end);
    let (damaged, pos) = seeded(&z, ADLER_FROM..span, |out| {
        out.len() == TEST_FONT.len()
            && out[..dir_end] != TEST_FONT[..dir_end]
            && Directory::parse(out).is_some()
            && failing(out).is_empty()
    });
    let out = inflate(&damaged, DEFAULT_CAP).out;
    let w = TtfLocalizer
        .window(&out, &InputTrace::of(&damaged, Mode::Zlib, true))
        .unwrap();
    assert_eq!(w.check, None);
    let mut tables: Vec<Range<usize>> = Directory::parse(TEST_FONT)
        .unwrap()
        .tables
        .iter()
        .map(|t| t.offset..t.end())
        .collect();
    tables.sort_by_key(|r| r.start);
    let mut gaps: Vec<Range<usize>> = std::iter::once(0..tables[0].start).collect();
    gaps.extend(
        tables
            .windows(2)
            .filter(|p| p[0].end < p[1].start)
            .map(|p| p[0].end..p[1].start),
    );
    assert_eq!(gaps, [0..172, 8290..8292, 9418..9420]);
    let trace = trace_of(&damaged);
    assert_eq!(w.ranges, Map(&trace).inputs(&gaps));
    assert!(w.ranges.iter().any(|r| r.contains(&pos)));
    let in_glyf = Map(&trace).producer(4_000);
    assert!(!w.ranges.iter().any(|r| r.contains(&in_glyf)));
}

// ── the repair ───────────────────────────────────────────────────────────

/// The test-font stream damaged in `glyf` is repaired byte-exact under
/// `work`. Its complete window is `glyf`'s whole input span (5,732
/// positions; enumerating it costs about 3.9e9 W), so under `work` the grade
/// is `Accepted`, not `Exact`.
#[test]
fn glyf_damage_is_repaired_byte_exact_under_work() {
    let z = font_stream();
    let (damaged, pos) = glyf_damage(&z);
    let s = salvage_inflate(&damaged, &under_work());
    let Salvage::Repaired {
        data,
        edits,
        grade: Grade::Accepted { searched, window },
        survivors,
        adler_rerun: false,
        trailer_edit: false,
        ..
    } = &s
    else {
        panic!("{s:?}");
    };
    assert_eq!(data, TEST_FONT);
    assert_eq!(edits, &[(pos, damaged[pos], z[pos])]);
    assert_eq!(patched(&damaged, edits), z);
    assert_eq!(survivors.len(), 1);
    assert!(searched < window);
}

/// Damage in a small late table (`loca`) leaves a window of the directory,
/// `loca` and nothing else, which `work` exhausts: `Exact`.
#[test]
fn loca_damage_is_repaired_exact_under_work() {
    let z = font_stream();
    let loca = table(TEST_FONT, b"loca");
    let map_trace = trace_of(&z);
    let map = Map(&map_trace);
    let span = map.producer(loca.offset + 8)..map.producer(loca.end());
    let (damaged, pos) = seeded(&z, span, |out| failing(out) == [*b"loca"]);
    let out = inflate(&damaged, DEFAULT_CAP).out;
    let w = TtfLocalizer
        .window(&out, &InputTrace::of(&damaged, Mode::Zlib, true))
        .unwrap();
    let positions: usize = w.ranges.iter().map(|r| r.len()).sum();
    assert!(positions < 500, "a few hundred positions: {positions}");
    let s = salvage_inflate(&damaged, &under_work());
    let Salvage::Repaired {
        data,
        edits,
        grade: Grade::Exact,
        work,
        adler_rerun: false,
        ..
    } = &s
    else {
        panic!("{s:?}");
    };
    assert_eq!(data, TEST_FONT);
    assert_eq!(edits, &[(pos, damaged[pos], z[pos])]);
    assert!(*work < under_work().work);
}

/// A font whose original `glyf` checksum is wrong: B refuses every
/// candidate, the window is searched again under the Adler-32 alone, and
/// the repair says so. The same font with a right checksum is repaired by
/// B, for less W.
#[test]
fn a_wrong_original_checksum_is_repaired_by_the_adler_rerun() {
    let mut results = Vec::new();
    for wrong in [Some(b"glyf"), None] {
        let font = small_font(wrong);
        assert_eq!(
            failing(&font),
            if wrong.is_some() {
                vec![*b"glyf"]
            } else {
                vec![]
            }
        );
        let z = zlib(&font);
        let glyf = table(&font, b"glyf");
        let top = Map(&trace_of(&z)).producer(glyf.end());
        let (damaged, pos) = seeded(&z, (top - 60..top - 40).rev(), |out| {
            out[glyf.offset..glyf.end()] != font[glyf.offset..glyf.end()]
                && out[glyf.end()..] == font[glyf.end()..]
        });
        let s = salvage_inflate(&damaged, &under_work());
        let Salvage::Repaired {
            data,
            edits,
            grade: Grade::Exact,
            work,
            adler_rerun,
            ..
        } = s
        else {
            panic!("{s:?}");
        };
        assert_eq!(data, font);
        assert_eq!(edits, [(pos, damaged[pos], z[pos])]);
        assert_eq!(adler_rerun, wrong.is_some());
        results.push(work);
    }
    assert!(
        results[0] > results[1],
        "the re-run costs more: {results:?}"
    );
}

/// Determinism under budget (as in T-08): `work` of 1e6, 1e7 and 1e8 gives
/// the same `data` and `edits` whenever the grade is `Exact` or
/// `Accepted`, and every result, `work` included, is the same run to run.
#[test]
fn determinism_under_budget() {
    let z = font_stream();
    let (damaged, _) = glyf_damage(&z);
    let mut repairs = Vec::new();
    for work in [1_000_000u64, 10_000_000, 100_000_000] {
        let a = salvage_inflate(&damaged, &with_work(work));
        let b = salvage_inflate(&damaged, &with_work(work));
        assert_eq!(a, b, "work {work}");
        if let Salvage::Repaired {
            data, edits, grade, ..
        } = a
        {
            assert!(matches!(grade, Grade::Exact | Grade::Accepted { .. }));
            repairs.push((data, edits));
        }
    }
    assert!(repairs.len() >= 2, "1e7 and 1e8 find the repair");
    assert!(repairs.windows(2).all(|p| p[0] == p[1]));
    assert_eq!(repairs[0].0, TEST_FONT);
}

/// B rejects exactly what A rejects: over the same window (part of `glyf`
/// and positions past it), every candidate A refuses is refused by B either
/// after decoding to `end(glyf)` or, past `glyf`, with no inflate, the
/// accepts are the same, B spends less W, and every count is the same on
/// two runs.
#[test]
fn b_rejects_what_a_rejects() {
    let z = font_stream();
    let (damaged, pos) = glyf_damage(&z);
    let glyf = table(TEST_FONT, b"glyf");
    let top = Map(&trace_of(&damaged)).producer(glyf.end());
    let past = top + 300..top + 304;
    let ranges = vec![past.clone(), pos - 10..pos + 10];
    let counts = |check: bool| {
        let l = Narrow {
            ranges: ranges.clone(),
            check,
        };
        let p = hunt(&damaged, &l, u64::MAX);
        let h = &p.hunt;
        let found: Vec<_> = h.found.values().copied().collect();
        (
            h.tried,
            h.accepted,
            h.checked_out,
            h.no_inflate,
            h.spent,
            found,
        )
    };
    let a = counts(false);
    let b = counts(true);
    assert_eq!(a, counts(false), "A twice");
    assert_eq!(b, counts(true), "B twice");
    let (a_tried, a_accepted, a_checked, a_none, a_spent, a_found) = a;
    let (b_tried, b_accepted, b_checked, b_none, b_spent, b_found) = b;
    assert_eq!((a_checked, a_none), (0, 0));
    assert_eq!(a_tried, b_tried);
    assert_eq!((a_accepted, &a_found), (b_accepted, &b_found));
    assert_eq!(a_found.len(), 1);
    assert_eq!(b_none, past.len() as u64 * 255, "every candidate past glyf");
    assert!(b_checked > 0);
    let a_rejected = a_tried - a_accepted;
    let b_rejected_inflated = b_tried - b_accepted - b_none;
    assert_eq!(b_rejected_inflated + b_none, a_rejected);
    assert!(b_spent < a_spent, "{b_spent} < {a_spent}");
}
