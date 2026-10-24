# 0023. The research phase is closed; the knowledge base becomes an archive

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** owner request, 2026-10-23. Replaces research Stages 5 and 6 (review board, cold-start test, research-vs-specs recommendation).

## Decision

1. The research program stops at Stage 4. The Stage 5 board and Stage 6 recommendation will not run.
2. These ADRs are the ruling on what the board would have decided for v1. Items it would have ruled on, and how each was settled:

| Board item | Settled by |
|---|---|
| lopdf as sole loader and emitter | [0007](0007-carver-loads-lopdf-emits.md): loader half settled, emitter half open |
| Replay oracle (D3) | [0010](0010-replay-oracle-stock-zlib.md): stock zlib, under a C exception |
| Adler-32 sufficiency, no first-accept | [0009](0009-c9-salvage-certainty.md) |
| Last copy wins | [0008](0008-last-well-formed-copy-wins.md) |
| Refuse encrypted files | [0001](0001-v1-scope.md) |
| OCR apparatus | [0019](0019-metrics.md): per-script Tesseract, no Document AI |
| Learned vs fixed selector | [0011](0011-repair-selection.md): fixed in v1 |
| Producer profiles (D4), LLM/VLM trial (D9), certification target | Deferred. Backlog items below. |

3. `research/` is a read-only evidence archive. Cite its IDs (OBS-, CLM-, GAP-, TOOL-) from ADRs and tickets. Do not add new records to it.
4. New research is one GitHub issue per question, labelled `wayfinder:research`. Its answer lands as an ADR or a ticket.
5. Research backlog, highest value first:
   1. Fabrication metric for dictionary-ranked font picks (GAP-201).
   2. hayro-syntax text quality and lopdf writer alternatives.
   3. Held-out evaluation set grouped by document and font family (WP-3.11).
   4. Pure-Rust OCR route measurement.
   5. Proposed damage classes beyond C1 to C10.
   6. A replay route for Print to PDF streams ([0010](0010-replay-oracle-stock-zlib.md)).

## Why

- Every research item that bears on v1 is either settled by measurement or settled by these ADRs.
- The remaining stages were review process. They would not change v1's build.
- A growing knowledge base beside a growing codebase gives agents two sources of truth.

## Consequences

- PR #19's research work (committees 004 and 005, charter rev 2.3, 5k rev 3, 10k rev 6) should merge as the final archive state. It is referenced throughout these ADRs.
- The research README's status section should say "closed" and point here.
