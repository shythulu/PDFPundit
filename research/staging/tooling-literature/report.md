# Stage 2 tooling report: literature pipeline (agent tooling-literature, block 6)

Date: 2026-09-27. Paid Parallel Search calls used: **5 of 7** (2 remaining, deliberately unused —
sub-tasks 2, 6, 7 and the hands-on parsing check all closed out with free registry APIs and local
tooling instead, per the mid-task chair update to minimize approval-needing/paid actions).
Validator: `kb_validate.py --staging research/staging/tooling-literature --write --online` reports
**0 errors, 0 warnings** (records: tool=26, observation=15 [2 new: OBS-0600, OBS-0601], search=23).

## 1. Ledger summary

| id | name | category | verdict | verified |
|---|---|---|---|---|
| TOOL-300 | Crossref REST API | discovery | Adopt | pass |
| TOOL-301 | OpenCitations (Index API v2/COCI) | discovery | Trial | pass |
| TOOL-302 | arXiv API | discovery | Trial | fail |
| TOOL-303 | DBLP | discovery | Hold | fail |
| TOOL-304 | OpenAlex API | discovery | Trial | fail |
| TOOL-305 | Semantic Scholar Graph API | discovery | Trial | fail |
| TOOL-306 | CORE API | discovery | **Adopt** | pass |
| TOOL-307 | Europe PMC REST API | discovery | Assess | pass |
| TOOL-308 | BASE | discovery | Hold | fail |
| TOOL-309 | Lens.org / Dimensions | discovery | Assess | untested |
| TOOL-310 | Unpaywall API | fulltext-access | Adopt | pass |
| TOOL-311 | Community MCP paper-search servers | discovery | Assess | untested |
| TOOL-312 | Elicit | gap-mining | Assess | untested |
| TOOL-313 | Consensus | gap-mining | Assess | untested |
| TOOL-314 | scite.ai | gap-mining | Assess | untested |
| TOOL-315 | Connected Papers | discovery | Assess | untested |
| TOOL-316 | ResearchRabbit | discovery | Assess | untested |
| TOOL-317 | GROBID (CRF-only Docker image) | paper-parsing | Trial | untested |
| TOOL-318 | Docling | paper-parsing | Assess | untested |
| TOOL-319 | Marker (marker-pdf) | paper-parsing | Assess | untested |
| TOOL-320 | MinerU | paper-parsing | Trial | untested |
| TOOL-321 | Nougat (nougat-ocr) | paper-parsing | Assess | untested |
| TOOL-322 | pdfplumber | paper-parsing | Trial | **partial** |
| TOOL-323 | PyMuPDF4LLM | paper-parsing | **Adopt** | **pass** |
| TOOL-324 | Citance/future-work extraction methods | gap-mining | Assess | untested |
| TOOL-325 | Dedicated reference manager vs JSONL | reference-mgmt | Hold | pass |

Full field detail (licence, quota, exact command+output, rationale citing evidence) is in
`tooling/ledger.jsonl`.

## 2. Top recommendation per need (the six needs-table rows)

1. **Discovery.** Adopt Crossref (already in use) and CORE (works keyless, huge index, no key
   needed — the sweep's best surprise). Trial OpenAlex and Semantic Scholar once free keys are in
   hand (both keyless-429 today, TOOL-304/305); a free key is the single highest-leverage account
   for closing the IEEE/ACM coverage gap OBS-0600 quantifies. Hold DBLP (anti-bot page) and BASE
   (explicit IP/UA denial, `probe/base.xml`). No MCP server or hosted AI tool (Elicit/Consensus/
   scite/Connected Papers/ResearchRabbit) beats direct free-API access; all capped at Assess per
   charter and by the account-signup block.
2. **Forward-citation coverage.** Measured, not guessed: OBS-0600 queried Crossref
   `is-referenced-by-count` and OpenCitations Index API v2 for 10 sources spanning forensics,
   compression, fonts/ML, PDF/IR and OCR (IEEE/ACM/Elsevier/PoPETs). Crossref is the more complete
   free source (0/10 sources at zero vs OpenCitations' worst case) and OpenCitations specifically
   under-counts IEEE and very-recent works — confirming and quantifying the needs-table's DAS-2018/
   CPR observation rather than just repeating it. Use Crossref as primary, OpenCitations as a
   secondary cross-check.
3. **Full-text access.** Adopt Unpaywall (confirmed keyless today, `research@pdfpundit.invalid`)
   and CORE (also a repository aggregator) alongside direct PDF fetch and the GitHub workarounds
   already documented in the needs-table. No new blockers found beyond the pre-existing ones
   (pdfa.org, api.github.com, repdf.site).
4. **Paper → structured text.** Hands-on evidence (OBS-0601, on the cached CPR PDF, SRC-0123):
   **PyMuPDF4LLM is the clear winner** — correct leveled section headings, a correctly-isolated
   References heading, and clean text across a two-column boundary where both pdfplumber's default
   mode and the existing pdftotext baseline splice unrelated columns together. GROBID has the
   strongest published support (arXiv 2303.09957: best-in-class for reference/metadata extraction)
   and its CRF-only Docker image is confirmed to fit the 1.5 GB budget (~486 MiB), but could not be
   run here — no Docker daemon access (starting one was denied). MinerU's licence history is fully
   resolved: v2.5.0 ("2509") is AGPL-3.0 as the brief warned, but the current v4.0.7 is
   Apache-2.0-plus-commercial-threshold — **pin the version and re-check LICENSE.md at that tag**.
   Nougat has the strongest published support specifically for equation-heavy scientific text
   (arXiv 2410.09871v1) but its model-weight licence (CC BY-NC 4.0, if the project's README claim
   holds) is unconfirmed this session and it was not hands-on tested (disk-budget discipline).
5. **Limitation/future-work extraction.** ACL Anthology 2022.sdp-1.6 (open access) gives a real,
   peer-reviewed citation-context classifier (fine-tuned RoBERTa, 98.84% accuracy) that could
   pre-filter candidate quotes the way scite.ai does commercially, without an account or fee.
   Recommend caching it as a proper SRC/CLM record in P4 and prototyping a small classifier as a
   pre-filter — any output would still need per-quote verification against the real PDF via
   `kb_validate.py`; no tool replaces that check.
6. **Hidden-recall shortfall (classic file carving).** Tested three concrete Crossref recipes
   today: (a) journal-restricted search on Digital Investigation (ISSN 1742-2876) for "carving",
   filtered to ≤2012 — 3/3 hits, all genuinely classic carving papers, zero noise; (b)
   `query.bibliographic=DFRWS` filtered by date surfaces one front-matter DOI per year
   ("Sixth..Thirteenth Annual DFRWS Conference", all under Digital Investigation) usable to walk
   each year's proceedings; (c) `query.container-title=IFIP Advances in Information and
   Communication Technology` combined with carving-specific terms (a relevance boost, not a hard
   filter) surfaces real "Advances in Digital Forensics" volumes II–XV with on-topic carving
   chapters. **Recipe for P4:** never free-text search broad terms like "fragment reassembly" alone
   (76,911 noisy hits); always restrict to Digital Investigation/DFRWS/IFIP AICT container scope
   first, then apply carving-specific terms.

## 3. Bibliographic plumbing (sub-task 7)

DOI → BibTeX and DOI → CSL-JSON both work today via Crossref content negotiation
(`works/<doi>/transform/<mime>` or an `Accept:` header on `works/<doi>`), keyless, tested live
(`probe/das2018.bib`). Every KB source already carries its DOI. **Recommendation: Hold on adopting
a dedicated reference manager** (Zotero, JabRef, etc.) — it would be a second source of truth to
keep in sync with the JSONL registries, for a capability (BibTeX/CSL export) Crossref already gives
on demand. If a human collaborator later wants a browsable citation library, generate it from
`registry.jsonl` + content negotiation as a one-off script.

## 4. API-account list (for the chair's single batched ask)

| Service | Unlocks | Free quota (URL, fetch date) | Depends on |
|---|---|---|---|
| OpenAlex | Removes today's keyless 429; best coverage-per-account-cost for our IEEE/ACM/Elsevier gap | A `mailto=`/free-key "polite pool" is documented at docs.openalex.org → redirects to help.openalex.org; **exact current numeric limits not independently re-confirmed this session** (WebFetch's page summarizer could not extract them from the redirected help-center pages on 2026-09-27) | TOOL-304, OBS-0600, W1 discovery |
| Semantic Scholar | Removes today's keyless 429; strong CS/ML citation graph | Free key requested via semanticscholar.org/product/api ("delivered via email"); **exact current numeric limits not independently re-confirmed this session** (one fetch attempt returned an implausible figure, not trusted) | TOOL-305, W1 discovery |

Both are flagged Assess-level on the exact quota numbers pending a fresh check by whichever agent
next has Parallel/WebFetch budget — the *decision* to request them is well supported (both throttle
keyless requests today, both are free, both index venues OpenCitations under-covers), only the
precise rate-limit figures are unverified. CORE, Unpaywall, Crossref, OpenCitations, arXiv and
Europe PMC need no account. Lens.org/Dimensions/Elicit/Consensus/scite.ai/Connected
Papers/ResearchRabbit are not recommended for an account yet (see ledger rationale for each) —
none clears a bar the free/keyless options above don't already clear, and hosted AI tools are
capped at Assess by the charter regardless.

## 5. What I did NOT check

- Did not sign up for any account (Lens.org, Dimensions, Elicit, Consensus, scite.ai, Connected
  Papers, ResearchRabbit, or a Semantic Scholar/OpenAlex key) — outside this agent's authority.
- Did not run GROBID: no Docker daemon in this sandbox; starting one was denied by policy and not
  retried or worked around, per the mid-task chair update. Image-size/licence facts are unaffected.
- Did not hands-on test Docling, Marker, MinerU or Nougat (only registry/license metadata checked)
  — time/disk budget went to the two tools (pdfplumber, pymupdf4llm) with the strongest a-priori
  case for a quick win; Nougat additionally needs a multi-GB model download not attempted here.
- Did not re-verify DBLP's static XML-dump route (a possible offline alternative to the blocked
  live API) — out of scope for a "quick check."
- Did not retry arXiv's id-lookup 406, OpenAlex's 429, Semantic Scholar's 429, or BASE's denial —
  each was tested exactly once (with the wrapper's built-in 2-retry backoff) and recorded as
  fail/untested per the no-retry-loop rule, not chased further.
- The exact original query text for 4 of the 5 paid Parallel calls could not be recovered after a
  context-compaction event this session; `search-log.jsonl` records their topics and findings
  faithfully (each ledger entry's rationale/URLs are independently checkable) but flags those 4
  entries' `query` field as a reconstruction, not a verbatim transcript. The 5th call's parameters
  are verbatim.
- Did not cache full SRC/CLM records for arXiv 2303.09957, arXiv 2410.09871v1 or ACL 2022.sdp-1.6
  (the three papers backing the paper-parsing and citation-context recommendations) — they are
  cited as Parallel-Search-sourced leads (`SRCH-0617`) in the ledger's `provenance`/`rationale`
  fields rather than as fully verified-quote claim records. Recommend a future agent (or this one,
  if re-engaged) cache and quote them properly; they are open access and cheap to fetch.

## 6. Decisions for the chair

1. Adopt Crossref + CORE for discovery, Unpaywall for full-text; request free OpenAlex + Semantic
   Scholar keys per §4 (batched with the other agents' asks).
2. Adopt PyMuPDF4LLM for section/reference structuring now (hands-on pass); Trial GROBID the
   moment any Stage 2/3/4 agent has Docker daemon access — its published accuracy edge over
   PyMuPDF4LLM for reference-list quality specifically is worth the one-time setup cost.
3. MinerU: if adopted anywhere in the pipeline, pin to v4.0.7+ (Apache-2.0) and never to v2.5.0-era
   tags (AGPL-3.0-only) or earlier; re-check LICENSE.md at whatever tag is actually used.
4. Hold on any dedicated reference manager; keep DOI + Crossref content negotiation as the
   bibliographic layer.
5. sub-task 6's three-part Crossref recipe (Digital Investigation journal restriction + DFRWS
   annual-proceedings enumeration + IFIP AICT container-title boost) is ready to hand to whichever
   agent runs the P4 full literature sweep, to close the classic-carving recall gap without needing
   to guess which sources were missed.
