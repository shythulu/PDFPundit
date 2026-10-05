# Research knowledge base: bleeding-edge PDF repair

**Start here.** This directory holds everything we know, and how we know it, about moving
PDFPundit past the state of the art in PDF repair. It is built for agents picking up the work
cold, as well as for humans.

## Status
- **Stage:** Stage 2 tooling research, in progress. Two of the four tooling ledgers (literature pipeline, damage and corpora) are merged; the 5k plan is a rev 0 skeleton whose tool slots they fill.
  - P0 (foundations) is done.
  - The Charter Committee (001) is done: all three members approved with changes, 12 blocking issues were resolved, and one partial rejection was escalated to and decided by the user.
  - The P1 scoping sweep (committee 002) is done: five agents, a blind screening audit (`audits/002-screening/`), 10k rev 4 and charter rev 2.2.
  - An independent review of the whole knowledge base (committee 003, `audits/003-independent-review/`) reproduced the REPDF observations and re-verified the claims; its fixes are in.
- **Program plan:** `nimbalyst-local/plans/bleeding-edge-repair-10k.md` (rev 4); work packages: `nimbalyst-local/plans/bleeding-edge-repair-5k.md` (rev 0)
- **Charter:** `charter.md` (rev 2.2)
- **Evidence so far** (live counts: `python research/tools/kb_validate.py` prints them):
  - 163 sources (159 screening decisions: 138 include, 3 maybe, 18 exclude) and 279 claims, of which 262 are verified quotes or code citations (9 are critiques, which never count);
  - 17 reproducible observations, 5 of them on how the REPDF corpus is actually damaged (`observations/index.md`);
  - 51 gaps, 23 proposed damage classes, 3 draft hypotheses, 54 tooling verdicts.
- **Paid search used:** 22 of 80.
- **Next:** the remaining Stage 2 agents (engines/oracles/metrics, wildcard scout), hands-on tool verification, the batched access request, then the review board on the 5k plan.

*(This section is rewritten at the end of every stage.)*

## Map
| Path | What it is |
|---|---|
| `charter.md` | Goal, definitions, success criteria (a/b/c), scope, constraints, governance |
| `sources/registry.jsonl` → `sources/index.md` | Every paper, dataset, standard and web source (`SRC-`) |
| `sources/claims.jsonl` | Claims, limitations and future-work statements with verified quotes (`CLM-`) |
| `sources/screening.jsonl` | Include/exclude decisions with reason codes, so nobody re-screens |
| `sources/notes/SRC-*.md` | Structured reading notes |
| `gaps/gaps.jsonl` → `gaps/register.md` | Research gaps (`GAP-`), scored and prioritized |
| `damage/classes.jsonl` → `damage/classes.md` | Damage classes beyond REPDF's C1–C10 (`DMG-`) |
| `observations/observations.jsonl` → `observations/index.md` | Reproducible observations (`OBS-`): what we ran, with committed scripts in `experiments/` |
| `experiments/` | Scripts behind observations, plus their committed result files |
| `hypotheses/hypotheses.jsonl` → `hypotheses/index.md` | Testable hypotheses (`HYP-`) and whether each is ready for a spec |
| `tooling/ledger.jsonl` → `tooling/ledger.md` | Tooling verdicts (`TOOL-`): Adopt / Trial / Assess / Hold |
| `search-log.jsonl` | Every search, including paid Parallel calls (the budget) |
| `committees/` | Minutes for every reviewed decision (000 = protocol) |
| `agents/brief-template.md` | Rules every research agent follows |
| `schemas/` | JSON Schemas and controlled vocabulary |
| `tools/` | Validator, full-text and code fetchers, staging merger, view renderer, search logger, regression tests (`test_kb.py`) |
| `staging/` | Per-agent work areas before merge |
| `cache/` | **Gitignored.** Full texts, PDFs and API caches. Rebuild with `tools/fetch_fulltext.py`. |

The `.md` views marked GENERATED are rebuilt from the JSONL files by `tools/render_views.py`. Never edit them by hand.

## Conventions
- **IDs:** `SRC-0001`, `CLM-0001`, `OBS-0001`, `GAP-001`, `HYP-001`, `DMG-001`, `TOOL-001`, `SRCH-0001`. REPDF's own classes stay `C1`–`C10`.
- **Evidence:** three kinds count, all machine-checked.
  - **Verified quotes:** found in the cached full text, at least 8 words, on the right page, and every number in the paraphrase appears in the quote.
  - **Code citations:** found in the cited lines of the file at a pinned commit.
  - **Observations:** a committed script with hashed inputs (external corpora are pinned by commit; the validator flags inputs that have neither).

  Our own critiques are inference, never evidence. Anything from memory is a lead.
- **Decisions:** committee minutes live in `committees/`. Decisions that affect the product become ADRs in `docs/adr/`.
- **Issues:** hands-on experiment specs become GitHub wayfinder issues (`docs/agents/issue-tracker.md`).
  In cloud sessions, use the GitHub MCP tools; the `gh` CLI isn't installed.

## Everyday commands
```bash
pip install pypdf pdfminer.six pymupdf rapidfuzz jsonschema   # plus poppler-utils for pdftotext
python research/tools/kb_validate.py                 # must report 0 errors before commit
python research/tools/kb_validate.py --write --online # recompute quote/DOI checks
python research/tools/render_views.py                # regenerate the .md views
python research/tools/fetch_fulltext.py SRC-xxxx <url-or-path>
python research/tools/fetch_code.py SRC-xxxx <git-url> <sha|HEAD> --license <SPDX>
python research/tools/merge_staging.py research/staging/<agent>
python research/tools/test_kb.py                     # regression tests for the evidence guards
```

## External data
- **REPDF corpus (SRC-0002):**
  `GIT_LFS_SKIP_SMUDGE=1 git clone --depth 1 https://github.com/dfrc-korea/repdf /home/user/dfrc-korea/repdf`
  (1.6 GB, commit e547d4d). It is not vendored into this repo.
