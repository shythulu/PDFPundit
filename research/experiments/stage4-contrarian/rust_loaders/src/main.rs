// Loader probe: opens each PDF (paths on stdin) with lopdf, pdfrum and hayro-syntax,
// and prints one TSV row per (file, library): ok, pages, non-whitespace text chars,
// repair-diagnostic count (pdfrum only), wall ms. Panics are caught and reported.
use std::io::BufRead;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

fn nonws(s: &str) -> usize { s.chars().filter(|c| !c.is_whitespace()).count() }

// If TEXT_DIR is set, the extracted text is also written to TEXT_DIR/<lib>/<file name>.txt.
fn dump(lib: &str, path: &str, text: &str) {
    if let Ok(dir) = std::env::var("TEXT_DIR") {
        let d = format!("{dir}/{lib}");
        let _ = std::fs::create_dir_all(&d);
        let name = path.rsplit('/').next().unwrap();
        let _ = std::fs::write(format!("{d}/{name}.txt"), text);
    }
}

fn row(path: &str, lib: &str, r: std::thread::Result<Result<(usize, i64, i64), String>>, ms: u128) {
    match r {
        Ok(Ok((pages, chars, diags))) => println!("{path}\t{lib}\tok\t{pages}\t{chars}\t{diags}\t{ms}\t"),
        Ok(Err(e)) => println!("{path}\t{lib}\terr\t0\t0\t-1\t{ms}\t{}", e.replace(['\t', '\n'], " ").chars().take(120).collect::<String>()),
        Err(_) => println!("{path}\t{lib}\tpanic\t0\t0\t-1\t{ms}\t"),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    println!("path\tlib\tstatus\tpages\ttext_chars\tdiagnostics\tms\terror");
    for line in std::io::stdin().lock().lines() {
        let path = line.unwrap();
        let bytes = std::fs::read(&path).unwrap();

        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let doc = lopdf::Document::load_mem(&bytes).map_err(|e| e.to_string())?;
            let nums: Vec<u32> = doc.get_pages().keys().copied().collect();
            let chars = match doc.extract_text(&nums) {
                Ok(s) => { dump("lopdf-0.45.0", &path, &s); nonws(&s) as i64 }
                Err(_) => -1,
            };
            Ok((nums.len(), chars, -1))
        }));
        row(&path, "lopdf-0.45.0", r, t.elapsed().as_millis());

        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let doc = pdfrum::Document::from_bytes(bytes.clone()).map_err(|e| e.to_string())?;
            let n = doc.page_count() as usize;
            let mut chars = 0i64;
            let mut all = String::new();
            for p in doc.pages() { let t = p.text().to_string(); chars += nonws(&t) as i64; all.push_str(&t); all.push('\n'); }
            dump("pdfrum-0.4.0", &path, &all);
            Ok((n, chars, doc.all_diagnostics().recorded() as i64))
        }));
        row(&path, "pdfrum-0.4.0", r, t.elapsed().as_millis());

        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let pdf = hayro_syntax::Pdf::new(bytes.clone()).map_err(|e| format!("{e:?}"))?;
            Ok((pdf.pages().len(), -1, -1))
        }));
        row(&path, "hayro-syntax-0.8.0", r, t.elapsed().as_millis());
    }
}
