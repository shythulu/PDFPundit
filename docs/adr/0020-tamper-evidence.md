# 0020. Signatures and tamper evidence: report, never repair away; provenance grades wait

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-025 (owner answer (a)), D-052; research charter §2 and §3(c), FO-B1, GAP-101, GAP-104

## Decision

1. Tamper indicators are detected and reported. A repair never removes them.
2. A signed input gets a `Signed` Info finding and one fixed sentence: "the original is digitally signed; the repaired file is not". `impl/H-01` rewords "original" to "input".
3. v1 does not compute provenance grades and does not report shadow revisions or signature shadows. Losing object copies are still kept ([0008](0008-last-well-formed-copy-wins.md)), so that analysis can come later.
4. The verification rules that the charter attaches to grades 2 and 3 apply now, without the grade labels. They live in [0009](0009-c9-salvage-certainty.md).

## Why

- A repair tool that silently "fixes" tampering destroys the evidence a forensic user came for.
- The full provenance record is a research-scale design. The owner chose to keep it out of v1.
- Deferring the labels is cheap. Deferring the honesty rules is not, so they ship now.

## Conflict flagged

- Research charter §3(c) makes the provenance record a success criterion. This ADR defers it for v1 on the owner's answer to D-025. The charter is not changed. A v2 that adds provenance grades should cite both.
