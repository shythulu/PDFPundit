//! For each (raw, original zlib stream, memLevel, strategy) in the sample directory, re-encode `raw`
//! with (a) zlib-rs via its zlib-compatible C API (deflateInit2_, level 6, wbits 15, same memLevel and
//! strategy) and (b) miniz_oxide level 6 (zlib wrapper), and count exact byte matches with the original.
use std::ffi::c_int;
use std::fs;
use std::path::Path;

use libz_rs_sys::{deflate, deflateEnd, deflateInit2_, z_stream, zlibVersion, Z_FINISH, Z_STREAM_END};

fn zlib_rs(raw: &[u8], mem: c_int, strat: c_int) -> Vec<u8> {
    let mut out = vec![0u8; raw.len() + raw.len() / 100 + 1024];
    unsafe {
        let mut s: z_stream = std::mem::zeroed();
        let r = deflateInit2_(
            &mut s, 6, 8, 15, mem, strat, zlibVersion(), std::mem::size_of::<z_stream>() as c_int,
        );
        assert_eq!(r, 0, "deflateInit2_ failed");
        s.next_in = raw.as_ptr() as *mut u8;
        s.avail_in = raw.len() as _;
        s.next_out = out.as_mut_ptr();
        s.avail_out = out.len() as _;
        let r = deflate(&mut s, Z_FINISH);
        assert_eq!(r, Z_STREAM_END, "deflate did not finish");
        out.truncate(s.total_out as usize);
        deflateEnd(&mut s);
    }
    out
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: rust_replay <sample-dir>");
    let index = fs::read_to_string(Path::new(&dir).join("index.tsv")).expect("index.tsv");
    let (mut n, mut zrs_ok, mut mz_ok, mut zrs_rt, mut zrs_bytes, mut z_bytes) = (0, 0, 0, 0, 0usize, 0usize);
    let dump = std::env::args().nth(2); // optional: directory to write zlib-rs outputs into
    let (mut zrs_ok_by_mem, mut n_by_mem) = ([0usize; 10], [0usize; 10]);
    for line in index.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, mem, strat): (&str, c_int, c_int) = (f[0], f[1].parse().unwrap(), f[2].parse().unwrap());
        let raw = fs::read(Path::new(&dir).join(format!("{name}.raw"))).unwrap();
        let z = fs::read(Path::new(&dir).join(format!("{name}.z"))).unwrap();
        n += 1;
        n_by_mem[mem as usize] += 1;
        let zr = zlib_rs(&raw, mem, strat);
        zrs_bytes += zr.len();
        z_bytes += z.len();
        if miniz_oxide::inflate::decompress_to_vec_zlib(&zr).map(|d| d == raw).unwrap_or(false) {
            zrs_rt += 1;
        }
        if let Some(d) = &dump {
            fs::write(Path::new(d).join(format!("{name}.zrs")), &zr).unwrap();
        }
        if zr == z {
            zrs_ok += 1;
            zrs_ok_by_mem[mem as usize] += 1;
        }
        if miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6) == z {
            mz_ok += 1;
        }
    }
    let ver = unsafe { std::ffi::CStr::from_ptr(zlibVersion()) }.to_string_lossy().into_owned();
    println!("libz-rs-sys zlibVersion() = {ver}");
    println!("streams {n}: zlib-rs exact {zrs_ok}, miniz_oxide(level 6) exact {mz_ok}");
    println!("zlib-rs round-trips {zrs_rt}/{n}; total bytes zlib-rs {zrs_bytes} vs original {z_bytes}");
    for m in 1..10 {
        if n_by_mem[m] > 0 {
            println!("  memLevel {m}: zlib-rs exact {}/{}", zrs_ok_by_mem[m], n_by_mem[m]);
        }
    }
}
