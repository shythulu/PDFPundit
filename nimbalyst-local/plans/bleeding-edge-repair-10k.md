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
  updated: "2026-09-27T00:00:00.000Z"
  progress: 5
---
# Bleeding-edge PDF repair — 10,000 ft program plan

> **Altitude:** this is the broad view: goal, questions, workstreams, order, gates and risks.
> Specifics live elsewhere:
> - success criteria and metrics: [research/charter.md](../../research/charter.md)
> - work packages, tools and procedures: [bleeding-edge-repair-5k.md](bleeding-edge-repair-5k.md)
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
4. **Trust:** how do we measure repair quality, including what was fabricated?
5. **Strategy:** how does the engine choose a repair path per file, and when should it ask a human?
6. **Transfer:** which ideas from other fields and other engines carry over?

## Workstreams
- **W1 Evidence base**: literature plus engine source study, screened and extracted with verified quotes.
- **W2 Gaps → hypotheses**: what's still open, ranked, and turned into testable claims.
- **W3 Damage and evaluation**: realistic damage models, corpora, metrics and baselines.
- **W4 Experiment specs**: hand-off to coding agents.

Two practices run alongside them rather than as workstreams: knowledge-base stewardship, and feeding results into the product through ADRs.

## Order and gates
```
P0 Foundations ─▶ P1 Scoping ─▶ P2 Tooling ─▶ P3 5k plan ─▶ P4 Full review ∥ Eval build ─▶ P5 Experiments ─▶ P6 Integration
  (charter,       (evidence     (verified     (review       (depth + frozen       (coding       (ADRs,
   knowledge base) shapes plan)  tools)        board)        held-out suite)       agents)       regression gate)
```
- **After P1:** has the evidence changed the plan? If not, say why.
- **After P3:** the review board approves, or escalates to the user.
- **Before P5:** the held-out damage suite is frozen before any tuning.
- **After P5:** gains count only against the baselines and metrics registered in advance.

This planning session covers P0 to P3.

## Major risks
- **Believing invented evidence.** Every quote and citation is machine-checked.
- **Staying anchored to REPDF.** Adjacent fields, engine study and a red-team review counter it.
- **Optimizing to a synthetic benchmark.** A held-out, realistic suite counters it.
- **Access limits** (paywalls, API keys). Free sources come first, and the user is asked for keys at a gate.
- **Drifting into building the product.** This program ends at specs.

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 1 | 2026-09-26 | Initial draft | REPDF ingestion (SRC-0001), committee 000 |
| 2 | 2026-09-27 | Pruned to the broad altitude (details moved to the charter and 5k); added engine source study to W1 and RQ6 | User clarified altitudes; user approved the engine source study |
