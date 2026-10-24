//! T-08 acceptance: the ladder, the grades, the budget, the deep pool, the
//! two-phase scheme, the filter domain and the memory bound.

use super::*;

use lopdf::{Document, Object};

use crate::pdf::fixtures::{
    TEST_FONT, corrupt_with_log, golden_pdf, with_flate_then_dct, with_multi_filter,
    with_predictor_image,
};
use crate::pdf::model::CorruptionClass;

// ── helpers ──────────────────────────────────────────────────────────────

/// Deterministic text that compresses with dynamic Huffman blocks (the
/// inflater's own test sample).
fn sample(len: usize, seed: u64) -> Vec<u8> {
    let words = [
        "BT ",
        "/F1 ",
        "12 ",
        "Tf ",
        "(quick) ",
        "Tj ",
        "0 -14 Td ",
        "ET\n",
        "q ",
        "Q ",
    ];
    let mut s = Vec::with_capacity(len + 16);
    let mut x = 0x2545_f491_4f6c_dd1du64 ^ seed;
    while s.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.extend_from_slice(words[(x % words.len() as u64) as usize].as_bytes());
        s.extend_from_slice(format!("{} ", x % 997).as_bytes());
    }
    s.truncate(len);
    s
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// A zlib stream of stored blocks: input bytes are output bytes, so the
/// tests can place damage exactly.
fn stored_zlib(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let len = b.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(b);
    }
    z.extend_from_slice(&adler32(data).to_be_bytes());
    z
}

/// The first single-byte flip (positions from `from`, in `step`s, masks 0x01,
/// 0x10, 0x80) whose inflate status satisfies `want`.
fn damage(z: &[u8], from: usize, step: isize, want: fn(InflateStatus) -> bool) -> (Vec<u8>, usize) {
    let mut pos = from as isize;
    while pos >= 0 && (pos as usize) < z.len() {
        for mask in [0x01u8, 0x10, 0x80] {
            let mut m = z.to_vec();
            m[pos as usize] ^= mask;
            if want(inflate(&m, DEFAULT_CAP).status) {
                return (m, pos as usize);
            }
        }
        pos += step;
    }
    panic!("no flip from {from} gives the wanted status");
}

fn is_adler(s: InflateStatus) -> bool {
    s == InflateStatus::AdlerMismatch
}

fn is_error(s: InflateStatus) -> bool {
    matches!(s, InflateStatus::Failed { .. })
}

fn budget(work: u64, deep_work: u64, deep_pool: u64) -> SalvageBudget {
    SalvageBudget {
        work,
        deep_work,
        deep_pool,
        ..SalvageBudget::default()
    }
}

fn under_work() -> SalvageBudget {
    SalvageBudget {
        deep_pool: 0,
        ..SalvageBudget::default()
    }
}

fn threads(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

/// Applies `edits` to `input`.
fn patched(input: &[u8], edits: &[Edit]) -> Vec<u8> {
    let mut m = input.to_vec();
    for &(at, old, new) in edits {
        assert_eq!(m[at], old, "edit at {at} names the byte there");
        m[at] = new;
    }
    m
}

/// A file's streams for `salvage_all`.
#[derive(Debug, Default)]
struct Streams(Vec<(ObjId, Dictionary, Vec<u8>)>);

impl StreamSource for Streams {
    fn streams(&self) -> Vec<(ObjId, &Dictionary, &[u8])> {
        self.0
            .iter()
            .map(|(id, d, r)| (*id, d, r.as_slice()))
            .collect()
    }
}

fn flate_dict() -> Dictionary {
    let mut d = Dictionary::new();
    d.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    d
}

/// Every top-level stream of `original`, its dictionary from lopdf and its
/// data read from `damaged` at the same place (C9 edits in place).
fn pdf_streams(original: &[u8], damaged: &[u8]) -> Streams {
    let doc = Document::load_mem(original).expect("the golden loads");
    let mut v = Vec::new();
    for (&(n, g), object) in &doc.objects {
        let Ok(s) = object.as_stream() else { continue };
        let at = memchr::memmem::find(original, &s.content).expect("stream data in the file");
        v.push((
            (n, g),
            s.dict.clone(),
            damaged[at..at + s.content.len()].to_vec(),
        ));
    }
    Streams(v)
}

fn stream_of(pdf: &[u8], id: u32) -> (Dictionary, Vec<u8>) {
    let s = pdf_streams(pdf, pdf);
    let (_, d, r) = s.0.into_iter().find(|(i, _, _)| i.0 == id).unwrap();
    (d, r)
}

// ── checkpoints ──────────────────────────────────────────────────────────

#[test]
fn checkpoint_resume_reproduces_output_and_adler() {
    let data = sample(9_000, 1);
    let z = zlib(&data);
    let base = baseline(&z, Mode::Zlib, true);
    assert_eq!(base.end, End::Done);
    assert_eq!((base.consumed, &base.out), (z.len(), &data));
    assert_eq!(base.checkpoints.len(), z.len().div_ceil(CHECKPOINT_EVERY));
    for (i, cp) in base.checkpoints.iter().enumerate() {
        assert_eq!(
            cp.in_pos,
            i * CHECKPOINT_EVERY,
            "checkpoint {i} sits on a boundary"
        );
        let mut st = cp.state.clone();
        let mut out = base.out[..cp.out_pos].to_vec();
        let r = drive(
            &mut st,
            &z[cp.in_pos..],
            &mut out,
            cp.out_pos,
            Mode::Zlib.flags(),
            DEFAULT_CAP,
            None,
        );
        // `Done` from a zlib decode means miniz compared the Adler-32.
        assert_eq!(r.end, End::Done, "resume from checkpoint {i}");
        assert_eq!(cp.in_pos + r.used, z.len());
        assert_eq!(&out[..r.out_end], data.as_slice());
        assert_eq!(adler32(&out[..r.out_end]).to_be_bytes(), z[z.len() - 4..]);
    }
}

/// The chunked baseline the search resumes from ends as T-06's single-call
/// inflate does, for every flip of a small stream.
#[test]
fn chunked_baseline_matches_the_inflater() {
    let z = zlib(&sample(1_200, 2));
    for pos in 0..z.len() {
        let mut m = z.clone();
        m[pos] ^= 0x10;
        let one = inflate(&m, DEFAULT_CAP);
        let base = baseline(&m, Mode::Zlib, true);
        let end = match one.status {
            InflateStatus::Done => End::Done,
            InflateStatus::AdlerMismatch => End::AdlerMismatch,
            InflateStatus::Failed { .. } => End::Failed,
            InflateStatus::NeedsMoreInput => End::Starved,
            InflateStatus::CapHit => End::CapHit,
        };
        if pos < 2 && one.status == InflateStatus::Done {
            continue; // the raw retry, which the zlib baseline does not do
        }
        assert_eq!(
            (base.end, base.consumed, &base.out),
            (end, one.consumed, &one.out),
            "pos {pos}"
        );
    }
}

#[test]
fn adler32_is_the_zlib_trailer() {
    for len in [0, 1, 5_551, 5_552, 5_553, 70_000] {
        let data = noise(len, len as u64 + 3);
        let z = zlib(&data);
        assert_eq!(adler32(&data).to_be_bytes(), z[z.len() - 4..], "len {len}");
    }
}

#[test]
fn the_input_trace_maps_every_input_byte() {
    let data = sample(3_000, 4);
    let z = zlib(&data);
    let t = InputTrace::of(&z, Mode::Zlib, true);
    assert_eq!(t.out_after().len(), z.len());
    assert!(t.out_after().windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(*t.out_after().last().unwrap(), data.len());
}

// ── the ladder on the C9 golden ──────────────────────────────────────────

const CONTENT_STREAMS: [u32; 2] = [11, 12];
const FONT_FILE: u32 = 8;

/// Every C9 hit in a golden content stream (each under 512 bytes), over the
/// first seeds: repaired byte-exact, grade `Exact`, under `work`.
#[test]
fn golden_content_streams_are_repaired_exact() {
    let golden = golden_pdf();
    let clean = pdf_streams(&golden, &golden);
    let mut hits = 0;
    for seed in 0..12 {
        let (damaged, log) = corrupt_with_log(CorruptionClass::C9ZlibTampered, &golden, seed);
        let streams = pdf_streams(&golden, &damaged);
        for ((id, _, raw), (_, _, orig)) in streams.0.iter().zip(&clean.0) {
            if !CONTENT_STREAMS.contains(&id.0) || raw == orig {
                continue;
            }
            assert!(raw.len() <= 512);
            hits += 1;
            let want = decode_chain(orig, &[Filter::Flate], DEFAULT_CAP).unwrap();
            let s = salvage_inflate(raw, &under_work());
            let Salvage::Repaired {
                data,
                edits,
                grade,
                work,
                ..
            } = &s
            else {
                panic!("seed {seed} stream {id:?}: {s:?} (log {log:?})");
            };
            assert_eq!(*grade, Grade::Exact, "seed {seed} stream {id:?}");
            assert_eq!(data, &want);
            assert_eq!(inflate(&patched(raw, edits), DEFAULT_CAP).out, want);
            assert!(*work < SalvageBudget::default().work);
        }
    }
    assert!(hits >= 4, "the seeds hit the content streams {hits} times");
}

/// The 9.6 KB embedded font with Adler-only damage early in the stream is
/// out of reach under `work` with no localizer registered (T-08b's
/// localizer and its tests are in `ttf`).
#[test]
fn font_stream_is_checksum_mismatch_under_work() {
    let golden = golden_pdf();
    let (_, raw) = stream_of(&golden, FONT_FILE);
    assert_eq!(inflate(&raw, DEFAULT_CAP).out, TEST_FONT);
    let (damaged, _) = damage(&raw, raw.len() / 4, 1, is_adler);
    let s = salvage_with(&damaged, &under_work(), &[]);
    let Salvage::ChecksumMismatch { data } = &s else {
        panic!("{s:?}");
    };
    assert_eq!(data.len(), TEST_FONT.len());
    assert_ne!(data.as_slice(), TEST_FONT);
}

// ── grades ───────────────────────────────────────────────────────────────

/// Writes `bits` low bits of `v`, least significant first.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u64, bits: u32) {
        self.acc |= v << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code goes in most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        let rev = (0..len).fold(0u64, |r, i| r | u64::from((code >> i) & 1) << (len - 1 - i));
        self.put(rev, len);
    }
}

/// A zlib stream of one fixed-Huffman block of literals 0–143 only: literal
/// `k` takes exactly the 8 bits starting at stream bit 19 + 8k, and its last
/// code bit (the one that tells `'0'` from `'1'`) is bit 2 of byte `k + 3`.
fn fixed_literals(data: &[u8]) -> Vec<u8> {
    let mut b = Bits {
        out: vec![0x78, 0x01],
        acc: 0,
        n: 0,
    };
    b.put(1, 1); // BFINAL
    b.put(1, 2); // fixed Huffman
    for &x in data {
        assert!(x < 144);
        b.code(0x30 + u32::from(x), 8);
    }
    b.code(0, 7); // end of block
    if b.n > 0 {
        b.put(0, 8 - b.n);
    }
    b.out.extend_from_slice(&adler32(data).to_be_bytes());
    b.out
}

/// A localizer that only sets the window (the collision test's search
/// order).
struct Fixed(Vec<Range<usize>>);

impl Localizer for Fixed {
    fn window(&self, _: &[u8], _: &InputTrace) -> Option<Window> {
        Some(Window {
            ranges: self.0.clone(),
            check: None,
            widen: false,
        })
    }
    fn early_reject(&self, _: &Check, _: &[u8], _: usize) -> Option<bool> {
        unreachable!("a window with no check asks nothing")
    }
}

/// An Adler-32 collision (eng-r1-fr1 (c), constructed): the damage turns a
/// `'0'` into a `'1'` at output `i`; restoring it is one survivor, and turning
/// the `'1'` at `i + 65,521` into a `'0'` is another with a different output
/// and the same Adler-32 (the byte sum is unchanged and the weighted sum moves
/// by 65,521). Whichever the search meets first, `data` is the lowest-offset
/// survivor's, which is the original.
#[test]
fn adler_collision_is_ambiguous_and_keeps_the_lowest_offset() {
    let (i, j) = (100usize, 100 + ADLER_MOD as usize);
    let mut text: Vec<u8> = noise(70_000, 9).iter().map(|b| b'0' + (b & 1)).collect();
    text[i] = b'0';
    text[j] = b'1';
    let z = fixed_literals(&text);
    assert_eq!(inflate(&z, DEFAULT_CAP).out, text);
    let mut damaged = z.clone();
    damaged[i + 3] ^= 0x04;
    let mut wrong = text.clone();
    wrong[i] = b'1';
    assert_eq!(inflate(&damaged, DEFAULT_CAP).out, wrong);
    assert_eq!(
        inflate(&damaged, DEFAULT_CAP).status,
        InflateStatus::AdlerMismatch
    );

    let near_i = i + 1..i + 6;
    let near_j = j + 1..j + 6;
    let mut results = Vec::new();
    for order in [vec![near_i.clone(), near_j.clone()], vec![near_j, near_i]] {
        let l = Fixed(order);
        let s = salvage_with(&damaged, &under_work(), &[&l]);
        let Salvage::Repaired {
            data,
            edits,
            grade,
            survivors,
            trailer_edit,
            ..
        } = &s
        else {
            panic!("{s:?}");
        };
        assert_eq!(*grade, Grade::Ambiguous { outputs: 2 });
        assert_eq!(data, &text, "the lowest-offset survivor is the original");
        assert_eq!(edits, &vec![(i + 3, damaged[i + 3], z[i + 3])]);
        assert_eq!(survivors.len(), 2);
        assert_eq!(survivors[0], *edits);
        assert_eq!(survivors[1][0].0, j + 3);
        assert!(!trailer_edit);
        results.push(s);
    }
    assert_eq!(results[0], results[1]);
}

/// A damaged Adler-32 trailer byte: the body decodes to the original and the
/// trailer rewrite is the only survivor.
#[test]
fn trailer_damage_is_repaired_with_a_trailer_edit() {
    let data = sample(400, 5);
    let z = zlib(&data);
    let at = z.len() - 4;
    let mut damaged = z.clone();
    damaged[at] ^= 0x20;
    let s = salvage_inflate(&damaged, &under_work());
    let Salvage::Repaired {
        data: out,
        edits,
        grade,
        trailer_edit,
        ..
    } = &s
    else {
        panic!("{s:?}");
    };
    assert_eq!(*grade, Grade::Exact);
    assert!(*trailer_edit);
    assert_eq!(out, &data);
    assert_eq!(edits, &vec![(at, damaged[at], z[at])]);
    assert_eq!(s.grade(), Some(SalvageGrade::Exact));
}

#[test]
fn budget_out_before_an_accept_keeps_what_decoded() {
    let data = sample(6_000, 6);
    let z = zlib(&data);
    let tiny = budget(10_000, 0, 0);

    let (adler, _) = damage(&z, z.len() / 3, 1, is_adler);
    let s = salvage_inflate(&adler, &tiny);
    let Salvage::ChecksumMismatch { data: out } = &s else {
        panic!("{s:?}");
    };
    assert_eq!(out, &inflate(&adler, DEFAULT_CAP).out);

    let (error, _) = damage(&z, z.len() / 3, 1, is_error);
    let r = inflate(&error, DEFAULT_CAP);
    let InflateStatus::Failed { at } = r.status else {
        unreachable!()
    };
    let s = salvage_inflate(&error, &tiny);
    assert_eq!(
        s,
        Salvage::Prefix {
            data: r.out,
            in_used: at,
            in_total: error.len()
        }
    );
}

/// An error stream whose damage sits a few bytes before `k`: the first
/// accept comes early, the window (about 1,200 positions) much later.
fn early_accept_stream() -> (Vec<u8>, Vec<u8>) {
    let data = sample(8_000, 7);
    let z = zlib(&data);
    for from in (z.len() / 2..z.len() - 64).step_by(7) {
        let (m, pos) = damage(&z, from, 1, is_error);
        let InflateStatus::Failed { at } = inflate(&m, DEFAULT_CAP).status else {
            unreachable!()
        };
        if at >= pos && at - pos < 4 {
            return (data, m);
        }
    }
    panic!("no flip fails within 4 bytes of itself");
}

#[test]
fn budget_out_after_an_accept_is_accepted_with_the_patched_bytes() {
    let (data, m) = early_accept_stream();
    let s = salvage_inflate(&m, &budget(10_000_000, 0, 0));
    let Salvage::Repaired {
        data: out,
        edits,
        grade,
        work,
        ..
    } = &s
    else {
        panic!("{s:?}");
    };
    let Grade::Accepted { searched, window } = *grade else {
        panic!("{grade:?}");
    };
    assert!(searched < window);
    assert_eq!(out, &data);
    assert_eq!(inflate(&patched(&m, edits), DEFAULT_CAP).out, data);
    assert!(*work >= 10_000_000);
}

/// D-041: the result never depends on how much budget there was beyond the
/// grade, and W is the same on every run.
#[test]
fn determinism_under_budget() {
    let (data, m) = early_accept_stream();
    let mut kept: Option<(Vec<u8>, Vec<Edit>)> = None;
    let mut repaired = 0;
    for work in [1_000_000u64, 10_000_000, 100_000_000] {
        let b = budget(work, 0, 0);
        let s = salvage_inflate(&m, &b);
        assert_eq!(s, salvage_inflate(&m, &b), "work {work} twice");
        match s {
            Salvage::Repaired {
                data: out,
                edits,
                grade: Grade::Exact | Grade::Accepted { .. },
                ..
            } => {
                repaired += 1;
                assert_eq!(out, data);
                match &kept {
                    Some(k) => assert_eq!(k, &(out, edits), "work {work}"),
                    None => kept = Some((out, edits)),
                }
            }
            Salvage::Prefix { .. } => assert!(kept.is_none(), "a larger budget lost the accept"),
            other => panic!("work {work}: {other:?}"),
        }
    }
    assert!(repaired >= 2);
}

/// A damaged raw-deflate stream: T-06's inflate reports it as a zlib header
/// failure with nothing decoded, and the raw decode stops at `k`.
fn damaged_raw_deflate() -> (Vec<u8>, Vec<u8>) {
    let data = sample(2_000, 8);
    let raw = miniz_oxide::deflate::compress_to_vec(&data, 6);
    let (m, _) = damage(&raw, raw.len() - 40, -1, |s| {
        matches!(s, InflateStatus::Failed { at: 0..=2 })
    });
    assert!(inflate(&m, DEFAULT_CAP).out.is_empty());
    (data, m)
}

/// Raw deflate has no Adler-32: any candidate that decodes to the input's
/// end is an accept, so the best grade is `Ambiguous`. Every survivor reads
/// the input to its end (within the 2-byte slack): one that stops early is a
/// truncation, not a repair.
#[test]
fn raw_deflate_is_ambiguous_at_best() {
    let (_, m) = damaged_raw_deflate();
    let s = salvage_inflate(&m, &budget(20_000_000, 0, 0));
    let Salvage::Repaired {
        grade,
        edits,
        survivors,
        ..
    } = &s
    else {
        panic!("{s:?}");
    };
    assert!(
        matches!(grade, Grade::Ambiguous { outputs } if *outputs >= 1),
        "{grade:?}"
    );
    assert_eq!(edits.len(), 1);
    for e in survivors {
        let b = baseline(&patched(&m, e), Mode::Raw, false);
        assert_eq!(b.end, End::Done, "{e:?}");
        assert!(
            b.consumed + RAW_END_SLACK >= m.len(),
            "{e:?} stops at {}",
            b.consumed
        );
    }
}

/// A zlib stream with its CMF byte damaged: T-06 reports a rejected header,
/// and many such bytes also decode as raw deflate for a while (`7A` and `7B`
/// start fixed-Huffman blocks, `7D` a dynamic one). Before the fix `7A` came
/// out `Repaired` as raw deflate, with byte 0 rewritten to `03`. The
/// zlib search runs first, so every value is repaired as zlib, byte-exact
/// and `Exact`, never as a raw survivor or a raw prefix. (`03` reads as an
/// empty final fixed block; T-06's inflate no longer calls that `Done`.)
#[test]
fn a_damaged_zlib_header_is_repaired_as_zlib() {
    let data = sample(2_000, 21);
    let z = zlib(&data);
    assert_eq!(z[0], 0x78);
    let mut raw_done = 0;
    for cmf in (0..=255u8).filter(|&v| v != 0x78) {
        let mut m = z.clone();
        m[0] = cmf;
        if inflate(&m, DEFAULT_CAP).status == InflateStatus::Done {
            // T-06's raw retry decoded the whole input; not the ladder's call.
            raw_done += 1;
            continue;
        }
        let s = salvage_inflate(&m, &under_work());
        let Salvage::Repaired {
            data: got,
            edits,
            grade,
            trailer_edit,
            ..
        } = &s
        else {
            panic!("cmf {cmf:#04x}: {s:?}");
        };
        assert_eq!(got, &data, "cmf {cmf:#04x}");
        assert_eq!(edits, &vec![(0, cmf, 0x78)], "cmf {cmf:#04x}");
        assert_eq!(*grade, Grade::Exact, "cmf {cmf:#04x}");
        assert!(!trailer_edit);
    }
    assert_eq!(raw_done, 0);
}

/// A truncated raw-deflate stream: the raw read runs out of input, and T-06's
/// rule holds: a raw retry's partial output is never kept as a `Prefix`.
#[test]
fn a_raw_read_that_runs_out_keeps_nothing() {
    let raw = miniz_oxide::deflate::compress_to_vec(&sample(3_000, 22), 6);
    let cut = &raw[..raw.len() / 2];
    assert!(matches!(
        inflate(cut, DEFAULT_CAP).status,
        InflateStatus::Failed { at: 0..=2 }
    ));
    assert_eq!(baseline(cut, Mode::Raw, false).end, End::Starved);
    assert_eq!(salvage_inflate(cut, &under_work()), Salvage::Unrecoverable);
}

/// F-04 (D-128): 59 streams of noise. Almost every one is rejected at the
/// zlib header, so the ladder searches it as zlib and then reads it as raw
/// deflate, and noise often decodes a few bytes as raw deflate before it
/// fails. Those bytes are a guess: no stream whose header was rejected comes
/// out `Prefix`.
#[test]
fn noise_never_keeps_a_raw_guess() {
    let (mut rejected, mut raw_decoded) = (0, 0);
    for seed in 1..=59u64 {
        let m = noise(64 + 31 * seed as usize, seed);
        if !matches!(read(&m), Read::Header { .. }) {
            continue;
        }
        rejected += 1;
        if raw_fallback(&m).is_some() {
            raw_decoded += 1;
        }
        let s = salvage_inflate(&m, &under_work());
        assert!(!matches!(s, Salvage::Prefix { .. }), "seed {seed}: {s:?}");
    }
    assert!(rejected >= 50, "{rejected} rejected");
    assert!(raw_decoded >= 10, "{raw_decoded} raw decodes");
}

/// A raw-deflate stream of stored blocks whose last block header is invalid
/// (`LEN` 0, `NLEN` not its complement): T-06 rejects its zlib header, and
/// the raw read decodes every data byte, then fails within
/// [`RAW_END_SLACK`] of the input's end.
fn raw_failing_at_its_end() -> (Vec<u8>, Vec<u8>) {
    let data = sample(1_000, 23);
    let mut m = vec![0x00];
    let len = data.len() as u16;
    m.extend_from_slice(&len.to_le_bytes());
    m.extend_from_slice(&(!len).to_le_bytes());
    m.extend_from_slice(&data);
    m.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00]);
    assert!(matches!(read(&m), Read::Header { .. }));
    let b = baseline(&m, Mode::Raw, false);
    assert_eq!(b.end, End::Failed);
    assert!(b.consumed + RAW_END_SLACK >= m.len(), "{}", b.consumed);
    assert_eq!(b.out, data);
    (data, m)
}

/// A raw-deflate search that found nothing keeps the raw read's output as a
/// `Prefix` only when the read failed within [`RAW_END_SLACK`] of the end;
/// short of that it is `Unrecoverable`. A zlib read keeps its prefix as
/// before.
#[test]
fn a_raw_search_with_no_survivor_keeps_only_a_read_to_the_end() {
    let concluded = |mode, k| {
        let plan = Plan {
            windows: [Vec::new(), Vec::new()],
            localizer: None,
            check: None,
            widen: None,
        };
        let damage = Damage::Error { k };
        let p = Pending::new(mode, damage, b"abc".to_vec(), 100, plan, false);
        p.conclude(0, true).salvage
    };
    let kept = |in_used| Salvage::Prefix {
        data: b"abc".to_vec(),
        in_used,
        in_total: 100,
    };
    assert_eq!(concluded(Mode::Raw, 100), kept(100));
    assert_eq!(concluded(Mode::Raw, 98), kept(98));
    assert_eq!(concluded(Mode::Raw, 97), Salvage::Unrecoverable);
    assert_eq!(concluded(Mode::Raw, 7), Salvage::Unrecoverable);
    assert_eq!(concluded(Mode::Zlib, 7), kept(7));
}

/// An unsearched raw-deflate stream whose raw read fails at its end:
/// `decoded()` gives that read's output, the stream's own bytes.
#[test]
fn an_unsearched_raw_stream_read_to_its_end_decodes_in_raw_mode() {
    let (data, m) = raw_failing_at_its_end();
    let streams = Streams(vec![((1, 0), flate_dict(), m)]);
    let tiny = SalvageBudget {
        max_search_stream: 16,
        ..under_work()
    };
    let index = salvage_all(&streams, &tiny, threads(1), 1 << 30, &|| false).unwrap();
    assert!(matches!(
        index.by_obj[&(1, 0)].salvage,
        Salvage::Unsearched { .. }
    ));
    let got = index.decoded(&streams, (1, 0), DEFAULT_CAP).unwrap();
    assert_eq!(got.as_ref(), data.as_slice());
}

/// An unsearched raw-deflate stream whose raw read fails short of its end:
/// that read is a guess, and a searched stream would keep none of it, so
/// `decoded()` gives no bytes.
#[test]
fn an_unsearched_raw_guess_decodes_to_nothing() {
    let (_, m) = damaged_raw_deflate();
    let streams = Streams(vec![((1, 0), flate_dict(), m.clone())]);
    let tiny = SalvageBudget {
        max_search_stream: 16,
        ..under_work()
    };
    let index = salvage_all(&streams, &tiny, threads(1), 1 << 30, &|| false).unwrap();
    assert!(matches!(
        index.by_obj[&(1, 0)].salvage,
        Salvage::Unsearched { .. }
    ));
    let guess = baseline(&m, Mode::Raw, false);
    assert!(!guess.out.is_empty());
    assert!(guess.consumed + RAW_END_SLACK < m.len());
    let got = index.decoded(&streams, (1, 0), DEFAULT_CAP).unwrap();
    assert!(got.is_empty());
}

/// The zlib header `78 01` damaged to `78 1D` (FCHECK fails): restoring the
/// FLG byte at offset 1 and rewriting CMF at offset 0 to `08` both verify
/// and give the same output. The search runs top down, so a budget that
/// stops between them reports `Accepted` with the offset-1 edit, and the
/// whole window gives `Exact` with the offset-0 edit (the pinned order's
/// lowest offset). `data` is the same; `edits` are not. This pins the
/// current rule (the spec's "identical data and edits" cannot hold for
/// same-output edits under a top-down search; reported as a decision).
#[test]
fn same_output_edits_keep_data_but_not_edits_across_budgets() {
    let data = sample(600, 12);
    let mut z = zlib(&data);
    z[1] = 0x01;
    assert_eq!(inflate(&z, DEFAULT_CAP).out, data);
    let mut m = z.clone();
    m[1] = 0x1D;
    assert!(is_error(inflate(&m, DEFAULT_CAP).status));
    let mut fixed = m.clone();
    fixed[0] = 0x08;
    assert_eq!(inflate(&fixed, DEFAULT_CAP).status, InflateStatus::Done);

    let whole = salvage_inflate(&m, &budget(100_000_000, 0, 0));
    let Salvage::Repaired {
        data: exact,
        edits,
        grade: Grade::Exact,
        survivors,
        ..
    } = &whole
    else {
        panic!("{whole:?}");
    };
    assert_eq!(exact, &data);
    assert_eq!(edits, &vec![(0, 0x78, 0x08)]);
    assert_eq!(survivors.len(), 1, "one distinct output");

    let mut work = 0;
    let first = loop {
        work += 2_000;
        let s = salvage_inflate(&m, &budget(work, 0, 0));
        if matches!(s, Salvage::Repaired { .. }) {
            break s;
        }
        assert!(work < 100_000_000);
    };
    let Salvage::Repaired {
        data: accepted,
        edits,
        grade: Grade::Accepted { .. },
        ..
    } = &first
    else {
        panic!("{first:?}");
    };
    assert_eq!(accepted, exact, "the same data");
    assert_eq!(edits, &vec![(1, 0x1D, 0x01)], "a different edit");
}

/// A check that refuses every candidate leaves the first pass with no
/// accept; the search then widens with no check (the window, then the rest
/// of the ladder's own), the repair carries `adler_rerun`, and it costs more
/// W than the plain search.
#[test]
fn a_check_refusing_everything_is_rerun_under_adler() {
    struct No;
    impl Localizer for No {
        fn window(&self, _: &[u8], _: &InputTrace) -> Option<Window> {
            Some(Window {
                ranges: std::iter::once(2..300).collect(),
                check: Some(Check {
                    at_out: 1,
                    from: 0,
                    memo: Arc::new(()),
                }),
                widen: false,
            })
        }
        fn early_reject(&self, _: &Check, _: &[u8], _: usize) -> Option<bool> {
            Some(true)
        }
    }
    let data = sample(400, 10);
    let z = zlib(&data);
    let (m, _) = damage(&z, 150, 1, is_adler);
    let refused = salvage_with(&m, &under_work(), &[&No]);
    let plain = salvage_with(
        &m,
        &under_work(),
        &[&Fixed(std::iter::once(2..300).collect())],
    );
    let (
        Salvage::Repaired {
            data: d1,
            edits: e1,
            grade: Grade::Exact,
            work: w1,
            adler_rerun: true,
            ..
        },
        Salvage::Repaired {
            data: d2,
            edits: e2,
            grade: Grade::Exact,
            work: w2,
            adler_rerun: false,
            ..
        },
    ) = (&refused, &plain)
    else {
        panic!("{refused:?} / {plain:?}");
    };
    assert_eq!((d1, e1), (d2, e2));
    assert_eq!(d1, &data);
    assert!(w1 > w2, "{w1} > {w2}");
}

/// The widened window: the localized positions at or past `from` first,
/// then the ladder's own window without the localized positions, each piece
/// from its top down.
#[test]
fn the_widened_window_skips_what_was_judged_by_the_adler_alone() {
    let own = std::iter::once(2..100).collect::<Vec<_>>();
    assert_eq!(
        widened(&[40..60, 10..20], 15, &own),
        [40..60, 15..20, 60..100, 20..40, 2..10]
    );
    assert_eq!(
        widened(&[40..60, 10..20], usize::MAX, &own),
        [60..100, 20..40, 2..10]
    );
    assert_eq!(widened(&own, 0, &own), own);
    assert_eq!(widened(&own, usize::MAX, &own), []);
    // The deep error window extends the phase-1 one, and so do the widened.
    let [w0, w1] = own_windows(Damage::Error { k: 5_000 }, 6_000);
    let localized = std::iter::once(4_990..5_000).collect::<Vec<_>>();
    let (a, b) = (widened(&localized, 0, &w0), widened(&localized, 0, &w1));
    assert_eq!(a[..], b[..a.len()]);
    assert_eq!(b.len(), a.len() + 1);
}

// ── salvage_all ──────────────────────────────────────────────────────────

/// Twelve streams with Adler-only damage about 24 positions before the
/// trailer: out of reach under `work`, within reach of a deep draw. Raw
/// lengths are padded to distinct sizes, ids scrambled so the smallest are
/// not the lowest ids; two share a size so the id breaks the tie.
fn twelve() -> (Streams, Vec<ObjId>) {
    let sizes = [
        3_900u64, 3_100, 3_700, 3_000, 3_400, 3_300, 3_800, 3_200, 3_000, 3_600, 3_500, 4_000,
    ];
    let mut v = Vec::new();
    for (n, size) in sizes.iter().enumerate() {
        let z = zlib(&sample(2_400 + 40 * n, 100 + n as u64));
        assert!((z.len() as u64) < sizes[3]);
        let (mut m, _) = damage(&z, z.len() - 4 - 24, -1, is_adler);
        m.resize(*size as usize, b'\n');
        let id = (40 - 3 * n as u32, 0);
        v.push((id, flate_dict(), m));
    }
    let mut by_size: Vec<(u64, ObjId)> = v.iter().map(|(id, _, r)| (r.len() as u64, *id)).collect();
    by_size.sort();
    let smallest = by_size[..3].iter().map(|(_, id)| *id).collect();
    (Streams(v), smallest)
}

fn deep_budget() -> SalvageBudget {
    budget(200_000, 10_000_000, 30_000_000)
}

#[test]
fn deep_pool_goes_to_the_three_smallest_in_order() {
    let (streams, smallest) = twelve();
    let pending: Vec<(u64, ObjId)> = streams
        .0
        .iter()
        .rev()
        .map(|(id, _, r)| (r.len() as u64, *id))
        .collect();
    let b = deep_budget();
    assert_eq!(deep_allocation(&pending, &b), smallest);
    assert_eq!(deep_allocation(&pending, &b), deep_allocation(&pending, &b));
    let first = salvage_all(&streams, &b, threads(4), 1 << 30, &|| false).unwrap();
    for (id, e) in &first.by_obj {
        match &e.salvage {
            Salvage::Repaired { work, .. } => {
                assert!(smallest.contains(id), "{id:?} was repaired without a draw");
                assert!(*work > b.work);
            }
            Salvage::ChecksumMismatch { .. } => assert!(!smallest.contains(id), "{id:?}"),
            other => panic!("{id:?}: {other:?}"),
        }
    }
    assert_eq!(first.by_obj.len(), 12);
    assert_eq!(
        first,
        salvage_all(&streams, &b, threads(4), 1 << 30, &|| false).unwrap()
    );
}

#[test]
fn salvage_all_is_the_same_at_one_two_and_eight_threads() {
    let (streams, _) = twelve();
    let golden = golden_pdf();
    let (damaged, _) = corrupt_with_log(CorruptionClass::C9ZlibTampered, &golden, 3);
    let c9 = pdf_streams(&golden, &damaged);
    for (s, b) in [
        (&streams, deep_budget()),
        (&c9, budget(1_000_000, 4_000_000, 8_000_000)),
    ] {
        let one = salvage_all(s, &b, threads(1), 1 << 30, &|| false).unwrap();
        assert!(one.work_total > 0);
        for n in [2, 8] {
            assert_eq!(
                one,
                salvage_all(s, &b, threads(n), 1 << 30, &|| false).unwrap(),
                "{n} threads"
            );
        }
    }
}

#[test]
fn salvage_all_is_cancellable_between_streams() {
    let (streams, _) = twelve();
    let b = deep_budget();
    assert_eq!(
        salvage_all(&streams, &b, threads(2), 1 << 30, &|| true),
        Err(Cancelled)
    );
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let later = || calls.fetch_add(1, Ordering::Relaxed) >= 2;
    assert_eq!(
        salvage_all(&streams, &b, threads(1), 1 << 30, &later),
        Err(Cancelled)
    );
    assert!(salvage_all(&streams, &b, threads(1), 1 << 30, &|| false).is_ok());
}

/// The C9 golden through `salvage_all`: every Flate stream has an entry, the
/// undamaged ones `Clean` and keeping nothing, and `decoded()` gives every
/// stream's bytes as a direct decode of the original does.
#[test]
fn salvage_all_on_the_c9_golden() {
    let golden = golden_pdf();
    let clean = pdf_streams(&golden, &golden);
    let (damaged, _) = corrupt_with_log(CorruptionClass::C9ZlibTampered, &golden, 3);
    let streams = pdf_streams(&golden, &damaged);
    let index = salvage_all(
        &streams,
        &budget(1_000_000, 0, 0),
        threads(2),
        1 << 30,
        &|| false,
    )
    .unwrap();
    let flate: Vec<ObjId> = clean
        .0
        .iter()
        .filter(|(_, d, _)| filters_of(d).iter().any(|(f, _)| *f == Filter::Flate))
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(index.by_obj.keys().copied().collect::<Vec<_>>(), flate);
    for ((id, dict, raw), (_, _, orig)) in streams.0.iter().zip(&clean.0) {
        let want = decode_chain(orig, &filters_of(dict), DEFAULT_CAP).unwrap();
        let got = index.decoded(&streams, *id, DEFAULT_CAP);
        match index.by_obj.get(id).map(|e| &e.salvage) {
            Some(Salvage::Clean {
                decoded_len,
                sha256,
                ..
            }) => {
                assert_eq!(raw, orig);
                assert_eq!(*decoded_len, want.len() as u64);
                assert_eq!(*sha256, <[u8; 32]>::from(Sha256::digest(&want)));
                assert_eq!(got.unwrap().as_ref(), want.as_slice());
            }
            Some(Salvage::Repaired {
                grade: Grade::Exact,
                ..
            }) => {
                assert!(matches!(got, Ok(Cow::Borrowed(_))));
                assert_eq!(got.unwrap().as_ref(), want.as_slice());
            }
            Some(other) => assert!(
                matches!(
                    other,
                    Salvage::Repaired { .. }
                        | Salvage::ChecksumMismatch { .. }
                        | Salvage::Prefix { .. }
                ),
                "{id:?}: {other:?}"
            ),
            None => assert_eq!(got.unwrap().as_ref(), want.as_slice()),
        }
    }
    assert!(index.decoded(&streams, (99, 0), DEFAULT_CAP).is_err());
}

/// The carve as the source: carving the damaged golden finds the same
/// streams, so the index, its work and every `decoded()` agree with the
/// lopdf-read source's.
#[test]
fn the_carve_is_a_stream_source() {
    let golden = golden_pdf();
    let (damaged, _) = corrupt_with_log(CorruptionClass::C9ZlibTampered, &golden, 3);
    let streams = pdf_streams(&golden, &damaged);
    let carve = crate::pdf::carver::carve(&damaged, &|| false).unwrap();
    let source = CarveSource::new(&carve, &damaged);
    let b = budget(1_000_000, 0, 0);
    let want = salvage_all(&streams, &b, threads(2), 1 << 30, &|| false).unwrap();
    let got = salvage_all(&source, &b, threads(2), 1 << 30, &|| false).unwrap();
    assert!(want.work_total > 0);
    assert_eq!(got, want);
    for (id, _, raw) in &streams.0 {
        assert_eq!(source.stream(*id).map(|(_, r)| r), Some(raw.as_slice()));
        assert_eq!(
            got.decoded(&source, *id, DEFAULT_CAP).unwrap(),
            want.decoded(&streams, *id, DEFAULT_CAP).unwrap()
        );
    }
    assert_eq!(source.stream((99, 0)), None);
}

// ── the filter domain ────────────────────────────────────────────────────

fn ascii85_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut g = [0u8; 4];
        g[..chunk.len()].copy_from_slice(chunk);
        let mut v = u32::from_be_bytes(g);
        let mut digits = [0u8; 5];
        for d in digits.iter_mut().rev() {
            *d = b'!' + (v % 85) as u8;
            v /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// One Flate-stage byte of stream `id` in `pdf` replaced (a C9 hit), the
/// earlier filters re-encoded around it, and the salvage run over it.
#[test]
fn the_filter_domain_is_the_flate_input() {
    let cases: [(Vec<u8>, u32); 3] = [
        (with_predictor_image(), 10),
        (with_multi_filter(), 11),
        (with_flate_then_dct(), 10),
    ];
    for (pdf, id) in cases {
        let (dict, raw) = stream_of(&pdf, id);
        let chain = filters_of(&dict);
        let flate_at = chain.iter().position(|(f, _)| *f == Filter::Flate).unwrap();
        let input = decode_chain(&raw, &chain[..flate_at], DEFAULT_CAP).unwrap();
        let (damaged_input, pos) = damage(&input, input.len() / 2, 1, |s| s != InflateStatus::Done);
        let damaged_raw = if flate_at == 0 {
            damaged_input.clone()
        } else {
            ascii85_encode(&damaged_input)
        };
        let streams = Streams(vec![((id, 0), dict.clone(), damaged_raw)]);
        let index = salvage_all(&streams, &under_work(), threads(1), 1 << 30, &|| false).unwrap();
        let entry = &index.by_obj[&(id, 0)];
        let Salvage::Repaired { edits, grade, .. } = &entry.salvage else {
            panic!("{id}: {:?}", entry.salvage);
        };
        assert_eq!(*grade, Grade::Exact, "{id}");
        assert_eq!(
            edits[0].1, damaged_input[edits[0].0],
            "the offset is a Flate-input offset"
        );
        assert!(edits[0].0.abs_diff(pos) < 64);
        assert_eq!(
            decode_chain(
                &patched(&damaged_input, edits),
                &chain[flate_at..],
                DEFAULT_CAP
            )
            .unwrap(),
            decode_chain(&input, &chain[flate_at..], DEFAULT_CAP).unwrap()
        );
        let want = decode_chain(&raw, &chain, DEFAULT_CAP).unwrap();
        assert_eq!(
            index
                .decoded(&streams, (id, 0), DEFAULT_CAP)
                .unwrap()
                .as_ref(),
            want.as_slice()
        );
        let stage = &entry.stage;
        assert_eq!(
            stage.earlier,
            chain[..flate_at]
                .iter()
                .map(|(f, _)| f.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(stage.flate_parms, chain[flate_at].1);
        assert_eq!(stage.later, chain[flate_at + 1..].to_vec());
    }
    // What each case put where.
    let (d, _) = stream_of(&with_predictor_image(), 10);
    assert!(
        filters_of(&d)[0]
            .1
            .as_ref()
            .is_some_and(|p| p.has(b"Predictor"))
    );
    let (d, _) = stream_of(&with_multi_filter(), 11);
    assert_eq!(filters_of(&d)[0].0, Filter::Ascii85);
    let (d, _) = stream_of(&with_flate_then_dct(), 10);
    assert_eq!(filters_of(&d)[1].0, Filter::Dct);
}

/// The predictor is undone in `ChecksumMismatch` data, so no reader sees PNG
/// row-filter bytes as samples.
#[test]
fn partial_data_has_its_predictor_undone() {
    let (dict, raw) = stream_of(&with_predictor_image(), 10);
    let want = decode_chain(&raw, &filters_of(&dict), DEFAULT_CAP).unwrap();
    let mut m = raw.clone();
    let at = m.len() - 2;
    m[at] ^= 0x01; // Adler-only: the trailer
    m[at - 1] ^= 0x01;
    let streams = Streams(vec![((10, 0), dict, m)]);
    let index = salvage_all(&streams, &budget(0, 0, 0), threads(1), 1 << 30, &|| false).unwrap();
    let Salvage::ChecksumMismatch { data } = &index.by_obj[&(10, 0)].salvage else {
        panic!("{:?}", index.by_obj[&(10, 0)].salvage);
    };
    assert_eq!(data, &want);
}

/// A PNG-predicted stream whose damaged output has a row filter byte above
/// 4 keeps the rows before it, as `ChecksumMismatch` (budget out) or
/// `Prefix` (truncated), never `Unrecoverable`.
#[test]
fn a_garbage_predictor_row_keeps_the_rows_before_it() {
    let mut parms = Dictionary::new();
    parms.set("Predictor", Object::Integer(12));
    parms.set("Columns", Object::Integer(4));
    let mut dict = flate_dict();
    dict.set("DecodeParms", Object::Dictionary(parms.clone()));
    let rows: Vec<u8> = (0..50u8).flat_map(|r| [2, r, r ^ 0x55, 3, 7]).collect();
    let bad_row = 20;
    let want = unpredict_flate(rows[..5 * bad_row].to_vec(), Some(&parms)).unwrap();
    assert_eq!(want.len(), 4 * bad_row);

    // Stored blocks: the damaged input byte is the damaged output byte.
    let mut z = stored_zlib(&rows);
    let header = 2 + 5;
    z[header + 5 * bad_row] = 0x07;
    assert!(is_adler(inflate(&z, DEFAULT_CAP).status));
    let streams = Streams(vec![((1, 0), dict.clone(), z.clone())]);
    let index = salvage_all(&streams, &budget(0, 0, 0), threads(1), 1 << 30, &|| false).unwrap();
    assert_eq!(
        index.by_obj[&(1, 0)].salvage,
        Salvage::ChecksumMismatch { data: want.clone() }
    );

    let cut = header + 5 * 30;
    let streams = Streams(vec![((1, 0), dict, z[..cut].to_vec())]);
    let index = salvage_all(&streams, &budget(0, 0, 0), threads(1), 1 << 30, &|| false).unwrap();
    assert_eq!(
        index.by_obj[&(1, 0)].salvage,
        Salvage::Prefix {
            data: want,
            in_used: cut,
            in_total: cut
        }
    );
}

/// A repair verifies the inflate, not the predictor: when the original's own
/// rows hold an invalid filter type, the repaired output is undone only up
/// to that row, so the entry is a `ChecksumMismatch` of those rows rather
/// than a graded `Repaired` that silently lost its tail.
#[test]
fn a_repair_with_an_invalid_predictor_row_is_not_graded() {
    let mut parms = Dictionary::new();
    parms.set("Predictor", Object::Integer(12));
    parms.set("Columns", Object::Integer(4));
    let mut dict = flate_dict();
    dict.set("DecodeParms", Object::Dictionary(parms.clone()));
    let bad_row = 20;
    let mut rows: Vec<u8> = (0..50u8).flat_map(|r| [2, r, r ^ 0x55, 3, 7]).collect();
    rows[5 * bad_row] = 0x09;
    let want = unpredict_flate(rows[..5 * bad_row].to_vec(), Some(&parms)).unwrap();

    let z = stored_zlib(&rows);
    let header = 2 + 5;
    let mut m = z.clone();
    m[header + 5 * 40 + 1] ^= 0x10;
    assert!(is_adler(inflate(&m, DEFAULT_CAP).status));
    let s = salvage_inflate(&m, &under_work());
    assert!(
        matches!(&s, Salvage::Repaired { data, grade: Grade::Exact, .. } if *data == rows),
        "{s:?}"
    );
    let streams = Streams(vec![((1, 0), dict, m)]);
    let index = salvage_all(&streams, &under_work(), threads(1), 1 << 30, &|| false).unwrap();
    assert_eq!(
        index.by_obj[&(1, 0)].salvage,
        Salvage::ChecksumMismatch { data: want }
    );
}

// ── memory (D-072) ───────────────────────────────────────────────────────

#[test]
fn clean_entries_keep_no_bytes() {
    // The largest clean payload is three plain fields, no vector.
    assert!(size_of::<(u64, StreamClass, [u8; 32])>() <= 48);
    let mut streams = Streams::default();
    for n in 0..5 {
        let data = sample(5_000 + n * 999, n as u64);
        streams
            .0
            .push(((n as u32 + 1, 0), flate_dict(), zlib(&data)));
    }
    let index = salvage_all(
        &streams,
        &SalvageBudget::default(),
        threads(2),
        1 << 30,
        &|| false,
    )
    .unwrap();
    assert_eq!(index.heap_bytes(), 0);
    assert_eq!(index.work_total, 0);
    for (id, _, raw) in &streams.0 {
        let got = index.decoded(&streams, *id, DEFAULT_CAP).unwrap();
        assert!(matches!(got, Cow::Owned(_)));
        assert_eq!(got.as_ref(), inflate(raw, DEFAULT_CAP).out.as_slice());
        let want = Salvage::Clean {
            decoded_len: got.len() as u64,
            class: classify(&flate_dict(), &got),
            sha256: Sha256::digest(&got).into(),
        };
        assert_eq!(index.by_obj[id].salvage, want);
    }
}

#[test]
fn heap_bytes_counts_retained_data() {
    let data = sample(6_000, 11);
    let z = zlib(&data);
    let (m, _) = damage(&z, z.len() / 3, 1, is_adler);
    let streams = Streams(vec![((1, 0), flate_dict(), m)]);
    let index = salvage_all(
        &streams,
        &budget(10_000, 0, 0),
        threads(1),
        1 << 30,
        &|| false,
    )
    .unwrap();
    assert_eq!(index.heap_bytes(), data.len() as u64);
}

/// The counting allocator for the scratch test: allocations and frees on
/// threads that are tracked (the test's own and the `salvage_all` workers it
/// starts) move a live counter, whose peak is read back.
pub(super) mod track {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicI64, Ordering};

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
    }
    static LIVE: AtomicI64 = AtomicI64::new(0);
    static PEAK: AtomicI64 = AtomicI64::new(0);

    pub(crate) fn current() -> bool {
        ON.try_with(Cell::get).unwrap_or(false)
    }

    pub(crate) fn set(on: bool) {
        let _ = ON.try_with(|c| c.set(on));
    }

    /// One tracked run at a time: the counters are process-wide.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Runs `f` tracked and returns its result and the peak live bytes.
    pub(crate) fn peak<R>(f: impl FnOnce() -> R) -> (R, u64) {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        LIVE.store(0, Ordering::SeqCst);
        PEAK.store(0, Ordering::SeqCst);
        set(true);
        let r = f();
        set(false);
        (r, PEAK.load(Ordering::SeqCst).max(0) as u64)
    }

    fn moved(delta: i64) {
        if current() {
            let now = LIVE.fetch_add(delta, Ordering::SeqCst) + delta;
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
    }

    struct Counting;

    // SAFETY: every call forwards to `System` unchanged; the counting only
    // touches atomics and a const-initialised thread-local `Cell`, neither
    // of which allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            moved(layout.size() as i64);
            // SAFETY: the caller's contract is `System`'s.
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            moved(-(layout.size() as i64));
            // SAFETY: as above.
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            moved(layout.size() as i64);
            // SAFETY: as above.
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            moved(new_size as i64 - layout.size() as i64);
            // SAFETY: as above.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;
}

/// `len` bytes of stored-block zlib with one payload byte damaged `back`
/// bytes before the trailer (Adler-only damage), plus its original output.
fn big_stream(len: usize, back: usize) -> (Vec<u8>, Vec<u8>) {
    let data = noise(len, len as u64);
    let mut z = stored_zlib(&data);
    let at = z.len() - 4 - back;
    z[at] ^= 0x5a;
    (z, data)
}

fn with_small_ones(big: Vec<u8>) -> Streams {
    let mut v = vec![((1, 0), flate_dict(), big)];
    for n in 0..3u32 {
        let z = zlib(&sample(1_500 + 200 * n as usize, 50 + u64::from(n)));
        let (m, _) = damage(&z, z.len() - 4 - 8, -1, is_adler);
        v.push(((n + 2, 0), flate_dict(), m));
    }
    Streams(v)
}

#[test]
fn a_16_mib_stream_is_unsearched_and_its_raised_search_fits_the_cap() {
    let (big, data) = big_stream(16 << 20, 10);
    let streams = with_small_ones(big);
    let cap = 1u64 << 30;
    let small = SalvageBudget {
        work: 4_000_000,
        deep_pool: 0,
        ..SalvageBudget::default()
    };
    let default = salvage_all(&streams, &small, threads(8), cap, &|| false).unwrap();
    assert!(matches!(
        default.by_obj[&(1, 0)].salvage,
        Salvage::Unsearched {
            reason: OverMaxSearchStream { raw_len, limit: 4_194_304 }
        } if raw_len == streams.0[0].2.len() as u64
    ));
    assert_eq!(
        default.by_obj[&(1, 0)].salvage.grade(),
        Some(SalvageGrade::Unsearched)
    );
    assert!(
        default.heap_bytes() < 1 << 20,
        "an unsearched stream keeps nothing"
    );
    let got = default.decoded(&streams, (1, 0), DEFAULT_CAP).unwrap();
    assert_eq!(
        got.len(),
        data.len(),
        "decoded() gives what a ChecksumMismatch would keep"
    );

    let raised = SalvageBudget {
        max_search_stream: 32 << 20,
        ..small
    };
    let mut digests = Vec::new();
    for n in [1, 2, 8] {
        let (index, peak) =
            track::peak(|| salvage_all(&streams, &raised, threads(n), cap, &|| false).unwrap());
        assert!(peak < cap, "{n} threads: peak scratch {peak} B");
        // The checkpoints alone (10.5 KB per 256 input bytes) are counted.
        assert!(
            peak > 40 * (16 << 20),
            "{n} threads: the allocator saw {peak} B"
        );
        let Salvage::Repaired { data: out, .. } = &index.by_obj[&(1, 0)].salvage else {
            panic!("{:?}", index.by_obj[&(1, 0)].salvage.grade());
        };
        assert!(out == &data);
        digests.push(index);
    }
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn a_5_mib_stream_is_searched_only_under_a_raised_limit() {
    let (big, data) = big_stream(5 << 20, 10);
    let b = SalvageBudget {
        work: 4_000_000,
        deep_pool: 0,
        ..SalvageBudget::default()
    };
    assert!(matches!(
        salvage_inflate(&big, &b),
        Salvage::Unsearched { .. }
    ));
    let raised = SalvageBudget {
        max_search_stream: 8 << 20,
        ..b
    };
    let Salvage::Repaired { data: out, .. } = salvage_inflate(&big, &raised) else {
        panic!("not repaired");
    };
    assert!(out == data);
}

#[test]
fn admission_counts_scratch_per_search() {
    assert_eq!(scratch_estimate(0, 0), 1 << 20);
    let four = 4u64 << 20;
    // Up to 3:1 the plan's formula holds: a 4 MiB search needs about 312
    // MiB, so three fit under 1 GiB.
    assert_eq!(scratch_estimate(four, 3 * four), 66 * four + 12 * four);
    assert!(3 * scratch_estimate(four, 3 * four) <= 1 << 30);
    assert!(4 * scratch_estimate(four, 3 * four) > 1 << 30);
    // A flat stream at 1000:1 is charged for its four output buffers.
    assert_eq!(
        scratch_estimate(25_000, 25_000_000),
        66 * 25_000 + 100_000_000
    );
}

/// D-072 with highly compressible streams: four flat streams (16 MiB of
/// zeros, about 16 KB compressed) with Adler-only damage. Each search holds
/// several 16 MiB buffers, far over the plan's raw-length estimate, so
/// admission charges the output: under a 64 MiB cap the searches run one at
/// a time and the peak stays near one search's, where charging the raw
/// length alone lets all four run together.
#[test]
fn admission_charges_a_high_ratio_streams_output() {
    let mut v = Vec::new();
    for n in 0..4u32 {
        let mut flat = vec![0u8; 16 << 20];
        flat[n as usize] = 1;
        let z = zlib(&flat);
        assert!(z.len() < 64 << 10);
        let (m, _) = damage(&z, z.len() - 4 - 8, -1, is_adler);
        v.push(((n + 1, 0), flate_dict(), m));
    }
    let streams = Streams(v);
    let b = budget(1_000_000, 0, 0);
    let cap = 64u64 << 20;
    let (one, alone) =
        track::peak(|| salvage_all(&streams, &b, threads(1), cap, &|| false).unwrap());
    let (four, peak) =
        track::peak(|| salvage_all(&streams, &b, threads(4), cap, &|| false).unwrap());
    assert_eq!(one, four);
    assert_eq!(four.by_obj.len(), 4);
    // The searches never overlap: four threads peak as one does (charging
    // the raw length alone, all four run together and peak at about 4x).
    assert!(
        peak < alone + alone / 2,
        "4 threads peaked at {peak} B, one at {alone} B"
    );
}

/// D-041: no clock in this module (clippy's `disallowed-types` enforces it
/// crate-wide; this keeps the module honest even where clippy allows it).
#[test]
fn no_clock_in_the_salvage() {
    let src = include_str!("../salvage.rs");
    for word in ["Instant", "SystemTime", "Duration", "std::time"] {
        assert!(!src.contains(word), "salvage.rs names {word}");
    }
}
