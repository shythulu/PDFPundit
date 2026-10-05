---
committee: 002
topic: Stage 1.5 scoping sweep: merge, independent screening audit, and the 10k rev 4 / charter rev 2.2 changes
date: 2026-09-27
charge: Is the P1 evidence sound enough to reshape the plan, and what did it change?
members:
  - seat: chair (merges, links evidence, revises the plan)
  - seat: independent screening auditor (blind re-screen of a stratified 20% sample, spot-check of 10 random verified claims against raw sources); it saw neither the original decisions nor the chair's reasoning
  - review of the plan changes themselves: deferred to the Stage 5 review board (charter §7), since the chair authored them
inputs: five sweep agents (scoping-forensics, scoping-langsec, scoping-compression, scoping-fonts, engine-study), reports in research/staging/*/report.md
paid_searches: 15 of 80 used by the end of the sweep
outcome: 10k rev 4, charter rev 2.1 and 2.2, GAP-017, DMG-009 attack templates, evidence links into older gaps and damage classes (listed below), Stage 2 needs table
---

# Committee 002: scoping sweep

## Flow (PRISMA-style counts)
| Agent | Screened | Include | Maybe | Exclude | Verified claims (exact + fuzzy) |
|---|---|---|---|---|---|
| scoping-forensics | 38 | 32 | 0 | 6 | 39 + 0 |
| scoping-langsec | 39 | 34 | 1 | 4 | 67 + 0 |
| scoping-compression | 40 | 34 | 0 | 6 | 38 + 2 |
| scoping-fonts | 35 | 31 | 2 | 2 | 40 + 5 |
| engine-study | 7 | 7 | 0 | 0 | 48 code citations |
| **Total** | **159** | **138** | **3** | **18** | **239** |

After merge: 150 sources, 271 claims, 51 gaps, 23 damage classes, 13 observations, 3 draft hypotheses.
Validator: 0 errors, 0 warnings.

**Chair's reading of the flow.** An 87% include rate is high. The agents searched narrowly and
logged mostly titles they already judged relevant, so the screening log is not the full set of
titles seen. The audit below tests whether the includes hold up; the P4 full review must log every
judged title, as the brief requires.

## Hidden recall check
Before the sweep, the chair compiled 16 known-relevant works (Crossref-verified, kept outside the
repository, never shown to any agent). Result: **13 of 16 found and included (81%)**.

| Community | Found |
|---|---|
| DEFLATE recovery | 3 / 3 |
| LangSec, parser robustness and differentials, corpora | 5 / 5 |
| fonts, evaluation, format repair, CDR | 4 / 4 |
| **classic file carving** | **1 / 4** |

The misses cluster in classic file carving (2004–2012 carving and forensic-corpus work). Stage 2
agent A was asked which venues and indexes would surface them, without being told which works they
are. P4 must add a carving-specific search route. The gold set stays hidden so it can be reused
once, on the P4 review; after that it is spent and gets committed.

## Screening audit (independent seat)
Full report: `research/audits/002-screening/report.md`.

| Check | Result |
|---|---|
| Blind re-screen, 31 works (stratified 20%) | include vs not: 84% agreement, kappa 0.38, PABAK 0.68. 3-way: 77%, kappa 0.17, PABAK 0.66. |
| Claim spot-check, 10 random verified claims | 9 faithful, 1 overstated (CLM-0138) |
| Raw-source re-extraction, 3 claims | 3 of 3 reproduce verbatim on the stated page |

**Chair's reading.**
- **No core work flipped.** Every disagreement sits at the edge:
  - 5 are *maybe* against include or exclude, on adjacent-method papers (learning to defer, audio repair, Hangul font CNNs, messaging reconstruction, compressed-file carving).
  - The 2 include→exclude cases (SRC-0108 iLovePDF, SRC-0120 Wondershare) are the commercial baselines REPDF itself used. The blind sample showed only a vendor homepage with no URL, so the auditor could not know that. The original include stands; both are baseline leads, not literature.
- **Kappa is low partly because of prevalence.** 24 of 31 are includes on both sides, which depresses kappa; PABAK is 0.66–0.68. It is still below where we want a full review to be, and 17 of the 31 re-screens were title-only because publishers blocked abstracts.
- **The claim layer holds up.** One overstated paraphrase in 10, and none of the checked quotes drifted from the raw source.

**Chair decisions.**
| # | Issue | Decision |
|---|---|---|
| A1 | CLM-0138 overstated | **Fixed:** paraphrase cut back to what the quote says. GAP-051 and DMG-050 still stand on the corrected claim, since carving by obj/endobj signatures is enough for the source-mixing risk. |
| A2 | Duplicate sample row | **Fixed:** the sampler samples each work once. |
| A3 | Adjacent-method papers split screeners | **For P4:** the 5k plan's review WP defines an adjacency rule (include a transferable method only when it can be stated as a PDF procedure; otherwise *maybe*) and runs a calibration round on 20 titles before the sweep, with a target of include-vs-not kappa ≥ 0.6, reporting PABAK beside it. |
| A4 | Abstracts blocked by publishers | **For P4 and the access gate:** abstracts through Crossref's `abstract` field and Semantic Scholar or OpenAlex, which need keys (Stage 2 agent A). |
| A5 | Blind rows with no URL or context | **For P4:** web and tool sources carry a URL; the sample tool shows the venue field, which should say "baseline tool" for such rows. |
| A6 | Auditor saw charter §6–7 | Aggregate counts only; no effect on per-work decisions. Recorded. |

## Sweep decisions and chair responses
| # | From | Decision asked for | Response |
|---|---|---|---|
| 1 | forensics | Fold 8 duplicate pairs | **Done** at merge through dedupe keys; langsec's SRC-0200, 0229 and 0231 carry `duplicate_of`. |
| 2 | forensics, compression | Fix `polite_get` crash on 429 | **Done**, commit d9de5a3, with a regression test. |
| 3 | forensics | Unpaywall in the brief | **Done**, commit cda8219. |
| 4 | compression, langsec | Move experiment scripts to `research/experiments/` | **Done** before each merge; OBS paths updated. |
| 5 | compression | Amend GAP-052 (one byte per stream) | **Done**, commit 248b495; independently re-run as OBS-0005. |
| 6 | forensics | Revise charter grade 3 | **Done**, charter rev 2.1: grade 3 needs a unique candidate plus an independent check; grade 2 no longer admits a decode-until-error prefix. |
| 7 | forensics, fonts | CPR as a runnable baseline | **Accepted.** CPR is a pre-P5 baseline in 10k rev 4; Stage 2 agent B checks CPU feasibility and its CC BY-NC terms. |
| 8 | langsec | Adopt prefix-verified Common Crawl pairs as a real-world source | **Accepted as a candidate** in charter rev 2.2 §3(a). Adoption waits for a stratified sample (Stage 2 agent C). |
| 9 | fonts | UDHR as a real-world source | **Accepted as a candidate** in charter rev 2.2 §3(a), pending the OHCHR terms (agent C). |
| 10 | langsec | DMG-009 attack templates | **Done** at merge: 7 templates with evidence. |
| 11 | langsec | Incremental-update prevalence contradiction | **Recorded** as GAP-017. It must be settled before damage rates are fitted (P4). |
| 12 | fonts | Planted-token faithfulness test in the metric suite | **Accepted in principle;** agent B looks for tooling. The metric itself is fixed in the 5k plan and pre-registration. |
| 13 | engine-study | Baseline protocol: option sets per tool, score outputs not exit codes, pin HEAD builds | **Accepted in direction;** agent B supplies the option sets. Written into the 5k plan. |
| 14 | engine-study | Reopen "last in byte order wins" (§17.2) | **To the review board.** It is a product decision; only a board-backed change becomes an ADR. |
| 15 | engine-study | Keep Adler-32 as C9 acceptance oracle? | **To the review board**, with GAP-253 (base rate of false Adler matches). |
| 16 | engine-study | Encrypted files out of scope in v1? | **To the review board.** |
| 17 | engine-study | Engine corpora as robustness-only, with a data-ethics rule for pdf.js `.link` files | **Accepted as robustness-only.** Agent C drafts the PII screening procedure; no `.link` target is cached until it exists. |
| 18 | compression | Approve HYP-150 | **To the review board.** Approving a hypothesis is not a sweep decision. |
| 19 | fonts | GAP-203 separate from GAP-004? | **Kept separate** for now; the 5k plan decides whether one work package serves both. |
| 20 | forensics, compression | Paywalled papers (Hughes 2024, Casey 2019, IEEE and Springer items) | **User's call.** Listed for the access gate. |
| 21 | langsec | Ask the PDF Association for File Observatory tables and SafeDocs labels | **User's call.** Outward-facing; listed for the access gate. |
| 22 | langsec | Scratchpad collision between agents | **Done:** the brief now gives each agent its own scratch subdirectory. Nothing was lost. |

## Evidence linked into older records
- **Forensics, fonts, engine-study merge** (commit 8cb9a38): GAP-001..005, 008, 009, 010, 015, 050, 053; DMG-006, 007. GAP-010 moved to supported.
- **Compression merge** (commit 304313f): GAP-005, 006, 010, 013, 052, 053, 150..156, 253; DMG-002, 007.
- **LangSec merge** (commit 8ca08c3): GAP-001, 002, 006, 012, 015 (to supported), 016, 100, 101, 104, 105, 250, and new GAP-017; DMG-008, 009, 011, 013, 014, 016.

## Plan delta
The plan changed substantially, which is what the P1 gate asks for:
- **10k rev 4:** trust and verification became the spine (RQ4); engine disagreement is framed as an attack surface; natural pairs make a corpus-first W3 feasible; CPR joins as a second reference; carving is flagged for P4; five contradicted design assumptions go to the board.
- **Charter rev 2.1 and 2.2:** grades 2 and 3 tightened; candidate real-world sources named.
- **Stage 2 briefing** was rewritten from the sweep: a needs table per agent, with what is already verified in this container.

## What this committee did not check
- Whether the plan changes are right. That is the review board's job (Stage 5), with the chair as author.
- Titles the agents saw but did not log (see the flow note).
- Paywalled full texts, pdfa.org material, and repdf.site (blocked here).
