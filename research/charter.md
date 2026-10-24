---
title: Research charter — bleeding-edge PDF repair
status: rev 2.3 (Charter Committee 001 applied; scoping-sweep and Stage 2 evidence applied; pending the review board)
owner: shythulu
updated: 2026-10-05
---

# Research charter: bleeding-edge PDF repair

## 1. Goal
Advance PDFPundit from *a Rust reimplementation of REPDF* (SRC-0001) to **the most capable and
most trustworthy PDF repair engine we can evidence**. Evidence comes from three places:
- the literature, read systematically;
- the recovery practice built into mature engines (their source code);
- reproducible experiments on real and realistic damage.

What those sources leave open becomes testable hypotheses, and those become specs for coding agents.

Writing or publishing articles is **out of scope**. Outputs are knowledge-base records, plans, specs and, later, code.

## 2. Definitions
| Term | Meaning here |
|---|---|
| **Repair** | A new, standards-conforming PDF produced from a damaged one. The original is never mutated. "Conforming" means it passes a named conformance oracle (chosen in W3), not a lenient reader. |
| **Recovery** | Getting content out (text, images, structure), whether or not a valid PDF results. |
| **Provenance grade** | Every output element is labelled with exactly one of the six grades below. |
| **Fabrication** | Content in the output (grade 6, or a wrong grade 3–4 inference) that wasn't in the original: a wrong glyph→Unicode mapping, wrong page order, wrong revision, wrong font inference. |
| **Omission** | Content still present in the damaged file's surviving bytes that the repair drops: unreachable objects, earlier revisions, partially decodable streams. It is measured against an *upper bound* computed from the damaged file itself. |
| **Damage class** | REPDF's C1–C10 **as the corpus actually implements them** (OBS-0001..0004), extended by `DMG-` records (`research/damage/`). |
| **Tamper indicator** | Structure that suggests deliberate manipulation (shadow/incremental-update attacks, object-number reuse, parser-differential constructs). It is detected and reported, never "repaired away". |

**The six provenance grades:**

| # | Grade | Examples |
|---|---|---|
| 1 | Verbatim | Bytes copied unchanged from the damaged file |
| 2 | Partially decoded | Output of a corrupt stream shown to be independent of the damage: bytes decoded before a *known* damage position, or output identical under two different dummy dictionaries (CLM-0337) **from a restart point confirmed by trailer alignment or a following decodable block**. Unconfirmed restarts certify wrong bytes: a blind scan found 51,120 false starts that agree under both dictionaries (OBS-0900). A decode-until-error prefix is **not** grade 2 by default: in C9 it holds wrong bytes in 95% of damaged streams (OBS-0300, GAP-151). |
| 3 | Corrected and verified | A correction that is the **only** candidate passing a checksum **and** an independent check (encoder replay, content grammar, /Length1 or font-table checksums), where "only" means: candidates were enumerated exhaustively over the search space, **trailer edits included**; a stated prior prefers a body correction over a trailer rewrite; and a content-grammar check was applied wherever the stream type has a grammar. A checksum match alone is not verification, and neither is checksum plus encoder replay (GAP-052, GAP-253, GAP-154; OBS-0903, OBS-0904). When several candidates pass, the differing bytes are marked indeterminate, not guessed (CLM-0105). |
| 4 | Structurally inferred | From surviving bytes: page order, object renumbering, re-linked fonts, chosen revision |
| 5 | Non-content synthesis | xref, trailer, `/Length`, header |
| 6 | Content synthesis | Substituted fonts, generated ToUnicode, default MediaBox, inferred placement |

## 3. Success criteria (all three must hold)

### (a) Held-out realistic damage
- **Fidelity metrics use known originals.** Fidelity and fabrication are scored only on documents whose originals we know, damaged by **damage models fitted to real-world evidence**. That evidence comes from ≥2 named real-world damage sources, identified in P1, with a fallback.
  - Verified *natural pairs* are also allowed: real damaged files whose originals are independently available, such as truncated crawl captures with later full fetches. Each must be verified before use.
  - **Named in P1** (candidates until the review board confirms them):
    1. Common Crawl truncated captures paired with SafeDocs full refetches: truncation, natural pairs. 34 of 35 checked pairs are exact prefixes; every pair needs that check (OBS-0201, GAP-100).
    2. Corrupted UDHR translations paired with unicode.org texts: font encoding, natural pairs. The pairs and the OHCHR terms are not yet verified (GAP-202).
    3. Measured NAND bit-flip rates from chip-off dumps, as damage-model parameters (SRC-0126).
    - Fallback, robustness only: the engines' regression corpora (OBS-0500) and the SafeDocs Issue Tracker corpus (CLM-0293).
- **Found files feed robustness only.** Files found already corrupted, with no known original, count only toward robustness metrics: crash rate, plausibility, cross-engine agreement.
- **The suite is walled off from the method work.**
  - The suite is authored by an agent with no access to the engine design or the method work.
  - Its originals and producers are disjoint from SRC-0002.
  - It is frozen by hash, signed off by the user, and never used for tuning.
- **REPDF's corpus (SRC-0002) is for replication and regression only.** Its real generator differs from the paper:
  - C9 changes exactly one byte in each of 11–28 stream bodies, plus bytes in object syntax (font width arrays, dictionaries, indirect lengths) in 99 of 100 files: 12–30 bytes per file (OBS-0002, OBS-0005, OBS-0300).
  - C7/C8 blank font streams in place with spaces (OBS-0003).
  - One C6 file is a generator defect (OBS-0004).
  - It is too well known to serve as a held-out test.

### (b) Head-to-head baseline protocol
- **Tools compared:**
  - qpdf/pikepdf;
  - MuPDF (mutool);
  - Ghostscript;
  - Poppler;
  - pdf.js;
  - PDFium;
  - plus a **null-repair baseline**: tolerant readers opening the damaged file directly, with no repair.
- **Protocol:**
  - Tool versions are pinned.
  - Each tool gets the best of a small declared set of option configurations.
  - The same inputs and time limits apply to every tool.
  - Every output is scored after rendering and extraction in **≥2 engines**.
- **REPDF's published numbers** are a replication reference on SRC-0002 only. REPDF's code isn't public (SRCH-0001), so it isn't a runnable baseline. Replicating it, by black-box runs of repdf.site if allowed or by reimplementation, is a **W3 gate before P5** (decided by the user, 2026-09-27).

### (c) Forensic fidelity
- **Provenance record.** Every repair emits a record, in the style of a custody log, containing:
  - input and output hashes;
  - tool, version and configuration;
  - the provenance grade per element;
  - tamper indicators found;
  - the version and checksum of any learned model used.
- **Fabrication and omission are both measured.** Better recovery doesn't count if it comes with more fabrication or omission.
- **Learned components in the product path** (ML, LLM, VLM) must be local, versioned and deterministic.
  Research exploration may use hosted models, but only with recorded versions, and its results aren't product evidence.
- **Signatures.** Repairing a signed file invalidates its signature. Where feasible, prefer emission strategies that keep the signed bytes intact (see RQ on emission). Never imply a repaired file is authentic.

### Decision rule (pre-registered before P5)
Before any experiment runs, `research/preregistration.md` is committed. It contains:
- one **primary endpoint**;
- a **fabrication ceiling**;
- a minimum meaningful margin;
- the suite size;
- a paired per-file comparison with bootstrap confidence intervals and correction for multiple comparisons.

The user signs off on the endpoint and the frozen suite.

**Metric suite** (specified in the 5k plan, W3):
- extractable-text accuracy (word F1 plus an order-aware measure);
- visual fidelity (per page, in ≥2 engines);
- structural fidelity;
- cross-engine consistency;
- fabrication rate and omission rate;
- runtime and failure rate.

All are reported per class with confidence intervals.

## 4. Scope
**In scope:**
- Repair targets:
  - structural, stream, font and text recovery;
  - truncated, fragmented, carved and partially overwritten files;
  - transfer-mangled files and files with producer bugs;
  - incremental updates and choosing the right revision;
  - signatures and encryption, when the key is available;
  - filters other than Flate;
  - emission strategy, including append-only incremental overlays.
- Methods and neighbouring disciplines:
  - robust and differential parsing;
  - content disarm and reconstruction (CDR);
  - methods transferable from other formats;
  - ML, LLM or VLM assistance under §3(c);
  - repair-strategy selection;
  - evaluation infrastructure.

**Threat model:**
- Accidental damage is the repair target.
- **Hostile input** is a detection and safety target: decompression bombs, resource exhaustion, exploits, anti-forensic manipulation.

**Out of scope:**
- Publishing.
- Password cracking.
- Building malware.
- Non-PDF formats, except as a source of transferable methods.
- UI and aesthetics.

## 5. Constraints
- **Product decisions** (currently in force, from `nimbalyst-local/plans/pdfpundit-*.md`): pure Rust, no compiled C in the shipped binary; `lopdf` as the only emitter; originals never mutated; the C1–C10 taxonomy.
  - Research may recommend reopening any of them, but must flag the conflict with evidence.
  - Known flag: `pdfpundit-technical-design.md` §4.5/§16 (single-byte-flip C9 salvage). Per stream, the one-byte premise holds (OBS-0005, OBS-0300). But each C9 file has 11–28 damaged streams plus changes outside streams that a stream-only salvage never touches (OBS-0005), and accepting a candidate on an Adler-32 match alone is not verification (GAP-052, GAP-253). A replay oracle is a candidate fix (GAP-154, HYP-150).
- **The research harness is unconstrained.** Any language, tool or service is allowed as an oracle, baseline or generator.
- **Clean-room rule for copyleft sources** (MuPDF and Ghostscript are AGPL, Poppler is GPL):
  - research agents may read them and cite short quotes;
  - behaviour is described in our own words;
  - specs describe behaviour only;
  - coding agents are never pointed at copyleft source;
  - nothing is copied into PDFPundit.
- **Data ethics:**
  - public corpora only;
  - no committing samples that contain personal data;
  - corpus licences recorded;
  - only open-access full text cached, never committed;
  - no scraping that breaches terms of service.
- **Budget:** Parallel Search is capped at 80 paid calls for the planning campaign (user-approved), and every call is logged. Free APIs come first. New paid or keyed services need the user's approval: the user creates the accounts, batched at the tooling gate.

## 6. Governance and evidence
- **Committees.** The protocol is in `research/committees/000-orchestration-plan-review.md`.
  - The chair answers every blocking issue with a reason.
  - **Any blocking objection the chair rejects is escalated to the user.**
  - One seat per committee spot-checks 5 random claims against raw sources.
  - Review boards include a replication seat that runs something.
- **Screening.** Every screening decision is logged with a reason code. A second, independent agent re-screens a 20% sample, and agreement is reported. Recall is measured against a quasi-gold set of known-relevant works that is hidden from the sweep agents.
- **What counts as evidence.** Three kinds, all machine-checked by `research/tools/kb_validate.py`:
  - a **verified quote**: in the cached text, ≥8 words, correct page, and every number in the paraphrase present in the quote;
  - a **code citation**: quote found in the cited lines of the file at the pinned commit;
  - a **reproducible observation** (`OBS-`): committed script, hashed inputs (or a pinned commit for an external corpus).
  - Critiques (`kind: critique`) are our own inference and **never count as evidence**.
- **Spec-ready.** A hypothesis is spec-ready only when all of these hold:
  - it has a measurable metric and a `refute_if`;
  - its data can be reached from the container;
  - its baselines have been verified in the container;
  - its gaps are `still_open: yes` and were checked within 90 days;
  - its gaps cite at least one verified piece of evidence;
  - the provenance-record schema (§3c) exists.
- **Stop rules.** P1 scoping is time-boxed to one sweep round. The P4 full review stops when an iteration adds fewer than 5% new includes, or after 3 iterations.
- **Before P6 (integration):** a validation report with tool-testing assertions and documented known limitations.

## 7. Evidence so far
- SRC-0001 (REPDF) and SRC-0002 (its dataset); OBS-0001..0005 characterize how the corpus generator actually works.
- **P1 scoping sweep** (five agents, committee 002): 150 sources, 271 claims, 51 gaps, 23 damage classes, 13 observations, 3 draft hypotheses.
- Live counts: `research/README.md`. Views: `research/gaps/register.md`, `research/damage/classes.md`.
- Committee minutes 000, 001 and 002.

## Revision log
- **rev 2.3 (2026-10-05, chair, committee 004):** tightened grade 2 (dictionary-invariant output counts only from a confirmed restart point, OBS-0900) and grade 3 (exhaustive enumeration including trailer edits, a body-over-trailer prior, a grammar check where one exists; replay plus checksum is not uniqueness, OBS-0903/0904). Nothing was loosened. The review board checks these changes.
- **rev 2.2 (2026-09-27, chair):** named the P1 real-world damage sources in §3(a) as candidates for the board; updated §7.
- **rev 2.1 (2026-09-27, chair, from scoping-sweep evidence):** tightened provenance grades 2 and 3, which the sweep showed were unsafe as written (a decode-until-error prefix is usually wrong; a checksum match is not unique). Corrected the C9 description (one byte per damaged stream, plus non-stream changes) and the §5 product flag. Nothing was loosened. The review board checks these changes.
- **rev 2 (2026-09-27):** Charter Committee 001 applied (`research/committees/001-charter-committee.md`).
