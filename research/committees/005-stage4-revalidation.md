---
committee: 005
topic: Stage 4 re-validation (the hands-on verifier and the contrarian scout) and the plan changes that follow (5k rev 2 and rev 3, 10k rev 6)
date: 2026-10-05
charge: Do the Stage 2 tool picks still hold after the container restart and against 2025–26 alternatives? What must the user provision? What goes to the review board?
members:
  - seat: chair (merges, applies record updates, rules on each finding, writes 5k rev 2 and rev 3)
  - seat: verifier (re-ran every Adopt and Trial pick hands-on and audited access; decided nothing)
  - seat: contrarian scout (rewarded for overturning the plan's choices; decided nothing)
  - review of the chair's decisions: the Stage 5 review board, since the chair authored the plan being challenged
inputs: research/staging/verifier/{report,access}.md; research/staging/contrarian/report.md; 5k rev 1 and rev 2
paid_searches: 30 of 80 at the end of Stage 4 (verifier 0; contrarian 5 of its 10)
outcome: OBS-1200..1215 and OBS-1100..1104; TOOL-500..519; GAP-350..359; chair notes on 6 ledger entries and 2 gaps; D3 re-opened and D4 narrowed; a new product flag (lopdf as sole loader) for the board; 5k rev 2 (verifier) and rev 3 (contrarian); 10k rev 6
---

# Committee 005: Stage 4 re-validation

## What was merged
| Agent | Commit | Ledger | Observations | Other |
|---|---|---|---|---|
| verifier | 67a9b79 | re-checks of 45 Adopt and Trial entries; 11 entries changed | OBS-1200..1215 | corrected commands for OBS-0700..0704, 0706, 0906; access list |
| chair (verifier changes) | 67a9b79, 2ca67c8, f0124ac | as above | none | 5k rev 2 |
| contrarian | b30c363 | 20 new (TOOL-500..519) | OBS-1100..1104 | SRC-1100..1107; CLM-1100..1116; GAP-350..359; SRCH-1100..1123 |
| chair (this committee) | this commit | notes on TOOL-359, 361, 366, 382, 470, 476 | none | GAP-003 and GAP-154 updated; 5k rev 3; 10k rev 6 |

Validator after each merge: 0 errors. The 9 warnings are the 7 standing ones (3 web sources with no
cached text, 4 unpinned remote inputs) plus OBS-1207 and OBS-1208, which probe live endpoints
(Common Crawl and the discovery APIs) and so cannot pin a hash.

## Verifier (hands-on re-validation and access audit)
| # | Finding | Chair response |
|---|---|---|
| V1 | 44 of 45 Adopt/Trial tools pass after the restart; the engine and oracle smoke tests reproduce exactly at seed 20260927 (OBS-1200, 1201) | **Accepted.** Each passing entry's `checked_at` is now 2026-10-05, with the OBS cited. |
| V2 | TOOL-413 LibreOffice fails: the Writer module is gone and reinstalling it needs apt | **Accepted.** `verified_in_container` fail; the verdict stays Adopt. Provisioning item at the gate. |
| V3 | TOOL-306 CORE refuses keyless requests (429 twice) | **Accepted.** Downgraded Adopt to Trial, verification fail, until a key exists. WP-1.1 updated. |
| V4 | Upgrades: TOOL-304, 305 (fail to pass, keyless); 403 (fail to pass, paced); 404 (partial to pass); 408 (untested to partial); 465, 470 (untested to pass) | **Accepted.** The two Trial verdicts that had no hands-on run (committee 004's finding) now have one. |
| V5 | MuPDF 1.28.5 regresses on two C1 files against 1.28.0 (OBS-1203); the verifier asked for a pin choice | **Chair decision, not a user item:** WP-3.1 pins 1.28.0 and runs 1.28.5 as a second build to track the regression. It is a harness choice with no product effect. |
| V6 | pdfcpu 0.16.1 needs its own config directory; hayro 0.8.0 broke its render API; zlib 1.3.2 output is identical to 1.3; TeX Live 2026 replaces Debian's 2023 | **Accepted** as ledger notes and 5k rev 2 pins. |
| V7 | OBS-0906's decipherer trains on the whole gitignored cache, which grew from 29 to 33 texts; pinned to the manifest it reproduces exactly (OBS-1215) | **Accepted.** OBS-0906's command now names the frozen-list route; TOOL-462 notes the pin. A `--lm-manifest` option in `decipher_codes.py` is left for P4 (a code change). |
| V8 | Corrected commands for OBS-0700..0704 and 0706, whose scratch paths the restart deleted | **Applied** as a "Corrected command" suffix; the original command text is kept for the record. |
| V9 | PR #16's mid-size search pools to 63 of 95 = 66.3% vs CLM-1019's 66.1% (OBS-1214) | **Accepted.** CLM-1019 now has an in-house reproduction. |
| V10 | Common Crawl works when paced: 30 of 30 ranged reads (OBS-1207); no AWS account needed | **Accepted.** WP-3.7 switches to byte-verified pairs; the metadata-derived fallback is a contingency only. The AWS item leaves the gate list. |
| V11 | GROBID not run: no Docker daemon, and setting one up was refused at a permission check | **Accepted as a provisioning item.** The chair did not retry the refused step. If Docker is not provided, TOOL-317 moves to Assess (WP-1.2). |
| V12 | SRC-0132's host still answers 500; the cached PDF matches the registry hash | **No action.** CLM-0129..0136 re-check as exact offline. |
| V13 | Not checked: GROBID, the Ollama blob and LLM step, Hugging Face weights, arXiv (TOOL-302), oracle smoke at the new pins, paywalled items, NapierOne data files | **Recorded** as open items for P4. |

## Contrarian scout (seven upsets, strongest first)
The contrarian searched for 2025–26 work that beats the plan's choices and ran five experiments on
REPDF. It found nothing that beats REPDF or engine repair outright. Its upsets change how the plan
measures and certifies, and which pure-Rust components it trials.

| # | Upset | Confidence (its own) | Chair ruling |
|---|---|---|---|
| C1 | **WP-3.3 replay oracle.** zlib-bitexact-rs 0.131.1 (TOOL-500), patched to take level and memLevel, replays 400 of 400 sampled Save As streams, including preflate-rs's 5 misses, with no panics and no private-struct mirror. Its memLevel sets equal stock zlib's on all 800 sampled streams. It replays 0 of 400 Print to PDF streams (OBS-1100, GAP-350). | high | **Accepted; committee 004's D3 is re-opened.** WP-3.3's Trial runs both candidates. TOOL-500 is the lead for zero-correction replay of `deflate_slow` output (levels 4–9). preflate-rs (TOOL-366) keeps the correction-size damage signal, the only signal on Print to PDF streams. The run on all 1,444 damaged-stream counterparts is the Stage 5 DEFLATE reviewer's replication task, not the chair's: the chair authored D3. Whether the product may carry a patched crate (a fork or an upstream PR; 3 months old, one maintainer) goes to the board (WP-4.2). There is no ADR conflict, since both candidates are pure Rust. |
| C2 | **WP-3.14 producer profiles.** The replay parameter self-calibrates per file, with no producer label: each Save As C9 file's intact streams intersect to memLevel {7} in 50 of 50 files, and that reproduces 819 of 819 counterparts. Print to PDF calibrates to the empty set in 50 of 50 (OBS-1101, GAP-351). | high (replay) | **Accepted; D4 narrowed.** WP-3.14 takes replay parameters from per-file calibration by default. A profile supplies them only when a file has too few intact streams; that threshold is unmeasured (GAP-351). Profiles keep their non-replay content: the xref bias, the EOL style, the font facts and the encoder fingerprint. The board decides D4 on that content alone. GAP-352 (survivor counts under calibrated replay) joins WP-3.3's acceptance tests. |
| C3 | **WP-3.2 and WP-3.5 OCR reference.** Tesseract with `-l eng` recovers 2.5% of Arabic, 3.5% of Devanagari and 9.2% of Han characters. With the page script's tessdata_best model it recovers 93.8%, 88.9% and 96.1%. RapidOCR's PP-OCRv6 reaches 99.3% on Han (OBS-1102, GAP-353). | high | **Accepted.** The reference becomes per-script tessdata_best (TOOL-501), with the script taken from the original's text layer or a detector. RapidOCR (TOOL-502) is the second vote on Han pages and docTR (TOOL-383) elsewhere. WP-3.5 and WP-3.12 report per-script strata and margins. The numbers are indicative: 24 pages from 4 documents at 200 dpi, run on libtesseract 5.5.1. WP-3.2 re-runs them at the plan's DPI with the 5.5.3 CLI. At the access gate, the Document AI alternative is now per-script Tesseract. |
| C4 | **WP-3.12 and WP-3.13 certification.** Conformal guarantees assume exchangeable rows (CLM-1100, 1102), but our slots cluster by base document (CLM-1064). On document extraction, a 2026 study measures a design effect of 1.84–2.45 (CLM-1106). Document-level certificates are near-vacuous at small scale (CLM-1108), and CRC broke its risk budget in up to 47% of trials under group shift (CLM-1110). | medium-high | **Accepted.** WP-3.13 measures the design effect on its calibration fold and certifies at document level or with a cluster-aware method (TOOL-511, Assess). WP-3.12 states that CLM-1061's "about 150 error-free slots" holds only for independent slots. By the rule of three, 2% at document level needs about 150 error-free documents, three times REPDF's 50 base documents. MAPIE (TOOL-470) stays Trial for the plain case. To the board's evaluation scientist: relax the 2% target, or grow the document count from natural pairs and new producers. |
| C5 | **WP-3.11 splits leak fonts; REPDF C7/C8 cannot test glyph methods.** Leaving one Save As base document out, 93.1% of its ToUnicode entries also occur in the other 49 documents, and 94.0% of those agree. C7 and C8 keep no font program even as an unreferenced object, and C8 keeps no CMap (OBS-1104, GAP-357, 358). | medium-high | **Accepted.** WP-3.11 groups by base document and font family, adds a held-out font set from WP-3.8's pairs and WP-3.9's producers, and reports the held-out ToUnicode overlap per split as an acceptance figure. WP-3.6 and WP-3.8 note that REPDF C7/C8 cannot test outline-based methods. WP-3.9 proposes a generator that strips ToUnicode but keeps the programs. GAP-003 updated. |
| C6 | **WP-3.7 truncation strata.** On the same Common Crawl corpus, a 2026 preprint finds that truncated files are 23.06% of documents but hold 63.08% of the text, that PyMuPDF recovers several times what PDFium does from the same fragments, that under MuPDF 72.4% of truncated files open with no text, and that under PyMuPDF linearized files recover 1.1% of tokens against 18.5% (SRC-1107, CLM-1112..1116, GAP-355). | medium | **Accepted as design changes**, with the evidence graded as one unreplicated preprint. WP-3.7 adds linearization as a stratum and reports token-weighted results beside document-weighted ones. "Opens with no text" counts as plausible-but-wrong. The upper bound stays the union across engines (WP-3.2), since recoverability is engine-specific. |
| C7 | **Product assumption: lopdf as sole loader.** On all 1,100 REPDF files, hayro-syntax 0.8.0 alone opens C1 (100 of 100) and C10 Save As (50 of 50) with the right page count; lopdf and pdfrum open none. pdfrum beats lopdf on text recall for C5 Print, C6 and C9 (OBS-1103, GAP-356, 359). | medium | **Flagged to the board, not decided.** It conflicts with the current decision "lopdf sole emitter" (`docs/agents/domain.md`), so the chair does not rule on it. WP-4.2 adds the flag. WP-1.4 and WP-3.1 add hayro-syntax and pdfrum (TOOL-504) as harness loaders. P4 measures hayro's C10 page content and pdfrum's save path before the board rules. Two caveats: an open with the right page count may still be an empty page, and pdfrum's 0.485 precision on C6 is plausible-but-wrong text. |

**Searched and found nothing** (recorded, no plan change):
- learned, LLM or VLM PDF repair that beats REPDF or engine repair;
- CDR recovery accuracy (vendors publish sanitization tests only);
- an alternative to the C9 ladder (the 255-value search and the grammar localizer);
- the Print to PDF encoder's identity;
- a ToUnicode tool that applies to C8;
- PP-OCRv6 models for Arabic or Devanagari.

**The contrarian's "not checked" list** becomes P4 items:
- pdfrum's save path, and hayro's text and render on C10;
- TOOL-500 on `deflate_fast` and on producers other than REPDF's two;
- GAP-352's survivor counts;
- OCR at other DPIs, on more pages and with the 5.5.3 CLI;
- the untested Rust writers (TOOL-505, 508, 509) and precomp2;
- surya, OnnxTR and olmOCR;
- CPR Step 2, and Glasswall and OPSWAT hands-on (accounts not allowed);
- HG-CRC's code, and the design effect on our own calibration rows;
- how REPDF's exact-word metric segments Han text.

## Chair spot-checks (protocol: 5 claims against raw sources)
- **CLM-1106, 1107, 1108, 1110, 1112, 1114:** each paraphrase matches its quote, and the validator verifies every quote as exact. The one caveat is CLM-1114's "8.4×", which is the source's own figure: its rounded numbers give 11.4 / 1.4 = 8.1×. The plan cites the token and page figures, not the ratio.
- **OBS-1101 recount:** the summary's `damaged_streams` (887 Save As, 1,535 Print) counts every stream body that does not inflate cleanly, so it includes non-Flate streams. The Flate figures are the counterpart rows: 819 Save As plus 625 Print = 1,444, against OBS-0300's 1,445 (820 + 625). The missing Save As stream has a cause. In Sustainable_Fashion_Innovation(saveas), a separate C9 change at offset 178579 alters a `stream` keyword, which misaligns the script's stream regex. The 819 of 819 result stands.
- **TOOL-500 crate (the crates.io archive, sha256 `2823c223…ca6c2`, as the patch script pins it):**
  - `#![forbid(unsafe_code)]`;
  - no dependencies and no build script;
  - BSD-3-Clause, as a port of zlib 1.3.1, which is under the zlib licence.
  
  It is pure Rust as claimed.

## Plan delta
5k rev 2 changed 11 WPs (verifier). 5k rev 3 changes 13 WPs (contrarian): WP-1.4, 3.1, 3.2, 3.3, 3.5, 3.6, 3.7, 3.8, 3.9, 3.11, 3.12, 3.13 and 3.14, plus the WP-4.2 flag table. The verifier confirmed tools and the contrarian challenged method, so their findings do not overlap. No correlated-view warning applies.

## Escalations
No blocking objection was rejected, so nothing goes to the user from a rejection.

**Review board (Stage 5):**
- D3 re-opened: TOOL-500 against preflate-rs, and whether the product carries a patched crate;
- D4, narrowed to profiles' non-replay content;
- the new flag "lopdf as sole loader and emitter" (C7);
- the certification target against the document count (C4);
- D9 (an LLM/VLM fabrication trial);
- the five earlier flags (WP-4.2).

Replication seats: the DEFLATE reviewer runs TOOL-500 on all 1,444 counterparts.

**Access gate (one batch to the user, after this committee):** the items in `research/staging/verifier/access.md` and committee 004's gate list, minus the ones Stage 4 resolved:
- AWS is not needed (V10);
- the MuPDF pin is a chair decision (V5);
- NapierOne's bucket is reachable (OBS-1208);
- the contrarian needed no accounts.
