# 0005. Clean-room rule: coding agents never read copyleft source

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-042 (owner answer (c)), D-046; research charter §5; committee 004 A3, C4

## Decision

1. The project is MIT. Code dependencies must be permissive. The allow-list in `deny.toml` has no exceptions. Bundled data is the one exception: the word lists are CC BY-NC ([0016](0016-generated-assets-committed.md)).
2. Coding agents never open copyleft or unlicensed source. Research agents may read it and quote short passages.
3. Every engine rule is restated inside its ticket and grounded on a permissive source: qpdf, pdf.js, PDFium, lopdf, hayro, ISO 32000, or our own measurement.
4. Specs describe behaviour only. They never paste code or code comments from the list below.

| Source | Licence | Allowed use |
|---|---|---|
| MuPDF, Ghostscript, PyMuPDF, PyMuPDF4LLM | AGPL | Run as a harness baseline. Never read by coding agents. |
| Poppler, ZipRec | GPL | Same |
| peepdf-3 | GPL | External command only |
| FaithC4 scorer | AGPL/GPL dependencies | Do not adopt |
| CPR | CC BY-NC | Run only, never copy |
| Shadow-attack artefacts | No licence | Cite only |
| MinerU | AGPL before v4.0.7 | Only v4.0.7 or later |

## Why

- Copyleft code read by the agent that writes our code risks a derived work, which would break the MIT licence.
- Restating each rule from a permissive source keeps the reasoning auditable.

## Outstanding

- The owner chose to scrub copyleft links and verbatim quotes from the research notes, keeping the facts (D-042 (c)). **This is not done.** `nimbalyst-local/plans/research/research_notes/.../tool_repair_heuristics.md` on main still has 31 line-anchored MuPDF/Poppler links. Until it is scrubbed, coding agents must not open that file or the `research/` claims with copyleft anchors.

## Rules for agents

- If a ticket's only grounding is MuPDF or Poppler behaviour, stop and ask for a permissive source.
