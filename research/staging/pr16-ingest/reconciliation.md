# PR #16 reconciliation against the evidence-checked KB (pr16-ingest, 2026-10-05)

## Inputs

**PR #16** was merged as c7a6ad4 and is registered as SRC-1000 (the report) and SRC-1001..1005 (its five notes). It is a source, not evidence.
- Its claims are CLM-1000..1079 (79 claims; CLM-1012 is unused), all with exact quotes.
- Our re-runs are OBS-1000..1015, on REPDF at commit e547d4d. The scripts are in `experiments/`; the patches are listed in `experiments/README.md`.

**Tags on the claims**
- `replicated-by:OBS-…`: the claim reproduces exactly.
- `replicated-approx:OBS-…`: it reproduces within run-to-run or sampling noise.
- `replicated-in-part:OBS-…`: some parts reproduce.
- `contradicted-in-part:OBS-…`: some parts are contradicted.
- `unreplicated`: we could not run it, or it is not a measurement.

**This file proposes changes only. No main registry was edited.**

## 1. Dedupe table

**Verdict codes:**
- **agrees**: same finding as an existing record.
- **refines**: same direction, sharper or with a corrected denominator.
- **contradicts**: conflicts with an existing record.
- **new**: no existing record.
- **claim-only**: not reproduced here.

### 1a. C9 damage profile and inflate outcomes (E1–E3)

| PR #16 claim | Our check | Existing KB record | Verdict |
|---|---|---|---|
| CLM-1000: 12–30 changed bytes per file, mean 21.62 | OBS-1000, exact | OBS-0002, OBS-0005 | agrees |
| CLM-1000: 7–26 damaged **Flate** streams per file, mean 14.45 | OBS-1000, exact | OBS-0002 says "11–28 distinct stream bodies"; repeated in GAP-005, GAP-013, GAP-052 | refines. OBS-0002 counts every stream body hit (Flate, unfiltered and DCT); PR #16 counts Flate only. No contradiction, but the gap texts say "11–28 streams" where they mean Flate streams. |
| CLM-1000: 1,445 of 2,162 changes in Flate data, one per stream | OBS-1000, OBS-1012 | OBS-0300 (1,442 damaged Flate streams), OBS-0302 (1,443 Flate-body bytes plus 3 "FlateDecode that does not inflate cleanly in the original") | contradicts OBS-0300..0302 by 3 streams. **PR #16 is right.** The KB probe's EOL trimming (`trim_eol`, CR LF) cut off a final 0x0D zlib byte in 3 Print to PDF streams (OBS-1012). New GAP-301. |
| CLM-1001, CLM-1002: 3.1% single-bit changes; Hamming histogram | OBS-1000, exact | none. 5k plan WP-3.9 feeds "NAND bit-flip rates" | new. C9 is uniform random byte replacement, not a bit-flip model. |
| CLM-1003: kinds (854 fonts, …) | OBS-1000, exact | none at this granularity | new |
| CLM-1004..1006: 84.4% Adler-only, 15.3% inflate error, 4 truncated; error messages | OBS-1001, exact (1,220 / 221 / 4) | OBS-0300: 1,217 / 221 / 4 | agrees, apart from the 3 streams above (all Adler-only) |
| CLM-1004, CLM-1007: an inflate error fires a median of **5** compressed bytes after the damage; 99.1% inside [k−4096, k+64] | OBS-1001, exact | GAP-150 says "when zlib does fail it is thousands of compressed bytes later"; OBS-0300's latency median is 3,204 | contradicts GAP-150's wording. OBS-0300's 3,204 is taken over all damaged streams, including Adler-only ones whose "error" is the trailer check. For the 221 streams that really raise, the median is 5 and the p90 is 68 (OBS-1001). |
| CLM-1008..1010: synthetic replacements; position and kind effects | OBS-1002, exact (seeded) | none | new |
| CLM-1011: one stored block in 3,398 streams; CMaps and 94% of text streams are a single block | OBS-1004, exact | OBS-0301 (2,199 of 3,425 single-block), GAP-152 | agrees (2,201 of 3,398 single-block). The stream sets differ slightly. |
| CLM-1013: 31.6% of Adler-only streams keep their length; median 5 wrong bytes, 99.99% correct | OBS-1003, exact | GAP-151 / OBS-0300: "median 11,426 wrong bytes" | refines GAP-151. The 11,426 is a position-wise count, so every byte after a length shift counts as wrong. For the 386 same-length Adler-only streams the output is 99.99% correct. |
| CLM-1014: prefix 48.1%; prefix plus suffix 59.0% (Adler-only 53.6% / 66.6%) | OBS-1003, exact after fixing our summariser | OBS-0300 verbatim prefix median 0.48065 (a median, not a mean) | agrees on the prefix. The suffix gain is new and backs keep-all salvage with suffix alignment. |

### 1b. Search, localizers and the Adler-32 oracle (E4, E5)

| PR #16 claim | Our check | Existing KB record | Verdict |
|---|---|---|---|
| CLM-1015: checkpoint is 10,504 bytes; InflateState is 43,296 bytes | OBS-1005, exact on miniz_oxide 0.8.9 **and** 0.9.1 | none | new. Removes the "re-check on 0.9" caveat of CLM-1067. |
| CLM-1016, CLM-1017: inflate-error search fixes 218/221, all exact; 8.2 M candidates in 229 s | OBS-1006 (218/221 exact; 8.02 M in 241.7 s); OBS-1015 (miniz_oxide 0.9.1: 219/221 exact, 8.18 M in 227.9 s) | HYP-150 (not run), GAP-150 | new evidence for HYP-150 on the 221 streams. Counts exact; time approximate on a shared host. The 218 vs 219 depends on the 60 s budget and the CPU load. |
| CLM-1018: small Adler-only streams 245/245 accepted, 243 exact, median 0.23 s | OBS-1007: **243/245 accepted, 242 exact**, median 0.24 s | none | **not reproduced with the committed code.** mzbench searches only `0..clen-4`, so the 2 streams with trailer damage cannot be found. Our inference: PR #16 ran a trailer-inclusive variant. Under it, the audit (OBS-1013) gives 242 − 1 + 2 = 243 exact and 245 accepted, which matches CLM-1018. |
| CLM-1028: one genuine collision on 2.7 KB of output; one false accept from trailer damage | OBS-1013 | GAP-052, GAP-253, CLM-0333/0334 (Adler-32 weak on short input) | contradicts in part. The genuine collision (true offset, wrong value) is on **21,134** bytes of output (3,971 compressed). The 2,707-byte stream is a CMap whose corrupted output's Adler-32 differs from the stored trailer in one byte, so a trailer-inclusive search "fixes" it by editing the trailer. The two damaged-trailer streams are different streams. The conclusion that Adler-32 alone admits wrong accepts stands. |
| CLM-1029: false accepts 1/245 (small) and 1/324 (grammar) | OBS-1007, OBS-1009 | GAP-052 (untested whether a checksum-passing candidate is unique) | new measurement for GAP-052. These are first-accept rates, not uniqueness counts: the search stops at the first value that passes. |
| CLM-1019, CLM-1025: unlocalized search finishes 158/239 (66%) of 4–16 KiB streams in 20 s | OBS-1010: 28/47 (59.6%, Wilson 95% CI 45–72%), all exact | none | approximately reproduced (stride-10 sample, shared CPU) |
| CLM-1020..1022: grammar localizer flags 343/403 (85%); true offset in [flag−64, flag+8] for 91.8% | OBS-1008, exact | GAP-150 lists "content-stream operator grammar" as a candidate signal | new evidence; GAP-150's candidate is now measured |
| CLM-1023, CLM-1024: grammar-localized search fixes 324/343, 3 timeouts and 16 outside the window | OBS-1009: 324/343 (323 exact), 4 timeouts and 15 outside | GAP-150, HYP-150 | reproduced. The timeout/outside split is approximate (CPU-dependent 20 s budget). |
| CLM-1026, CLM-1027: TrueType checksum localizer finds the damaged table in 765/766 fonts | **claim-only** (E6 not committed) | GAP-150 lists "font-table structure (TrueType table directory/checksums)" | claim-only. New HYP-300 tests it. |
| CLM-1030, CLM-1031: about 39% of damaged streams byte-exact (218 + 324 + 20) | OBS-1014: 562/1,445 = 38.9% (561 byte-exact) | none | arithmetic reproduced. The "≤3% under the current plan" and the 60–70% projection are not checked. |
| CLM-1032: gains not measured on REPDF's OCR metric | — | GAP-008 (metric suite) | new GAP-300 (the translation from stream gains to OCR recall) |

### 1c. C10, REPDF tables, corpus scan and fonts

| PR #16 claim | Our check | Existing KB record | Verdict |
|---|---|---|---|
| CLM-1033, CLM-1034: C10 cut at 70.0%; content-byte ceiling of about 64% vs 35.35%; 92 of 97 straddling streams are fonts | **claim-only** (E8 not committed) | CLM-0002/0003, GAP-006 (layout hypothesis untested) | claim-only. It bears directly on GAP-006 but needs a committed census first. |
| CLM-1035: REPDF Table 2 vs Table 5 disagree (C10 35.35 vs 30.49; C9 60.11 vs 58.77/62.70) | not checked against SRC-0001's text here | GAP-013, CLM-0022 (per-language and aggregate tables disagree) | agrees with GAP-013 and adds a second table conflict. Verify against SRC-0001's cached text before citing. |
| CLM-1036..1038: ObjStm use, BaseFont census, Noto coverage | **claim-only** (ad hoc scan, CLM-1068) | GAP-251, GAP-003, GAP-200 | claim-only |
| CLM-1039, CLM-1040: width disagreement between fonts; letter entropy; Han all 1000 units | OBS-1011, exact (same system fonts) | GAP-003 (width vectors for identification), GAP-205 (decipherment) | agrees. Supports width fingerprinting for Latin; useless for Han. |

### 1d. Engine rules (from PR #16's reading; no code citations)

| PR #16 claim | Existing KB record | Verdict |
|---|---|---|
| CLM-1043: qpdf rejects a length that swallows another object's header | CLM-0503 | agrees |
| CLM-1044: MuPDF keeps a good copy over a truncated object at EOF | CLM-0510, CLM-0511 (the scan stops at an unparsable object once a root is seen) | refines, if verified |
| CLM-1045, CLM-1046: pdf.js lets a later copy win only if it parses; proposes "last well-formed copy wins" | CLM-0517 (pdf.js) agrees. CLM-0501 (qpdf), CLM-0541 (Ghostscript) and CLM-0525/0533 (PDFium, Poppler) use last-wins or generation rules. GAP-250 says §17.2 fixes "last in byte order wins" without evidence. | agrees with CLM-0517. It is a design proposal for GAP-250 (new HYP-301), not evidence that one rule is better. |
| CLM-1047: MuPDF bug-708286, ObjStm member vs later uncompressed copy | CLM-0510 | agrees |
| CLM-1048: pdf.js and hayro validate Root → Pages → Kids/Count | CLM-0518 (pdf.js) | agrees for pdf.js. hayro is new and unverified. |
| CLM-1041, CLM-1042, CLM-1049: lopdf rules (unique endstream, line-start headers, ±64-byte offset fix) | none (lopdf is not checked out; 5k WP-1.4 lists it) | new and unverified. Needs code citations. |
| CLM-1050: qpdf rebases offsets on a %PDF header in the first 1 KiB | CLM-0540 (Ghostscript searches the whole file for %PDF); GAP-256 | new for qpdf; unverified |
| CLM-1051: qpdf and hayro accept a bare CR after `stream`; strip exactly one EOL before `endstream` | CLM-0526 (PDFium trims one EOL) | partly agrees. The one-EOL rule is ambiguous when data ends in 0x0D (GAP-301, OBS-1012). |
| CLM-1052, CLM-1053: qpdf page-tree recovery; 5,000-element cap | CLM-0507 | refines CLM-0507 (dedupe, depth cap 100); the cap is new; unverified |
| CLM-1055: no engine uses decompression to find a stream's extent | CLM-0503, 0509, 0520, 0526, 0535, 0544 (all scan for `endstream`) | agrees with all six KB engine claims |
| CLM-1056: engines harvest the trailer dictionary of xref streams | CLM-0532 (Poppler parses XRef streams as trailer candidates), CLM-0519 | agrees for Poppler and pdf.js; others unverified |

### 1e. Evaluation, calibration and ranked changes

| PR #16 claims | Existing KB record | Verdict |
|---|---|---|
| CLM-1054 (shadow attacks) | GAP-015, GAP-016, GAP-104 | agrees. A secondary citation; read the primary source before using it as evidence. |
| CLM-1057..1061 (generate-and-validate; plausible-but-wrong KPI; Platt scaling; rule of three) | GAP-155, GAP-204, GAP-055 | agrees in direction; design proposals. CLM-1058 and CLM-1063 cite papers second-hand. |
| CLM-1062 (Dice/LCS-F1 is not REPDF's OCR recall) | GAP-008, 5k WP-3.5 | agrees. The basis of GAP-300. |
| CLM-1064..1066 (grouped splits; non-inferiority; held-out corruptors) | 5k WP-3.11, GAP-055 | agrees; design proposals |
| CLM-1069..1079 (ranked changes 1–11) | various | proposals, not evidence. Rank 1 (C9 ladder) is the best supported: OBS-1006..1010 and 1014. |

## 2. Proposed field updates to main-registry records

**For the chair. None of these are applied.**

### Observations and the probe script

| ID | Field | Old | New | Reason |
|---|---|---|---|---|
| OBS-0300 | `result` / `statement` | 1,442 damaged Flate streams; check-only 1,217 | Add: "undercounts by 3. PR #16 and OBS-1012 find 1,445 (1,220 Adler-only); flate_c9_probe.py's `trim_eol` strips CR LF and cuts a final 0x0D zlib byte." Better: fix the script and re-run. | OBS-1012 |
| OBS-0300 | `result` (detection latency) | "min/median/max 1/3204/2341864" | Add: "over all damaged streams; for the 221 streams that raise an inflate error, the median is 5 compressed bytes (OBS-1001)" | OBS-1001; stops the "thousands of bytes" misreading |
| OBS-0301, OBS-0302 | `result` | counts built on the 1,442 streams; "FlateDecode (does not inflate cleanly in the original) 3" | Re-run after the fix. Those 3 bytes are ordinary Flate damage (OBS-1012). | OBS-1012 |
| `research/experiments/flate_c9_probe.py` | code | strips CR LF before `endstream` | Strip one EOL. If the result does not end the zlib stream exactly, retry with the CR kept. | GAP-301 |

### Gaps

| ID | Field | Old | New | Reason |
|---|---|---|---|---|
| GAP-150 | `statement` | "…and when zlib does fail it is thousands of compressed bytes later." | "…and when zlib does fail (15% of damaged streams) it fails a median of 5 compressed bytes later (p90 68); latency is long only for Adler-only streams." | OBS-1001, CLM-1007 |
| GAP-150 | `evidence` | … | Add OBS-1001, OBS-1008, OBS-1009, CLM-1026 | grammar localizer measured; TrueType localizer is claim-only (HYP-300) |
| GAP-150 | `hypotheses` | [HYP-150] | [HYP-150, HYP-300] | new hypothesis |
| GAP-151 | `statement` | "emits wrong bytes in 95% of damaged streams (median 11,426 per stream)" | Add: "counted position-wise. 31.6% of Adler-only damaged streams keep their length and are then 99.99% correct (median 5 wrong bytes). Prefix plus common suffix keeps 59.0% of decoded bytes, against 48.1% for the prefix alone." | OBS-1003 |
| GAP-151 | `evidence` | … | Add OBS-1003 | |
| GAP-052 | `statement` / `evidence` | "nobody has measured how often a unique checksum-passing candidate exists…" | Add: "first-accept false-accept rates on REPDF C9: 1 of 245 small streams and 1 of 324 grammar-localized (OBS-1007, OBS-1009, OBS-1013). Including the 4 trailer bytes in the search adds a further wrong accept (one stream whose corrupted output's Adler-32 differs from the trailer in one byte); excluding them makes trailer damage (2 of 1,445 streams) unfixable. Uniqueness is still unmeasured." Change "11-28 such streams per file" to "7-26 damaged Flate streams per file (11-28 stream bodies of any filter)". | OBS-1007, 1009, 1013, 1000 |
| GAP-253 | `evidence` | … | Add OBS-1013 | trailer-edit false accept |
| GAP-005, GAP-013 | `statement` | "11-28 streams" / "in 11-28 streams per file" | "7-26 Flate streams (11-28 stream bodies of any filter)" | OBS-1000 vs OBS-0002 |
| GAP-013 | `evidence` | … | Add CLM-1035 (after verifying it against SRC-0001's text) and OBS-1012 | second REPDF table conflict; the KB's own count needed a fix |
| GAP-006 | `inference` | — | "PR #16 E8 (CLM-1033/1034, claim-only): cut at exactly 70.0%; content-byte ceiling of about 64% vs 35.35%. Rebuild the census with a committed script before relying on it." | lead only |
| GAP-003 | `evidence` | … | Add OBS-1011 | width-vector discrimination measured for 4 Latin fonts; useless for Han |
| GAP-205 | `evidence` | … | Add OBS-1011 | widths cut lowercase letter entropy from 4.18 to between 0.28 and 2.10 bits |
| GAP-250 | `hypotheses` | [] | [HYP-301] | last-well-formed-wins test |
| GAP-008 | `related_gaps` | … | Add GAP-300 | |

### Hypotheses and claims

| ID | Field | Old | New | Reason |
|---|---|---|---|---|
| HYP-150 | `notes` | — | "Partly run by PR #16's mzbench (OBS-1006, 1007, 1009, 1010): 218/221 inflate-error streams and 323/343 grammar-flagged streams byte-exact, first-accept false accepts 1/245 and 1/324. Uniqueness (all passing candidates) and the secondary oracles are still untested." | OBS-1006..1010 |
| HYP-150 | `data` | "per-stream ground truth from … flate_c9_probe.csv (OBS-0300)" | Add: "or the 1,445 cases from build_cases.py (OBS-1012: the probe misses 3)" | |
| CLM-0003 | `tags` | — | Add `see:CLM-1033` | C10 layout lead |

## 3. Changes the 5k plan needs (`nimbalyst-local/plans/bleeding-edge-repair-5k.md`)

1. **WP-3.3 Verification oracles.** Acceptance currently reads "on REPDF C9 (SRC-0002), report how many candidates each oracle leaves per stream; the replay oracle must never accept a candidate that differs from the original". Add:
   - Report a first-accept false-accept KPI next to candidate counts. Measured with Adler-32 alone: 1/245 and 1/324 (OBS-1007, OBS-1009).
   - Treat the 4 Adler-32 trailer bytes explicitly. Excluding them leaves trailer damage unfixable; including them admits one-byte trailer matches (OBS-1013).
   - Add a TrueType table-checksum oracle and localizer (step 4 already lists the font tables' checksums; make localization part of it; HYP-300).
   - Use the grammar localizer from OBS-1008/1009 as the content-grammar oracle's first consumer.
2. **WP-3.9 Damage models.** Inputs list "NAND bit-flip rates". C9 is uniform random byte replacement: only 3.1% of changes are single-bit (OBS-1000). Keep NAND rates for a separate bit-flip model, but the C9 fixture must be one random byte per damaged Flate stream plus non-Flate hits (OBS-0005). Add CR-before-LF-before-`endstream` data endings to the stream-extent fixtures (GAP-301).
3. **WP-2.2 Settle the contradictions.** It currently says "GAP-013 (settled by OBS-0001..0005; close it in the register)". Don't close it yet:
   - fix and re-run flate_c9_probe.py (OBS-1012's 3-stream undercount);
   - verify CLM-1035 (Table 2 vs Table 5) against SRC-0001;
   - reword "11-28 streams" as Flate vs all bodies.
4. **WP-1.4 Engine matrix.** lopdf and hayro are already planned checkouts. Add rows, with code citations, for PR #16's unverified engine rules:
   - CLM-1041, 1042, 1049 (lopdf);
   - CLM-1048, 1051 (hayro);
   - CLM-1050, 1052, 1053 (qpdf);
   - CLM-1056 (xref-stream trailer harvest).

   Add a row "stream extent when data ends in CR" (GAP-301).
5. **WP-4.2 Product flags → ADRs.** The "last copy wins" flag should name the candidate replacement "last well-formed copy wins (pdf.js, CLM-0517; PR #16 CLM-1046)" and be decided on HYP-301's test, not on PR #16's argument. The "Adler-32 acceptance for C9" flag should cite OBS-1007, 1009 and 1013 (measured false accepts).
6. **WP-3.5 REPDF replication.** PR #16's stream-level gains are not on REPDF's metric (CLM-1032, CLM-1062). Add a per-stream to per-file attribution run on C9 (HYP-302, GAP-300).
7. **miniz_oxide.** PR #16's "re-check 0.9" caveat is resolved (OBS-1005, OBS-1015): `DecompressorOxide` is still `Clone` in 0.9.1 at 10,504 bytes, and mzbench builds and runs unchanged against it. No plan change is needed beyond citing these.

## 4. What this ingest did not check

- **Unverified PR #16 measurements, which need committed scripts:**
  - E6 (TrueType localizer, CLM-1026);
  - E8 (C10 census, CLM-1033/1034);
  - the ObjStm/BaseFont corpus scan (CLM-1036..1038).
- **Engine rules** (CLM-1041..1056): no code was read for them in this ingest.
- **Other claims:**
  - CLM-1035's REPDF table numbers were not re-checked against the paper;
  - the papers that PR #16 cites second-hand (CLM-1054, 1058, 1063) were not fetched.
- **mzbench:**
  - the 239-case sample of mid-size streams that PR #16 used was not recorded, and we ran a stride-10 sample;
  - we did not run a trailer-inclusive variant of mzbench (the 245/245 reading is inferred from OBS-1013);
  - we did not count all passing candidates per stream, so uniqueness was not tested.
- **One-byte residual between OBS-0302 and OBS-0300, not examined:** OBS-0302 has 1,443 Flate-body bytes; OBS-0300 has 1,442 damaged streams.
