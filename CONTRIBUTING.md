# Contributing to PDFPundit

PDFPundit is in its research stage. Most useful work right now is research, not Rust code. The [wiki](https://github.com/shythulu/PDFPundit/wiki/Contributing) has this guide with more background.

## Ways to help

| Kind of work | Where it goes | Start with |
|---|---|---|
| Report a bug or idea | GitHub issue | [New issue](https://github.com/shythulu/PDFPundit/issues/new) |
| Pick up a ticket | GitHub issue | The [wayfinder map](https://github.com/shythulu/PDFPundit/issues/1) |
| Add research evidence | `research/` | `research/README.md` |
| Run an experiment | `research/experiments/` | `research/observations/index.md` |
| Write engine code | `src/` | `nimbalyst-local/plans/pdfpundit-technical-design.md` |

## Before you start

The licence isn't chosen yet. Open an issue before sending code, so we can agree on terms first.

## Issues and labels

Issues live on GitHub. Each one moves through these labels.

| Label | Meaning |
|---|---|
| `needs-triage` | The maintainer hasn't looked at it yet |
| `needs-info` | Waiting on the reporter |
| `ready-for-agent` | Fully specified. A coding agent can take it alone. |
| `ready-for-human` | Needs a person to do it |
| `wontfix` | Won't be done |

Planned work hangs off one map issue, labelled `wayfinder:map`. Each child ticket has a type label: `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling` or `wayfinder:task`. A ticket is free to take when all its blockers are closed and nobody is assigned. Assign yourself before you start.

## Research contributions

The research lives in `research/`. Read these first, in order:

1. `research/README.md`. Status and a map of every file.
2. `research/charter.md`. The goal, the definitions and the rules.
3. `research/agents/brief-template.md`. The rules for adding records.

### What counts as evidence

Only three things count. The validator checks all three.

| Evidence | What it needs |
|---|---|
| Verified quote | Found in the cached full text, at least 8 words, on the right page. Every number in your summary appears in the quote. |
| Code citation | Found in the cited lines of the file, at a pinned commit |
| Observation | A committed script with pinned inputs |

Your own reasoning is a critique. Critiques are recorded but never count as evidence. Anything from memory is a lead to check, not a fact.

### Workflow

1. Work in your own folder, `research/staging/<your-name>/`. Never edit the main records directly.
2. Cache sources with `research/tools/fetch_fulltext.py` or `fetch_code.py`.
3. Run the validator. It must report 0 errors.
4. Ask the maintainer to merge your staging folder with `research/tools/merge_staging.py`.

```sh
pip install pypdf pdfminer.six pymupdf rapidfuzz jsonschema
python research/tools/kb_validate.py     # must print 0 error(s)
python research/tools/render_views.py    # rebuilds the .md views
python research/tools/test_kb.py         # tests for the validator
```

Files marked GENERATED are rebuilt from the JSONL records. Don't edit them by hand, because the next render overwrites them.

## Code contributions

The shipped tool follows four rules. A change that breaks one needs evidence and a maintainer decision first.

- Pure Rust. No compiled C in the binary, so no `-sys` crates that build C.
- `lopdf` is the only PDF writer.
- The original file is never changed. Output always goes to a new file.
- Dependencies are MIT or Apache licensed.

```sh
cargo build
cargo test
cargo clippy
```

## The clean-room rule

MuPDF and Ghostscript are AGPL. Poppler is GPL. Their code must never reach PDFPundit.

- Research may read them and quote short passages.
- Describe their behaviour in your own words.
- Specs describe behaviour only. They contain no copied code.
- Never point a coding agent at copyleft source.

## Data rules

- Use public corpora only.
- Never commit a PDF that contains personal data.
- Record the licence of every corpus you use.
- Cache full texts in `research/cache/`, which git ignores. Never commit them.
- Don't scrape in breach of a site's terms.

## Decisions

Reviewed research decisions go in `research/committees/`. Decisions that change the product become ADRs in `docs/adr/`. If your change contradicts an ADR, say so in the PR and give the evidence.

## Pull requests

1. Branch from `main`.
2. Keep one change per PR.
3. Run the validator for research changes, and `cargo test` for code changes.
4. Say which issue the PR closes.
