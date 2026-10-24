# 0017. Text without a font: detect in v1; OCR is offline and never touches the repair

- **Status:** accepted for detection and OCR rules; proposed for the OCR engine
- **Date:** 2026-10-23
- **Decided in:** D-015, D-050 (owner answer (a)); PR #18 note `outlined_text_and_ocr.md` (independently reviewed)

## Decision

### Detection (v1)
1. Outlined text gets an Info `OutlinedText { glyph_runs, contours }` finding. Type3 text gets `Type3Text`.
2. Repair copies those streams verbatim. Info findings never trigger a pass.
3. The stream classifier accepts path operators as well as text operators. Otherwise an outline-only content stream would not classify as content.
4. The V1 retention check counts path fills, so a repair that drops outlined text fails.
5. Markdown export writes a visible "not extracted" note for such text.

### OCR (after v1)
6. OCR never changes `<name>.repaired.pdf`. OCR text goes only to Markdown or a separate searchable copy.
7. OCR text carries its source: model, version and confidence.
8. OCR ships with bundled models or a model pack installed from a file. No download ever ([0004](0004-pure-rust-no-network.md)).
9. **Proposed engine:** tract-onnx with PaddleOCR models, behind a cargo feature. The pure-Rust route is unbuilt and unmeasured. Treat it as a candidate.

## Why

- Print to PDF outlines 4.7% of glyphs (16.5% of English, 25.2% of Chinese) in 43 of 50 corpus files.
- The likely trigger is OS/2 `fsType` 4. That is inferred from one corpus (2 of 2 vs 0 of 42 fonts). Microsoft documents nothing on it.
- REPDF's OCR metric counts this text. Our text-layer metric cannot see it, so a repair that drops it would look fine without the path-fill check.
- The forensic output must stay a pure reconstruction of what the file held. OCR is an inference from pixels.

## Rejected

- Tesseract or onnxruntime in the product. Both are C/C++.
- OCR as a text source inside the repair.
- Hosted OCR (Google Document AI) in the product.
