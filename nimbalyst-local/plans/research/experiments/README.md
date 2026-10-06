# C9 / font-width / outlined-text experiments

Scripts behind the measured results (E1–E8) in
`../research_notes/Corrupted PDF repair beyond REPDF/deflate_and_stream_salvage.md`,
the width-entropy numbers in `font_and_text_recovery.md`, and the outlined-text
and OCR numbers in `outlined_text_and_ocr.md`. They were written during the
research pass and are kept as-run, not as polished tools.

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
| `outline_census.py` | Per original: text-show operators, path segments, curves, fills, images, Form XObjects (walked), Type3 fonts. Shows Save As files have no curves and no file uses Type3. | `python3 outline_census.py repdf-repo` |
| `outline_loss.py` | Per document: non-whitespace characters extractable from the text layer, Save As vs Print to PDF, plus glyph-run paths and contours in the Print file. | `python3 outline_loss.py repdf-repo` |
| `glyph_count.py` | Glyphs shown via text operators per page language, Save As vs Print, plus outline contours (the 4.7% / 16.5% / 25.2% table). | `python3 glyph_count.py` (expects `repdf-repo/` beside it) |
| `outline_fonts.py` | Fonts used per page (Save As) against outlined Print pages; finds the two always-outlined fonts. | `python3 outline_fonts.py` (expects `repdf-repo/` beside it) |
| `font_fstype.py` | OS/2 `fsType` (embedding permission) and outline table of every embedded font program, against which Print pages draw outlines; shows the two outlined fonts are the only Preview & Print fonts (needs `fontTools`). | `python3 font_fstype.py [repdf-repo]` |
| `ocr_outlined.py` | Renders English and Chinese pages (PDFium, 200 dpi), OCRs them with PaddleOCR PP-OCRv4 via RapidOCR, and scores text-layer vs OCR recall against the Save As text. Writes `ocr/ocr_outlined.jsonl`. Needs `pypdf`, `pypdfium2`, `rapidocr-onnxruntime`. | `python3 ocr_outlined.py [n_docs]` (expects `repdf-repo/` beside it) |
| `ocr_summary.py` | Pools `ocr_outlined.jsonl` by language and by whether the Print page has outlines. | `python3 ocr_summary.py [jsonl]` |

The outlined-text scripts need `pypdf` (6.19 was used) and `font_fstype.py`
also `fontTools` (4.66); the OCR script also needs `pypdfium2` and
`rapidocr-onnxruntime` (1.4.4 was used, which bundles the PP-OCRv4 ONNX
models). onnxruntime is C++, which is fine for research and dev
tooling but not for the product build.

Known gap: the step that wrote the per-stream `cases/` directory (`<id>.z`
corrupted stream, `<id>.orig` original decoded bytes, `<id>.meta` true offset)
was not saved as a script. Rebuilding it from `partA.json` is straightforward
but has not been done.

`mzbench` was measured with `miniz_oxide` 0.8.9; the main crate pins 0.9, so
re-check that `DecompressorOxide` is still `Clone` there before relying on the
checkpoint approach.
