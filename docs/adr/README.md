# Architecture decision records

These ADRs record the decisions that shape PDFPundit. Read the ones that touch your area before you change it. If your change contradicts one, say so in the PR and amend or supersede the ADR. Do not override it silently.

## Index

| # | Decision | Status |
|---|---|---|
| **Product and boundaries** | | |
| [0001](0001-v1-scope.md) | v1 scope: REPDF damage, Latin fonts, three outputs | accepted |
| [0002](0002-outputs-placed-never-replaced.md) | Originals are never modified; outputs are placed, never replaced | accepted |
| [0003](0003-cat-is-the-only-way-in.md) | The cat is the only way in: no command-line interface | accepted |
| [0004](0004-pure-rust-no-network.md) | Pure-Rust build with no network code | accepted |
| [0005](0005-clean-room-copyleft.md) | Clean-room rule: coding agents never read copyleft source | accepted |
| **Engine** | | |
| [0006](0006-private-sync-engine-facade.md) | One package with a private, synchronous engine facade | accepted |
| [0007](0007-carver-loads-lopdf-emits.md) | The carver loads; lopdf emits and gates | accepted |
| [0008](0008-last-well-formed-copy-wins.md) | Duplicate objects: the last well-formed copy wins | accepted |
| [0009](0009-c9-salvage-certainty.md) | C9 stream salvage: the measured search, and certainty we can defend | accepted; needs a build change |
| [0010](0010-replay-oracle-stock-zlib.md) | Encoder replay checks C9 candidates, using stock zlib | accepted; timing assumed v1 |
| [0011](0011-repair-selection.md) | Repair selection: generate every candidate, pick by a verification tuple | accepted |
| [0012](0012-determinism.md) | Determinism: work budgets, pure reports | accepted |
| [0013](0013-batch-never-blocks.md) | The batch never blocks, and every answer records its source | accepted |
| **Fonts and text** | | |
| [0015](0015-font-recovery-ladder.md) | Font recovery: the document's own fonts first; no text beats wrong text | accepted |
| [0016](0016-generated-assets-committed.md) | Generated assets are committed | accepted |
| [0017](0017-outlined-text-and-ocr.md) | Text without a font: detect in v1; OCR is offline and never touches the repair | accepted; OCR engine **proposed** |
| **Evaluation** | | |
| [0018](0018-repdf-corpus-regression-only.md) | The REPDF corpus is a regression set, kept outside the repo | accepted |
| [0019](0019-metrics.md) | Metrics: a labelled regression gate, kept apart from REPDF-style OCR recall | accepted |
| **Forensics** | | |
| [0020](0020-tamper-evidence.md) | Signatures and tamper evidence: report, never repair away; provenance grades wait | accepted |
| [0021](0021-custody-mode.md) | Custody mode is a future feature, not v1 | accepted (deferred) |
| **UI** | | |
| [0014](0014-terminal-input-ownership.md) | Terminal input: the app owns stdin on kitty | accepted |
| [0022](0022-ui-canvas-and-goldens.md) | UI: a framework-free canvas, tested cell for cell against the mockup | accepted |
| **Process** | | |
| [0023](0023-research-phase-closed.md) | The research phase is closed; the knowledge base becomes an archive | accepted |

## Reference shorthand

Several sources live on branches that were unmerged when these ADRs were written (2026-10-23).

| Shorthand | Meaning | Where |
|---|---|---|
| D-001 … D-161 | Decision-log rows | `nimbalyst-local/plans/implementation/decision-log.md` on `claude/wayfinder-implementation` (PR #21) |
| A-1 … A-7 | ADR candidates in that log | Same file, "ADR candidates" section |
| T-, F-, G-, H- | Implementation tickets | `nimbalyst-local/plans/implementation/plan.md` and `build-wave.md`, same branch |
| TD, DA | Technical design, desktop-app plan | `nimbalyst-local/plans/` on main. **Stale where these ADRs disagree.** |
| OBS-, CLM-, GAP-, TOOL-, HYP-, DMG- | Research evidence records | `research/` on `claude/festive-johnson-ym2hr6` (PR #19) |
| Committee 000 … 005, charter, WP- | Research minutes, charter, work packages | `research/committees/`, `research/charter.md`, `nimbalyst-local/plans/bleeding-edge-repair-5k.md`, same branch |

## Status values

- **accepted:** follow it.
- **proposed:** the owner has not ruled. Build nothing that depends on the proposal without asking.
- **superseded by NNNN:** read NNNN instead.

## Adding one

Write one only when a decision is hard to reverse, surprising without context, and the result of a real trade-off. Number it next in sequence. Keep the same sections: Decision, Why, Rejected, Rules for agents, Revisit if. Drop any section with nothing to say.
