# 0015. Font recovery: the document's own fonts first; no text beats wrong text

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-010, D-154, D-035, D-040, D-084, D-122, D-144, D-146, D-156, D-157, D-158; GitHub issues #3, #5, #13; research committee 004 D5 and D6, GAP-201

## Decision

### Decoding ladder, in order
1. `/ToUnicode`.
2. `/Encoding` and `/Differences` with the Adobe Glyph List.
3. The font program's own reverse cmap and `post` names.
4. Only then, inference against the bundled font database.

### Identity
5. A font's identity comes first from its embedded `name` table, read with read-fonts/skrifa. All 889 page-font objects in the 50 Print to PDF originals match (OBS-0907).

### Output substitute
6. When a font program is lost (C7, C8), the substitute is bundled, unhinted Noto Sans or Noto Serif Regular.
7. The font index names 24 more OFL families found in the corpus. They are matched by name only. Their programs are not shipped.
8. The substitute carries the input's `/W` widths (D-156, flagged to the owner).

### Honesty
9. **No text is better than wrong text.** On a weak C8 guess, `UseBest` recovers text through a rebuilt `/ToUnicode` without substituting the font (`TextOnly`).
10. A font never embedded is an Info `FontNotEmbedded`, not C7/C8. A descriptor blanked with a run of 15 or more 0x20 bytes is C7/C8.
11. AGL heuristics carry `HEURISTICS_VERSION`.

### Pins
12. Font crates follow hayro-interpret 0.8.0: skrifa 0.46.2, read-fonts 0.43.3, `harfrust =0.13.3`, one copy each. fontique waits for M5.

## Why

- Only 2 of the corpus's 44 fonts are Noto. Inference from the bundled database alone cannot reach the paper's numbers.
- Dictionary and word-frequency scoring is a language prior. It can turn damage into plausible but invented text (GAP-201). So it ranks candidates; it never creates text.
- Decipherment without width evidence gets digits right 6.5% of the time (OBS-0906).

## Rejected

- "Inference only from the bundled database" (TD :457).
- System-font substitution through fontique, until M5.
- FrequencyWords dictionaries (CC BY-SA). Leipzig lists replace them. Their licence is settled in [0016](0016-generated-assets-committed.md): CC BY-NC.

## Open

- No fabrication metric exists yet for dictionary-ranked picks ([0019](0019-metrics.md)).
