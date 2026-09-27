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
  updated: "2026-09-27T18:00:00.000Z"
  progress: 5
---
# Bleeding-edge PDF repair — 5,000 ft work packages

> **Altitude:** in the weeds. Each work package (WP) names its inputs by ID, its procedure step by
> step, its outputs and formats, its acceptance tests, its effort and its dependencies. Writing a
> coding-agent spec should mostly mean lifting a WP into a ticket.
> The broad view is in [bleeding-edge-repair-10k.md](bleeding-edge-repair-10k.md); definitions,
> success criteria and constraints are in [research/charter.md](../../research/charter.md).
> Tool picks cite `TOOL-` entries in `research/tooling/ledger.jsonl`. *[Stage 2] marks a slot the
> merged ledger fills.*

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
- **Inputs:** `research/sources/screening.jsonl`; `research/audits/002-screening/`; the hidden gold set (chair only); discovery services [Stage 2: agent A].
- **Procedure:**
  1. Write one search recipe per community (forensics and carving, LangSec, compression, fonts/OCR, engines, evaluation, CDR, ML-assisted repair): the services, the query strings, the venue filters, and the snowball seeds.
  2. The carving recipe adds [Stage 2: agent A's route] for 2004–2012 carving and fragment-reassembly work.
  3. Adjacency rule: a transferable method is *include* only when it can be written as a PDF procedure; otherwise *maybe*.
  4. Calibration round: two screeners, 20 titles with abstracts, before the sweep. Target include-vs-not kappa ≥ 0.6; report PABAK beside it. If below target, refine the rule and repeat once.
  5. Every judged title is logged, including ones rejected on sight.
- **Outputs:** `research/reviews/p4-search-plan.md`; calibration results in `research/audits/003-calibration/`.
- **Acceptance:** calibration kappa ≥ 0.6, or the shortfall is reported to the board with the reason.
- **Effort:** 1 session. **Depends on:** Stage 2 ledger; the API-account gate (keys change the recipes).
- **Decision points:** which keyed services the user provisions.
- **Unknowns:** how much publisher bot walls block abstracts (17 of 31 in audit 002 were title-only).

### WP-1.2 Full-text and structure pipeline
- **Serves:** W1 throughput; quote verification.
- **Inputs:** `research/tools/fetch_fulltext.py`; the parser [Stage 2: agent A, e.g. GROBID CRF or Docling].
- **Procedure:**
  1. Add an optional structured pass after the text cache: sections, reference list, and the limitations and future-work paragraphs.
  2. Pre-filter candidate limitation and future-work sentences for agents. The validator stays the only check: a pre-filtered sentence is a lead until its quote verifies.
  3. Pin the parser version and record it with each cached text.
- **Outputs:** structured sidecars beside the cached text (`research/cache/fulltext/<sha>.struct.json`, gitignored); a `--struct` flag on the fetcher.
- **Acceptance:** on 10 cached papers, references parse with ≥ 90% of Crossref's reference count; the limitations section is found where one exists.
- **Effort:** 1 session. **Depends on:** Stage 2 ledger.

### WP-1.3 Still-open checks
- **Serves:** the spec-ready rule (charter §6).
- **Inputs:** the 29 gaps with `still_open: unknown` (`research/gaps/gaps.jsonl`); forward-citation sources [Stage 2: agent A's coverage OBS].
- **Procedure:** for each gap's cited sources, collect citing works from the best-covering service; screen them; set `still_open` and `still_open_checked_at`, with the citing works logged.
- **Outputs:** updated gap records; a search-log entry per gap.
- **Acceptance:** every gap is `yes` or `no` with a date, or `unknown` with a stated reason (no citation data).
- **Effort:** 1 session. **Depends on:** WP-1.1 services.

### WP-1.4 Engine behaviour matrix (engine study, round 2)
- **Serves:** RQ2, RQ6, RQ7; GAP-250, 251, 252, 254, 256; board flags on §17.2, Adler-32 and encryption.
- **Inputs:** checkouts in `research/cache/code/` (qpdf, MuPDF, pdf.js, PDFium, Poppler, Ghostscript); new checkouts of PDFBox, pdfcpu, hayro and **lopdf** (the product's emitter).
- **Procedure:**
  1. One row per behaviour: duplicate-object rule, trailer choice, ObjStm carving, truncated-stream handling, Adler-32 handling, lost /ID with encryption, header loss, cycle handling (DMG-101), expansion guard.
  2. One column per engine, each cell a code citation at a pinned commit, or "not found".
  3. A differential test per row: a minimal crafted file per behaviour, run through every engine in WP-3.1. Code reading says what the engine intends; the run says what it does.
- **Outputs:** `research/engines/behaviour-matrix.md` (generated from claims); crafted files in `research/eval/crafted/` (synthetic, committed); an OBS per differential run.
- **Acceptance:** every cell is a verified code citation or an explicit "not found"; every row has a differential result.
- **Effort:** 2 sessions. **Depends on:** WP-3.1 for the runs.

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
- **Serves:** GAP-013 (settled by OBS-0001..0005; close it in the register); GAP-017 (how common incremental updates are).
- **Procedure for GAP-017:**
  1. Take the same natural-pair sample as WP-3.7.
  2. Count files with more than one `startxref`, more than one `%%EOF`, or a `/Prev` chain, with a committed script.
  3. Report by producer and year.
- **Outputs:** an OBS; GAP-017 status.
- **Acceptance:** the rate is reported with a 95% CI, and the gap between Caradoc's 2016 set and the 2021 crawl sample is explained or left open with a reason.
- **Effort:** 0.5 session. **Depends on:** WP-3.7 sample.

### WP-2.3 Hypotheses
- **Serves:** the spec-ready rule.
- **Inputs:** the top-ranked gaps; HYP-150, 151, 152 (drafts).
- **Procedure:** each hypothesis gets a primary metric from WP-3.2, a `refute_if`, the baselines from WP-3.1, the data from WP-3.7–3.10, and its gaps' `still_open` dates. The board approves or rejects each one.
- **Outputs:** `research/hypotheses/hypotheses.jsonl`.
- **Acceptance:** validator passes the spec-ready checks for every hypothesis marked `spec-ready`.
- **Effort:** 1 session. **Depends on:** WP-2.1, WP-3.2.

---

## W3 Damage and evaluation

### WP-3.1 Baseline harness
- **Serves:** charter §3(b); GAP-009, 053, 102, 256; HYP-152.
- **Inputs:** the engines and their option sets [Stage 2: agent B]; the null-repair baseline (tolerant readers opening the damaged file directly).
- **Procedure:**
  1. One adapter per tool: `repair(in_path, out_dir, config_id) -> RunRecord`.
  2. Each run is in a subprocess with a wall-clock limit (default 60 s), a memory limit (default 2 GB) and no network.
  3. Every output is extracted and rendered by ≥ 2 engines (WP-3.2).
  4. Versions come from the pinned builds [Stage 2]; the adapter records the version string the tool reports.
- **Outputs:** `research/eval/harness/`; runs as JSONL: `{input_sha256, tool, version, config_id, exit_code, signal, wall_ms, max_rss_mb, output_sha256, text_paths, render_dir, stderr_tail}`.
- **Acceptance:**
  - reruns of the same input and config give the same output hash (or the tool is marked non-deterministic);
  - a timeout, a crash and a segfault are each recorded, not fatal (OBS-0500's qpdf segfault is the test);
  - the header-loss case scores Ghostscript with PDF forced, per OBS-0501.
- **Effort:** 2 sessions. **Depends on:** Stage 2 ledger; Stage 4 verification.
- **Decision points:** the option sets per tool are frozen in the pre-registration.

### WP-3.2 Metric suite
- **Serves:** charter §3 metric suite; GAP-008, 055, 201; RQ4.
- **Inputs:** originals and outputs; libraries [Stage 2: agent B].
- **Metrics:**
  - **Text:** CER after NFC normalization and whitespace folding; flexible character accuracy (robust to reading order); word F1; an order-aware score (normalized edit distance over the word sequence).
  - **Visual:** per-page SSIM at 150 dpi, grayscale, in two renderers; a missing page scores 0; page-count match.
  - **Structural:** page count and order, fonts per page, image XObjects, annotations, outline; conformance oracle result (WP-3.3).
  - **Cross-engine consistency:** pairwise text and visual agreement between renderers of the same output.
  - **Fabrication rate:** output content that cannot be aligned to the original, by provenance grade.
  - **Omission rate:** content in the upper bound missing from the output. The upper bound is the content recoverable from the damaged file's surviving bytes: the union of every engine's extraction and a raw object and stream carve, aligned to the original.
  - **Planted-token faithfulness:** originals carry nonce tokens outside any dictionary; count nonces reproduced exactly vs "corrected" into words (GAP-201).
  - **Runtime and failure:** wall time, crash, timeout.
- **Statistics:** per class, paired per-file bootstrap (10,000 resamples) 95% CIs; Holm correction across classes; the margin is fixed in the pre-registration.
- **Outputs:** `research/eval/metrics/`; a scores table per run (Parquet or CSV).
- **Acceptance:**
  - unit tests on hand-made cases: identical file (all perfect), empty output (fabrication 0, omission 1), a shuffled page (text scores fall only in the order-aware measure);
  - the upper bound never exceeds the original's content.
- **Effort:** 3 sessions. **Depends on:** WP-3.1.
- **Unknowns:** the alignment method for fabrication on non-Latin scripts.

### WP-3.3 Verification oracles
- **Serves:** provenance grades 2 and 3; GAP-052, 106, 151, 153, 154, 253; HYP-150, 151.
- **Inputs:** [Stage 2: agents B and D] for encoder replay, conformance oracles and content grammars.
- **Procedure:**
  1. **Encoder replay:** find the zlib level, strategy and window that reproduce each intact stream in a file (OBS-0303: stock zlib reproduces 54% of Word streams). A candidate correction for a damaged stream is grade 3 only if it is the unique candidate that passes the checksum **and** re-encodes to the damaged stream everywhere but the damaged bytes.
  2. **Conformance oracle:** the tool chosen for GAP-106 runs on every output.
  3. **Content grammar:** a content-stream operator grammar check on decoded streams (operators, operand counts and types).
  4. **Font checks:** /Length1..3 and the font tables' own checksums.
- **Outputs:** `research/eval/oracles/`; a result per stream and per file.
- **Acceptance:** on REPDF C9 (SRC-0002), report how many candidates each oracle leaves per stream; the replay oracle must never accept a candidate that differs from the original.
- **Effort:** 2 sessions. **Depends on:** WP-3.1.

### WP-3.4 Provenance record
- **Serves:** charter §3(c); GAP-010, 050; spec-ready rule (the schema must exist).
- **Inputs:** [Stage 2: agent C] on DFXML, CASE/UCO and NIST CFTT.
- **Procedure:** a JSON Schema for the per-repair record: input and output hashes; tool, version, configuration; per-element provenance grade with byte ranges; tamper indicators found; learned-model versions and checksums; repair choices where engines differ (GAP-101). Map it to the chosen standard for export.
- **Outputs:** `research/schemas/provenance-record.schema.json`; an example record for one REPDF file.
- **Acceptance:** a forensics-practitioner review on the board signs it off; the example validates.
- **Effort:** 1 session.

### WP-3.5 REPDF replication (gate before P5)
- **Serves:** charter §3(b); RQ2, RQ3.
- **Inputs:** SRC-0001 (paper; CLM-0001, 0002, 0016, 0018, 0020, 0021), SRC-0002 (corpus at commit e547d4d); OBS-0001..0005.
- **Routes, in order:**
  1. black-box runs through repdf.site, if the user allows the host (it is blocked here);
  2. the authors' code, if the user asks for it;
  3. a reimplementation from the paper (Python, pikepdf and fontTools, as the paper states in CLM-0018).
- **Procedure:** reproduce the per-class, per-producer table (Save As vs Print to PDF) with the paper's metric: OCR of originals and outputs, exact word matches (CLM-0016); report our metrics beside it.
- **Acceptance:** per-class means within a tolerance fixed before the run (proposed ±5 points), or each difference explained by OBS-0001..0005 or by the OCR substitute.
- **Effort:** 3 sessions (route 3). **Depends on:** WP-3.1, 3.2.
- **Decision points:**
  - the paper's OCR is Google Document AI, a hosted paid service. Replicating with a local OCR changes the metric. The user chooses between access and a documented substitute;
  - the user's call on routes 1 and 2.

### WP-3.6 CPR baseline (gate before P5)
- **Serves:** RQ3; GAP-003, 004, 200, 201.
- **Inputs:** SRC-0123; github.com/BeenyHail/CPR (CC BY-NC 4.0); [Stage 2: agent B's feasibility check].
- **Procedure:** run CPR on the REPDF font classes (C7, C8) and on the UDHR pairs (WP-3.8), in a deterministic configuration if one exists.
- **Acceptance:** CPR's published numbers are reproduced within a stated tolerance, or the difference is explained.
- **Effort:** 1–2 sessions. **Depends on:** WP-3.1, 3.2; model weights provisioned (size and licence in the ledger).

### WP-3.7 Natural pairs: truncation
- **Serves:** charter §3(a) source 1; GAP-100, 006, 017; DMG-100.
- **Inputs:** OBS-0201 (34 of 35 pairs are exact prefixes); the CC-MAIN-2021-31 truncated and refetched sets; [Stage 2: agent C's sampler and metadata notes].
- **Procedure:**
  1. Stratify by source ZIP, truncation reason (length cap vs disconnect), size bucket and producer.
  2. For each pair, check that the truncated file is a byte prefix of the refetched file. Reject any pair that isn't.
  3. Keep URLs and hashes in the repository; files only in the cache.
- **Outputs:** `research/eval/pairs/truncation.jsonl`: `{url, truncated_sha256, full_sha256, truncated_len, full_len, reason, zip, producer}`.
- **Acceptance:** ≥ 1,000 verified pairs, ≥ 20% of them disconnect truncations (if the metadata allows), within about 10 GB.
- **Effort:** 1 session. **Unknowns:** the disconnect share in the metadata.

### WP-3.8 Natural pairs: font encoding
- **Serves:** charter §3(a) source 2; GAP-202, 201, 203; DMG-200, 201.
- **Inputs:** the UDHR PDFs (CLM-0415: 21 of 526 corrupted); the unicode.org/udhr texts; [Stage 2: agent C on the OHCHR terms and alignment].
- **Procedure:** confirm the terms; extract text from each PDF; align it to the reference; classify the failure (no ToUnicode, wrong ToUnicode, producer codes); keep only pairs whose reference matches the PDF's edition.
- **Outputs:** `research/eval/pairs/udhr.jsonl`.
- **Acceptance:** each pair is verified by a second extractor; the terms are recorded.
- **Effort:** 1 session. **Decision points:** the user, if the terms restrict research use.

### WP-3.9 Damage models and generators
- **Serves:** RQ1; charter §3(a); DMG-001..016, 050, 100, 101, 200, 201, 250, 251.
- **Inputs:** WP-3.7 truncation lengths; NAND bit-flip rates (SRC-0126, CLM-0341); fragmentation statistics (SRC-0137); [Stage 2: agent C on generators and mutators].
- **Procedure:**
  1. Fit each parameter to its source: the truncation-point distribution, bit-flip rate and burst shape, and fragment-size distribution.
  2. Write a generator spec per DMG: parameters, their fitted values, and a seed.
  3. Validate: the generated damage's statistics match the source within its CI.
- **Outputs:** `research/damage/models/`; generator specs in the DMG records.
- **Acceptance:** every DMG used in the held-out suite is `accepted`, citing a verified claim or OBS.
- **Effort:** 3 sessions.

### WP-3.10 Robustness corpora and differential taxonomy
- **Serves:** GAP-257, 102, 105; the red team's corpus-first alternative (committee 001).
- **Inputs:** OBS-0500 (2,873 engine regression files); the SafeDocs Issue Tracker corpus (CLM-0293); [Stage 2: agent C's PII procedure and subset plan].
- **Procedure:**
  1. Apply the PII procedure before any file is cached.
  2. Run every engine differentially (WP-3.1).
  3. Cluster failures by signature (engine × error × structural feature) into a taxonomy mapped to C1–C10 and DMG.
- **Outputs:** `research/eval/robustness/`; a taxonomy OBS.
- **Acceptance:** every cluster has ≥ 3 files and a mapping or a new DMG proposal.
- **Effort:** 2 sessions.

### WP-3.11 Held-out suite
- **Serves:** charter §3(a).
- **Procedure:**
  1. An agent with no access to the method work authors the suite: originals and producers disjoint from SRC-0002, damage from WP-3.9 models plus natural pairs from WP-3.7 and 3.8.
  2. Freeze by hash.
  3. The user signs off.
- **Outputs:** `research/eval/suite/manifest.jsonl` (hashes only in the repository).
- **Acceptance:** the manifest hash is in the pre-registration; the user's sign-off is recorded.
- **Effort:** 2 sessions. **Depends on:** WP-3.7–3.9.

### WP-3.12 Pre-registration
- **Serves:** charter decision rule.
- **Procedure:** one primary endpoint, a fabrication ceiling, a minimum margin, the suite size (from a power estimate on WP-3.5's variance), the baselines and option sets, the statistics.
- **Outputs:** `research/preregistration.md`, committed before P5.
- **Acceptance:** the user signs off the endpoint.
- **Effort:** 1 session. **Depends on:** WP-3.2, 3.5, 3.11.

---

## W4 Experiment specs

### WP-4.1 Spec template and readiness
- **Procedure:** a spec template lifted from the WP fields; apply the spec-ready rule (charter §6) to every hypothesis; list which are ready and which need research.
- **Outputs:** `research/specs/TEMPLATE.md`; the Stage 6 recommendation.

### WP-4.2 Product flags → ADRs
- **Serves:** the five contradicted design assumptions (10k risks): "last copy wins", Adler-32 acceptance for C9, refusing encrypted files, no expansion guard, exact keyword matching.
- **Procedure:** the board rules on each, with WP-1.4's matrix as evidence; only board-backed changes become `docs/adr/NNNN-*.md`.

### WP-4.3 Wayfinder map
- **Procedure:** in a later session, turn ready specs into GitHub issues per `docs/agents/issue-tracker.md`. No issues are created in this planning session.

---

## Order
```
Stage 2 ledger ─▶ WP-1.1 ─▶ WP-1.3, 1.5 ─────────────────────────────▶ P4 review
             └─▶ WP-3.1 ─▶ WP-3.2 ─▶ WP-3.3 ─▶ WP-3.5, 3.6 (gates) ─┐
                 WP-3.4                                             ├─▶ WP-3.11 ─▶ WP-3.12 ─▶ P5
                 WP-3.7, 3.8 ─▶ WP-3.9 ─────────────────────────────┘
                 WP-3.10, WP-1.4 (needs 3.1)
WP-2.1 ─▶ WP-2.3 (needs 3.2) ; WP-2.2 (needs 3.7)
```

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 0 | 2026-09-27 | Skeleton from the P1 evidence; tool slots open | Stage 1.5 merge and committee 002 |
