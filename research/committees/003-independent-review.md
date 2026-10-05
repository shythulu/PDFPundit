---
committee: 003
topic: Independent review of the research program (PR #17) before merge: tooling, observations, statistics, claims, methodology, consistency with main
date: 2026-10-05
charge: Do the knowledge base, its tools and its plans hold up when someone who did not build them re-runs the checks from a fresh clone? Fix what is verifiably wrong; change no research decision.
members:
  - seat: independent reviewer (fresh clone, isolated worktree; saw the PR, the repository and the REPDF corpus, not the authors' session or cache)
inputs: PR #17 at 6db5b0b (23 commits after the merge of main); the REPDF corpus at commit e547d4d; the REPDF paper PDF in nimbalyst-local/plans/; the PR #16 report on main
paid_searches: 0 (the review used plain fetches of recorded URLs only)
outcome: 7 fixes committed (validator, number rule, three claim paraphrases, two under-quoted claims, one observation statement, README/charter/10k wording); verdict merge after fixes; residual risks listed below
---

# Committee 003: independent review of PR #17

Data files: `research/audits/003-independent-review/` (claim sample, verdicts, report with commands).

## What was reproduced

| Check | Result |
|---|---|
| Validator on a fresh clone (`kb_validate.py`, offline) | **33 errors**, not 0: every `supported` gap lost its evidence because the gitignored cache is absent and the validator counted no quote as verified. Fixed (below). `test_kb.py` failed on the same count. |
| OBS-0002, 0005, 0300, 0301, 0302, 0303 (C9) | Scripts re-run on the local corpus (C9 and C10 files): every CSV and summary JSON byte-identical to the committed results. An independent diff script written from scratch gives the same figures: 100 files, 12–30 changed bytes per file (median 21, mean 21.62, total 2162), 11–28 stream bodies hit (1747 in all), top replacement byte 0.7% of changes. |
| OBS-0001, 0003, 0004 (C1–C8) | The local clone holds only C9/C10, so the summary was recomputed from the committed per-file CSV (1,000 rows): identical to the committed summary. 65 files (8 per class C1–C8 plus the C6 outlier) were fetched from GitHub at e547d4d and re-measured with the PR's script: all 65 rows identical to the committed rows. C7/C8: all same size, all changed bytes 0x20, first change inside a `/Length1` font stream. C6 outlier: 436,319 → 33,754 bytes, 9-byte common prefix. |
| Screening audit statistics | Recomputed from `key.jsonl` and `rescreen.jsonl` independently of `screening_audit.py`: 3-way agreement 24/31 = 77.4%, kappa 0.169, PABAK 0.661; include-vs-not 26/31 = 83.9%, kappa 0.383, PABAK 0.677. Confusion as reported. The duplicate row (SRC-0132) has identical decisions on both sides, so it does not change the result. Sample size is the 20% stratified draw (8/8/8/7/1). |
| Screening tallies and counts | 159 screening records: 138 include, 3 maybe, 18 exclude (per agent as in committee 002). Live counts now: 163 sources, 280 claims (279 before this review; 9 critiques), 51 gaps, 23 damage classes, 17 observations, 3 hypotheses, 54 tooling verdicts, 22 of 80 paid calls. The 150/271/13 figures in committee 002 and the PR description were correct at the sweep merge and are now stale; the README was staler still (2 sources, 27 claims) and is rewritten. |
| Hidden recall set (13 of 16) | **Not verifiable from the repository.** The gold list was never committed and no commitment (hash) of it was committed before the sweep, so neither its content nor its timing can be checked. Procedure as described is sound; see residual risks. |
| Code review | All Python under `research/tools/` and `research/experiments/` read. Findings: the three validator defects below; `check_ids_and_refs` crashed with KeyError on an unknown id prefix instead of reporting it; everything else (kappa, stratified sampler, merge folding, polite_get back-off, experiment scripts) correct as far as static reading and the re-runs above show. Views regenerate without diff. No secrets, no personal e-mail addresses (only `research@pdfpundit.invalid`), no copyleft code beyond ≤91-character quotes (clean-room rule respected; the copyleft notes files contain no code blocks). |

## Claim verification

**Automated.** Every cached-text source was re-fetched from its recorded URL (or read from the repository, or at the pinned commit for code) and the validator's own matcher run on an independent extraction (PyMuPDF instead of pdftotext):
- 19 of 20 re-fetched documents are byte-identical to the recorded `fulltext_sha256`; the REPDF paper in the repository matches too. SRC-0127's server now serves a different PDF build (all 8 quotes still match); SRC-0313 is a dynamic API response (quote still matches); SRC-0132's host answers HTTP 500 (8 claims not re-checked).
- 265 quoted non-critique claims: **254 re-verified** (241 exact, 10 fuzzy, 2 exact→fuzzy from extraction differences, 1 table quote whose numbers are all on the cited page but in a different extraction order); 8 unreachable (SRC-0132); 3 uncached tooling claims that broke the number rule (fixed).
- All **48 code citations** found in the cited lines (±5) of the files at the pinned commits (qpdf, MuPDF, pdf.js, GhostPDL from GitHub; PDFium from googlesource; Poppler from a local checkout at c8dc23d plus gitlab).

**Manual sample.** 33 claims, seed 20261005, stratified 6 per community (forensics, LangSec, compression, fonts), 6 engine-study, 3 chair: **32 faithful, 1 overstated relative to its quote** (CLM-0137 added "independent of the xref", which the paper says elsewhere but the quote does not; fixed by dropping the clause). Directed cross-check of the REPDF statements against the paper found two more: CLM-0016's quote covered neither "Google Document AI" nor "exact word matches" (both true of the paper, §5.3); CLM-0026 read "a random byte within a zlib-compressed stream" as "a single byte in one stream", which the sentence does not say.

## Fixes made (all on the PR branch)
| # | File | Problem | Fix |
|---|---|---|---|
| F1 | `tools/kb_validate.py` | On any machine without the gitignored cache every quote computed `no-fulltext`, so no claim counted as evidence (33 gap errors) and `--write` would have erased all 262 recorded `exact`/`fuzzy` values. | A recorded exact/fuzzy survives a missing cache (one summary warning says how many), and `--write` keeps it. Tests added. |
| F2 | `tools/kb_validate.py`, `tools/kbcommon.py` | The "every number in the paraphrase appears in the quote" rule ran only after a quote was found in the cache, so it never fired for uncached sources; and the regex skipped the second number of a range ("12-30" checked only 12). | Number rule checked before the cache lookup; ranges yield both endpoints (identifiers such as Adler-32 still skipped). Zero existing verified claims change status; 3 uncached claims became `partial`. Tests added. |
| F3 | `sources/claims.jsonl` CLM-0800, 0801, 0805 | Paraphrases carried numbers (404, ~40) absent from their quotes. | Numbers removed from the paraphrases. |
| F4 | CLM-0016, new CLM-0028, GAP-008 | Quote did not cover two of the three facts in the paraphrase. | Quote extended to the sentence naming Google Cloud Document AI; the exact-word-match metric moved to CLM-0028 with its own §5.3.1 quote; both cited by GAP-008. Their `quote_check` is left null: this reviewer verified both on page 6 with the tool's matcher, but only `--write` on a machine with the cache may set the field. |
| F5 | CLM-0026, OBS-0002 | "The paper states ... a single random byte inside one zlib stream" and "contradicting the paper" overstate a sentence that is silent on the number of streams per file. | Paraphrase and observation statement now say what the sentence says and what it leaves open. GAP-013 stands on the changes outside stream bodies (OBS-0005/0302) and the C6/C7/C8 findings. |
| F6 | CLM-0137 | "independent of the xref" not in the cited quote. | Clause dropped (GAP-051 unaffected). |
| F7 | `README.md`, `charter.md` §6, 10k plan header | README status two stages stale; "hashed inputs" overstated (external corpora are pinned by commit, several inputs by neither); 10k said the 5k plan was "not written yet". | Counts and stage rewritten; wording qualified; validator now warns on unpinned inputs (4 today). |

Also: `check_ids_and_refs` reports an unknown id prefix instead of crashing.

## Methodology review (no changes)
- **Pre-registered decision rule, held-out suite, null-repair baseline, clean-room rule, provenance grades, fabrication and omission rates:** present, mutually consistent across charter rev 2.2, 10k rev 4 and the 5k skeleton, and not circular: tuning data (REPDF, natural pairs) and the frozen suite are kept disjoint, the suite author is walled off, and REPDF is replication-only. The grade-2/3 tightening (rev 2.1) follows from OBS-0300 and GAP-052 as stated.
- **Unfalsifiable or leaky rules found:** none. Two soft spots: the hidden recall set has no pre-commitment (below), and the engine-study `still_open` fields are all `unknown` (the agent says so).
- **REPDF statements vs the paper:** OCR ground truth is Google Cloud Document AI (§5.3, p. 6) and matching is exact word-by-word (§5.3.1): now quoted. C9 wording: see F5. 90.67%, pikepdf/fontTools, three commercial baselines, Chrome as the only viewer (dataset README): all as quoted.
- **Consistency with main (PR #16 report):** no contradiction. Differences are definitional and both sides are right: PR #16 counts 1,445 changed bytes inside Flate data and 7–26 damaged Flate streams per file (mean 14.45); PR #17 counts 1,443 (3 bytes in a Flate stream that does not inflate cleanly are classed "other") and 11–28 stream bodies of any filter. 84.4% vs 84% Adler-only agree. PR #16's single-bit-flip share (3.1%) and MS-Print-to-PDF outline-glyph finding have no counterpart in PR #17 and are not contradicted by it. The branch has begun ingesting PR #16 (source type `report`).

## Residual risks (for the owner and the review board)
1. **Evidence re-verification depends on refetching.** The cache is gitignored, so a fresh clone cannot re-check quotes; it trusts the recorded statuses (F1). This review did refetch and found them honest, but a later edit to a quote on a cache-less machine is caught only when someone with the cache runs `--write`. Mitigation options: commit `text_sha256`-keyed sidecars of quote spans, or run the validator in CI with a rebuilt cache.
2. **Hidden recall set.** 13/16 cannot be audited. Committing `sha256(gold.json)` now, and the file itself after P4, would make the P4 reuse checkable.
3. **SRC-0132** (khai.edu) is unreachable; its 8 claims rest on the recorded check and the 2026-09-27 fetch.
4. **Committee 002's "0 errors, 0 warnings"** was true on the authors' machine with the cache; on a fresh clone the validator now reports 0 errors and 10 warnings (3 uncached tooling quotes, 2 claims awaiting `--write`, 1 summary line, 4 unpinned observation inputs).
5. Counts in `README.md` drift at every merge; the validator prints live counts, which the README now points to.
6. `research/audits/003-independent-review/` takes the number the 5k plan (WP-1.1) had pencilled in for the P4 calibration audit; renumber the calibration audit when it is created.

## Data and commands (`research/audits/003-independent-review/`)
- `claim-sample.jsonl`: the 33 claims, seed **20261005**, items sorted by id before `random.Random(seed).sample`, strata and sizes: scoping-forensics 6, scoping-langsec 6, scoping-compression 6, scoping-fonts 6, engine-study 6, chair 3.
- `claim-verdicts.jsonl`: one verdict per claim (`faithful` 32, `overstated` 1). `raw_checked` is true for all 33: each source was re-fetched from its recorded URL (or read from the repository, or taken from the file at the pinned commit) and the quote located with `kb_validate.quote_status` on an independent extraction (PyMuPDF 1.28.2; the cache had used pdftotext 24.02.0).
- Stored → recomputed `quote_check` over all 265 quoted non-critique claims: exact→exact 241; fuzzy→fuzzy 10; exact→fuzzy 2 (CLM-0009, CLM-0337, extraction differences); exact→not-found 1 (CLM-0261, a table row whose 12 numbers are all on the cited page in another column order); exact→no-fulltext 8 (SRC-0132, HTTP 500); no-fulltext→partial 3 (fixed).
- Local corpus: a blob:none partial clone of dfrc-korea/REPDF at e547d4d with the 100 originals and the 200 C9/C10 files; the 65 C1–C8 files were fetched by raw URL at the same commit (seed 20261005, 3 text + 1 text+img per creation method per class, plus the OBS-0004 file).
- Commands, fresh clone, `PYTHONPATH` holding jsonschema, rapidfuzz and pymupdf:
  - `python research/tools/kb_validate.py` — before the fixes 33 errors, 265 warnings; after 0 errors, 10 warnings.
  - `python research/tools/kb_validate.py --write` — after the fixes changes no record beyond the edits.
  - `python research/tools/test_kb.py` — before 1 of 8 failed (`test_clean_main_registries`); after 11 of 11 pass.
  - `python research/tools/render_views.py` — regenerates only `gaps/register.md` and `observations/index.md`, where records changed.
  - `python research/tools/screening_audit.py score --audit research/audits/002-screening` — same figures as the report.
  - `--online` was not run; no Crossref budget was spent.

## What this committee did not check
- The 5k plan's tool choices and effort estimates (Stage 4 and the review board).
- Experiments that need tools or hosts not available here: OBS-0500/0501 (qpdf, mutool, gs, pdftotext), OBS-0200/0201/0800 (digitalcorpora, Common Crawl), OBS-0600 (Crossref/OpenCitations), OBS-0601, OBS-0801. Their scripts were read, not run.
- Whether a quote *supports* a paraphrase was judged on the 33 sampled claims only; the other 232 were machine-matched.
- Paywalled or blocked sources, and the 290-odd titles the sweep agents rejected on sight without logging.
