---
title: Research charter — bleeding-edge PDF repair
status: draft (rev 1, pending Charter Committee)
owner: shythulu
updated: 2026-09-26
---

# Research charter: bleeding-edge PDF repair

## 1. Goal
Advance PDFPundit from *a Rust reimplementation of REPDF* (SRC-0001) to **the most capable and
most trustworthy PDF repair engine we can evidence**. We get there by reading the literature
systematically, turning what it leaves open into testable hypotheses, and handing those
hypotheses to coding agents as experiment specs.

Writing or publishing articles is **out of scope**. The outputs are knowledge-base records,
plans, specs and, later, code.

## 2. Definitions
| Term | Meaning here |
|---|---|
| **Repair** | Producing a new, standards-conforming PDF from a damaged one. The original is never mutated. |
| **Recovery** | Getting content out (text, images, structure) whether or not a valid PDF results. |
| **Recovered vs synthesized** | Recovered content is derived from bytes in the damaged file. Synthesized content is inserted from outside: substituted fonts, rebuilt xref, default MediaBox, inferred placement. |
| **Fabrication** | Output content that was not in the original document. Examples: wrong glyph→Unicode mappings, wrong page order, text from the wrong font inference. |
| **Bleeding edge** | Satisfies §3 (a), (b) and (c) together. It is not a single benchmark number. |
| **Damage class** | REPDF's C1–C10, extended by `DMG-` records (`research/damage/`). |

## 3. Success criteria (all three must hold)
**(a) Held-out, realistic damage.**
- Performance is measured on a damage suite that is specified independently of our repair methods,
  from real-world evidence (GAP-001, GAP-002).
- The suite is frozen before tuning and kept held out from development.
- It includes compound faults and multiple producers.
- The REPDF corpus (SRC-0002) is the **replication and regression** set, not the finish line.
  Its generator is known (C9 flips exactly one byte; C10 keeps 70%), so it can be gamed.

**(b) Head-to-head against the strongest existing tools.** At minimum: qpdf/pikepdf, MuPDF (mutool),
Ghostscript, Poppler, pdf.js, PDFium, and REPDF's reported numbers (GAP-009). The same inputs,
metrics and time limits apply to every tool.

**(c) Forensic fidelity.** Every repair reports which content is recovered and which is
synthesized. The metric suite measures **fabrication rate** alongside recovery (GAP-008, GAP-010).
A repair that recovers more but fabricates more doesn't count as better.

**Metric suite** (to be specified in W3):
- **Extractable-text accuracy**: word-level F1 and order-aware similarity of *extracted* text against the original's extracted text. This is separate from OCR-visible text.
- **Visual fidelity**: per-page render similarity in at least 2 engines.
- **Structural fidelity**: pages, order, annotations, outlines, forms, metadata.
- **Cross-engine consistency**: parser-differential agreement (GAP-012).
- **Fabrication rate**, **runtime** and **failure rate**.

## 4. Scope
**In scope:**
- Structural, stream, font and text recovery.
- Truncated, fragmented, carved and partially overwritten files.
- Incremental updates and prior revisions.
- Robust and differential parsing.
- Content disarm and reconstruction (CDR) as a neighbouring discipline.
- Methods transferable from other formats (ZIP, JPEG, Office).
- ML, LLM and VLM assistance with calibrated confidence.
- Choosing a repair strategy (toolpath matrix).
- Evaluation infrastructure.

**Out of scope:**
- Publishing.
- Password cracking. Encrypted files count as repairable only when the key is available.
- Building malware.
- Non-PDF formats, except as a source of transferable methods.
- UI and aesthetics (see `nimbalyst-local/plans/pdfpundit-ui-design.md`).

## 5. Constraints
- **Product decisions** (currently in force, from `nimbalyst-local/plans/pdfpundit-*.md`):
  - pure Rust, no compiled or vendored C in the shipped binary;
  - `lopdf` is the only emitter;
  - originals are never mutated;
  - the REPDF C1–C10 taxonomy.
  - Research may recommend reopening any of these. That recommendation must state the conflict and
    the evidence (`docs/agents/domain.md`: "flag ADR conflicts"). It is never silently overridden.
- **The research harness is unconstrained.** Python, Java, C tools and web services are all allowed
  as oracles, baselines or generators.
- **Legal and ethical limits:**
  - cache only open-access full text, and never commit it;
  - respect corpus licenses;
  - no scraping that breaches terms of service.
- **Budget:** Parallel Search is capped at 80 paid calls for the planning campaign (user-approved),
  and every call is logged in `search-log.jsonl`. Free APIs come first. New paid or keyed services
  need the user's approval: the user creates accounts on request.

## 6. Governance
- Major decisions go through a committee. The protocol is in
  `research/committees/000-orchestration-plan-review.md`.
  - Reviewers work independently, then the chair synthesizes.
  - The chair answers every blocking issue with a reason.
  - **Any blocking objection the chair rejects is escalated to the user.**
- Decisions that affect the product become ADRs in `docs/adr/`. Process decisions stay in committee minutes.
- A hypothesis is **spec-ready** when:
  - its metric is measurable;
  - its data can be reached from the container;
  - its baselines have been verified in the container;
  - it cites at least one gap with a verified quote.
  - Only spec-ready hypotheses become coding-agent specs (GitHub "wayfinder" issues, `docs/agents/issue-tracker.md`).

## 7. Initial evidence
- REPDF (SRC-0001) and its dataset (SRC-0002).
- 25 claims (21 verified quotes), 12 gaps and 8 proposed damage classes. See `research/gaps/register.md` and `research/damage/classes.md`.
