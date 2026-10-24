# Stage 4 contrarian scout: report

Agent "contrarian", ID block 11. Date: 2026-10-05. Target: `nimbalyst-local/plans/bleeding-edge-repair-5k.md`, rev 2.
Staging: `research/staging/contrarian/`. `kb_validate.py --staging research/staging/contrarian --write --online` reports 0 errors.

**Records:**
- SRC-1100..1107 (screened in);
- CLM-1100..1116 (all `exact`);
- OBS-1100..1104;
- TOOL-500..519;
- GAP-350..359;
- SRCH-1100..1123.

**Paid Parallel Search calls: 5 of 10**: SRCH-1111, 1116, 1121, 1122 and 1123.

## Upsets, strongest first

### 1. WP-3.3 product replay oracle: preflate-rs (TOOL-476/366) is beaten by an exact pure-Rust zlib port (TOOL-500). Confidence: high

**Evidence:** OBS-1100, CLM-1105, SRC-1104 and GAP-350, against OBS-0908.

zlib-bitexact-rs 0.131.1 (BSD-3-Clause, crates.io) is a port of stock zlib 1.3.1's `deflate_slow`. We patched it so that level and memLevel are arguments (`experiments/zbx_replay/patch_zbx.py`). On OBS-0908's 400 sampled Save As streams:

| | zlib-bitexact-rs (patched) | preflate-rs (OBS-0908) |
|---|---|---|
| Streams reproduced byte-for-byte | 400 of 400, including preflate's 5 misses (0010, 0054, 0070, 0243, 0357) | 395 of 400 |
| Matching memLevel set | equal to stock C zlib's on all 800 sampled streams | — |
| Panics | none | 14 with forced parameters |
| Private-struct mirror needed | no | yes |
| Speed | 1.26–1.30× stock C zlib | — |
| Print to PDF | 0 of 400 | 0 of 400 |

**Caveats:**
- The published API is fixed at level 9, memLevel 8 (CLM-1105), so the product needs a small fork or an upstream PR.
- Levels 1–3 (`deflate_fast`) and non-default strategies are not ported.
- The crate is 3 months old and has one maintainer.
- preflate-rs keeps its other role: correction size as a damage signal (OBS-0703), and the only handle on Print to PDF streams.

**Settle (Stage 4 or P4):**
1. Run the patched crate on all 1,445 C9 counterparts.
2. Port `deflate_fast` if WP-3.9's producers use levels 1–3.
3. Re-take committee 004's D3 with both candidates.

### 2. WP-3.14 producer profiles as a replay input (OBS-0908's "profile needed"): self-calibration per file works for Save As. Confidence: high (for replay)

**Evidence:** OBS-1101 and GAP-351.

The method needs no producer label. Take the memLevels that reproduce each of a file's intact Flate streams, then intersect them across the file.

| | Save As | Print to PDF |
|---|---|---|
| Calibrated set per C9 file | {7} in 50 of 50 files | empty in 50 of 50 files |
| Damaged streams' originals reproduced | 819 of 819 | 0 of 625 |

OBS-0908's 47 of 399 measured preflate-rs's per-stream estimator, not exact replay across a file. A profile adds nothing to replay in either producer. Profiles keep their non-replay content: the xref bias (OBS-0905), the EOL style, and font facts (OBS-0907).

**Settle:**
- Run the same calibration on WP-3.9's other producers, on files with few intact streams, and on multi-producer incremental updates.
- Decide D4 on profiles' non-replay value only.

Related, untested: grade-3 survivor counts with replay restricted to the calibrated parameter (GAP-352). This could remove OBS-0903's two wrong survivors.

### 3. WP-3.2 OCR reference and WP-3.5 Document AI substitute: TOOL-382 configured with `-l eng` fails on half of REPDF's pages. Confidence: high

**Evidence:** OBS-1102, GAP-353 and TOOL-501..503.

Setup:
- 24 pages from 4 Save As originals;
- 200 dpi;
- bag-of-characters recall against the text layer.

| Script (pages) | `-l eng` | tessdata_best, page's script | unhinted, six languages | RapidOCR default (PP-OCRv6) | PP-OCRv5 script recognisers |
|---|---|---|---|---|---|
| Arabic (4) | 0.025 | 0.938 | 0.875 | 0.018 | 0.781 |
| Devanagari (4) | 0.035 | 0.889 | 0.806 | 0.031 | 0.725 |
| Han (4) | 0.092 | 0.961 | 0.907 | **0.993** | 0.993 |
| Latin (12) | 0.982 | 0.992 | 0.992 | 0.971 | 0.963 |

Half of every REPDF document is Han, Devanagari or Arabic. On those pages an `-l eng` reference scores the OCR rather than the repair. The plan lists this as an unknown, and it is now measured.

PP-OCRv6 has no Arabic or Devanagari models (SRCH-1121), so there is no 2026 CPU challenger to tessdata_best on those two scripts.

**Settle (P4):**
- Use per-script tessdata_best for the reference. Use RapidOCR as the Han vote and docTR elsewhere.
- Use per-script strata and per-script margins in CLM-1065's non-inferiority table.
- Re-run at the plan's DPI with the 5.5.3 CLI. This run used libtesseract 5.5.1 via tesserocr.

### 4. WP-3.12 and WP-3.13: the "about 150 error-free slots" certificate (CLM-1061) and the use of MAPIE (TOOL-470) ignore clustering. Confidence: medium-high

**Evidence:** CLM-1100..1111, GAP-354 and TOOL-511.

**What the guarantees assume:**
- Split conformal needs exchangeable rows (CLM-1100).
- LTT assumes an i.i.d. calibration set (CLM-1102).
- CRC's guarantee holds only in expectation (CLM-1104).

**What the 2026 work measures:**
- On document extraction, the design effect is 1.84–2.45 (CLM-1106).
- Slot-level certificates therefore overstate the evidence about twofold (CLM-1107).
- A certificate with the document as the i.i.d. unit is near-vacuous at 800 documents (CLM-1108).
- Standard CRC broke its risk budget in up to 47% of trials under group shift (CLM-1110).

Our slots cluster by base document (CLM-1064). By the rule of three, certifying 2% at document level needs about 150 error-free documents, three times REPDF's 50 base documents.

**Settle:**
- Report the design effect on WP-3.13's calibration fold.
- Certify at document level, or with a cluster-corrected or hierarchical method (HG-CRC, CLM-1111).
- Keep MAPIE for the plain case.

### 5. WP-3.11 grouped splits leak font knowledge; REPDF C7/C8 cannot test glyph-based recovery. Confidence: medium-high

**Evidence:** OBS-1104, GAP-357 and GAP-358.

**Leakage.** Leave one Save As base document out. Of its ToUnicode entries (font name without the subset tag, plus code):
- 93.1% also occur in the other 49 documents' maps;
- 94.0% of those agree.

So a split by base document alone leaves C6/C8 decoders and font databases already holding most of the test mapping.

**C7/C8 content.** C7 and C8 keep no embedded font program anywhere in the file, not even orphaned. C8 also has no CMap stream. Glyph-shape tools (TOOL-514 poppler-science, TOOL-515) and outline-based checks have no input there.

**Settle:**
- Group by font family as well as by base document, or hold out a font set.
- Evaluate outline-based methods on natural pairs (WP-3.8), or on a generator that strips ToUnicode but keeps the programs.

### 6. WP-3.7 truncation pairs: missing strata and units. Confidence: medium

**Evidence:** SRC-1107, CLM-1112..1116 and GAP-355. SRC-1107 is a 2026 preprint on the same CC-MAIN-2021-31-PDF-UNTRUNCATED corpus.

- **Units:** truncated files are 23.06% of documents but hold 63.08% of the text (CLM-1112).
- **Engine dependence:** PyMuPDF recovers 8.4× what PDFium recovers from identical fragments (CLM-1114).
- **Empty opens:** 72.4% of truncated files open and yield no text (CLM-1115).
- **Linearization:** linearized files recover 1.1% vs 18.5% under PyMuPDF (CLM-1116).

**Settle:**
- Add linearization as a stratum.
- Report token-weighted results beside document-weighted ones.
- Count "opens with no text" as plausible-but-wrong.
- Treat the upper bound as engine-specific.

### 7. Product assumption, flagged and not decided: lopdf as sole loader and emitter (TOOL-359); hayro (TOOL-361) underused. Confidence: medium

**Evidence:** OBS-1103, GAP-356, GAP-359 and TOOL-504.

All 1,100 REPDF files were opened with three pure-Rust loaders:

| Class | hayro-syntax 0.8.0 | lopdf 0.45.0 | pdfrum 0.4.0 |
|---|---|---|---|
| C1 | 100 of 100 open, correct page count | 0 | 0 |
| C10 Save As | 50 of 50 open, correct page count | 0 | 0 |
| C4 Save As | — | opens 50, 0 correct page counts | — |
| C5 Print | 50 of 50 | opens 50, 0 correct page counts | text recall 1.0 |
| C9 text recall (Save As / Print) | not measured | 0.645 / 0.463 | 0.701 / 0.614 |
| C6 Save As text | not measured | — | recall 0.578, precision 0.485 (plausible-but-wrong, GAP-359) |

Caveats:
- hayro has no text API. Its C10 opens may be pages without content (compare CLM-1115). OBS-0700, with hayro 0.7.1, counted C10 0 of 2 under a "produced output" definition.
- pdfrum is 4 weeks old and its write path was not tested.

**Settle (WP-1.4/4.2):**
- Measure hayro's rendered content on C10.
- Test pdfrum's save path.
- Consider a tolerant reader in front of lopdf's writer. Writers TOOL-505, 508 and 509 are Assess only.

## Searched and found nothing

| Topic | Searches and venues | Result |
|---|---|---|
| Learned / LLM / VLM PDF repair, 2025–26 | SRCH-1111 (paid web); SRCH-1112 and 1113 (Crossref, from 2025); SRCH-1119 (arXiv) | Only CPR and REPDF (already registered) and fsidi.2025.301972 / fsidi.2026.302104 (already registered). DOI 10.1016/j.compeleceng.2026.111144 is a drone flight-log paper (off topic). Vol et al. 2018 (DAS, 10.1109/DAS.2018.64) is pre-2025 and not registered. No learned system that beats REPDF or engine repair. |
| CDR products | SRCH-1116 (paid web) | OPSWAT, Glasswall, Votiro and other vendors publish sanitization tests, not recovery accuracy. Glasswall claims it "repairs deviations" with no figures. A hands-on test needs vendor accounts, which are not allowed. |
| DEFLATE/zlib recovery alternatives to the 255-value search and grammar localizer | SRCH-1114 (Crossref), SRCH-1120 (arXiv), SRCH-1105 (crates.io) | Nothing 2025–26. The C9 ladder stands, unchallenged. |
| Replay / recompression engines | SRCH-1102..1104 and 1108 (crates.io) | zlib-bitexact-rs (upset 1). precomp2 0.2.0 untested (TOOL-510). |
| Print to PDF encoder identity | SRCH-1122 (paid web) | Nothing. Print streams remain unreplayable. |
| ToUnicode / glyph recovery tools | SRCH-1123 (paid web), SRCH-1106 (crates.io) | poppler-science (GPL, Latin-centric), pdf-cmap-fixer, pdf-encoding-repair, PDFix Font Fix. None applies to C8 (OBS-1104). |
| 2026 OCR for Arabic and Devanagari | SRCH-1121 (paid web), SRCH-1109 (PyPI) | PP-OCRv6 lacks both scripts. Vendor benchmarks are not credible. |
| Conformal methods for clustered data | SRCH-1107 (crates.io), SRCH-1110, 1117 and 1118 (arXiv) | Found and used: SRC-1100..1106. crepes and puncc (PyPI) add nothing for clustering. |
| Pure-Rust PDF readers and writers | SRCH-1100, 1101 and 1108 (crates.io) | pdfrum (tested), oxidize-pdf, pdf_oxide, pdf-syntax, krilla and pdf-writer (untested, Assess or Hold). |

## Ledger entries (staging)

| TOOL | Name | Verdict | Tested | Challenges |
|---|---|---|---|---|
| 500 | zlib-bitexact-rs (patched) | Trial | pass (OBS-1100) | TOOL-476/366 |
| 501 | Tesseract + tessdata_best per script | Trial | pass (OBS-1102) | TOOL-382's `-l eng` |
| 502 | RapidOCR 3.9.2 | Trial (Han only) | pass (OBS-1102) | TOOL-383 as second vote |
| 503 | PaddleOCR 3.7.0 / PP-OCRv6 | Hold | untested | — |
| 504 | pdfrum 0.4.0 | Trial (loader) | pass (OBS-1103) | TOOL-359 |
| 505 | oxidize-pdf 5.2.0 | Assess | untested | TOOL-359 |
| 506 | pdf_oxide 0.3.78 | Assess | untested | TOOL-359 / 361 |
| 507 | pdf-syntax 0.5.7 | Hold | untested | — |
| 508 | krilla 0.8.2 | Assess | untested | sole emitter |
| 509 | pdf-writer 0.15.0 | Assess | untested | sole emitter |
| 510 | precomp2 0.2.0 | Assess | untested | TOOL-366 |
| 511 | Cluster-aware certification (HG-CRC, document-level LTT) | Assess | papers only | TOOL-470 usage, CLM-1061 |
| 512 | crepes 0.9.1 | Assess | untested | TOOL-470 |
| 513 | puncc 0.9.3 | Hold | untested | TOOL-470 |
| 514 | poppler-science | Assess | untested | — (no C8 upset) |
| 515 | pdf-cmap-fixer | Hold | untested | — |
| 516 | pdf-encoding-repair 0.1.1 | Hold | untested | — |
| 517 | PDFix Font Fix | Hold | untested | — |
| 518 | OnnxTR 0.9.0 | Assess | untested | TOOL-383 speed |
| 519 | surya-ocr 0.22.1 | Assess | untested | — |

**Proposed main-ledger updates (for the chair):**
- **TOOL-361:** record hayro-syntax 0.8.0 with OBS-1103 (the only Rust loader that opens C1 and C10 Save As).
- **TOOL-382:** state the per-script configuration (TOOL-501).
- **TOOL-476:** cite OBS-1100 as the competing oracle.

**API accounts needed:** none. Everything ran from crates.io, PyPI, GitHub and modelscope.cn (model files) without keys.

## Not checked

- **pdfrum:** its write/save path.
- **hayro:** text and render on C10, i.e. whether its C10 opens contain content.
- **The zlib port:**
  - `deflate_fast` (levels 1–3);
  - streams from producers other than REPDF's two.
- **Survivors under calibrated replay:** grade-3 survivor counts with the calibrated parameter (GAP-352).
- **OCR:**
  - the Tesseract 5.5.3 CLI with tessdata_best;
  - more than 24 pages;
  - other DPIs.
- **Untested tools:**
  - oxidize-pdf, pdf_oxide, krilla, pdf-writer and precomp2 (none run);
  - poppler-science (not built);
  - surya, OnnxTR and olmOCR (not run).
- **Learned repair and CDR:**
  - VLM/LLM repair hands-on (TOOL-384 stays Hold);
  - CPR Step 2;
  - Glasswall and OPSWAT CDR hands-on (accounts not allowed).
- **Certification:**
  - HG-CRC code;
  - the design effect on our own calibration rows.
- **Han segmentation:** how REPDF's exact-word metric handles Han text without spaces. This was considered, but no evidence record was made.
- **Other metrics:** SSIM, LPIPS, structure diff and planted tokens were not challenged.

## Next steps and decisions for the chair

1. **D3 (replay oracle).** Re-open it with TOOL-500 as a candidate. P4 runs TOOL-500 on all 1,445 C9 streams and decides fork vs upstream. **Decision:** whether the product may carry a patched BSD-3 crate.
2. **D4 (producer profiles).** Narrow it to non-replay facts, since replay parameters self-calibrate (OBS-1101).
3. **WP-3.2 / WP-3.5 references.**
   - Replace the `-l eng` reference with per-script tessdata_best.
   - Set per-script strata and margins in WP-3.12.
   - Add RapidOCR as the Han vote.
4. **WP-3.12 / WP-3.13 certification.**
   - Add the design-effect measurement and document-level certification.
   - Acknowledge that REPDF cannot certify 2% at document level.
5. **WP-3.11 splits.** Add font-family grouping or a held-out font set. Note in WP-3.6 and WP-3.8 that REPDF C7/C8 cannot test outline-based methods.
6. **WP-3.7 strata.** Add linearization and token-weighted units, and count "opens with no text" as plausible-but-wrong.
7. **WP-4.2 (for the board).** Add a flag: lopdf as the sole loader. hayro-syntax or pdfrum in front of it opens C1, C5 and C10 files that lopdf cannot open. The board decides.
