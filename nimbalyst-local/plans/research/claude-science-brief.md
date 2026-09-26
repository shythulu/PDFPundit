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

REPDF's reported text recovery is about 100% on C1–C5, 99% on C7, 88–99% on
C6/C8, 59–63% on C9, and on C10 99.6% for "Save As" but only 30–35% for
"Print to PDF". Its Table 2 and Table 5 disagree slightly, so don't quote a
single number: re-run the metric. A concurrent paper, CPR (Kim et al., FSI:DI
56, 2026, same DFRWS issue), is the only direct rival. A literature and code
review found that C9, C10 on "Print to PDF" files, and the font classes are
where REPDF can be beaten. It left open questions that need experiments
rather than reading.

The product is pure Rust with no C libraries. Your research tooling does
**not** have that limit: use Python, Tesseract, fontTools, qpdf or anything
else. But every method you recommend for the product must be implementable
in pure Rust. Flag any that aren't.

## Data

- **REPDF corpus:** `git clone https://github.com/dfrc-korea/REPDF`. It
  is built from 50 Word documents × 10 corruption classes × 2 creation
  methods ("Save As" and "Print to PDF"): 1,000 corrupted files and 100
  originals. Each six-page document repeats the same text in English,
  Chinese, Hindi, Spanish, French and Arabic, one language per page. REPDF
  scores text as word recall against Google Document AI OCR of the rendered
  original. I found no license file, so don't redistribute derived files.
- **Optional, for Q5:** SafeDocs `CC-MAIN-2021-31-PDF-UNTRUNCATED` (about
  7.9M PDFs on AWS) and the matching Common Crawl WARC records.

Split by **source document**, not by corrupted file, into dev / calibration /
held-out test, stratified by corruption class, language and creation method.
Tune only on dev and calibration. Report test results once. With only 50
base documents, a 60/20/20 split leaves about 10 in the test set, so expect
wide intervals and say so.

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

Three pieces of the preliminary work have no script, so rebuild them as
scripts:
- The step that extracted per-stream cases (`<id>.z`, `<id>.orig`,
  `<id>.meta`) for `mzbench`.
- The TrueType table-checksum localizer. The first bad table contained the
  damage in 765 of 766 fonts, but the search cost using it was never
  measured. Fonts are 798 of the 1,220 streams that fail only Adler-32, so
  this decides whether the 39% can reach the estimated 60–70%.
- The C10 census. It found that 92 of the 97 streams crossing the 70% cut
  are fonts, and that "Print to PDF" files lose 35.6% of text content bytes.

### Q2. Do bundled-font glyph maps work on this corpus at all?

My design decodes glyph IDs through bundled Noto fonts. The research
suggests glyph IDs belong to the exact font that produced the file: Chrome
subsets keep the original IDs, and macOS renumbers them. If that's right,
the Noto approach fails on Word-made files.

An ad hoc scan found 44 distinct `/BaseFont` names in the "Save As"
originals, mostly Google Fonts plus Cambria, and only two Noto faces. The
"Print to PDF" files name every font `CIDFont+F1`–`F22`. The same scan found
that every "Save As" original uses object streams, but no page or font
dictionary sits inside one. Confirm all of this with a committed script,
then:

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

REPDF scores recall of OCR words from rendered pages, using Google Document
AI. My harness plans an LCS-F1 score (Dice on the longest-common-subsequence
match, i.e. ROUGE-L F1) on extracted text.

1. Implement both, plus separate precision and recall and a character-level
   score for Chinese. Use Document AI if you can, otherwise another OCR
   engine, and say which one you used.
2. Run all of them on the same repaired and pristine pairs.
3. Show where the metrics disagree, and recommend what the corpus harness
   should report.

### Q5 (stretch). Test sets beyond REPDF's generator

The C9 finding shows how easily a tool overfits one corpus generator. Build
two test sets to measure that.

**A real-world truncation benchmark.** Common Crawl cut roughly 2M PDFs at
1 MB and later refetched complete copies (CC-MAIN-2021-31-PDF-UNTRUNCATED).

1. Build truncated/complete pairs.
2. Keep only pairs whose first 1 MB hashes match.
3. Characterize what truncation removes: fonts, content streams, images, xref.
4. Compare that with REPDF's synthetic 70% cut.

**A held-out corruptor family**, applied to the REPDF originals:
- multi-byte bursts and zeroed sectors
- variable truncation points
- shifted rather than deleted xref offsets
- partial `/Kids` damage
- page and font dictionaries moved inside object streams, which the corpus
  never tests

Report recovery on REPDF's own corruptors and on this held-out family side
by side; the gap is the overfitting measure.

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
