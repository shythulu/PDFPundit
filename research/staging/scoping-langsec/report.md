# scoping-langsec report (block 2)

*The agent couldn't write report files, so the chair saved its hand-back text here.*

Scope: LangSec, parser differentials, SafeDocs, content disarm and reconstruction (CDR), and real-world malformed-PDF prevalence. Date: 2026-09-27.

## Counts
- **Screening:** 39 titles, four over the 35 cap. Three of the extras came from checking whether the gaps are still open.
- **Full texts:** 4 read in full: Caradoc, Shadow Attacks, Kuchta et al. 2018 and Robinson 2021. Two claims were also taken from Allison 2020, one paper over the read quota, because it has the only peer-reviewed truncation rate.
- **Claims:** 70, of which 67 are verified quotes.
- **Records:** 7 gaps (GAP-100..106), 2 proposed damage classes (DMG-100, DMG-101), 2 observations (OBS-0200, OBS-0201).
- **Paid searches:** 4 of 6 (SRCH-0200, 0206, 0207, 0208).

## What I did
- **Search route:**
  - Started from the seed paper's Crossref reference list (SRCH-0201).
  - Pulled OpenCitations citing works for Caradoc (SRCH-0202).
  - Swept the IEEE S&P LangSec workshop through Crossref (SRCH-0203).
  - Checked the LangSec 2016–2023 programme pages.
  - Ran Crossref title lookups (SRCH-0204).
  - Checked whether the gaps are still open through OpenCitations citers of SRC-0201 to SRC-0205 (SRCH-0205).
- **Duplicates:** SRC-0200, SRC-0229 and SRC-0231 carry `duplicate_of` (SRC-0112, SRC-0320, SRC-0411), because other agents merged the same works meanwhile.

## Key findings
1. **Two real-world damage sources and one natural-pair route.**
   - Common Crawl truncation: CLM-0270, CLM-0280..0284, OBS-0201. 22% of PDFs in the December 2019 crawl were cut off. In the July/August 2021 crawl 2,020,913 PDF URLs were cut, and SafeDocs later refetched complete copies of 1,922,505 of them.
   - Pairing test (OBS-0201): 40 rows, 35 completed. In 34 the truncated capture is an exact byte prefix of the complete file, always cut at 1,048,576 bytes. The failing pair was a different document at the same URL, so every pair needs a prefix check before use.
   - UNSAFE-DOCS also ships truncated Common Crawl files (CLM-0297).
   - The Issue Tracker corpus (over 32K bug-report PDFs; CLM-0293, CLM-0294) has no originals, so it counts for robustness only.
2. **Divergent repair is an attack surface.** Without rules for how to repair, parsers repair differently and crafted files exploit that (CLM-0263). Engine-level detail is in GAP-250 (engine-study).
3. **Shadow attacks are standard-compliant.** They reuse object numbers and flip xref entries between in-use and free (CLM-0221..0224). The only detector compares the signed revision with the final one (CLM-0225). It was tested on 26 self-made files (CLM-0226).
4. **Normalizers drop unreachable objects by design** (CLM-0203). Their equivalence to the original was checked only by hand (CLM-0206).
5. **Engine agreement is a weak check.** 13.5% of intact files already disagree across readers (CLM-0240), and failures shared by every engine are invisible (CLM-0245).
6. **How common malformed files are depends on the checker:** a strict parser accepts 1,465 of 10,000 web PDFs; experts rejected 1,794 of 9,000 crawled files; 13.5% of government PDFs render differently across readers; lenient parsers fail on under 0.5% of refetched files.

## Top gaps
- **GAP-104:** tamper detection has never been tested on files that are also damaged or repaired.
- **GAP-101:** a repair engine should report when a file allows more than one repair, because attackers exploit the choice.
- **GAP-100:** no repair tool has been evaluated on real truncated files with known originals.

## Evidence to link into existing records
| Record | Evidence |
|---|---|
| GAP-001 | CLM-0270, CLM-0280..0284, CLM-0293, CLM-0294, OBS-0201 |
| GAP-006 | OBS-0201 |
| GAP-012 | CLM-0240, CLM-0242..0245, CLM-0249, CLM-0266 |
| GAP-015 | CLM-0220..0228, CLM-0263; can move to supported |
| GAP-016 | CLM-0223, CLM-0225, CLM-0203, CLM-0206 |
| GAP-002 and DMG-011 | CLM-0204, CLM-0205 |
| DMG-013 | CLM-0288, OBS-0200 |
| DMG-016 | CLM-0212 |
| DMG-008 and DMG-014 | CLM-0202 vs OBS-0200 (the contradiction) |

**DMG-009 templates:**
- **Shadow Hide:** set the overlay object to free in an appended xref.
- **Shadow Replace:** a form-field `/BBox` overlay, or a replaced font.
- **Shadow Hide-and-Replace:**
  - two objects share one number, and an appended xref re-points it (CLM-0222, CLM-0223);
  - or an appended xref flips objects between in-use and free (CLM-0224).
- **Malformed incremental update:** objects missing or not closed properly (CLM-0230).
- **Name-escape split:** `/Foo#7/Bar` read as one name or two (CLM-0208).
- **Generation numbers:** undefined, negative or 65535 (CLM-0209).
- **Duplicate dictionary keys** (CLM-0210).
- Apply each to known originals, alone and combined with DMG-008 or C2 damage.

## What I did not check
- **The seed paper's full text:** paywalled, and its preprint returns 403 here.
- **CDR and related papers not read:** SRC-0215 (Dubin, open access; code at randubin/PDF-CDR), SRC-0219, SRC-0223/0224, SRC-0218, SRC-0208 and SRC-0207. GAP-103, 105 and 106 stay `unknown` until these are read.
- **pdfa.org material (403 here):** the stressful-corpus pages, the PDF Days 2022 slides ("16% warnings, 25% errors"), and the claim that 25.6% of PDFs in the 2023 crawl are truncated. All are leads only.
- **Access questions:**
  - Whether Robinson's labelled test data is public. It separates correctable from uncorrectable malformations (CLM-0267).
  - The Issue Tracker and UNSAFE-DOCS files were not downloaded.
- **Title-only screening:** papers screened by title only, and summaries marked "(memory, unverified)", are leads.
- **Scope of OBS-0201:** it covers one ZIP (35 pairs). Five Common Crawl requests failed with 403 throttling.

## Harness problems
- `polite_get` crashed on HTTP 429 with an `UnboundLocalError`. This is fixed in commit d9de5a3.
- The scratchpad is shared between agents; per-agent subdirectories would prevent the collision below.

## Decisions for the chair
1. **Common Crawl pairs:** adopt the prefix-verified pairs as a real-world source.
2. **Experiment scripts:** move the two scripts from staging into `research/experiments/` and update the paths in OBS-0200 and OBS-0201.
3. **DMG-009:** update its generator spec with the attack templates above.
4. **Incremental-update contradiction:** most files in Caradoc's 2016 set use incremental updates, but only about 26% in the 2021 sample. Settle this before fitting damage rates.
5. **Scratchpad clash:** I accidentally ran scoping-compression's `build_registry.py` twice. It rebuilds their staging registry from their spec file, but it resets `notes_path`.

## Next steps
- Scale OBS-0201 to a stratified sample across ZIPs, including disconnect truncations, which cut at arbitrary points rather than at the cap.
- Run qpdf, MuPDF, pdf.js and a carving baseline on the verified pairs.
- Read the unread sources listed above.
- Ask the PDF Association for the File Observatory's parser-error tables and the SafeDocs test-data labels.

*Chair's notes, 2026-09-27:*
- (2) Done before merge; the OBS paths now point at `research/experiments/`.
- (5) Scoping-compression was already merged (commit 304313f) before the clash, and its staging directory shows no change since, so nothing was lost. The brief now gives each agent its own scratch subdirectory.
- (3) Applied at merge; see committee 002 minutes.
- (1) and (4) go to the 10k revision; contacting the PDF Association is the user's call.
