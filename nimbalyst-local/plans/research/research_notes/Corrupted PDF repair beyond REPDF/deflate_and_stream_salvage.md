# DEFLATE / zlib stream salvage and damaged-image recovery for PDFPundit's C9/C10 ladder

Evidence key. Published sources are cited inline with links. Items marked **E1–E8** are **local measurements made for this note** (not published results), run on the public REPDF dataset (https://github.com/dfrc-korea/REPDF, 50 originals × 2 creation methods; the 100 `*_stream_zlib.pdf` C9 files and 100 `*_partial_cut.pdf` C10 files), using Python 3 + zlib 1.3 and a Rust 1.94 release build of `miniz_oxide` 0.8.9 on one core of an "Intel Xeon @ 2.80 GHz" container (4 vCPU). Scripts lived in the session scratchpad (`c9exp.py`, `grammar_loc.py`, `blocks.py`, `mzbench/src/main.rs`). They were not committed.

- **E1**: byte diff of each C9 file against its original, mapping each modified byte to the zlib stream (if any) that contains it.
- **E2**: byte-fed inflate of every damaged Flate stream. Records the error type, the error offset k, the distance k − (true offset), prefix-correct bytes, and prefix+suffix-correct bytes ("keep-all" proxy).
- **E3**: synthetic test. 1,500 random single-byte replacements at uniformly random offsets in 3,396 Flate streams (≤300 KB) taken from the 100 originals.
- **E4**: `miniz_oxide` search that substitutes each of the 255 other values at each candidate byte. It checkpoints decoder state every 256 input bytes, resumes from the nearest checkpoint, and accepts on `TINFLStatus::Done` (Adler-32 verified).
- **E5**: grammar localizer. A simple PDF-content/CMap tokenizer flags the first ungrammatical token, and the flag is mapped back to an input offset through a byte-fed inflate trace.
- **E6**: TrueType table-checksum localizer on damaged `FontFile2` streams.
- **E7**: census of DEFLATE block counts and block types in the corpus streams (custom block walker).
- **E8**: census of which streams lie before, across, or after the 70% truncation point in C10 files.

## Q1. Which published methods recover data after a corrupted point in a DEFLATE stream (including decompression from an arbitrary mid-stream offset with an unknown 32 KiB window), and what recovery rates did they report?

### Takeaway
The literature covers four kinds of method:
1. **Skip-and-restart**: gzrt/gzrecover, Park et al. 2008, pugz/rapidgzip block finders.
2. **Intra-block resynchronization with the already-known Huffman tables, plus reconstruction of unknown bytes with a language model**: Brown's ZipRec, DFRWS 2011/2013. This is the only published method that recovers the rest of the *same* block after corruption. On 128–4096-byte corrupted segments, its output differs from the original by only ≈1.5× the bytes the corruption represents.
3. **Oracle-checked brute force of bit flips**: Barral et al. 2022. It repaired 172 of 204 corrupted fragments with 1-bit search and raised recovered data from 8.32% to 99.68%.
4. **Language-model-guided fault-tolerant decoding for plain Huffman/LZSS**: Wang et al. 2018/2019. This is not DEFLATE.

REPDF itself only keeps the prefix before the error ("byte-by-byte" decompression) and scores 60.11% text on C9.

### Cited Findings
- **Brown 2011 (DFRWS 2011, ZipRec 0.9)** finds a synchronization point in a DEFLATE bitstream whose beginning is unknown or damaged. Decompressing forward from it yields a mix of literals and unknown bytes. — [Brown 2011, ACM DL abstract](https://dl.acm.org/doi/10.1016/j.diin.2011.05.015)
  - Timings from the 2011 slides. On a novel: unzip 30 ms, ZipRec *recover* 290 ms, ZipRec *reconstruct* 58–69 s. On ZipRec's own source code: unzip 105 ms, recover 795 ms, reconstruct 24 s. Scanning a disk image: about 2 min/GB. — [Brown 2011 slides](https://dfrws.org/sites/default/files/session-files/2011_USA_pres-reconstructing_corrupt_deflated_files.pdf)
  - The 2011 reconstruction took "200 or more times as long as extraction, e.g. 247 s for a five-megabyte file". — [Brown 2013, ScienceDirect](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- **Brown 2013 (DFRWS 2013, ZipRec 1.0), intra-packet recovery.**
  - Mechanism. When corruption is mid-packet, "the most important" surviving information is the two Huffman trees. With the trees known, the bitstream can be segmented from any symbol boundary. Because "a Huffman bitstring may be up to 48 bits long", there are 48 candidate restart points. All 48 are decoded incrementally until they converge on the same bit boundary, and "any further decompression will be correct". Recovery then decodes up to the corruption, skips to the resync point, clears the history window to unknowns, and continues. — [Brown 2013 PDF](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
  - Scope. It handles "a relatively short corrupt section (up to 4–8 kilobytes, depending on compression ratio)". It also realigns the history window across the gap by scoring the net byte matches of every possible shift. — [same](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
  - Results. Corrupted segments of 128–4096 bytes produce reconstructed output that differs from the original by "less than twice the number of bytes represented by the corrupted segment". The mean ratio of incorrect output bytes to corrupted uncompressed bytes was 1.44–1.65 and the median 1.36–1.48 across 21 Europarl languages (Table 4). English examples (Table 2): 128 compressed bytes corrupted → 371 uncompressed bytes affected → 409 incorrect (ratio 1.10); 2048 → 5411 → 7390 (ratio 1.37). In 6 of 126 cases the output differed by *fewer* bytes than the corruption represented. — [same](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
  - Reconstruction. Forward and backward byte 7/8-gram models reconstructed on average 86.61% of unknown bytes, with 95.5% of those correct. That is +8.9% absolute in reconstructed bytes and +20.9% absolute in correctly reconstructed bytes versus 2011, at one-twelfth the run time. — [same](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
  - Resync loss. Waiting for all segmentations to converge discards a median of "nearly 50" useful bytes. Brown suggests this could be cut to at most 6. — [same](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
  - Brown also tried inferring the Huffman trees of a packet whose beginning is missing, via a reverse search from the end of the packet. The paper reports this as an attempt, with no success figure. — [ScienceDirect](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- **Park et al. 2008 (MUE 2008).**
  - Their tool rescues DEFLATE data "even though the header block is missing or corrupted". — [IEEE Xplore abstract](https://ieeexplore.ieee.org/document/4505751)
  - Brown describes it as a "bit-by-bit scan for decompressable DEFLATE packets". — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
  - Barral et al. describe it as dropping "prefixes of unrepairable data until the corrupted area does not influence the rest of the file, yielding several chunks of uncorrupted data". — [Barral et al. 2022, arXiv](https://arxiv.org/pdf/2210.00856)
- **gzrt / gzrecover (Renn; v0.8, 2013).**
  - It "attempts to skip over corrupted portions … by scanning byte-by-byte until the zlib decompression code can successfully decompress the stream starting at that point". This "requires the periodic insertion of a zlib-style full synchronization point". — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
  - The author warns that it "gets faked out and is likely to 'recover' some bad data". — [gzrt page](https://www.urbanophile.com/arenn/hacking/gzrt/gzrt.html)
- **pugz (Kerbiriou & Chikhi 2019).**
  - Two-stage decompression from an arbitrary block with an "undetermined" 32 KB context. Its block finder tries every bit position with strict early-fail checks and finds the next block start in about 100–300 ms. — [arXiv 1905.07224](https://arxiv.org/pdf/1905.07224)
  - It accepts only byte values 9–126 and blocks of 1 KiB–4 MiB. — [Rapidgzip paper](https://arxiv.org/pdf/2308.08955v1)
  - On random DNA at the default level, undetermined characters vanish after about 540 KB. At high levels, "long chains of back-references" keep many characters undetermined. Guessing them "could be possible, but we did not yet explore this direction". Speeds (MB/s of compressed input): gunzip 37, libdeflate 118, pugz with 32 threads 611. — [arXiv 1905.07224](https://arxiv.org/pdf/1905.07224)
- **rapidgzip (Knespel & Brunst, HPDC 2023).**
  - It fills the unknown window with unique 15-bit markers and decodes into 16-bit intermediate symbols. Markers are resolved in a second pass. — [arXiv 2308.08955](https://arxiv.org/pdf/2308.08955v1)
  - The dynamic-block finder checks four things: final bit = 0, block type = 01₂ (the dynamic-Huffman code), literal/length-count field ≠ 30/31, and a valid, efficient precode.
  - The stored ("Non-Compressed") block finder produced one false positive per 514 ± 23 KiB of random data.
  - Fixed-Huffman blocks are not searched, because only 3 header bits can be checked.
  - The paper notes that the fast block finder "also improves the speed for the recovery of corrupted gzip files". — [same](https://arxiv.org/pdf/2308.08955v1)
- **Barral et al. 2022 (FSI:DI, Google Home).**
  - They treat Adler-32 as an oracle over bit-flip "repair candidates" in 204 corrupted zlib fragments (≤128 KiB output). The 1-bit-flip pass took 73 min on an i7-8700 (6c/12t). 172 fragments had exactly one valid candidate, 3 had several, and 29 needed 2 bit flips.
  - The 2-bit-flip search took from 22 days to more than a year per fragment ("proportional to the cube of compressed fragment's size") and was spread over 40 machines.
  - Multiple candidates were merged with three-valued logic.
  - Recovered data rose from 8.32% to 99.68% at bit level. — [arXiv 2210.00856](https://arxiv.org/pdf/2210.00856)
- **Wang, Zhao & Sun 2018.** Fault-tolerant decompression (FTD) of plain static-Huffman text files, using Huffman-coding and grammar priors plus a heuristic search, reached a "correction rate of 96.84% … when the source prior information is accurate". — [Springer](https://link.springer.com/article/10.1007/s11277-018-5277-5)
- **Wang, Peng & Tang 2019.** LZSS repair that works "by using the residual redundancy left by the encoder to carry the check information". It needs a cooperating encoder. — [JEIT](https://jeit.ac.cn/en/article/doi/10.11999/JEIT180942?viewType=HTML)
- **REPDF (2026).** It uses "byte-level decompression, checking for errors byte-by-byte to prevent total data loss". C9 average text recovery was 60.11%. — [REPDF paper, local copy `nimbalyst-local/plans/REPDF-…pdf`; DOI](https://doi.org/10.1016/j.fsidi.2026.302061)
  - The per-language range was 49.94–71.69%.
  - C9 image recovery/rendering: 80%/60% ('Save As') and 60%/40% ('Print to PDF').
  - PDF24 showed "exceptional strength specifically for C9". — [same](https://doi.org/10.1016/j.fsidi.2026.302061)

### Inferences
- Only Brown's method recovers data from the *same* block after the damage without a block boundary. E7 (Q3) shows that 94% of PDF content streams and 100% of CMaps in the corpus are a single DEFLATE block. For PDF streams, Brown-style resync is therefore the only resync that yields anything.
- For a *single* modified byte, exact repair (Barral-style oracle search) dominates resync. It reproduces the stream byte-exactly, whereas resync loses ~50+ bytes at the resync point and leaves co-indexed unknowns.

### Gaps
- No published recovery-rate evaluation on PDF streams specifically, other than REPDF's text/image scores.
- Park et al. 2008 full text (and any rates it reports) was not accessible.
- Did not check whether ZipRec (SourceForge, GPL) is maintained, or how it performs on binary (non-text) data.

## Q2. How reliable is locating the error? How far after the corrupted byte does inflate fail, is [k − 4 KiB, k + 64] the right window, and are there better localizers?

### Takeaway
REPDF's C9 is **not a bit flip**. It replaces a random byte in about 42% of each file's Flate streams (~14.5 streams and 21.6 bytes per file), with about 97% of replacements differing in more than one bit.

For those damaged streams, inflate **fails early in only ~15%** of cases. In those cases it fails soon after the damage: a median of 5 bytes later, and within the planned window 99.1% of the time. The other **~84% decode to the end and fail only the Adler-32 check**, so k = end of stream and the planned window is wrong.

Two domain-specific localizers work well:
- **PDF content/CMap grammar**: the first ungrammatical token lands within 64 input bytes after the damage in 92% of flagged cases.
- **TrueType per-table checksums**: the first bad table contains the damage in 765 of 766 cases.

### Cited Findings
- **REPDF's own definition of C9:**
  - Paper: "realized by modifying a random byte within a zlib-compressed stream". — [REPDF paper §5.1.9](https://doi.org/10.1016/j.fsidi.2026.302061)
  - Dataset README: "corrupts compressed stream data by modifying a random byte within a zlib-compressed stream". — [REPDF GitHub](https://github.com/dfrc-korea/REPDF)
- **E1: what the C9 files actually contain.** [dataset](https://github.com/dfrc-korea/REPDF)
  - The 100 C9 files differ from their originals in **12–30 bytes each (mean 21.6)**, with the file length unchanged.
  - 1,445 of 2,162 modified bytes (67%) are inside Flate data. Each damaged stream has exactly one modified byte. That is 7–26 damaged Flate streams per file (mean 14.45) out of ~34 Flate streams.
  - The rest (~33%) fall outside Flate data: in dictionaries or object headers (e.g. `/Length` → `\x1cength`, `115 0 obj` → `11l 0 obj`, `/StemV 55` → `/StemV 5\xc0`, widths arrays), in uncompressed XMP or CMap text, and in DCT image data (~17 bytes).
- **E1: Hamming weight of the modifications.** The distribution is 1: 68, 2: 253, 3: 509, 4: 582, 5: 456, 6: 220, 7: 66, 8: 8. **Only 3.1% are single-bit flips**, which is consistent with uniformly random replacement (8/255 = 3.1%). — [E1 on REPDF dataset](https://github.com/dfrc-korea/REPDF)
- **E2: damaged-stream types and inflate outcomes** on the 1,445 damaged Flate streams. [dataset](https://github.com/dfrc-korea/REPDF)
  - Stream types: 854 fonts, 229 text content, 87 graphics content, 175 CMaps, 69 other text, 31 binary.
  - Outcomes: **1,220 (84.4%) decode to the end and fail only the Adler-32 check**; 221 (15.3%) raise an inflate error; 4 hit a truncated ("needs more input") end.
  - Error messages: "invalid distance too far back" 183, literal/length or code-length set errors 28, "missing end-of-block" 7, invalid literal/length code 1, bad block type 1, bad zlib header 1.
- **E2: distance from damage to error (inflate-error cases).**
  - Quantiles of (error offset k − true offset): p10 = 1, p25 = 3, median 5, p75 16, p90 68, p95 675, p99 3,025, max 6,623 bytes.
  - 62% surface within 8 bytes, 89.6% within 64, 95.5% within 1 KiB.
  - **99.1% fall inside [k − 4096, k + 64]**, and none surface before the damage. — [E2](https://github.com/dfrc-korea/REPDF)
- **E3: synthetic replication** (1,500 random byte replacements). [dataset](https://github.com/dfrc-korea/REPDF)
  - Outcomes: 80.9% Adler-only, 18.3% inflate error, 0.7% truncated end, 0.1% no effect.
  - Error distance: median 5, p90 91, p95 303, p99 1,364, max 6,845 bytes. 99.6% fall inside [k − 4096, k + 64].
  - **Detection depends on how much history exists.** The early-error rate is 44.5% when the damage lands in the first 4 KiB of output, 10.9% in 4–32 KiB, and **0.5% beyond 32 KiB**. Most detections are "invalid distance too far back", which can only fire while history is shorter than 32 KiB.
  - Early-error rate by type: fonts 7.3%, content 29.7%, CMaps 41.8%.
- **Independent confirmation (Barral et al.).** For single bit flips in 204 zlib fragments of ≤128 KiB, restricting the search to the bytes consumed before the zlib error "significantly cuts the search interval for 5 out of 204 fragments, while for all others, the full input is read". — [Barral et al. 2022](https://arxiv.org/pdf/2210.00856)
- **Output left behind in Adler-only streams (E2).** [dataset](https://github.com/dfrc-korea/REPDF)
  - 31.6% keep the original decoded length. For those, the median number of wrong bytes is 5 (p10 = 1, p75 = 18, p90 = 70) and the median fraction of correct bytes is 99.99%.
  - Across all damaged streams, the mean correct prefix is 48.1% of the decoded bytes; prefix plus common suffix gives 59.0% (Adler-only streams: 53.6% → 66.6%).
- **Localizing the damage from the output is hard in general.**
  - ZipRec 1.0 "only detects corruption automatically where there is a sequence of at least 128 consecutive identical bytes".
  - Language-model detectors showed "a drop in scores at the point of corruption, [but] the natural variation in scores is larger than the drop".
  - A word-unigram detector declares corruption when more than one-third of the words in a 512-byte block are out of vocabulary. — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- **Why decoding resynchronizes silently.** Some binary Huffman codes contain a codeword that resynchronizes the decoder whatever slippage preceded it, so they are "self-synchronizing in a probabilistic sense" (IEEE Trans. IT correspondence, Ferguson & Rabinowitz 1984). — [IEEE](https://ieeexplore.ieee.org/document/1056931)
  - Brown's 48-candidate convergence relies on the same property: segmentations that reach the same bit position segment identically from then on. — [Brown 2013](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
- **E5: grammar localizer** on the 403 Adler-only content, CMap and other-text streams. [E5](https://github.com/dfrc-korea/REPDF)
  - The originals all pass the tokenizer. In the damaged versions, 343 (85%) contain a grammar violation and 60 (15%) look grammatical.
  - In all 343 flagged cases the true damaged byte lies at or before the flag's input offset (+8 bytes slack).
  - Distance from true offset to flag: median 1, p75 6, p90 34, p95 251 bytes.
  - The true offset lies within [flag − 64, flag + 8] in 91.8% of flagged cases, within 256 in 94.8%, within 1,024 in 95.9%, and within 4,096 in 98.8%.
- **E6: TrueType checksum localizer.**
  - In 854 original embedded TrueType fonts, **all 12,532 tables have valid directory checksums**.
  - In the 798 Adler-only damaged fonts, the first mismatching table (in file order) contains the start of the damage in **765 of 766** checkable cases. Six had damage in the table directory; in 26 no table mismatched, because the damage fell in padding.
  - The first bad table averages 36.9% of the font (median 35.5%). — [E6](https://github.com/dfrc-korea/REPDF)
  - The checksum rule (sum of big-endian uint32 per table, `head.checkSumAdjustment` zeroed for the head table): [OpenType spec 1.8.1](https://learn.microsoft.com/en-us/typography/opentype/otspec181/otff).
  - WOFF tools "MUST validate these checksums". — [W3C WOFF](http://www.w3.org/submissions/WOFF)
- **Adler-32 is weak on short data.**
  - RFC 3309: "For small packets Adler-32 provides weak detection of errors … for very short packets, Adler32 is guaranteed to give poor coverage of the available bits". — [RFC 3309](https://www.rfc-editor.org/rfc/rfc3309)
  - Barral et al. found 3 of 204 fragments with several Adler-valid 1-bit candidates. — [Barral et al.](https://arxiv.org/pdf/2210.00856)
  - E4 false accepts: in the full-range search over 245 Adler-only streams ≤4 KiB compressed, two accepted candidates were not byte-exact. One was an artifact: changing a byte of the stored Adler trailer made it match. One was a genuine Adler collision on a 2.7 KB output. The grammar-localized search had 1 non-exact accept out of 324. — [E4](https://github.com/dfrc-korea/REPDF)

### Inferences
- **The [k − 4096, k + 64] window is right for the 15% of inflate-error streams** (99.1–99.6% coverage).
  - Scan it *backwards from k*: the median damage lies 5 bytes before k.
  - k + 8 is enough slack past k (bit-buffer lookahead). k + 64 wastes candidates.
- **The window is wrong for the 84% Adler-only majority.** There k is the stream end, and the damage is spread across the whole stream.
- **8 single-bit flips per byte cover only ~3% of REPDF's C9 modifications.** Combined with the window problem, the planned step 3 would exactly repair roughly 3% of damaged streams (the bit-flip cases among the inflate-error and small Adler-only streams).
- **The step-3 acceptance clause "or classifies as valid content" would be a false-accept generator.** Among ~10⁶ candidates, many decode to plausible content. Acceptance should require Adler-32. Grammar or checksums should only rank candidates or break ties.
- Localizer ranking for PDF streams:
  1. Inflate error offset (when present).
  2. Content/CMap grammar (text streams).
  3. TrueType table checksums (FontFile2). A finer glyph-level localizer through `loca`/`glyf` structure checks is plausible but untested.
  4. For short streams, no localizer is needed (full search).
- **Speculative, untested idea (Adler-syndrome localization).** Adler-32 is a pair of modular sums. When the damage changes a single output byte, (ΔA, ΔB) determines the position and delta when output ≤ 65,521 bytes. Only a minority of Adler-only cases qualify (p10 of wrong bytes = 1).

### Gaps
- REPDF's corruptor code is not in the repository (only the README and PDFs), so the exact rule that also hits non-Flate bytes is inferred from diffs.
- No published measurement of DEFLATE error-surfacing distance was found; E2/E3 are the only numbers here.
- The grammar localizer used a minimal tokenizer without per-operator operand-count checks. A fuller grammar would likely flag more of the 15% unflagged cases (untested).

## Q3. What works beyond single-byte changes (multi-byte corruption, insertions/deletions, truncation)? Is there evidence for language-model-guided search, SAT/constraint solving, or Huffman-table repair?

### Takeaway
Multi-byte damage (two or more bytes) is where brute force collapses: 2-bit-flip search took from 22 days to over a year per fragment. There, published practice switches to **resync plus language-model reconstruction** (Brown), which is demonstrated for 128–4096-byte zeroed spans.

No published SAT/constraint-solver or LLM-guided repair of DEFLATE was found. Huffman-table repair for missing headers was attempted by Brown, with no reported success.

In PDFs, **block-boundary resync is nearly useless**: 94% of content streams, 100% of CMaps and 45% of fonts in the corpus are one block, and only 1 of 3,398 streams contains a stored block. Truncation (C10) needs no DEFLATE cleverness, because the decodable prefix always survives.

### Cited Findings
- Brown's recovery tolerates corrupted spans of 128–4096 compressed bytes, "up to 4–8 kilobytes, depending on compression ratio". Beyond 4096 bytes was not tested, because 4096 compressed bytes "already represent half or more of the 32,768 byte history window" for some languages. — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- Barral et al. 2-bit-flip search: 22 days for the smallest fragment and more than a year for the largest on one machine, with cost proportional to the cube of the fragment size. Some fragments ended with 1,354–40,717 Adler-valid candidates, merged by three-valued logic. — [arXiv 2210.00856](https://arxiv.org/pdf/2210.00856)
- On stream compressors, "a bitflip corrupts the whole subsequent stream". For those, Barral et al. use greedy backward bit-flip search maximizing the successfully decompressed length, but it "requires significant manual intervention to get the algorithm out of local minima". — [arXiv 2210.00856](https://arxiv.org/pdf/2210.00856)
- Language-model-guided fault-tolerant decoding (Wang et al. 2018, 96.84% correction) is demonstrated only on static-Huffman natural-language files. — [Springer](https://link.springer.com/article/10.1007/s11277-018-5277-5)
  - Its successor, the LZSS repair, needs encoder-side redundancy. — [JEIT 2019](https://jeit.ac.cn/en/article/doi/10.11999/JEIT180942?viewType=HTML)
- Brown "attempted to infer the Huffman trees used by a DEFLATE packet missing its beginning … by performing a reverse search from the end of the packet", with no results reported. — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- **E7: block census** of 3,398 Flate streams ≤300 KB from the originals, as blocks per stream. [E7](https://github.com/dfrc-korea/REPDF)

  | Stream type | Streams | 1 block | 2 blocks | 3 blocks | 4 blocks | 5+ blocks |
  | --- | --- | --- | --- | --- | --- | --- |
  | Content (text) | 578 | 543 | 2 | 33 | 0 | 0 |
  | Content (graphics) | 181 | 24 | 34 | 123 | 0 | 0 |
  | CMap | 618 | 618 | 0 | 0 | 0 | 0 |
  | Other text | 156 | 156 | 0 | 0 | 0 | 0 |
  | Font | 1,806 | 806 | 545 | 282 | 39 | 134 |

  Only **1 stream contained a stored block** and 22 a fixed-Huffman block.
- Stored-block finders see about one false positive per ~514 KiB of random data even with extra checks. — [Rapidgzip](https://arxiv.org/pdf/2308.08955v1)
- **E2: header damage is fixable too.** About 28 of the 221 inflate-error streams failed in the dynamic-header code-length tables ("invalid literal/lengths set", "invalid code lengths set", "invalid bit length repeat", "invalid distances set"). The 255-value search in E4 still fixed all but 3 of the 221. — [E2/E4](https://github.com/dfrc-korea/REPDF)

### Inferences
- **Ladder step 4 ("resync to a later stored-block boundary") should be dropped or demoted.** Measured coverage on the corpus is effectively zero (1 in 3,398 streams contains a stored block).
- **The useful resync is Brown's intra-block resync with known tables**, followed by unknown-window markers. It matters only where exact repair fails: the few budget timeouts, Adler-only streams with no localizer, and real-world multi-byte damage.
- **Insertions/deletions (byte-count changes)** are equivalent to a bit-position shift for everything after the damage. The same 48-candidate convergence handles them because Huffman segmentation self-synchronizes. This is an inference; no DEFLATE-specific insertion/deletion study was found.
- For multi-byte damage in PDF content streams, the most promising (speculative) search is a **beam search over (position, value) edits scored by the PDF operator grammar, with Adler-32 as the final oracle**. It is a structured version of Wang's language-model-guided decoding. The measured grammar signal from E5 supports its use as a scorer.

### Gaps
- No published SAT/SMT or constraint formulation of DEFLATE repair was found.
- No published LLM- or neural-guided DEFLATE repair was found as of Sept 2026 (searched; nothing surfaced).
- No published study of byte insertion/deletion inside DEFLATE streams was found.

## Q4. After resync with unknown back-references, how can the missing window bytes be filled for PDF content streams?

### Takeaway
The demonstrated method is Brown's: co-indexed unknown bytes (every copy of the same unknown source byte shares one index) filled by forward+backward byte n-gram models. It reconstructed 86.6% of unknowns with 95.5% precision on natural-language text.

PDF content streams are far more constrained than prose (operators, operand counts, font code spaces). That should make gap filling *easier*, but no published PDF-specific method exists. Such constraint-based filling is a design opportunity, not demonstrated practice.

### Cited Findings
- Every unknown byte carries a co-index from copying out of a single unknown source instance, "thus, certain sets of bytes are known to be identical, which combined with contextual clues from language models permits inference of their values". — [Brown 2013](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
- Brown's decision rule assigns a value to a co-index only if its score is at least twice the second best. The results were 86.61% of unknown bytes reconstructed and 95.50% of those correct, averaged over 21 languages. — [same](https://dfrws.org/sites/default/files/session-files/2013_USA_paper-improved_recovery_and_reconstruction_of_deflated_files.pdf)
- Once decompression runs past the unknown window, literals progressively replace unknowns: "if sufficiently many literals are produced … any undetermined character no longer get back-referenced". High compression levels keep long back-reference chains alive. — [pugz](https://arxiv.org/pdf/1905.07224)
- rapidgzip-style markers (a 15-bit window offset per unknown byte) are resolved in the second pass once the window is known, at 1,254 MB/s for marker replacement. — [Rapidgzip](https://arxiv.org/pdf/2308.08955v1)
- **E5**: 85% of damaged content/CMap streams produce a grammar violation near the damage, confirming that PDF content syntax is a strong discriminator for candidate text. — [E5](https://github.com/dfrc-korea/REPDF)

### Inferences
Speculative, PDF-specific ways to fill co-indexed unknowns:
1. **Operator grammar.** Tokens must be numbers, names, strings or operators from the ~73-operator set, and operand counts are fixed per operator. For example, `Tf` takes a name and a number.
2. **Font code-space constraints.** Bytes inside `(...)`/`<...>` strings shown with a font must be valid codes for that font: inside its ToUnicode/CMap codespace, with a /Widths entry, and for Identity-H CIDs below the glyph count.
3. **Cross-stream redundancy.** Other pages built with the same producer share headers, footers, graphics-state prologues and repeated `BT … Tf … Td` skeletons. These can feed a per-document n-gram model, the analogue of Brown's trained language model.
4. **Parallel text.** `/ActualText` and structure-tree `/Alt` text, bookmarks and XMP carry duplicate strings that can confirm reconstructed spans.

For REPDF's single-byte C9, exact repair (Q2/Q6) makes gap filling mostly unnecessary. Gap filling matters for residual failures and for real-world multi-byte damage.

### Gaps
- No published work applies window-gap filling to PDF content streams, CMaps or fonts; everything above is untested design reasoning.
- Brown's language models were trained on natural text. Their performance on PDF operator streams (mostly ASCII numbers and operators) is unknown.

## Q5. For image streams (DCT JPEG, JPX JPEG 2000, Flate raster), which partial-recovery techniques exist, and what can salvage images in truncated PDFs?

### Takeaway
- **JPEG**: the entropy-coded data resynchronizes at restart markers (RST0–7) when present. After a byte error, decoders usually keep going with local block damage and colour shifts from DC-difference drift. A truncated baseline JPEG decodes top-down, with the missing area filled with gray. Carving research adds pseudo-header synthesis for headerless fragments.
- **JPEG 2000**: codestreams are built for truncation. OpenJPEG decodes partial bitstreams when strict mode is off.
- **Flate rasters**: the same inflate salvage as text applies. With PNG predictors, each row starts with a filter byte in 0–4, a useful resync and consistency check (not tested here).

In the corpus, C10 loses whole images rather than cutting through them. In 'Print to PDF' files, 45.7% of DCT image bytes lie beyond the 70% cut and no image straddles it.

### Cited Findings
- **REPDF on C9 images.** It "C9 affects the image data or the content stream that renders images … the image data often remain intact, whereas the rendering operators in the content stream are corrupted. In some files, images remained structurally intact and visually recognizable despite substantial color corruption." — [REPDF §5.5](https://doi.org/10.1016/j.fsidi.2026.302061)
  - Recovery/rendering: C9 80/60 ('Save As') and 60/40 ('Print'); C10 100/100 ('Save As') and 55/50 ('Print'). — [REPDF Table 3](https://doi.org/10.1016/j.fsidi.2026.302061)
- **Sencar & Memon 2009 (DFRWS).**
  - Restart markers FFD0–FFD7 repeat cyclically. At each one, "DC difference is reset to zero and the bitstream is synchronized to a byte boundary". After an error the decoder can compute the number of skipped MCUs from the marker sequence and resume at the right place.
  - Headerless fragments can be decoded with a constructed *pseudo header*. Huffman tables are identified by bit-pattern matching, and MCU boundaries by searching for a likely Y-AC end-of-block (EOB) code, which is at most 3 bytes from the boundary. — [Sencar & Memon 2009](https://dfrws.org/sites/default/files/session-files/2009_USA_paper-identification_and_recovery_of_jpeg_files_with_missing_fragments.pdf)
- **Most JPEGs lack restart markers.** Karresand & Shahmehri (2008) reassemble fragmented JPEGs that contain restart markers, but "in practice, most JPEGs do not have such restart markers". In a 2016 study, 20 of 120 fragmented files had broken headers and were excluded as unrecoverable by their method and by the Adroit Photo Forensics (APF) carving tool. — [Recovery of heavily fragmented JPEG files, 2016](https://www.sciencedirect.com/science/article/pii/S1742287616300512)
- **Header estimation.** Huffman tables, image width, quantization tables and sampling factors can be estimated from fragment content to decode headerless fragments without restart markers (J. Image & Graphics 2013). — [CJIG 2013](https://www.cjig.cn/en/issue/2013/1)
  - Related tools: JPGcarve (De Bock & De Smet, IEEE TIFS 2015), Uzun & Sencar, "Carving Orphaned JPEG File Fragments" (IEEE TIFS 2015), and quantization-table reconstruction via saturated overflow (Huang et al. 2011). — [NICC slides](https://jpeg.org/downloads/privacyworkshop-brussels/8.NICC-JPEG-recovery-public.pdf)
- **Truncated JPEG behaviour.**
  - libjpeg-based decoders report "Premature end of JPEG file" and display what decoded, with the rest grayed out, at the correct dimensions when the header is intact. — [Qt forum demonstration](https://forum.qt.io/topic/155253/load-image-report-corrupt-jpeg-data-premature-end-of-data-segment)
  - Pillow requires `LOAD_TRUNCATED_IMAGES = True` to load such files. — [Pillow #3185](https://github.com/python-pillow/Pillow/issues/3185)
- **Rust JPEG decoders.**
  - zune-jpeg "tries to decode as many images as possible, as a best effort, even those violating the standard", with a `set_strict` option. — [zune-jpeg docs](https://docs.rs/zune-jpeg/latest/zune_jpeg/)
  - image-rs's `jpeg-decoder` is in maintenance mode as image-rs moves to zune-jpeg. — [jpeg-decoder repo](https://github.com/image-rs/jpeg-decoder)
- **JPEG 2000 partial decoding.**
  - OpenJPEG `opj_decoder_set_strict_mode`: "If strict decoding is disabled, the decoder will decode partial bitstreams as much as possible without erroring". — [OpenJPEG API](https://www.openjpeg.org/doxygen/openjpeg_8c.html)
  - The feature was added in a pull request on partial bitstream decoding. — [openjpeg #1251](https://github.com/uclouvain/openjpeg/issues/1251)
  - Pure Rust: hayro-jpeg2000 0.4.0 (Sept 2026) is a memory-safe JPEG 2000 decoder "tested against 20.000+ images scraped from random PDFs". — [docs.rs](https://docs.rs/crate/hayro-jpeg2000/latest)
- **Flate predictors.** PDF `/DecodeParms /Predictor 10–15` apply PNG filters per row. Each row carries a filter-type byte, with `Columns`, `Colors` and `BitsPerComponent` defining the row length (ISO 32000-1 §7.4.4.4). — [ExPDF Flate filter docs](https://ex-pdf.hexdocs.pm/1.0.3/Pdf.Reader.Filter.Flate.html); [Stack Overflow pointer to §7.4.4.4](https://stackoverflow.com/questions/54538054/understand-pdf-structure-with-flatedecode)
- **E8: C10 image census.** The cut is exactly 70.0% of the file in every C10 file. [E8](https://github.com/dfrc-korea/REPDF)
  - 'Save As': 100% of DCT image bytes lie before the cut.
  - 'Print to PDF': 54.3% before and 45.7% after, with no DCT image crossing the cut.
- **E1**: about 17 of the 2,162 C9 modifications land inside DCT image data (rough attribution). — [E1](https://github.com/dfrc-korea/REPDF)

### Inferences
- **C9 in DCT data.** A single wrong byte in baseline JPEG entropy data usually decodes. Huffman self-sync keeps the damage local, but DC prediction shifts colours for later blocks until the next RST, matching REPDF's "colour corruption" observation.
  - Exact repair is possible in principle by the same 255-value search, scored by image smoothness/blockiness at the damaged MCU. It is speculative and has no Adler-like oracle, so it risks false fixes.
  - A lower-risk policy: dump the stream verbatim and decode leniently with zune-jpeg.
- **C10 straddlers.**
  - DCT: keep the truncated bytes and append EOI (`FFD9`) so strict decoders accept the file. zune-jpeg or libjpeg-style decoding then renders the top rows.
  - JPX: keep the truncated codestream. Whether hayro-jpeg2000 tolerates truncation is unverified.
  - Flate rasters: keep the decodable prefix, which gives whole rows plus one partial row, and pad the rest.
- In this corpus, no C10 image-level salvage beats "extract every image that lies wholly before the cut". Images after the cut are simply gone.

### Gaps
- Not verified: hayro-jpeg2000 behaviour on truncated or corrupted codestreams, and the OpenJPEG release that introduced non-strict mode.
- No quantitative study found of how often a single random byte in JPEG entropy data leaves a visually acceptable image.
- Recovery rates from Uzun & Sencar 2015 and JPGcarve were not retrieved.
- PNG-predictor-based resync for Flate rasters is untested; the corpus images are DCT.

## Q6. What does each approach cost, which are feasible in pure Rust, and which Rust crates help?

### Takeaway
In pure Rust with `miniz_oxide`, a checkpointed 255-value single-byte search is cheap for inflate-error streams: median 13.5 ms, p90 129 ms, 98.6% exactly fixed. It is cheap for grammar-localized Adler-only text streams: median 25 ms, p90 0.71 s, 94.5% of flagged streams fixed.

It is affordable for any stream ≤4 KiB compressed with no localizer (median 0.23 s, 100% fixed), but grows roughly quadratically without a localizer. At 4–16 KiB only 66% finish within 20 s.

`miniz_oxide`'s decompressor state is `Clone`, so checkpoint/resume needs no custom inflater. A custom inflater is only needed for Brown-style resync with unknown-window markers. No Rust crate implementing markers was found; rapidgzip and pugz are C++.

### Cited Findings
- **E4: inflate-error streams** (221), window [k − 4096, k + 8], scanned backward from k, 60 s budget. [E4](https://github.com/dfrc-korea/REPDF)
  - **218/221 fixed (98.6%), all 218 byte-exact**, with no false accepts.
  - Two budget timeouts and one not found.
  - Time per stream: p50 0.0135 s, p90 0.129 s, p99 13.6 s, max 60 s.
  - 8.2 M candidates took 229 s in total, about 28 µs per candidate.
- **E4: Adler-only streams ≤4 KiB compressed** (245), full range scanned backward from the end, 30 s budget. [E4](https://github.com/dfrc-korea/REPDF)
  - **245/245 accepted; 243 byte-exact.** The two non-exact accepts are the trailer artifact and the Adler collision described in Q2.
  - Time per stream: p50 0.23 s, p90 4.36 s, max 10.4 s.
  - About 6.3 µs per candidate.
- **E4: Adler-only streams 4–16 KiB** (239 sampled), full range, 20 s budget, four processes sharing 4 vCPUs. [E4](https://github.com/dfrc-korea/REPDF)
  - **158/239 (66%) fixed, all exact; 81 timed out.**
  - Median 9–14 s per stream; about 13 µs per candidate (2,734 s for 211 M candidates).
- **E4 + E5: grammar-localized Adler-only text streams** (343 flagged, any size up to 400 KB), window [flag − 256, flag + 8], Adler trailer excluded, 20 s budget. [E4/E5](https://github.com/dfrc-korea/REPDF)
  - **324/343 fixed (94.5%), 323 byte-exact.**
  - Three timeouts; in 16 cases the damage lay outside the window.
  - Time per stream: p50 0.025 s, p90 0.71 s, p99 12.7 s. 160 s in total.
- **Where the load falls.** 49% of the damaged Adler-only fonts are ≤16 KiB compressed and only 2.5% ≤4 KiB (median 16,623 bytes ≈ 16.2 KiB). Fonts make up 798 of the 1,220 Adler-only streams. — [E4](https://github.com/dfrc-korea/REPDF)
- **`miniz_oxide` 0.8.9 source, verified locally.** [crates.io miniz_oxide](https://crates.io/crates/miniz_oxide)
  - `DecompressorOxide` and `InflateState` both derive `Clone` (`inflate/core.rs:234`, `inflate/stream.rs:60`).
  - The core `decompress(r, in, out, out_pos, flags)` returns `(status, in_consumed, out_written)`, which supports resume with `TINFL_FLAG_HAS_MORE_INPUT`.
  - An Adler mismatch returns `TINFLStatus::Adler32Mismatch` after the output has been written.
  - Available flags include `TINFL_FLAG_IGNORE_ADLER32` (64), `TINFL_FLAG_COMPUTE_ADLER32` (8), and `TINFL_FLAG_STOP_ON_BLOCK_BOUNDARY` (128, behind the `block-boundary` cargo feature).
  - Flag semantics: [docs.rs inflate_flags](https://docs.rs/miniz_oxide/latest/miniz_oxide/inflate/core/inflate_flags/).
- **Throughput of related tools.**
  - rapidgzip single-threaded: 169 MB/s. Dynamic-block finder: 43 MB/s. zlib trial-and-error block finding: 0.12 MB/s. — [Rapidgzip](https://arxiv.org/pdf/2308.08955v1)
  - pugz finds a block start in about 100–300 ms. — [pugz](https://arxiv.org/pdf/1905.07224)
  - Barral et al. 1-bit-flip brute force: 73 min for 204 fragments of ≤128 KiB on 12 threads. — [Barral et al.](https://arxiv.org/pdf/2210.00856)
  - ZipRec: recover about 10× unzip time; the n-gram reconstruction was 12× faster than 2011's, which took 247 s per 5 MB. — [Brown 2013](https://www.sciencedirect.com/science/article/pii/S1742287613000492)
- **Image crates**: zune-jpeg (lenient by default) — [docs](https://docs.rs/zune-jpeg/latest/zune_jpeg/); hayro-jpeg2000 0.4.0 (pure Rust, `simd` feature optional) — [docs.rs](https://docs.rs/crate/hayro-jpeg2000/latest); jpeg-decoder (maintenance mode) — [repo](https://github.com/image-rs/jpeg-decoder).
- **Tooling.** Mark Adler's `infgen` is a "Deflate disassembler to convert a deflate, zlib, or gzip stream into a readable form", useful for debugging salvage on fixtures. — [madler GitHub](https://github.com/madler)

### Inferences
- **Cost model.** Adler-only candidates rarely fail early, so each costs a decode from the checkpoint to the end of the stream. A full-range search is therefore O(N² × 255) in compressed size N, and localizers turn it into O(W × 255 × suffix length).
- **Per-file load.** A C9 file carries ~14.5 damaged Flate streams, so:
  - Rayon parallelism across streams (independent) is a straightforward 3–4× on 4 cores.
  - Parallelism across candidate values within a stream also works, because each worker clones the checkpoint state.
- **Checkpointing cost** (sizes measured locally with `size_of`):
  - `DecompressorOxide` is 10,504 bytes. With the non-wrapping output-buffer API, the 32 KiB window lives in the shared output buffer, so one checkpoint is about 10.5 KB.
  - `InflateState`, the streaming API, is 43,296 bytes because it embeds the window.
  - Checkpoints every 256 input bytes are fine for streams ≤256 KiB: at most 1,024 checkpoints, about 11 MB (non-wrapping) or 44 MB (`InflateState`). Use coarser spacing, or re-derive checkpoints lazily, for bigger streams.
- **Brown-style resync** is feasible in pure Rust with a custom inflater that emits u16 symbols (literal or marker). The inflater itself is small: the RFC 1951 decoder logic plus the 48-offset convergence loop. rapidgzip's design (15-bit markers, marker replacement pass) is the reference. It is an estimate of a few hundred lines; nothing like it was found on crates.io.

### Gaps
- Checked only `miniz_oxide` 0.8.9. The design pins 0.9, whose `Clone` derives and flags should be re-verified.
- Did not benchmark `zlib-rs` or `libdeflater` as the candidate engine.
- Did not measure the TrueType-localized font search; its cost is extrapolated from the ~37% window.

## Q7. What concrete, ranked changes should PDFPundit make to its C9/C10 ladder, and what should each gain and cost?

### Takeaway
The current ladder's steps 3–4 will repair almost nothing on REPDF's C9. The damage is a random byte (97% multi-bit), 84% of damaged streams fail only at Adler-32, and stored blocks are essentially absent.

The highest-value changes are:
1. Keep all decoded output on an Adler mismatch.
2. Search all 255 values, backward from k, with `miniz_oxide` checkpoints.
3. Add grammar and TrueType-checksum localizers for Adler-only streams.
4. Tighten acceptance.

C10 gains will come from carving, image extraction and font substitution, not from DEFLATE tricks.

### Cited Findings
- C9 facts from E1–E4:
  - One random byte in about 42% of Flate streams; only 3.1% single-bit.
  - 84.4% of damaged streams are Adler-only and 15.3% hit an inflate error.
  - Inflate errors surface a median of 5 bytes after the damage.
  - 1 stored block in 3,398 streams.
  - Exact-fix rates: 98.6% of inflate-error streams, 94.5% of flagged text streams, 100% of streams ≤4 KiB.
  - Sources: [E1–E4 on the REPDF dataset](https://github.com/dfrc-korea/REPDF).
- **About 33% of C9 modifications fall outside Flate data**: dictionary keys such as `/Length`, object headers, descriptor numbers, uncompressed XMP/CMap text, and DCT data. — [E1](https://github.com/dfrc-korea/REPDF)
- **E8: C10 stream census** (70.0% cut). [E8](https://github.com/dfrc-korea/REPDF)

  | Method | Stream type | Wholly before cut | Crosses cut: decoded | Crosses cut: lost | Wholly after cut |
  | --- | --- | --- | --- | --- | --- |
  | Save As | Content streams | 100% | 0% | 0% | 0% |
  | Save As | DCT images | 100% | 0% | 0% | 0% |
  | Save As | Font bytes | 64.9% | 8.9% | 4.0% | 22.2% |
  | Save As | CMap bytes | 58.0% | — | — | 41.8% |
  | Print to PDF | Text content streams | 63.3% | — | — | 35.6% |
  | Print to PDF | Graphics content streams | 96.8% | — | — | — |
  | Print to PDF | DCT images | 54.3% | 0% | 0% | 45.7% |
  | Print to PDF | Font bytes | 56.3% | 13.3% | 13.4% | 17.0% |

  92 of the 97 streams that cross the cut are fonts.
- REPDF's C10 text results: 99.57% ('Save As') and 35.35% ('Print'). REPDF hypothesizes that 'Print to PDF' "tends to place text streams and font resources near the end of the file". — [REPDF §5.4](https://doi.org/10.1016/j.fsidi.2026.302061)

### Inferences
Ranked changes. Gains are on the REPDF corpus unless noted. "Measured" means E-series numbers; "est." means projection.

1. **Never throw away a completed decode on an Adler-32 mismatch.**
   - What: new result state `Salvage::ChecksumMismatch{data}`. Decode with `miniz_oxide` streaming (or `TINFL_FLAG_IGNORE_ADLER32` plus your own Adler) rather than relying on `flate2`'s error path, so every byte produced is kept.
   - Gain: per damaged stream, mean correct bytes rise from 48.1% (prefix only, i.e. REPDF-like) to 59.0% (prefix plus suffix). For the 84% of damaged streams that are Adler-only, the 31.6% that keep their length are a median 99.99% correct (measured).
   - Cost: roughly zero.
   - This is also the fallback whenever the search below fails.
2. **Replace the 8-bit-flip search with a 255-value single-byte substitution search, driven by `miniz_oxide` checkpoints.**
   - How: clone `DecompressorOxide` every 256 input bytes. For inflate-error streams, scan positions from k downward to k − 4096, with k + 8 as the upper slack, not k + 64. Accept only on `TINFLStatus::Done`.
   - Gain: covers 97% more modifications than bit flips. **98.6% of inflate-error streams exactly repaired**, median 13.5 ms, p90 129 ms (measured).
   - Cost: about 150 lines of Rust. Replace the fixed 256 K-attempt cap with a time/candidate budget per stream; one window is 1.04 M candidates, but the median hit comes after about 1.3 K.
3. **Add localizers so Adler-only streams get the same search.**
   - (a) **Content/CMap grammar localizer**: search [flag − 256, flag + 8]. 85% of text streams are flagged; **94.5% of flagged streams exactly repaired, median 25 ms** (measured).
   - (b) **Streams ≤4 KiB**: full-range search. **100% accepted, median 0.23 s** (measured).
   - (c) **FontFile2 streams**: use the first TrueType table whose checksum fails as the window. It is correct in 765/766 cases and averages ~37% of the font (measured). Its search cost is unmeasured; est. several seconds per median ~16 KiB font on 4 cores. Refine to glyph level via `loca`/`glyf` structure checks (speculative).
   - Always exclude the 4 trailer bytes and the 2 zlib-header bytes from the Adler-driven search; handle those separately (item 6).
   - Est. total: ≈39% of all damaged Flate streams byte-exact demonstrated (218 + 324 + ~20 small fonts out of 1,445), plausibly about 60–70% once fonts are localized. The rest fall back to item 1. Compare about 3% at most with the current plan.
4. **Harden acceptance.**
   - Require an Adler-32 match. Never accept on "classifies as valid content" alone.
   - After a hit, test the remaining values at that position and at ±8 positions. If more than one candidate passes, use the grammar or font checksums to break the tie. If still ambiguous, emit `Partial` with the differing bytes marked (three-valued, Barral-style).
   - Measured Adler false accepts: 1/245 in small streams (plus 1 trailer artifact) and 1/324 in localized text streams.
   - Cost: small. Gain: avoids silent wrong fixes, which matters for a forensic tool.
5. **Budgets and parallelism.**
   - Per stream: about 2 s by default and about 20 s in a "deep" mode, spent in the order err-window → localized window → full range if ≤16 KiB.
   - Per file: a total budget across the ~14.5 damaged streams, run with rayon.
   - Progress and cancellation hook into the job system (§3 of the design).
   - Est. typical C9 file: well under 10 s on 4 cores, with fonts dominating.
6. **zlib-framing special cases.**
   - Header damage (1 in E2): try the standard CMF/FLG pairs (`78 01/5E/9C/DA`) or raw inflate.
   - Trailer damage (2 in E2): the decode completes, and the only fix candidates are in the trailer. Report `ChecksumMismatch` with the note "trailer possibly damaged" rather than "repairing" the checksum.
7. **Replace step 4 (stored-block resync) with Brown-style intra-block resync** using the known Huffman tables. Try 48 bit offsets past the damage until they converge, then continue with u16 unknown-window markers.
   - Gain on the REPDF corpus: small. It only applies to budget failures, with the stored-block version worth about 0%. On real-world multi-byte damage it is demonstrated to recover the tail with output errors about 1.5× the corrupted span (Brown).
   - Cost: a custom inflater of an est. few hundred lines.
   - Optional later work: grammar or per-document n-gram filling of co-indexed unknowns (Q4, speculative).
8. **Make the C9 pass robust to the ~33% of C9 modifications outside Flate data.** This is outside DEFLATE, but it lands in the same files.
   - The carver's inflate-probe extent (§14.3 d) must not depend on the `/Length` key surviving.
   - The lexer should accept near-miss keys and object headers such as `11l 0 obj`. A C5-style path is one option.
   - Uncompressed CMap and XMP text can reuse the grammar localizer to patch single bad bytes: guess the most grammatical replacement, marked as inferred.
9. **C10: DEFLATE-level work is marginal. Aim effort at:**
   - (a) Keeping the decodable prefix of each stream that crosses the cut. These are mostly fonts. From a truncated TrueType font, salvage the tables that lie before the cut and the glyphs reachable via `loca`.
   - (b) Extracting every image that lies wholly before the cut. For a JPEG or JPX that crosses the cut, keep the partial data (JPEG plus an appended `FFD9`) and decode leniently with zune-jpeg or hayro-jpeg2000.
   - (c) Font substitution for fonts that are lost. The ToUnicode survival rate by creation method is unmeasured.
   - For 'Print to PDF' files, 35.6% of text content-stream bytes lie beyond the cut. That is a hard ceiling of about 64% by content bytes, against REPDF's 35.35% text. The gap is mostly lost or truncated fonts (30% of font bytes), so item (c) is the lever (inference).
10. **Images under C9.** Dump DCT and JPX verbatim and decode leniently. Optionally try a 255-value search on JPEG entropy data scored by blockiness at the damaged MCU (speculative, no oracle, off by default).

### Gaps
- The text-recovery impact of these changes (REPDF's OCR metric or PDFPundit's `hayro` text diff) was not measured. The gains above are stream-byte-level.
- The per-file wall-clock time with all localizers and fonts combined was not measured.
- The TrueType-localized font search cost and success rate were not measured.
- Behaviour on non-REPDF, real-world damage (burst errors, sector loss) is covered only by Brown's and Barral's published results.
