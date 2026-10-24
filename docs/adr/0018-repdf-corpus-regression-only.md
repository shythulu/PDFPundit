# 0018. The REPDF corpus is a regression set, kept outside the repo

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-002, D-012, D-013, D-036, D-077; GitHub issues #7, #8; research charter §3, committee 000 B3, committee 001 FO-B2

## Decision

1. The REPDF corpus has no licence, so it lives outside the repo. Local runs reach it through `PDFPUNDIT_CORPUS`, pinned at commit `e547d4d`.
2. `bench/repdf_manifest.txt` lists its 1,100 git blob SHA-1s. The harness refuses any file not in the manifest.
3. CI does a sparse, depth-1 checkout of the 44-file smoke subset. Nothing is cached or uploaded.
4. The corpus is for replication and regression only. It is never a held-out test set.
5. Fixtures follow the measured per-class byte table, not the paper's one-line descriptions:
   - C9 replaces bytes, as described in [0009](0009-c9-salvage-certainty.md).
   - C7 and C8 overwrite font data in place with 0x20 bytes. No font program survives anywhere. C8 has no CMap.
   - One C6 file is defective.
   - Save As originals use xref and object streams. Print originals use a classic xref.

## Why

- Shipping an unlicensed corpus is not ours to do.
- A blob manifest makes every score traceable to exact bytes.
- Tuning on the same 50 base documents you test on inflates scores. Font maps leak across files of one document (93.1% ToUnicode overlap, OBS-1104).

## Rules for agents

- "Beat REPDF on its corpus" is not a success claim. Say "regression baseline on the REPDF corpus".
- Generalisation claims need held-out data: known originals with fitted damage, or verified natural pairs. Splits group by base document and font family.
