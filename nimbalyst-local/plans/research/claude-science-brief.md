# Claude Science brief — PDF repair experiments on the REPDF corpus

Paste everything below the line into a new Claude Science project. Upload
the five notes files from `research_notes/Corrupted PDF repair beyond REPDF/`
and the scripts in `experiments/` alongside it so the agent starts from the
preliminary results instead of redoing them.

---

## Context

I'm building PDFPundit, a forensic tool that repairs corrupted PDFs. It
re-implements and aims to beat REPDF (Forensic Science International: Digital
Investigation, 2026), which defines ten corruption classes:

| Class | Damage |
| --- | --- |
| C1 | header damaged |
| C2 | xref missing |
| C3 | trailer damaged |
| C4 | page tree broken |
| C5 | object tags stripped |
| C6 | font ToUnicode map lost |
| C7 | font program deleted |
| C8 | font resources deleted |
| C9 | Flate (zlib) stream tampered |
| C10 | file truncated at 70% |

REPDF's reported text recovery is about 100% on C1–C5, 99% on C7, 90% on
C6/C8, 60% on C9 and 35–99% on C10. A literature and code review found that
C9, C10 on "Print to PDF" files, and the font classes are where REPDF can be
beaten, and it left open questions that need experiments rather than reading.

The product is pure Rust with no C libraries. Your research tooling does
**not** have that limit: use Python, Tesseract, fontTools, qpdf or anything
else. But every method you recommend for the product must be implementable
in pure Rust. Flag any that aren't.

## Data

- **REPDF corpus:** `git clone https://github.com/dfrc-korea/REPDF`. It has
  1,000 corrupted files plus 100 originals, all made with Microsoft Word via
  "Save As" or "Print to PDF". I found no license file, so don't redistribute
  derived files.
- **Optional, for Q5:** SafeDocs `CC-MAIN-2021-31-PDF-UNTRUNCATED` (about
  7.9M PDFs on AWS) and the matching Common Crawl WARC records.

Split by **source document**, not by corrupted file, into dev / calibration /
held-out test, stratified by corruption class, language and creation method.
Tune only on dev and calibration. Report test results once.

## Questions, in priority order

### Q1. How much of C9 can be repaired exactly, and at what cost?

Preliminary measurements (see `deflate_and_stream_salvage.md`, E1–E8) say
REPDF's C9 is **not** a single flipped byte, as the paper and my design
assume. Each file has 12–30 random-byte replacements. About 67% of them fall
in Flate data, one per stream, and 84% of damaged streams decode fully but
fail only the Adler-32 check.

1. Reproduce that characterization independently. For each claim, give its
   distribution and a confidence interval.
2. Compare these salvage strategies per damaged stream:
   - prefix-only (REPDF-like)
   - keep the full decode on a checksum mismatch
   - a checkpointed search over all 255 replacement values at each candidate
     byte position, with four ways of choosing the positions: the inflate
     error offset, a content-stream/CMap grammar check, TrueType table
     checksums, and a full-range search for streams ≤4 KiB
3. For each strategy, measure:
   - the byte-exact repair rate
   - the fraction of bytes correct
   - the runtime distribution
   - the rate of wrong fixes that still pass Adler-32
   - how the results vary with stream size
4. Carry the byte-level gains through to text recovery on whole files, using
   the metric from Q4.
5. Figures:
   - CDF of how far after the damage the inflate error surfaces
   - repair rate vs. compressed stream size, per strategy
   - runtime CDF
   - text recovery per strategy, next to REPDF's number

The preliminary numbers to confirm or refute are:
- 98.6% exact on inflate-error streams
- 94.5% on grammar-flagged text streams
- 245/245 accepted (243 exact) on Adler-only streams ≤4 KiB
- about 39% of all damaged streams exact, overall

### Q2. Do bundled-font glyph maps work on this corpus at all?

My design decodes glyph IDs through bundled Noto fonts. The research
suggests glyph IDs belong to the exact font that produced the file: Chrome
subsets keep the original IDs, and macOS renumbers them. If that's right,
the Noto approach fails on Word-made files.

1. Inventory the embedded fonts:
   - `BaseFont` names
   - subset prefixes
   - font type (TrueType, CFF or Type0)
   - encoding
   - whether ToUnicode is present
2. Break the inventory down by creation method and language.
3. For the "Save As" and "Print to PDF" originals, test whether content-stream
   glyph codes are the original font's glyph IDs, subset-renumbered IDs, or
   something else.
4. Estimate the share of C6–C8 font slots that each approach could decode:
   Noto glyph maps, glyph maps from the matching system font, and neither.

### Q3. How accurate is glyph-code decipherment, by script and text length?

When no font map can be trusted, treat glyph codes as a substitution cipher.

1. Simulate a lost ToUnicode on the originals, where the answer is known.
2. Measure character-level accuracy of character n-gram models with beam
   search against the number of characters per font slot, for each language:
   en, fr, es, ar, hi, zh.
3. Repeat with the surviving `/Widths` array as an extra constraint.
4. Plot an accuracy-vs-length curve per script.

No published decipherment numbers exist for Arabic, Hindi or Chinese, so
this part is new. Also measure how well widths alone identify the exact font
among a candidate set.

### Q4. Make my metric comparable with REPDF's

REPDF scores recall of OCR words from rendered pages. My harness plans an
LCS-F1 score (Dice on the longest-common-subsequence match, i.e. ROUGE-L F1)
on extracted text.

1. Implement both, plus separate precision and recall and a character-level
   score for Chinese.
2. Run all of them on the same repaired and pristine pairs.
3. Show where the metrics disagree, and recommend what the corpus harness
   should report.

### Q5 (stretch). A real-world truncation benchmark

Common Crawl cut roughly 2M PDFs at 1 MB and later refetched complete copies.

1. Build truncated/complete pairs.
2. Keep only pairs whose first 1 MB hashes match.
3. Characterize what truncation removes: fonts, content streams, images, xref.
4. Compare that with REPDF's synthetic 70% cut.

## How to work

- Fix random seeds and save every script and its environment.
- Keep measured results apart from inferences. Label each number as measured
  here, taken from my notes, or from a paper.
- Report intervals: bootstrap clustered by source document for rates, and
  Wilson intervals for proportions.
- Stop and report if a preliminary number doesn't reproduce. Don't paper
  over it.

## Deliverables

For each question:
- runnable code
- the figures
- a short results table with intervals
- a one-paragraph recommendation for PDFPundit's design, backed by the
  numbers

End with a combined table: class, REPDF's score, the best measured strategy
here, and its score.
