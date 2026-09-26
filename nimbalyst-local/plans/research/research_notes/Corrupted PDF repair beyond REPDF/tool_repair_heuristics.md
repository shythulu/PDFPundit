# Repair heuristics in existing PDF implementations vs. PDFPundit's planned carver

Scope: how qpdf/pikepdf, MuPDF, pdf.js, PDFium, Poppler (xpdf lineage), pypdf, lopdf, pdf-rs and hayro-syntax rebuild a broken PDF, at algorithm level, judged against PDFPundit's design (`nimbalyst-local/plans/pdfpundit-technical-design.md` §4.2–4.4, §13–14, §17).

Method: I read the source directly from shallow clones of each project's default branch as of 2026-09-26. Line anchors below point at these commits:

| Project | Commit / version read | Main repair entry points |
|---|---|---|
| qpdf | `54d6053` (2026-09-06; CMake says 12.4.2-dev) | `Objects::reconstruct_xref`, `recoverStreamLength`, `insertXrefEntry`, `Pages::getAllPagesInternal` |
| MuPDF | `d587982` (2026-09-24) | `pdf_repair_xref_base`, `pdf_repair_obj`, `pdf_repair_obj_stm(s)`, `pdf_repair_roots`, `pdf_repair_trailer` |
| pdf.js | `d52fdf4` (2026-09-25) | `XRef.indexObjects`, `Parser.#findStreamLength`, `Catalog.getAllPageDicts` |
| PDFium | `8ca5b73` (googlesource, 2026-09-25) | `CPDF_Parser::RebuildCrossRef`, `CPDF_SyntaxParser::ReadStream/FindStreamEndPos`, `CPDF_CrossRefTable::AddNormal/AddCompressed` |
| Poppler | `c8dc23d` (26.09 series, 2026-09-25) | `XRef::constructXRef`, `constructObjectEntry`, `saveTrailerDict`, `Parser::makeStream` |
| pypdf | `54d3518` (6.19.0) | `PdfReader._rebuild_xref_table`, `_find_pdf_objects`, `root_object` |
| lopdf | `0e781ce` (master after v0.45.0; the reconstruction code is already in the v0.45.0 tag) | `Reader::reconstruct_xref_and_trailer`, `scan_object_markers`, `parser::recover_stream_length` |
| hayro-syntax | `ae09437` (crate 0.7.2) | `xref::fallback_xref_map_inner`, `Pages::new_brute_force`, `stream::parse_fallback` |
| pdf-rs (`pdf` crate) | `5dea68f` (0.10.0) | `File::scan` (no real reconstruction) |

---

## Q1. How do the libraries rebuild the xref and find objects when it is broken? (object headers, duplicates, stream length, keywords inside binary data, missing endobj/endstream, finding the trailer/Root)

### Takeaway
All mature engines use the same basic strategy: scan the whole file for `N G obj` and a trailer or `/Root`, with "the later copy in the file wins" as the default. They differ in four places that matter to PDFPundit:
1. **How they avoid phantom objects inside stream data.** MuPDF and PDFium tokenize and skip stream bodies. qpdf and lopdf only accept headers at the start of a line. Poppler and pypdf do nothing to avoid them.
2. **How they bound a stream when /Length is wrong.** Every surveyed engine uses the first `endstream` keyword, sometimes also `endobj`. lopdf additionally requires the match to be unique within the object. None of them uses an inflate probe.
3. **How they break ties between duplicates.** Rules vary by generation number, by whether the copy is in an object stream, and by whether the copy is truncated.
4. **How they validate a trailer or Root candidate.** Several check that Root → Pages → Count actually resolves.

PDFPundit's landmark scan plus dead-range design, with its stream-extent ladder, is stronger than any surveyed engine on phantom avoidance and stream extents. It misses several proven heuristics:
- Candidate validation for trailer, Root and page tree.
- A rule that a truncated copy at EOF must not override a good earlier copy.
- A precedence rule between object-stream members and uncompressed copies.
- Harvesting trailer keys from xref-stream dictionaries.
- Global resource caps.
- Skipping landmarks that fall inside already-parsed non-stream objects.

### Cited Findings

**1a. Triggering repair / cheap fixes before full reconstruction**
- qpdf looks for `%PDF-` anywhere in the first 1024 bytes. If the header is not at offset 0, it wraps the input in an `OffsetInputSource`, so all xref offsets are read relative to the header. It searches for `startxref` only in the last 1054 bytes. Any exception while reading the xref chain triggers `reconstruct_xref` unless recovery is suppressed — [qpdf QPDF_objects.cc `Objects::parse`](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L218).
- During reconstruction, qpdf also collects every `startxref` it sees. If the tail search found none but a `startxref` exists after the last object, qpdf first retries a normal xref read from it and keeps the result if Root/Pages resolves (warning: "startxref was more than 1024 bytes before end of file"). This covers junk appended after `%%EOF` — [qpdf reconstruct_xref](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L313).
- lopdf 0.45.0 (released 2026-09-08) added "Recover from slightly miswritten startxref / Prev offsets" — [lopdf CHANGELOG](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/CHANGELOG.md). `correct_xref_offset` scans ±64 bytes around a bad offset for the nearest `xref` keyword and never matches inside `startxref` — [lopdf reader.rs](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1645).
- PDFium rebuilds when `startxref` is missing or ≥ file size, when the xref chain fails to load, or when Root is missing or has an invalid object number. It searches backwards up to 4096 bytes for `startxref` — [PDFium cpdf_parser.cpp StartParseInternal/ParseStartXRef](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_parser.cpp#246).
- PDFium `VerifyCrossRefTable` spot-checks only the **first** non-zero xref offset and requires the number read there to equal the expected object number — [PDFium cpdf_parser.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_parser.cpp#374).
- MuPDF repairs when xref loading throws ("trying to repair broken xref") — [MuPDF pdf-xref.c pdf_init_document](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-xref.c#L1918). It also repairs lazily: `pdf_cache_object` triggers a one-time repair when the object at an xref offset has the wrong number ("found object (%d 0 R) instead of (%d 0 R)") — [pdf-xref.c pdf_cache_object](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-xref.c#L2551).
- hayro-syntax repairs lazily as well. If an xref entry does not yield an object with the expected id, it runs the fallback scan once and retries ("broken xref, attempting to repair") — [hayro xref.rs get_with](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L532).
- pypdf (non-strict) checks that `startxref` points at `xref` or `N G obj` (`_get_xref_issues`). It fixes xrefs whose numbering is shifted by an offset. It deletes xref entries whose target does not parse as an object header ("Ignoring wrong pointing object", #2326). If the object numbers are wrong, it rebuilds — [pypdf _reader.py read()](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L696).
- qpdf treats xref offset 0 as a null object ("a common error handled correctly by qpdf and most other applications"; the source comment attributes it to "Mac OS X 10.7.5 Quartz PDFContext") — [qpdf readObjectAtOffset](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1532). Caradoc's 10,000-file crawl also found files that "incorrectly declared in-use objects at offset zero instead of using the appropriate syntax for free objects" — [Endignoux et al., Caradoc, IEEE SPW 2016](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).
- In a 3,977-file Common Crawl sample, 23 files (~0.5%) had a bad xref declaration. About half of those errors came from a non-zero content start offset (junk before `%PDF-`) — [Eliot Jones, "So you want to parse a PDF?" (2025)](https://eliot-jones.com/2025/8/pdf-parsing-xref).

**1b. Locating `N G obj` (the object-header scan)**

| Tool | Scan unit | Anchoring / validation | Skips stream bodies during scan? |
|---|---|---|---|
| qpdf | Tokenizer, reading **only the first token on each line** (`readToken` then `findAndSkipNextEOL`); tokens capped at 10 chars (`MAX_LEN`) | `int int obj`; obj > 0; 0 ≤ gen < 65535; obj ≤ `xref_table_max_id` (capped to file_size/3) or "ignoring object with impossibly large id" | No, but line anchoring cuts phantoms — [src](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L349) |
| MuPDF | Continuous lexing (`pdf_lex_no_string`) with a 2-int sliding window; `obj` token closes the header | num in 1..`PDF_MAX_OBJECT_NUMBER`; gen clamped to 0..65535 | **Yes**: `pdf_repair_obj` consumes the object incl. its stream body (via /Length or `endstream` scan) — [src](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L397) |
| PDFium | Word scanner (`GetNextWord`), last two numbers; skips `( … )` strings and `< … >` hex strings | then **fully parses the object** with `GetIndirectObject(kStrict)`; obj ≤ `kMaxObjectNumber` (24·1024·1024) | **Yes**, the parse consumes the stream — [src](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_parser.cpp#761) |
| Poppler | Raw 4 KiB byte buffer | digit run after whitespace, `num gen obj` with whitespace (newlines tolerated, e.g. `nnn\nnn\nobj`); digits capped at 100000000 | **No** (only records line-start `endstream`, `trailer`, `>> stream`) — [src](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L897), [constructObjectEntry](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1058) |
| pdf.js | "Token" = bytes up to next CR/LF/`<`; regex `^(\d+)\s+(\d+)\s+obj\b` at token start | none beyond regex | Partly: after a header it jumps to the next `endobj` / `N G obj` / `xref` / `trailer <<` match (textual, **ignores /Length**) — [src](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L431) |
| pypdf | `data.find(b" obj")`, then backtracks over whitespace/digits | requires a literal space before `obj`; any digits | **No** — [src](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L1326) |
| lopdf 0.45 | Byte loop | **Line-start only** (leading blanks allowed), explicitly "so pseudo headers buried after another token (string literal, comment) are never mistaken"; num ≤ 1,000,000; stops at 1,000,000 markers | **Yes**: from `stream`+EOL to the first `endstream`; if none, falls back to a *direct* `/Length` hint — [src](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1452) |
| hayro-syntax | Reader over bytes; tries `ObjectIdentifier` at digits | inserts only if the **following object parses** (`skip::<Object>`) | Not explicitly; the scan continues byte-wise after the header — [src](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L76) |
| pdf-rs | `File::scan()` sequentially parses indirect objects up to the startxref offset (and `unwrap()`s `locate_xref_offset`) | strict parse | n/a; no reconstruction path used at load — [src](https://github.com/pdf-rs/pdf/blob/5dea68f36a26efab80e15eeb180de0966a578331/pdf/src/file.rs#L211) |

- lopdf's source comment explains its stream-skip rule: "an uncompressed payload may embed convincing `N G obj` lines whose later offsets would otherwise override the genuine entries for those object numbers" — [lopdf scan_object_markers](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1452).
- pdf.js searches for the next `N G obj` as well as `endobj` so that it does not skip a header when `endobj` is missing ("fixes issue9105_reduced.pdf") — [pdf.js xref.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L558).
- MuPDF skips the rest of the `%PDF-x.y` line, including spaces and `%`, "since some generators forget to terminate the comment with a newline". If lexing fails, it skips to the next whitespace and continues ("skipping ahead to next token") — [MuPDF pdf-repair.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L473).

**1c. Duplicate object numbers**

| Tool | Rule in reconstruction |
|---|---|
| qpdf | Found objects are inserted in **reverse byte order** with `try_emplace` (first insert wins), so the **last copy in the file wins** per (num, gen). Different generations are separate keys — [src](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L413), [insertXrefEntry](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1128) |
| MuPDF | List replayed in byte order; **last wins, including its gen** (no gen comparison). A truncated object at EOF throws first: "Don't let a truncated object at EOF overwrite a good one" — [src](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L102) |
| PDFium | `AddNormal`: later copy wins **unless its gen is lower** than the existing entry (higher gen wins; equal gen means later wins) — [cpdf_cross_ref_table.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_cross_ref_table.cpp#65) |
| Poppler | `constructXRefEntry`: overwrite if the slot is free or `gen >= existing gen` (later wins at equal gen) — [src](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1150) |
| pdf.js | **First wins**, unless the new copy has the *same* gen **and** parses without `ParserEOFException`, in which case the later copy wins ("fixes issue13783.pdf"). A later copy with a different gen never replaces — [src](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L526) |
| pypdf | dict assignment, so the last copy wins per (gen, num) — [src](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L1387) |
| lopdf | `xref.insert`, so the last wins ("Incremental updates append, so later revisions win") — [src](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1414) |
| hayro | HashMap insert, so the last wins — [src](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L102) |

- Caradoc tested how readers resolve references whose generation does not match: "some readers discard the generation number and look for an object with the same object number"; "Most readers incorrectly recognized" a negative gen (`7 -2 R` → `7 -2 obj`) — [Caradoc, SPW 2016](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).
- pdf.js has a `_generationFallback`: when a reference's gen does not match the xref entry, it falls back to a *previous* generation of the same number ("fixes issue15577.pdf"). This is enabled only after trailer validation errors in recovery — [pdf.js xref.js fetchUncompressed](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L902).

**1d. Stream extent when /Length is wrong or missing**

| Tool | Verify declared /Length | Fallback |
|---|---|---|
| qpdf | Requires an integer; seeks and requires the `endstream` token (whitespace skipped by the tokenizer) | `recoverStreamLength`: first `endobj` **or** `endstream` token after the data start. **Sanity check:** if any known object offset (type-1 xref entry) lies strictly between the stream start and that end, length = 0 ("unable to recover stream data; treating stream as empty") — [src](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1440) |
| MuPDF | Only a *direct* int /Length > 0; seek and lex; must be `endstream` | 9-byte sliding window for the literal `endstream` from data start; length = offset of match, **without stripping the EOL**. The repaired `/Length` is written back into the dict for unencrypted files — [src](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L204), [write-back](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L689) |
| PDFium | /Length via `GetDirectObjectFor` (indirect lengths cannot resolve during rebuild since no object list is passed); if `pos+len ≥ file_len` treated as missing; then EOL markers then word must be `endstream` | `FindStreamEndPos`: the **earlier** of the first whole-word `endstream` and the first whole-word `endobj`, then strip a preceding CRLF, or a single CR or LF — [cpdf_syntax_parser.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_syntax_parser.cpp#733) |
| Poppler | Normal mode: seek /Length, expect `endstream` | **In reconstructed mode it overrides /Length unconditionally** with the distance to the next *line-start* `endstream` recorded during the scan (`getStreamEnd` binary search). With no xref at all, it adds 5000 bytes to /Length "and hope its enough" — [Parser.cc makeStream](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/Parser.cc#L229), [XRef::getStreamEnd](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1501) |
| pdf.js | Non-integer /Length is treated as 0, then checks for the `endstream` command | `#findStreamLength`: scans 2 KiB blocks for `end` + `stream`, also accepting misspelled `endsteam` (issue18122) and truncated `endstrea` (issue10004) if followed by whitespace — [parser.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/parser.js#L652) |
| pypdf | Reads /Length bytes (indirect resolved); if `endstream` is off by one, chops 1 byte ("ReportLab" bug) | Non-strict: first `endstream`, data = up to 1 byte before it. A `maximum_declared_stream_length` cap raises `LimitReachedError` — [_data_structures.py](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/generic/_data_structures.py#L646) |
| lopdf | /Length (direct or resolved ref); `take(length)`, optional EOL, `endstream` | `recover_stream_length`: only EOL-framed `endstream` immediately followed by `endobj`, bounded to the current object's xref-derived end; **returns None if more than one candidate exists (ambiguity rejected)** — [parser/mod.rs](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/parser/mod.rs#L344) |
| hayro | `parse_proper` | `parse_fallback`: first `endstream`, data end = **all trailing ASCII whitespace trimmed** — [stream.rs](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/object/stream.rs#L314) |

- EOL after `stream`: qpdf accepts CRLF, LF, **a CR alone when followed by a non-LF byte** ("some readers, including Adobe reader, accept a carriage return by itself"), and extraneous spaces before the EOL, with a warning — [qpdf validateStreamLineEnd](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1381). pypdf skips spaces after `stream` (patch "Danial Sandler") and accepts CR or LF — [pypdf](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/generic/_data_structures.py#L646). hayro's fallback accepts `\n`, `\r\n` or `\r` ("Technically not allowed, but no reason to not try it") — [hayro](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/object/stream.rs#L314). MuPDF consumes one byte and, if it is CR followed by LF, the LF too — [MuPDF](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L191).
- qpdf's "Disregard data check errors when uncompressing /FlateDecode streams. This is consistent with most other PDF readers" (8.0.2, 2018) — [qpdf release notes](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/manual/release-notes.rst).
- In the code I read, no surveyed library determines a stream's extent by decompressing it. All fallbacks are keyword scans — see the table sources above.

**1e. Missing `endobj` / `endstream`**
- qpdf: a missing `endobj` is only a warning ("expected endobj"); a missing `endstream` goes to `recoverStreamLength` — [qpdf readObject/readStream](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1313).
- MuPDF: "object missing 'endobj' token" warning; the scan continues from the next token — [MuPDF pdf_repair_obj](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L243).
- PDFium: `endobj` substitutes for a missing `endstream`, whichever comes first — [FindStreamEndPos](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_syntax_parser.cpp#733).
- pdf.js: the object ends at the next `N G obj` / `xref` / `trailer <<` when `endobj` is missing — [pdf.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L558).
- lopdf: in normal loading, each object is bounded by the next known object's offset (`object_end(offset, next_object)`) — [lopdf load_objects_raw](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L959).

**1f. Finding the trailer / Root when the trailer is gone**
- qpdf:
  - Considers at most the **last 100** `trailer` keywords, newest first, and takes the first dictionary with `/Root` ("If none of them are valid, odds are this file is deliberately broken").
  - Otherwise, among reconstructed objects that are `/Type /XRef` streams, it picks the one with the largest `/Size` (tie → later offset), uses its dictionary as the trailer, and re-reads that xref stream.
  - Otherwise it scans the object cache for `/Type /Catalog` and takes the one with the highest object id.
  - It gives up with "unable to find any pages while recovering damaged file" if the page list is empty.
  - [qpdf reconstruct_xref](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L421)
  - qpdf 11.8.0 (Jan 2024): "Improve file recovery logic to better handle files with cross-reference streams … recover some files that it would previously have reported 'unable to find trailer dictionary'" — [qpdf release notes](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/manual/release-notes.rst).
  - A qpdf-dev discussion notes that reconstruction now "actively searches for /Root dictionaries even if unable to find valid trailer". The side effect is that "qpdf now finds many severely damaged pages trees where previously it would have found no pages" — [qpdf-dev discussion #19](https://github.com/qpdf/qpdf-dev/discussions/19).
- MuPDF:
  - Every top-level `<<…>>` dictionary encountered by the scan is harvested for `/Encrypt`, `/ID`, `/Root` and `/Info`, with later values replacing earlier ones. Inside `N G obj`, only `/Type /XRef` dictionaries contribute `/Root`, `/Encrypt` and `/ID`.
  - `pdf_repair_roots` takes the **last** collected Root that is indirect and resolves to a dictionary.
  - Otherwise `pdf_repair_trailer` scans objects from the highest number down for `/Type /Catalog`. It takes the first object with `/Creator` or `/Producer` as `/Info`.
  - The trailer `/Size` is set to maxnum+1.
  - [MuPDF pdf-repair.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L839)
- pdf.js:
  - Parses every `trailer` dictionary.
  - Accepts a candidate only if Root is a dict, `Root.Pages` is a dict, and `Pages.Count` is an integer.
  - Returns the **first** valid candidate that has `/ID` (and `/Encrypt`, if any trailer is encrypted). Otherwise it returns the last valid one, then `topDict` from an xref stream, then (if there are no trailers at all) the first indexed object that has a `/Root` key ("fixes issue18986.pdf").
  - [pdf.js indexObjects](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L652)
- PDFium: every `trailer` dictionary and every `/Type /XRef` stream dictionary is merged **key by key**, with later values overriding (`UpdateTrailer`) — [cpdf_cross_ref_table.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_cross_ref_table.cpp#163).
- Poppler:
  - Considers only `trailer` at the start of a line.
  - `saveTrailerDict` requires `/Root` to be a reference. In the normal case the last such trailer wins; with `needCatalogDict` the first one wins.
  - An xref-stream Root is accepted only if its number ≤ the highest object number found.
  - If the fetched catalog is not a dictionary, `getCatalog()` reconstructs again with `needCatalogDict=true`.
  - [XRef.cc saveTrailerDict](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1041), [getCatalog](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1292)
- pypdf:
  - Merges all `trailer <<` dictionaries and xref-stream trailer keys in file order, with later keys overwriting — [pypdf _rebuild_xref_table](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L1460).
  - `root_object`: if `/Root` is not `/Type /Catalog`, it probes objects 1..`/Size` for a Catalog (first found wins), capped by `root_object_recovery_limit=10_000`. As a last resort it accepts a Root that "has /Pages but is missing the /Catalog key" — [pypdf root_object](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L222).
- lopdf: `find_latest_trailer` walks back through at most 16 `trailer` keywords. It accepts the first one whose `/Root` is a reference present in the rebuilt xref, **and otherwise returns None, so reconstruction fails**. There is no xref-stream or Catalog fallback — [lopdf reader.rs](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1612).
- hayro: collects every dictionary that has `/Root`. It keeps the **last** one whose Root resolves (including via an ObjStm) to a dict containing `/Pages`. The fallback is the object number of the last `/Type /Catalog` dictionary seen — [hayro xref.rs](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L178).
- pikepdf wraps qpdf. Its `Pdf.open(attempt_recovery=True)` is documented as "attempt to recover from PDF parsing errors" (default True), so it inherits qpdf's `reconstruct_xref` — [pikepdf API docs](https://pikepdf.readthedocs.io/en/latest/api/main.html).

### Inferences
- **Phantom objects inside non-stream objects.** PDFPundit's assembly loop (§14.2) only skips landmarks inside claimed *stream* spans (`dead.contains(lm.hdr_start)`). A literal string or comment inside an ordinary dictionary that contains `12 0 obj` would still spawn a phantom object. lopdf solves this with line-start anchoring, and MuPDF and PDFium with tokenizer continuity. Recommendation: also skip any `ObjHdr` whose `hdr_start < cur` when the previous object parsed cleanly to `endobj`. Alternatively, score non-line-start headers lower and keep them only as fallback candidates when nothing else covers that byte range.
- **Inflate probe.** PDFPundit's inflate probe (§14.3 step d) is ahead of all surveyed engines, since none uses decompression to bound a stream. Keyword-only fallbacks are the documented weak point:
  - MuPDF, qpdf, pypdf and hayro take the *first* `endstream`.
  - Poppler even *overrides a correct /Length* in reconstructed mode.
  - Keep the probe, but order the ladder to guard against two failure modes. First, a probe can succeed on a *prefix* when the data is multi-filter or has trailing garbage: accept the probe end only if an `endstream` (±EOL slack) follows it. Second, a valid declared /Length should beat a keyword scan.
- **lopdf's uniqueness rule.** lopdf's rule ("recover only if exactly one EOL-framed `endstream endobj` exists within the object's bound; refuse if ambiguous") is a good invariant for PDFPundit's step (e). When several candidates exist, record a `Finding` rather than silently taking the first.
- **qpdf's cross-check.** qpdf rejects a recovered length that would swallow another object's header. PDFPundit gets something similar for free by ordering landmarks, but it should state this explicitly as an invariant: a stream extent may never contain the `hdr_start` of an `ObjHdr` whose own body parses and ends in `endobj`, unless the inflate probe proves otherwise.
- **EOL stripping.** Engines disagree on EOL handling before `endstream`:
  - Strip one EOL: PDFium and lopdf.
  - Strip one byte: pypdf.
  - Strip all trailing whitespace: hayro.
  - Strip nothing: MuPDF, Poppler and pdf.js.
  - PDFPundit should strip exactly one EOL (CRLF | LF | CR) and never more. Trimming all whitespace, as hayro does, can cut real bytes from binary payloads such as raw image data or stored-deflate blocks. That last point is my inference, not a reported bug.
- **Duplicates.** PDFPundit's "last in byte order wins" matches qpdf, MuPDF, pypdf, lopdf and hayro, but it misses three refinements:
  - MuPDF refuses to let a copy that is truncated at EOF override an earlier complete copy. This is directly relevant to C10.
  - PDFium and Poppler let a higher generation win over byte order.
  - pdf.js lets a later copy win only if it actually parses.
  - Suggested rule: last *well-formed* copy wins, where a copy is well-formed if its body parses and is not `TruncatedAtEof`/`Unparsed`. Record every shadowed copy as forensic evidence. Treat a different generation as a distinct `(num, gen)` key, as qpdf does, rather than as a duplicate.
- **Trailer/Root selection.** §17.1 picks catalog candidates by `/Type /Catalog` or a `/Pages` ref but has no validation or tie-break. Adopt pdf.js/hayro-style validation: Root → Pages (dict) → Kids array or integer Count. Then prefer, in order:
  1. the /Root of the newest valid trailer or xref-stream dictionary;
  2. the highest-numbered valid Catalog (qpdf, MuPDF);
  3. the latest valid Catalog in byte order.
  - Also harvest `/Info`, `/ID` and `/Encrypt` from every trailer *and* every `/Type /XRef` stream dictionary, as MuPDF does. §14.4 currently decodes xref streams "for diagnosis only".
- **C1 junk before the header.** For diagnosing C1 with junk before `%PDF-`, check startxref and xref offsets both as absolute and as header-relative (qpdf `OffsetInputSource`). About half of real-world bad xrefs in the Eliot Jones sample were of this kind, so a "C2/C3 but trivially fixable" finding would be common.

### Gaps
- I did not examine Acrobat, Foxit or PDFBox source. PDFBox's `BruteForceParser` would be a useful additional reference (Java; not requested).
- I did not trace MuPDF's `pdf_parse_dict` EOF behaviour in detail. The repair loop rethrows (aborting repair) when an object at EOF fails to parse *and no Root has yet been seen*. Whether this makes MuPDF fail on files truncated mid-dictionary before any trailer is inferred from code, not tested.
- Poppler merge request !67 ("Fail when PDF contains duplicate objects") could not be read (gitlab.freedesktop.org returned an Anubis bot-block page), so I could not confirm its content or outcome.

---

## Q2. How do they rebuild or tolerate a broken page tree (missing /Kids, cycles, orphan /Page objects, inherited MediaBox/Resources)?

### Takeaway
Only hayro finds pages that are not reachable from the tree: it brute-forces every object that has `/Contents`, in byte order. The C engines repair the tree *in place* instead: they fix `/Type`, recompute `/Count`, detect cycles, drop duplicates, and default a missing MediaBox. PDFPundit's "flat /Pages + byte-order orphans + inherited attributes pinned per page" is closest to hayro plus pikepdf's `inherit_page_attributes`. It should add the in-place validations (dedup, cycle sets that include the /Kids arrays, the case where Root/Pages points at a page, per-page error budgets, and orphan-page validity scoring).

### Cited Findings
- **qpdf** (`Pages::cache`, `getAllPagesInternal`):
  - If `Root/Pages` has a `/Parent`, qpdf climbs `/Parent` to the real root ("Files have been found in the wild where /Pages in the catalog points to the first page").
  - A missing `/Kids` on the root is fatal.
  - Any node with `/Kids` is treated as internal and its `/Type` is forced to `/Pages`; kids without `/Kids` get `/Type /Page`.
  - Maximum depth is 100. A loop in nodes **or reuse of the same /Kids array** is fatal.
  - Non-dictionary kids are ignored and the tree is flattened. Direct kids are converted to indirect objects.
  - A missing MediaBox (not inherited) defaults to Letter `[0 0 612 792]`; a missing Resources becomes `<<>>`.
  - A page appearing twice is shallow-copied in normal mode but **dropped in reconstructed mode**.
  - In reconstructed mode, a page with more than 2 errors is dropped ("has too many errors; ignoring page").
  - [qpdf QPDF_pages.cc](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_pages.cc#L52)
- qpdf 12.2.0 (May 2025): "More sanity checks have been added when files with damaged xref tables are recovered in order to avoid long runtimes and large memory use. Objects with very large arrays or dictionaries (more than 5000 elements) and duplicate pages are ignored as they are almost certainly invalid" — [qpdf release notes](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/manual/release-notes.rst).
- The qpdf-dev discussion explains the motivation: "accepting any dictionary found in the pages tree without /Kids array as page object" causes "large runtimes and/or memory usage" on severely damaged files. It lists the planned checks: /Resources and /Parent presence, per-page error counting, and rejecting pages with more than 2 errors — [qpdf-dev #19](https://github.com/qpdf/qpdf-dev/discussions/19).
- **MuPDF**:
  - A kid is a Pages node if `/Type /Pages`, or if it has no `/Type`, has `/Kids` and has no `/MediaBox`. Otherwise it is a page, with a warning "non-page object in page tree" if `/Type` is not `/Page` and there is no MediaBox.
  - Empty `/Kids` → "malformed page tree". Cycles are detected with a mark list.
  - [MuPDF pdf-page.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-page.c#L214)
  - `pdf_repair_page_tree_parents` rewrites wrong `/Parent` pointers during a walk of the tree (and of the AcroForm field tree).
  - `pdf_parent_root` / `pdf_walk_parent` use a delayed Floyd cycle detector on `/Parent` chains and trigger one repair before throwing.
  - [MuPDF pdf-repair.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L1007)
  - `pdf_flatten_inheritable_page_items` pins MediaBox, CropBox, Rotate and Resources onto the page — [pdf-page.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-page.c#L421).
- **pdf.js**:
  - `checkLastPage` validates `/Count` by loading page Count−1. On failure it walks the whole tree (`getAllPageDicts`) and uses the number of pages actually found — [pdf.js document.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/document.js#L1750).
  - The walker treats a node as a page if `/Type /Page` **or** it has no `/Kids`. It accepts inline (direct) page dicts in `/Kids` (issue9540, issue21436). A reference visited twice → "Pages tree contains circular reference" error entry. In recovery mode with `ignoreErrors`, an invalid first page is replaced by an empty dict so the viewer still loads (issue15590) — [pdf.js catalog.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/catalog.js#L1432).
- **PDFium**:
  - `GetNodeType` guesses `/Pages` vs `/Page` from the presence of `/Kids` when `/Type` is missing or wrong, and rewrites `/Type` in memory.
  - `CountPages` trusts `/Count` only if 0 < Count < `kPageMaxNum`; otherwise it recounts from the kids (skipping visited dicts) and rewrites `/Count`.
  - Traversal depth is capped at `kMaxPageLevel = 1024`.
  - [PDFium cpdf_document.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_document.cpp#47)
- **Poppler**: if the top-level `/Pages` is actually a single `/Page` ("Pages top-level is a single Page … trying to recover"), it builds a 1-page document. A real-valued `/Count` ("/Count 9.0") is accepted. A count ≤ 0 or larger than the number of objects is treated as 0 — [Poppler Catalog.cc getNumPages](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/Catalog.cc#L844).
- **pypdf** `_flatten`:
  - A node without `/Type` and without `/Kids` is a page. In strict mode it must also carry `/Contents`, `/MediaBox` or `/Parent`.
  - A `visited` set detects multi-hop cycles (A→B→C→A).
  - Inheritable attributes are pushed down.
  - [pypdf _doc_common.py](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_doc_common.py#L1274)
- **hayro**:
  - `resolve_pages`: a kid with `/Type /Pages` recurses; **anything else is treated as a page** ("Let's be lenient … see corpus test case 0083781").
  - If the tree fails, `Pages::new_brute_force` iterates all xref objects **sorted by byte offset** and makes a page from every dict that has `/Contents`.
  - Default MediaBox is A4; CropBox falls back to MediaBox.
  - [hayro page.rs](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/page.rs#L68)
- pikepdf `inherit_page_attributes=True` (default) pushes `/MediaBox`, `/CropBox`, `/Resources` and `/Rotate` down onto each page — [pikepdf API docs](https://pikepdf.readthedocs.io/en/latest/api/main.html).
- In the Parsley-reducer study, MuPDF "overwhelmingly" flagged adversarially restructured page trees ("non-page object in page tree (Catalog)"), while qpdf did not flag most of them. 18 generated files sent `mutool clean` (pre-1.19) into infinite loops, and one sent qpdf past a 1-minute timeout — [Anantharaman et al., "A Format-Aware Reducer for Scriptable Rewriting of PDF Files"](https://prashant.at/files/pdffixer.pdf).

### Inferences
- **Brute-force page fallback.** PDFPundit's byte-order fallback for unreferenced pages matches hayro's `new_brute_force`. hayro's page test (any dict with `/Contents`) would also accept annotation dictionaries, which carry a `/Contents` text string. This is my inference from code reading, not a reported bug. PDFPundit should score orphan-page candidates. Require `/Type /Page`, or all of the following: no `/Kids`, `/Contents` resolving to a stream or array of streams, and `/MediaBox` or `/Parent`. Exclude dicts with `/Subtype` in the annotation set. This is what pypdf's strict rule and qpdf's planned checks point toward.
- **qpdf checks worth adopting.** Add the following to §17.3:
  - Treat a Root /Pages that points to a page (has /Parent, or /Type /Page) as either "climb /Parent" (qpdf) or "a 1-page tree" (Poppler).
  - Deduplicate pages that appear twice. qpdf copies them in normal mode but drops them in recovery, because they are almost certainly bogus.
  - Keep visited sets over both node ids and /Kids array ids.
  - Enforce a depth cap (qpdf 100, PDFium 1024).
  - Drop pages with more than 2 structural errors in recovery mode, and report each drop as a Finding.
- **MediaBox fallback.** PDFPundit's "modal sibling MediaBox" before the config default is novel; qpdf uses Letter and hayro A4. Keep it, and record which rung of the fallback chain was used.
- **/Count and ordering.** Neither PDFium's `/Count` recount nor pdf.js's "validate /Count by fetching the last page" is needed, because PDFPundit re-emits a flat tree. For ordering, "surviving /Kids order → /Parent grouping → byte order" is more careful than any engine surveyed. hayro accepts that brute-force order "could result in the order of pages being messed up".

### Gaps
- I did not inspect lopdf's `PageTreeIter` cycle handling in detail.
- I found no engine that uses content-based page ordering, such as page labels, `/StructParents` or text continuity, to order orphan pages.

---

## Q3. Object streams (ObjStm), xref streams, hybrid-reference files and incremental updates during repair

### Takeaway
Most engines expand `/Type /ObjStm` containers during reconstruction (MuPDF, PDFium, Poppler, pypdf, lopdf, hayro). qpdf and pdf.js do not: they recover compressed objects only if an xref stream survives. Engines disagree about which copy wins when an object exists both compressed and uncompressed. MuPDF's rule, which compares byte positions (bug 708286), is the most principled. PDFPundit's mandatory ObjStm expansion is correct, but its design lacks a stated precedence rule and ignores xref-stream dictionaries as trailer sources.

### Cited Findings
- **MuPDF** `pdf_repair_obj_stms`:
  - For every scanned object that had a stream and is `/Type /ObjStm`, it reads the `N` header pairs and registers members as compressed (`'o'`) entries.
  - "Bug 708286: Do not allow an object from an ObjStm to override an object that isn't in an ObjStm that we've already read, that occurs after it in the file". It compares the existing object's offset (or its container's offset) with this container's offset.
  - Broken object streams are ignored with a warning.
  - Afterwards, entries whose container is not a real `'n'` object become free ("invalid reference to non-object-stream").
  - [MuPDF pdf-repair.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L277)
- **PDFium**: for each rebuilt object, `CPDF_ObjectStream::Create` is attempted and members are added via `AddCompressed`. That call never overrides an entry with gen > 0, never lets an object stream be registered as a member of another object stream (`is_object_stream_flag`), and marks the container. A later `AddNormal` of the same number overrides a compressed entry — [PDFium RebuildCrossRef](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_parser.cpp#761), [AddCompressed](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_cross_ref_table.cpp#38).
- **Poppler**:
  - After the byte scan it fetches every object whose dictionary was followed by `stream`.
  - `/Type /XRef` → saved as a trailer candidate.
  - `/Type /ObjStm` → `constructObjectStreamEntries`, which rejects N ≤ 0 or > 1,000,000 and member numbers ≥ 1,000,000.
  - Members are inserted with `gen = index`. Under the rule `gen >= existing.gen`, compressed and uncompressed copies are compared by index against generation. This is a quirk, not a deliberate policy (my reading).
  - [Poppler XRef.cc](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1123)
- Poppler MR !70, "Skip XRef reconstruction for new-style XRef streams", states that `constructXRef` was designed for old-style tables and, if applied to xref streams, "corrupts the existing XRef::entries array" — [Poppler MR !70 (search summary)](https://gitlab.freedesktop.org/poppler/poppler/-/merge_requests/70). I saw this only in a search snippet because the page was bot-blocked.
- **pypdf**: `_rebuild_xref_table` parses each found object. `/Type /XRef` contributes trailer keys. `/Type /ObjStm` headers are read as `(num, offset)` pairs into `xref_objStm`, with a warning if the actual count ≠ `/N` — [pypdf _reader.py](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L1387).
- **lopdf**: every loaded `/Type /ObjStm` is expanded, but "Only add entries, but never replace entries": a compressed copy never overrides a directly parsed object. When the xref names a container for an object, only that container's copy is accepted ("prevents stale ObjStm copies (e.g., from linearization first-page sections) from overriding") — [lopdf load_objects_raw](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L959).
- **hayro**: expands ObjStm during the fallback scan. A member never overrides a `Normal` entry ("Somewhat arbitrary and maybe we can do better, but that seems to work for the current set of tests"). If the chosen trailer has `/Encrypt`, the whole scan is re-run with a decrypting context, because encrypted object streams could not be read on the first pass — [hayro xref.rs](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L142).
- **qpdf**:
  - During reconstruction it deletes only type-1 (uncompressed) entries and keeps type-2 entries read from xref streams.
  - It does not scan ObjStm contents: "We could iterate through the objects looking for streams and try to find objects inside of them, but it's probably not worth the trouble. Acrobat can't recover files with any errors in an xref stream" (developer's claim) — [qpdf reconstruct_xref](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L518).
  - When reading an ObjStm, it drops entries that claim to contain the stream itself, have a number < 1, have a non-increasing offset, or have an offset past the data. It caches only members that the current xref says live in that container, so appended overrides still win — [qpdf resolveObjectsInStream](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1719).
- **pdf.js**: `indexObjects` flags any object whose body contains `/XRef` followed by a byte < 64 (i.e., not a letter) as a suspected xref stream and re-reads it via `readXRef(recoveryMode)`. There is no ObjStm scan in `indexObjects`; ObjStm is handled only in `fetchCompressed` — [pdf.js xref.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L578).
- **Incremental updates and hybrid files**:
  - qpdf's normal path makes "the first reference to an object that we see, which is the one in the latest xref table in which it appears", win by reading newer sections first — [insertXrefEntry](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1128).
  - qpdf has a special case for an xref-stream object number reused in a later update, where "the old object is already in the cache and so effectively prevails over the reused object" — [qpdf readObjectAtOffset](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1607).
  - qpdf 8.0.2: "When a loop is detected while following cross reference streams or tables, treat this as damage instead of silently ignoring the previous table" — [qpdf release notes](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/manual/release-notes.rst).
  - Caradoc cites Bogk et al.: two updates can each name the other as `/Prev`, since "an update is not required to be located before its predecessor in the file" — [Caradoc](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).
- Caradoc's crawl measured the prevalence of these structures: incremental updates in 64% of parsed files, object streams in 35%, free objects in 29%, encryption in 5% (10,000 files; 8,993 parsed) — [Caradoc, SPW 2016](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).
- A Rust PDF crate (pdf_oxide) reported a bug: after reconstruction the trailer `/Size` was set to the number of entries found, not max+1. "For sparse numbering or documents with object-stream-compressed objects, len < max+1". An incremental save then "can assign an object number that already exists, silently redefining it" — [pdf_oxide issue #1742](https://github.com/yfedoseev/pdf_oxide/issues/1742). lopdf explicitly normalises `size = max_id + 1` after reconstruction — [lopdf](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1432).
- Shadow attacks (NDSS 2021) show that the "latest incremental update wins" semantics are exploitable: 16 of 29 viewers were vulnerable to content hidden or replaced through incremental updates on signed PDFs — [NDSS 2021, Mainka et al.](https://www.ndss-symposium.org/ndss-paper/shadow-attacks-hiding-and-replacing-content-in-signed-pdfs/).

### Inferences
- **Precedence between compressed and uncompressed copies.** §14.4 pushes ObjStm members after the main loop, and §17.2's "last in byte order wins" is ambiguous for them. Adopt MuPDF's rule: a compressed member takes the *container's* byte offset as its position. It then competes in byte order with uncompressed copies, and a later uncompressed copy wins. Within one container, the member order is irrelevant.
- **Duplicate across containers.** When the same number appears in two containers, the later container wins. lopdf's "never replace", when combined with its optional rayon parallel iteration, looks order-dependent in the reconstructed case (my inference from code, not a reported bug). PDFPundit must iterate deterministically in byte order.
- **Using xref streams.** Treating xref streams as "diagnosis only" discards their dictionary. Use them as trailer candidates (Root, Info, ID, Encrypt, Size), as MuPDF, pdf.js, PDFium, qpdf and pypdf do. A *decodable* xref stream's type-2 entries are also the only record of which container is authoritative for an object when several exist; qpdf and lopdf use them to reject stale copies. Use them as a tie-breaker, not as ground truth.
- **Forensic revision preservation.** Because 64% of files in one crawl had incremental updates, "last wins" silently collapses revision history. A forensic tool should keep every shadowed revision in the `CarveReport` and flag cases where later revisions change page content, form values, or annotations appended after a signature's `/ByteRange` (shadow-attack signal).
- **Emitted /Size.** Set the emitted `/Size` to max id + 1 (lopdf does this automatically on save), never to a count of objects.

### Gaps
- I did not verify how each engine handles *linearized* files whose first-page xref section conflicts with the main xref during reconstruction. Only lopdf's comment mentions stale first-page ObjStm copies.
- I did not find a published measurement of how often hybrid-reference (`/XRefStm`) files appear in damaged corpora.

---

## Q4. Truncated files (missing %%EOF, stream cut mid-way)

### Takeaway
Most engines treat a stream with no findable `endstream` as lost or empty (qpdf, PDFium, pdf.js, lopdf, hayro). MuPDF keeps data up to EOF, minus a 9-byte window artifact. MuPDF also prevents a truncated trailing copy from overriding a good earlier one. PDFPundit's "clamp at EOF → `TruncatedAtEof`" recovers more than any surveyed engine; it needs MuPDF's shadowing guard and a clear partial-inflate path.

### Cited Findings
- qpdf: `recoverStreamLength` searches from the data start to EOF for `endstream` or `endobj`. If neither exists, "unable to recover stream data; treating stream as empty" (length 0). An object ending at EOF without trailing whitespace raises "EOF after endobj" — [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1440).
- MuPDF:
  - The `endstream` sliding-window scan stops at EOF, and `stm_len = tell − stm_ofs − 9`. A stream truncated mid-data therefore keeps all data except the last 9 bytes, which were still in the window.
  - A missing `endobj` is only a warning.
  - A header followed immediately by EOF throws "truncated object", and a dict that fails to parse *at EOF* is rethrown. Both prevent "a truncated object at EOF [from overwriting] a good one".
  - [MuPDF pdf_repair_obj](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L73)
  - If no Root has been seen yet when that happens, repair is aborted ("If we haven't seen a root yet, there is nothing we can do, but give up"). Otherwise: "cannot parse object … ignoring rest of file" — [pdf_repair_xref_base](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L554).
- PDFium: a declared length that runs to or past EOF is discarded (`len = -1`). If no `endstream`/`endobj` is found, `ReadStream` returns nullptr and the stream is lost — [PDFium ReadStream](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_syntax_parser.cpp#770).
- pdf.js: if `#findStreamLength` finds nothing it returns −1, which raises "Missing endstream command." In `indexObjects`, an object with no following terminator extends to the end of the buffer — [pdf.js parser.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/parser.js#L740).
- pdf.js also tolerates a truncated keyword: `endstrea` followed by whitespace (issue10004) — [pdf.js parser.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/parser.js#L666).
- pypdf `_find_eof_marker`:
  - Walks back line by line from the end.
  - Accepts truncated markers `%%EO`, `%%E`, `%%`, `%` ("EOF marker seems truncated").
  - Warns "startxref found while searching for %%EOF. The file might be truncated".
  - Non-strict mode continues with "EOF marker not found".
  - [pypdf _reader.py](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L787)
- lopdf: `recover_stream_length` requires `endstream` followed by `endobj`, so a truncated stream is not recovered. `payload_end_by_length` uses /Length only as a skip hint during the scan and returns None if the hint "points outside the buffer" — [lopdf reader.rs](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1531).
- hayro: `parse_fallback` requires `endstream`, so a truncated stream is lost — [hayro stream.rs](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/object/stream.rs#L314).
- Poppler: without an xref, a missing `endstream` means /Length is padded by 5000 bytes. In reconstructed mode the stream end comes from the next line-start `endstream`; if there is none after the stream start, `getStreamEnd` returns false and /Length is used — [Poppler Parser.cc](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/Parser.cc#L229).

### Inferences
- **Shadowing guard for truncated copies.** A truncated incremental update would put its (truncated) copies last in byte order, so PDFPundit's rule would pick them over complete earlier revisions. Adopt MuPDF's guard: `TruncatedAtEof`, `NoEndobj`-at-EOF and `Unparsed` copies never shadow an earlier complete copy of the same `(num, gen)`.
- **Truncated compressed streams.** For a truncated Flate stream, the inflate probe will end with an error (not `StreamEnd`). Route it straight to the §16 `Prefix` salvage rather than to keyword scanning, since no `endstream` exists. MuPDF's `tell − 9` behaviour shows that a naive sliding-window scan silently drops tail bytes at EOF; PDFPundit's clamp must include every byte up to `buf.len()`.
- **Trailer lost.** When the trailer is gone, a surviving object that was cut off at EOF is often the last page or the Info dictionary. MuPDF's "give up if no Root yet" is a lesson against making the Root search depend on scan order. PDFPundit already does the Root search after carving, which is the right structure.

### Gaps
- I did not test the engines against actual truncated fixtures; the behaviour above comes from code reading.
- I did not determine Poppler's exact behaviour when /Length overruns EOF (whether `makeSubStream` clamps) from code alone.

---

## Q5. Bug reports, CVEs and fuzzing findings where repair went wrong, and the lessons for carver invariants

### Takeaway
Repair code is a recurring source of memory-safety and denial-of-service bugs:
- Unbounded object numbers taken from object-stream headers.
- Recursive object-stream references.
- Infinite recursion in resolve/unparse.
- Update loops.
- Huge arrays or dictionaries in recovered objects.
- Page-tree cycles.
- Phantom headers inside stream payloads.
- Wrong /Size after reconstruction.

All mature engines have since added explicit caps and "only once" guards. PDFPundit's Rust code avoids the memory-safety class, but it must adopt the caps and the one-pass guarantees to avoid the denial-of-service and wrong-object classes.

### Cited Findings
- **CVE-2012-5340 (MuPDF 1.0 / SumatraPDF 2.1.1)**: in `pdf_repair_obj_stm`, an object number read from an ObjStm header overflowed in `lex_number`. It became negative, bypassed the bounds check, and was written to `xref->table[n]` — [Exploit-DB 23246](https://www.exploit-db.com/exploits/23246). The current code rejects `n < 0` and `n >= PDF_MAX_OBJECT_NUMBER` — [MuPDF pdf-repair.c](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L312).
- **CVE-2018-6544 (MuPDF 1.12.0)**: "pdf_load_obj_stm … could reference the object stream recursively and therefore run out of error stack" — [OpenCVE CVE-2018-6544](https://app.opencve.io/cve/CVE-2018-6544). The same class is guarded today by qpdf ("object stream claims to contain itself"), PDFium ("Don't add known object streams to object streams") and hayro ("cycle detected in object stream") — [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1797), [PDFium](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_cross_ref_table.cpp#51), [hayro](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L597).
- **CVE-2017-9208/9209/9210 (qpdf 6.0.0)**: infinite recursion and stack exhaustion in `releaseResolved`, `QPDFObjectHandle::parseInternal` and unparse functions via crafted PDFs; fixed in 7.0.0 — [OpenCVE CVE-2017-9208](https://app.opencve.io/cve/CVE-2017-9208), [CVE-2017-9209](https://app.opencve.io/cve/CVE-2017-9209), [CVE-2017-9210](https://app.opencve.io/cve/CVE-2017-9210). qpdf now detects "loop detected resolving object" (for example, a /Length that refers to its own object) and resolves the object to null — [qpdf resolve](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1668).
- **CVE-2019-15860**: "Xpdf 2.00 allows a SIGSEGV in XRef::constructXRef" — [OpenCVE CVE-2019-15860](https://app.opencve.io/cve/CVE-2019-15860). **CVE-2018-20481 (Poppler 0.72.0)**: `XRef::getEntry` "mishandles unallocated XRef entries", causing a NULL dereference "when XRefEntry::setFlag … is called from Parser::makeStream" — [OpenCVE CVE-2018-20481](https://app.opencve.io/cve/CVE-2018-20481).
- **Re-entrancy and one-shot guards**:
  - qpdf: "Avoid xref reconstruction infinite loops"; reconstruct is allowed once (`reconstructed_xref`).
  - qpdf: more than 1000 warnings during recovery → "too many errors while reconstructing cross-reference table".
  - MuPDF: "Repair failed already - not trying again" (`repair_attempted`).
  - hayro: repairs at most once ("attempt was made at repairing xref, but object … still couldn't be read").
  - Sources: [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L313), [MuPDF](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L435), [hayro](https://github.com/LaurenzV/hayro/blob/ae09437b4e5939fb76125ed8c5ed3c38d6d6ac09/hayro-syntax/src/xref.rs#L579).
- **Resource caps observed in current code**:
  - qpdf: object id ≤ file_size/3; tokens during the scan ≤ 10 chars; last 100 trailers; page-tree depth 100 — [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L239).
  - qpdf release notes: arrays/dicts > 5000 elements ignored in recovery (12.2.0); "reasonable limits on nesting and the depth of direct objects that may be created while repairing or recovering documents" (12.3.0, Jan 2026); improved "validation of object ids and generation numbers … during reconstruction of damaged files" (12.4.2-dev) — [qpdf release notes](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/manual/release-notes.rst).
  - lopdf: `MAX_RECONSTRUCTED_OBJECTS = 1_000_000`, `MAX_TRAILER_CANDIDATES = 16` — [lopdf](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L497). v0.44.0 added a bound on load-time decompression (`LoadOptions.max_decompressed_size`) against decompression bombs, and v0.45.0 fixed "four crash bugs from crafted PDFs" (#532) — [lopdf CHANGELOG](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/CHANGELOG.md).
  - pypdf: `root_object_recovery_limit` 10,000 and `maximum_declared_stream_length` — [pypdf](https://github.com/py-pdf/pypdf/blob/54d35184b9e984834823b3558dc096bd4e6c9e80/pypdf/_reader.py#L113).
  - PDFium: `kMaxObjectNumber = 24·1024·1024` — [cpdf_parser.h](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_parser.h#64); `kMaxPageLevel = 1024` — [cpdf_document.cpp](https://pdfium.googlesource.com/pdfium/+/8ca5b735df4263f43c830479b78213854a392c22/core/fpdfapi/parser/cpdf_document.cpp#38).
  - Poppler: ObjStm `N ≤ 1,000,000` — [Poppler](https://gitlab.freedesktop.org/poppler/poppler/-/blob/c8dc23d564f233875386b7aa1d59fc909ea58799/poppler/XRef.cc#L1123).
- **Phantom headers inside payloads**: lopdf documents why it skips stream payloads ("an uncompressed payload may embed convincing `N G obj` lines whose later offsets would otherwise override the genuine entries") and why it caps object numbers ("so a forged header can neither shadow a genuine entry nor poison the reconstructed size") — [lopdf](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1461).
- **Wrong-object-on-duplicate**:
  - MuPDF bug 708286 (an ObjStm copy overriding a later uncompressed object) — [MuPDF](https://github.com/ArtifexSoftware/mupdf/blob/d587982f971e2404e72b0539b2d9c60ad2c9b03f/source/pdf/pdf-repair.c#L323).
  - pdf.js issue13783 (a later duplicate that fails to parse must not replace a good one) — [pdf.js](https://github.com/mozilla/pdf.js/blob/d52fdf411a6e4d338180687456e0df019e28475e/src/core/xref.js#L530).
  - qpdf's reused-xref-stream-number cache bug, which the source comment says may also exist for linearization hint tables — [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1618).
- **Denial of service via adversarial restructuring**: 18 of 4,200 adversarially modified files made `mutool clean` (< 1.19) loop, and 1 pushed qpdf past a 1-minute timeout. 3,177 (MuPDF) and 221 (qpdf) previously accepted files were rejected after modification — [Anantharaman et al.](https://prashant.at/files/pdffixer.pdf).
- **Differential risk**: Caradoc's experiments on Adobe Reader, Evince, Foxit, Sumatra, Chrome and Firefox found inconsistent handling of generation numbers, cyclic outlines and page trees. "cyclic structures can trigger infinite recursion on most readers" — [Caradoc](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf). Albertini and Coldwind's "schizophrenic" files show one PDF rendering differently in different readers because of parser tolerance — [Schizophrenic files (slides)](https://www.slideshare.net/hashdays/schizophrens).

### Inferences
Carver invariants PDFPundit should adopt, each with a fixture:
1. **Forward progress and a single pass.** Carve, then reconcile, never recursively re-carve. There is no "repair inside resolve" path; the repair engines only guard this with once-flags.
2. **Bounded identifiers.** Parse object numbers and gens with checked arithmetic. Reject num = 0, num > min(8,388,607 [the ISO 32000 implementation limit, from my memory, not re-verified here], file_len/3 as qpdf does), and gen > 65535. Apply the same to ObjStm header numbers and offsets: enforce monotonic offsets, `First + off < len`, no self-containment, and no container that is itself a member of another container.
3. **Global budgets.** Cap:
   - landmark and object count (lopdf uses 1e6);
   - trailer candidates (16–100);
   - array/dict size in recovered objects (qpdf uses 5000);
   - nesting depth (the plan already has 100);
   - inflated bytes per stream and in total (lopdf `max_decompressed_size`);
   - warnings, with a qpdf-style "too damaged" abort that still returns a partial `CarveReport`;
   - byte-flip attempts (§16 already caps these).
4. **Cycle-safe graph walks.** Use visited sets for /Pages, /Kids arrays, /Parent chains (MuPDF's Floyd approach), outlines, the AcroForm field tree, /Length indirection (a /Length pointing to its own stream resolves to "unknown") and ObjStm containment.
5. **No shadowing by worse copies.** A later copy wins only if it is well-formed. Truncated or unparsed copies never shadow complete ones.
6. **No phantom objects.** A header inside a claimed stream span, a string literal or a comment is never promoted. Make this a property test that injects `N G obj` into every such context.
7. **Emitted /Size is max+1** and never the object count (pdf_oxide #1742).
8. **Determinism.** Iteration order must be byte order and independent of hash maps or parallelism, so the same input always yields the same repair and Findings.

### Gaps
- I did not systematically mine OSS-Fuzz issue trackers (many are restricted) for repair-specific findings in pdf.js, PDFium or Poppler.
- I could not access Poppler's GitLab issues because of bot protection.

---

## Q6. Published comparisons of repair quality across tools

### Takeaway
I found no rigorous public benchmark that ranks qpdf, MuPDF, pdf.js, PDFium, Poppler and others by how many damaged files each can open or repair. What exists is indirect:
- Parser-differential corpora (DARPA SafeDocs).
- One reducer paper with small repair counts for `mutool clean`, qpdf and Caradoc.
- Caradoc's crawl statistics.
- A blog sample putting the bad-xref rate at about 0.5%.

This gap is itself a finding. PDFPundit's benchmark harness (§8) could produce the first such comparison.

### Cited Findings
- **Parsley reducer paper** (Anantharaman, Cheung, Boorman, Locasto), Dataset 1 (6,753 files) / Dataset 2 (7,171 files) taken from DARPA SafeDocs collections:
  - "Fixups after transformation": MuPDF 48 / 31, `mutool clean` **726 / 96**, qpdf 5 / 16, PDFMiner 217 / 401.
  - "Errors after transformation": `mutool clean` 0 / 0, qpdf 1 / 2, PDFMiner 11 / 25.
  - Comparing cleaners on Dataset 2 (fixups / errors introduced): Caradoc 24 / 0, `mutool clean` 24 / 32, Parsley 47 / 4. Several `mutool clean` outputs "were empty, containing no data", and one `mutool clean` output made both MuPDF and qpdf demand a password on a file qpdf had previously opened.
  - [Anantharaman et al., "A Format-Aware Reducer for Scriptable Rewriting of PDF Files"](https://prashant.at/files/pdffixer.pdf). The venue and year are not visible in the fetched PDF text; its references run to 2021.
- **Caradoc** (Endignoux, Levillain, Migeon, IEEE SPW 2016): from 10,000 crawled files, the relaxed parser (with ad-hoc fixes such as offset-0 entries) normalised 8,993. Only 1,465 passed the strict parser directly — [Caradoc](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).
- **Bad-xref prevalence**: 23 of 3,977 Common Crawl PDFs (~0.5%) had a bad xref declaration; about half had a non-zero content start offset — [Eliot Jones (2025)](https://eliot-jones.com/2025/8/pdf-parsing-xref).
- **SafeDocs corpora**: the "universes" define validity by agreement among parsers (xpdf, qpdf, pdfbox, mutool; later poppler and pdfminer). A later study (VALARIN) used 31 parser configurations and found that "PDF parsers exhibit mutual biases when recovering from specification ambiguities" — [PDF investigation with parser differentials and ontology (IEEE TIFS 2024)](https://ieeexplore.ieee.org/document/10638649/), [UNSAFE-DOCS corpus](https://pdfa.org/unsafe-docs-a-new-safedocs-corpus-including-the-voice-of-the-offense/), [pdf-association/safedocs](https://github.com/pdf-association/safedocs).

### Inferences
- The only quantitative signal suggests that `mutool clean` repairs the most files but can also produce empty or broken outputs. qpdf repairs fewer but introduces few new errors.
- For PDFPundit, "opens after repair" is an insufficient metric. The verification step (§17.4) should also count objects preserved, pages preserved, and rendered-content differences against a reference render, not just whether lopdf can reload the file.
- A useful benchmark would run the §8 corpus (C1–C10) through `qpdf --check`/`qpdf in.pdf out.pdf`, `mutool clean`, pdf.js (`getDocument` + page count), PDFium (`pdfium_test`), `pdftocairo`, pypdf (`strict=False`) and PDFPundit. It would measure page count and text/image survival against the undamaged originals, which the REPDF-style corpus provides.

### Gaps
- I found no study measuring *repair fidelity* (content preserved relative to the undamaged original) across these libraries.
- I found no comparison that includes the Rust crates (lopdf, hayro, pdf-rs).

---

## Q7. Commercial repair tools (Stellar, Recovery Toolbox, PDF Fixer, iLovePDF, Acrobat silent repair): anything verifiable?

### Takeaway
Nothing verifiable about their algorithms. Success-rate numbers circulate only in SEO "best tools" listicles with no stated corpus or methodology. The only defensible public statements about Acrobat are that it repairs structural damage on open and that you must save the file to keep the repair. There is also qpdf's developer claim that Acrobat cannot recover files with errors in an xref stream.

### Cited Findings
- A web search surfaced listicle claims such as "Stellar successfully repaired 87% of corrupted files", "PDF Repair Toolbox achieved 82%" and iLovePDF "68% for minor corruption, 35% for severe damage". The likely source page ([mrgrid.io review](https://mrgrid.io/articles/pdf-repair-tools-review)) returned HTTP 503 when fetched, so neither the numbers nor any methodology could be verified. Similar pages ([PDFTEQ 2026 list](https://pdfteq.com/blog/posts/best-pdf-repair-tools-2026), [Wondershare Repairit list](https://repairit.wondershare.com/file-repair/best-pdf-repair-tool.html)) are vendor or affiliate content.
- An Adobe Community Expert, on the Acrobat message "This file is damaged but is being repaired", says: "Sounds like Acrobat repaired a structural error within the PDF. In conclusion the PDF has to be saved to keep the changes" — [Adobe Community thread](https://community.adobe.com/questions-12/this-file-is-damaged-but-is-being-repaired-1526515).
- qpdf's maintainers state in source: "Acrobat can't recover files with any errors in an xref stream" — [qpdf reconstruct_xref comment](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L518).
- qpdf and lopdf comments cite Adobe's tolerance of CR-only after `stream` and of slightly-off startxref offsets ("common readers (Acrobat, poppler, mupdf, pdf.js) recover from them") — [qpdf](https://github.com/qpdf/qpdf/blob/54d6053af283bbeb8b325f4886c0f65cc51f2b80/libqpdf/QPDF_objects.cc#L1381), [lopdf](https://github.com/J-F-Liu/lopdf/blob/0e781ce05f083330ecc24ac8c67833fbd076e7a2/src/reader.rs#L1638).
- Caradoc reports Adobe Reader PSIRT tickets 3939–3942, filed by Endignoux for structural-parsing issues found with crafted files — [Caradoc references](https://paperstreet.picty.org/yeye/resources/conf-spw-EndignouxLM16/document.pdf).

### Inferences
- PDFPundit should not benchmark against commercial success claims. If commercial comparison is wanted, run the tools on PDFPundit's own labelled corpus.
- Acrobat's "repair on open, prompt to save" matches what PDFPundit does by construction (re-emission). The forensic difference is that PDFPundit reports *what* it changed, which Acrobat does not.

### Gaps
- There is no public technical documentation of the algorithms used by Stellar, Recovery Toolbox, PDF Fixer or iLovePDF. iLovePDF's server-side stack is undisclosed.
- There is no independent, methodologically described test of these tools.
- Acrobat's repair algorithm is undocumented. Claims found online (for example, that it "scans sequentially and rebuilds the xref") come from secondary sources without primary citations.

---

### Appendix: technique → tools → PDFPundit verdict (quick reference; details and sources above)

| Technique | Who does it | PDFPundit plan | Verdict |
|---|---|---|---|
| Header search in the first 1 KiB, rebasing offsets | qpdf, MuPDF, pdf-rs | C1 detector; re-emit | Add a header-relative offset check to C2/C3 diagnosis |
| `startxref` tail search, then any-`startxref` retry, then ±64 B xref-keyword correction | qpdf, lopdf, PDFium | Parse xref for diagnosis only | Add as "cheap C3 fixes" with Findings |
| Object-header scan | all | memchr landmarks + ≤24 B backtrack | Good; add line-start scoring and skip headers inside parsed non-stream objects |
| Skip stream bodies during scan | MuPDF, PDFium, lopdf | dead ranges from resolved extents | Stronger than all engines, if extents are right |
| Stream extent: /Length verify → first `endstream` | all | ladder a→f | Keep; add lopdf's unique-candidate rule and qpdf's "no foreign header inside" check |
| Inflate-probe extent | none found | step d | Novel; guard against prefix success by requiring an `endstream` after the probe end |
| Duplicates: last-in-byte-order | qpdf, MuPDF, pypdf, lopdf, hayro | yes | Add a well-formedness gate (pdf.js), a truncation guard (MuPDF), and separate keys per gen |
| ObjStm vs uncompressed precedence | MuPDF (by container offset), lopdf/hayro (uncompressed always wins), PDFium (later wins) | unspecified | Adopt MuPDF's container-offset rule |
| Trailer/Root selection with validation | pdf.js, hayro, qpdf, MuPDF | catalog candidates by /Type or /Pages | Add Root→Pages→Kids/Count validation and a tie-break |
| Harvest trailer keys from xref-stream dicts | MuPDF, qpdf, pdf.js, PDFium, pypdf | xref streams diagnosis only | Adopt |
| Page tree: fix /Type by /Kids, dedupe, cycle sets, depth caps, error budget | qpdf, PDFium, MuPDF, pdf.js, pypdf | flat rebuild | Adopt the validations |
| Orphan-page brute force | hayro (dicts with `/Contents`) | byte-order orphans | Adopt, with stricter page scoring |
| Missing MediaBox default | qpdf Letter, hayro A4 | own → inherited → modal sibling → config | Keep (novel) |
| Positional orphan↔dangling-ref matching | none found (engines resolve missing refs to null) | REPDF-style | Novel; report confidence; try same-number/other-gen first (pdf.js generation fallback) |
| Truncated stream salvage | MuPDF partial (loses 9 B); others drop the stream | clamp to EOF + partial inflate | Better than all; add the truncation shadowing guard |
| Resource caps / once-only repair | qpdf, MuPDF, lopdf, pypdf, PDFium, hayro | depth 100; byte-flip budget | Add object, trailer, element, warning and inflate caps |
