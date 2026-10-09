// fontations_probe (Stage 4 verifier, TOOL-465): read the 'name' table family of embedded TrueType programs
// (FontFile2 streams extracted by ../fontations_names.py) with read-fonts 0.45.0 and skrifa 0.48.0.
// Usage: fontations_probe FILE...   One TSV line per file:
//   path  ok|err  read_fonts_family(nameID 1; Windows 3/1/0x409, else Mac 1/0/0, else first)  skrifa_family  num_glyphs
use read_fonts::{FontRef, TableProvider};
use skrifa::{string::StringId, MetadataProvider};

fn rf_family(font: &FontRef) -> Option<String> {
    let name = font.name().ok()?;
    let data = name.string_data();
    let mut best: Option<(u8, String)> = None;
    for rec in name.name_record() {
        if rec.name_id() != StringId::FAMILY_NAME {
            continue;
        }
        let rank = match (rec.platform_id(), rec.encoding_id(), rec.language_id()) {
            (3, 1, 0x409) => 0,
            (1, 0, 0) => 1,
            _ => 2,
        };
        if let Ok(s) = rec.string(data) {
            let s = s.to_string();
            if best.as_ref().map_or(true, |(r, _)| rank < *r) {
                best = Some((rank, s));
            }
        }
    }
    best.map(|(_, s)| s)
}

fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).expect("read");
        match FontRef::new(&data) {
            Ok(font) => {
                let rf = rf_family(&font).unwrap_or_default();
                let sk_font = skrifa::FontRef::new(&data).expect("skrifa parse");
                let sk = sk_font
                    .localized_strings(StringId::FAMILY_NAME)
                    .english_or_first()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let glyphs = font.maxp().map(|m| m.num_glyphs()).unwrap_or(0);
                println!("{path}\tok\t{rf}\t{sk}\t{glyphs}");
            }
            Err(e) => println!("{path}\terr\t\t\t{e}"),
        }
    }
}
