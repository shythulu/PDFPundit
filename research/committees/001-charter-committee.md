---
committee: 001
topic: Research charter (rev 1) and 10,000 ft program plan (rev 2)
date: 2026-09-27
charge: Are the charter and the 10k plan fit to steer a program toward bleeding-edge, forensically sound PDF repair?
members:
  - seat: PDF internals & repair engines (ran corpus and engine experiments)
  - seat: digital-forensics practitioner (ISO/IEC 27037/27041/27042, NIST SP 800-86, CFTT, SWGDE, Daubert, all cited from memory)
  - seat: red-team methodologist (PRISMA 2020, Kitchenham, Wohlin snowballing, cited from memory; required to raise ≥3 blocking issues)
  - diversity: three different model configurations; no reviewer saw the chair's reasoning
verdicts: [approve-with-changes, approve-with-changes, approve-with-changes]
blocking_issues: 12 (accepted 11, accepted-in-part 1; the part rejected was escalated to the user)
paid_searches: 0 by reviewers; 1 by the chair (SRCH-0001)
outcome: charter rev 2, 10k rev 3, validator hardening + 7 regression tests, OBS-0001..0004, new DMG and GAP records
---

# Committee 001: Charter Committee

## Weakest parts, as named by each reviewer
- **PDF internals:** "the forensic-fidelity pillar … rests on a two-way recovered/synthesized split
  that treats a rebuilt xref like a substituted font and never counts content a repair throws away."
- **Forensics:** "the one criterion that makes this a *forensic* … tool (§3c, resting on GAP-010) is
  simultaneously the least evidenced item in the register."
- **Red team:** "everything downstream … derives from one paper plus the chair's own inference,
  while the tooling projects an appearance of verification exceeding what is verified."

**Correlation check:** two of the three converged on the forensic-fidelity pillar. We treat that
as a real signal and have rebuilt §2 and §3(c) around it. We also count it as a correlated-view
warning: all three read the same chair-authored artifacts, and no reviewer saw raw sources other
than REPDF.

## Blocking issues and chair responses

**PDF internals**
| # | Issue | Response |
|---|---|---|
| PI-B1 | The corpus was described from its docs, not its bytes. C9 really has 12–30 substitutions over 15–25 streams, not 1. | **Accepted.** Reproduced over all 1,000 files (OBS-0001..0004, script `research/experiments/corpus_characterize.py`): C9 has 12–30 bytes in 11–28 streams. The same run also found that C7/C8 overwrite font streams in place with spaces, size-preserving (OBS-0003), and that one C6 file is a generator defect (OBS-0004). Charter §3(a) is corrected. GAP-013 records the paper-vs-dataset contradiction. The C9 salvage in the technical design is flagged, since it assumes one byte. |
| PI-B2 | The evidence model can't hold code citations or observations. | **Accepted.** Claims can now carry a code `locator`, checked against a cached checkout at the pinned commit (`research/tools/fetch_code.py`). A new `OBS-` record type holds reproducible observations. Gaps, damage classes and hypotheses may cite either. |
| PI-B3 | Provenance too coarse; no omission measure. | **Accepted.** Charter §2 now has six provenance grades plus an **omission rate** measured against an upper bound computed from the damaged file itself. |
| PI-B4 | No baseline protocol. | **Accepted.** Charter §3(b): a null-repair baseline, pinned versions, the best of the declared option sets per tool, and scoring in ≥2 engines. |

**Forensics**
| # | Issue | Response |
|---|---|---|
| FO-B1 | No adversarial or anti-forensic damage. | **Accepted.** DMG-009 added. §2 defines tamper indicators, and §3(c) requires them to be detected and reported, never repaired away. |
| FO-B2 | No ground truth for fabrication. | **Accepted.** §3(a): fidelity and fabrication are scored only on known originals damaged with fitted models, or on verified natural pairs. Found-corrupted files count toward robustness only. |
| FO-B3 | ML/LLM reproducibility. | **Accepted, with scope.** The product path must use local, versioned, deterministic models, recorded in the provenance record. Research exploration may use hosted models, provided versions are recorded and the results aren't treated as product evidence. |
| FO-B4 | GAP-010 is immature. | **Accepted.** The provenance-record schema is now a W3 deliverable, and its existence is a spec-readiness condition (§6). GAP-010 has been re-scored, and the scoping sweep targets the forensic tool-validation literature. |

**Red team**
| # | Issue | Response |
|---|---|---|
| RT-B1 | Everything grows from a single seed paper. | **Accepted.** Stage 1.5 starts from 4 literature seed communities plus an engine source study. A quasi-gold set of about 15 works is kept hidden from the sweep agents so recall can be measured. Counts are reported PRISMA-style. Stop rule for P4: fewer than 5% new includes, or 3 iterations. |
| RT-B2 | The validator verifies less than it claims. | **Accepted, all of it.** `fetch_fulltext.py` now requires a PDF for document types (unless `--local-ok`), a title match, and records `text_sha256` plus the extractor and its version. `kb_validate.py` now detects edited caches, requires quotes of ≥8 words, requires page numbers for multi-page sources, checks that every number in a paraphrase appears in its quote (`partial` otherwise), verifies code citations and OBS records, excludes critiques from evidence, and adds the damage-class acceptance and spec-ready rules. `research/tools/test_kb.py` has 7 regression tests, including a planted `.txt`, a wrong number and an edited cache; all pass. The validator also caught 3 of the chair's own claims (CLM-0001, CLM-0004, CLM-0012), which have been fixed. Its docstring now says what it does **not** verify: entailment. |
| RT-B3 | No decision rule; the held-out suite is circular. | **Accepted in part.** Adopted: pre-registration before P5, one primary endpoint, a fabrication ceiling, bootstrap CIs with multiplicity correction, a suite author walled off from the method work, originals disjoint from SRC-0002, user sign-off, and REPDF used only as a replication reference. **Rejected:** making "replicate 90.67%" a P1 gate. Replication needs a reimplementation, since REPDF's code isn't public (SRCH-0001), and P1 is literature scoping. **Escalated to the user, who chose a W3 gate before P5 (2026-09-27).** |
| RT-B4 | Governance is prone to sycophancy. | **Accepted.** A screening-auditor agent re-screens a 20% sample and reports agreement. One seat per committee spot-checks 5 random claims against raw sources. The board has a replication seat. The user signs off on the frozen suite and the endpoint. |

## Non-blocking suggestions
- **Adopted:**
  - the red team's **alternative plan as a hybrid**: W3 becomes corpus-first and differential (real malformed files from engine test suites, bug trackers and fuzz corpora; failures clustered into a taxonomy);
  - a clean-room rule for copyleft sources;
  - emission strategy as a research question, covering append-only overlays that preserve signatures;
  - a named conformance oracle;
  - a PII rule;
  - a validation report before P6;
  - confidence intervals;
  - `refute_if` on hypotheses;
  - pinned extractor versions;
  - the 5k plan link marked as a P3 deliverable;
  - a contradiction gap for REPDF's internally inconsistent tables (GAP-013);
  - new damage classes DMG-009 to DMG-016.
- **Moved:** contacting the REPDF authors for code. It's outward-facing, so it's offered to the user as an option.

## Plan delta
- Charter rewritten: §2 provenance grades, §3(a) to (c) plus the decision rule, §4 threat model, §5 clean-room and ethics, §6 evidence kinds.
- 10k plan: W3 reframed; RQ7 added on integrity and emission; two gates added.
- Validator hardened.
- 4 observations added.
- A substantial change overall.
