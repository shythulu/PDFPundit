// C9 single-byte-substitution search benchmark on REPDF corpus streams.
// For each case: corrupted zlib stream (.z), original decoded bytes (.orig), meta (true offset).
// Strategy: checkpoint DecompressorOxide every CK input bytes on the corrupted stream, then for
// candidate positions p (ordered) and all 255 replacement values v, resume from the nearest
// checkpoint <= p and decode to completion; accept on TINFLStatus::Done (Adler-32 verified).
use miniz_oxide::inflate::core::{decompress, inflate_flags::*, DecompressorOxide};
use miniz_oxide::inflate::TINFLStatus;
use std::time::{Duration, Instant};

const CK: usize = 256;

struct Ckpt { in_off: usize, out_pos: usize, r: DecompressorOxide }

fn full(data: &[u8], out: &mut Vec<u8>) -> (TINFLStatus, usize, usize) {
    let mut r = DecompressorOxide::new();
    let flags = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    decompress(&mut r, data, out, 0, flags)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let mode = args[2].as_str(); // "err" or "adler"
    let window: usize = args[3].parse().unwrap();
    let max_clen: usize = args[4].parse().unwrap();
    let budget = Duration::from_secs_f64(args[5].parse().unwrap());
    let min_clen: usize = args.get(6).map(|x| x.parse().unwrap()).unwrap_or(0);
    let stride: usize = args.get(7).map(|x| x.parse().unwrap()).unwrap_or(1);
    let phase: usize = args.get(8).map(|x| x.parse().unwrap()).unwrap_or(0);
    let mut seen = 0usize;
    let mut ids: Vec<String> = std::fs::read_dir(dir).unwrap()
        .filter_map(|e| { let p = e.unwrap().path(); if p.extension().map(|x| x == "z").unwrap_or(false) { Some(p.file_stem().unwrap().to_string_lossy().to_string()) } else { None } })
        .collect();
    ids.sort();
    let (mut n, mut fixed, mut exact, mut timeouts, mut notfound) = (0, 0, 0, 0, 0);
    let mut times = vec![]; let mut cands_total: u64 = 0; let mut decode_times = vec![];
    for id in ids {
        let meta = std::fs::read_to_string(format!("{dir}/{id}.meta")).unwrap();
        let status = if meta.contains("\"status\": \"err\"") { "err" } else if meta.contains("\"status\": \"adler\"") { "adler" } else { "trunc" };
        if status != mode { continue; }
        let off: usize = meta.split("\"off\": ").nth(1).unwrap().split(',').next().unwrap().trim().parse().unwrap();
        let data = std::fs::read(format!("{dir}/{id}.z")).unwrap();
        if data.len() > max_clen || data.len() <= min_clen { continue; }
        seen += 1; if seen % stride != phase { continue; }
        let orig = std::fs::read(format!("{dir}/{id}.orig")).unwrap();
        n += 1;
        let cap = orig.len() * 4 + (1 << 20);
        let mut out = vec![0u8; cap];
        let t0 = Instant::now();
        let (st, consumed, _w) = full(&data, &mut out);
        decode_times.push(t0.elapsed().as_secs_f64());
        let k = consumed; // input bytes consumed at failure / end
        // checkpoints on the corrupted stream
        let t = Instant::now();
        let flags = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
        let mut cks: Vec<Ckpt> = vec![Ckpt { in_off: 0, out_pos: 0, r: DecompressorOxide::new() }];
        {
            let mut r = DecompressorOxide::new();
            let (mut i, mut o) = (0usize, 0usize);
            while i < data.len() {
                let end = (i + CK).min(data.len());
                let (s, c, w) = decompress(&mut r, &data[i..end], &mut out, o, flags | TINFL_FLAG_HAS_MORE_INPUT);
                i += c; o += w;
                if s != TINFLStatus::NeedsMoreInput { break; }
                cks.push(Ckpt { in_off: i, out_pos: o, r: r.clone() });
            }
        }
        let base_out = out.clone(); // corrupted decode (prefix is correct up to the damage)
        // candidate positions
        let hi = (k + 8).min(data.len());
        let flagp = format!("{dir}/{id}.flag");
        let (lo, hi) = if mode == "err" { (k.saturating_sub(window), hi) }
            else if window > 0 { match std::fs::read_to_string(&flagp) { Ok(fs) => { let fl: usize = fs.trim().parse().unwrap(); (fl.saturating_sub(window), (fl + 8).min(data.len().saturating_sub(4))) }, Err(_) => { n -= 1; continue; } } }
            else { (0, data.len().saturating_sub(4)) };
        let positions: Vec<usize> = (lo..hi).rev().collect();
        let mut found: Option<(usize, u8)> = None; let mut tried: u64 = 0; let mut timed_out = false;
        let mut cand = data.clone();
        let mut last_ck_idx = usize::MAX;
        'outer: for &p in &positions {
            // nearest checkpoint with in_off <= p
            let ci = match cks.binary_search_by(|c| c.in_off.cmp(&p)) { Ok(x) => x, Err(x) => x - 1 };
            if ci != last_ck_idx { out[..].copy_from_slice(&base_out[..]); last_ck_idx = ci; }
            let ck = &cks[ci];
            let orig_byte = data[p];
            for v in 0u16..256 {
                let v = v as u8; if v == orig_byte { continue; }
                cand[p] = v; tried += 1;
                let mut r = ck.r.clone();
                let (s, _c, w) = decompress(&mut r, &cand[ck.in_off..], &mut out, ck.out_pos, flags);
                if s == TINFLStatus::Done {
                    found = Some((p, v));
                    let ok = ck.out_pos + w == orig.len() && out[..orig.len()] == orig[..];
                    if ok { exact += 1; }
                    cand[p] = orig_byte;
                    break 'outer;
                }
                if tried % 4096 == 0 && t.elapsed() > budget { timed_out = true; cand[p] = orig_byte; break 'outer; }
            }
            cand[p] = orig_byte;
        }
        cands_total += tried;
        let el = t.elapsed().as_secs_f64();
        times.push(el);
        match found { Some((p, _)) => { fixed += 1; if p != off { eprintln!("{id}: accepted p={p} true={off}"); } }
                      None => { if timed_out { timeouts += 1 } else { notfound += 1 } } }
        let _ = st;
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    decode_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |v: &Vec<f64>, f: f64| if v.is_empty() { 0.0 } else { v[((v.len() - 1) as f64 * f) as usize] };
    println!("mode={mode} window={window} max_clen={max_clen} cases={n} fixed={fixed} exact_match={exact} timeouts={timeouts} notfound={notfound}");
    println!("search time s: p50={:.4} p90={:.4} p99={:.4} max={:.4} total={:.2}", q(&times, 0.5), q(&times, 0.9), q(&times, 0.99), q(&times, 1.0), times.iter().sum::<f64>());
    println!("candidates tried total={cands_total} ; full decode time s p50={:.6} max={:.6}", q(&decode_times, 0.5), q(&decode_times, 1.0));
}
