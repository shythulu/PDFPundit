//! For each <name>.raw / <name>.z pair in a sample directory (written by zbx_replay.py), re-encode the plaintext with the
//! patched pure-Rust zlib-bitexact-rs at level 6 and every memLevel 1..9, and report which memLevels give a raw DEFLATE
//! body byte-identical to the stored stream (zlib header and Adler-32 trailer stripped).
//! Output TSV: name, rust_exact_mems (comma list, or "-"), micros.
use std::{env, fs, path::Path, time::Instant};

fn main() {
    let dir = env::args().nth(1).expect("usage: zbx_replay <sample_dir>");
    let index = fs::read_to_string(Path::new(&dir).join("index.tsv")).expect("index.tsv");
    println!("name\trust_exact_mems\tmicros");
    for line in index.lines().filter(|l| !l.is_empty()) {
        let name = line.split('\t').next().unwrap();
        let raw = fs::read(Path::new(&dir).join(format!("{name}.raw"))).unwrap();
        let z = fs::read(Path::new(&dir).join(format!("{name}.z"))).unwrap();
        let body = &z[2..z.len() - 4];
        let t = Instant::now();
        let mut hits = Vec::new();
        for mem in 1..=9usize {
            if zlib_bitexact_rs::deflate_raw_params(&raw, 6, mem) == body {
                hits.push(mem.to_string());
            }
        }
        let us = t.elapsed().as_micros();
        let h = if hits.is_empty() { "-".to_string() } else { hits.join(",") };
        println!("{name}\t{h}\t{us}");
    }
}
