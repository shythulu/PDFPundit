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
  updated: "2026-10-05T21:45:00.000Z"
  progress: 45
---
# Bleeding-edge PDF repair — 10,000 ft program plan

> **Altitude:** this is the broad view: goal, questions, workstreams, order, gates and risks.
> Specifics live elsewhere:
> - success criteria and metrics: [research/charter.md](../../research/charter.md)
> - work packages, tools and procedures: [bleeding-edge-repair-5k.md](bleeding-edge-repair-5k.md) (P3 deliverable; rev 3: tool slots filled from the Stage 2 ledgers and PR #16's evidence, re-verified hands-on and challenged in Stage 4)
> - evidence: [research/README.md](../../research/README.md)

## Destination
Take PDFPundit beyond the one paper its engine reimplements (REPDF, SRC-0001) to repair that:
- beats the strongest existing tools on **realistic** damage, and
- **can show which content it recovered and which it synthesized.**

We get there through the literature and through what mature engines already do, not by guessing.

## What we see from up here
*Rewritten from the P1 scoping evidence (rev 4) and updated from the Stage 2 tooling and PR #16 evidence (rev 5) and the Stage 4 re-validation (rev 6).*

- **Trust is the hard part, not coverage.** The usual ways of deciding that a repair is right are weaker than the design assumed:
  - salvaged stream prefixes are usually silently wrong (GAP-151);
  - a checksum match doesn't make a correction unique, and engines ignore checksums anyway (GAP-052, GAP-253);
  - language priors make decoders "correct" text into fabrication (GAP-201);
  - engines agreeing with each other proves little (GAP-102).

  Stronger checks now have first results. Replaying the encoder reproduces every sampled Word "Save As" stream but almost no "Print to PDF" stream, and even replay plus a checksum does not make a correction unique (GAP-154, GAP-052). Two pure-Rust replay candidates now exist, and a file's own intact streams supply the replay settings without knowing who wrote it. Grammar checks and natural pairs with known originals are still untested as acceptance checks (GAP-202, GAP-100).
- **Engines disagree on basic repair rules, and that is an attack surface.** They differ on which duplicate object wins, whether compressed objects are carved, and what happens to truncated streams (GAP-250, 251, 252). Attackers exploit exactly those choices (GAP-101, GAP-104). A forensic repair must report its choices, not hide them.
- **Real damage with known originals exists.** Truncated web-crawl captures pair with later full fetches (GAP-100). Corrupted UDHR translations pair with clean Unicode texts (GAP-202). Engine test suites add unlabelled real failures (GAP-257). Fidelity can be scored on real damage for two of REPDF's weakest areas, truncation and fonts.
- **Our reference benchmark doesn't match its paper** (GAP-013). Replication has to use the corpus as released.
- **Repair depends on who wrote the file.** The two producers in the benchmark differ in encoder, offsets and fonts, so producer profiles shape the damage models and some repair checks. Replay settings no longer need a profile, since each file calibrates its own (GAP-351). How far a profile transfers to other producers is open (GAP-002).
- **Proving a low error rate takes more documents than we have.** Errors cluster within a document, so certifying a small wrong-answer rate needs about 150 clean documents, not 150 clean slots (GAP-354). Splitting by document also leaks font knowledge between splits (GAP-358). REPDF cannot test glyph-shape methods at all, because its font-loss classes remove the font programs (GAP-357). OCR references must match each page's script (GAP-353).
- **A parallel study (PR #16) reproduces.** Its damage-level numbers reproduce from its committed scripts. Its stream-level gains are not yet shown on the benchmark's own per-file metric (GAP-300), so its design changes stay unreplicated until the 5k work packages test them.
- **One close competitor, several thin fields.** CPR (DFRWS EU 2026) is the nearest work on fonts, and its code can be run as a baseline. DEFLATE recovery has barely moved since 2013, and nobody identifies nameless subset fonts. Those thin fields are where new work is most likely.
- **Coverage is uneven.** A hidden recall check found 13 of 16 known-relevant works. The misses cluster in classic file carving, which P4 must fill.

## Research questions
1. **Damage:** what really happens to PDFs, how often, and how do we reproduce it?
2. **Structure and streams:** how much can be recovered from realistic structural and compressed-stream damage?
3. **Text and fonts:** how do we recover meaning when fonts or Unicode maps are lost, in any script, without a language prior inventing it?
4. **Trust:** how do we know a correction is right? How do we measure what was fabricated and what was dropped?
5. **Strategy:** how does the engine choose a repair path per file, and when should it abstain or ask a human?
6. **Transfer:** which ideas from other fields and other engines carry over?
7. **Integrity:** how do we repair without destroying evidence (earlier revisions, signatures, signs of tampering), report the choices we made, and write the result?

## Workstreams
- **W1 Evidence base**: literature plus an engine source study, screened and extracted with machine-checked evidence.
- **W2 Gaps → hypotheses**: what's still open, ranked, and turned into testable claims.
- **W3 Damage and evaluation**, corpus-first and differential:
  - real malformed files, natural pairs and damage evidence;
  - fitted damage models;
  - a held-out suite;
  - metrics, baselines, verification oracles and provenance records;
  - REPDF replication, with CPR as a second reference.
- **W4 Experiment specs**: hand-off to coding agents.

Two practices run alongside them rather than as workstreams: knowledge-base stewardship, and feeding results into the product through ADRs.

## Order and gates
```
P0 Foundations ─▶ P1 Scoping ─▶ P2 Tooling ─▶ P3 5k plan ─▶ P4 Full review ∥ Eval build ─▶ P5 Experiments ─▶ P6 Integration
  (charter,       (evidence     (verified     (review       (depth + frozen       (coding       (ADRs,
   knowledge base) shapes plan)  tools)        board)        held-out suite)       agents)       regression gate)
```
- **After P1:** has the evidence changed the plan? If not, say why. Each damage class must be characterized from real bytes, not from its documentation.
  - *Result (rev 4):* yes. Trust and verification became the spine of the plan (RQ4). Natural pairs made corpus-first W3 feasible. CPR joined as a reference. Carving is flagged for P4 top-up. REPDF's classes are characterized from its bytes (GAP-013).
- **After P3:** the review board approves, or escalates to the user. The board also rules on the product flags P1 raised (see risks).
- **Before P5:** four things must be done:
  - the REPDF replication;
  - CPR run as a baseline;
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

  It can't check that a quote actually supports the claim. An independent auditor spot-checks that part (audit 002).
- **Silent wrongness.** Output that parses and renders can still be wrong, and every cheap check we had was shown to be weak (see above). Grades 2 and 3 were tightened in charter rev 2.1 and again in rev 2.3. Natural pairs and replay oracles are the counter.
- **Staying anchored to REPDF.** Countered by:
  - four seed communities;
  - the engine study;
  - a hidden recall check, 13 of 16 found (carving is the gap);
  - a red-team review.
- **Optimizing to a synthetic benchmark.** A held-out suite built from real damage and natural pairs counters it.
- **Design assumptions contradicted by evidence.** P1 flagged five of them in the technical design, and Stage 4 a sixth:
  - "last copy wins";
  - Adler-32 acceptance for C9;
  - refusing encrypted files;
  - no expansion guard;
  - exact keyword matching;
  - lopdf as the sole loader (other pure-Rust loaders open benchmark files it cannot, GAP-356).

  None is changed here. They go to the review board, and only board-backed changes become ADRs.
- **Licences and data ethics.** Some sources can't be copied or reused:
  - copyleft engines and tools (MuPDF, Ghostscript, Poppler, ZipRec);
  - CPR's non-commercial licence;
  - personal documents linked from engine bug trackers.

  Clean-room rules and a no-PII rule apply.
- **Access limits** (paywalls, API keys, blocked hosts). Free sources come first. The user is asked for keys, and for any contact with authors or the PDF Association, at a gate.
- **Drifting into building the product.** This program ends at specs.

## Revision log
| Rev | Date | Change | Cause |
|---|---|---|---|
| 1 | 2026-09-26 | Initial draft | REPDF ingestion (SRC-0001), committee 000 |
| 2 | 2026-09-27 | Pruned to the broad altitude (details moved to the charter and 5k); added engine source study to W1 and RQ6 | User clarified altitudes; user approved the engine source study |
| 3 | 2026-09-27 | Added RQ7 (integrity and emission); W3 made corpus-first and differential; added a characterization gate, pre-P5 gates (replication, frozen suite, pre-registration) and a pre-P6 validation report; restated the evidence-risk wording | Charter Committee 001 (12 blocking issues); OBS-0002 showed the corpus's C9 differs from the paper; user chose the replication gate |
| 4 | 2026-09-27 | Rewrote "what we see" from evidence; sharpened RQ1, 3, 4, 5 and 7; added verification oracles and CPR to W3; recorded the P1 gate result; added CPR as a pre-P5 baseline; added risks: silent wrongness, contradicted design assumptions, licences and data ethics | Stage 1.5 scoping sweep: five agents, 150 sources, 271 claims, 51 gaps; hidden recall 13 of 16; committee 002 |
| 5 | 2026-10-05 | Updated "what we see": first results for encoder replay (producer-specific, not proof of uniqueness), producer dependence, PR #16 reproduced; 5k link now points at rev 1; noted charter rev 2.3 under silent wrongness | Stage 2 tooling (four agents), the PR #16 ingest and committee 004 |
| 6 | 2026-10-05 | Updated "what we see": two replay candidates and per-file calibration; producer profiles no longer feed replay; a new bullet on certification scale, font leakage, REPDF's missing font programs and per-script OCR; a sixth contradicted assumption (lopdf as sole loader); 5k link now points at rev 3 | Stage 4 verifier and contrarian; committee 005 |
