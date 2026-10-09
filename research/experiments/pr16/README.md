# C9 / font-width experiments

Scripts behind the measured results (E1–E8) in
`../research_notes/Corrupted PDF repair beyond REPDF/deflate_and_stream_salvage.md`
and the width-entropy numbers in `font_and_text_recovery.md`. They were
written during the research pass and are kept as-run, not as polished tools.

Inputs are not checked in. Clone the REPDF dataset first:

```sh
git clone https://github.com/dfrc-korea/REPDF repdf-repo
```

| File | What it does | Run |
| --- | --- | --- |
| `c9exp.py` | Part A: diffs each C9 (`*_stream_zlib.pdf`) file against its original, locates the replaced bytes, and records how inflate fails (`partA.json`). Part B: synthetic random-byte replacements in the originals' Flate streams (`partB.json`). | `python3 c9exp.py repdf-repo [trials]` |
| `blocks.py` | Counts DEFLATE blocks and block types per original stream (the "one dynamic block per stream" result). | `python3 blocks.py` (expects `c9exp.py` and `repdf-repo/` beside it) |
| `grammar_loc.py` | Content-stream/CMap grammar localizer for streams that decode fully but fail Adler-32. | `python3 grammar_loc.py` (reads `cases/*.meta`, `*.z`, `*.orig`) |
| `mzbench/` | Rust benchmark: checkpointed 255-value single-byte search with `miniz_oxide`, accepting on Adler-32. | `cargo run --release -- <cases-dir> <err\|adler> <window> <max_clen> <budget_s> [min_clen] [stride] [phase]` |
| `szcheck/` | Prints the size of `miniz_oxide`'s decoder state (checkpoint cost). | `cargo run` |
| `widths.py` | Letter-uncertainty reduction from glyph advance widths (needs `fontTools`). Font paths are hardcoded to the research container's system fonts (DejaVu, Liberation, WenQuanYi); edit them for your machine. | `python3 widths.py` |

Known gap: the step that wrote the per-stream `cases/` directory (`<id>.z`
corrupted stream, `<id>.orig` original decoded bytes, `<id>.meta` true offset)
was not saved as a script. Rebuilding it from `partA.json` is straightforward
but has not been done.

`mzbench` was measured with `miniz_oxide` 0.8.9; the main crate pins 0.9, so
re-check that `DecompressorOxide` is still `Clone` there before relying on the
checkpoint approach.

## pr16-ingest patches and additions (2026-10-05)

The scripts above are PR #16's (merged as c7a6ad4, under
`nimbalyst-local/plans/research/experiments/`), copied here so observations OBS-1000..1015
can cite them. Corpus: REPDF at commit e547d4d (`/home/user/dfrc-korea/repdf`).
Every run used `nice -n 10` and `timeout 1200`; cargo builds used `-j2 --locked` with
`CARGO_TARGET_DIR` in the scratchpad.

**Patches. Each changes paths only; no logic changed. Each is marked `# PATCH (pr16-ingest …)` in the file.**

| File | Patch |
|---|---|
| `c9exp.py` | Outputs go to `$C9_OUT` (default: beside the script, as upstream); `$C9_SKIP_A=1` reuses an existing `partA.json` so Part B can be rerun alone. |
| `blocks.py` | Corpus root from `argv[1]` (upstream: `./repdf-repo`); `c9exp.py` loaded from the script's own directory. |
| `grammar_loc.py` | Cases directory from `argv[1]` (upstream: `./cases`). |
| `widths.py` | fontTools path from `$PYLIB` (upstream: `./pylib`). |
| `mzbench/`, `szcheck/` | Unchanged. `Cargo.lock` pins miniz_oxide 0.8.9. |

**New scripts**

| File | What it does |
|---|---|
| `build_cases.py` | Rebuilds the `cases/` directory (`<id>.z`, `<id>.orig`, `<id>.meta`) from `partA.json` and the corpus. It replaces PR #16's unsaved builder and yields 1,445 cases. |
| `c9_summarize.py` | Recomputes the E1–E3 numbers (OBS-1000..1003) from `partA.json`, `partB.json` and the corpus. Corrupted output is "every byte zlib emits before it raises" (`feed_keep`). |
| `run_mzbench.sh` | Fixes the four mzbench configurations: `err`, `small`, `grammar`, and `mid` (every 10th 4–16 KiB case). |
| `szcheck_versions.sh` | Builds szcheck and mzbench against miniz_oxide 0.8.9 and 0.9.1. |
| `crosscheck_obs0300.py` | Matches PR #16's damaged streams against `research/experiments/results/flate_c9_probe.csv` (OBS-0300). It finds the 3-stream EOL-trim undercount (OBS-1012). |
| `small_adler_audit.py` | Classifies the 245 small Adler-only cases: trailer damage, collision at the true offset, and one-trailer-byte Adler match (OBS-1013). |
| `headline_c9.py` | Recomputes PR #16's 39% headline from the mzbench outputs (OBS-1014). |

**Outputs and inputs**
- Results are in `results/`.
- The large intermediates stay in the scratchpad and are not committed: `partA.json`, `partB.json` and `cases/` (145 MB). The observations record their sha256 values or listing hashes.

**Findings that affect the original README**
- The cases builder now exists (`build_cases.py`).
- `DecompressorOxide` is still `Clone` in miniz_oxide 0.9.1, at the same size (10,504 bytes). mzbench builds unchanged against 0.9.1 (OBS-1005).
- The committed `mzbench` searches positions `0..clen-4`, which excludes the Adler-32 trailer. Its `small` result is therefore 243/245 accepted, 242 exact, not PR #16's 245/245, 243 exact (OBS-1007, OBS-1013).
