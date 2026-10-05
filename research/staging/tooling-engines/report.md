# Stage 2 tooling report: engines, oracles, validators and metrics (agent tooling-engines, W3 / agent B)

Relaunched on 2026-10-05 after the container restart, following the salvage addendum. Everything from the 2026-09-27 run (scripts and results files) was salvaged into records. The tools still in scratch, and the mutool, gs and pdftotext builds (rebuilt from conda-forge for the check, then deleted), were re-checked on real REPDF input on 2026-10-05. One new run was added on 2026-10-05 (OBS-0708).

**Records written (all in `research/staging/tooling-engines/`, validator: 0 errors):**

| Kind | IDs | Count |
|---|---|---|
| TOOL | TOOL-350..TOOL-386 | 37 |
| OBS | OBS-0700..OBS-0708 | 9 |
| CLM | CLM-0700..CLM-0706 (all `quote_check: exact`) | 7 |
| SRC | SRC-0700 (CPR source code, cached at commit 9ecd685) | 1 |
| SRCH | SRCH-0700..SRCH-0705 (all free registry queries) | 6 |

**Why 37 entries, not 12–25.** The brief's scope spans six needs: 12 engines, oracles, 8 DEFLATE tools, metrics, OCR and CPR. Each named candidate gets its own entry, so its licence and verdict can be read on their own. The 11 Hold entries are the rejected candidates the brief asks for. pikepdf is not duplicated: it already has an entry (TOOL-426).

All runs were smoke tests or small probes, not benchmarks (n = 2 per class, seed 20260927).

## 1. Ledger summary

| ID | Name | Version tested | Verdict | Verified |
|---|---|---|---|---|
| TOOL-350 | qpdf (CLI, static release binary) | 12.4.2 | Adopt | pass |
| TOOL-351 | MuPDF mutool (clean, draw) | 1.28.0 | Adopt | pass |
| TOOL-352 | PyMuPDF | 1.28.2 | Adopt | pass |
| TOOL-353 | pdf.js (pdfjs-dist on node) | 6.3.289 | Adopt | pass |
| TOOL-354 | pypdfium2 (PDFium) | 5.13.0 | Adopt | pass |
| TOOL-355 | pdfcpu | 0.15.0 | Trial | pass |
| TOOL-356 | Apache PDFBox (pdfbox-app jar) | 3.0.8 | Adopt | pass |
| TOOL-357 | Poppler (pdftotext, pdftocairo) | 26.09.0 | Adopt | pass |
| TOOL-358 | Ghostscript (gs pdfwrite, txtwrite) | 10.08.0 | Adopt | pass |
| TOOL-359 | lopdf (Rust crate) | 0.45.0 | Adopt | pass |
| TOOL-360 | pdf-rs (crate pdf) | 0.10.0 | Hold | pass |
| TOOL-361 | hayro (Rust crate) | 0.7.1 | Trial | pass |
| TOOL-362 | veraPDF 1.30.2: Arlington model checker (+ greenfield) | 1.30.2 | Trial | pass |
| TOOL-363 | JHOVE PDF module | 1.32.1 | Hold | untested |
| TOOL-364 | zlib encoder replay (CPython zlib) | zlib 1.3 | Adopt | pass |
| TOOL-365 | python-zlib-ng (zlib-ng) | 1.0.0 | Hold | pass |
| TOOL-366 | preflate-rs | 0.7.6 | Trial | pass |
| TOOL-367 | gzrt (gzrecover) | v0.9.1 / 97b3f9c8 | Hold | pass |
| TOOL-368 | infgen | 3.6 | Adopt | pass |
| TOOL-369 | rapidgzip | 0.16.0 | Hold | pass |
| TOOL-370 | ZipRec (Brown 2011) | - | Hold | untested |
| TOOL-371 | pugz | - | Hold | untested |
| TOOL-372 | rapidfuzz | 3.14.6 | Adopt | pass |
| TOOL-373 | jiwer | 4.0.0 | Adopt | pass |
| TOOL-374 | dinglehopper (OCR-D) | 0.11.0 | Trial | pass |
| TOOL-375 | scikit-image (SSIM) | 0.26.0 | Adopt | pass |
| TOOL-376 | LPIPS (lpips, AlexNet) | 0.1.4 | Trial | pass |
| TOOL-377 | DeepDiff over qpdf --json=2 (structural diff) | 9.1.0 | Assess | pass |
| TOOL-378 | pyiqa | 0.1.16 | Hold | untested |
| TOOL-379 | FaithC4 planted-token benchmark (DoVLMsRead) | HEAD f9d4ebcd | Assess | untested |
| TOOL-380 | Flexible character accuracy (FCA) | - | Assess | untested |
| TOOL-381 | python-Levenshtein | 0.27.5 | Hold | untested |
| TOOL-382 | Tesseract OCR | 5.5.3 | Adopt | pass |
| TOOL-383 | docTR (python-doctr) | 1.1.0 | Trial | pass |
| TOOL-384 | VLM OCR (olmOCR-2, LightOnOCR-2, PaddleOCR-VL-1.6, DeepSeek-OCR-2) | metadata only | Hold | untested |
| TOOL-385 | CPR (BeenyHail/CPR) | commit 9ecd685 | Assess | partial |
| TOOL-386 | Llama 3.1 8B via Ollama (Q4_K_M) | llama3.1:8b | Assess | untested |

**How the evidence is worded.**
- **Tools still installed in scratch:** "ran 2026-09-27 (results file X); re-checked 2026-10-05: command -> output line". This applies to qpdf, pdfcpu, PDFBox, pdf.js, pypdfium2, PyMuPDF, lopdf, pdf-rs, hayro, veraPDF, gzrt, infgen, preflate-rs, zlib, zlib-ng, rapidgzip, rapidfuzz, jiwer, scikit-image, DeepDiff and Tesseract.
- **mutool, gs and pdftotext:** re-checked on 2026-10-05 after a conda-forge rebuild, which was deleted afterwards.
- **lpips/torch, docTR and dinglehopper:** use the addendum's exact wording ("tool absent after the 2026-10-05 restart; Stage 4 re-verifies").
- **Licence flags:**
  - copyleft: MuPDF/PyMuPDF and Ghostscript (AGPL); Poppler, gzrt and python-Levenshtein (GPL); veraPDF (GPL-3.0-or-later OR MPL-2.0); ZipRec (GPL, from memory).
  - non-commercial: CPR (CC BY-NC 4.0) and pyiqa (PolyForm-Noncommercial).
  - custom: Llama 3.1 Community License.

## 2. Top picks per work package

### WP-3.1 Baseline harness (GAP-009)

| Engine | Pin | Best-of option set (source) | Licence |
|---|---|---|---|
| qpdf | **12.4.2** static release binary (apt 11.9.0 segfaults on the OBS-0500 file; 12.4.2 exits 3, OBS-0701) | `qpdf in out` (recovery on by default, CLM-0702); `--ignore-xref-streams`; `--decode-level=specialized --object-streams=disable`; strict control `--suppress-recovery` (manual/cli.rst) | Apache-2.0 |
| MuPDF mutool | 1.28.x (conda-forge 1.28.0; tag 1.28.5 untested) | `clean -gg`, `clean -gggg`, `clean -gggg -s` (pdfclean.c usage, CLM-0704) | AGPL, binary only |
| Ghostscript | 10.08.0 | `gs -o out.pdf -sDEVICE=pdfwrite in` (repairs by default; `-dPDFSTOPONERROR` is the strict control, CLM-0705); `-sDEVICE=txtwrite`; force the PDF interpreter on header loss (OBS-0501) | AGPL, binary only |
| PDFBox | 3.0.8 | `java -jar pdfbox-app-3.0.8.jar decode in out` (rewrite); `export:text` | Apache-2.0 |
| pdf.js | 6.4.299 (6.3.289 tested) | defaults (`stopAtErrors` false, CLM-0706) | Apache-2.0 |
| pypdfium2 | 5.14.0 (5.13.0 tested) | open + `save()` | BSD-3-Clause / Apache-2.0 |
| Poppler | 26.10.0 (26.09.0 tested) | `pdftotext`; `pdftocairo` as a third renderer | GPL, binary only |
| pikepdf | 10.16.0 (TOOL-426 still says 10.14.0) | open + save | MPL-2.0 |
| lopdf | 0.45.0 | load + save; the weakest loader (C1, C4, C5 at 0/2), so use it as the Rust emitter, not as a strong baseline | MIT |
| hayro | Trial 0.8.0 | load + render; per-class output identical to pdf.js | Apache-2.0 OR MIT |

Smoke result (OBS-0700): no engine produced anything on C10. Only mutool produced output on C4, and that output keeps 1 page. Every text engine fell below a 0.95 text ratio on C9. "Produced output" is not fidelity.

### WP-3.2 Metric suite (GAP-008, GAP-055, GAP-201)

- **Text:**
  - rapidfuzz 3.14.6 for CER, jiwer 4.0.0 for WER, both Adopt. They agreed on every file (OBS-0704).
  - dinglehopper 0.11.0 for grapheme-aware CER on the multilingual pages, Trial.
  - FCA has no package (SRCH-0705), so we build it over rapidfuzz alignments (TOOL-380).
- **Text reference:** Tesseract 5.5.3 (`OMP_THREAD_LIMIT=1 tesseract page.png - -l eng --psm 3`, about 1 s per page), Adopt. docTR 1.1.0 as a second OCR vote, Trial.
  - **Finding (OBS-0708):** 2 of 19 Print to PDF originals have a text layer holding only 50–60% of the visible text. The reference must therefore be OCR of the original render, or the upper bound, not the original's text layer.
- **Visual:**
  - scikit-image SSIM per page within one renderer, with MuPDF (PyMuPDF) and PDFium (pypdfium2) as the two renderers. The renderer floor between them is SSIM min 0.904 (OBS-0706).
  - LPIPS as a Trial: 0.17 s per pair on CPU, and it separates C9 damage (0.42–0.57) from the floor (≤ 0.049).
- **Structure:** DeepDiff over `qpdf --json=2 --json-stream-data=none`, Assess. Raw diffs explode because a rewrite renumbers objects, so canonicalise the object graph first.
- **Fabrication and omission against the upper bound:** no existing tool. Build a planted-token harness modelled on FaithC4 (SRC-0422, TOOL-379), scored with rapidfuzz. Do not vendor FaithC4's scorer, which depends on AGPL and GPL libraries.
- **VLM OCR:** all four models are Hold. None was shown feasible on 4 CPUs, and their language priors are the GAP-201 risk.

### WP-3.3 Verification oracles (GAP-106, GAP-150..154)

- **Conformance:** an oracle triad, scored as a **delta against the original**:
  - Arlington checker (veraPDF 1.30.2, `arlington-pdf-model-checker`);
  - `qpdf --check`, which is syntax-only by its own manual (CLM-0703);
  - `pdfcpu validate -m strict`.

  Pass/fail alone is unusable: 13 of 19 undamaged originals already fail Arlington, and the three tools disagree on 13–14 repaired files (OBS-0702).
- **DEFLATE:**
  - zlib replay (Adopt) is the byte-exact verifier for replayable producers: 68% of Save As streams, but 1 of 343 Print to PDF streams.
  - preflate-rs 0.7.6 (Trial) round-trips 674 of 676 streams from either producer, and its correction size flags damage (OBS-0703).
  - infgen (Adopt) for block structure and the fault diagnosis (OBS-0707).
  - Hold: gzrt (GPL; wrong bytes or nothing, OBS-0705) and rapidgzip (silent wrong bytes in 53 of 54, OBS-0707).

### WP-3.6 CPR baseline

CPR is Assess.
- **Licence:** CC BY-NC 4.0 (CLM-0700). Run it only as a research comparison and never copy its code.
- **Step 1:** ran on the seeded sample (OBS-0704) after patching 28 Windows path literals.
- **Step 2:** needs llama3.1:8b Q4_K_M, 4.92 GB, which fits in 15 GB RAM. It was not downloaded (over the 1.5 GB cap), and throughput on 4 CPUs is unmeasured.
- **Determinism:** no deterministic mode. The Ollama payload has no temperature or seed (CLM-0701). FO-B3 needs a documented patch adding `"options": {"temperature": 0, "seed": N}`.
- **GAP-201 risk:** the LLM acts as an O/X judge of whether a candidate mapping reads as natural text, so a language prior decides the output.

## 3. API-account list

**No keyed account is needed for anything in this ledger.**
- All registries used (PyPI, npm, crates.io, conda-forge, the Maven search, the Hugging Face model API, the Ollama registry, `git ls-remote`) answered without keys.
- The only keyed option seen is a Hugging Face account. It would be needed only to pull Llama 3.1 from Meta's gated repository. That is avoidable, because the Ollama registry serves `llama3.1:8b` without an account (TOOL-386, GAP-201, WP-3.6). No quota page was fetched, because no account is recommended.

**Network hosts that Stage 4 or P4 need to reproduce the installs:**

| Purpose | Hosts |
|---|---|
| Source and release binaries (qpdf, pdfcpu, gzrt, infgen, preflate-rs, DoVLMsRead, CPR) | github.com and its release-asset host; raw.githubusercontent.com |
| Python packages | pypi.org and files.pythonhosted.org |
| Torch CPU wheel (lpips, docTR) | download.pytorch.org |
| pdfjs-dist | registry.npmjs.org |
| Rust crates | crates.io, index.crates.io, static.crates.io |
| mupdf, ghostscript, poppler and tesseract builds | conda.anaconda.org and api.anaconda.org |
| PDFBox jar | repo1.maven.org (search.maven.org timed out on 2026-10-05; the PDFBox version came from `git ls-remote`) |
| veraPDF installers | software.verapdf.org |
| Poppler tags | gitlab.freedesktop.org |
| Model metadata only | huggingface.co |
| llama3.1:8b blob (about 4.9 GB, needs a larger download cap) | registry.ollama.ai / ollama.com |

## 4. What was NOT checked

**Newer releases.** These came out after the 2026-09-27 smoke test and were not run: pdfcpu 0.16.1, Poppler 26.10.0, MuPDF 1.28.5 (no conda build), hayro 0.8.0, pikepdf 10.16.0, pypdfium2 5.14.0 and pdfjs-dist 6.4.299.

**Option sets.**
- The best-of option sets are documented, and each repair route was run once on one C2 file on 2026-10-05:
  - mutool `-gggg` / `-gggg -s`;
  - gs pdfwrite;
  - PDFBox decode;
  - pypdfium2 save;
  - qpdf `--suppress-recovery`.
- They were **not** run across the 10 classes.
- qpdf `--ignore-xref-streams` and `--decode-level` variants were not run.

**Not run at all:**
- Arlington TestGrammar (TOOL-424), JHOVE, ZipRec and pugz.
- Any VLM OCR model on CPU.
- Llama 3.1 throughput, and CPR Step 2.

**Not measured or not found:**
- No pdftocairo or hayro render was measured for SSIM (only MuPDF and PDFium).
- No FCA implementation was found or written.
- No existing fabrication/omission tool was found; it must be built.
- FCA's original paper is cited from memory and not sourced (TOOL-380).

**Not re-run on 2026-10-05:** LPIPS, docTR and dinglehopper (torch environment gone).

## 5. Paid calls used

**0 of 6.** All six searches (SRCH-0700..0705) were free registry or git queries.

## 6. Decisions for the chair

1. **Pin qpdf ≥ 12.4.2** (static release binary) for every baseline. The apt 11.9.0 build segfaults (OBS-0500, OBS-0701).
2. **Update TOOL-426 (pikepdf)** to 10.16.0 (PyPI, 2026-09-29). It is owned by tooling-damage, so I did not edit it.
3. **Text reference for GAP-008 / WP-3.5:** adopt OCR of the original's render (Tesseract), or the upper bound, as the reference, not the original's text layer (OBS-0708). Otherwise Print to PDF pages score impossible CERs above 1.
4. **Conformance oracle:** accept "delta against the original" across Arlington + `qpdf --check` + `pdfcpu strict` as the GAP-106 oracle. Pass/fail alone fails 13 of 19 clean originals.
5. **CPR determinism:** approve a local, documented patch to CPR's Ollama payload (temperature 0, fixed seed), and a time-boxed download of the 4.9 GB model in P4. Without these, WP-3.6 cannot meet FO-B3.
6. **Entry count:** accept 37 entries, over the 12–25 guide (see the top of this report), or ask me to fold the Hold entries into alternatives.

## 7. Overlap with PR #16

| PR #16 finding | Ours | Verdict |
|---|---|---|
| 84.4% of damaged C9 streams fail only Adler-32; 15.3% raise an inflate error | 54 of 63 (85.7%) and 9 of 63 (14.3%) on the seeded sample (OBS-0703) | **corroborate** |
| (encoder replay not studied) | zlib replay 68% of Save As vs 1/343 Print to PDF; replay verifies but does not locate (OBS-0703) | **extend** (also extends OBS-0303) |
| mzbench checkpointed search fixes inflate-error streams | preflate-rs round trip and correction size as a damage signal (OBS-0703); infgen fault diagnosis (OBS-0707); gzrt and rapidgzip emit wrong bytes or nothing (OBS-0705, OBS-0707) | **extend** (new tools, not a re-run of mzbench) |
| lopdf needs a unique endstream and object headers at line start | lopdf 0/2 on C1, C4, C5 (OBS-0700) | **corroborate** |
| Engine precedence rules; evaluation design (generate-and-validate, calibration) | not redone | n/a |
| REPDF's OCR-recall metric | Print to PDF text layers are incomplete on 2 of 19 pages, so text-layer references undercount (OBS-0708) | **extend** |
