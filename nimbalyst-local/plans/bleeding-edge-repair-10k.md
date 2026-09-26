---
planStatus:
  planId: plan-bleeding-edge-repair-10k
  title: Bleeding-edge PDF repair — 10,000 ft program plan
  status: draft
  planType: research-program
  priority: high
  owner: shythulu
  stakeholders: []
  tags:
    - research
    - pdf
    - forensics
    - repair
  created: "2026-09-26"
  updated: "2026-09-26T00:00:00.000Z"
  progress: 5
---
# Bleeding-edge PDF repair — 10,000 ft program plan

> Charter: [research/charter.md](../../research/charter.md) · Knowledge base: [research/README.md](../../research/README.md)
> · 5,000 ft breakdown: [bleeding-edge-repair-5k.md](bleeding-edge-repair-5k.md) (to be written)

## Why
PDFPundit's engine design reimplements a single paper, REPDF (SRC-0001). That paper is strong
on the damage it tested (C1–C5 about 100%) and weak on the rest:
- C9 (zlib) about 60%, and C10 Print-to-PDF about 35%.
- Arabic font recovery 33–40%.
- One synthetic fault per file, from one producer (Word).
- An OCR metric that can't see fabrication.
- No comparison against open-source repair engines.

"Bleeding edge" means beating the strongest tools on **realistic** damage while **proving what was
recovered and what was synthesized** (charter §3).

## Research questions
| RQ | Question | Seed gaps |
|---|---|---|
| RQ1 | What damage actually happens to PDFs in the wild, and how do we model it reproducibly? | GAP-001, GAP-002, DMG-001…008 |
| RQ2 | How far can structural and stream recovery go on realistic damage (holes, fragmentation, ObjStm/xref-stream damage, truncation, multi-error DEFLATE)? | GAP-005, GAP-006, GAP-011 |
| RQ3 | How do we recover text semantics when font or Unicode information is lost, across scripts, without depending on a fixed font DB? | GAP-003, GAP-004 |
| RQ4 | How do we measure repair forensically: extractability, fidelity, fabrication, cross-engine consistency? | GAP-008, GAP-009, GAP-010, GAP-012 |
| RQ5 | How should each file's repair strategy be chosen, with calibrated confidence and escalation to a human? | pending prior research (gentle-gosling §2) |
| RQ6 | Which methods from other fields transfer (carving, archive repair, error-correcting decoding, LLM/VLM)? | GAP-007 and cross-field gaps to come |

## Workstreams
**W1 — Evidence base.**
- A systematic literature review across:
  - PDF repair and recovery;
  - robust parsing, LangSec and parser differentials;
  - content disarm and reconstruction (CDR);
  - file carving and fragment reassembly, including memory forensics;
  - DEFLATE and compressed-stream error recovery;
  - font identification and glyph→Unicode recovery (OCR, VLM, cipher solving);
  - repair in other formats;
  - ML- and LLM-assisted repair;
  - strategy selection and program-repair analogues.
- **Output:** screened and extracted `SRC-` and `CLM-` records with verified quotes.

**W2 — Gap synthesis → hypotheses.**
- Turn claims into `GAP-` records: explicit future work, untested conditions, contradictions and cross-field transfers.
- Check whether each is still open using later citing work.
- Score, cluster and prioritize the gaps.
- Write testable `HYP-` records, each with a metric, a baseline and data.
- **Output:** the prioritized gap register and the hypothesis set.

**W3 — Damage realism and evaluation infrastructure.**
- Real-world damage evidence and corpora.
- `DMG-` damage classes with generator specs, and a frozen held-out suite.
- The metric suite from charter §3.
- A multi-engine baseline and oracle harness (qpdf, MuPDF, Ghostscript, Poppler, pdf.js, PDFium).
- REPDF replication.
- **Output:** specs for the benchmark harness, plus baseline numbers.

**W4 — Experiment specs (hand-off to coding agents).**
- Spec-ready hypotheses become GitHub wayfinder tickets.
- Each ticket has success criteria, data, baselines and a stop rule.
- **Output:** ready-for-agent issues. Integration into the product goes through ADRs.

**Practices, not workstreams:**
- **Knowledge-base stewardship**: the validator, merges and generated views.
- **Product integration**: ADRs against `pdfpundit-technical-design.md`.

## Phases and gates
| Phase | Content | Gate |
|---|---|---|
| P0 Foundations | Charter, knowledge base, validator, REPDF ingested, this plan | Charter Committee ✅ / ✗ |
| P1 Scoping | 60–100 titles screened, 10–15 read, 15–30 gaps; plan revised from evidence | Evidence changes the plan, or we justify why it doesn't |
| P2 Tooling | Tooling ledger, researched and then hands-on verified | Every Adopt/Trial entry verified in the container |
| P3 5k plan | Per-workstream plans | Multidisciplinary Review Board |
| P4 Full review + evaluation build | W1/W2 at full depth, alongside W3 harness specs and builds | Suite frozen before any tuning |
| P5 Experiments | Coding agents run the HYP- specs | Pre-registered metrics against the baselines |
| P6 Integration | Winning methods reach the PDFPundit design | ADR plus a regression gate on the REPDF corpus |

This session covers P0 to P3 and ends with a recommendation: more research, or start writing specs.

## Risks
| Risk | Mitigation |
|---|---|
| Hallucinated evidence | Validator: DOI↔Crossref check, and quotes must match cached full text. Unverified claims can't lift a gap above `candidate`. |
| Anchoring on REPDF | Adjacent-field search terms that don't come from REPDF; a red-team reviewer; plan-delta tracking. |
| Overfitting to a known generator | Held-out suite specified from real-world evidence (charter §3a). |
| Access limits | OpenAlex and Semantic Scholar are rate-limited without keys; paywalled papers. Use free APIs first and ask the user for keys at the gate. |
| Scope creep into building the product | W4 hands off specs. This program doesn't build PDFPundit. |

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 1 | 2026-09-26 | Initial draft | REPDF ingestion (SRC-0001) and the orchestration review (committee 000) |
