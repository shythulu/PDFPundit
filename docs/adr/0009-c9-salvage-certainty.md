# 0009. C9 stream salvage: the measured search, and certainty we can defend

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** owner ruling 2026-10-23 on ambiguous output; D-007, D-056, D-066, D-072, D-111, D-128; impl/H-01 wording; research charter grades 2 and 3, committee 004 D1 and D2

## Decision

### The search
1. The C9 search follows the measured corpus, not the technical design's single-bit-flip ladder (TD §16).
   - REPDF's C9 damage replaces whole bytes, not single bits. Each damaged Flate stream has one replaced byte, in 7 to 26 streams per file, plus bytes outside streams (OBS-0300 rev 2, OBS-1000).
   - The measured ladder repairs 219 of the 221 streams that raise an inflate error. Most damaged streams (1,220 of 1,445) inflate cleanly and fail only Adler-32 (OBS-0300).
2. The search is checkpointed over all 255 replacement values per byte position, inside a window set by localizers (error offset, TrueType table checksums).
3. Work is budgeted in work units, not time ([0012](0012-determinism.md)).

### What counts as a repair
4. **Adler-32 is necessary, not sufficient.** A candidate that passes Adler-32 is a candidate, not a proof.
5. **Never stop at the first accept.** An `Exact` grade means the whole window was searched and every passing edit gave one distinct output. Two edits with the same output are still `Exact`.
6. **Several distinct outputs mean `Ambiguous`, and the engine does not guess.** Owner ruling, 2026-10-23:
   - The repaired PDF keeps that stream as found. No survivor is written into it.
   - The stream is marked Partial.
   - The report lists every survivor's edits and marks the byte positions where they differ as indeterminate.
   - This reverses the build's D-041 option (i), which wrote the lowest-ordered survivor. That code must change.
7. `Accepted` (passed checks, not exhausted) is counted separately from `Exact`.
8. **Decoded prefixes are not trusted by default.** A raw-deflate resync counts only from a restart confirmed by trailer alignment or a following decodable block.
9. Never keep bytes that a raw-deflate guess invented (D-128).
10. User-facing text says what was tried, for example "every single-byte replacement in its window was tried". It never overclaims.

## Why

- OBS-0903: one stream had 3 candidates passing inflate, Adler-32 and encoder replay. Only a grammar check rejected the two wrong ones.
- OBS-0904: 87 of 820 canonical damaged bodies pass every check after a trailer rewrite.
- OBS-1007 and OBS-1009: Adler-only first-accept is wrong about once in 245 to 324 streams.
- OBS-0300: decode-until-error salvage emits wrong bytes in 1,378 of 1,445 damaged streams (95%), because the damage is detected late.
- OBS-0900: blind resync certified 8.5 million wrong bytes from 51,120 false starts.

## Rules for agents

- An `Ambiguous` result is never reported as `Exact` or `Accepted`. Every survivor stays listed in the report.
- Encoder replay filters survivors where it is available ([0010](0010-replay-oracle-stock-zlib.md)).
- Prefer a grammar check, where one exists, to break ties. It is the only check that separated survivors in OBS-0903.
- Trailer edits are part of the candidate space. A fix that only rewrites the Adler trailer must say that no checksum covers the data.

## Why no guess

- A forensic output must not hold a byte the tool guessed. Research charter grade 3 says the same.
- Cost: C9 scores drop on streams that are ambiguous. That is the honest number.

## Build change needed

- Replace D-041 option (i) in the salvage writer. Update the `c9_ambiguous` report text, which says "a fixed order chose one". Re-baseline the C9 golden if it moves.

## Not yet decided

- Damage outside streams: only keyword near-misses are detected (D-130). Value damage stays undetected.
- Whether work units count output bytes (D-110). The tree carries option (c).
