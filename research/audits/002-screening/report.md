# Audit 002: independent screening audit (report)

*The auditor returned its report as a hand-back; the chair saved it here. Scores are from
`python3 research/tools/screening_audit.py score --audit research/audits/002-screening`.*

Date: 2026-09-27. Blind: the auditor saw only bibliographic fields (`rescreen-sample.jsonl`), never
the original decisions, reasons or summaries (`key.jsonl` was kept outside the repository until scoring).

## Task A: blind re-screen (stratified 20% sample)
- 32 rows, one of them an exact duplicate (SRC-0132, screened by two agents before a merge fold), so 31 works.
- Auditor decisions: 25 include, 5 maybe, 2 exclude.
- Stage reached: 6 full text, 9 abstract, 17 title only. No abstract was reachable for 17 rows:
  IEEE Xplore (JavaScript challenge), ScienceDirect (redirect stub), Springer (fingerprint challenge)
  and ACM (403) blocked them, so they were screened on title and venue.
- Maybes: SRC-0330, 0337, 0323, 0414, 0111 (methodologically adjacent, but a different format or field).
- Excludes: SRC-0120 and SRC-0108 (vendor homepages with no URL; EX-weak).

**Scores:**
| Measure | Agreement | Cohen's kappa | PABAK (chair) |
|---|---|---|---|
| 3-way (include / maybe / exclude) | 77% (24/31) | 0.17 | 0.66 |
| include vs not | 84% (26/31) | 0.38 | 0.68 |

Confusion (original → auditor): include→include 24, include→maybe 3, exclude→maybe 2, include→exclude 2.

## Task B: claim spot-check (10 random verified claims)
- **9 faithful, 1 overstated.** CLM-0138 (CPR): the paraphrase added "independent of the xref",
  which the quote never states; a search of the cached paper for "xref" found one unrelated mention.
- **Raw-source check:** for CLM-0325 (SRC-0302, p. 8), CLM-0251 (SRC-0204, p. 30) and CLM-0203
  (SRC-0202, p. 11) the auditor fetched the raw PDF itself and ran `pdftotext -layout` on the stated
  page. Each reproduced the cached quote verbatim: no page-number or extraction drift.
- **Not raw-checked:** CLM-0402, 0004, 0014, 0116 (Elsevier, no open route) and CLM-0302 (a DFRWS
  presentation page, not a PDF). The minimum of 3 was met.

## Systematic issues the auditor raised
1. The duplicate sample row (SRC-0132).
2. Publisher bot walls made abstract-stage screening impossible for most IEEE, Elsevier, Springer
   and ACM rows. Of about 10 Unpaywall links tried, 2 gave a working PDF.
3. No dropped hedges or misattributions beyond CLM-0138.

## Blindness slip (disclosed by the auditor)
A `Read` of `research/charter.md` meant for §1–5 returned the whole file, so §6–7 and the revision
log were seen. They hold aggregate counts, not per-work decisions. The auditor judged that it did not
change any decision; the chair agrees.

## Chair's actions
- CLM-0138: paraphrase corrected to what the quote says.
- `screening_audit.py sample` now samples each work once.
- The other points are answered in `research/committees/002-scoping-sweep.md`.
