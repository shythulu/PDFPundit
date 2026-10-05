---
planStatus:
  planId: plan-bleeding-edge-repair-5k
  title: Bleeding-edge PDF repair — 5,000 ft work packages
  status: draft
  planType: research-program
  priority: high
  owner: shythulu
  stakeholders: []
  tags:
    - research
    - pdf
    - forensics
    - repair
    - evaluation
  created: "2026-09-27"
  updated: "2026-10-05T19:45:00.000Z"
  progress: 40
---
# Bleeding-edge PDF repair — 5,000 ft work packages

> **Altitude:** in the weeds. Each work package (WP) names its inputs by ID, its procedure step by
> step, its outputs and formats, its acceptance tests, its effort and its dependencies. Writing a
> coding-agent spec should mostly mean lifting a WP into a ticket.
> The broad view is in [bleeding-edge-repair-10k.md](bleeding-edge-repair-10k.md); definitions,
> success criteria and constraints are in [research/charter.md](../../research/charter.md).
> Tool picks cite `TOOL-` entries in `research/tooling/ledger.jsonl` (verdicts Adopt, Trial, Assess,
> Hold). Rev 1 fills the tool slots from the four Stage 2 ledgers and folds in PR #16's measured
> evidence (committee 004). Rev 2: Stage 4's verifier re-ran every Adopt and Trial pick hands-on on
> 2026-10-05, after the container restart, and installed every WP-3.1 pin (OBS-1200..1215, committee 005).

## How to read a WP
| Field | Meaning |
|---|---|
| Serves | research questions (10k), gaps, damage classes, hypotheses |
| Inputs | records, corpora and tools by ID or path |
| Procedure | numbered steps; commands where they are fixed |
| Outputs | paths and formats |
| Acceptance | pass/fail checks a reviewer can run |
| Effort | agent-sessions (one session ≈ one focused agent run of a few hours) |
| Depends on | WPs that must finish first |
| Decision points | where the user or the review board must choose |
| Unknowns | what could change the WP |

Common rules for every WP: the evidence rules in `research/agents/brief-template.md`; the
clean-room rule for copyleft code; no personal data committed; outputs validated with
`research/tools/kb_validate.py`; experiment scripts under `research/experiments/` with hashed inputs.
The research harness lives under `research/eval/` (Python 3.11, pinned in `research/eval/requirements.txt`).

---

## W1 Evidence base

### WP-1.1 P4 search plan and screener calibration
- **Serves:** RQ6; the recall shortfall (committee 002: carving 1 of 4); audit 002 actions A3–A5.
- **Inputs:** `research/sources/screening.jsonl`; `research/audits/002-screening/`; the hidden gold set (chair only); discovery services: Crossref (TOOL-300) and Unpaywall (TOOL-310), keyless; CORE (TOOL-306), which now refuses keyless requests (429 twice, 20 minutes apart; OBS-1208) and so needs a free key; OpenAlex (TOOL-304), which answers keyless but showed 0 of its daily quota left, so a key is recommended; Semantic Scholar (TOOL-305), keyless, with a key optional; DBLP (TOOL-303) is on Hold because an anti-bot page blocks it here.
- **Procedure:**
  1. Write one search recipe per community (forensics and carving, LangSec, compression, fonts/OCR, engines, evaluation, CDR, ML-assisted repair): the services, the query strings, the venue filters, and the snowball seeds.
  2. The carving recipe (tested on Crossref by Stage 2 agent A) restricts by container first and only then adds carving terms. Free-text "fragment reassembly" alone returned 76,911 noisy hits. The three routes are:
     - Digital Investigation (ISSN 1742-2876) up to 2012;
     - each year's DFRWS proceedings, walked from its front-matter DOI;
     - IFIP AICT *Advances in Digital Forensics*, with carving terms as a relevance boost.
  3. Adjacency rule: a transferable method is *include* only when it can be written as a PDF procedure; otherwise *maybe*.
  4. Calibration round: two screeners, 20 titles with abstracts, before the sweep. Target include-vs-not kappa ≥ 0.6; report PABAK beside it. If below target, refine the rule and repeat once.
  5. Every judged title is logged, including ones rejected on sight.
- **Outputs:** `research/reviews/p4-search-plan.md`; calibration results in `research/audits/004-calibration/` (003 is the independent review).
- **Acceptance:** calibration kappa ≥ 0.6, or the shortfall is reported to the board with the reason.
- **Effort:** 1 session. **Depends on:** Stage 2 ledger; the API-account gate (keys change the recipes).
- **Decision points:** which keyed services the user provisions.
- **Unknowns:** how much publisher bot walls block abstracts (17 of 31 in audit 002 were title-only).

### WP-1.2 Full-text and structure pipeline
- **Serves:** W1 throughput; quote verification.
- **Inputs:** `research/tools/fetch_fulltext.py`; PyMuPDF4LLM (TOOL-323, Adopt): in OBS-0601 it kept the heading structure and the two-column reading order that pdftotext and pdfplumber both spliced. For reference lists, trial GROBID CRF (TOOL-317). Stage 4 could not run it: the container has no Docker daemon, and setting one up is a provisioning item for the user (access gate). If Docker is not provided, GROBID moves to Assess and reference lists come from Crossref's `reference` field where the publisher deposits one.
- **Procedure:**
  1. Add an optional structured pass after the text cache: sections, reference list, and the limitations and future-work paragraphs.
  2. Pre-filter candidate limitation and future-work sentences for agents. The validator stays the only check: a pre-filtered sentence is a lead until its quote verifies.
  3. Pin the parser version and record it with each cached text.
- **Outputs:** structured sidecars beside the cached text (`research/cache/fulltext/<sha>.struct.json`, gitignored); a `--struct` flag on the fetcher.
- **Acceptance:** on 10 cached papers, references parse with ≥ 90% of Crossref's reference count; the limitations section is found where one exists.
- **Effort:** 1 session. **Depends on:** Stage 2 ledger.

### WP-1.3 Still-open checks
- **Serves:** the spec-ready rule (charter §6).
- **Inputs:** the 29 gaps with `still_open: unknown` (`research/gaps/gaps.jsonl`); citing works from OpenCitations (TOOL-301), which returns the list; Crossref's `is-referenced-by-count` (TOOL-300) is a count only. OBS-0600 found neither consistently ahead (Crossref higher on 3 of 10 works, OpenCitations on 2), and both show 0 for the 2024+ works.
- **Procedure:**
  1. For each gap's cited sources, collect citing works from OpenCitations.
  2. Where Crossref's count is higher than the list, fill in with Semantic Scholar or OpenAlex (both answer keyless, but OpenAlex's keyless quota runs out; OBS-1208), or record the shortfall.
  3. Screen the citing works and set `still_open` and `still_open_checked_at`, with the citing works logged.
- **Outputs:** updated gap records; a search-log entry per gap.
- **Acceptance:** every gap is `yes` or `no` with a date, or `unknown` with a stated reason (no citation data).
- **Effort:** 1 session. **Depends on:** WP-1.1 services.

### WP-1.4 Engine behaviour matrix (engine study, round 2)
- **Serves:** RQ2, RQ6, RQ7; GAP-250, 251, 252, 254, 256; board flags on §17.2, Adler-32 and encryption.
- **Inputs:** checkouts in `research/cache/code/` (qpdf, MuPDF, pdf.js, PDFium, Poppler, Ghostscript); new checkouts of PDFBox, pdfcpu, hayro and **lopdf** (the product's emitter), at the versions WP-3.1 pins. PR #16's engine rules CLM-1041..1056 are claims from its report, not code citations: no code was read for them in the ingest (agent E §4).
- **Procedure:**
  1. One row per behaviour: duplicate-object rule, trailer choice, ObjStm carving, truncated-stream handling, Adler-32 handling, lost /ID with encryption, header loss, cycle handling (DMG-101), expansion guard. Rev 1 adds a row for each PR #16 rule, to be confirmed or refuted by a code citation:

     | Row | PR #16 claim | Engines to cite |
     |---|---|---|
     | Stream length recovered only from a unique EOL-framed `endstream endobj` | CLM-1041 | lopdf; compare qpdf's CLM-1043 guard |
     | Object headers accepted only at line start | CLM-1042 | lopdf, qpdf, pdf.js |
     | startxref and /Prev corrected within ±64 bytes | CLM-1049 | lopdf |
     | Root candidates validated (Root → Pages → Kids or Count) | CLM-1048 | pdf.js, hayro |
     | Bare CR after `stream`; one EOL stripped before `endstream` | CLM-1051 | qpdf, hayro; PDFium (CLM-0526) |
     | Offsets rebased on a `%PDF` header in the first 1 KiB | CLM-1050 | qpdf |
     | Page-tree recovery: /Parent climb, duplicate drop, visited sets, depth cap 100 | CLM-1052 | qpdf |
     | Recovered containers over 5,000 elements ignored | CLM-1053 | qpdf 12.2+ |
     | Xref-stream trailer keys harvested (Root, Info, ID, Encrypt, Size) | CLM-1056 | MuPDF, PDFium, pdf.js, qpdf, pypdf |
     | Later duplicate wins only if it parses (last well-formed copy) | CLM-0517, CLM-1045, CLM-1044 | pdf.js, MuPDF |
     | ObjStm member takes its container's offset for precedence | CLM-1047 | MuPDF |
     | Stream extent when the zlib data ends in CR before an LF and `endstream` | GAP-301, CLM-1055 | all; does any engine use decompression to find the extent? |

  2. One column per engine, each cell a code citation at a pinned commit, or "not found".
  3. A differential test per row: a minimal crafted file per behaviour, run through every engine in WP-3.1. Code reading says what the engine intends; the run says what it does.
- **Outputs:** `research/engines/behaviour-matrix.md` (generated from claims); crafted files in `research/eval/crafted/` (synthetic, committed); an OBS per differential run.
- **Acceptance:** every cell is a verified code citation or an explicit "not found"; every row has a differential result. Each PR #16 rule ends as *confirmed* (a code citation agrees), *refuted* (one disagrees) or *not found*, and its CLM gets a `see:` tag to the citation.
- **Effort:** 2–3 sessions. **Depends on:** WP-3.1 for the runs.

### WP-1.5 Reading backlog and access list
- **Serves:** GAP-103, 105, 106 (still `unknown`).
- **Inputs:** unread sources named in the sweep reports (the CDR papers SRC-0215, 0219, 0223, 0224, 0218, 0208, 0207); paywalled items for the access gate.
- **Procedure:** read the open-access items with the fetcher; list the rest for the user.
- **Outputs:** claims and gap updates; an access list in the P4 report.
- **Effort:** 1 session.

---

## W2 Gaps → hypotheses

### WP-2.1 Gap ranking
- **Serves:** choosing what P5 tests.
- **Inputs:** all gaps; charter §3.
- **Procedure:** score each gap 1–3 on:
  - effect on success criteria (a), (b) and (c);
  - novelty (thin field vs crowded);
  - feasibility here (data reachable, tools verified);
  - evidence strength (verified claims and observations, not critiques).

  Rank by the product of the scores, then have one independent agent re-score the top 15 blind. Report agreement.
- **Outputs:** `scores` on each gap; `research/gaps/ranking.md`.
- **Acceptance:** the top 10 are stable under the blind re-score (at least 7 of 10 shared), or the board sees both lists.
- **Effort:** 1 session.

### WP-2.2 Settle the contradictions
- **Serves:** GAP-013 (REPDF's paper vs its dataset); GAP-017 (how common incremental updates are).
- **GAP-013 stays open (rev 1).** The dataset side is characterized: OBS-0001..0005, and the corrected C9 counts (1,445 damaged Flate streams, 7–26 per file; OBS-1000, OBS-0300 rev 2). Two parts are not settled:
  - the paper disagrees with itself: C10 Print to PDF is 35.35% in Table 2 but 30.49% in Table 5, and C9 60.11% vs 58.77/62.70 (CLM-1035, chair-checked);
  - which table WP-3.5 replicates against.

  Procedure: record WP-3.5's choice of table and why. Close GAP-013 only if the authors explain the tables (contacting them is the user's call) or the board rules the conflict immaterial to the tolerance.
- **Procedure for GAP-017:**
  1. Take the same natural-pair sample as WP-3.7.
  2. Count files with more than one `startxref`, more than one `%%EOF`, or a `/Prev` chain, with a committed script.
  3. Report by producer and year.
- **Outputs:** an OBS; GAP-017 status.
- **Acceptance:** the rate is reported with a 95% CI, and the gap between Caradoc's 2016 set and the 2021 crawl sample is explained or left open with a reason.
- **Effort:** 0.5 session. **Depends on:** WP-3.7 sample.

### WP-2.3 Hypotheses
- **Serves:** the spec-ready rule.
- **Inputs:** the top-ranked gaps; the drafts:

  | ID | Status | What it claims | Evidence so far |
  |---|---|---|---|
  | HYP-150 | draft | exhaustive single-byte search with strong oracles restores C9 streams byte-exactly | partly run by PR #16's mzbench: 218 of 221 inflate-error streams and 323 of 343 grammar-flagged streams byte-exact (OBS-1006, 1009); uniqueness untested |
  | HYP-151 | draft | intra-block resynchronization after the damage | restarts must be confirmed (OBS-0900); 984 of 1,445 damaged streams have no later block (OBS-0301) |
  | HYP-152 | needs-research | a per-file toolpath selector closes half the gap to the per-file oracle | none; design in WP-3.13 |
  | HYP-300 | draft | TrueType table checksums localize damage in Adler-only font streams | PR #16 E6, claim only (CLM-1026, no script) |
  | HYP-301 | needs-research | "last well-formed copy wins" beats "last in byte order" on truncated incremental updates | engine rules CLM-0517, 1044–1046; no test yet |
  | HYP-302 | draft | PR #16's checkpointed ladder beats prefix salvage on REPDF's own per-file metric | stream-level only: 562 of 1,445 streams byte-exact (OBS-1014); the per-file effect is GAP-300 |

  Leads that are not hypotheses yet: PR #16's ranked C9 ladder (CLM-1069) is HYP-150 + HYP-300 + HYP-302 together; the C10 content-byte ceiling of about 64% (CLM-1033) needs a committed census before anything rests on it.
- **Procedure:** each hypothesis gets a primary metric from WP-3.2, a `refute_if`, the baselines from WP-3.1, the data from WP-3.7–3.10, and its gaps' `still_open` dates. Hypotheses about C9 also report the uniqueness count and the false-accept KPI from WP-3.3. The board approves or rejects each one.
- **Outputs:** `research/hypotheses/hypotheses.jsonl`.
- **Acceptance:** validator passes the spec-ready checks for every hypothesis marked `spec-ready`.
- **Effort:** 1 session. **Depends on:** WP-2.1, WP-3.2.

---

## W3 Damage and evaluation

### WP-3.1 Baseline harness
- **Serves:** charter §3(b); GAP-009, 053, 102, 256; HYP-152.
- **Inputs:** the null-repair baseline (tolerant readers opening the damaged file directly), and these engines with their best-of option sets (agent B, committee 004):

  | Engine | TOOL | Pin | Option sets (`config_id`) | Licence |
  |---|---|---|---|---|
  | qpdf | 350 (Adopt) | **12.4.2** static release binary; never apt 11.9.0, which segfaults on OBS-0500's file (OBS-0701) | `default` (`qpdf in out`, recovery on, CLM-0702); `ignore-xref-streams`; `decode-specialized` (`--decode-level=specialized --object-streams=disable`); strict control `--suppress-recovery` | Apache-2.0 |
  | MuPDF `mutool` | 351 (Adopt) | **1.28.0** (conda-forge). A 1.28.5 source build regresses on C1: `draw -F txt` exits 1 on two C1 files and extracts about half the text 1.28.0 gets (OBS-1203). Run 1.28.5 as a second build to track it | `clean -gg`; `clean -gggg`; `clean -gggg -s` (CLM-0704) | AGPL, binary only |
  | PyMuPDF | 352 (Adopt) | 1.28.2 (MuPDF 1.28.2) | open + save; also the first renderer | AGPL, binary only |
  | Ghostscript | 358 (Adopt) | 10.08.0 (conda-forge) | `-o out.pdf -sDEVICE=pdfwrite` (repairs by default); `-sDEVICE=txtwrite`; strict control `-dPDFSTOPONERROR` (CLM-0705); force the PDF interpreter on header loss (OBS-0501) | AGPL, binary only |
  | PDFBox | 356 (Adopt) | 3.0.8 | `decode in out`; `export:text` | Apache-2.0 |
  | pdf.js | 353 (Adopt) | pdfjs-dist 6.4.299 (verified, OBS-1202) | defaults (`stopAtErrors` false, CLM-0706) | Apache-2.0 |
  | pypdfium2 | 354 (Adopt) | 5.14.0 (verified) | open + `save()`; also the second renderer | BSD-3 / Apache-2.0 |
  | Poppler | 357 (Adopt) | 26.10.0 (source build, HarfBuzz off; verified). conda-forge stops at 26.09.0 | `pdftotext`; `pdftocairo` as a third renderer | GPL, binary only |
  | pikepdf | 426 (Adopt) | 10.16.0 (verified) | open + save | MPL-2.0 |
  | pdfcpu | 355 (Trial) | 0.16.1 (verified) with its own `XDG_CONFIG_HOME`: it refuses every command if it finds a 0.15.0 config | `optimize`; `validate -m strict` belongs to WP-3.3 | Apache-2.0 |
  | lopdf | 359 (Adopt) | 0.45.0 | load + save. The weakest loader (0 of 2 on C1, C4, C5; OBS-0700), so it measures the product emitter's starting point, not a strong baseline | MIT |
  | hayro | 361 (Trial) | 0.8.0 (verified). Its render API changed; the probe needs `research/experiments/stage4-verifier/rsprobe080/main.rs.patch` | load + render; its per-class output matched pdf.js in OBS-0700 | Apache-2.0 / MIT |

  CPR (WP-3.6) and REPDF (WP-3.5) are references with their own WPs, not harness adapters.
- **Procedure:**
  1. One adapter per tool: `repair(in_path, out_dir, config_id) -> RunRecord`. Copyleft engines run as external binaries only; no copyleft code enters the harness or the product.
  2. Each run is in a subprocess with a wall-clock limit (default 60 s), a memory limit (default 2 GB) and no network.
  3. Every output is extracted and rendered by ≥ 2 engines (WP-3.2).
  4. Versions come from the pins above; the adapter records the version string the tool reports. Installs: static release binaries (qpdf, pdfcpu), conda-forge (mutool, gs, Poppler), the Maven jar (PDFBox), npm (pdf.js), PyPI (PyMuPDF, pypdfium2, pikepdf) and crates.io (lopdf, hayro). The hosts are listed at the access gate. The install steps that worked after the restart, without apt, are in the headers of `research/experiments/stage4-verifier/rerun_smoke.sh` and `research/experiments/stage4-verifier/build_pins.sh`.
  5. Score outputs, not exit codes. OBS-0700: no engine produced anything on C10, only mutool produced output on C4 (1 page kept), and every text engine stayed under a 0.95 text ratio on C9. "Produced output" is not fidelity.
- **Outputs:** `research/eval/harness/`; runs as JSONL: `{input_sha256, tool, version, config_id, exit_code, signal, wall_ms, max_rss_mb, output_sha256, text_paths, render_dir, stderr_tail}`.
- **Acceptance:**
  - reruns of the same input and config give the same output hash (or the tool is marked non-deterministic);
  - a timeout, a crash and a segfault are each recorded, not fatal (OBS-0500's qpdf segfault is the test);
  - the header-loss case scores Ghostscript with PDF forced, per OBS-0501.
- **Effort:** 2 sessions. **Depends on:** Stage 2 ledger (re-verified: the engine and oracle smoke tests reproduce exactly at seed 20260927, OBS-1200, OBS-1201).
- **Decision points:** the option sets per tool are frozen in the pre-registration.

### WP-3.2 Metric suite
- **Serves:** charter §3 metric suite; GAP-008, 055, 201; RQ4.
- **Inputs:** originals and outputs; these libraries (agent B, committee 004):

  | Measure | Tool | Verdict | Note |
  |---|---|---|---|
  | CER, WER | rapidfuzz 3.14.6 (TOOL-372), jiwer 4.0.0 (TOOL-373) | Adopt | they agreed on every file (OBS-0704) |
  | Grapheme-aware CER for multilingual pages | dinglehopper 0.11.0 (TOOL-374) | Trial | re-verified, OBS-1213 |
  | Flexible character accuracy | built here over rapidfuzz alignments (TOOL-380) | Assess | no package exists (SRCH-0705); FCA's paper is cited from memory and needs a source |
  | Text reference | Tesseract 5.5.3 (TOOL-382) on the original's render, `OMP_THREAD_LIMIT=1 tesseract page.png - -l eng --psm 3`, about 1 s per page | Adopt | the original's text layer is not the reference: 2 of 19 Print to PDF originals hold only 50–60% of the visible text in it (OBS-0708) |
  | Second OCR vote | docTR 1.1.0 (TOOL-383), 6–7 s per page | Trial | re-verified, OBS-1213 |
  | SSIM | scikit-image 0.26.0 (TOOL-375) | Adopt | within one renderer only: PyMuPDF and pypdfium2 differ by up to SSIM 0.904 on undamaged originals, which overlaps C7/C8 damage (OBS-0706) |
  | Perceptual distance | LPIPS 0.1.4, AlexNet, CPU (TOOL-376) | Trial | re-verified, OBS-1213; 0.17 s per pair; C9 damage 0.42–0.57 vs renderer floor ≤ 0.049 |
  | Structure diff | DeepDiff 9.1.0 over `qpdf --json=2 --json-stream-data=none` (TOOL-377) | Assess | canonicalize the object graph first: a rewrite renumbers objects and raw diffs explode |
  | Planted tokens | built here, modelled on FaithC4 (TOOL-379) and scored with rapidfuzz | Assess | do not vendor FaithC4's scorer, which depends on AGPL and GPL libraries |
  | VLM OCR | olmOCR-2, LightOnOCR-2, PaddleOCR-VL, DeepSeek-OCR-2 (TOOL-384) | Hold | none shown feasible on 4 CPUs; their language priors are the GAP-201 risk |

- **Metrics:**
  - **Text:** CER after NFC normalization and whitespace folding; flexible character accuracy (robust to reading order); word F1; an order-aware score (normalized edit distance over the word sequence).
  - **REPDF-comparable:** REPDF's metric, OCR of original and output renders and exact word matches (CLM-0016), computed separately. PR #16 shows the design's Dice over a Myers alignment equals LCS-F1 and is not comparable with REPDF's OCR word recall (CLM-1062), so the two are never mixed in one table.
  - **Visual:** per-page SSIM at 150 dpi, grayscale, within each of two renderers (PyMuPDF and pypdfium2; pdftocairo as a third); a missing page scores 0; page-count match. LPIPS beside it if its Trial holds.
  - **Structural:** page count and order, fonts per page, image XObjects, annotations, outline; conformance oracle result (WP-3.3).
  - **Cross-engine consistency:** pairwise text and visual agreement between renderers of the same output.
  - **Fabrication rate:** output content that cannot be aligned to the original, by provenance grade.
  - **Omission rate:** content in the upper bound missing from the output. The upper bound is the content recoverable from the damaged file's surviving bytes: the union of every engine's extraction and a raw object and stream carve, aligned to the original.
  - **Planted-token faithfulness:** originals carry nonce tokens outside any dictionary; count nonces reproduced exactly vs "corrected" into words (GAP-201).
  - **Plausible-but-wrong rate:** the share of outputs that pass every gate (WP-3.3 oracles, the selector's checks) yet fall below the target recovery (CLM-1059). In program repair, 104 of 110 plausible GenProg patches deleted functionality (CLM-1058, cited second-hand), so this is the KPI that catches a repair that passes its own checks.
  - **Runtime and failure:** wall time, crash, timeout.
- **Statistics:** per class, paired per-file bootstrap (10,000 resamples) 95% CIs; Holm correction across classes; the margin is fixed in the pre-registration.
- **Outputs:** `research/eval/metrics/`; a scores table per run (Parquet or CSV).
- **Acceptance:**
  - unit tests on hand-made cases: identical file (all perfect), empty output (fabrication 0, omission 1), a shuffled page (text scores fall only in the order-aware measure);
  - the upper bound never exceeds the original's content;
  - a Print to PDF original whose text layer is incomplete (OBS-0708's two files) scores CER ≤ 1 against the OCR reference.
- **Effort:** 3 sessions. **Depends on:** WP-3.1.
- **Unknowns:** the alignment method for fabrication on non-Latin scripts; OCR quality as a reference on Arabic, Devanagari and Han pages (Tesseract's language packs per script are a P4 install).

### WP-3.3 Verification oracles
- **Serves:** provenance grades 2 and 3; GAP-052, 106, 151, 153, 154, 253; HYP-150, 151.
- **What changed in rev 1.** Three Stage 2 findings reshape this WP:
  - **Replay is not uniqueness.** One Save As ToUnicode stream has 3 single-byte candidates that pass inflate, Adler-32 and stock-zlib replay together: the true byte, an Adler collision and a one-byte trailer rewrite (OBS-0903). 87 of 820 damaged Save As streams are themselves canonical zlib output, so a trailer rewrite passes all three oracles (OBS-0904). Charter rev 2.3 grade 3 therefore needs exhaustive enumeration with trailer edits, a body-over-trailer prior and a grammar check.
  - **Adler-32 false accepts are measured:** first-accept rates of 1 in 245 (small streams, full-range search) and 1 in 324 (grammar-localized) (OBS-1007, OBS-1009). Searching the 4 trailer bytes admits a further wrong accept; excluding them leaves trailer damage (2 of 1,445 streams) unfixable (OBS-1013).
  - **Replay coverage is producer-specific:** with memLevel swept 1–9, stock zlib 1.3 reproduces 2,074 of 2,074 sampled Save As streams, but Print to PDF only 1 of 1,264 (OBS-0901). The old "54%" (OBS-0303) was level 6 at the default memLevel only.
  - **The miniz_oxide version mismatch is resolved:** PR #16's search ran on miniz_oxide 0.8.9 while the design pins 0.9 (CLM-1067). The checkpoint structs are the same size in both, and the inflate-error search gives the same result on 0.9.1: 219 of 221 fixed, all byte-exact (OBS-1005, OBS-1015).
- **Inputs:**

  | Oracle | Tool | Verdict | Evidence |
  |---|---|---|---|
  | Encoder replay, research | stock zlib 1.x (TOOL-450, TOOL-364), memLevel 1–9 × strategy × level sweep | Adopt | OBS-0901 |
  | Encoder replay, product (pure Rust) | preflate-rs 0.7.6 token predictor with zero corrections and producer-profile parameters (TOOL-476; crate entry TOOL-366) | Trial | 395 of 400 Save As streams byte-for-byte; 47 of 399 with per-stream parameter estimation; 0 of 400 Print to PDF; 14 panics with forced parameters; the probe mirrors a private struct (OBS-0908). zlib-rs 0 of 400 and miniz_oxide 5 of 400 cannot replay (OBS-0902) |
  | Damage signal from correction size | preflate-rs round trip (TOOL-366) | Trial | 674 of 676 streams round-trip; corrections grow at damage (OBS-0703) |
  | Block structure, fault diagnosis | infgen 3.6 (TOOL-368) | Adopt | OBS-0707 |
  | Restart confirmation for resync | two-dummy-dictionary decode only at restarts confirmed by trailer alignment (TOOL-456) | Trial | blind scan: 51,120 false starts certified 8.5 M wrong bytes; trailer alignment rejects all and keeps 486 of 807 true starts (OBS-0900) |
  | Single-byte search | PR #16's checkpointed 255-value search (mzbench, SRC-1001) plus the replay arm (TOOL-457) | Trial | 218 of 221 inflate-error streams byte-exact (OBS-1006) |
  | Content grammar, localizer | PR #16's `grammar_loc.py` (`research/experiments/pr16/`) | Trial | flags 343 of 403 Adler-only content, CMap and text streams, always at or after the damage (OBS-1008); search in [flag−256, flag+8] fixed 324 of 343 (OBS-1009) |
  | Content grammar, checker | DaeDaLus PDF DDL rules for CMap and content streams, ported to Rust (TOOL-471); grmtools as a token-level repair generator (TOOL-473) | Assess | not built (needs GHC) |
  | Font checks | TrueType table checksums as oracle and localizer (HYP-300; PR #16 E6, claim only, CLM-1026); OTS (TOOL-466) as an independent check; fontTools (TOOL-464) | Draft / Assess / Adopt | E6 has no script; OTS not run |
  | Conformance | delta against the original across the Arlington checker in veraPDF 1.30.2 (TOOL-362), `qpdf --check` (syntax only, CLM-0703) and `pdfcpu validate -m strict` (TOOL-355) | Trial | 13 of 19 clean originals fail Arlington; the three disagree on 13–14 repaired files (OBS-0702) |
  | Hold | gzrt (wrong bytes or nothing, OBS-0705), rapidgzip (silent wrong bytes in 53 of 54, OBS-0707), zlib-ng (TOOL-365, 451) | Hold | |

- **Procedure:**
  1. **Encoder replay.** Per file, find the zlib level, strategy, window and memLevel that reproduce each intact Flate stream (research: TOOL-450 sweep; product: TOOL-476 with the file's producer profile, WP-3.14). Record the share of intact streams reproduced per producer. Where it is 0 (Print to PDF), replay is unavailable and that file's corrections cannot reach grade 3 by replay.
  2. **Candidate enumeration.** For each damaged stream, enumerate every single-byte candidate in the localized window and in the 4 trailer bytes. Run every oracle on every candidate and keep all that pass; never stop at the first accept. Localizers, in order: the inflate-error offset (search backward from it), the grammar flag ([flag−256, flag+8]), the first failing TrueType table (HYP-300), and the full range for streams up to 4 KiB.
  3. **Uniqueness and prior.** Grade 3 needs exactly one surviving candidate after all oracles, with body edits preferred over trailer edits by a stated prior. Two or more survivors: grade 4 at best, with the alternatives listed in the provenance record (WP-3.4).
  4. **Resynchronization.** Dictionary-invariant bytes count (grade 2) only from a restart confirmed by trailer alignment or a following decodable block.
  5. **Conformance oracle.** Run the triad on the original and the output; score the delta (new failures, fixed failures), never raw pass/fail.
  6. **Content grammar.** Operators, operand counts and types on decoded content streams; CMap syntax on ToUnicode streams. Start with `grammar_loc.py`'s rules; port the relevant DaeDaLus rules if the Assess succeeds.
  7. **Font checks.** /Length1..3, table directory and table checksums; OTS on the result.
- **Outputs:** `research/eval/oracles/`; per damaged stream, a JSONL row `{file, stream, localizer, window, candidates_tried, survivors: [{offset, value, in_trailer, oracles_passed}], grade, wall_ms}`; per file, the conformance delta.
- **Acceptance** (on REPDF C9, SRC-0002, all 1,445 damaged Flate streams):
  - report the survivor count per stream, per oracle combination;
  - report the **first-accept false-accept rate** with a Wilson 95% interval, beside the survivor counts; it must reproduce OBS-1007 and OBS-1009's 1/245 and 1/324 for Adler-32 alone;
  - the grade-3 combination accepts no candidate that differs from the original. If one does, that combination is not grade 3; record the counter-example as an OBS;
  - OBS-0903's stream is a fixed regression case: its three survivors must all be listed and the result must not be grade 3 unless the grammar check removes the two wrong ones.
- **Effort:** 3 sessions (was 2). **Depends on:** WP-3.1; WP-3.14 for product replay.
- **Unknowns:** why preflate-rs misses 4 memLevel-7 streams and panics on forced parameters; whether replay still works on a damaged stream's undamaged prefix; whether preflate's misprediction bursts localize damage in Print to PDF streams (all unchecked, D §5).

### WP-3.4 Provenance record
- **Serves:** charter §3(c); GAP-010, 050; spec-ready rule (the schema must exist).
- **Inputs:** TOOL-427:
  - DFXML (dfxml_python 1.0.2, CC0 or LGPL-3.0). A round trip was verified in OBS-0801. It is not on PyPI; install from its git repository at a pinned commit.
  - CASE/UCO 1.5.0 (Apache-2.0).
  - The structure of NIST CFTT test specs. Its lack of a document-repair category is carried over from memory and still to be re-checked.
- **Procedure:**
  1. Write a JSON Schema for the per-repair record. It holds:
     - input and output hashes;
     - tool, version and configuration;
     - the provenance grade of each element, with byte ranges;
     - tamper indicators found;
     - learned-model versions and checksums;
     - repair choices where engines differ (GAP-101).
  2. Export the per-file layer (hashes, sizes, byte ranges) as DFXML `fileobject` records.
  3. Export the repair event (tool, action, evidence used) as a CASE/UCO action chain.
  4. Test-spec documents follow the CFTT layout: assertions, test cases, results.
- **Outputs:** `research/schemas/provenance-record.schema.json`; an example record for one REPDF file.
- **Acceptance:** a forensics-practitioner review on the board signs it off; the example validates.
- **Effort:** 1 session.

### WP-3.5 REPDF replication (gate before P5)
- **Serves:** charter §3(b); RQ2, RQ3; GAP-013, GAP-300; HYP-302.
- **Inputs:**
  - SRC-0001 (paper; CLM-0001, 0002, 0016, 0018, 0020, 0021) and SRC-0002 (corpus at commit e547d4d); OBS-0001..0005.
  - CLM-1035: REPDF's Table 2 and Table 5 disagree (C10 Print to PDF 35.35% vs 30.49%; C9 60.11% vs 58.77/62.70). GAP-013 stays open.
  - CLM-1062: our planned text metric (LCS-F1) is not REPDF's OCR word recall, so WP-3.2's REPDF-comparable metric is the one used here.
  - CLM-1065: REPDF publishes only aggregates, so the claim we can defend is non-inferiority per stratum.
  - OCR: Tesseract 5.5.3 (TOOL-382, Adopt) as the documented substitute for Google Document AI, and docTR (TOOL-383, Trial) as a second vote. WP-3.2's OCR-reference acceptance check (OBS-0708's two incomplete text layers) passes first.
- **Routes, in order:**
  1. black-box runs through repdf.site, if the user allows the host (it is blocked here; access gate);
  2. the authors' code, if the user asks for it (the user's call only);
  3. a reimplementation from the paper (Python, pikepdf and fontTools, as the paper states in CLM-0018).
- **Procedure:**
  1. Record which table the run replicates (Table 2 or Table 5) before any number is computed, and why.
  2. OCR every original's render with Tesseract at a fixed DPI and language set; this is the reference (WP-3.2: OCR of the original's render, not its text layer).
  3. Run the replicated pipeline (route 1, 2 or 3) and OCR each output the same way.
  4. Score REPDF's metric (exact word matches of output OCR against original OCR, CLM-0016) and our metrics beside it, per class and per producer.
  5. **Attribution run (HYP-302, GAP-300):** score the same files with only the C9 Flate streams swapped between prefix salvage and PR #16's checkpointed search (WP-3.3 output). This converts stream-level gains into REPDF's per-file metric. Report the per-file delta with a paired bootstrap CI.
  6. Write the non-inferiority table: per stratum, our lower 95% bound vs REPDF's point estimate minus the margin fixed in WP-3.12.
- **Outputs:** `research/eval/repdf/replication.csv` (`file, class, producer, route, repdf_metric, lcs_f1, cer, ocr_engine, ocr_version`); `research/eval/repdf/attribution.csv`; an OBS per table.
- **Acceptance:**
  - per-class means within ±5 points (fixed before the run) of the chosen table, or each difference explained by OBS-0001..0005, by the OCR substitute (measured by re-scoring a subset with docTR) or by CLM-1035's table disagreement;
  - the attribution run reports a per-file delta for C9 with a CI, whatever its sign;
  - the table choice is written down before the scores.
- **Effort:** 3 sessions (route 3), plus 1 for the attribution run. **Depends on:** WP-3.1, 3.2; WP-3.3 for step 5.
- **Decision points:**
  - the paper's OCR is Google Document AI, a hosted paid service. Tesseract changes the metric; the user chooses between access and the documented substitute (access gate);
  - the user's call on routes 1 and 2;
  - closing GAP-013 needs the authors (the user's call) or a board ruling.
- **Unknowns:** Tesseract's accuracy on REPDF's non-Latin documents (Arabic, Devanagari) is unmeasured; if it is poor, those strata are reported separately and not pooled.

### WP-3.6 CPR baseline (gate before P5)
- **Serves:** RQ3; GAP-003, 004, 200, 201.
- **Inputs:**
  - SRC-0123 (the CPR paper) and SRC-0700 (its code, BeenyHail/CPR at 9ecd685). Licence CC BY-NC 4.0 (CLM-0700): run as a baseline only, never copied (clean-room rule). TOOL-385, **Assess**.
  - **Step 1 (no LLM)** ran on 20 sampled REPDF files after patching 28 Windows-only paths (OBS-0704; reproduced exactly in Stage 4, OBS-1212). It fell below the PyMuPDF null baseline on C1–C3 and C7–C9, beat it on C4, C6 and one C5 file, and recovered nothing from either C10 file.
  - **Step 2** calls llama3.1:8b (Q4_K_M, 4.92 GB) through a local Ollama server (TOOL-386, Assess). registry.ollama.ai answers (the model manifest returns 200, OBS-1208), but no blob was fetched; the download is an access-gate item. As published it sets no temperature or seed (CLM-0701), so it has no deterministic mode.
  - OBS-0907: in Print to PDF files every embedded TrueType program keeps its family in the `name` table, so font identity comes from there first; CPR's font matching is compared against that.
- **Procedure:**
  1. Apply the local determinism patch (temperature 0, fixed seed in the Ollama payload, CLM-0701) and record its diff in `research/eval/cpr/PATCH.md`. Never commit CPR code itself.
  2. Pull the model by digest; record the digest and Ollama version in every output.
  3. Run Step 1 + Step 2 on all C7 and C8 files and on the UDHR pairs (WP-3.8). Run Step 2 twice with the same seed and check the outputs are identical.
  4. Score with WP-3.2's metrics and the plausible-but-wrong rate; for glyph-to-Unicode outputs, count language-prior fabrications (GAP-201): characters emitted where the font gives no evidence.
  5. Compare against CPR's published numbers and against our null baseline (PyMuPDF open).
- **Outputs:** `research/eval/cpr/results.csv`; an OBS per run.
- **Acceptance:** CPR's published numbers reproduced within a stated tolerance, or the difference explained; two seeded runs are byte-identical; the fabrication count is reported.
- **Effort:** 1–2 sessions, **time-boxed in P4**. **Depends on:** WP-3.1, 3.2; the model download (access gate).
- **Decision points:** if the download is not allowed, CPR stays Assess with Step 1 results only, and the board decides whether the gate before P5 can pass without Step 2.
- **Unknowns:** whether seeded Ollama output is stable across CPU builds; whether CPR's published runs used a different model build.

### WP-3.7 Natural pairs: truncation
- **Serves:** charter §3(a) source 1; GAP-100, 006, 017; DMG-100.
- **Inputs:** OBS-0201 (34 of 35 pairs are exact prefixes); the CC-MAIN-2021-31-PDF-UNTRUNCATED corpus (TOOL-404, SRC-0206).
  - Its metadata CSV and ZIP central directories are range-read from digitalcorpora's S3 without credentials (TOOL-402, OBS-0800): 12,946 metadata rows in 1.3 s.
  - Stage 2 wrote `research/experiments/cc_natural_pairs_other_zips.py`, which extends OBS-0201's sampler to later ZIPs. Its two runs timed out with no pairs because of the 403s below (OBS-0800).
  - The truncated captures come from data.commoncrawl.org (TOOL-403). Ranged reads there went from 206 to 403 after a burst of requests on 2026-09-27. Paced at 2–3 s with back-off on 403, Stage 4 got 30 of 30 ranged reads (206) and read a real WARC record of a truncated PDF (OBS-1207). No AWS account is needed; the sampler keeps the pacing.
- **Procedure:**
  1. Stratify by source ZIP, truncation reason (length cap vs disconnect), size bucket and producer.
  2. For each pair, check that the truncated file is a byte prefix of the refetched file. Reject any pair that isn't.
  3. Keep URLs and hashes in the repository; files only in the cache.
- **Outputs:** `research/eval/pairs/truncation.jsonl`: `{url, truncated_sha256, full_sha256, truncated_len, full_len, reason, zip, producer}`.
- **Acceptance:** ≥ 1,000 pairs, each byte-verified as a prefix; within about 10 GB. Disconnect truncations are rare: in the first ~13,000 metadata rows of ZIP 0001, 1 of 255 truncated rows is a disconnect. So scan the metadata across all ZIPs and take every disconnect row, aiming for ≥ 100. Scanning is cheap; only the byte fetches are slow. Report the achieved count and don't pad it.
- **Effort:** 1 session.
- **Decision point:** Stage 4 found paced reads work (OBS-1207), so byte-verified pairs are the plan. If this WP finds Common Crawl refusing ranged reads again, the fallback builds each truncated file by cutting the refetched file at the capture length recorded in the metadata. Those pairs carry the grade *metadata-derived, not byte-verified*, and the OBS-0201 check (34 of 35 exact prefixes) is the evidence that the cut is faithful. The board decides whether such pairs may score fidelity or only robustness.

### WP-3.8 Natural pairs: font encoding
- **Serves:** charter §3(a) source 2; GAP-202, 201, 203; DMG-200, 201.
- **Inputs:**
  - **The UDHR PDFs from OHCHR** (CLM-0415: 21 of 526 corrupted). www.ohchr.org returns 403 here. archive.org lists 2022 snapshots, but web.archive.org resets connections from this container. This is an access-gate item: allow one of the hosts, or the user downloads the PDFs.
  - **Reference texts:** unicode.org stopped hosting UDHR in Unicode in January 2024 (TOOL-405). Use NLTK's `udhr2` package instead: public domain, 1,653,975 bytes, sha256 `0796c314…66c7f3`, reachable on raw.githubusercontent.com.
  - **Alignment:** difflib (TOOL-406) as a first pass.
- **Procedure:** confirm the OHCHR terms; extract text from each PDF; align it to the reference; classify the failure (no ToUnicode, wrong ToUnicode, producer codes); keep only pairs whose reference matches the PDF's edition.
- **Outputs:** `research/eval/pairs/udhr.jsonl`.
- **Acceptance:** each pair is verified by a second extractor; the terms are recorded.
- **Effort:** 1 session. **Decision points:** the user, if the terms restrict research use.

### WP-3.9 Damage models and generators
- **Serves:** RQ1; charter §3(a); DMG-001..016, 050, 100, 101, 200, 201, 250, 251.
- **Inputs:** WP-3.7 truncation lengths; NAND bit-flip rates (SRC-0126, CLM-0341); fragmentation statistics (SRC-0137).
  - **What REPDF's C9 actually is:** uniform random byte replacement, 12–30 changed bytes per file, at most one per damaged Flate stream; only 68 of 2,162 changes (3.15%) are single-bit (OBS-1000). The NAND rates therefore parameterise a separate bit-flip model (DMG-007), not C9. Our C9 generator reproduces OBS-1000's statistics; it does not borrow NAND parameters.
  - **Producer profiles** (WP-3.14) set each generator's producer-specific details: EOL style, xref offset bias (+1 for Print to PDF, OBS-0905), encoder parameters (OBS-0901), font instances per page (OBS-0907).
  - **New damage classes from PR #16** (CLM-1079): ObjStm and xref-stream damage (DMG-004, 005), incremental chains (DMG-008, 014), linearized files, CR/LF mangling (DMG-010), sector holes (DMG-002), multi-fault (DMG-001). Linearized-file damage has no DMG record yet; this WP proposes one.
  - **The EOL edge case** (GAP-301): a Flate stream whose zlib data ends in CR, followed by LF and `endstream`. Stripping one EOL is ambiguous there, and one C9 change in REPDF hit exactly that CR (OBS-0302 rev 2).
  - **Generators:**
    - radamsa and zzuf (TOOL-421, 422) for byte-level mutation. Both were verified in OBS-0801.
    - The Arlington TSV grammar (TOOL-424, Apache-2.0, trial) for grammar-aware invalid objects, which fit DMG-009's templates better than blind mutation.
    - pikepdf (TOOL-426) for structural edits.
  - **Producers for originals:** LibreOffice, ps2pdf, Chromium print-to-PDF, pdflatex, reportlab and pycairo (TOOL-413..418, verified in OBS-0801). After the restart all but LibreOffice pass again (OBS-1209): its Writer module is gone and reinstalling it needs apt (access gate). pdflatex now comes from a scratch TeX Live 2026 install (`research/experiments/stage4-verifier/texlive_install.sh`). macOS Quartz output (DMG-201) can't be produced here (TOOL-420); source it from an existing corpus or a contributor's Mac.
  - **Open questions:**
    - Parameter datasets: NapierOne (TOOL-408, open licence). Its bucket now lists through the path-style URL `s3.eu-north-1.amazonaws.com/napierone.com/` (OBS-1208); no data file has been read yet.
    - The NAND data's licence is unconfirmed (TOOL-407).
    - The DFRWS 2006 and 2007 challenge images (TOOL-409) are candidates for carving damage.
    - The shadow-attack artefacts (TOOL-425) have no licence: study and cite them only, never copy them.
- **Procedure:**
  1. Fit each parameter to its source: the truncation-point distribution, bit-flip rate and burst shape, fragment-size distribution, and for C9 the per-file change count and per-stream placement (OBS-1000).
  2. Write a generator spec per DMG: parameters, their fitted values, the producer profile it applies to, and a seed.
  3. Add fixtures: one C9 file with one random byte per damaged stream (CLM-1069's fixture); the CR-before-LF-before-`endstream` stream (GAP-301), intact and with that CR changed.
  4. Validate: the generated damage's statistics match the source within its CI; the C9 generator matches OBS-1000's changed-byte histogram and single-bit share.
- **Outputs:** `research/damage/models/`; generator specs in the DMG records; fixtures under `research/eval/fixtures/` (generated, hashed, not hand-edited).
- **Acceptance:** every DMG used in the held-out suite is `accepted`, citing a verified claim or OBS; the C9 generator's per-file change count and single-bit share fall inside OBS-1000's ranges; both GAP-301 fixtures exist and WP-1.4 records how each engine handles them.
- **Effort:** 3 sessions. **Depends on:** WP-3.7, 3.14.

### WP-3.10 Robustness corpora and differential taxonomy
- **Serves:** GAP-257, 102, 105; the red team's corpus-first alternative (committee 001).
- **Inputs:** OBS-0500 (2,873 engine regression files); the SafeDocs Issue Tracker corpus (CLM-0293), subset plan in TOOL-411; GovDocs1 (TOOL-412, public domain) as a lower-risk volume source; peepdf-3 (TOOL-423, GPL-3.0, run only as an external command) for triage.
- **Procedure:**
  1. Apply the PII procedure (`research/staging/tooling-damage/report.md`) before any file is kept:
     - screen the filename and the tracker entry before download;
     - download into a temporary quarantine outside the repository and the cache;
     - check the Info dictionary, XMP and page-1 text;
     - delete rejects at once and log only the issue id and "excluded: PII";
     - if in doubt, exclude.
     
     The quarantine step is a chair fix: the procedure as written asks for metadata checks "before caching" but also "never cache, even temporarily", which can't both hold.
  2. Run every engine differentially (WP-3.1).
  3. Cluster failures by signature (engine × error × structural feature) into a taxonomy mapped to C1–C10 and DMG.
- **Outputs:** `research/eval/robustness/`; a taxonomy OBS.
- **Acceptance:** every cluster has ≥ 3 files and a mapping or a new DMG proposal.
- **Effort:** 2 sessions.

### WP-3.11 Held-out suite
- **Serves:** charter §3(a); CLM-1064, CLM-1066.
- **Inputs:** WP-3.9 damage models and producer profiles; WP-3.7 and 3.8 natural pairs.
  - **Grouped splits:** REPDF's 1,000 damaged files come from 50 base documents, so any split of REPDF must group by base document (CLM-1064). A 60/20/20 grouped split leaves only about 10 base documents in the sealed test, so REPDF can supply a development set but not the held-out suite.
  - **Held-out corruptors** (CLM-1066): a corruptor family that method work never sees (multi-byte bursts, variable truncation, shifted xrefs, partial `/Kids` damage), run only at release as the overfitting indicator.
- **Procedure:**
  1. An agent with no access to the method work authors the suite: originals and producers disjoint from SRC-0002, damage from WP-3.9 models plus natural pairs from WP-3.7 and 3.8.
  2. Split every set by base document (and by producer where a producer has fewer than 5 documents); record the grouping key per file.
  3. Seal the held-out corruptor family separately: its generator specs, seeds and outputs live only in the sealed manifest until release.
  4. Freeze by hash.
  5. The user signs off.
- **Outputs:** `research/eval/suite/manifest.jsonl` (hashes and grouping keys only in the repository); `research/eval/suite/sealed.jsonl.sha256` (the hash of the sealed manifest, whose content stays outside the repository until release).
- **Acceptance:** no base document appears in two splits; the manifest hash and the sealed hash are in the pre-registration; the user's sign-off is recorded.
- **Effort:** 2 sessions. **Depends on:** WP-3.7–3.9, 3.14.
- **Decision points:** where the sealed manifest is kept (outside the repository, held by the user, or by an agent with no method access) is the user's call.

### WP-3.12 Pre-registration
- **Serves:** charter decision rule; CLM-1065, 1061, 1059.
- **Procedure:** write, before P5:
  1. **One primary endpoint** and its claim form. Against REPDF the claim is non-inferiority per stratum (CLM-1065): our lower 95% bound on REPDF's metric is at least REPDF's point estimate minus a stated margin. Against qpdf, MuPDF and the other baselines, superiority on the primary endpoint with Holm correction.
  2. **A fabrication ceiling** and **a plausible-but-wrong ceiling** (WP-3.2, CLM-1059).
  3. **The minimum margin** and the suite size from a power estimate on WP-3.5's per-file variance.
  4. **The auto-accept threshold** as a risk target, not a fixed constant: certifying at most 2% wrong auto-picks at 95% confidence needs about 150 error-free labelled slots (rule of three, CLM-1061). The calibrator is Platt scaling below about 1,000 slots (CLM-1060) or conformal risk control (MAPIE, TOOL-470, verified in Stage 4: OBS-1205). On our 40-row calibration set MAPIE finds no valid threshold and abstains, which is the right answer at that size.
  5. The baselines and option sets (WP-3.1), the statistics (WP-3.2) and the held-out corruptor rule (WP-3.11).
- **Outputs:** `research/preregistration.md`, committed before P5.
- **Acceptance:** the user signs off the endpoint and the margins; every number in the file has a cited source or a stated derivation.
- **Effort:** 1 session. **Depends on:** WP-3.2, 3.5, 3.11, 3.13.

### WP-3.13 Selection and abstention design
- **Serves:** RQ5; GAP-155; HYP-152; the design changes in CLM-1075 and CLM-1076.
- **Inputs:**
  - CLM-1057: every toolpath runs in seconds, so selection is generate-and-validate (run all candidates, then pick) rather than predict-then-act.
  - CLM-1060 (Platt scaling), TOOL-470 (MAPIE 1.5.0, Trial, verified in Stage 4: 14 valid thresholds and test precision 0.94 on a synthetic target; abstains on our 40 rows; OBS-1205), CLM-1061 (risk-targeted threshold).
  - WP-3.3 oracle outputs (one row per candidate), WP-3.2 fidelity labels (agreement with the known original).
  - CLM-1063: render comparators pay a 50% false-positive rate at 80% true-positive in an 11-reader study, so render similarity alone cannot label a repair correct.
- **Procedure:**
  1. For each damaged file, run every toolpath from WP-3.1 and every oracle from WP-3.3; keep all outputs (retention, CLM-1075).
  2. Label each candidate with WP-3.2's fidelity scores against the original. "Opens in a viewer" is never a label.
  3. Fit a selector on grouped splits (WP-3.11's grouping key): features are oracle results, engine agreement, blank-page checks and output length; target is the fidelity label.
  4. Calibrate its confidence (Platt; MAPIE if verified) on a held-out calibration fold, and set the auto-accept threshold from the risk target in WP-3.12.
  5. Below the threshold the selector abstains and returns an escalation reason (which oracle disagreed, which pages differ; CLM-1076).
  6. Report: single best toolpath, the per-file oracle (virtual best), the selector, and the share of the gap it closes (HYP-152's criterion), with the abstention rate and the wrong-pick rate among accepted files.
- **Outputs:** `research/eval/selection/` (candidate table, labels, fitted selector, calibration curve); an OBS per run.
- **Acceptance:**
  - the wrong-pick rate among auto-accepted files has an upper 95% bound at or below the risk target;
  - HYP-152 is confirmed or refuted on the grouped test fold, and the result is written to the hypothesis record;
  - every abstention carries a reason.
- **Effort:** 2 sessions. **Depends on:** WP-3.1, 3.2, 3.3, 3.11 (splits).
- **Decision points:** whether the product ships a learned selector or a fixed rule ladder is a board question (WP-4.2), since any learned component in the product path must be local, versioned and deterministic (charter).
- **Unknowns:** whether there are enough error-free labelled slots per class to reach the risk target (about 150, CLM-1061); if not, the threshold is set per pooled stratum and the per-class claim is dropped.

### WP-3.14 Producer profiles
- **Serves:** RQ1, RQ2; GAP-002, GAP-154; D4 (committee 004).
- **Inputs:**
  - OBS-0901: stock zlib 1.3 at level 6 reproduces 2,074 of 2,074 sampled Save As streams once memLevel is swept, but only 1 of 1,264 Print to PDF streams.
  - OBS-0905: xref offset bias is 0 for every Save As file and +1 for every Print to PDF file.
  - OBS-0907: Print to PDF embeds one anonymous TrueType font instance per page and keeps the family in the `name` table. The pure-Rust fontations crates (read-fonts and skrifa, TOOL-465) parse all 1,252 embedded programs and agree with fontTools on all 889 family rows (OBS-1204), so the product can read the same facts.
  - OBS-0908: preflate-rs reproduces Save As streams only from a profile's parameters, not from per-stream estimates (47 of 399), and fingerprints the Print to PDF encoder as a zlib-like lazy matcher with different limits.
- **Procedure:**
  1. For each producer in the corpus (Word Save As, Microsoft Print to PDF; then WP-3.9's producers), record: the replay parameters (level, memLevel, strategy, window), the xref offset bias, the EOL style, per-page font instances, the encoder fingerprint, and the first bytes of every Flate stream.
  2. Identify the producer of a file from `/Producer`, XMP and the fingerprint, and record how each was decided.
  3. Test: each profile reproduces its producer's intact streams and xref offsets byte-for-byte on a held-out sample.
  4. Test generalisation: apply each profile to a producer it was not fitted on and report what fails (GAP-002).
- **Outputs:** `research/eval/profiles/<producer>.json`; an OBS per profile.
- **Acceptance:** the Save As profile reproduces ≥ 99% of sampled intact Save As streams; the Print to PDF profile states its replay coverage honestly (OBS-0901 suggests near zero) and still records the bias, EOL style and font facts; the cross-producer test is reported.
- **Effort:** 1–2 sessions. **Depends on:** WP-3.1.
- **Decision points:** whether the product uses producer profiles is a board question (WP-4.2, D4).
- **Unknowns:** how many producers in real-world files can be profiled at all; whether a profile fitted on REPDF's single Word version transfers to other Word versions.

---

## W4 Experiment specs

### WP-4.1 Spec template and readiness
- **Procedure:** a spec template lifted from the WP fields; apply the spec-ready rule (charter §6) to every hypothesis; list which are ready and which need research.
- **Outputs:** `research/specs/TEMPLATE.md`; the Stage 6 recommendation.

### WP-4.2 Product flags → ADRs
- **Serves:** the five contradicted design assumptions (10k risks), now with PR #16's evidence; D3, D4 and D9 from committee 004.
- **The flags and the evidence each ruling uses:**

  | Flag (current design) | Evidence | Proposed reading for the board |
  |---|---|---|
  | "Last copy wins" for duplicate objects | CLM-0517 (pdf.js keeps a later copy only if it parses), CLM-1046 (a truncated update would shadow complete revisions), CLM-1073 | "Last **well-formed** copy wins", each generation a distinct key; decided on HYP-301's result, not before |
  | Adler-32 match accepts a C9 correction | False accepts 1 of 245 (OBS-1007) and 1 of 324 (OBS-1009); a trailer-byte edit makes Adler match (OBS-1013); replay is not uniqueness (OBS-0903, 0904) | Adler-32 is necessary, not sufficient; acceptance needs WP-3.3's uniqueness count and grade-3 rules (charter rev 2.3) |
  | Refuse encrypted files | WP-1.4 matrix | Board ruling on WP-1.4's rows |
  | No expansion guard | WP-1.4 matrix; DMG-101 | Board ruling on WP-1.4's rows |
  | Exact keyword matching | DMG-250; WP-1.4 matrix | Board ruling on WP-1.4's rows |

  Committee 004 adds three questions: D3 (preflate-rs as the replay oracle, after its Trial), D4 (producer profiles in the product, WP-3.14) and D9 (fund an LLM/VLM fabrication trial).
- **PR #16's ranked design changes, mapped to the WPs that test them** (all graded "unreplicated, internal" until the WP runs):

  | Claim | Change | Tested in |
  |---|---|---|
  | CLM-1069 | C9 ladder: keep-all, checkpointed 255-value search, localizers, Adler-required acceptance, resync, C9 fixture | WP-3.3, 3.9; HYP-150, 300, 302 |
  | CLM-1070 | Decode through the document's own fonts; drop inference only from the bundled DB | WP-3.6, 3.8; OBS-0907 |
  | CLM-1071 | C6 as orphan font re-linking per (page, slot); ignore CIDFont+Fn names | WP-1.4; OBS-0907 amends the "ignore names" advice (D5) |
  | CLM-1072 | gid-to-String maps, word-level shaping, /ActualText for Arabic and Devanagari | WP-3.8; WP-3.2 unknowns (non-Latin OCR) |
  | CLM-1073 | Well-formed-last-wins and the lexer guards | WP-1.4; HYP-301 |
  | CLM-1074 | Trailer candidates, Root validation, offset tolerance, page-tree checks | WP-1.4; OBS-0905 |
  | CLM-1075 | Generate-and-validate with retention and gates; plausible-but-wrong KPI | WP-3.13, 3.2 |
  | CLM-1076 | Calibrated posterior, risk-targeted threshold, escalation reasons | WP-3.13, 3.12 |
  | CLM-1077 | REPDF-comparable recall, decomposed metrics, grouped splits, held-out corruptors, oracle battery | WP-3.2, 3.5, 3.11, 3.3 |
  | CLM-1078 | Salvage truncated fonts and images; route truncated Flate to Prefix for C10 | WP-3.7; WP-2.3 (CLM-1033's ceiling, once a census is committed) |
  | CLM-1079 | New damage classes, truncation pairs, Stressful corpus, revision-history findings | WP-3.9, 3.7, 3.10 |
- **Procedure:** the board rules on each flag with WP-1.4's matrix and the rows above as evidence; only board-backed changes become `docs/adr/NNNN-*.md`. A flag whose deciding WP has not run stays open.

### WP-4.3 Wayfinder map
- **Procedure:** in a later session, turn ready specs into GitHub issues per `docs/agents/issue-tracker.md`. No issues are created in this planning session.

---

## Order
```
Stage 2 ledger ─▶ WP-1.1 ─▶ WP-1.3, 1.5 ──────────────────────────────────────▶ P4 review
             └─▶ WP-3.1 ─▶ WP-3.2 ─▶ WP-3.3 ─▶ WP-3.5, 3.6 (gates) ──────────────┐
                 WP-3.1 ─▶ WP-3.14 ─▶ WP-3.3 (replay), WP-3.9                     │
                 WP-3.4                                                           ├─▶ WP-3.11 ─▶ WP-3.13 ─▶ WP-3.12 ─▶ P5
                 WP-3.7, 3.8 ─▶ WP-3.9 ──────────────────────────────────────────┘
                 WP-3.10, WP-1.4 (needs 3.1)
WP-2.1 ─▶ WP-2.3 (needs 3.2) ; WP-2.2 (needs 3.7)
WP-1.4, 2.3, 3.13, 3.14 ─▶ WP-4.2 (board) ─▶ WP-4.1 ─▶ WP-4.3
```

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 0 | 2026-09-27 | Skeleton from the P1 evidence; tool slots open | Stage 1.5 merge and committee 002 |
| 1 | 2026-10-05 | Filled the tool slots (WP-3.1, 3.2, 3.3, 3.6) from the Stage 2 ledgers; folded in PR #16: WP-1.4 rule rows, WP-2.3 hypothesis table, WP-3.3 rewritten around uniqueness and false accepts, WP-3.5 attribution run and non-inferiority, WP-3.9 C9 model and EOL fixture, WP-3.11 grouped splits and held-out corruptors, WP-3.12 risk-targeted threshold; added WP-3.13 (selection and abstention) and WP-3.14 (producer profiles); WP-4.2 maps PR #16's ranked changes to WPs | Stage 2 tooling (agents A–D), the PR #16 ingest (agent E) and committee 004 |
| 2 | 2026-10-05 | Stage 4 verifier results: WP-3.1 pins MuPDF 1.28.0 (1.28.5 regresses on C1) and marks the other pins verified; WP-1.1 and 1.3 record which discovery services need keys now (CORE does); WP-1.2 records that GROBID could not run; WP-3.2's Trial metrics, WP-3.6's Step 1, WP-3.12/3.13's MAPIE and WP-3.14's fontations are verified; WP-3.7 switches to byte-verified pairs now that paced Common Crawl reads work; WP-3.9 notes LibreOffice needs reinstalling | Stage 4 verifier (OBS-1200..1215) and committee 005 |
