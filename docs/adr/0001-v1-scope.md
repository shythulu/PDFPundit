# 0001. v1 scope: REPDF damage, Latin fonts, three outputs

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-001, D-025, D-114, D-070; research charter rev 2.3

## Decision

v1 repairs the damage the REPDF corpus contains and nothing wider.

| In v1 | Out of v1 |
|---|---|
| Diagnose and repair REPDF classes C1 to C10 | The 23 proposed damage classes beyond C1 to C10 (`research/damage/classes.md`) |
| Latin fonts: English, French, Spanish | Arabic shaping, Chinese bigram scoring, Hindi |
| `<name>.repaired.pdf` that passes the gates in [0011](0011-repair-selection.md) | Image recovery scoring and re-placement |
| Markdown export, including the PDF's extracted images | Provenance grades, shadow revisions, signature shadows ([0020](0020-tamper-evidence.md)) |
| Info findings: Encrypted, Signed, OutlinedText, Type3Text, FontNotEmbedded, PagesOutsideTree | Repairing encrypted files. They are detected and refused. |
| | Custody mode, a future feature ([0021](0021-custody-mode.md)) |
| | Editing, forms, signing, cloud, accounts, mobile, Office input |
| | Setting our scores beside the REPDF paper's tables (D-070) |

## Why

- The research program asked "how far past REPDF can we go" and left every answer at draft or proposed. No hypothesis reached spec-ready (`research/hypotheses/index.md`, PR #19 branch).
- A narrow v1 gives a working, measured baseline first. Every later extension is then scored against it.
- Encrypted input yields garbage if repaired without the key. "Decrypt first" is the honest answer.

## Rules for agents

- A ticket that adds a damage class, a script or an output must cite a new ADR or an amendment to this one.
- The proposed DMG classes are the backlog for v2, not hidden v1 requirements.

## Revisit if

- v1 ships and the smoke gate is stable. Then pick extensions from the research backlog ([0023](0023-research-phase-closed.md)).
