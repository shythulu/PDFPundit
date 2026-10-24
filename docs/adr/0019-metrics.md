# 0019. Metrics: a labelled regression gate, kept apart from REPDF-style OCR recall

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-063, D-014, D-067, D-068, D-069, D-070, D-095, D-151; GitHub issue #11; research CLM-1062, CLM-0016, OBS-0708, OBS-1102, 5k WP-3.2

## Decision

### Two metric families, never mixed

| | Text-layer regression gate | REPDF-style OCR recall |
|---|---|---|
| What | Word-level LCS-F1, plus recall, precision, bag F1, char F1 | Exact-word recall over OCR of the rendered output |
| Reference | The original's text layer | OCR of the original's render |
| Engine | hayro-interpret through its `Device` trait | Tesseract with the page script's model |
| Used for | CI gate on the 44-file smoke subset | Per-release number |
| Gate | Per class: fail on a drop of more than 0.020 absolute below the golden | Reported, not gated |
| Label | "regression baseline … not REPDF's metric" on every table | "REPDF-style", never "REPDF's" |

### Rules
1. "Nothing to repair" scores the input as is.
2. Tesseract runs in the harness only ([0004](0004-pure-rust-no-network.md)). It is chosen per script, never `-l eng` for everything.
3. Scores are never set beside the REPDF paper's tables. The paper's metric cannot be reproduced from its text, and its Tables 2 and 5 disagree (D-070).
4. No Google Document AI.

## Why

- The text layer misses what the page shows. 2 of 19 sampled original pages, both Print to PDF, hold under 60% of the visible text (OBS-0708).
- `-l eng` recovers 2.5 to 9.2% of non-Latin text (OBS-1102). A single-language OCR reference punishes correct non-Latin repairs.
- The "2%" here is a regression margin. It is unrelated to the research's 2% wrong-auto-pick certification target.

## Next additions (not v1 gates)

- A fabrication metric: planted tokens and a plausible-but-wrong rate. Needed before dictionary-ranked picks can be trusted ([0015](0015-font-recovery-ladder.md)).
- CER, an order-aware score and per-script strata with paired bootstrap confidence intervals (research WP-3.2).
- A word-segmentation rule for Han text before word F1 means anything there.

## Outstanding

- Tesseract is not pinned. The nightly job installs apt `tesseract-ocr` on `ubuntu-latest` (5.3.x) and has never run on a real binary. Research measured 5.5.3 with `tessdata_best`. Pin it (D-151 (b)).
- `bench/golden_smoke.csv` says C8 0.260 and C10 0.408. The tree measures C8 0.293 and C10 0.407. Re-baseline it.
