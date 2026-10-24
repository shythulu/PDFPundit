# Research close-out, 2026-10-23

The research phase is closed ([ADR 0023](adr/0023-research-phase-closed.md)). v1's decisions now live in [`docs/adr/`](adr/README.md). This page says where everything stands and what is left.

## Where the work lives

| Track | Branch / PR | State |
|---|---|---|
| Research knowledge base, part 1 | main (PR #17, #16, #18 merged) | Archive |
| Research, part 2: tooling, PR #16 ingest, re-validation | `claude/festive-johnson-ym2hr6`, draft PR #19 | Stages 2 to 4 done. Stages 5 and 6 replaced by these ADRs. |
| v1 build | `claude/wayfinder-implementation`, PR #21 | 49 of 51 planned tickets merged, plus F-01…10, G-01…11, CI-01, C8-01, C8-02, H-24 |
| Custody evidence schema | `impl/H-01` | Schema, worked example and validator scripts written; not reviewed; no custody code. Branches from before C8-02. |
| ADRs, glossary, this page | this branch | New |

Main holds none of the build and only part of the research. Agents reading main alone see stale design docs.

## Research: done

- 178 sources, 383 claims, 72 reproducible observations, 63 gaps, 138 tooling verdicts.
- REPDF's corpus damage measured per class. It differs from the paper's description, and the build follows the measurement.
- Verification rules for C9 salvage that the build adopts ([0009](adr/0009-c9-salvage-certainty.md)).
- 44 of 45 Adopt/Trial tools verified hands-on.

## Research: not done, and where it went

| Item | Outcome |
|---|---|
| Stage 5 review board | Replaced by ADRs 0007 to 0011, 0019 and 0023 |
| Stage 6 cold-start test and recommendation | Replaced by this page and `CONTEXT.md` |
| Access requests (CORE key, Docker for GROBID, etc.) | Dropped. Only research needed them. |
| Gates before research P5 (REPDF replication, CPR baseline, held-out suite, preregistration) | Dropped with P5. The held-out suite is backlog item 3 in ADR 0023. |
| No hypothesis reached spec-ready | They become backlog issues, not v1 work |

## Decisions the owner still has to make

| # | Decision | Recommendation | Where |
|---|---|---|---|
| 1 | Replay in v1 or after it | In v1, after the ambiguous-output change (assumed) | [0010](adr/0010-replay-oracle-stock-zlib.md) |
| 2 | OCR engine | tract-onnx + PaddleOCR, after a measurement | [0017](adr/0017-outlined-text-and-ocr.md) |
| 3 | Substitute font carries the input's `/W` | Keep, as built | D-156 |
| 4 | Work units count output bytes or not | Pick (a) or (b); the tree runs (c) | D-110 |
| 5 | Debug-log retention | Cap it | D-132 |
| 6 | When to tag v0.1.0 | After the release blockers below | D-142 |

Settled by the owner, 2026-10-23:

| Decision | Ruling | ADR |
|---|---|---|
| Ambiguous C9 repair | No guess. Keep the stream as found, mark differing bytes indeterminate. Needs a build change. | [0009](adr/0009-c9-salvage-certainty.md) |
| Leipzig word-list licence | Treat as CC BY-NC 4.0 | [0016](adr/0016-generated-assets-committed.md) |
| Custody mode | Future feature, out of v1 | [0021](adr/0021-custody-mode.md) |
| Replay encoder | Stock zlib, as the one exception to the no-C rule | [0004](adr/0004-pure-rust-no-network.md), [0010](adr/0010-replay-oracle-stock-zlib.md) |

## Housekeeping before more building

1. **Scrub copyleft links** from the research notes, as the owner chose in D-042. They are still on main and on the research branch. See [0005](adr/0005-clean-room-copyleft.md).
2. **Merge PR #19** as the final research archive.
3. **Merge or rebase `impl/H-01`** onto PR #21, then merge PR #21.
4. **Supersede the stale design docs.** Add a banner to `pdfpundit-technical-design.md` and `pdfpundit-desktop-app.md` pointing at these ADRs. The C9 ladder, the blocking job flow and "last in byte order wins" there are all overruled.
5. **Re-baseline** `bench/golden_smoke.csv` (C8 0.260 → 0.293, C10 0.408 → 0.407).
6. **Close GitHub issue #1** (wayfinder map) and open one issue per ADR 0023 backlog item.

## Release blockers on the build

- `THIRD_PARTY_NOTICES.md` and release notes say the word lists are CC BY 4.0. Change them to CC BY-NC 4.0 (D-143).
- `widths_fit` `i64` overflow (T-13b), marked "fix before any release".
- Windows test job: cancelled in both CI runs, so unproven (D-161).
- OSC 72 drop recheck in Ghostty, WezTerm, foot and iTerm2.

## Build work still open

| Work | State |
|---|---|
| Ambiguous C9 output: stop writing a pinned guess ([0009](adr/0009-c9-salvage-certainty.md)) | New, required for v1 |
| Encoder replay with static stock zlib, per-file calibration; purity gate and `deny.toml` updates ([0010](adr/0010-replay-oracle-stock-zlib.md)) | New, assumed v1 |
| Custody mode (T-33; H-02…H-21, H-32…H-35) | Deferred to a future release |
| Image extraction for Markdown export (D-114, H-06…H-10) | Not built |
| History, setup and help screens (H-25…H-30) | Designed, not built. Custody and evidence screens deferred with custody mode. |
| OCR (T-34) | Not started; engine unmeasured |
| C6 (0.355), C8 (0.293), C10 (0.407) smoke scores | Weakest classes; C8 is lowest |
