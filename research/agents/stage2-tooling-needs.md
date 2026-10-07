# Stage 2: tooling needs per workstream (rev 2, from the P1 sweep)

The chair wrote this table to brief the four Stage 2 tooling agents. Each row is a need, what is already
verified in this container (2026-09-27), and the open tooling question. IDs point at the evidence.
Tool facts from memory are leads only: each ledger entry needs a URL and a fetch date.

## W1 / W2: literature pipeline (agent A)
| Need | Verified state | Tooling question |
|---|---|---|
| Discovery | **Crossref** and **OpenCitations v2** work but are throttled. **Unpaywall** works keyless. **arXiv** id lookups work; search sometimes returns 406. **DBLP** serves an anti-bot page. **OpenAlex** and **Semantic Scholar** return 429 without keys. **Parallel Search** is capped at 80 calls (15 used). | Which free keyed services (OpenAlex, Semantic Scholar, CORE, others) would unblock discovery? Give each one's free quota and what it unlocks. Is there a usable MCP paper-search server? |
| Forward-citation coverage for still-open checks | OpenCitations misses many IEEE and 2026 works: DAS 2018 and CPR show 0 citers. | Which source covers forward citations best for free? How do we measure that coverage? |
| Full-text access | Direct PDF fetch works when the URL is known. Blocked: pdfa.org and api.github.com (403), repdf.site (egress). | Unpaywall, CORE, institutional repositories; `raw.githubusercontent.com` and `github.com/<o>/<r>/releases.atom` as GitHub workarounds |
| Paper → structured text (sections, references, limitations) | pdftotext only | GROBID (CRF Docker image), Docling, Marker, MinerU (pin the version: 2509 is AGPL), Nougat (NC licence): accuracy and cost on 4 CPUs |
| Limitation and future-work extraction | Done by agents by hand; quotes are checked by `kb_validate.py` | Published methods or datasets for extracting future-work sentences; citation-context tools |
| Hidden-recall shortfall | 13 of 16 found. Classic file carving was missed (3 of 4). | Which venue or index would have found them? Feed into the P4 search plan. |

## W3: engines, oracles, validators, metrics (agent B)
| Need | Verified state | Tooling question |
|---|---|---|
| Baseline engines with "best-of" option sets | apt versions installed: qpdf 11.9.0, mutool 1.23.10, gs 10.02.1, pdftotext 24.02.0; PyMuPDF 1.28.2. qpdf 11.9.0 **segfaults** on one of its own regression files (OBS-0500). gs reads a header-less file as PostScript unless PDF is forced (OBS-0501, GAP-256). | HEAD or latest builds for qpdf, MuPDF, pdf.js (pdfjs-dist on node), PDFium (pypdfium2), pikepdf, pdfcpu, PDFBox, and Rust hayro, lopdf and pdf-rs. Option sets per tool (GAP-009). |
| CPR baseline (fonts; SRC-0123) | Code is public under CC BY-NC 4.0 at github.com/BeenyHail/CPR. It uses a local Llama 3.1 8B. | Can it run on 4 CPUs and 15 GB (ollama or llama.cpp, quantized)? Is its licence acceptable for research-only use? |
| Conformance oracle (GAP-106) | none | veraPDF, the Arlington PDF Model with TestGrammar, `qpdf --check` |
| DEFLATE recovery and verification oracles (GAP-150..154) | Stock zlib reproduces 54% of Word "Save As" streams (OBS-0303). | zlib and zlib-ng replay; preflate-rs; gzrt; ZipRec (GPL); pugz and rapidgzip (resynchronization). Licences and whether each runs here. |
| Metrics (GAP-008, GAP-201, GAP-055) | none | Text: CER, flexible character accuracy, order-aware scores. Faithfulness: planted-token tests (FaithC4). Visual: per-page SSIM or LPIPS across ≥2 renderers. Structure: a structural diff. Fabrication and omission against the upper bound (charter §2). |
| OCR for text-from-render comparisons | none | Tesseract and docTR (lowest rewrite rates); olmOCR, LightOnOCR, PaddleOCR-VL and DeepSeek-OCR only if they are feasible on CPU (GAP-201 warns about their language priors) |

## W3: damage realism, corpora, generators (agent C)
| Need | Verified state | Tooling question |
|---|---|---|
| Natural pairs: truncation | 34 of 35 Common Crawl/SafeDocs pairs are exact prefixes cut at 1,048,576 bytes (OBS-0201). The metadata is on digitalcorpora S3. | How to sample thousands of pairs, stratified across ZIPs and including disconnect truncations, within about 10 GB. Tools for WARC offset reads. |
| Natural pairs: font encoding | 21 of 526 UDHR PDFs are corrupted (CLM-0415) | OHCHR terms of use; alignment tooling against unicode.org/udhr texts |
| Damage-model parameters | NAND bit-flip rate, about 1 per 248,253 bytes (CLM-0341); NTFS fragmentation statistics (SRC-0137) | Datasets behind those figures; NapierOne; the DFRWS 2006/2007 carving challenges |
| Robustness corpora | Engine regression corpora: 2,873 files (OBS-0500). pdf.js `.link` files may hold personal data. SafeDocs Issue Tracker: 32K files, 31 GB (CLM-0293). | Licences, a PII screening procedure, a subset plan |
| Producer diversity (GAP-002) | Word only in REPDF | Headless producers runnable here: LibreOffice, Chrome print-to-PDF, LaTeX, Ghostscript ps2pdf, cairo, Skia. macOS Quartz output (DMG-201) is not available on Linux: where can it be sourced? |
| Damage generators and attack templates (DMG-009, GAP-104) | `corpus_characterize.py` only | Fault injectors; format-aware PDF mutators; grammar-based generators; the shadow-attack and PDF-Detector artefacts |
| Test specification and provenance format (GAP-050, GAP-010) | none | NIST CFTT test-spec templates; DFXML and CASE/UCO for the provenance record |

## Cross-field wildcard (agent D)
Anything that could overturn the plan. Seeds from P1:
- DEFLATE resynchronization and bit-error correction, including the two-dummy-dictionary trick (CLM-0337);
- SMT or constraint solving for offsets and xref;
- substitution-cipher and HMM decipherment for glyph codes (GAP-205);
- glyph-width leakage (SRC-0411) and name-free font identification (GAP-003);
- LLM- or VLM-guided repair with measured fabrication;
- CDR toolchains (Dubin's PDF-CDR);
- E-APR-style strategy selection with abstention (GAP-155);
- program-repair transfer.

## API-account gate
Each agent lists every free-tier keyed service worth an account: what it unlocks, its free quota, and the ledger items and gaps that depend on it. The chair asks the user once, in one batch.
