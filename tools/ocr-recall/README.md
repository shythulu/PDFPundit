# ocr-recall

`ocr-recall` scores how much of each original page's visible text a repair keeps, read by OCR.
It is dev tooling. Nothing in the shipped binary uses it, so the binary's purity rule is untouched.

Every number it prints is a **REPDF-style regression baseline**.
It is not REPDF's metric, and it is not comparable to the paper's tables.

## How a run fits together

1. `cargo test --release --lib -- --ignored bench::corpus::ocr` (T-38a) repairs the smoke subset.
   Then it renders every page of each original and each repair through hayro at 200 dpi.
   The PNGs and `manifest.csv` land in `target/corpus/ocr_inputs/`.
2. `ocr-recall` reads the manifest and OCRs every PNG once, in manifest order.
3. Each original page's OCR text is the ground truth for the same page of its ten repaired variants.
4. The scorer writes one CSV row per page and prints two aggregate tables to the log.

```
manifest.csv ──> manifest.py ──> engine.ocr(png) per PNG ──> recall.py ──> ocr_results.csv
 (T-38a)          (checks rows)    (engines/*.py)              (scores)      + log tables
```

## Usage

Python 3.12 or 3.13. Never 3.14, because paddlepaddle 3.3.1 ships no cp314 wheel.

```sh
uv venv --python 3.12 .venv-ocr
uv pip install --python .venv-ocr --require-hashes -r tools/ocr-recall/requirements.lock
PYTHON=.venv-ocr/bin/python tools/ocr-recall/ocr-recall \
    --manifest target/corpus/ocr_inputs/manifest.csv \
    --out target/corpus/ocr_results.csv [--tier tiny|medium] [--engine paddle|tesseract|documentai]
```

| flag | default | effect |
|---|---|---|
| `--tier` | `tiny` | model pair for Latin and Han pages; see the model table |
| `--engine` | `paddle` | which OCR apparatus reads the PNGs |
| `--dry-run` | off | checks the engine is installed and reads the manifest; fetches and OCRs nothing |

The wrapper runs `ocr_recall.py` under `python3 -I`.
A missing PaddleOCR exits with one line naming `requirements.lock`.

Tests need only pytest, from its own hash lock. They download no model.

```sh
uv pip install --python .venv-test --require-hashes -r tools/ocr-recall/requirements-test.lock
.venv-test/bin/python -I -m pytest tools/ocr-recall/tests
```

## The metric (D-068)

The paper's section 5.3.1 fixes only two things.
The formula is "correctly recovered words / words in the original", and a word matches exactly.
It leaves five choices open, and each one moves the number.

| choice | the paper | this harness |
|---|---|---|
| tokenisation | Document AI's, unstated | whitespace runs; `zh` pages one token per character, as T-25 does |
| normalisation | unstated | NFC, then each character lower-cased, then whitespace collapsed (TD §8, T-25's `normalize`) |
| matching | unstated | clipped bag of words |
| granularity | unstated | per page; tables are means of per-page values |
| OCR engine and input | Document AI, version unstated, PDF input | PaddleOCR on hayro's 200 dpi PNGs |

Per page:

```
ocr_word_recall = Σ_w min(c_orig(w), c_rep(w)) / Σ_w c_orig(w)
```

`c_orig(w)` counts word `w` in the original's OCR text and `c_rep(w)` in the repair's.
The `min` clips the bag, so repeated words cannot push recall past 1.0.
`a a` against `a a a` scores 1.0. `a a a` against `a` scores 1/3.

Two sensitivity columns always sit beside it.
On one pair of OCR outputs, these choices alone spread Latin recall from 0.69 to 0.99 (eng-r3-fr1 §6).
Printing them shows the spread instead of hiding it.

| column | definition |
|---|---|
| `ocr_word_recall` | the formula above |
| `ocr_word_recall_nopunct` | the same, after removing Unicode punctuation from every token |
| `ocr_char_recall` | the same clipped bag over non-whitespace characters |
| `text_visual_gap` | the page's text-layer LCS-F1 (T-26) minus `ocr_word_recall` |

A large positive `text_visual_gap` means the repair kept the text bytes but lost the rendering.

Edge cases:

- A repair hayro cannot open (`open_ok = 0`) scores 0 on every page of its original, with `ocr_pages = 0`.
- A page the repair lacks, or could not render, scores 0.
- An original page whose OCR finds no words has no ground truth. Its recall columns are empty and the means skip it.
- Pages a repair has beyond its original's count have no ground truth and get no row.
- A file hayro cannot open has no per-page LCS-F1 in the manifest, so its `text_visual_gap` is empty.

All arithmetic uses exact fractions. Output rounds half to even: six decimals in the CSV, three in the tables.

### Why "REPDF-style"

The paper's word recall cannot be reproduced from its text.
Five free choices above are undefined there, and the repo has no scoring code.
This harness also differs in engine, input and models.
So the number is a trend indicator across PDFPundit releases, nothing more.

Whether the paper's per-class figures appear beside it, as cited reference columns that are not like-for-like, is the user's decision (D-070).
This README does not rule that out.

## Output

Both the CSV and every log table start with the D-063 label.
The nightly log is public, so the label travels with the numbers:

```
regression baseline; 44-file REPDF smoke subset; REPDF-style OCR word recall, engine <name> <version>; not REPDF's metric; not comparable to the paper; not a published recovery rate
```

| output | holds |
|---|---|
| `ocr_results.csv` | label line, then one row per scored page: engine, version, tier, file, class, page, lang, script, models, the four columns, `lcs_f1` |
| log, header | label, then an apparatus line: tier, each model the run uses with its Hugging Face revision, the paddleocr, paddlex and paddlepaddle versions, `cpu_threads` |
| log, per page | page, script, line count, seconds, sha256 of the OCR text |
| log, tables | means per class and script, then per class |

The per-page sha256 and seconds are measurements, not gates.
The first ubuntu run measures runner latency and cross-platform OCR text equality.
eng-r3-fr1 §4 found the text identical across two runs on one machine. Cross-platform is untested.

No artifact is uploaded. The CSV names corpus files, so it stays on the runner.

## Models and routing (D-067)

`models.toml` pins each model's Hugging Face repo, revision and the sha256 of its `inference.pdiparams`.
The harness fetches each model at that revision into `PADDLE_PDX_CACHE_HOME`, default `target/corpus/paddle-models`.
It hashes every model before building any pipeline, so a mismatch stops the run before the first page.

Model names are always passed explicitly.
`PaddleOCR(lang="en")` in 3.7.0 silently picks PP-OCRv6_medium, 138 MB and about 13 times the cost of tiny.

A page's script comes from its manifest `lang`, never from its page index (D-069).
The corpus orders its six languages differently in each document.

| script | `lang` | tier `tiny` (default) | tier `medium` |
|---|---|---|---|
| latin | en, fr, es, und | PP-OCRv6_tiny_det + PP-OCRv6_tiny_rec | PP-OCRv6_medium_det + PP-OCRv6_medium_rec |
| han | zh | PP-OCRv6_tiny_det + PP-OCRv6_tiny_rec | PP-OCRv6_medium_det + PP-OCRv6_medium_rec |
| arabic | ar | PP-OCRv5_mobile_det + arabic_PP-OCRv5_mobile_rec | same as tiny |
| devanagari | hi | PP-OCRv5_mobile_det + devanagari_PP-OCRv5_mobile_rec | same as tiny |

Script routing exists because the English models read almost nothing on the 79 Arabic and Hindi pages of the 264.
The script-specific recognition models do read them.

| model | `inference.pdiparams` bytes |
|---|---|
| PP-OCRv6_tiny_det | 1,718,055 |
| PP-OCRv6_tiny_rec | 4,420,410 |
| PP-OCRv6_medium_det | 61,960,476 |
| PP-OCRv6_medium_rec | 76,465,087 |
| PP-OCRv5_mobile_det | 4,692,937 |
| arabic_PP-OCRv5_mobile_rec | 7,922,839 |
| devanagari_PP-OCRv5_mobile_rec | 7,836,203 |

Orientation, unwarping and textline models are off.
`cpu_threads` is the machine's CPU count.
One process reads the pages in manifest order, because 4-way sharding roughly doubled per-page latency.

## Measured cost (eng-r3-fr1 §2 to §3)

Latency was measured on the authoring Mac under other load, so it is a lower bound for a hosted runner.

| quantity | value |
|---|---|
| install, Linux x86_64 cp312 | 70 wheels, 342 MB download, 1.17 GB unpacked |
| tiny tier, 264 pages at 4-way | 2.1 s per page, about 145 s wall |
| medium tier, 264 pages at 4-way | 27.9 s per page, about 30 min wall |
| medium on a 4 vCPU runner | extrapolated to more than an hour, not measured |
| Latin word / char recall vs text layer | tiny 0.873 / 0.934, medium 0.891 / 0.933 |

The nightly job runs the tiny tier.
Medium buys about two points of Latin recall for 13 times the cost, so it runs only on demand through `workflow_dispatch`.

## The nightly job

`.github/workflows/corpus-ocr.yml` runs on `ubuntu-latest` every night with a 180-minute limit. It is never a PR gate.

1. Run these tests in a venv without PaddleOCR.
2. Install `requirements.lock` with `--require-hashes`.
3. Sparse-checkout the 44 files of `bench/smoke_subset.txt` at the pinned corpus commit.
4. Run the corpus `ocr` mode, then `ocr-recall`.

The corpus clone and the rasters are never cached.
Wheels and models may be cached, because they are not corpus bytes.

## Engines and their status (D-071, D-014)

The apparatus that feeds the published number is research-pending, so the harness is engine-agnostic.
An engine is a module in `engines/` with one interface. Its recognisers expose `ocr(png) -> list[str]`, the text lines of one page.
The scorer never knows which engine produced the lines.

| `--engine` | status | why |
|---|---|---|
| `paddle` | built | DA M7 names local PaddleOCR in CI |
| `tesseract` | stub, exits "blocked on D-071" | PR #19's committee 005 accepted per-script Tesseract as its text reference. The PR #19 board has not ruled between it and DA M7 |
| `documentai` | stub, exits "blocked on D-014" | a Document AI account costs money, about $10 per run, and that is the user's decision |

When the board rules, the chosen engine becomes a second module here and nothing else changes.
Only one apparatus ever feeds a published number (D-063). The other may appear as a sensitivity column.

Even the Document AI leg would give a REPDF-style number, because the paper names no processor version, tokeniser or normalisation.
Its value is calibrating the local engine against the paper's engine on the originals.

Publishing any release number waits on the user (D-063).
