# scoping-forensics report (block 1)

*The agent couldn't write report files, so the chair saved its hand-back text here.*

Scope: digital forensics, file carving, tool validation, and REPDF's backward and forward citation snowball. Date: 2026-09-27.

## Counts
- Screened 38 titles (3 over the 35 cap, because all 23 REPDF references had to be screened); 32 included.
- Read 4 full texts: SRC-0126, SRC-0127, SRC-0129, SRC-0132.
- 39 claims, all 39 verified exact.
- 6 gaps (GAP-050..055) and 1 new damage class (DMG-050).
- Paid calls: 2 of 6.

## Method
- **Backward snowball:** REPDF's 23 references (SRCH-0100, 0101). 15 have DOIs; Al-Saleh 2013 and Al-Sharif 2015 aren't in Crossref; 6 are web tools, services or the standard.
- **Forward snowball:** 113 citing works via OpenCitations (SRCH-0102). Kept Hughes 2024 and Boiko 2023.
- **Venue sweep:** 13 Crossref queries on the two Digital Investigation journals, 187 titles (SRCH-0103). This found CPR, Brown 2013, Barral 2022, Force Open, Casey 2019, Horsman, Lyle and the certainty-descriptor paper.
- **Topical queries:** tool validation and admissibility (SRCH-0104).
- **Still-open checks:** OpenCitations on 12 works (SRCH-0105).
- **Open-access lookups:** Unpaywall (SRCH-0107).
- **Paid calls:** SRCH-0106 found the CPR PDF on dfrws.org; SRCH-0108 fetched the CPR excerpts.

## Top gaps
1. **GAP-052:** a checksum match doesn't make a correction unique. In a real zlib repair, several candidates passed Adler-32, up to 40,717 per fragment. This contradicts the charter's grade 3 ("Adler-32 match = verified").
2. **GAP-051:** REPDF and CPR both carve objects by obj/endobj signature and object number, so objects from another document or revision can get mixed in (DMG-050).
3. **GAP-050:** NIST's tool-testing programme has no test spec or reference data for PDF repair, which the P6 gate needs.
4. **GAP-054:** nobody reassembles out-of-order fragmented PDFs using PDF's own syntax. `still_open` is unknown until Hughes 2024 is read.
5. **GAP-053:** nobody has measured how much repair engines complement each other; in one study, combining two tools repaired 31% more PDFs.

## Existing gaps with new evidence
- GAP-010, supported and extended: CLM-0111, 0121, 0123, 0105.
- GAP-005, supported: CLM-0100, 0102, 0108, 0131, 0136.
- GAP-001: CLM-0116, 0128. GAP-008: CLM-0123, 0127. GAP-009: CLM-0114, 0115. GAP-015 (weak): CLM-0118.
- Realism for DMG-006 (CLM-0134) and DMG-007 (CLM-0109, 0110).

## Real-world damage sources
- NAND chip-off dumps read without ECC: sparse bit flips, 204 of 920 fragments corrupted (SRC-0126).
- NTFS fragmentation: 51% of DOCX files in 3+ fragments, 72% of those out of order (SRC-0137, figures via SRC-0132).
- NIST: carved or recovered data can be partly overwritten or mixed from several sources (SRC-0129).

## What I did not check
- Hughes 2024 (blocked with 403) and Casey 2019 (paywalled), plus other paywalled or blocked works.
- NIST's live tool-testing catalogue, SWGDE and ISO/IEC 27041.
- The legal admissibility literature.
- About 290 titles rejected at a glance were not logged.

## Leads for other agents
- Brown 2011 (10.1016/j.diin.2011.05.015)
- Park et al. 2008, damaged compressed-file extraction
- Docovery (Kuchta et al., ASE 2014)
- DFXML (10.1016/j.diin.2011.11.002) and CASE (10.1016/j.diin.2017.08.002)
- Searching Corrupted Document Collections (10.1109/das.2016.28)
- PDF format-aware reducer (SPW 2022)
- Scalpel3 (2026)
- NapierOne (10.1016/j.fsidi.2021.301330)
- Hargreaves et al. 2024 (10.1016/j.fsidi.2023.301679)
- Old Wine in New Wineskins (10.1145/3769683)
- NIST's 2014 file-carving tool specification and test plan

## Decisions for the chair
- Set `duplicate_of` at merge:

  | Mine | Other staging |
  |---|---|
  | SRC-0123 | SRC-0402 (fonts) |
  | SRC-0125 | SRC-0301 (compression) |
  | SRC-0126 | SRC-0338 (compression) |
  | SRC-0127 | SRC-0319 (compression) |
  | SRC-0132 | SRC-0318 (compression) |
  | SRC-0135 | SRC-0308 (compression) |
  | SRC-0112 | SRC-0200 (langsec) |
  | SRC-0119 | SRC-0400 (fonts) |

- Add fonts CLM-0409, 0410 and 0446 to GAP-050, 053 and 054.
- Revise charter grade 3.
- Use CPR as a runnable baseline: its code is public (fonts CLM-0412).
- Ask the user for Hughes 2024 and Casey 2019.
- `polite_get.py` has a bug: a 429 retry raises `UnboundLocalError`, so it never backs off.
- Add Unpaywall (keyless) to the brief's list of free APIs.

*Chair's note, 2026-09-27:*
- The `polite_get` bug was fixed in commit d9de5a3, with a regression test.
- The duplicates fold automatically at merge through their dedupe keys; the chair checks the fold count against the table above.

## Suggested next steps
- Read Hughes 2024 and settle GAP-054.
- Get Casey 2019 for GAP-010.
- Prototype a PDF-repair test spec built on NIST's error classes (GAP-050).
- Run the candidate-multiplicity experiment on C9 and DMG-007 files (GAP-052).
