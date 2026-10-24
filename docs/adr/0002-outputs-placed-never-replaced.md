# 0002. Originals are never modified; outputs are placed, never replaced

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-7 (D-044, D-060, D-061, D-062); research charter §2

## Decision

1. The engine never writes to an input file.
2. Every output is placed with a no-clobber move. Tiers, tried in order:
   1. Atomic: `renameat2(RENAME_NOREPLACE)` or `MoveFileExW` without replace.
   2. Linked: hard link, then remove the temp name.
   3. Claimed-then-renamed: create the name exclusively, then rename over the claim. exFAT needs this.
3. The tier used is recorded in the run record.
4. Outputs go to `output_dir` or beside the input. A read-only destination fails loudly. Nothing is written anywhere else as a fallback.
5. Image directories are named by content (`<hash8>.images/`), so two runs never fight over a name.
6. Output text never implies the repaired file is authentic or original.

## Why

- PDFPundit is a forensic tool. Deleting or overwriting a file to make room can destroy evidence.
- A silent fallback location means a user cannot find, or worse, mistakes, the output.

## Rejected

- Overwrite with a backup copy. Still replaces bytes the user may need.
- Write to a temp or home directory when the destination is read-only. Hides where the output went.

## Rules for agents

- No code path opens an input with write access.
- Any new output kind goes through the same placement function.
