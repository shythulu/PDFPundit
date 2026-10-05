# scoping-fonts report (block 4)

*The agent couldn't write report files, so the chair saved its hand-back text here.*

Scope: font identification, glyph→Unicode recovery, OCR and document AI, and evaluation of text extraction. Date: 2026-09-27.

## What I did
- **Seed:** DAS 2018 (SRC-0400, `10.1109/das.2018.64`).
  - OpenCitations v2 lists 0 citing works, and Crossref's count is also 0, so a forward snowball was impossible.
  - Snowballed sideways instead, through the cipher-OCR line (Ho & Nagy 2000, Huang 2007, Kae 2009/2011, and their citers) and the extraction-benchmark line (Bast & Korzen 2017, Meuschke 2023, Clausner 2020, and their citers).
- **Searches:**
  - Crossref: 8 logged free query groups (SRCH-0403..0410).
  - arXiv: metadata taken from abs pages (the export API returned 406 at the time).
  - Parallel Search: 4 of 6 paid calls (SRCH-0400, 0401, 0402, 0411).
- **Screening:** 35 titles; 31 included, 2 maybe, 2 excluded.
- **Read in full, with notes:** 4 open-access papers (SRC-0401, 0402, 0422, 0427).
- **Recorded:** 47 claims (45 with verified quotes, 2 critiques), 6 gaps and 2 damage classes.

## Key findings
1. **CPR (SRC-0402, DFRWS EU 2026) is the nearest competitor to our font path.**
   - It verifies candidate decodings with a local Llama 3.1:8B that answers yes/no on how natural the text reads (CLM-0401).
   - With both ToUnicode and FontFile removed, it recovers 33.3% of files at a character error rate of 0.048 (CLM-0402). The authors attribute most errors to the LLM verification step (CLM-0403).
   - Its font database contained every test font (CLM-0407).
   - It counts any partial recovery as success (CLM-0409).
   - Its baselines are web tools only (CLM-0446).
   - Its code is CC BY-NC, so it can serve as a research baseline only.
2. **macOS character codes.**
   - macOS assigns character codes sequentially from 21 (CLM-0411).
   - Once ToUnicode is lost, CPR can't recover these (CLM-0404).
   - Leads to GAP-200 and DMG-201.
3. **Language priors fabricate text.**
   - In VLM OCR, olmOCR-2-7B rewrote 58.63% of scrambled words (CLM-0429).
   - The errors spread to untouched words, by up to 7.3× (CLM-0430).
   - A "don't correct" prompt removes only 30–50% of the effect (CLM-0431).
   - The same effect has never been measured for font-map decoding. Leads to GAP-201.
4. **Real encoding damage comes with natural pairs.**
   - 21 of 526 UDHR translations (4%) are corrupted born-digital PDFs (CLM-0415).
   - Many have matching Unicode texts at unicode.org (CLM-0416).
   - Real code mappings are many-to-one (CLM-0427).
   - Leads to GAP-202 and DMG-200.
5. **Extraction metrics can't see a single wrong glyph.**
   - Meuschke counts a token as correct at a Levenshtein ratio of at least 0.7 (CLM-0437), so a 5-letter word with one wrong glyph still passes (critique CLM-0444).
   - Its ground truth was itself produced by PDFMiner and PDFPlumber (CLM-0439).
6. **No academic work was found on identifying subset fonts in PDFs without their names** (SRCH-0402).
   - This is an absence of evidence, so confidence is low.
   - The nearest transferable result is that glyph widths leak text (SRC-0411).

## New gaps
| id | title | type | evidence |
|---|---|---|---|
| GAP-201 | Language-prior fabrication in glyph→Unicode recovery has never been measured | evaluation-weakness | CLM-0429, 0430, 0431, 0405, 0403, 0012 |
| GAP-202 | No evaluation on naturally corrupted encodings (UDHR natural pairs) | untested-condition | CLM-0415, 0416, 0427, 0410, 0407 |
| GAP-200 | Codes assigned by the producer defeat font-database mapping | stated-limitation | CLM-0404, 0411, 0402 |
| GAP-205 | Decipherment of code sequences hasn't been tried for PDF recovery | cross-field-transfer | CLM-0418, 0414, 0402 |
| GAP-204 | No calibrated per-mapping confidence for provenance grading | method-weakness | CLM-0401, 0405, 0403 |
| GAP-203 | Logical-order reconstruction for Indic and vertical scripts | stated-limitation | CLM-0422, 0423 |

## Existing gaps with new evidence
- **GAP-003:**
  - CPR's font database is also closed-world (CLM-0407).
  - No literature found on name-independent font identification in PDFs.
  - Image-based candidates: DeepFont and HanFont (SRC-0413, SRC-0414).
- **GAP-004:**
  - Devanagari needs reordering after decoding (CLM-0422).
  - VLM faithfulness hasn't been tested on Arabic or Devanagari (CLM-0432).
  - Arabic sub-word language models (SRC-0417) could replace dictionary scoring.
- **GAP-008:**
  - Claims CLM-0409, 0435, 0437–0440, 0442, plus critique CLM-0444.
  - Candidate metrics: flexible character accuracy (SRC-0428); page-level metrics that separate recognition errors from reading-order errors (SRC-0430); an OCR-metric survey (SRC-0429).
- **GAP-009:** CPR compares only against web tools (CLM-0446).
- **GAP-010:** CPR's LLM-accepted mappings carry no provenance (critique CLM-0445).
- **GAP-001 and GAP-002:**
  - UDHR as a real-world damage source (CLM-0415, 0416).
  - DocBank covers a single producer family (CLM-0443).

## Local models and datasets (fetched 2026-09-27)
Licences for Hugging Face items come from their model cards. The tooling stage should verify them.

| Item | Licence | Where | Note |
|---|---|---|---|
| olmOCR-2-7B-1025 | Apache-2.0 | huggingface.co/allenai/olmOCR-2-7B-1025 (sha e52d6f09) | Rewrites 58.63% of scrambled words |
| olmocr toolkit | Apache-2.0 | github.com/allenai/olmocr | |
| LightOnOCR-2-1B | Apache-2.0 | Hugging Face (lightonai) | |
| PaddleOCR-VL-1.6 | Apache-2.0 | Hugging Face (PaddlePaddle) | |
| DeepSeek-OCR-2 | Apache-2.0 | Hugging Face (deepseek-ai) | |
| MinerU2.5-Pro-2605-1.2B | Apache-2.0 | Hugging Face (opendatalab) | The 2509 build is AGPL-3.0, so pin the version |
| nougat-base | CC-BY-NC-4.0 | Hugging Face (facebook) | Not usable in the product |
| Tesseract, docTR | Apache-2.0 | GitHub LICENSE files | Lowest rewrite rates |
| FaithC4 | Apache-2.0 | github.com/gwanglee/DoVLMsRead | Planted-perturbation test set |
| OmniDocBench | Apache-2.0 (repository) | github.com/opendatalab/OmniDocBench | |
| CPR code, dataset and font database | CC BY-NC 4.0 | github.com/BeenyHail/CPR | Research use only |
| pdf-cmap-fixer, pdf-glyph-mapping (Rust) | MIT (from search metadata; not fetched) | GitHub | |
| UDHR PDFs and unicode.org/udhr texts | Not checked | ohchr.org, unicode.org | Candidate natural pairs |

## What I did not check
- **The DAS 2018 paper itself** (IEEE, paywalled). What I know of it comes from CPR's and Stefanovitch's descriptions.
- **Papers seen only by title or abstract:** cipher OCR, font recognition, and Arabic/Indic OCR.
- **Not read at all:** olmOCR, OmniDocBench, READoc and KITAB-Bench.
- **Citation counts:** OpenCitations reporting 0 citing works may just reflect patchy coverage of IEEE and 2026 papers. Semantic Scholar and OpenAlex weren't used.
- **Not searched:** literature in Chinese or Japanese, and patents.
- **Not verified:** the Llama 3.1 licence, and whether the UDHR pairs actually match.

## Tooling problems
- **`polite_get.py`** crashed with UnboundLocalError on a 429 instead of backing off.
- **`kb_validate.py --online`** called Crossref without pacing and got 429s.
- **arXiv:** the export API returned 406 at times.
- **GitHub:** `api.github.com` returns 403, while `raw.githubusercontent.com` works.
- **`merge_staging.py`** copies only `fulltext_sha256` when folding duplicate sources. CPR is staged three times (SRC-0123, SRC-0325, SRC-0402).

*Chair's note, 2026-09-27:*
- The first three are fixed: `polite_get` backs off correctly and has a regression test; the validator now paces its Crossref calls through `polite_get`; arXiv's 406 is treated as retryable.
- The merge now carries all cached-text fields together.

## Decisions for the chair
1. Fold CPR into one record. I suggest treating it as a second replication and baseline reference alongside REPDF.
2. Should UDHR become real-world damage source #1 under charter §3(a)? It needs a check of the OHCHR terms and a verification pass on the pairs.
3. Add a planted-token faithfulness test to the metric suite. Text from any decoder that relies on a language prior would default to provenance grade 6.
4. Keep GAP-203 separate, or fold it into GAP-004?
5. Fix the tools above before the next round.

## Next steps
- Read DAS 2018 and Scharwächter & Vogel 2015.
- Prototype HMM decipherment on REPDF C6 files with ToUnicode stripped (GAP-205).
- Build DMG-201 samples from macOS Quartz output.
