//! Two modes over the sample directory written by ../preflate_prep.py (zlib streams; the 2-byte header and
//! 4-byte Adler-32 trailer are stripped before use):
//!
//! default: run preflate_whole_deflate_stream (parameter estimation + token prediction + corrections),
//!   recreate the stream from plaintext + corrections, and print one TSV row per stream: compressed size,
//!   total corrections size, the size of the CABAC misprediction blob alone, and the estimated parameters.
//!
//! --replay: use preflate-rs's token predictor as a pure-Rust encoder-replay oracle. Re-encode the plaintext
//!   from parameters alone with ZERO corrections (a CABAC blob of zero bytes decodes every 'action seen' flag
//!   as false, so the decoder returns 'no correction' for every decision) and compare with the original
//!   stream byte-for-byte. Two parameter choices: (a) zlib level 6 (lazy good 8 / lazy 16, chain 128,
//!   nice 128, TOO_FAR 4096) at memLevel 1-9, (b) the stream's own estimated parameters.
use std::fs;
use std::path::Path;

use preflate_rs::{
    preflate_whole_deflate_stream, recreate_whole_deflate_stream, HashAlgorithm, PreflateConfig,
    TokenPredictorParameters,
};

/// Mirrors preflate-rs 0.7.6's private ReconstructionData (same field order) to split off the CABAC blob.
#[derive(bitcode::Encode, bitcode::Decode)]
struct Rd {
    parameters: TokenPredictorParameters,
    corrections: Vec<u8>,
}

/// Returns None on error; a panic inside preflate-rs (seen in tree_predictor.rs with forced parameters)
/// is caught, counted in PANICS and also returned as None.
fn zero_correction_replay(plain: &[u8], p: TokenPredictorParameters) -> Option<Vec<u8>> {
    let blob = bitcode::encode(&Rd { parameters: p, corrections: vec![0u8; 64] });
    match std::panic::catch_unwind(|| recreate_whole_deflate_stream(plain, &blob).ok()) {
        Ok(v) => v,
        Err(_) => {
            PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            None
        }
    }
}

static PANICS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn zlib6(template: TokenPredictorParameters, mem: u32) -> TokenPredictorParameters {
    let mut p = template;
    let hash_bits = mem + 7;
    p.window_bits = 15;
    p.max_chain = 128;
    p.nice_length = 128;
    p.min_len = 3;
    p.zlib_compatible = true;
    p.max_dist_3_matches = 4096;
    p.very_far_matches_detected = false;
    p.matches_to_start_detected = false;
    p.max_token_count = ((1u32 << (6 + mem)) - 1) as u16;
    p.hash_algorithm = HashAlgorithm::Zlib { hash_mask: ((1u32 << hash_bits) - 1) as u16, hash_shift: (hash_bits + 2) / 3 };
    p
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: preflate_probe <sample-dir> [--replay]");
    let replay = std::env::args().nth(2).as_deref() == Some("--replay");
    let index = fs::read_to_string(Path::new(&dir).join("index.tsv")).expect("index.tsv");
    let config = PreflateConfig::default();
    let mut rows: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    for line in index.lines().filter(|l| !l.is_empty()) {
        let f: Vec<String> = line.split('\t').map(String::from).collect();
        let z = fs::read(Path::new(&dir).join(format!("{}.z", f[0]))).unwrap();
        rows.push((f, z[2..z.len() - 4].to_vec()));
    }
    if !replay {
        println!("name\tproducer\tzlib_replay\tcomp_bytes\tstatus\trecreated_exact\tcorr_bytes\tcabac_bytes\tzlib_compatible\tparams");
        for (f, deflate) in &rows {
            let row = match preflate_whole_deflate_stream(deflate, &config) {
                Err(e) => format!("{:?}\t0\t0\t0\t-\t-", e.exit_code()),
                Ok((r, plain)) => {
                    let exact = recreate_whole_deflate_stream(plain.text(), &r.corrections)
                        .map(|v| v == deflate[..r.compressed_size] && r.compressed_size == deflate.len())
                        .unwrap_or(false);
                    let rd: Rd = bitcode::decode(&r.corrections).expect("decode reconstruction data");
                    let p = rd.parameters;
                    format!(
                        "ok\t{}\t{}\t{}\t{}\twin={} strat={:?} hash={:?} chain={} nice={} match={:?} add={:?} blocks={:?} far={} start={}",
                        exact as u8, r.corrections.len(), rd.corrections.len(), p.zlib_compatible, p.window_bits,
                        p.strategy, p.hash_algorithm, p.max_chain, p.nice_length, p.matching_type, p.add_policy,
                        p.block_type_strategy, p.very_far_matches_detected, p.matches_to_start_detected
                    )
                }
            };
            println!("{}\t{}\t{}\t{}\t{}", f[0], f[1], f[2], deflate.len(), row);
        }
        return;
    }
    // --replay: a template carrying the zlib level 4-9 enum values (lazy 8/16, AddAll, Default, Dynamic),
    // taken from the first stream whose estimate has them; numeric fields are then set explicitly.
    let mut template = None;
    for (_, deflate) in &rows {
        if let Ok((r, _)) = preflate_whole_deflate_stream(deflate, &config) {
            let p = r.parameters.unwrap();
            let d = format!("{:?}", p);
            if d.contains("Lazy { good_length: 8, max_lazy: 16 }") && d.contains("AddAll")
                && d.contains("strategy: Default") && d.contains("block_type_strategy: Dynamic") {
                template = Some(p);
                break;
            }
        }
    }
    let template = template.expect("no zlib-6-like template found");
    std::panic::set_hook(Box::new(|_| {}));
    println!("name\tproducer\tzlib_replay\tcomp_bytes\tzlib6_exact_mem\town_params_exact");
    for (f, deflate) in &rows {
        let Ok((r, plain)) = preflate_whole_deflate_stream(deflate, &config) else {
            println!("{}\t{}\t{}\t{}\terr\terr", f[0], f[1], f[2], deflate.len());
            continue;
        };
        let mut hit = 0u32;
        for mem in [8u32, 7, 9, 6, 5, 4, 3, 2, 1] {
            if zero_correction_replay(plain.text(), zlib6(template, mem)).as_deref() == Some(&deflate[..]) {
                hit = mem;
                break;
            }
        }
        let own = zero_correction_replay(plain.text(), r.parameters.unwrap()).as_deref() == Some(&deflate[..]);
        println!("{}\t{}\t{}\t{}\t{}\t{}", f[0], f[1], f[2], deflate.len(), hit, own as u8);
    }
    println!("# preflate-rs panics caught: {}", PANICS.load(std::sync::atomic::Ordering::Relaxed));
}
