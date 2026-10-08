//! The single inflater (D-023): miniz_oxide 0.9.1's core API with a
//! non-wrapping output buffer. `decode_chain`, the carver's inflate probe and
//! the C9 salvage all decide "does this stream decode cleanly" here; flate2
//! is never called. The outcome comes from `TINFLStatus`, never from an error
//! message (eng-r1-fr2).
// T-07 is the first caller outside the tests.
#![cfg_attr(not(test), allow(dead_code))]

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_PARSE_ZLIB_HEADER, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};

/// How an [`inflate`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InflateStatus {
    /// The stream ended and, for a zlib stream, its Adler-32 matched.
    Done,
    /// The data decoded to its end but the Adler-32 trailer disagrees.
    AdlerMismatch,
    /// Invalid deflate data; `at` is the input consumed when it failed.
    Failed { at: usize },
    /// The input ran out before the stream ended (truncated).
    NeedsMoreInput,
    /// The output reached the cap before the stream ended.
    CapHit,
}

/// The status, the bytes decoded (for an error, the valid prefix decoded
/// before it) and the input bytes consumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InflateResult {
    pub status: InflateStatus,
    pub out: Vec<u8>,
    pub consumed: usize,
}

const ZLIB: u32 = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
const RAW: u32 = TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;

/// Inflates `raw` as a zlib stream into at most `cap` bytes. When the zlib
/// header itself is rejected, it retries once as raw deflate (some producers
/// omit the header) and keeps that result only if it reaches `Done`.
pub(crate) fn inflate(raw: &[u8], cap: usize) -> InflateResult {
    let first = run(raw, cap, ZLIB);
    if header_rejected(&first) {
        let retry = run(raw, cap, RAW);
        if retry.status == InflateStatus::Done {
            return retry;
        }
    }
    first
}

/// The zlib header (its first two bytes) did not parse: nothing was decoded
/// and the failure is inside the header.
fn header_rejected(r: &InflateResult) -> bool {
    matches!(r.status, InflateStatus::Failed { at } if at <= 2) && r.out.is_empty()
}

/// One pass over `raw` with `flags`. The output buffer starts at
/// `max(4 × input, 64 KiB)` (the eng-r1-fr2 harness's size), doubles on
/// `HasMoreOutput` and never exceeds `cap`.
fn run(raw: &[u8], cap: usize, flags: u32) -> InflateResult {
    let mut r = Box::<DecompressorOxide>::default();
    let mut out = vec![0u8; raw.len().saturating_mul(4).max(64 << 10).min(cap)];
    let (mut in_pos, mut out_pos) = (0usize, 0usize);
    let status = loop {
        let (st, used, wrote) = decompress(&mut r, &raw[in_pos..], &mut out, out_pos, flags);
        in_pos += used;
        out_pos += wrote;
        break match st {
            TINFLStatus::HasMoreOutput => {
                if out.len() >= cap {
                    InflateStatus::CapHit
                } else {
                    let grown = out.len().saturating_mul(2).clamp(1, cap);
                    out.resize(grown, 0);
                    continue;
                }
            }
            TINFLStatus::Done => InflateStatus::Done,
            TINFLStatus::Adler32Mismatch => InflateStatus::AdlerMismatch,
            TINFLStatus::FailedCannotMakeProgress | TINFLStatus::NeedsMoreInput => {
                InflateStatus::NeedsMoreInput
            }
            // `Failed`, and `BadParam` or anything newer (the enum is
            // non-exhaustive): invalid data at this offset.
            _ => InflateStatus::Failed { at: in_pos },
        };
    };
    out.truncate(out_pos);
    InflateResult {
        status,
        out,
        consumed: in_pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The miniz column of eng-r1-fr2: its harness function, verbatim in
    /// behaviour (buffer `max(4 × input, 64 KiB)`, doubled on
    /// `HasMoreOutput`, zlib header parsed, no raw retry).
    fn oracle(input: &[u8]) -> (InflateStatus, usize, Vec<u8>) {
        let mut r = DecompressorOxide::new();
        let mut out = vec![0u8; (input.len() * 4).max(65536)];
        let (mut in_pos, mut out_pos) = (0usize, 0usize);
        loop {
            let (st, ci, co) = decompress(&mut r, &input[in_pos..], &mut out, out_pos, ZLIB);
            in_pos += ci;
            out_pos += co;
            let status = match st {
                TINFLStatus::Done => InflateStatus::Done,
                TINFLStatus::Adler32Mismatch => InflateStatus::AdlerMismatch,
                TINFLStatus::Failed => InflateStatus::Failed { at: in_pos },
                TINFLStatus::FailedCannotMakeProgress | TINFLStatus::NeedsMoreInput => {
                    InflateStatus::NeedsMoreInput
                }
                TINFLStatus::HasMoreOutput => {
                    let n = out.len() * 2;
                    out.resize(n, 0);
                    continue;
                }
                other => panic!("oracle saw {other:?}"),
            };
            out.truncate(out_pos);
            return (status, in_pos, out);
        }
    }

    /// Deterministic text that compresses with dynamic Huffman blocks.
    fn sample(len: usize) -> Vec<u8> {
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
        let mut x = 0x2545_f491_4f6c_dd1du64;
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

    fn zlib(data: &[u8]) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
    }

    #[test]
    fn clean_stream_is_done_and_consumes_the_stream_only() {
        let data = sample(20_000);
        let z = zlib(&data);
        let mut with_eol = z.clone();
        with_eol.extend_from_slice(b"\r\n");
        let r = inflate(&with_eol, 1 << 20);
        assert_eq!(r.status, InflateStatus::Done);
        assert_eq!(r.out, data);
        assert_eq!(r.consumed, z.len());
        assert_eq!((r.status, r.consumed, r.out), oracle(&with_eol));
    }

    #[test]
    fn adler_only_damage_decodes_everything_and_says_so() {
        let data = sample(20_000);
        let mut z = zlib(&data);
        let last = z.len() - 1;
        z[last] ^= 0x01;
        let r = inflate(&z, 1 << 20);
        assert_eq!(r.status, InflateStatus::AdlerMismatch);
        assert_eq!(r.out, data);
        assert_eq!(r.consumed, z.len());
        assert_eq!((r.status, r.consumed, r.out), oracle(&z));
    }

    #[test]
    fn data_error_at_k_keeps_the_prefix_and_reports_k() {
        let data = sample(20_000);
        let z = zlib(&data);
        // Find a mid-stream flip that is a data error, as C9 makes them.
        let (mutant, r) = (z.len() / 2..z.len())
            .find_map(|i| {
                let mut m = z.clone();
                m[i] ^= 0x10;
                let r = inflate(&m, 1 << 20);
                matches!(r.status, InflateStatus::Failed { .. }).then(|| (m, r))
            })
            .expect("some flip is a data error");
        let InflateStatus::Failed { at } = r.status else {
            unreachable!()
        };
        assert_eq!(at, r.consumed);
        assert!(at < mutant.len());
        // The output before the flip's first effect is the original's.
        let same = r.out.iter().zip(&data).take_while(|(a, b)| a == b).count();
        assert!(same >= data.len() / 4 && r.out.len() < data.len());
        assert_eq!((r.status, r.consumed, r.out), oracle(&mutant));
    }

    #[test]
    fn truncated_stream_needs_more_input() {
        let z = zlib(&sample(5_000));
        let r = inflate(&z[..z.len() / 2], 1 << 20);
        assert_eq!(r.status, InflateStatus::NeedsMoreInput);
        assert_eq!((r.status, r.consumed, r.out), oracle(&z[..z.len() / 2]));
        assert_eq!(inflate(b"", 1 << 20).status, InflateStatus::NeedsMoreInput);
    }

    /// Every single-byte flip of a stream (xor 0x01, 0x10, 0x80, the
    /// eng-r1-fr2 sweep) gets the oracle's status, consumed count and output,
    /// except a header the raw retry decodes to `Done`.
    #[test]
    fn mutation_sweep_matches_the_miniz_column() {
        let z = zlib(&sample(1_500));
        let mut seen = [0usize; 4];
        for pos in 0..z.len() {
            for mask in [0x01u8, 0x10, 0x80] {
                let mut m = z.clone();
                m[pos] ^= mask;
                let r = inflate(&m, 1 << 20);
                let o = oracle(&m);
                if pos < 2 && r.status == InflateStatus::Done && o.0 != InflateStatus::Done {
                    continue;
                }
                assert_eq!(
                    (r.status, r.consumed, &r.out),
                    (o.0, o.1, &o.2),
                    "pos {pos} mask {mask:#x}"
                );
                seen[match r.status {
                    InflateStatus::Done => 0,
                    InflateStatus::AdlerMismatch => 1,
                    InflateStatus::Failed { .. } => 2,
                    _ => 3,
                }] += 1;
            }
        }
        assert!(
            seen[1] > 0 && seen[2] > 0,
            "the sweep hit both error classes: {seen:?}"
        );
    }

    #[test]
    fn raw_deflate_is_retried_once() {
        let data = sample(3_000);
        let raw = miniz_oxide::deflate::compress_to_vec(&data, 6);
        let r = inflate(&raw, 1 << 20);
        assert_eq!(r.status, InflateStatus::Done);
        assert_eq!(r.out, data);
        assert_eq!(r.consumed, raw.len());
    }

    #[test]
    fn output_stops_at_the_cap() {
        let data = vec![0u8; 1 << 20];
        let z = zlib(&data);
        let r = inflate(&z, 64 << 10);
        assert_eq!(r.status, InflateStatus::CapHit);
        assert!(r.out.len() <= 64 << 10);
        assert!(data.starts_with(&r.out));
        let r = inflate(&z, 1 << 20);
        assert_eq!(r.status, InflateStatus::Done);
        assert_eq!(r.out.len(), 1 << 20);
        assert_eq!(inflate(&z, 0).status, InflateStatus::CapHit);
    }
}
