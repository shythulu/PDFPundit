# Access and provisioning: what the user must decide (Stage 4 verifier, 2026-10-05)

Each item was probed once from this container on 2026-10-05, unauthenticated and paced at 2 s or more per host (OBS-1207, OBS-1208). "Unlocks" names the plan work it feeds. No account was created, no key was used and nobody was contacted.

## 1. API keys (free accounts; the user creates them and adds the keys as environment secrets)

| Item | Today without a key | Unlocks | Depends |
|---|---|---|---|
| **CORE API key** (recommended) | 429 twice, 20 min apart, `x-ratelimit-limit 10`, `x-ratelimit-remaining 0` (Cloudflare) | Keyless CORE no longer answers from this egress IP. The OA full-text search and second discovery source in the P4 search plan need it. | TOOL-306 (Adopt, propose Trial until a key is in place), WP-1.1 |
| **OpenAlex key** (recommended) | 200, but `x-ratelimit-limit 1000`, `x-ratelimit-remaining 0`, reset in 20,921 s | Keyless calls work, but the daily budget is already spent on the shared IP. The key unlocks discovery, forward citations and the citation-count fill-in. | TOOL-304, WP-1.1, WP-1.3 step 2 |
| **Semantic Scholar key** (optional) | 200, no rate-limit headers | Keyless works today. The key gives a dedicated quota, abstracts at volume (audit 002 A4) and the fill-in. | TOOL-305, WP-1.1, WP-1.3 step 2 |
| **Hugging Face token** (optional) | Model metadata `GET /api/models/<HF model>` returns 200 unauthenticated | Not needed for metadata. A token is needed only for gated weights or higher download rates in a funded LLM/VLM fabrication trial. Weight download was not tested. | D9 (WP-4.2) |
| Crossref, Unpaywall, OpenCitations | 200 keyless. Crossref's polite pool is `x-rate-limit 1/1s`. Unpaywall uses `research@pdfpundit.invalid`. | Nothing to provision. | TOOL-300, TOOL-310, TOOL-301 |

## 2. Accounts

| Item | Verdict | Depends |
|---|---|---|
| **AWS account for Common Crawl**: **not needed** | Paced at about 2 s, data.commoncrawl.org answered **30 of 30** ranged GETs with **206**, with no 403. The requests covered warc.paths.gz, cc-index.paths.gz and a real CC-MAIN-2021-31 WARC record. digitalcorpora's unsigned S3 also works: 12,946 metadata rows, 1,000 central-directory entries (OBS-1207). Keep the sampler's pacing and 403 back-off. | TOOL-403, TOOL-404, TOOL-402, WP-3.7, GAP-100, GAP-006, GAP-017 |
| No other account is needed for anything the verifier checked. | | |

## 3. Network hosts (environment network policy)

| Host | Today | Unlocks | Depends |
|---|---|---|---|
| **repdf.site** | The egress proxy refuses it: 502 on CONNECT | Black-box REPDF runs (route 1 of the replication) | WP-3.5, GAP-013, GAP-300 |
| **www.ohchr.org** | HEAD 403 (Cloudflare) | UDHR PDFs for the font-encoding natural pairs. The alternative is for the user to download the translations by hand. | WP-3.8, GAP-201, GAP-202, GAP-203, DMG-200, DMG-201 |
| **web.archive.org** | Connection reset after 12 s | The 2022 OHCHR snapshots, as an alternative to www.ohchr.org | WP-3.8 (same GAPs) |
| **registry.ollama.ai** | Manifest GET returns 200 (no blob fetched) | The registry is reachable. The model blob comes from a separate CDN host, which was **not** tested (no download). Allow the blob host too, or test it before WP-3.6 step 2. | WP-3.6, TOOL-385, TOOL-386, GAP-003, GAP-004, GAP-200, GAP-201 |
| NapierOne S3 bucket | Path-style ListObjectsV2 returns 200 (`s3.eu-north-1.amazonaws.com/napierone.com/`) | **Change since Stage 2:** reachable now. The virtual-host form failed on the bucket name's dot. Nothing to provision; use the path-style URL. | TOOL-408, WP-3.9 |
| data.commoncrawl.org, digitalcorpora S3 | 206 / 200 (see §2) | Nothing to provision | WP-3.7 |
| nti.khai.edu and doi.org for **SRC-0132** | 500 both ways | Nothing. The PDF cached on 2026-09-27 (`research/cache/pdf/e7bd5fa0…2615.pdf`) is still present and its sha256 equals the registry's `fulltext_sha256`. kb_validate re-checks all 8 quotes (CLM-0129..0136) as `exact` against it. | SRC-0132 |

## 4. Local provisioning (container image, not network)

| Item | Today | Unlocks | Depends |
|---|---|---|---|
| **Docker daemon** | No daemon in this container. Setting one up was not attempted after a permission denial. | GROBID CRF reference parsing (not run) | TOOL-317, WP-1.2 |
| **libreoffice-writer** | `soffice` 24.2.7.2 is present but the Writer module is gone after the restart, so `--convert-to pdf` fails. Reinstalling needs apt. | LibreOffice originals for the producer set | TOOL-413, WP-3.9 |
| TeX Live | Debian's texlive is gone. TeX Live 2026 (pdfTeX 1.40.29) installs into scratch from a CTAN mirror in a few minutes (`texlive_install.sh`), so no apt is needed. | pdflatex originals | TOOL-416, WP-3.9 |
| MuPDF pin | 1.28.5 regresses on two C1 files against 1.28.0 (OBS-1203). Choose the pin. | Baseline harness | TOOL-351, WP-3.1 |

## 5. Paywalled items and outward-facing calls (the user's decision only; not probed)

| Item | Unlocks | Depends |
|---|---|---|
| Hughes 2024, Casey 2019, the IEEE and Springer items (committee 002 #20) | Reading backlog | WP-1.5, GAP-103, GAP-105, GAP-106 |
| PDF Association: File Observatory tables, SafeDocs labels (committee 002 #21) | Robustness corpora and labels | WP-3.10 |
| Google Document AI (hosted, paid), or accept Tesseract 5.5.3 as the documented substitute | The REPDF replication metric as published | WP-3.5, GAP-013, TOOL-382 |
| Contacting the REPDF authors for code | Replication route 2 | WP-3.5, GAP-300 |
