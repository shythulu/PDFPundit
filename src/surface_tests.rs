//! What a dependant of this crate can reach (D-047): the facade gate and the
//! public-surface tripwire. Both run a nested cargo with its own target directory,
//! so they never wait on the lock of the build that runs them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const REPO: &str = env!("CARGO_MANIFEST_DIR");

/// The nested builds' target directory, beside the test binary's profile
/// directory (`target/<profile>/pdfpundit-gate`) so it is reused between runs and
/// removed by `cargo clean`.
fn gate_target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test binary path");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("test binary sits in <target>/<profile>/deps");
    profile_dir.join("pdfpundit-gate")
}

/// A cargo command that inherits nothing from the outer build that could change
/// what is compiled: every rustflags source and the target directory are cleared.
fn cargo(rustflags: Option<&str>) -> Command {
    let exe = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(exe);
    for var in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
        "CARGO_BUILD_RUSTDOCFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_BUILD_TARGET_DIR",
    ] {
        cmd.env_remove(var);
    }
    cmd.env("CARGO_TARGET_DIR", gate_target_dir());
    if let Some(flags) = rustflags {
        cmd.env("RUSTFLAGS", flags);
    }
    cmd
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("spawn cargo")
}

fn text(out: &Output) -> String {
    format!(
        "status: {}\n--- stdout\n{}\n--- stderr\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A dependant crate under the system temp directory, never under the repo:
/// Cargo reads `.cargo/config.toml` upward from the cwd, so a dependant inside the
/// repo would inherit any config the repo ever gains and pass for the wrong reason.
struct Dependant {
    dir: PathBuf,
}

impl Dependant {
    fn create() -> Self {
        let dir =
            std::env::temp_dir().join(format!("pdfpundit-facade-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("create dependant");
        // A TOML literal string takes a Windows path without escaping.
        let manifest = format!(
            "[package]\nname = \"facade-gate\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
             [workspace]\n\n[dependencies]\npdfpundit = {{ path = '{REPO}' }}\n"
        );
        std::fs::write(dir.join("Cargo.toml"), manifest).expect("write manifest");
        // Same versions as this crate's lock, so the check needs no registry access.
        std::fs::copy(Path::new(REPO).join("Cargo.lock"), dir.join("Cargo.lock"))
            .expect("copy lock");
        Dependant { dir }
    }

    fn check(&self, main_rs: &str, rustflags: Option<&str>) -> Output {
        std::fs::write(self.dir.join("src/main.rs"), main_rs).expect("write main.rs");
        run(cargo(rustflags).current_dir(&self.dir).args([
            "check",
            "--offline",
            "--quiet",
            "--color",
            "never",
        ]))
    }
}

impl Drop for Dependant {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn a_dependant_reaches_run_and_not_the_engine() {
    let dep = Dependant::create();
    for rustflags in [None, Some("-Dwarnings")] {
        let ok = dep.check(
            "fn main() -> std::process::ExitCode {\n    pdfpundit::run()\n}\n",
            rustflags,
        );
        assert!(
            ok.status.success(),
            "pdfpundit::run must resolve (RUSTFLAGS={rustflags:?})\n{}",
            text(&ok)
        );

        let bad = dep.check(
            "#[allow(unused_imports)]\nuse pdfpundit::engine::analyze;\n\nfn main() {}\n",
            rustflags,
        );
        let stderr = String::from_utf8_lossy(&bad.stderr);
        assert!(
            !bad.status.success(),
            "the engine must be unreachable (RUSTFLAGS={rustflags:?})\n{}",
            text(&bad)
        );
        // E0432 (unresolved import) while `engine` has no `analyze`; E0603 (private
        // module) once T-02b defines it. Either way the error is about `engine`.
        assert!(
            (stderr.contains("error[E0432]") || stderr.contains("error[E0603]"))
                && stderr.contains("engine"),
            "expected E0432 or E0603 on `pdfpundit::engine` (RUSTFLAGS={rustflags:?})\n{}",
            text(&bad)
        );
    }
}

/// Item paths (`pdf::model::Finding`) listed in rustdoc's `all.html`.
fn documented_items(all_html: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = all_html;
    while let Some(start) = rest.find("<ul class=\"all-items\">") {
        let list = &rest[start..];
        let end = list.find("</ul>").expect("closed item list");
        let mut body = &list[..end];
        while let Some(at) = body.find("href=\"") {
            body = &body[at + 6..];
            let close = body.find('"').expect("closed href");
            let href = &body[..close];
            let mut parts: Vec<&str> = href.split('/').collect();
            let file = parts.pop().expect("file name");
            // `struct.Finding.html` -> `Finding`
            let name = file
                .strip_suffix(".html")
                .and_then(|f| f.split_once('.'))
                .map(|(_, n)| n)
                .expect("rustdoc item file name");
            parts.push(name);
            items.push(parts.join("::"));
            body = &body[close..];
        }
        rest = &list[end..];
    }
    items.sort();
    items
}

fn allowed(item: &str) -> bool {
    item == "run"
        || item == "pdf::fontdb::build::build_from_ttf"
        || ["pdf::model::", "pdf::fixtures::", "pdf::write::"]
            .iter()
            .any(|prefix| item.starts_with(prefix))
}

#[test]
fn public_surface_is_run_and_the_pdf_tool_modules() {
    let out = run(cargo(None).current_dir(REPO).args([
        "doc",
        "--lib",
        "--no-deps",
        "--all-features",
        "--locked",
        "--offline",
        "--quiet",
        "--color",
        "never",
    ]));
    assert!(out.status.success(), "cargo doc failed\n{}", text(&out));

    let all = gate_target_dir().join("doc/pdfpundit/all.html");
    let html = std::fs::read_to_string(&all).expect("read all.html");
    let items = documented_items(&html);
    assert!(
        items.iter().any(|i| i == "run"),
        "run() must be public: {items:?}"
    );
    let extra: Vec<&String> = items.iter().filter(|i| !allowed(i)).collect();
    assert!(
        extra.is_empty(),
        "items outside the public surface: {extra:?}"
    );
}

#[test]
fn documented_items_reads_rustdoc_item_lists() {
    let html = "<a href=\"index.html\">x</a>\
        <h3 id=\"structs\">Structs</h3><ul class=\"all-items\">\
        <li><a href=\"pdf/model/struct.Finding.html\">pdf::model::Finding</a></li></ul>\
        <h3 id=\"functions\">Functions</h3><ul class=\"all-items\">\
        <li><a href=\"fn.run.html\">run</a></li>\
        <li><a href=\"engine/fn.analyze.html\">engine::analyze</a></li></ul>";
    let items = documented_items(html);
    assert_eq!(items, ["engine::analyze", "pdf::model::Finding", "run"]);
    assert!(allowed("run") && allowed("pdf::model::Finding"));
    assert!(allowed("pdf::fontdb::build::build_from_ttf"));
    assert!(!allowed("engine::analyze") && !allowed("pdf::fontdb::build::other"));
}
