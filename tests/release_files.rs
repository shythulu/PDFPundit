//! Release packaging wiring (T-35, G-08): the files the release path names exist,
//! and the guard's legs come from the dist plan rather than a hard-coded list.
//! `tools/release-guard-matrix.py --self-test` covers the matrix rules themselves.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// Every `.github/workflows/<name>.yml` that Cargo.toml mentions, in order.
fn workflow_mentions(text: &str) -> Vec<String> {
    const PREFIX: &str = ".github/workflows/";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || "./-_".contains(c)))
            .unwrap_or(tail.len());
        out.push(tail[..end].trim_end_matches('.').to_string());
        rest = &tail[end..];
    }
    out
}

#[test]
fn every_workflow_cargo_toml_names_exists() {
    let mentions = workflow_mentions(&read("Cargo.toml"));
    assert!(
        !mentions.is_empty(),
        "Cargo.toml names the release workflow"
    );
    for m in mentions {
        assert!(
            repo().join(&m).is_file(),
            "Cargo.toml names {m}, which does not exist"
        );
    }
}

#[test]
fn msvc_artefact_keeps_the_static_crt() {
    let manifest: toml::Table = toml::from_str(&read("Cargo.toml")).expect("Cargo.toml parses");
    let dist = &manifest["workspace"]["metadata"]["dist"];
    assert_eq!(
        dist.get("msvc-crt-static").and_then(toml::Value::as_bool),
        Some(true)
    );
}

#[test]
fn guard_matrix_comes_from_the_plan() {
    let guard = read(".github/workflows/release-guard.yml");
    assert!(guard.contains("python -I tools/release-guard-matrix.py --self-test"));
    assert!(guard.contains("PLAN: ${{ inputs.plan }}"));
    assert!(guard.contains("python -I tools/release-guard-matrix.py >> \"$GITHUB_OUTPUT\""));
    assert!(guard.contains("matrix: ${{ fromJson(needs.matrix.outputs.matrix) }}"));
    assert!(
        !guard.contains("archive: pdfpundit-"),
        "release-guard.yml still hard-codes an archive"
    );
    assert!(repo().join("tools/release-guard-matrix.py").is_file());
}

#[test]
fn ci_runs_the_matrix_self_test() {
    let ci = read(".github/workflows/ci.yml");
    assert!(ci.contains("python3 -I tools/release-guard-matrix.py --self-test"));
}
