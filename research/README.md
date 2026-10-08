# Research knowledge base: bleeding-edge PDF repair

**Start here.** This directory holds everything we know, and how we know it, about moving
PDFPundit past the state of the art in PDF repair. It is built for agents picking up the work
cold, as well as for humans.

## Status
- **Stage:** Stage 4 re-validation is done; Stage 5 (the review board) is next. The access items below are a standing checklist, not a gate: nothing in Stages 5–6 waits on them.
  - P0 (foundations) is done.
  - The Charter Committee (001) is done: all three members approved with changes, 12 blocking issues were resolved, and one partial rejection was escalated to and decided by the user.
  - The P1 scoping sweep (committee 002) is done: five agents, a blind screening audit (`audits/002-screening/`), 10k rev 4 and charter rev 2.2.
  - An independent review of the whole knowledge base (committee 003, `audits/003-independent-review/`) reproduced the REPDF observations and re-verified the claims; its fixes are in.
  - Stage 2 tooling is done: four ledgers (literature pipeline, engines/oracles/metrics, damage and corpora, cross-field wildcard). PR #16, a parallel study on main, was ingested and its committed scripts reproduced (agent E). Committee 004 (`committees/004-stage2-tooling.md`) records every decision.
  - Stage 3 is done: 5k rev 1 fills the tool slots and adds WP-3.13 (selection and abstention) and WP-3.14 (producer profiles); 10k rev 5; charter rev 2.3.
  - Stage 4 is done (committee 005, `committees/005-stage4-revalidation.md`). The hands-on verifier re-ran the Adopt and Trial picks after the container restart (OBS-1200..1215; 5k rev 2). The contrarian scout overturned or narrowed seven choices (OBS-1100..1104; 5k rev 3, 10k rev 6): a second pure-Rust replay oracle, per-file replay calibration, per-script OCR, cluster-aware certification, font leakage across splits, truncation strata, and a board flag on lopdf as sole loader.
- **Program plan:** `nimbalyst-local/plans/bleeding-edge-repair-10k.md` (rev 6); work packages: `nimbalyst-local/plans/bleeding-edge-repair-5k.md` (rev 3)
- **Charter:** `charter.md` (rev 2.3)
- **Evidence so far** (live counts: `python research/tools/kb_validate.py` prints them):
  - 178 sources (167 screening decisions: 146 include, 3 maybe, 18 exclude) and 383 claims, of which 366 are verified quotes or code citations (9 are critiques, which never count);
  - 72 reproducible observations (`observations/index.md`), including the REPDF damage characterization, the reproduction of PR #16's C9 numbers and the Stage 4 re-runs;
  - 63 gaps, 23 proposed damage classes, 6 hypotheses, 138 tooling verdicts (31 Adopt, 33 Trial, 44 Assess, 30 Hold).
- **Paid search used:** 30 of 80.
- **Next:** Stage 5, a review board of six on the 5k plan, with replication seats (the DEFLATE reviewer runs both replay oracles on all 1,444 damaged-stream counterparts; the evaluation scientist rules on the certification target against the document count). The board also reads PR #18's outlined-text and OCR note, merged from main on 2026-10-08 (`nimbalyst-local/plans/research/research_notes/Corrupted PDF repair beyond REPDF/outlined_text_and_ocr.md`). Then the cold-start test and the research-vs-specs recommendation.

### Standing access checklist (for the user, whenever convenient)
These feed the **P4 full literature review** and some P1/P5 work packages, not Stages 5–6. Work goes on without them; each one only makes a later step faster or fuller. Sources: `staging/verifier/access.md` and committee 005's escalations.

**Keys.** Add each as a variable in the cloud environment's settings (environment menu in the session title bar → Edit). A new session picks them up. Never paste a key into chat.

| Variable | Priority | Why |
|---|---|---|
| `CORE_API_KEY` | Recommended | CORE now refuses keyless calls (HTTP 429, OBS-1208). It is P4's main open-access full-text source (WP-1.1, TOOL-306). |
| `OPENALEX_API_KEY` | Recommended | Keyless calls work, but this container's shared IP spends the daily budget. It feeds discovery, forward citations and citation counts (WP-1.1, WP-1.3). |
| `S2_API_KEY` | Optional | Semantic Scholar works keyless today; a key gives a dedicated quota and bulk abstracts. |
| `HF_TOKEN` | Only if D9 is funded | Needed only for the LLM/VLM trial, if the board funds it. |

Not needed: AWS, Crossref, Unpaywall, OpenCitations (all keyless). Parallel search stays capped at 80 calls.

**Network hosts** (environment network settings), optional: `repdf.site` (REPDF black-box runs, the first replication route); `www.ohchr.org` or `web.archive.org` (UDHR translation pairs, WP-3.8); the Ollama model CDN (CPR model weights).

**Provisioning**, optional: a working Docker daemon (GROBID, WP-1.2) and `libreoffice-writer` (second producer, WP-3.14).

**User-only decisions** (agents never act on these):
- Paywalled papers: whether to obtain any (the P4 screen lists them).
- Contacting the PDF Association, or the REPDF corresponding author for code (replication route b).
- Google Document AI as REPDF's OCR, or per-script Tesseract as the substitute. Per-script Tesseract is the default until the user says otherwise.

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
