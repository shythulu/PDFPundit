---
committee: 004
topic: Stage 2 tooling research, the PR #16 ingest, and the Stage 3 plan changes (5k rev 1, 10k rev 5, charter rev 2.3)
date: 2026-10-05
charge: Which tools, oracles and evaluation methods does the plan adopt, what did PR #16 change, and which decisions go to the review board or the user?
members:
  - seat: chair (merges, applies record updates, writes the 5k work packages)
  - seat: four Stage 2 tooling agents (A literature pipeline, B engines/oracles/metrics, C damage and corpora, D cross-field wildcard), each deciding nothing and asking the chair
  - seat: agent E (pr16-ingest), which reproduced PR #16's committed scripts and proposed record updates without applying them
  - review of the chair's decisions: deferred to the Stage 4 contrarian and verifier and the Stage 5 review board, since the chair authored them
inputs: research/staging/{tooling-literature,tooling-damage,tooling-engines,tooling-wildcard}/report.md; research/staging/pr16-ingest/{report,reconciliation}.md; PR #16 (merged to main as c7a6ad4)
paid_searches: 25 of 80 at the end of Stage 2 (B used 0 of 6, D 3 of 7)
outcome: 118 ledger entries; 51 observations; charter rev 2.3 (grades 2 and 3); C9 probe rev 2; gap, hypothesis and claim updates listed below; 5k rev 1; 10k rev 5
---

# Committee 004: Stage 2 tooling and the PR #16 ingest

## What was merged
| Agent | Commit | Ledger | Observations | Other |
|---|---|---|---|---|
| A tooling-literature | 2ff9430 | 26 (TOOL-300..) | OBS-0600, 0601 | carving search recipe |
| C tooling-damage | 25a869f | 28 (TOOL-400..) | OBS-0800, 0801 | PII screening procedure |
| B tooling-engines | d732922 | 37 (TOOL-350..386) | OBS-0700..0708 | CLM-0700..0706; SRC-0700 (CPR source at 9ecd685) |
| D tooling-wildcard | 622c743 | 27 (TOOL-450..476) | OBS-0900..0908 | SRCH-0900..0902 |
| E pr16-ingest | 6eafec1 | none | OBS-1000..1015 | SRC for the report and its 5 notes; CLM-1000..1079; GAP-300, 301; HYP-300..302 |
| chair (this committee) | a398e50 | none | OBS-0300..0302 rev 2 | probe fix; record updates below |

Validator after each merge: 0 errors. The 7 standing warnings are 3 web sources with no cached text
(CLM-0800, 0801, 0805) and 4 unpinned remote inputs (OBS-0201 ×2, OBS-0600, OBS-0800).

B and D were stopped by a container restart on 2026-09-27 and relaunched on 2026-10-05 with a
salvage-first brief. Tools they had installed were gone, so several entries record
`verified_in_container` as of 2026-09-27. Stage 4's verifier re-runs them.

## Decisions asked for, and responses

### Agent A (literature pipeline), applied in 5k rev 0
| # | Decision asked for | Response |
|---|---|---|
| A1 | Crossref + CORE + Unpaywall; ask for OpenAlex and Semantic Scholar keys | **Accepted.** The keys go to the single access gate after Stage 4. |
| A2 | PyMuPDF4LLM now; GROBID once Docker runs | **Accepted.** WP-1.2. Stage 4's verifier tries GROBID in Docker. |
| A3 | MinerU only at v4.0.7+ (Apache-2.0) | **Accepted** as a constraint; not adopted. |
| A4 | No reference manager | **Accepted.** |
| A5 | The three-route carving recipe | **Accepted.** WP-1.1 step 2. |

### Agent C (damage and corpora), applied in 5k rev 0
| # | Decision asked for | Response |
|---|---|---|
| C1 | GAP-202's sources are unreachable | **Partly solved:** NLTK `udhr2` replaces unicode.org (WP-3.8). OHCHR stays an access-gate item. |
| C2 | Common Crawl started refusing ranged reads | **Stage 4 re-tests at a slow rate.** WP-3.7 has a metadata-derived fallback, graded below byte-verified. |
| C3 | NapierOne and index.commoncrawl.org blocked | **Access-gate item.** |
| C4 | peepdf-3 is GPL-3.0 | **Accepted:** external command only. |
| C5 | 28 entries | **Accepted.** |
| — | PII procedure contradiction ("check before caching" vs "never cache") | **Chair fix:** a quarantine outside the repository and cache (WP-3.10). |

### Agent B (engines, oracles, metrics)
| # | Decision asked for | Response |
|---|---|---|
| B1 | Pin qpdf ≥ 12.4.2 | **Accepted.** The apt 11.9.0 build segfaults (OBS-0500, OBS-0701). WP-3.1. |
| B2 | Update TOOL-426 (pikepdf) | **Done:** 10.16.0 (2026-09-29), with a note that 10.16.0 itself was not run. |
| B3 | Text reference = OCR of the original's render or the upper bound, not its text layer | **Accepted.** 2 of 19 Print to PDF originals have text layers holding 50–60% of the visible text (OBS-0708). WP-3.2 and WP-3.5. |
| B4 | Conformance oracle = delta against the original across Arlington, `qpdf --check` and `pdfcpu validate -m strict` | **Accepted** as the GAP-106 oracle. 13 of 19 clean originals fail Arlington, so pass/fail alone is unusable (OBS-0702). WP-3.3. |
| B5 | Patch CPR for determinism; download the 4.9 GB model in P4 | **Accepted, time-boxed in P4.** The patch is local and documented (temperature 0, fixed seed in the Ollama payload, CLM-0701). The download needs registry.ollama.ai, so it is listed at the access gate. CPR stays Assess until Step 2 runs. WP-3.6. |
| B6 | 37 entries, over the 12–25 guide | **Accepted.** Each Hold is a rejected candidate the brief asked for. |
| — | B's zlib replay swept memLevel {8, 9} only | **Chair note on TOOL-364:** with memLevel 1–9, stock zlib 1.3 reproduces 2,074 of 2,074 sampled Save As streams (OBS-0901). WP-3.3 uses the full sweep. |
| — | TOOL-366 and TOOL-476 cover the same crate | **Cross-referenced, not merged:** TOOL-366 is the crate (round trip with corrections), TOOL-476 its zero-correction replay use. Both Trial. |

### Agent D (cross-field wildcard)
| # | Decision asked for | Response |
|---|---|---|
| D1 | Reword charter grade 2 | **Done, charter rev 2.3:** dictionary invariance counts only from a restart confirmed by trailer alignment or a following decodable block. A blind scan certified 8,521,181 wrong bytes from 51,120 false starts (OBS-0900). |
| D2 | Reword charter grade 3 | **Done, charter rev 2.3:** "only" means exhaustive enumeration with trailer edits included, a stated body-over-trailer prior, and a grammar check where one exists. Checksum plus encoder replay is not uniqueness (OBS-0903, OBS-0904). |
| D3 | Keep pure Rust; Trial preflate-rs as the replay oracle; stock zlib in the harness; drop "port zlib" | **Accepted.** No product decision changes. The Trial must fix three things: a public API for zero-correction replay (the probe mirrors a private struct), panic isolation (14 panics), and the memLevel-7 misses (395 of 400, OBS-0908). Print to PDF is out of reach for both (0 of 400). |
| D4 | Producer profile as a first-class model input | **Accepted for research.** A producer profile holds the replay parameters, xref bias (+1 for Print to PDF), EOL style, per-page font instances and the encoder fingerprint (OBS-0901, 0905, 0907). WP-3.9 adds a profile per producer; GAP-002 caps how far two producers generalise. Whether the product uses profiles goes to the board (WP-4.2). |
| D5 | Amend PR #16's "ignore CIDFont+Fn names" advice; scope GAP-003 | **Done.** 889 of 889 embedded TrueType programs keep their family in the `name` table (OBS-0907). GAP-003 now applies mainly to C7/C8, where no font program survives. |
| D6 | Decipherment output is grade 4, with abstention | **Accepted.** Digits are 6.5% correct without width evidence (OBS-0906). |
| D7 | SMT, CDR toolchains and Kaitai on Hold for P4 | **Accepted.** z3 stays for research prototypes. The xref solver needed no SMT on 100 of 100 files (OBS-0905). |
| D8 | Reconcile with agent B | **Done** (B's memLevel note and the TOOL-366/476 cross-reference above). |
| D9 | Fund an LLM/VLM fabrication trial? | **To the review board.** No run is planned before P4. A Hugging Face token is optional (Ollama serves the CPR model without one) and is listed at the gate as optional. |

### Agent E (PR #16 ingest)
E's proposed updates (reconciliation §2) were applied as proposed, with these chair checks:

| Item | Response |
|---|---|
| OBS-0300 undercounts damaged streams by 3 (OBS-1012) | **Fixed at the source.** `flate_c9_probe.py` rev 2 keeps a CR that is the last zlib byte. The re-run gives 1,445 damaged streams: 1,220 Adler-only, 221 inflate errors, 4 truncated. That matches PR #16 and OBS-1001 exactly. OBS-0300..0302 now cite the rev 2 outputs. The rev 1 outputs stay committed because OBS-1012 pins them by hash. |
| OBS-0300's latency misread | **Fixed.** The median of 3,208 bytes covers every reporting stream; for the 221 that raise an inflate error the median is 5 bytes (OBS-1001). GAP-150 says so. |
| E's unexamined one-byte residual (1,446 Flate-span bytes vs 1,445 damaged streams) | **Explained:** one C9 change hit the CR of the CR LF before `endstream` in one Save As file and damages no zlib data (OBS-0302 rev 2; `research/experiments/flate_c9_eol_check.py`). |
| CLM-1035 (REPDF's Table 2 vs Table 5) | **Chair-checked** against SRC-0001's tables; tagged `chair-checked:SRC-0001`. GAP-013 cites it and stays open. |
| GAP-052, 150, 151, 154 | **Updated** with the measured false accepts (1/245, 1/324), the trailer-edit accept (OBS-1013), replay non-uniqueness (OBS-0903, 0904), length-preserving salvage (OBS-1003) and the memLevel sweep (OBS-0901). |
| E's 5k changes (reconciliation §3) | **Applied in 5k rev 1**, items 1–7. |
| PR #16's claims without committed scripts (E6 TrueType localizer, E8 C10 census, the corpus scan) | **Leads only.** HYP-300 tests E6; WP-2.3 lists the E8 ceiling (CLM-1033) as a lead until a committed census exists. |

## Chair's own findings
- **Two Trial verdicts have no hands-on run:** TOOL-465 (read-fonts/skrifa) and TOOL-470 (MAPIE). The verification rule says every Adopt or Trial entry needs hands-on evidence. Stage 4's verifier runs both or downgrades them to Assess.
- **Newer releases unrun:** pdfcpu 0.16.1, Poppler 26.10.0, MuPDF 1.28.5, hayro 0.8.0, pikepdf 10.16.0, pypdfium2 5.14.0, pdfjs-dist 6.4.299. The verifier runs the pinned versions.
- **PR #16 and our KB now agree on every reproduced C9 number.** The disagreements left are about what the numbers mean: stream-level gains vs REPDF's per-file OCR metric (GAP-300, HYP-302).

## Escalations
None to the user from this committee. The chair rejected no blocking objection. Items for others:
- **Review board:** D3's Trial outcome before any ADR on replay; D4 producer profiles in the product; D9 LLM/VLM trial; the five product flags (WP-4.2), now with PR #16's evidence.
- **Access gate (one batch after Stage 4):** OpenAlex and Semantic Scholar keys (A1); OHCHR, NapierOne and index.commoncrawl.org (C1, C3); registry.ollama.ai for the CPR model (B5); an optional Hugging Face token (D9); repdf.site (WP-3.5 route 1).
