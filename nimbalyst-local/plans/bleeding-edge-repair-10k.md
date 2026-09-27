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
  updated: "2026-09-27T12:00:00.000Z"
  progress: 5
---
# Bleeding-edge PDF repair — 10,000 ft program plan

> **Altitude:** this is the broad view: goal, questions, workstreams, order, gates and risks.
> Specifics live elsewhere:
> - success criteria and metrics: [research/charter.md](../../research/charter.md)
> - work packages, tools and procedures: [bleeding-edge-repair-5k.md](bleeding-edge-repair-5k.md) (P3 deliverable, not written yet)
> - evidence: [research/README.md](../../research/README.md)

## Destination
Take PDFPundit beyond the one paper its engine reimplements (REPDF, SRC-0001) to repair that:
- beats the strongest existing tools on **realistic** damage, and
- **can show which content it recovered and which it synthesized.**

We get there through the literature and through what mature engines already do, not by guessing.

## What we see from up here
- REPDF is excellent at the damage it tested and weak at what it didn't: compressed-stream
  damage, truncation, complex scripts, realistic or compound damage, and forensic
  trustworthiness. Its own authors call for real-world datasets, more producers, extensible
  font knowledge, and AI-assisted interpretation.
- Several neighbouring fields probably already solve parts of the problem:
  - file carving;
  - compression error recovery;
  - robust and differential parsing;
  - content disarm and reconstruction;
  - OCR and vision models.
- Mature open-source engines contain years of undocumented recovery practice.

## Research questions
1. **Damage:** what really happens to PDFs, and how do we reproduce it?
2. **Structure and streams:** how much can be recovered from realistic structural and compressed-stream damage?
3. **Text and fonts:** how do we recover meaning when fonts or Unicode maps are lost, in any script?
4. **Trust:** how do we measure repair quality, including what was fabricated and what was dropped?
5. **Strategy:** how does the engine choose a repair path per file, and when should it ask a human?
6. **Transfer:** which ideas from other fields and other engines carry over?
7. **Integrity:** how do we repair without destroying evidence (earlier revisions, signatures, signs of tampering), and how should the result be written?

## Workstreams
- **W1 Evidence base**: literature plus an engine source study, screened and extracted with machine-checked evidence.
- **W2 Gaps → hypotheses**: what's still open, ranked, and turned into testable claims.
- **W3 Damage and evaluation**, corpus-first and differential:
  - real malformed files and damage evidence;
  - fitted damage models;
  - a held-out suite;
  - metrics, baselines and provenance records;
  - REPDF replication.
- **W4 Experiment specs**: hand-off to coding agents.

Two practices run alongside them rather than as workstreams: knowledge-base stewardship, and feeding results into the product through ADRs.

## Order and gates
```
P0 Foundations ─▶ P1 Scoping ─▶ P2 Tooling ─▶ P3 5k plan ─▶ P4 Full review ∥ Eval build ─▶ P5 Experiments ─▶ P6 Integration
  (charter,       (evidence     (verified     (review       (depth + frozen       (coding       (ADRs,
   knowledge base) shapes plan)  tools)        board)        held-out suite)       agents)       regression gate)
```
- **After P1:** has the evidence changed the plan? If not, say why. Each damage class must be characterized from real bytes, not from its documentation.
- **After P3:** the review board approves, or escalates to the user.
- **Before P5:** three things must be done:
  - the REPDF replication;
  - a frozen, user-signed held-out suite;
  - a committed pre-registration.
- **After P5:** gains count only against the baselines and metrics registered in advance.
- **Before P6:** a validation report listing known limitations.

This planning session covers P0 to P3.

## Major risks
- **Believing invented or unsupported evidence.** The validator checks four things:
  - that each quote or code line exists in the cached source;
  - that the source's title matches;
  - that numbers in a paraphrase appear in the quote;
  - that observations are reproducible.

  It can't check that a quote actually supports the claim. Reviewers spot-check that part.
- **Staying anchored to REPDF.** Four seed communities, the engine study, a hidden recall check and a red-team review counter it.
- **Optimizing to a synthetic benchmark.** A held-out, realistic suite counters it.
- **Access limits** (paywalls, API keys). Free sources come first, and the user is asked for keys at a gate.
- **Drifting into building the product.** This program ends at specs.

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 1 | 2026-09-26 | Initial draft | REPDF ingestion (SRC-0001), committee 000 |
| 2 | 2026-09-27 | Pruned to the broad altitude (details moved to the charter and 5k); added engine source study to W1 and RQ6 | User clarified altitudes; user approved the engine source study |
| 3 | 2026-09-27 | Added RQ7 (integrity and emission); W3 made corpus-first and differential; added a characterization gate, pre-P5 gates (replication, frozen suite, pre-registration) and a pre-P6 validation report; restated the evidence-risk wording | Charter Committee 001 (12 blocking issues); OBS-0002 showed the corpus's C9 differs from the paper; user chose the replication gate |
