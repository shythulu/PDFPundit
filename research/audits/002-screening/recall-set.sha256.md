# Hidden recall set: commitment

```
99c270350914f49f4fe535ff161c741e77e40e335430526554197799a44d2a67  qgs.json
```

- **What it is:** 16 Crossref-verified works, as a JSON list of `{title, doi, year}`. The chair compiled it before Stage 1.5 and kept it outside the repository, so the sweep agents never saw it. Committee 002 used it to measure recall: 13 of 16.
- **When it was made:** the file's modification time is 2026-09-27 15:53 UTC. That is before the Stage 1.5 agents were launched.
- **When this hash was committed:** 2026-10-05, after the sweep, following committee 003's residual risk 2.
  - So this hash does **not** prove the Stage 1.5 recall figure was measured against a set fixed in advance.
  - It **does** fix the set from now on. The P4 full review must report recall against exactly this file.
- **When the file will be published:** it will be committed in full after the P4 recall measurement, so anyone can check it against this hash.
- **How to check:** `sha256sum qgs.json`, then `python research/tools/screening_audit.py recall --gold qgs.json --exclude-refs-of SRC-0001`.
