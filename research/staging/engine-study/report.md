# Engine source study: report (agent engine-study, block 5)

*This report is the agent's hand-back text, saved by the chair because the agent couldn't write report files.*

- **Date:** 2026-09-27.
- **Parallel Search:** 0 calls (cap 0). Everything came from git checkouts through the container proxy, plus local runs.
- **Validator:** `python research/tools/kb_validate.py --staging research/staging/engine-study --write` reports 0 errors and 0 warnings.

## What I did
1. **Checked out six engines** at pinned HEAD commits using `research/tools/fetch_code.py`, plus Poppler's separate test repository. The checkouts total about 840 MB in `research/cache/code/`.
2. **Read each engine's recovery code** for the nine targets (a)–(i) and for its guards.
   - This produced 48 code-citation claims, CLM-0500..0547, 8 per engine. All have `quote_check: exact`.
   - Copyleft claims are tagged `copyleft`, and every behaviour is described in my own words.
   - Each engine has a notes file (`sources/notes/SRC-050x.md`) recording further behaviour with line references.
3. **OBS-0500:** harvested the engines' test corpora with `research/experiments/engine_test_corpora.py`. Each file is labelled by running `qpdf --check`. No PDF was copied or committed.
4. **OBS-0501:** a follow-up experiment, `research/experiments/engine_header_loss.py`, prompted by a Ghostscript source finding (CLM-0540). It runs the installed engines on REPDF's C1 and C2 files, plus a C1+C2 compound built in a temp dir.
5. **Recorded** 8 gaps (GAP-250..257), 2 proposed damage classes (DMG-250/251), 7 screening records and 1 search-log entry (SRCH-0500, the free git fetches).

## Engines checked out
| SRC | Engine | Commit | Licence | Claims |
|---|---|---|---|---|
| SRC-0500 | qpdf | 4eba95899886 | Apache-2.0 | CLM-0500..0507 |
| SRC-0501 | MuPDF | d587982f971e | AGPL-3.0-or-later (**copyleft**) | CLM-0508..0515 |
| SRC-0502 | pdf.js | d52fdf411a6e | Apache-2.0 | CLM-0516..0523 |
| SRC-0503 | PDFium | 8ca5b735df42 | BSD-3-Clause | CLM-0524..0531 |
| SRC-0504 | Poppler | c8dc23d564f2 | GPL-2.0-or-later (**copyleft**) | CLM-0532..0539 |
| SRC-0505 | Ghostscript/GhostPDL (pdfi) | a3cc9bd74ac0 | AGPL-3.0-or-later (**copyleft**) | CLM-0540..0547 |
| SRC-0506 | poppler/test (test PDFs) | 48b6219b84fc | NOASSERTION (harness GPL-2.0) | none (corpus only) |

Each licence was verified from the checkout's LICENSE/COPYING files and file headers.

## Recovery techniques per engine (our words)
| | qpdf | MuPDF | pdf.js | PDFium | Poppler | Ghostscript (pdfi) |
|---|---|---|---|---|---|---|
| **(a) xref rebuild** | Scans every token for `N G obj`/`trailer`; drops old uncompressed entries | Full scan when opening fails; also a lazy scan, once, on an object-number mismatch. **Stops at the first unparsable object** once a root is known | "Recovery mode" re-parse: scans the whole file, tolerates missing `endobj`/`startxref` | Word scan; the rebuilt table is merged over the parsed one | Scans 4 KiB buffers for a `trailer` or `endstream` at line start, and for object headers | Needs `%PDF` somewhere, else gives up; two passes; repairs once per file |
| **(b) wrong /Length** | Searches for `endstream`/`endobj`; a span with another object starting inside it is rejected | Tries the declared length, then scans for `endstream`; writes the corrected length back (unencrypted files only) | Scans in blocks; also accepts `endsteam`/`endstrea` | Uses whichever of `endstream`/`endobj` is nearer, then trims the end-of-line | After a rebuild, uses the recorded next `endstream`; otherwise pads the length by 5k | Always checks the length; rescans byte by byte (AES needs exact lengths) |
| **(c) ObjStm / XRef streams** | **Doesn't carve ObjStm**; uses the last or largest XRef stream | Parses every ObjStm header | **Only through surviving XRef streams** | Registers the members of every ObjStm | Parses XRef streams and ObjStm headers after the scan | Registers ObjStm members on the second pass |
| **(d) truncation** | Stream **emptied** | Keeps the bytes up to EOF; a truncated object can't overwrite a good one | Stream **rejected** | Stream **dropped** | Reads to EOF | **I/O error** |
| **(e) page tree** | Fixes Kids and /Type; supplies empty /Resources; **defaults MediaBox to US Letter**; after a rebuild, drops pages with more than 2 errors | Corrects an over-large /Count; falls back to a slow lookup | Walks the whole tree if /Count fails | Counts pages by walking /Kids | Treats a lone /Page as the root; rejects an implausible /Count | Corrects /Count |
| **(f) fonts / ToUnicode** | n/a (no rendering) | System or built-in substitute font; stretches widths | **Treats an empty font file as missing**; heuristics from glyph names | Arial-based substitute; ToUnicode falls back to the font's encoding | Base-14 or system substitute; numeric and ligature glyph names | Substitute chosen by font flags; CID fallback font |
| **(g) Flate errors** | Ignores Adler-32; on decode failure **writes the original bytes unchanged** | Keeps the partial output; ignores Adler-32 | Ends the stream, keeps the output | **Silently returns the prefix** | Keeps the output so far | Ignores Adler-32 |
| **(h) damaged /ID** | Empty string plus a warning | Warning; no ID bytes | Empty string | Empty | Empty | Error message, then continues |
| **(i) which copy wins** | Last in file; newest trailer that has /Root | Last in file; an ObjStm member loses to a *later* uncompressed copy | Same generation: the later copy if it parses, otherwise the first. **First** valid trailer that has /ID | Highest generation, then latest; an ObjStm member overrides a gen-0 entry | Highest generation, where the ObjStm index counts as the generation; last trailer, or the first in catalog-recovery mode | Last in file regardless of generation; ObjStm overrides; the highest-numbered Catalog wins |
| **Guards** | Stops after 1000 warnings; ids ≤ size/3; Flate memory limit | Bomb check at 200× output (with a floor); one repair attempt | Recursion sets and depth limits | 1 GiB Flate cap; recursion depth 64; page depth 1024 | ≤4096 XRefStm; ≤1e6 objects per ObjStm | Nesting limit 100; loop detection; one repair |

## Test corpora (OBS-0500)
| Source | Files (unique) | Kinds | qpdf flags damaged | Of which needed an xref rebuild |
|---|---|---|---|---|
| qpdf | 901 (848) | 717 pdf, 105 qdf/zdf and similar, 79 OSS-Fuzz | 238 (incl. 1 **segfault** of qpdf 11.9.0 on its own regression file) | 129 |
| pdf.js | 982 (981), plus **459 .link** | pdf | 277 (132 are linearization-hint problems only) | 42 |
| PDFium | 903 (889) | 341 pdf + 562 expanded `.in` templates | 300 | 90 |
| Poppler (poppler/test) | 83 | pdf | 11 | 4 |
| Ghostscript | 4 (no public corpus) | pdf | 1 | 0 |
| MuPDF | 0 (no public corpus) | – | – | – |
| **Total** | **2,873 (2,802)** | | **827** (664 not linearization-only) | **265** |

**The `.link` files point to 11 hosts:** github.com 191, web.archive.org 178, bugzilla.mozilla.org 80, and others. Many are real users' bug-report attachments. **Some look like personal documents (invoices, for example): don't mirror them.**

## Header-loss experiment (OBS-0501)
100 REPDF documents. A tool "recovers" a file when it extracts at least 95% of the text it extracts from the original.

| Variant | mutool | gs (default) | gs (PDF forced) | pdftotext |
|---|---|---|---|---|
| C1 (header only) | 100 | **0** | 100 | 94 |
| C1+C2 compound | 100 | **0** | **0 (exits 0)** | 94 |

The default gs command line reads a header-less file as PostScript. With PDF interpretation forced, gs fails only when repair is needed, which is what the source code predicts (CLM-0540).

## Top gaps
1. **GAP-250: which copy wins.** The six engines use six different rules for duplicate objects and trailers during reconstruction. No paper evaluates them, and PDFPundit's design picks "last in byte order" without evidence.
2. **GAP-253: Adler-32 isn't a trustworthy oracle.** Three engines ignore checksum failures, because producers write bad checksums. Yet the design's C9 byte-flip acceptance, and the charter's provenance grade 3, rely on Adler-32.
3. **GAP-251: engines split 4–2 on carving objects out of ObjStm.** qpdf and pdf.js lose compressed objects, and no evaluation covers XRef-stream or ObjStm damage.
4. **GAP-252: qpdf, PDFium, pdf.js and Ghostscript discard truncated streams.** PDFPundit's clamp-and-salvage design is *stronger* here, but by how much hasn't been measured.
5. **GAP-254: /ID loss.** Every engine falls back to an empty ID, and none searches for candidate IDs verified against /U. PDFPundit refuses encrypted files outright, so it's weaker than all six engines here.

Also:
- **GAP-255:** the design has no decompression-bomb or output guard.
- **GAP-256:** after header loss, baseline results depend on how each tool is invoked.
- **GAP-257:** the engine corpora are a real-world damage source, but unlabelled.

## Where PDFPundit's design is weaker or stronger than the engines
**Weaker:**
- Encrypted files are refused (GAP-254).
- No output or expansion guard (GAP-255).
- Keyword matching in §14.3(e) is exact, while pdf.js also accepts misspelled and cut-off `endstream` (DMG-250).
- No diagnosis-code taxonomy as systematic as Ghostscript's 83 error and 96 warning codes (useful for §19).

**Stronger:**
- Carves ObjStm, like 4 of the 6 engines and unlike qpdf and pdf.js.
- Clamps truncated streams at EOF instead of discarding them.
- Uses the modal sibling MediaBox before falling back to a default, where qpdf goes straight to US Letter.

**Worth adopting:** qpdf's forensically conservative fallback of writing an undecodable stream unchanged. It maps directly onto provenance grade 1.

## What I did NOT check
- Linearized or progressive loading paths, and repair of linearization hint tables.
- XFA, forms and annotations.
- Error paths for JBIG2, JPX, CCITT and DCT. Beyond a glance, no filters other than Flate.
- Signature handling after repair.
- Output fidelity of `mutool clean` and qpdf rewrites.
- Ghostscript's legacy PostScript-based PDF interpreter.
- Any engine version other than the pinned HEAD, apart from the installed binaries used in OBS-0500/0501 (qpdf 11.9.0, mutool 1.23.10, gs 10.02.1 and pdftotext 24.02.0 are all older than HEAD).
- Whether any paper already evaluates these engine behaviours. `still_open` is `unknown` for every gap: no OpenCitations or literature check was done.
- Engine issue trackers, which would give per-file provenance for the corpora.

## Decisions for the chair
1. **Baseline protocol (charter §3b).**
   - Declare an option set per tool, e.g. gs with PDF interpretation forced.
   - Score on the output, not on exit codes.
   - Pin tool builds at HEAD: the installed qpdf 11.9.0 segfaults on one of qpdf's own regression files.
2. **Revision rule.** Reopen PDFPundit's "last in byte order wins" (§17.2) pending a GAP-250 experiment? Whichever rule is chosen should be reported as grade-4 inference.
3. **C9 and grade 3.** Keep Adler-32 as the acceptance oracle for byte-flip correction before the GAP-253 base-rate measurement?
4. **Encryption scope.** Keep encrypted files as "unrepairable" in v1, when every engine opens blank-password files?
5. **Corpora.** Adopt the harvested engine corpora (OBS-0500) as a robustness-only set? That needs a data-ethics rule for the pdf.js `.link` files first.

## Suggested next steps
- Inflate every Flate stream in the OBS-0500 corpora, and count streams that decode completely but fail Adler-32 (GAP-253).
- Build DMG-004/005/008/014 test files with a known intended revision, run all six engines on them, and observe how their rules diverge (GAP-250, GAP-251).
- Label each corpus file as natural, hand-made or fuzz, using issue ids (pdf.js issueNNNN, PDFium bug_NNN, OSS-Fuzz ids).
