//! Notices completeness (T-35): every data file the binary can carry is named in
//! `THIRD_PARTY_NOTICES.md` by its repo path.
//!
//! Two sets are checked. Every file under `assets/`, and every non-Rust target of
//! an `include_bytes!` or `include_str!` in `src/` outside test-only code. Code is
//! test-only when its module, or a module above it, is declared under
//! `#[cfg(test)]`, or when it sits inside a top-level `#[cfg(test)] mod x { … }`
//! block (rustfmt puts that block's closing brace alone at column 0). Anything
//! the scan cannot place counts as shipped, so a miss fails loudly.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn notices() -> String {
    std::fs::read_to_string(repo().join("THIRD_PARTY_NOTICES.md")).expect("read notices")
}

/// Repo-relative paths with `/` separators, sorted.
/// Every file under `dir`, skipping dot-prefixed entries. The repo tracks no
/// dotfiles under `assets/` or `src/`, and a local OS file such as macOS's
/// gitignored `.DS_Store` must not fail the check on one machine only.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read dir") {
            let entry = entry.expect("entry");
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(rel(&path));
            }
        }
    }
    out.sort();
    out
}

fn rel(path: &Path) -> String {
    let r = path.strip_prefix(repo()).expect("path under the repo");
    let parts: Vec<String> = r
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}

/// `a/b/../c` → `a/c`, on `/`-separated relative paths.
fn normalise(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop().expect("include path escapes the repo");
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// Each `include_bytes!`/`include_str!` in `source`: (1-based line, literal path).
/// A non-literal argument is an error: the scan cannot see where it points.
fn includes(source: &str) -> Result<Vec<(usize, String)>, String> {
    let mut out = Vec::new();
    for mac in ["include_bytes!(", "include_str!("] {
        let mut from = 0;
        while let Some(at) = source[from..].find(mac) {
            let start = from + at;
            let line = source[..start].matches('\n').count() + 1;
            let rest = source[start + mac.len()..].trim_start();
            let Some(lit) = rest.strip_prefix('"') else {
                return Err(format!("line {line}: {mac} without a string literal"));
            };
            let end = lit
                .find('"')
                .ok_or(format!("line {line}: unterminated literal"))?;
            out.push((line, lit[..end].to_string()));
            from = start + mac.len();
        }
    }
    out.sort();
    Ok(out)
}

/// 1-based line ranges of top-level `#[cfg(test)] mod x { … }` blocks.
fn inline_test_blocks(source: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] == "#[cfg(test)]" {
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with("#[") {
                j += 1;
            }
            if j < lines.len() && is_inline_mod(lines[j]) {
                let end = (j + 1..lines.len())
                    .find(|&k| lines[k] == "}")
                    .unwrap_or(lines.len() - 1);
                out.push((i + 1, end + 1));
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn is_inline_mod(line: &str) -> bool {
    let l = line
        .strip_prefix("pub(crate) ")
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line);
    l.strip_prefix("mod ")
        .and_then(|r| r.strip_suffix(" {"))
        .is_some_and(|name| name.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

/// Whether `mod <name>;` in `parent` is declared under `#[cfg(test)]`. `None` if
/// `parent` declares no such module.
fn declared_test_only(parent: &str, name: &str) -> Option<bool> {
    let lines: Vec<&str> = parent.lines().collect();
    let decl = format!("mod {name};");
    let at = lines.iter().position(|l| {
        let t = l.trim_start();
        let t = t
            .strip_prefix("pub(crate) ")
            .or_else(|| t.strip_prefix("pub "))
            .unwrap_or(t);
        t == decl
    })?;
    let mut k = at;
    while k > 0 && lines[k - 1].trim_start().starts_with("#[") {
        k -= 1;
        if lines[k].trim() == "#[cfg(test)]" {
            return Some(true);
        }
    }
    Some(false)
}

/// Whether the module file `file` (absolute, under `src/`) is test-only.
fn module_test_only(file: &Path) -> bool {
    let src = repo().join("src");
    if file == src.join("lib.rs") || file == src.join("main.rs") {
        return false;
    }
    let (dir, name) = if file.file_name().is_some_and(|n| n == "mod.rs") {
        let dir = file.parent().expect("mod.rs dir");
        (
            dir.parent().expect("dir parent").to_path_buf(),
            dir.file_name()
                .expect("dir name")
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        (
            file.parent().expect("file dir").to_path_buf(),
            file.file_stem()
                .expect("stem")
                .to_string_lossy()
                .into_owned(),
        )
    };
    let parent = if dir == src {
        src.join("lib.rs")
    } else if dir.with_extension("rs").is_file() {
        dir.with_extension("rs")
    } else {
        dir.join("mod.rs")
    };
    let text = std::fs::read_to_string(&parent)
        .unwrap_or_else(|e| panic!("{}: parent of {}: {e}", rel(&parent), rel(file)));
    match declared_test_only(&text, &name) {
        Some(true) => true,
        Some(false) => module_test_only(&parent),
        None => panic!(
            "{} declares no `mod {name};` for {}",
            rel(&parent),
            rel(file)
        ),
    }
}

/// Non-Rust include targets in shipped code: repo path → first including site.
fn shipped_include_targets() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for file in files_under(&repo().join("src")) {
        if !file.ends_with(".rs") {
            continue;
        }
        let path = repo().join(&file);
        let source = std::fs::read_to_string(&path).expect("read source");
        let found = includes(&source).unwrap_or_else(|e| panic!("{file}: {e}"));
        if found.is_empty() || module_test_only(&path) {
            continue;
        }
        let blocks = inline_test_blocks(&source);
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        for (line, target) in found {
            if target.ends_with(".rs") || blocks.iter().any(|&(a, b)| a <= line && line <= b) {
                continue;
            }
            out.push((
                normalise(&format!("{dir}/{target}")),
                format!("{file}:{line}"),
            ));
        }
    }
    out.sort();
    out
}

#[test]
fn every_asset_is_named_in_the_notices() {
    let notices = notices();
    let missing: Vec<String> = files_under(&repo().join("assets"))
        .into_iter()
        .filter(|f| !notices.contains(f.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "THIRD_PARTY_NOTICES.md does not name: {missing:?}"
    );
}

#[test]
fn every_shipped_include_target_is_named_in_the_notices() {
    let notices = notices();
    let targets = shipped_include_targets();
    // The scan sees the bundled fonts, the word lists and the palette; an
    // empty or tiny set means it broke, not that the notices are complete.
    for known in [
        "assets/fonts/NotoSans-Regular.ttf",
        "assets/dicts/en.txt",
        "assets/dicts/fr.txt",
        "assets/dicts/es.txt",
        "assets/theme/darkberry-palette.json",
    ] {
        assert!(
            targets.iter().any(|(t, _)| t == known),
            "scan missed {known}: {targets:?}"
        );
    }
    for (target, _) in &targets {
        assert!(
            repo().join(target).is_file(),
            "{target} is not a file under the repo"
        );
    }
    let missing: Vec<&(String, String)> = targets
        .iter()
        .filter(|(t, _)| !notices.contains(t.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "THIRD_PARTY_NOTICES.md does not name these include targets (path, site): {missing:?}"
    );
}

/// The archive ships this file but not `assets/`, so each licence text there is
/// quoted in full, unchanged (line endings aside: a Windows checkout may convert
/// the notices file).
#[test]
fn the_notices_quote_every_bundled_licence_text() {
    let notices = notices().replace("\r\n", "\n");
    for file in files_under(&repo().join("assets/licenses")) {
        let text = std::fs::read_to_string(repo().join(&file)).expect("read licence");
        let text = text.replace("\r\n", "\n");
        assert!(
            notices.contains(&format!("```\n{}\n```", text.trim_end_matches('\n'))),
            "THIRD_PARTY_NOTICES.md does not quote {file} verbatim"
        );
    }
}

/// The word lists are CC BY 4.0 (G-09): their section names the licence, the
/// source corpora and the changes made, which attribution requires.
#[test]
fn the_word_lists_carry_their_attribution() {
    let notices = notices();
    let at = notices
        .find("## Leipzig Corpora Collection word lists")
        .expect("a word-list section");
    let section = &notices[at..];
    let section = &section[..section[3..].find("\n## ").map_or(section.len(), |e| e + 3)];
    for needle in [
        "https://creativecommons.org/licenses/by/4.0/",
        "Creative Commons Attribution 4.0",
        "https://wortschatz.uni-leipzig.de/en/usage",
        "eng_news_2025_1M",
        "fra_news_2024_1M",
        "spa_news_2024_1M",
        "Changes made",
        "tools/cut-wordlist.py",
    ] {
        assert!(
            section.contains(needle),
            "the word-list section lacks {needle}"
        );
    }
}

#[test]
fn the_scan_skips_test_code_and_finds_the_rest() {
    let source = "\
const A: &[u8] = include_bytes!(\"../../assets/a.bin\");
const B: &str = include_str!(
    \"b.txt\"
);

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    const C: &str = include_str!(\"../c.json\");
    fn f() {
        let s = \"}\";
    }
}

pub(crate) mod later {
    const D: &[u8] = include_bytes!(\"d.bin\");
}
";
    assert_eq!(
        includes(source).unwrap(),
        vec![
            (1, "../../assets/a.bin".to_string()),
            (2, "b.txt".to_string()),
            (9, "../c.json".to_string()),
            (16, "d.bin".to_string()),
        ]
    );
    assert_eq!(inline_test_blocks(source), vec![(6, 13)]);
    assert!(includes("include_bytes!(concat!(\"a\", \"b\"))").is_err());
    assert_eq!(normalise("src/pdf/../../assets/x"), "assets/x");

    let parent = "\
pub(crate) mod shipped;
#[cfg(test)]
mod corpus;
#[cfg(test)]
#[path = \"x/y.rs\"]
mod pathed;
#[cfg(not(test))]
mod not_test;
";
    assert_eq!(declared_test_only(parent, "shipped"), Some(false));
    assert_eq!(declared_test_only(parent, "corpus"), Some(true));
    assert_eq!(declared_test_only(parent, "pathed"), Some(true));
    assert_eq!(declared_test_only(parent, "not_test"), Some(false));
    assert_eq!(declared_test_only(parent, "absent"), None);
}

#[test]
fn the_module_walk_places_known_files() {
    let src = repo().join("src");
    assert!(module_test_only(&src.join("bench/corpus.rs")));
    assert!(module_test_only(&src.join("engine/tests.rs")));
    assert!(!module_test_only(&src.join("pdf/fontdb/template.rs")));
    assert!(!module_test_only(&src.join("ui/theme.rs")));
}
