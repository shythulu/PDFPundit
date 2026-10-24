# 0010. Encoder replay checks C9 candidates, using stock zlib

- **Status:** accepted. Timing assumed: in v1, after the ambiguous-output change in [0009](0009-c9-salvage-certainty.md). The owner may move it after v1.
- **Date:** 2026-10-23
- **Decided in:** owner ruling 2026-10-23 (C exception for zlib); research committee 004 D3, reopened by committee 005 C1 and C2; OBS-1100, OBS-1101, OBS-0902, OBS-0903, OBS-0908

## What replay is

1. A C9 fix changes bytes in a stream's compressed data, then decompresses it.
2. Replay re-compresses the decompressed result with the settings the original writer used.
3. If the output matches the file's compressed bytes everywhere outside the damage, the candidate passes replay.

A checksum can pass by luck. Replay checks every compressed byte of the stream, so a lucky pass is far less likely.

## Decision

1. **Replay is an extra filter on C9 survivors.** It runs after Adler-32, on every candidate that survived. It never replaces the search or the other checks.
2. **The encoder is stock zlib, linked statically**, under the one exception in [0004](0004-pure-rust-no-network.md).
3. **Settings calibrate per file.** The engine finds the zlib level, window and memLevel that reproduce the file's own intact streams. Producer labels never supply settings.
4. **No settings, no replay.** If nothing reproduces the intact streams, replay is unavailable for that file. Print to PDF files are in this group today (0 of 400 streams replay).
5. **Replay narrows; it does not prove.** If replay leaves one distinct output and the window was exhausted, the grade is `Exact`. If it leaves several, the result stays `Ambiguous` under [0009](0009-c9-salvage-certainty.md).
6. The report says whether replay ran, with which settings, and how many survivors it removed.

## Why

- Under [0009](0009-c9-salvage-certainty.md), every ambiguous stream stays unrepaired. Replay turns some of those into certain fixes.
- Per-file calibration works without labels: it reproduced 819 of 819 intact Save As streams (OBS-1101).
- Replay alone is not enough. In OBS-0903, 3 candidates passed inflate, Adler-32 and replay, and only a grammar check separated them.

## Rejected

| Option | Why not |
|---|---|
| zlib-rs, miniz_oxide | Reproduce 0 and 5 of 400 streams (OBS-0902) |
| zlib-bitexact-rs, patched | 400 of 400, but needs a fork. Only `deflate_slow` is ported, and the public API is fixed at level 9 / memLevel 8. |
| preflate-rs | 395 of 400, 14 panics with forced settings |
| zlib-ng | Its output differs from stock zlib |
| Producer profiles supplying settings | Calibration from the file itself is exact and needs no label |

## Revisit if

- A maintained pure-Rust encoder reproduces stock zlib byte for byte. Then drop the C exception.
- Print to PDF streams need a replay route. preflate-rs keeps a damage signal there, its correction size, but it was never tested as a check.
