# PDFPundit

A forensic PDF repair tool in pure Rust. It rebuilds damaged PDFs from their raw bytes and tells you which parts it recovered and which it had to make up.

**Wiki:** https://www.slacklab.ca/PDFPundit/

> **Status: research stage. There is no working tool yet.**
> `src/main.rs` prints "Hello, world!". The work so far is the design and an evidence base for the repair engine. See [Status](#status).

## What it will do

1. You give it a damaged PDF.
2. It carves every object that survives in the file's bytes.
3. It rebuilds the cross-reference table, trailer, page tree and fonts.
4. It writes a new file, `<name>.repaired.pdf`. The original is never changed.
5. It labels every part of the output with how it was obtained.

Later milestones add PDF to Markdown export and optional OCR.

## Why it exists

Most PDF readers repair quietly. They guess, and they don't say what they guessed. PDFPundit treats a damaged PDF as evidence, so it has to report its choices.

| Most repair tools | PDFPundit |
|---|---|
| Patch the file so it opens | Rebuild a new, standards-conforming file |
| Hide what they changed | Label every output element with a provenance grade |
| Trust a checksum match | Require a checksum and an independent check |
| Pick a revision silently | Report earlier revisions and signs of tampering |

## The damage it targets

The starting point is the REPDF method and its ten damage classes ([code and corpus](https://github.com/dfrc-korea/REPDF)).

| ID | Damage | Repair approach |
|---|---|---|
| C1 | Header overwritten | Rewrite a valid `%PDF-x.y` header |
| C2 | Cross-reference table removed | Carve all objects, rebuild the table |
| C3 | Trailer removed | Find `/Root` and `/Info`, rebuild the trailer |
| C4 | Page tree broken | Collect `/Page` objects, rebuild `/Pages` |
| C5 | Object tags stripped | Find object boundaries by structure |
| C6 | Font mapping lost | Re-link pages to their fonts |
| C7 | Font program deleted | Substitute from a bundled font database |
| C8 | Font program and `/ToUnicode` deleted | Substitute the font, rebuild the Unicode map |
| C9 | Compressed stream damaged | Salvage what decodes, verified |
| C10 | File truncated | Assemble a document from what survives |

The research has found 23 more damage classes beyond these ten.

## Provenance grades

Every element of a repaired file gets exactly one grade.

| # | Grade | Example |
|---|---|---|
| 1 | Verbatim | Bytes copied unchanged |
| 2 | Partially decoded | Stream output shown to be independent of the damage |
| 3 | Corrected and verified | The only candidate that passes a checksum and an independent check |
| 4 | Structurally inferred | Page order, renumbered objects, chosen revision |
| 5 | Non-content synthesis | New xref, trailer, `/Length` |
| 6 | Content synthesis | Substituted fonts, generated `/ToUnicode` |

## Status

Snapshot from 2026-10-19. `python research/tools/kb_validate.py` prints live counts.

| Area | State |
|---|---|
| Rust engine | Not started. Crate scaffold only. |
| Design | Technical design v1 drafted. UI choices still open. |
| Research | Stage 2 of the plan: choosing tools |
| Evidence base | 163 sources, 280 claims, 17 reproducible observations |
| Open questions | 51 research gaps, 3 draft hypotheses |

The next step is the [wayfinder map](https://github.com/shythulu/PDFPundit/issues/1): engine to first verified repair.

## Repository layout

```
src/                 Rust crate (pdfpundit). Scaffold only.
research/            Evidence base: sources, claims, observations, gaps, hypotheses
  README.md          Start here for research work
  charter.md         Goal, definitions, success criteria, rules
  tools/             Validator, fetchers, view renderer, tests
  experiments/       Scripts behind every observation
nimbalyst-local/     Plans and mockups
  plans/             Product, technical and research plans
docs/agents/         How agents use the issue tracker and labels
site/                The wiki, published to GitHub Pages
```

## Build

The crate needs Rust 1.85 or newer, because it uses edition 2024.

```sh
cargo build
cargo run
```

## Design rules

These hold for the shipped tool. Research may argue to reopen one, with evidence.

- Pure Rust. No compiled C in the shipped binary.
- `lopdf` is the only PDF writer.
- The original file is never changed.
- Dependencies are MIT or Apache licensed.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). The [wiki](https://www.slacklab.ca/PDFPundit/contributing.html) has the same guide with more context.

## Licence

No licence has been chosen yet. Until one is, all rights are reserved by the author.
