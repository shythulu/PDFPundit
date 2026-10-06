# Text without a font (outlines, Type3, images) and OCR

Scope: text that reaches the page without character codes shown through a font, and what OCR can do about it. Prompted by the question "does the plan account for fonts or words converted to SVG?" PDF has no native SVG, so text that began as SVG (or as SVG-in-OpenType glyphs) arrives as one of three things: filled vector paths (outlines), Type3 glyph procedures, or an image. The plan was read first (technical design §4.2, §5, §8, §15, §19; desktop-app plan Non-Goals, crate table, Pundit+). Measurements were made on the REPDF originals (`github.com/dfrc-korea/REPDF`, `original/`) on 2026-10-04. The scripts are in [`../../experiments/`](../../experiments/README.md).

---

## 1. How much REPDF text is drawn without a font?

### Takeaway
Microsoft Print to PDF draws two of the corpus's 44 fonts as vector outlines: GmarketSansTTFMedium and BousungEG-Light-GB, the only two whose embedding permission (OS/2 `fsType`) is Preview & Print. Those two fonts account for **4.7% of all glyphs in the Print to PDF originals**: **16.5% on English pages** and **25.2% on Chinese pages**. They appear in **43 of the 50** Print to PDF originals. Save As files contain no outlines at all, and the corpus contains no Type3 fonts.

### Measured findings
- **Census** ([outline_census.py](../../experiments/outline_census.py)): Save As originals contain no curve segments anywhere. Print to PDF originals contain filled curve paths. No original in either set uses a Type3 font.
- **Glyph counts per page language** ([glyph_count.py](../../experiments/glyph_count.py)). Glyphs shown with text operators count 2 bytes per glyph for Type0 fonts and 1 byte otherwise. Contours are counted in filled paths with ≥3 curve segments.

  | Page language | Save As glyphs | Print glyphs as text | Share as text | Outline contours (Print) |
  |---|---:|---:|---:|---:|
  | en (page 1) | 75,940 | 63,415 | 83.5% | 15,673 |
  | zh | 23,436 | 17,530 | 74.8% | 19,999 |
  | hi | 61,690 | 61,690 | 100.0% | 0 |
  | es (page 4) | 82,188 | 82,188 | 100.0% | 0 |
  | fr (page 5) | 84,244 | 84,244 | 100.0% | 0 |
  | ar | 64,222 | 64,214 | 100.0% | 0 |
  | **All** | **391,720** | **373,281** | **95.3%** | **35,672** |

  The 8-glyph Arabic difference is a shaping difference between the producers, not outlining (0 contours).
- **Which fonts** ([outline_fonts.py](../../experiments/outline_fonts.py)). Two fonts are outlined on every page that uses them, and every outlined page uses at least one of them:
  - **GmarketSansTTFMedium**: 29 pages, all outlined.
  - **BousungEG-Light-GB**: 30 pages, all outlined.
  - Outlined pages: 58 (28 English, 30 Chinese) in 43 documents. No outlined page lacks these fonts, and no page using them lacks outlines.
  - In Save As, these two fonts show 12,526 + 5,904 = 18,430 glyphs. The Print text layer is missing 18,439 glyphs. **All of their text is outlined**; the other 42 fonts' text is not.
- **A licence effect** ([font_fstype.py](../../experiments/font_fstype.py), added in review: fontTools over every embedded `FontFile2` in the Save As originals). All 44 fonts are TrueType (`glyf`). 25 have `fsType` 0 (installable embedding), 16 have 8 (editable), Peignot has the deprecated value 1, and exactly two have 4, **Preview & Print embedding**: GmarketSansTTFMedium and BousungEG-Light-GB, the two outlined fonts. The OpenType specification says documents containing Preview & Print fonts "must be opened read-only" ([OS/2 table, `fsType`](https://learn.microsoft.com/en-us/typography/opentype/spec/os2#fstype)). Word's Save As embeds them regardless; Print to PDF embeds the other 42 fonts as anonymous `CIDFont+F*` TrueType programs (1,252 font dictionaries, `fsType` 0, 1 or 8, never 4) and draws these two as paths.
- **Text-layer loss per document** ([outline_loss.py](../../experiments/outline_loss.py)): non-whitespace characters from pypdf `extract_text`, Print vs Save As, plus glyph-run paths per Print file.

### Inferences
- **Outlined text is untouched by font damage and exposed to stream damage.** C6–C8 delete font resources, programs or maps; outlined glyphs use none of them, so C6–C8 leave them intact. They still show only if the viewer draws the rest of the page despite the missing font and the repair copies the content stream verbatim rather than re-typesetting it. C5, C9 and C10 damage or drop content streams, and outlined glyphs go with them.
- **SVG in the strict sense does not occur.** A producer that receives SVG text or an SVG-in-OpenType glyph writes paths, a Type3 font or an image. Chrome's PDF backend (Skia), for instance, falls back to Type3 when a font is not embeddable (`kNotEmbeddable_FontFlag`), and also for variable fonts, bare CFF fonts and text drawn with a mask filter ([SkPDFFont.cpp, `SkPDFFont::FontType`](https://github.com/google/skia/blob/main/src/pdf/SkPDFFont.cpp)). Held-out and real-world files will contain all three forms even though REPDF contains only outlines.

### Gaps
- The embedding-permission trigger rests on a perfect split (2 of 2 Preview & Print fonts outlined, 0 of 42 others), not on Microsoft documentation of the Print to PDF driver. A test on fonts outside the corpus (a Preview & Print font that is not Korean or Chinese, and a Restricted License font, `fsType` 2, of which the corpus has none) would confirm it and show whether Restricted License text is outlined or dropped. Until then, `fsType` 4 is the predictor to use for files outside the corpus.
- The glyph-run heuristic (filled paths with ≥3 curve segments) had no false positives on REPDF because the documents contain no vector art. Its precision on files with logos, charts or diagrams is untested.

---

## 2. Where the current plan breaks

### Takeaway
Repair preserves outlined text. Everything that reads text through fonts or the text layer cannot see it: font inference, `/ToUnicode` rebuild, the evaluation metric and Markdown export. The typeless-stream classifier also misses outline-only streams.

### Findings (from reading the plan as it stood on 2026-10-04; §6 lists the changes made since)
- **Classifier**: technical design §4.2 step 5 and §15 classify typeless streams as content only if they contain `BT/ET/Tf/Tj`. A C5 tag-stripped stream from a page that draws text only as outlines would not classify as content. REPDF's own classifier has the same rule: "the presence of text-related operators (e.g., Tf, Tj, BT/ET) or an image rendering operator (e.g., Do) indicates a content stream" ([REPDF §3.4](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf)). On REPDF this does not matter, because outlined glyphs share streams with ordinary text. It matters for other producers.
- **Font pipeline**: §5.2 inference takes "hex-code runs (`<…> Tj`, `[<…>…] TJ`) per unknown font slot"; §5.4 builds `/ToUnicode` from used codes. Outlined text has no codes and no font slot.
- **Evaluation**: §8 extracts text with `hayro-interpret` from both files. Outlined words are absent from both, so the metric neither rewards keeping them nor penalises losing them.
- **Markdown export** (desktop-app plan M8) extracts through `hayro-interpret`. Outlined words vanish without any sign. OCR was deferred to Pundit+.
- **C9**: the 255-value search with an Adler-32 check works on any stream. The grammar localizer ([grammar_loc.py](../../experiments/grammar_loc.py)) already accepts path operators, but a corrupted digit inside a coordinate is still a valid number, so path-heavy streams localize less sharply. Not measured.

---

## 3. Does OCR recover outlined text? (measured)

### Takeaway
Yes. On Print to PDF pages that contain outlines, the text layer loses **29% of English words** and **44% of Chinese characters**. OCR of the same pages recovers the text to within a few points of OCR on the Save As original.

### Method
[ocr_outlined.py](../../experiments/ocr_outlined.py) and [ocr_summary.py](../../experiments/ocr_summary.py):
- Pages 1 (English) and 2 (Chinese) of all 50 document pairs: 100 pages.
- Ground truth: the Save As text layer, which is never outlined.
- Pages rendered with PDFium at 200 dpi.
- OCR: PaddleOCR PP-OCRv4 through RapidOCR 1.4.4 with onnxruntime. Models: Chinese/English detection (`ch_PP-OCRv4_det_infer.onnx`, 4.75 MB), Chinese/English recognition (`ch_PP-OCRv4_rec_infer.onnx`, **10.9 MB**) and a direction classifier (0.59 MB).
- Bag-of-words recall, pooled over pages: words `[a-z0-9]+` for English, Han characters for Chinese. Alphanumeric-character recall is also reported. Recall counts ground-truth tokens found in the output and ignores tokens the output adds or misreads into other words, so it bounds what OCR can recover and does not measure misreads or fabrication; REPDF's metric has the same shape (knowledge base [CLM-0024 and GAP-008](../../../../../research/gaps/register.md)). Save As extraction is a sound ground truth here: pypdf reads it with no ligature glyphs, two non-ASCII letters and eleven apostrophes in 10,444 English tokens, and the Print text layer scores 100.0% against it on every page without outlines.

### Results

| Page language | Print to PDF page | Pages | Tokens | Text layer | OCR of Print render | OCR of Save As render |
|---|---|---:|---:|---:|---:|---:|
| en (words) | no outlines | 22 | 4,462 | 100.0% | 90.0% | 91.8% |
| en (words) | outlined | 28 | 5,982 | **70.8%** | **87.3%** | 90.0% |
| zh (chars) | no outlines | 20 | 7,351 | 100.0% | 99.4% | 99.2% |
| zh (chars) | outlined | 30 | 10,672 | **56.4%** | **99.7%** | 99.7% |

Alphanumeric-character recall on outlined English pages: text layer 70.9%, OCR of Print 93.5%, OCR of Save As 94.1%.

Each document page is rendered and OCR'd twice (Print and Save As). The original run took about 2 s per rendered page on 4 vCPUs; the review re-run on the same class of machine took about 4 s per rendered page (79 s for 20 renders, uncontended). Neither number says anything about the tract build.

### Inferences
- **OCR closes the outlining gap to within the engine's own error.** On outlined pages OCR of the Print render is within 2.7 points (English words) and 0 points (Chinese) of OCR on the Save As render, which has no outlines.
- **English word recall is capped near 90% by the engine, not by outlining.** The same model reaches only 90–92% on clean pages, where the text layer is perfect, because the Chinese recognition model often merges adjacent English words. A Latin recognition model (PaddleOCR `en`, or `ocrs`) should be used for Latin-script pages. Character recall (93.5–95.3%) shows that most errors are spacing. Checked in review on three clean English pages: most tokens OCR added were concatenations of two to four ground-truth words (`greenhousegasemissions`, `systemsforfuturegenerations`), the rest misreads such as `AI` → `al` and `700` → `7o0`.
- **The text-versus-visual gap is a usable but incomplete detector.** Pooled, OCR recall minus text-layer recall is +16.5 points (English) and +43.3 points (Chinese) on outlined pages and negative on clean pages. Per page, a gap above zero flags 30 of 30 outlined Chinese pages and 20 of 28 outlined English pages, and none of the 42 clean pages; the 8 English misses have 2–16% of their words outlined, under this engine's own 8–10-point word error on Latin text. A Latin recognition model would lower that floor. (Per-page figures computed in review from `ocr/ocr_outlined.jsonl`.)
- **REPDF's metric counts this text.** REPDF scores both original and repaired files by OCR, so the 16.5% of English and 25.2% of Chinese glyphs that are outlined are part of its ground truth. A repair that drops a stream carrying outlined words loses points under REPDF's metric and none under the plan's text-layer metric.

### Gaps
- Arabic and Hindi pages were not OCR'd. They contain no outlines, and they need other recognition models (Arabic 73.6–81.3% line accuracy, Devanagari 96.4%; see [font_and_text_recovery.md §5](font_and_text_recovery.md)).
- The measurement used onnxruntime, which is C++. The product would use `tract-onnx` (pure Rust); op coverage and latency there are unmeasured.
- Repaired files were not OCR'd. The effect of each corruption class on outlined words is inferred, not measured.

---

## 4. Which OCR engine REPDF uses, and how to reproduce its metric

### Takeaway
REPDF uses the OCR engine of **Google Cloud Document AI**, not Tesseract. It OCRs the original and repaired files and reports word recall against the original's OCR. The processor version, the request options and whether it sent PDFs or Chrome renders are not stated.

### Cited findings
- "A Python script was developed to automate the evaluation process by using the OCR engine of the Google Cloud Document AI API (Google Cloud, 2025) through extracting words from repaired PDF files and comparing them with the originals. In other words, the OCR results of the original PDF files were used as ground truth and compared with those of the recovered PDF files" — [REPDF §5.3](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).
- "Matching is determined on a word-by-word basis; any differences in spelling or phrasing are considered recovery failures" — [REPDF §5.3](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).
- "While content rendering and error handling can vary across PDF viewers due to the PDF format's flexibility, the built-in viewer in the Google Chrome Web Browser was used for all assessments" — [REPDF README](https://github.com/dfrc-korea/REPDF). The repository holds no evaluation code.
- **Processor versions** of Enterprise Document OCR: `pretrained-ocr-v1.2-2022-11-10` (a frozen model "for up to 18 months"), `pretrained-ocr-v2.0-2023-06-02` and `pretrained-ocr-v2.1-2024-08-07` (stable), and `pretrained-ocr-v2.1.1-2025-01-31` (release candidate, not offered in the `US`, `EU` or `asia-southeast1` regions) — [Enterprise Document OCR](https://docs.cloud.google.com/document-ai/docs/enterprise-document-ocr); [Managing processor versions](https://docs.cloud.google.com/document-ai/docs/manage-processor-versions).
- **Native PDF parsing** is opt-in. With `enable_native_pdf_parsing=true`, Document AI extracts embedded text from digital PDFs and falls back to optical OCR only "in the regions when the PDF being processed contains non-digital text" — [Document AI release notes, 2022-12-19](https://docs.cloud.google.com/document-ai/docs/release-notes).
- **Price**: Enterprise Document OCR costs $1.50 per 1,000 pages from 1,000 to 5 million pages a month; the first 1,000 pages a month are free and pages above 5 million cost $0.60 per 1,000 — [Document AI pricing](https://cloud.google.com/document-ai/pricing).
- **Knowledge base**: `research/` records the same facts as CLM-0016 (Document AI OCR of the originals as ground truth) and CLM-0028 (exact word-by-word matching), and the metric critique as CLM-0024 and GAP-008 ([claims](../../../../../research/sources/claims.jsonl), [gaps](../../../../../research/gaps/register.md)); the 5k plan's WP-3.5 names the same choice between Document AI access and a documented local substitute. Nothing here contradicts them.

### Inferences
- **A REPDF-comparable number needs Document AI.** The paper's wording ("extracting words from repaired PDF files") suggests the PDFs were sent directly, so that Google's renderer decides what is visible; the README's Chrome remark leaves open that Chrome (PDFium) renders were sent instead. Send the PDFs directly with native parsing off, and on one release also send PDFium renders to see whether the two differ. Pin a processor version. Because REPDF states neither, exact replication is not guaranteed; report what was used.
- **Cost is negligible.** A full run over the corpus (1,000 repaired files × 6 pages, plus 100 originals × 6 pages) is about 6,600 pages, or about $10.
- **A local proxy is needed for CI.** PaddleOCR (as measured above) is free, deterministic and offline, and it uses the same models as the product's optional OCR. Calibrate it once against Document AI on the originals.
- **Data handling.** Only the public corpus goes to Google, never user files. The harness is dev tooling and sits outside the product's no-FFI rule.

### Gaps
- Whether REPDF's word matching is a bag of words or an ordered alignment is still not stated.
- Whether REPDF enabled native PDF parsing is not stated. If it did, the "OCR" scores partly measured the text layer, and outlined text was read by the fallback OCR.

---

## 5. OCR in the product

### Takeaway
OCR belongs in the plan as an optional, feature-gated module on `tract-onnx` with PaddleOCR models downloaded on first use. Its value, in order: exporting outlined, Type3 and image text; voting code→character maps for font inference; a text-versus-visual verification gate; scanned pages.

### Findings
- Pure-Rust engines: `tract-onnx` (the [`pure-onnx-ocr`](https://docs.rs/crate/pure-onnx-ocr/latest) crate, 0.2.1 of 2026-10-03, runs PaddleOCR PP-OCRv5/v6 DBNet detection and CTC recognition ONNX models on tract-onnx 0.23 with no C/C++ dependency; the PP-OCRv4 models measured above have not been tried on it), and `rten` + `ocrs` (Latin only, "early preview"). Tesseract, onnxruntime and MNN bindings are C/C++ — [font_and_text_recovery.md §5](font_and_text_recovery.md).
- Model sizes: detection 4.75 MB; Chinese/English recognition 10.9 MB (PP-OCRv4, measured above); Devanagari 7.9 MB; Arabic 7.8 MB; direction classifier 0.59 MB. With a Latin recognition model the set is about 35–45 MB (estimate).

### Inferences
- **Do not embed the models in the default binary.** Download them on first use, opt-in, checked against pinned SHA-256 hashes, with an `ocr-bundled` build for offline use.
- **Never change the forensic output with OCR.** OCR text goes to Markdown export and to an optional searchable copy with an invisible text layer. `<name>.repaired.pdf` stays a reconstruction of what the file contained.
- **Mark provenance.** OCR-derived text should carry its model and confidence, and reports should list pages whose text came from OCR.
- **Recovery without a model is worth a spike.** Outlined glyphs are scaled copies of the font's own outlines, and a TrueType quadratic converts to a cubic exactly. Matching normalized contours against candidate glyph outlines (skrifa) could identify glyphs exactly, with no OCR error. This is untested.

---

## 6. Ranked changes

1. **Detect and report** (v1, cheap): an `Info` finding per page for outlined text and Type3 glyphs. Markdown export writes a visible note instead of silently dropping words.
2. **Classifier** (v1, cheap): classify typeless streams by content-stream grammar (text **or** path operators), and add an outline-only C5 fixture.
3. **Retention gate** (v1, cheap): count path fills next to text-show operators when checking that repair did not drop content.
4. **Evaluation** (M7): REPDF's OCR word recall via Document AI per release and local PaddleOCR in CI; a per-file text-versus-visual gap; strata by creation method and outlined-glyph share.
5. **OCR module** (M9, feature-gated): tract + PaddleOCR; export of outlined, Type3 and image text; alignment oracle; text-versus-visual gate; scans.
6. **Geometric outline matching** (spike): exact glyph identification for outlines, without a model.
