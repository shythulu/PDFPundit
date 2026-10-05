// Smoke-test probe for three Rust PDF crates. Usage:
//   rsprobe lopdf <in.pdf> <out.pdf>   -> lenient load + save; prints pages/objects
//   rsprobe pdfrs <in.pdf>             -> tolerant open; prints pages and decodable content streams
//   rsprobe hayro <in.pdf> <out.png>   -> load + render every page at 1x; writes page 1 PNG
use std::env;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = env::args().collect();
    match a[1].as_str() {
        "lopdf" => {
            match lopdf::Document::load(&a[2]) {
                Ok(mut doc) => {
                    let pages = doc.get_pages().len();
                    let objs = doc.objects.len();
                    match doc.save(&a[3]) {
                        Ok(_) => println!("lopdf ok pages={pages} objects={objs}"),
                        Err(e) => { println!("lopdf save-error {e}"); std::process::exit(2) }
                    }
                }
                Err(e) => { println!("lopdf load-error {e}"); std::process::exit(1) }
            }
        }
        "pdfrs" => {
            let f = pdf::file::FileOptions::cached()
                .parse_options(pdf::object::ParseOptions::tolerant())
                .open(&a[2]);
            match f {
                Ok(file) => {
                    let n = file.num_pages();
                    let mut ok = 0;
                    for p in file.pages() {
                        if let Ok(p) = p {
                            if let Some(c) = &p.contents {
                                if c.operations(&file.resolver()).is_ok() { ok += 1; }
                            }
                        }
                    }
                    println!("pdfrs ok pages={n} pages_with_decodable_content={ok}");
                }
                Err(e) => { println!("pdfrs open-error {e}"); std::process::exit(1) }
            }
        }
        "hayro" => {
            let data = std::fs::read(&a[2]).unwrap();
            match hayro::hayro_syntax::Pdf::new(Arc::new(data)) {
                Ok(pdf) => {
                    let cache = hayro::RenderCache::new();
                    let is = hayro::hayro_interpret::InterpreterSettings::default();
                    let rs = hayro::RenderSettings { x_scale: 1.0, y_scale: 1.0,
                        bg_color: hayro::vello_cpu::color::palette::css::WHITE, ..Default::default() };
                    let pages = pdf.pages();
                    let n = pages.len();
                    for (i, page) in pages.iter().enumerate() {
                        let pix = hayro::render(page, &cache, &is, &rs);
                        if i == 0 { std::fs::write(&a[3], pix.into_png().unwrap()).unwrap(); }
                    }
                    println!("hayro ok pages={n}");
                }
                Err(e) => { println!("hayro load-error {e:?}"); std::process::exit(1) }
            }
        }
        _ => panic!("mode"),
    }
}
