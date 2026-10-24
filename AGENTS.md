# PDFPundit

## Agent skills

### Issue tracker

Issues are tracked as GitHub Issues on `shythulu/PDFPundit`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage labels are used as-is: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Build

- **RUSTFLAGS:** nothing in this repo depends on `RUSTFLAGS` or on a `.cargo/config.toml`, and none may be added for correctness. Cargo's four rustflags sources are mutually exclusive (`CARGO_ENCODED_RUSTFLAGS`, then `RUSTFLAGS`, then `target.<triple>.rustflags`, then `build.rustflags`), so any environment `RUSTFLAGS` (a developer's `target-cpu`, CI's `-Dwarnings`, coverage tools, release tooling) silently replaces a config file's flags. A build with `RUSTFLAGS` set builds the same crate.
- **Gates** (all in `.github/workflows/ci.yml`, each runnable locally):
  - `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings`. `clippy.toml` bans the `std::time` types and every `f32`/`f64` method that lowers to platform libm, crate-wide.
  - `tools/lint-meta.sh` proves those bans fire.
  - `python3 -I tools/purity-gate.py` checks for no compiled or vendored C or assembly, per CI target. `--network` checks for no HTTP/TLS crate in `Cargo.lock`.
  - `cargo deny check licenses` enforces the dependency licence allow-list in `deny.toml`.

## Corpus

- REPDF corpus: set `PDFPUNDIT_CORPUS` to a local depth-1 clone of github.com/dfrc-korea/REPDF at e547d4d (plain blobs, no git-lfs needed, 1.5 GB on disk, no licence: never inside the repo, never in CI artifacts).
