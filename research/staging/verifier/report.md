# Stage 4 verifier report (agent "verifier", 2026-10-05)

Brief: `stage4-verifier.md`, including the chair's post-committee-004 additions. REPDF commit e547d4d1b77ead7e8cccce8b02878a6d33aa427a. All runs were in this container after the 2026-10-05 restart, with no apt, no sudo, no paid search, no account and no contact. Scratch peaked at 5.1 GB for a short time during the metric-suite rebuild; I cut it back to 3.0 GB at once and it stayed there for the rest of the stage.

**Outputs**
- `verification.md`: one row per TOOL with an action; the WP-3.1 pin table; the corrected OBS commands.
- `access.md`: what the user must provision.
- `observations/observations.jsonl`: OBS-1200..1215.
- `experiments/`: scripts, with results in `experiments/results/`.

## Headline

| Check | Result |
|---|---|
| 45 Adopt/Trial TOOLs with `verified_in_container = pass` | **44 pass, 1 fail** (TOOL-413 LibreOffice: the Writer module is gone after the restart and needs apt). **1 downgrade proposed** (TOOL-306 CORE: keyless 429 twice). |
| Engine and oracle smoke, seed 20260927 | **Reproduce exactly** at the recorded versions: 624 and 79 CSV rows, 0 differing cells (OBS-1200, OBS-1201). |
| WP-3.1 pins (MuPDF 1.28.5, Poppler 26.10.0, pdfcpu 0.16.1, hayro 0.8.0, pikepdf 10.16.0, pypdfium2 5.14.0, pdfjs-dist 6.4.299) | All installed within 20 min. The smoke table is equal except **MuPDF 1.28.5's mutool text on C1 (2 of 2 at 95% or more → 0 of 2)**, a version regression confirmed with a same-recipe 1.28.0 build (OBS-1202, OBS-1203). pdfcpu 0.16.1 needs a fresh config directory, and hayro 0.8.0 needs a probe API patch. |
| TOOL-465 fontations | **Pass**: 1,252/1,252 embedded TrueType programs parsed; family matches OBS-0907 on 889/889 rows (OBS-1204). Keep Trial. |
| TOOL-470 MAPIE 1.5.0 | **Pass**: risk control certifies precision 0.9 on synthetic data (test precision 0.9401) and correctly abstains on the 40-row OBS-derived set (OBS-1205). Keep Trial. |
| OBS-0700..0704 (deleted scratch paths) | Rebuilt and re-run. **All reproduce**, so no result changed (OBS-1200, 1201, 1211, 1212). OBS-0706 too (OBS-1213). The corrected commands are in verification.md §D. |
| OBS-0906 decipherer (found while re-running TOOL-462) | **Reproduces only with the stored LM corpus.** The command trains on the whole gitignored `research/cache/fulltext`, which grew from 29 to 33 texts. On 33 texts token_acc is 0.7486 vs 0.7657; on the manifest's 29, every value is identical (**OBS-1215**). |
| Common Crawl paced (TOOL-403/404, WP-3.7) | **30 of 30 ranged GETs return 206** at 2-3 s gaps, with no 403. **Works paced; no AWS account needed** (OBS-1207). |
| GROBID (TOOL-317) | **Not run.** There is no Docker daemon here; the first setup step was denied at a permission check, so it was not pursued. |
| Keyless APIs | Crossref, Unpaywall, OpenCitations, OpenAlex and Semantic Scholar return 200. **CORE returns 429** (limit 10, remaining 0). OpenAlex returns 200 but its keyless quota shows remaining 0 (OBS-1208). |
| Gate hosts | repdf.site: proxy 502 on CONNECT. ohchr.org: 403. web.archive.org: connection reset. **NapierOne bucket: 200** (path-style; a change since Stage 2). registry.ollama.ai manifest: 200. Hugging Face metadata: 200 unauthenticated. |
| SRC-0132 | 500 directly and via doi.org. The PDF cached on 2026-09-27 is still local, and its sha256 equals the registry's, so kb_validate re-checks CLM-0129..0136 as exact against it. |
| PR #16 | The mid-size Adler-only search (CLM-1019) on a new stride sample: 35/48; with OBS-1010's sample (reproduced, 28/47), pooled 63/95 = 66.3% vs 66.1% (OBS-1214). |

## Proposed ledger changes (for the chair)

**Downgrades and failures**
- TOOL-306: Adopt to Trial, and keyless verification to fail, until a free key is supplied.
- TOOL-413: verification to fail until the provisioning is fixed.

**Upgrades of `verified_in_container`**
- TOOL-304 and TOOL-305: fail to pass (keyless).
- TOOL-403: fail to pass.
- TOOL-404: partial to pass.
- TOOL-408: untested to partial.
- TOOL-465 and TOOL-470: untested to pass.

**Notes and evidence fixes**
- TOOL-351: pin 1.28.0, or record the 1.28.5 C1 regression.
- TOOL-355: record that 0.16.1 needs a fresh config directory.
- TOOL-361: record the 0.8.0 API break.
- TOOL-416: now TeX Live 2026, installed in scratch.
- TOOL-450: 1.3.2 output is identical to 1.3.
- TOOL-462 and OBS-0906: pin the LM text list.

## Not checked
- GROBID (no Docker daemon).
- The Ollama model blob download and any LLM step (TOOL-386, CPR Step 2).
- Hugging Face weight download.
- arXiv (TOOL-302).
- The oracle smoke at the WP-3.1 pins; only the engine smoke ran at the pins.
- Paywalled items.
- NapierOne data files (bucket listing only).

## OBS index
| OBS | Topic |
|---|---|
| 1200 | engine smoke, recorded versions |
| 1201 | oracle smoke |
| 1202 | engine smoke at the pins |
| 1203 | MuPDF 1.28.5 C1 regression |
| 1204 | fontations |
| 1205 | MAPIE |
| 1206 | zlib 1.3.2 vs 1.3 |
| 1207 | Common Crawl paced |
| 1208 | access probe |
| 1209 | tool checks (50 pass, 1 fail) |
| 1210 | wildcard prototypes |
| 1211 | deflate replay (OBS-0703) |
| 1212 | CPR Step 1 (OBS-0704) |
| 1213 | metric suite (OBS-0706) |
| 1214 | PR #16 mzbench phases 0 and 5 |
| 1215 | decipher LM-corpus sensitivity |
