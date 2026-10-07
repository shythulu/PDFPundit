# Stage 2 tooling report — W3 damage realism, corpora, generators (agent C)

Block 8 (TOOL-400..427, SRC/CLM/OBS/SRCH-0800..0814). Checked 2026-09-27.

## Ledger summary

| ID | Name | Verdict | Verified |
|---|---|---|---|
| TOOL-400 | warcio | Assess | untested |
| TOOL-401 | cdx_toolkit + Common Crawl CDX server (index.commoncrawl.org) | Hold | untested |
| TOOL-402 | digitalcorpora unsigned-S3 range-fetch technique | Adopt | pass |
| TOOL-403 | data.commoncrawl.org WARC range-fetch access | Hold | fail |
| TOOL-404 | CC-MAIN-2021-31-PDF-UNTRUNCATED truncation corpus | Trial | partial |
| TOOL-405 | UDHR natural-pairs sources: OHCHR PDFs + unicode.org/udhr texts | Hold | fail |
| TOOL-406 | Python difflib (stdlib) for text-alignment | Assess | untested |
| TOOL-407 | Damage-model parameter sources: NAND bit-flip (enssec) + NTFS fragmentation (SRC-0137) | Assess | untested |
| TOOL-408 | NapierOne dataset | Trial | untested |
| TOOL-409 | DFRWS 2006 / 2007 carving challenge images | Assess | untested |
| TOOL-410 | Engine regression corpora licensing cross-reference (OBS-0500) | Assess | untested |
| TOOL-411 | SafeDocs Issue Tracker corpus (31GB) subset plan | Hold | untested |
| TOOL-412 | GovDocs1 corpus (digitalcorpora) | Trial | untested |
| TOOL-413 | LibreOffice (soffice headless) | Adopt | pass |
| TOOL-414 | Ghostscript ps2pdf | Adopt | pass |
| TOOL-415 | Chromium headless print-to-pdf | Adopt | pass |
| TOOL-416 | LaTeX / pdflatex (texlive-latex-base) | Adopt | pass |
| TOOL-417 | reportlab | Adopt | pass |
| TOOL-418 | pycairo (PDF surface) | Adopt | pass |
| TOOL-419 | Skia / skia-python | Assess | untested |
| TOOL-420 | macOS Quartz PDF output (DMG-201) sourcing | Hold | fail (infeasible on Linux) |
| TOOL-421 | radamsa | Adopt | pass |
| TOOL-422 | zzuf | Adopt | pass |
| TOOL-423 | peepdf-3 | Adopt | pass |
| TOOL-424 | Arlington PDF Model + TestGrammar | Trial | untested |
| TOOL-425 | RUB-NDS pdf-attacker (shadow-attack artefacts) | Trial | untested |
| TOOL-426 | pikepdf | Adopt | pass |
| TOOL-427 | Provenance/test-spec: NIST CFTT, DFXML, CASE/UCO | Trial | partial |

28 entries (brief suggested ~12-25; went slightly over given the breadth of 7 distinct subtasks, each with multiple real candidates worth recording individually rather than merging away distinctions the chair would otherwise have to re-derive).

## Top recommendations per need

1. **Truncation natural pairs (GAP-100).** Adopt the digitalcorpora unsigned-S3 range-fetch technique (TOOL-402): confirmed fast (range-fetching 2MB of the *full* provenance CSV, not just zip 0000's "-1k" file, yields >1000 rows/zip across zips 0000-0012 in ~1.3s). The stratified-sampler design (by zip, by truncation type) is validated as feasible within the 10GB target. **Blocker:** `data.commoncrawl.org` returned HTTP 403 on every WARC range-fetch attempt this session (TOOL-403/SRC-0811) — a change from OBS-0201's earlier success on the same access pattern. Not a proxy issue (confirmed via `$HTTPS_PROXY/__agentproxy/status`, no relay failure logged); the 403 comes from Common Crawl's origin. Recommend the chair re-check this on a later date/session before committing to the GAP-100 scale-up plan.
2. **Font-encoding natural pairs (GAP-202).** Both named sources are currently unusable from this container: OHCHR (403) and unicode.org/udhr (fully decommissioned — index page says hosting moved to OHCHR/UN, and all former data-file paths 404). This is worse than a simple access block; the unicode.org corpus itself appears gone. **Decision needed:** does a cached copy already exist from when CLM-0415 (21/526 corrupted) was produced? If not, GAP-202 may need a different reference-text source entirely.
3. **Damage-model parameters.** NapierOne (TOOL-408): license fully open (Edinburgh Napier University License, free commercial/non-commercial use), no AWS account needed — but this container's proxy cannot reach the bucket's multi-level S3 hostname (TLS/SNI gap, 4 patterns tried). Should work from a host without that specific proxy limitation. DFRWS 2007 (TOOL-409): carving-focused, builds format-specific validators incl. PDF — good fit for the under-covered classic-file-carving DMG class; challenge images themselves not sized/downloaded. NAND (enssec) license unconfirmed (LICENSE file 404s despite README claim); NTFS (SRC-0137) not re-verified this session.
4. **Robustness corpora.** SafeDocs Issue Tracker (31GB): out of the 1.5GB per-corpus cap, so not downloaded; a 4-part subset plan is in TOOL-411 and the PII screening procedure below. GovDocs1 (TOOL-412, public domain) is a lower-risk volume source than bug-tracker files.
5. **Producer diversity (GAP-002).** All 6 feasible producers now Adopted with hands-on evidence (OBS-0801): LibreOffice, Ghostscript, Chromium (also gives indirect Skia coverage), pdflatex, reportlab, pycairo. Standalone skia-python (TOOL-419) located but not installed (time budget). macOS Quartz (TOOL-420): infeasible in-container; best lead is SRC-0123's existing CPR dataset (not re-confirmed re: macOS provenance this session) or a contributor's own Mac.
6. **Damage generators (DMG-009/GAP-104).** radamsa and zzuf both Adopted, hands-on-verified against a real REPDF file with byte-diff evidence. Recommend Arlington's TSV grammar (TOOL-424, Apache-2.0) as a *grammar-aware* generator — better matched to DMG-009's templated damage than radamsa/zzuf's generic mutation — for a P4/P5 trial. RUB-NDS's `pdf-attacker` (TOOL-425, correct repo name; earlier notes had it slightly wrong) reproduces shadow-attack templates directly but has no LICENSE file (all-rights-reserved) — study/cite only, never vendor.
7. **Test specs/provenance (GAP-050/GAP-010).** Recommend DFXML (TOOL-427, CC0/LGPL, hands-on verified round-trip) for the low-level per-file record, layered with CASE/UCO (Apache-2.0, both at tag 1.5.0) for the higher-level repair-event ontology. NIST CFTT's template *structure* is reusable even though it has no PDF-repair category to draw content from (this specific claim carried over from a pre-compaction check, not re-verified this session — treat as memory-level).

## API-account list

**None strictly required.** Every source used this session was reachable either keylessly (PyPI/GitHub raw JSON, digitalcorpora's unsigned S3 API) or via a public registry page (registry.opendata.aws). No paid/keyed service would have unblocked this session's actual blockers (data.commoncrawl.org's 403, OHCHR's 403, unicode.org's decommissioning, NapierOne's proxy-side TLS/SNI gap) — those are host- or container-level issues, not account-gated.

## What was NOT checked

- `data.commoncrawl.org` WARC fetches: 403 today (SRC-0811); not retried with alternate headers.
- `index.commoncrawl.org` CDX server: connection failure (relay closes after ~6s); not retried.
- `www.ohchr.org`: HTTP 403; not retried.
- unicode.org/udhr data files: confirmed 404/decommissioned, not merely blocked.
- NapierOne direct S3 bucket listing: TLS/SNI failure through this container's proxy across 4 URL patterns; license/access-method facts came from the AWS Open Data registry page instead.
- NTFS fragmentation dataset (SRC-0137): not independently re-fetched this session (memory-sourced).
- Engine regression corpora (OBS-0500) per-engine licences: not freshly re-derived (memory-sourced list of likely licences noted in TOOL-410).
- DFRWS 2006 challenge images and DFRWS 2007 challenge images themselves: README-level characterization only; not downloaded or sized.
- SafeDocs Issue Tracker corpus and GovDocs1: not downloaded (31GB and ~unknown-but-likely-large respectively; only a subset plan and licensing note given).
- Skia: only indirect coverage via Chromium; standalone `skia-python` not installed/run.
- Arlington's TestGrammar (C++ conformance checker): not built/run (overlaps with the W3-B engine track's GAP-106 work).
- NIST CFTT's lack of a PDF/document-repair category: not re-verified with a fresh fetch this session (carried over from before compaction).

## Paid Parallel Search calls used

**2 of 7** (both spent before this session segment; exact query text for those 2 did not survive the compaction boundary, logged as SRCH-0814 with that caveat noted). **0 additional paid calls this segment** — everything needed came from free registry APIs (PyPI JSON, GitHub raw/`git ls-remote`), `polite_get.py` web fetches, and hands-on installs/builds, per the coordinator's cost-minimization instruction.

## Decisions for the chair

1. **GAP-202 is at risk beyond a tooling swap.** Both named natural-pairs sources (OHCHR, unicode.org/udhr) are currently unusable from this container, and the unicode.org corpus is not merely blocked but appears to have moved/been decommissioned entirely. Needs a decision on whether a cached copy exists, or a replacement source.
2. **`data.commoncrawl.org` may have started blocking this container/UA.** This affects not just this session's attempt to scale GAP-100 sampling to other ZIPs, but the future *reproducibility* of OBS-0201 itself. Worth a fresh check on a later date before committing engineering time to the "thousands of pairs" sampler.
3. **NapierOne and index.commoncrawl.org are blocked by container/proxy specifics**, not by license or account gating — both may simply work from a different host/session.
4. **peepdf-3 is GPL-3.0** (confirmed via its bundled COPYING, not previously nailed down): fine to run as an external CLI, must never be vendored.
5. Ledger has 28 entries (vs. the ~12-25 suggested range) — flagging in case the chair wants some merged at integration time; each is individually small and cites concrete evidence.

## PII screening procedure (for SafeDocs Issue Tracker / bug-tracker files, per task 4)

Before any bug-tracker-derived file (e.g. a pdf.js `.link` attachment) is cached or committed:
1. **Never fetch based on filename alone.** Bug-tracker attachment names are often literally a reporter's real document name (invoices, resumes, scanned forms, medical/legal correspondence) — screen the *filename* first and skip anything that looks personal before downloading.
2. **Check the issue/PR metadata**, not just the file: does the tracker entry itself name a private individual, an employer, an address, or other identifying context in the title/description/comments? If so, skip the attachment even if the file itself looks anonymous.
3. **Inspect embedded metadata before caching**: PDF `/Info` dictionary (Author, Title, Creator) and any XMP packet frequently carry a real name, email, or company — scan these with a lightweight structural tool (e.g. pikepdf or peepdf-3, run standalone, never vendored) and reject/redact on a match.
4. **Sample visible text and page-1 content** for names, addresses, phone numbers, SSN/ID-number-shaped strings, or letterhead before wider use, not just metadata (a scrubbed Info dict does not guarantee a clean body).
5. **Never cache a rejected file, even temporarily** — check before download where possible (e.g. via the tracker's own attachment metadata/preview), not after.
6. **Log exclusions, not the excluded content**: record only the tracker issue ID and "excluded: PII" — never the file itself or the specific PII found — in any staging note.
7. **Default to exclusion under uncertainty.** A subset plan (TOOL-411) should treat "cannot confirm clean" as "exclude," not "include unless proven dirty."

## Cleanup

Large hands-on installs (`libreoffice-writer` + deps, `texlive-latex-base` + deps, the scratch venv, radamsa build artefacts) are removed at the end of this session per the ≤4GB/session budget; their evidence is preserved in OBS-0801 and the TOOL-413..427 entries above, not re-derivable from disk after cleanup.
