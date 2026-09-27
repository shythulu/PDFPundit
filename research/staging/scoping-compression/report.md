# scoping-compression report (block 3)

*The agent couldn't write report files, so the chair saved its hand-back text here.*

Scope: compressed-stream (DEFLATE) error resilience, repair in other formats, program repair and strategy selection. Date: 2026-09-27.

## Counts
- **Screening:** 40 titles (34 included, 6 excluded), over the 25–35 quota. Six were already in the main registry after the scoping-forensics merge, so they carry `duplicate_of`.
- **Full texts:** 4 read with notes: Brown 2011 (SRC-0300), Brown 2013 (SRC-0301), pugz (SRC-0302) and E-APR (SRC-0328).
- **Claims:** 40, all quotes verified (38 exact, 2 fuzzy).
- **Experiment:** `research/experiments/flate_c9_probe.py` (moved there by the chair from the staging area), which produced OBS-0300..0303.
- **Records:** 7 gaps, all still open (checked with OpenCitations); 3 draft hypotheses.
- **Paid searches:** 4 of 6.

## Main finding
In REPDF C9, every damaged Flate stream has exactly one changed byte (1442 streams).
- In 84% of those streams, zlib only reports a failed Adler-32 check at the end.
- Salvaging by decoding until the first error emits wrong bytes in 95% of streams.
- 64% of Flate streams are a single block, so restarting at the next intact block cannot help.
- Word "Save As" streams are 54% byte-identical to stock zlib level 6 re-compression.

This narrows the charter §5 flag: C9 changes 12–30 bytes per file, but only one byte per damaged stream. The 415 changes outside streams are something a stream-only repair never touches.

## Observations
The script runs in about 80 s using zlib's own block reporting.

| | Result |
|---|---|
| OBS-0300 | 1442 damaged Flate streams, each with 1 changed byte. 1217 fail only the Adler-32 check, 221 hit a decode error, 4 run out of input. zlib notices a median of 3,204 compressed bytes late. 1375 streams get wrong output from decode-until-error salvage (median 11,426 bytes). |
| OBS-0301 | 2199 of 3425 Flate streams are a single block. 982 of 1442 damaged streams have no block boundary after the damage. |
| OBS-0302 | Of 2162 changed bytes in C9 files, 1443 are in Flate bodies, 304 in other stream bodies and 415 outside streams, in 99 of 100 files. |
| OBS-0303 | Stock zlib reproduces 1124 of 2081 "Save As" streams exactly, but only 1 of 1344 "Print to PDF" streams. |

## What I did
- **Screening:**
  - SRC-0300..0339 are logged in `sources/screening.jsonl`.
  - Duplicates of main-registry records: SRC-0301→0125, 0308→0135, 0318→0132, 0319→0127, 0325→0123, 0338→0126.
- **Reading:**
  - Barral 2022 (SRC-0338) is the same PDF as the main SRC-0126. I kept only CLM-0341 (a flash bitflip rate of about one per 248,253 bytes) and dropped my duplicate note.
  - RFC 3309 (Adler-32 is weak on small inputs) and Mark Adler's zlib salvage answer (fetched through the Stack Exchange API) are cached for targeted quotes only.
- **Searches:** SRCH-0300..0310. The paid ones are SRCH-0300, 0306, 0307 and 0310.

## Literature
- **Thin field:** DEFLATE recovery is almost untouched since Brown; his two papers have 8 and 6 citing works, none extending the method.
- **Finding the damage:** Brown 2013's method needs the corruption position as input. It detects corruption automatically only for runs of at least 128 identical bytes, and two general-case detectors failed (CLM-0308, 0316, 0317).
- **Resynchronizing:** restarting inside a block converges within 150 bytes (CLM-0310, 0311). Unknown history can be tracked as co-indexed symbols (CLM-0323). Adler's trick keeps only output that is identical when decoded with two different dummy dictionaries (CLM-0337).
- **Filling in lost bytes:** this has only been done for prose (CLM-0303). Repetitive markup, which PDF content streams resemble, is the hardest case (CLM-0321).
- **Strategy selection (RQ5):** E-APR picks the right repair technique with 88% precision. It was trained on repairs that merely passed tests, and it cannot abstain (CLM-0329..0331).

## Gaps and their links to existing ones
- **GAP-150** (finding where the corruption is) refines GAP-005 and relates to GAP-253.
- **GAP-151** (salvaged prefixes contain silently wrong bytes) links to GAP-010, 252 and 008. It shows the charter's grade 2 ("salvaged prefix") is unsafe as currently defined.
- **GAP-152** (resynchronizing inside a block) links to GAP-005 and 006.
- **GAP-153** links to GAP-005, 006 and 004.
- **GAP-154** (encoder replay as a stronger check than Adler-32) links to GAP-052, 253 and 016.
- **GAP-155** (a strategy selector must learn from fidelity to the original and be able to abstain) links to GAP-053 and 204.
- **GAP-156** links to GAP-001, 013 and 050.

## Hypothesis seeds
- **HYP-150:** try every single-byte substitution in each damaged C9 stream (at most 255 × length inflations, median length 9,547 bytes). Accept a candidate only if it inflates, passes Adler-32 and passes further checks: exact re-encoding by stock zlib, /Length1, TrueType table checksums, or content-stream tokens. Recommended as the first W1 spec.
- **HYP-151:** resynchronize inside the block, Brown 2013 style, on the 853 damaged single-block streams, with zero bytes emitted as known but wrong. Also ready to test.
- **Not yet a record:** find the damage by comparing the stream's LZ77 symbols with a replayed zlib or preflate-rs parse.
- **HYP-152 (needs research):** a toolpath selector trained on fidelity labels, with a reject threshold.

## What I did not check
- rapidgzip, the paywalled Luo 2024, Park 2008 and the Wang et al. papers, and the licences of gzrt and preflate-rs.
- The strategy-selection papers were screened only. No repair technique was run.
- The engine-agent topics were left to them.
- OpenCitations coverage is incomplete, so every "still open" judgement rests only on the citing works it lists.
- Stream bodies were found with the same regex as OBS-0002, so streams inside object streams may be missed.

## Decisions for the chair
1. Move `experiments/` to `research/experiments/` and update the OBS paths.
2. Re-run `fetch_fulltext` for SRC-0125 after the merge.
3. Amend GAP-052's inference (one byte per stream, not several).
4. Approve HYP-150.
5. Fix `polite_get.py`: it crashes on HTTP 429 instead of backing off.
6. arXiv `id_list` lookups also return 406; `arxiv.org/abs` pages work.
7. The screening quota was exceeded (40 titles).
8. SRC-0305 (IEEE) and the Springer papers SRC-0306 and SRC-0307 need institutional access, if you want them read.
9. ZipRec is GPL, so if it is used as a baseline the clean-room rule applies.

*Chair's notes, 2026-09-27:*
- (1) Done before merge.
- (5) and (6) were fixed in commit d9de5a3; `polite_get` retries arXiv 406s.
- (2) The merge now carries the cached-text fields when it folds a duplicate.
- (3) and (4) go to the 10k revision and the review board: approving a hypothesis is not a sweep decision.
