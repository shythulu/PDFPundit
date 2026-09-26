# Research knowledge base: bleeding-edge PDF repair

**Start here.** This directory holds everything we know, and how we know it, about moving
PDFPundit past the state of the art in PDF repair. It is built for agents picking up the work
cold, as well as for humans.

## Status
- **Stage:** P0 (foundations), with the Charter Committee in progress.
- **Program plan:** `nimbalyst-local/plans/bleeding-edge-repair-10k.md`
- **Charter:** `charter.md`

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
| `hypotheses/hypotheses.jsonl` → `hypotheses/index.md` | Testable hypotheses (`HYP-`) and whether each is ready for a spec |
| `tooling/ledger.jsonl` → `tooling/ledger.md` | Tooling verdicts (`TOOL-`): Adopt / Trial / Assess / Hold |
| `search-log.jsonl` | Every search, including paid Parallel calls (the budget) |
| `committees/` | Minutes for every reviewed decision (000 = protocol) |
| `agents/brief-template.md` | Rules every research agent follows |
| `schemas/` | JSON Schemas and controlled vocabulary |
| `tools/` | Validator, full-text fetcher, staging merger, view renderer, search logger |
| `staging/` | Per-agent work areas before merge |
| `cache/` | **Gitignored.** Full texts, PDFs and API caches. Rebuild with `tools/fetch_fulltext.py`. |

The `.md` views marked GENERATED are rebuilt from the JSONL files by `tools/render_views.py`. Never edit them by hand.

## Conventions
- **IDs:** `SRC-0001`, `CLM-0001`, `GAP-001`, `HYP-001`, `DMG-001`, `TOOL-001`, `SRCH-0001`. REPDF's own classes stay `C1`–`C10`.
- **Evidence:** a quote counts only if the validator finds it in the cached full text (`quote_check: exact|fuzzy`). Anything from memory is a lead, not evidence.
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
python research/tools/merge_staging.py research/staging/<agent>
```

## External data
- **REPDF corpus (SRC-0002):**
  `GIT_LFS_SKIP_SMUDGE=1 git clone --depth 1 https://github.com/dfrc-korea/repdf /home/user/dfrc-korea/repdf`
  (1.6 GB, commit e547d4d). It is not vendored into this repo.
