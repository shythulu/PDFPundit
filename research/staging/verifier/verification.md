# Stage 4 verification: ledger tools re-run after the 2026-10-05 restart

Agent: verifier. Date: 2026-10-05. REPDF at commit e547d4d1b77ead7e8cccce8b02878a6d33aa427a. Scripts are in `research/experiments/stage4-verifier/` (abbreviated `E/` below) and results in `E/results/`. `$S` is the verifier scratch directory; every tool was reinstalled there, with no apt or sudo, `nice -n 10` and `-j2`. The chair applies the proposed actions; the main ledger was not edited.

**Totals for the 45 Adopt/Trial TOOLs with `verified_in_container = pass`:** 44 pass, 1 fail (TOOL-413, provisioning). 1 downgrade is proposed (TOOL-306 CORE, keyless access now refused). The engine and oracle smoke tests at seed 20260927 reproduce the stored results exactly (OBS-1200, OBS-1201).

Abbreviations: **tc** = `E/tool_checks.py` (OBS-1209; the full command is in that record; the key lines are in `E/results/tool_checks.txt`). **es-rec** / **es-pin** = `S=$S bash E/rerun_smoke.sh engine recorded|pinned` (OBS-1200 / OBS-1202). **os** = `S=$S bash E/rerun_smoke.sh oracle recorded` (OBS-1201). **ap** = `python3 E/access_probe.py` (OBS-1208). **cc** = `python3 E/cc_paced_probe.py` (OBS-1207). **wild** = `S=$S bash E/rerun_wildcard.sh <mode>` (OBS-1210, OBS-1211). **cm** = `S=$S bash E/rerun_cpr_metrics.sh cpr|metrics` (OBS-1212, OBS-1213).

## A. The 45 Adopt/Trial TOOLs with prior `verified_in_container = pass`

| ID | Name | Prior | Today (2026-10-05) | Command | Key line | Action |
|---|---|---|---|---|---|---|
| TOOL-300 | Crossref REST API | Adopt / pass | **pass** | ap | `works?query.bibliographic=pdf+repair&rows=1` returns 200, `x-rate-limit-limit 1`, `x-rate-limit-interval 1s` | keep |
| TOOL-301 | OpenCitations Index API v2 | Trial / pass | **pass** | ap | `citations/doi:10.1109/das.2018.64` returns 200, body `[]`; it now redirects to `api.opencitations.net/index/v2/...` | keep (note the redirect) |
| TOOL-306 | CORE API | Adopt / pass | **fail (keyless)** | ap, then `ap … core` 20 min later | 429 both times, `x-ratelimit-limit 10`, `x-ratelimit-remaining 0` (Cloudflare) | **downgrade** Adopt to Trial and `verified_in_container` pass to fail (keyless) until the user supplies a free key (access.md §1) |
| TOOL-310 | Unpaywall API | Adopt / pass | **pass** | ap (email research@pdfpundit.invalid) | `v2/10.1109/das.2018.64` returns 200, 1,294 bytes | keep |
| TOOL-323 | PyMuPDF4LLM | Adopt / pass | **pass** | tc | 1.28.2: CPR paper to Markdown in 7.0 s, 52,604 chars, 39 headings, 1 'References' heading | keep |
| TOOL-402 | digitalcorpora unsigned-S3 range fetch | Adopt / pass | **pass** | cc (part A) | ListObjectsV2 200; 2,000,000-byte range of cc-provenance 206 with 12,946 rows (= OBS-0800); 0001.zip central directory 1,000 entries | keep |
| TOOL-413 | LibreOffice (soffice headless) | Adopt / pass | **FAIL** | tc | `soffice --headless --convert-to pdf` (24.2.7.2) produces no file: "Writer module present: False; Error: source file could not be loaded" | keep the verdict; set `verified_in_container` to fail until libreoffice-writer is reinstalled (needs apt: access.md §4) |
| TOOL-414 | Ghostscript ps2pdf | Adopt / pass | **pass** | tc | /usr/bin/ps2pdf (gs 10.02.1): 2,444 bytes, 1 page | keep |
| TOOL-415 | Chromium headless print-to-pdf | Adopt / pass | **pass** | tc (`TMPDIR=$S/tmp`) | Chromium 141.0.7390.37: 23,278 bytes, 1 page | keep |
| TOOL-416 | pdflatex | Adopt / pass | **pass** (different install) | tc; TeX Live via `S=$S bash E/texlive_install.sh` | pdfTeX 3.141592653-2.6-1.40.29 (TeX Live 2026, scratch install): 13,542 bytes, 1 page. Debian's TeX Live 2023 is gone after the restart | keep; update `version` to TeX Live 2026 / pdfTeX 1.40.29 and add the scratch-install route |
| TOOL-417 | reportlab | Adopt / pass | **pass** | tc | 5.0.1: 1,360 bytes, 1 page | keep |
| TOOL-418 | pycairo | Adopt / pass | **pass** | tc | 1.29.1: 5,727 bytes, 1 page | keep |
| TOOL-421 | radamsa | Adopt / pass | **pass** | tc (built from source in `$S/src/radamsa`) | Radamsa 0.8a: original 685,037 bytes, mutated 685,122, 668,882 bytes differ | keep |
| TOOL-422 | zzuf | Adopt / pass | **pass** | tc | zzuf 0.15: 685,037 to 685,037 bytes, 5,486 differ | keep |
| TOOL-423 | peepdf-3 | Adopt / pass | **pass** | tc | 5.4.1 `peepdf -l`: PDF 1.7, 165 objects, 42 streams, 0 errors | keep |
| TOOL-426 | pikepdf | Adopt / pass | **pass** (10.14.0 and 10.16.0) | tc; es-rec; es-pin | Opens the 5 producer outputs (1 page each) on both versions; engine smoke rows identical | keep |
| TOOL-350 | qpdf (static binary) | Adopt / pass | **pass** | tc; es-rec | 12.4.2: rewrite C2 rc 3 "operation succeeded with warnings"; `--suppress-recovery` rc 2 "xref not found". OBS-0500 re-check: apt qpdf 11.9.0 rc -11, 12.4.2 rc 3 | keep |
| TOOL-351 | MuPDF mutool | Adopt / pass | **pass** at 1.28.0; **regression** at the 1.28.5 pin | tc; es-rec; es-pin; `S=$S bash E/mutool_c1_versions.sh` | C2: `clean -gggg` 6 pages, `draw -F txt` 13,856 bytes on both versions. On two C1 files, 1.28.5 exits 1 and extracts 4,500 / 3,609 bytes, where 1.28.0 (conda-forge and a same-recipe source build) gets 9,205 / 4,804 (OBS-1203) | keep Adopt; **pin 1.28.0** in WP-3.1, or keep 1.28.5 and record the C1 regression |
| TOOL-352 | PyMuPDF | Adopt / pass | **pass** | tc; es-rec | 1.28.2: C2 opens, 6 pages, 9,221 chars | keep |
| TOOL-353 | pdf.js (pdfjs-dist) | Adopt / pass | **pass** (6.3.289 and 6.4.299) | tc; es-rec; es-pin | `{"ok":true,"pages":6,"chars":7604}` on both versions | keep; 6.4.299 is verified |
| TOOL-354 | pypdfium2 | Adopt / pass | **pass** (5.13.0 and 5.14.0) | tc; es-rec; es-pin | open + save C2: 6 pages, 6 saved (PDFium 153.0.7999.0 / 156.0.8076.0) | keep; 5.14.0 is verified |
| TOOL-355 | pdfcpu | Trial / pass | **pass** (0.15.0 and 0.16.1) | tc; es-rec; es-pin | `validate -m relaxed` C2 "validation ok" on both. 0.16.1 exits 1 on every command ("run: pdfcpu config reset") if it finds a 0.15.0 config in `~/.config/pdfcpu`; it passes with its own `XDG_CONFIG_HOME` | keep; add a note to `verification_evidence`: 0.16.1 needs a fresh config directory |
| TOOL-356 | Apache PDFBox | Adopt / pass | **pass** | tc; es-rec | 3.0.8: `export:text` C2 12,363 bytes; decode 6 pages | keep |
| TOOL-357 | Poppler | Adopt / pass | **pass** (26.09.0 and 26.10.0) | tc; es-rec; es-pin; `S=$S bash E/build_pins.sh poppler` | `pdftotext` C2 13,155 bytes on both. 26.10.0 was built from source with HarfBuzz off (3 min 11 s wall) (conda-forge's newest is 26.09.0) | keep; 26.10.0 is verified |
| TOOL-358 | Ghostscript gs | Adopt / pass | **pass** | tc; es-rec | 10.08.0: pdfwrite 6 pages; txtwrite 16,415 bytes; `-dPDFSTOPONERROR` rc 1 "/syntaxerror in --runpdf--" | keep |
| TOOL-359 | lopdf | Adopt / pass | **pass** | tc; es-rec; es-pin | 0.45.0: "lopdf ok pages=6 objects=165" in both probe builds | keep |
| TOOL-361 | hayro | Trial / pass | **pass** (0.7.1 and 0.8.0) | tc; es-rec; es-pin (rsprobe080 = Cargo.toml, Cargo.lock and main.rs.patch in `E/rsprobe080/`) | "hayro ok pages=6"; PNG 99,708 bytes (0.7.1) vs 134,959 bytes (0.8.0). 0.8.0 changed its render API (`RenderSettings`, `PixmapSettings`; `render()` takes 5 arguments), so the committed probe does not compile against it without the patch | keep Trial; record the 0.8.0 API break. The PNG size difference was not investigated |
| TOOL-362 | veraPDF 1.30.2 (Arlington, greenfield) | Trial / pass | **pass** | tc; os | `arlington-pdf-model-checker 1.30.2`; greenfield rc 1 "FAIL … 1b" on the print original (expected: not PDF/A). Oracle smoke summary identical to the stored one | keep |
| TOOL-364 | zlib encoder replay (CPython zlib) | Adopt / pass | **pass** | tc | `zlib.ZLIB_RUNTIME_VERSION` 1.3 in all three Pythons | keep |
| TOOL-366 | preflate-rs | Trial / pass | **pass** | tc; wild preflate; wild deflate | 0.7.6 pfprobe: 3 Save As streams "ok corrections=178/96/290 roundtrip=true"; round trips 331/333 Save As and 343/343 Print (= OBS-0703) | keep |
| TOOL-368 | infgen | Adopt / pass | **pass** | tc | `infgen 3.6` (built from source) | keep |
| TOOL-372 | rapidfuzz | Adopt / pass | **pass** | tc | 3.14.6: CER 0.0 page 1, C2 vs original (1,889 normalised chars) | keep |
| TOOL-373 | jiwer | Adopt / pass | **pass** | tc | 4.0.0: WER 0.0, same pair | keep |
| TOOL-374 | dinglehopper | Trial / pass | **pass** | cm metrics | 0.11.0: every OCR CER equal to OBS-0706's | keep |
| TOOL-375 | scikit-image (SSIM) | Adopt / pass | **pass** | tc; cm metrics | 0.26.0: SSIM 0.9398 MuPDF vs PDFium; renderer floor SSIM min 0.9039, median 0.9414 (= OBS-0706) | keep |
| TOOL-376 | LPIPS | Trial / pass | **pass** | cm metrics | 0.1.4 (torch 2.14.0+cpu): renderer-floor LPIPS max 0.0485; all values = OBS-0706; 0.37 s per pair vs 0.17 s (host load 3-5) | keep |
| TOOL-382 | Tesseract OCR | Adopt / pass | **pass** | tc; cm metrics | 5.5.3 (conda-forge in `$S/cenv`): page 1 at 200 dpi, 1,896 chars, CER vs text layer 0.0011; OBS-0706 CERs identical | keep |
| TOOL-383 | docTR | Trial / pass | **pass** | cm metrics | 1.1.0: every CER = OBS-0706; 9.5-11.4 s per page vs 6.1-7.2 s stored (host load 3-5) | keep |
| TOOL-450 | stock zlib 1.x as replay reference | Adopt / pass | **pass**; 1.3.2 checked | `python E/zlib_version_diff.py` (OBS-1206) | zlib 1.3.2 output is byte-identical to 1.3 in all 17 configurations on every Flate stream of the 100 originals; L6M7 reproduces 2,081 Save As streams with both | keep; update `version` to "1.3 and 1.3.2 tested, identical output" |
| TOOL-456 | Two-dummy-dictionary resync | Trial / pass | **pass** | wild resync | dict_resync summary identical: precision_reliable 1.0, 51,120 false starts, 8,521,181 reliable-but-wrong bytes | keep |
| TOOL-457 | Replay-guided single-byte Flate corrector | Trial / pass | **pass** | wild c9 | 388 streams, 266 replayable, 196 unique replay-correct, 1 wrong candidate (identical; wall 867.5 s vs 723.9 s) | keep |
| TOOL-461 | Closed-form xref deletion solver | Trial / pass | **pass** | wild xref | brute_force_unique 50/50, EOL-style byte-exact 50/50 (Print / Save As); CSV identical apart from timings | keep |
| TOOL-462 | Simulated-annealing glyph-code decipherer | Trial / pass | **pass** with the stored LM corpus; **differs** with today's cache | wild decipher; wild decipher-frozen | With the 29 manifest texts (`lm_fulltext.inputs.sha256`): token_acc 0.7657, digit_acc 0.0652, margin>10 0.8682 / 0.8629; 117 of 119 summary values identical, and the other 2 are wall time and the fontTools label; 0 differing CSV cells. With all 33 texts now in `research/cache/fulltext`: token_acc 0.7486, 400 of 664 slots change (OBS-1215) | keep Trial and pass; change `verification_evidence` and OBS-0906's command to train on a pinned text list (the manifest), not the whole gitignored cache directory |
| TOOL-464 | fontTools | Adopt / pass | **pass** | wild fonts; fontations cross-check | 4.66.1: print_font_names identical to OBS-0907 (family 889/889, cmap vs ToUnicode 35,673/35,792); agrees with read-fonts on 1,252/1,252 programs | keep |
| TOOL-476 | preflate-rs as a zero-correction replay oracle | Trial / pass | **pass** | wild preflate | Save As zero-correction replay 395/400 with zlib-6 parameters (memLevel 8: 220, 7: 175); 47 with its own estimated parameters; Print 0/400; 14 panics caught (identical to OBS-0908) | keep |

## B. Other TOOLs touched by this stage

| ID | Name | Prior | Today (2026-10-05) | Command | Key line | Action |
|---|---|---|---|---|---|---|
| TOOL-304 | OpenAlex API | Trial / fail | **pass (keyless)**, quota exhausted | ap | 200, `x-ratelimit-limit 1000`, `x-ratelimit-remaining 0`, reset 20,921 s | **upgrade** `verified_in_container` fail to pass; keep Trial; a key is recommended (access.md §1) |
| TOOL-305 | Semantic Scholar Graph API | Trial / fail | **pass (keyless)** | ap | `paper/DOI:10.1109/das.2018.64?fields=title` returns 200 | **upgrade** `verified_in_container` fail to pass; keep Trial |
| TOOL-302 | arXiv API | Trial / fail | not checked | n/a | n/a | keep (not in this brief) |
| TOOL-317 | GROBID (CRF Docker image) | Trial / untested | **not run** | n/a | There is no Docker daemon in this container. The first step towards setting one up was denied at a permission check, so it was not pursued | keep untested; it depends on the user provisioning Docker (access.md §4). If Docker is not provided, move it to Assess |
| TOOL-385 | CPR corrupted-PDF recovery baseline | Assess / partial | **Step 1 reproduces** | cm cpr (CPR at 9ecd6853a5eedad71631ec8e48c50ae4c23ff87c) | Summary identical to OBS-0704 (by-class numbers, CER libraries agree, 28 path literals patched); only cpr_seconds differ | keep partial (Step 2, the LLM step, was not run) |
| TOOL-386 | local LLM via Ollama | Assess / untested | **host reachable** | ap | registry.ollama.ai manifest for `<TOOL-386 model>` returns 200 (858 bytes). No blob was fetched and no model was run | keep untested; the blob CDN host is still untested (access.md §3) |
| TOOL-403 | data.commoncrawl.org WARC range fetch | Trial / fail | **pass (paced)** | cc (30 requests, 2 s gap, back-off on 403) | 30 of 30 return 206, no 403; gaps 2-3 s; wall 74.3 s | **upgrade** `verified_in_container` fail to pass; keep Trial; keep the pacing and 403 back-off in the sampler. No AWS account is needed |
| TOOL-404 | CC-MAIN-2021-31-PDF-UNTRUNCATED corpus | Trial / partial | **pass** | cc | metadata, ZIP central directory and a real WARC record of a truncated PDF are all range-readable | **upgrade** `verified_in_container` partial to pass |
| TOOL-408 | NapierOne dataset | Trial / untested | **host reachable** | ap | path-style `s3.eu-north-1.amazonaws.com/napierone.com/?list-type=2&max-keys=1` returns 200 | **upgrade** untested to partial (bucket listing only; no data file was read). Use the path-style URL: the bucket name's dot breaks virtual-host TLS |
| TOOL-465 | fontations read-fonts / skrifa | Trial / untested | **pass** | `cargo build` of `E/fontations_probe`, then `python E/fontations_names.py` (OBS-1204) | read-fonts 0.45.0 / skrifa 0.48.0: 1,252/1,252 FontFile2 programs parsed; family matches OBS-0907 on 889/889 rows; read-fonts = skrifa = fontTools on all | **upgrade** untested to pass; keep Trial |
| TOOL-470 | MAPIE | Trial / untested | **pass** | `python E/mapie_crc.py` (OBS-1205) | 1.5.0 `BinaryClassificationController`: synthetic target precision 0.9 gives 14 valid thresholds and test precision 0.9401; on the 40-row OBS-derived calibration set it abstains (0 valid thresholds) | **upgrade** untested to pass; keep Trial |

## C. WP-3.1 pins (5k plan rev 1)

Every pin installed within 20 minutes. Each was run through `engine_smoke.py` at seed 20260927 (es-pin, OBS-1202) and through tc.

| Pin | Tested in Stage 2 | How installed today | Result |
|---|---|---|---|
| MuPDF 1.28.5 | 1.28.0 | source build, `S=$S bash E/build_pins.sh mupdf` | Smoke table equal except mutool_text C1: 2 of 2 at 95% or more of the text drops to 0 of 2 (OBS-1203) |
| Poppler 26.10.0 | 26.09.0 | source build, `S=$S bash E/build_pins.sh poppler` (HarfBuzz off) | equal |
| pdfcpu 0.16.1 | 0.15.0 | release tarball | equal, given a fresh `XDG_CONFIG_HOME`. With the shared 0.15.0 config it produced no output at all (`results/engine_smoke_pinned_run1_sharedconfig.*`) |
| hayro 0.8.0 | 0.7.1 | cargo, `E/rsprobe080/` | equal after the API patch (`main.rs.patch`) |
| pikepdf 10.16.0 | 10.14.0 | pip, `$S/venv` | equal |
| pypdfium2 5.14.0 | 5.13.0 | pip, `$S/venv` | equal |
| pdfjs-dist 6.4.299 | 6.3.289 | npm, `$S/npm-6.4.299` | equal |

## D. Corrected `command` fields for observations with deleted scratch paths

The 2026-10-05 restart deleted `.../scratchpad/tooling-engines/`. Each command below rebuilds the tool under a fresh scratch directory `$S` (install steps are in the script headers) and re-runs the committed script. Results land in `research/experiments/stage4-verifier/results/`. Each re-run below agrees with the stored result (column 3), so none of these needed a new OBS to correct a result; the reproduction OBS ids are given. The one result that differs, OBS-0906's decipherer with today's cache, is OBS-1215 (§E).

| OBS | Proposed `command` | Re-run today |
|---|---|---|
| OBS-0700 | after the install steps in the header of `rerun_smoke.sh` (qpdf, pdfcpu and PDFBox release files; a micromamba env for mutool, gs, Poppler and Tesseract; pdf.js via npm; two venvs; the Rust probe; veraPDF): `S=$S bash research/experiments/stage4-verifier/rerun_smoke.sh engine recorded` (runs `research/experiments/engine_smoke.py $R <out>/engine_smoke_recorded --recheck <qpdf xref-compressed-in-compressed.pdf>` with the Stage 2 versions; seed 20260927, 60 s timeout, 2 workers) | identical summary; 624 CSV rows, 0 differing cells (OBS-1200) |
| OBS-0701 | same as OBS-0700 (the `--recheck` step runs `qpdf --check` with `QPDF=$S/dl/bin/qpdf12/bin/qpdf QPDF_APT=/usr/bin/qpdf`) | apt qpdf 11.9.0 rc -11; qpdf 12.4.2 rc 3, the same as stored (OBS-1200) |
| OBS-0702 | `S=$S bash research/experiments/stage4-verifier/rerun_smoke.sh oracle recorded` (after the same install steps) | identical summary; 79 rows, 0 differing cells (OBS-1201) |
| OBS-0703 | `(cd research/experiments/pfprobe && CARGO_TARGET_DIR=$S/rust/target_pf cargo build --release --locked -j2) && mkdir -p $S/dl/bin/pf && cp $S/rust/target_pf/release/pfprobe $S/dl/bin/pf/ && S=$S bash research/experiments/stage4-verifier/rerun_wildcard.sh deflate` | equal to the stored summary and both CSVs (OBS-1211) |
| OBS-0704 | `git clone https://github.com/BeenyHail/CPR $S/cpr && git -C $S/cpr checkout 9ecd6853a5eedad71631ec8e48c50ae4c23ff87c && S=$S bash research/experiments/stage4-verifier/rerun_cpr_metrics.sh cpr` (the venv steps are in the script header) | identical summary; 20 rows, identical except `cpr_seconds` (OBS-1212) |
| OBS-0706 (also scratch-bound) | `S=$S bash research/experiments/stage4-verifier/rerun_cpr_metrics.sh metrics` (rebuild `$S/venv-old`, `TORCH_HOME` and `DOCTR_CACHE_DIR` as in the script header) | every metric identical; timings slower (OBS-1213) |
| OBS-0906 (unpinned LM corpus, not scratch-bound) | `S=$S bash research/experiments/stage4-verifier/rerun_wildcard.sh decipher-frozen` (trains only on the 29 texts of `research/experiments/results/lm_fulltext.inputs.sha256`) | identical with the frozen list; with today's 33-text cache, token_acc 0.7486 vs 0.7657 (**new OBS-1215**) |

## E. Other re-runs (PR #16, wildcard prototypes)

- **PR #16 mid-size Adler-only search (CLM-1019), OBS-1214.** Phase 0 reproduces OBS-1010 exactly (28 of 47). Phase 5 is new: 35 of 48. Pooled, 63 of 95 = 66.3% (Wilson 56.3-75.0%), against CLM-1019's 66.1%.
- **Wildcard prototypes, OBS-1210.** c9_replay_correct, dict_resync, xref_constraints, print_font_names and the preflate probe and replay are all identical to the stored results apart from timing.
- **decipher_codes, OBS-1215.** See the TOOL-462 row. The prototype is deterministic. Its stored command trains the language model on every text in a shared, gitignored cache, which other agents added 4 texts to after the stored run. Pinned to the stored run's 29 texts, it reproduces OBS-0906 exactly. On today's 33 texts, overall token accuracy falls by 1.7 points, and slots of 1,024-4,096 codes fall from 0.858 to 0.819. **Proposed corrected command for OBS-0906:** `S=$S bash research/experiments/stage4-verifier/rerun_wildcard.sh decipher-frozen`, or better, a `--lm-manifest` option in `decipher_codes.py` (a code change for the chair).

## F. Notes for the ledger

- Only TOOL-426's `verification_evidence` names a scratch path, and the restart deleted it. The other entries cite OBS-0700..0706, whose commands are corrected in §D. The install steps for every engine are now in the headers of `E/rerun_smoke.sh`, `E/build_pins.sh` and `E/rerun_cpr_metrics.sh`.
- SRC-0132 still answers 500, directly and via doi.org. The PDF cached on 2026-09-27 is still in `research/cache/pdf/`, and its sha256 equals the registry's `fulltext_sha256` (`e7bd5fa0…2615`). kb_validate re-checks CLM-0129..0136 as `exact` against it, so the 8 claims are re-checked offline.
