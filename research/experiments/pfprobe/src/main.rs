// pfprobe: run preflate-rs 0.7.6 on zlib-wrapped streams (PDF /FlateDecode data).
// Usage: pfprobe FILE...   Each FILE holds one raw /FlateDecode stream (2-byte zlib header + deflate + Adler-32).
// Prints one tab-separated line per file:
//   path  ok|err  compressed_size  plain_len  corrections_len  roundtrip_exact  params-or-error
use preflate_rs::{preflate_whole_deflate_stream, recreate_whole_deflate_stream, PreflateConfig};
use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).expect("read");
        if data.len() < 3 {
            println!("{path}\terr\t0\t0\t0\tfalse\ttoo short");
            continue;
        }
        let deflate = &data[2..]; // skip the zlib header; trailing Adler-32 is simply not consumed
        let t = Instant::now();
        let cfg = PreflateConfig { verify_compression: false, ..PreflateConfig::default() };
        match preflate_whole_deflate_stream(deflate, &cfg) {
            Ok((res, plain)) => {
                let rt = recreate_whole_deflate_stream(plain.text(), &res.corrections)
                    .map(|v| v.as_slice() == &deflate[..res.compressed_size])
                    .unwrap_or(false);
                let p = res.parameters.map(|p| format!("{:?}", p)).unwrap_or_default();
                println!("{path}\tok\t{}\t{}\t{}\t{}\t{}ms {}", res.compressed_size, plain.text().len(),
                         res.corrections.len(), rt, t.elapsed().as_millis(), p);
            }
            Err(e) => println!("{path}\terr\t0\t0\t0\tfalse\t{:?} {}", e.exit_code(), e.message()),
        }
    }
}
