---
committee: 000
topic: Orchestration plan for the bleeding-edge PDF repair research campaign
date: 2026-09-26
charge: Approve, amend or reject the plan for how this campaign itself runs (stages, committees, agents, knowledge base, budget).
members:
  - role: process architect (independent reviewer, read-only)
verdict: approve-with-changes
outcome: all 5 blocking issues accepted; plan rev 2 adopted
---

# Committee 000: review of the orchestration plan

## Protocol (applies to every committee in this campaign)
- **The charge.** Reviewers get the charge and the artifact under review. They don't see the chair's
  reasoning, and the artifact doesn't name its author.
- **Round 1.** Reviewers work in parallel and independently.
- **Each reviewer returns:**
  - a verdict: approve, approve-with-changes, or reject;
  - blocking issues, each with a fix;
  - suggestions;
  - missing items;
  - their confidence;
  - what would change their mind;
  - what they did *not* check.
- **Anti-rubber-stamp rules:**
  - An approval must still name the weakest part.
  - Red-team reviewers must raise at least 3 blocking issues or justify having none, and must sketch the strongest alternative.
- **Chair synthesis.**
  - Each item is accepted or rejected, with a reason.
  - **Any blocking objection the chair rejects goes to the user.** The chair is also the author.
  - If every reviewer names the same weakest part, that's treated as a sign of correlated views, not as reassurance.
  - The plan's change at each stage is tracked. A stage that changes nothing is a red flag.
- **Round 2** happens only for blocking objections still unresolved, with at most 2 reviewers.
- **Records.** Minutes go in `research/committees/NNN-*.md`. Only decisions that affect the product become `docs/adr/` entries.
- **Diversity.** Reviewer seats use mixed model configurations. The weakest configurations aren't used for review.

## Blocking issues and chair responses
| # | Issue (reviewer) | Chair response |
|---|---|---|
| B1 | The 5k plans would be written with no papers read beyond REPDF, so "the plan should evolve" has nothing to evolve from. | **Accepted.** Added Stage 1.5, a scoping sweep (3 agents: backward/forward citations, venues, adjacent fields; 60–100 titles screened, 10–15 read, 15–30 gaps). The 10k plan gets revised from that evidence before tooling research starts. |
| B2 | The container can't extract PDF text, and access (the REPDF corpus, API keys) hasn't been requested. | **Accepted.** Stage 0 installed pypdf, pdfminer, PyMuPDF, poppler-utils, qpdf, mupdf-tools and Ghostscript. The REPDF corpus is cloned (git proxy, commit e547d4d). API keys go to the user at the tooling gate. |
| B3 | "Beat REPDF on its corpus" can be gamed: the generator is known and C1–C5 are already about 100%. | **Accepted.** Charter §3 now requires all three of: (a) held-out realistic damage, (b) head-to-head against the strongest tools, (c) forensic fidelity including fabrication rate. The REPDF corpus becomes the replication and regression set. |
| B4 | Provenance was claimed but never checked, so the register would fill with invented quotes and DOIs. | **Accepted.** `research/tools/kb_validate.py`: DOI↔Crossref title match, and quotes must match cached full text (`quote_check` is computed by the tool, never set by hand). Unverified claims can't raise a gap above `candidate`. |
| B5 | No budget cap, and parallel agents would write to the same registries. | **Accepted.** Cap of 80 paid calls (user-approved; the reviewer had proposed 100). Each agent writes to its own staging directory with its own ID block, and `merge_staging.py` dedupes. |

## Non-blocking suggestions
- **Accepted:**
  - tooling research split by workstream need (A literature pipeline, B engines/oracles/metrics, C damage and corpora, D wildcard scout);
  - Charter Committee = PDF internals + forensics practitioner + red-team methodologist;
  - board lenses sharpened (evaluation scientist, font/OCR engineer, DEFLATE expert);
  - cold-start test as a practical task;
  - readiness rule decided in advance;
  - multi-letter ID prefixes (C1 already means a corruption class);
  - JSONL as the source of truth, with generated Markdown views;
  - a screening log with reason codes;
  - evidence grading;
  - `still_open` with `checked_at`;
  - a DMG- register;
  - a revision log in each plan;
  - full text cached but not committed (copyright);
  - CDR and parser differentials added to scope.
- **Modified:** per-record provenance records the agent's role but **no model identifier**. The repository policy forbids model identifiers in committed artifacts.
- **Rejected:** none.

## Weakest part (reviewer's words)
"It writes 5k plans and a tooling ledger without reading any paper beyond REPDF (which the
container can't currently extract), so the plan stays anchored on its author's assumptions and
the final research-vs-specs recommendation has no evidence behind it." Fixed by B1 and B2.

## Plan delta
Rev 1 → rev 2 added 2 stages (bootstrap, scoping sweep) and a validator, and rewrote the success definition. A substantial change.
