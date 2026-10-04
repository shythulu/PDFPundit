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
