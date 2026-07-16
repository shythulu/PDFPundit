---
planStatus:
  planId: plan-pdfpundit-desktop-app
  title: PDFPundit — Rust Multiplatform PDF Analysis & Repair Tool
  status: draft
  planType: feature
  priority: medium
  owner: shythulu
  stakeholders: []
  tags:
    - rust
    - desktop
    - pdf
    - forensics
    - repair
    - cross-platform
  created: "2026-07-15"
  updated: "2026-07-16T13:39:02.000Z"
  progress: 0
---
# PDFPundit — Rust Multiplatform PDF Analysis & Repair Tool

## Summary

PDFPundit is a cross-platform, pure-Rust tool for **forensic-level analysis and
repair of corrupted `.pdf` files**. A user drops a damaged PDF onto the window
(or picks it via a `.pdf`-filtered browser); PDFPundit reconstructs it from the
raw bytes up — carving surviving objects, rebuilding the cross-reference table
and trailer, restoring the page tree, and re-mapping fonts — then writes a clean,
renderable `<name>.repaired.pdf`. It targets Windows, macOS, and Linux from a
single codebase.

The emphasis is **forensic depth with simple, reliable results**, presented in a
**cute retro interface** (think early-90s terminal/utility aesthetic). The design
is grounded in the peer-reviewed **REPDF** method (DFRWS 2026 — *"Repairing
corrupted PDF files through font mapping and object relationship reconstruction"*,
[repo](https://github.com/dfrc-korea/REPDF)), whose ten-scenario corruption
taxonomy (C1–C10) defines PDFPundit's analysis and repair passes and serves as its
benchmark corpus.

A second capability (a fast-follow after repair) is **high-quality PDF → Markdown
export**. Because the repair pipeline reconstructs correct fonts and `/ToUnicode`
mappings, the text feeding extraction is *correct Unicode* — so export quality on
damaged or CID-font PDFs beats tools that extract from the raw file. Layout,
column, and table reconstruction reuse the engine-agnostic spatial-projection core
of **`spdf`** (MIT), driven by our own pure-Rust `PdfEngine` (no PDFium).

> **Detailed technical design:** [pdfpundit-technical-design.md](pdfpundit-technical-design.md)
> — module tree, algorithms, event architecture, UI spec, and the finalized
> crate stack (this plan's table below reflects those decisions).

## Goals

- Ship a self-contained Rust binary on Windows, macOS, and Linux from one codebase.
- Drop-anywhere drag-and-drop plus a `.pdf`-filtered filesystem browser to add files.
- Forensically diagnose a PDF against the REPDF C1–C10 corruption taxonomy.
- Repair corruption by byte-level carving + object-relationship reconstruction, writing a new renderable file — never mutating the original.
- Present findings and repair results simply and reliably in a cute retro UI.
- Keep a searchable history of analyzed/repaired files with per-run findings.
- Export a PDF to **high-quality Markdown** (headings, paragraphs, lists, tables,
  images, links) with correct Unicode, in reading order — pure-Rust, no PDFium.

## Non-Goals (v1)

- PDF editing, annotation, form-filling, or signing.
- Cloud sync / accounts / multi-user.
- Mobile (iOS/Android) targets.
- Re-implementing REPDF's server or web UI — we adopt its *method*, not its hosting.
- Office/non-PDF input conversion and OCR of scanned pages for Markdown export
  (spdf's LibreOffice/Tesseract paths) — deferred to the "Pundit+" phase.

## Prior Art & Repair Method (REPDF)

REPDF repairs corrupted PDFs "by using **font mapping** with an external font
database and **reconstructing residual object relationships**." Rather than trusting
the file's own (possibly destroyed) cross-reference structures, it treats the PDF
as a forensic artifact and rebuilds it from what survives on disk. PDFPundit
re-implements this method in Rust and evaluates against REPDF's public corpus
(1,000 corrupted PDFs, 6 languages, 10 corruption types × 2 creation methods).

### Corruption taxonomy (C1–C10) → PDFPundit passes

| ID | Corruption | How REPDF's corpus induces it | PDFPundit repair approach |
| --- | --- | --- | --- |
| C1 | Header corruption | Overwrites up to 12 bytes of the `%PDF-` header | Detect/rewrite a valid `%PDF-x.y` header from the first surviving object generation. |
| C2 | XRef table removal | Deletes the cross-reference table before the trailer | **Carve** all `N G obj … endobj` objects, recompute byte offsets, synthesize a fresh xref table. |
| C3 | Trailer damage | Removes the EOF trailer via the `startxref` pointer | Rebuild trailer: locate `/Root`/`/Info` among carved objects, emit `trailer`/`startxref`/`%%EOF`. |
| C4 | Page tree broken | Deletes a span around `/Pages`, `/Kids`, `/Count` | Reconstruct the page tree by collecting `/Type /Page` objects and rebuilding `/Pages`→`/Kids`→`/Count`. |
| C5 | Object tag removal | Strips an object's number and `obj` keyword | Re-detect object boundaries by structure (dict/stream heuristics) and re-assign object numbers. |
| C6 | Font mapping loss | Deletes `/Font` in a page's `/Resources` | Re-link `/Page` → `/Font` from carved font objects; restore the `/Resources`→`/Font` mapping. |
| C7 | Font stream deletion | Removes `/FontFile2` stream data | Substitute from the **external font database** by font name/metrics; re-embed a compatible `/FontFile2`. |
| C8 | Font resources deletion | Removes `/FontFile2` **and** `/ToUnicode` | Restore font program *and* rebuild `/ToUnicode` CMap so text extracts/searches correctly. |
| C9 | Zlib tampering | Flips a byte inside a Flate-compressed stream | Detect Flate decode failure; attempt partial/resynchronized inflate and salvage recoverable content. |
| C10 | Partial truncation | Cuts the file, keeping ~70% of bytes | Carve whatever objects survive and assemble a minimal valid document from them. |

### REPDF's repair pipeline (7 steps)

The paper's key insight is **template-based reconstruction**: rather than patching
the broken file, REPDF rebuilds the document *into a pre-made template PDF* that
already contains full, reference-embedded fonts. PDFPundit follows the same seven
steps (paper §3):

1. **Build template PDFs / font database** — a set of reference PDFs, each embedding
   a *full* (non-subset) `/FontFile2` plus `W`/`Widths` glyph metrics, and the
   font's `glyphorder` + `cmap` tables. This is precomputed and bundled, not built
   at runtime. It's what makes font recovery possible when originals are gone.
2. **Byte-level object/stream scan** — scan raw bytes for `obj`/`endobj`/`stream`/
   `endstream`, recovering object blocks even when tags/numbers are damaged; note
   each object's `/Type` (`/Page`, `/Pages`, `/Font`, `/XObject`, `/Catalog`).
3. **Extract page info** — for every `/Type /Page`, read `/Contents` and `/MediaBox`
   to place restored content in the right spot.
4. **Decompress & classify streams** — inflate stream data; when `/Subtype`/object
   numbers are lost, classify by content: `/Subtype /Image` → image; `BT`/`ET`/
   `Tf`/`Tj` → content stream; `Do` → image draw.
5. **Analyze resources** — insert resource refs + data into the template so each
   page renders; images that can't be placed are extracted to standalone files.
6. **Reconstruct font mappings** — two cases:
  - `/BaseFont` name survives → look it up in the template DB, insert the full font.
  - name lost → **infer** the font: pull hex codes `<…>` from the content stream,
     map code→Unicode via each candidate font's `glyphorder`/`cmap`, generate a
     candidate string per font, and **score** against bundled 6-language word
     dictionaries (bonus for real words; penalties for mixed-language / rare chars).
     If `/ToUnicode` survives, decode the real text and use it as a custom dictionary.
7. **Emit repaired PDF** — integrate recovered content/font/image objects into the
   template; each page gets `/Contents`, `/MediaBox`, `/Resources`.

**Shared engine primitives** (steps map onto these modules): object **carver**
(step 2), stream **inflate + classifier** (step 4), **object-graph reconstructor**
(steps 3, 5), **template/font database** (steps 1, 6), and repaired-file **emitter**
(step 7).

**Interactive font selection.** The paper notes automatic scoring is unreliable for
languages like Arabic (word forms change with grammar), so it calls for "an
interactive step in which the user reviews the recovered text and selects the most
contextually appropriate font." This maps perfectly onto PDFPundit's TUI — when the
font scorer is low-confidence, we present the top candidate renderings and let the
user pick. (A genuine advantage of our interactive TUI over REPDF's batch web tool.)

**Known difficulty (from the paper's results, avg text recovery 90.67%):** C1–C5 ≈100%;
C7 ≈99%; C6/C8 ≈90% but **Arabic and "Print to PDF" files are weak** (font names
like `CIDFont+F1` change, defeating name lookup); C9 (zlib) ≈60%; C10 truncation is
~99% for "Save As" but ~35% for "Print to PDF" (content/font streams sit near EOF).

## Proposed Architecture

Chosen stack (a pure-Rust, single-process TUI — "FrankenTUI"):

| Layer | Chosen approach | Rationale |
| --- | --- | --- |
| App shell | **FrankenTUI** — a `ratatui` terminal UI over `crossterm`, with mouse support | Pure-Rust, single self-contained binary per OS, no webview or system deps. Runs anywhere a terminal does and keeps the whole app in one Rust process. |
| Input / drag-drop | `crossterm` events + terminal path-paste | Keyboard + mouse in-terminal. Dragging a file onto most terminals pastes its absolute path; the input layer captures that as an "add file" action. A `browse…` action opens an in-TUI filesystem picker filtered to `.pdf`. |
| Forensic engine | Custom Rust carver + reconstructor, orchestrated by a job runner on std threads + mpsc channels (no async runtime) | The core of the app: byte-level object carving, xref/trailer rebuild, object-graph reconstruction, and the C1–C10 repair passes. Runs off the UI thread and streams progress to the panels. |
| PDF core | **lopdf** (pure Rust) to build/emit the template + repaired doc (REPDF used Python `pikepdf`) | lopdf models objects/dictionaries/streams and re-serializes a valid file. The carver operates below lopdf on raw bytes when the file is too broken to parse. **Pure-Rust only — no `qpdf`/`mupdf`/`pdfium` FFI** (see crate stack). |
| Font tooling | `skrifa`/`read-fonts` (or `allsorts`/`ttf-parser`) for glyf/cmap/hmtx (REPDF used Python `fonttools`) | Reads `glyphorder`/`cmap`/`W` metrics from bundled fonts to drive code→Unicode inference and metric restoration. |
| Template / font DB | Pre-built template PDFs with full-embedded fonts + a JSON font index; bundled 6-language word dictionaries | The forensic reference data. Fonts must cover en/fr/es/ar/hi/zh (Noto family is the natural open-licensed choice). Backs C6–C8. |
| Persistence | **JSON store** via `serde` (atomic write-temp-then-rename) | Stores the recent-file history plus each run's findings and repair log. (`rusqlite --features bundled` would compile SQLite's C source — conflicts with the pure-Rust rule.) |
| Packaging | `cargo-dist` cross-platform archives + installers | One CI matrix produces signed binaries/installers for macOS, Windows, and Linux from the single crate. |

### Rust PDF & font crate stack

No single crate does forensic repair, so PDFPundit composes a stack. Preference
order: **pure-Rust first** (keeps the single clean binary), native FFI only as a
fallback. Crate versions/health verified on crates.io (Jul 2026).

| Pipeline role | Primary (pure-Rust) | Alternatives / fallback | Notes |
| --- | --- | --- | --- |
| Parse well-formed PDFs | **`lopdf`** (0.44) | `pdf` (pdf-rs, 0.10) | lopdf for mutate/emit; pdf-rs has a stricter typed model useful for validation. |
| Byte-level carving (step 2) | custom scanner + **`memchr`** | — | Fast keyword search for `obj`/`endobj`/`stream`; this is our own code, no crate does it. |
| Inflate/deflate streams (step 4) | **`flate2`** (1.1) | **`miniz_oxide`** (0.9) | flate2 for normal streams; drop to miniz_oxide's low-level API for **byte-by-byte salvage on C9** (zlib tampering). |
| Emit repaired / template PDF (steps 1, 7) | **`lopdf`** (decided — sole emitter) | (`subsetter` still used for font trimming) | **Decision:** lopdf is the single emitter for both templates and repaired output — one model, minimal deps, mirrors REPDF's single-tool approach. `pdf-writer`/`krilla` explicitly *not* adopted. |
| Font tables — glyphorder/cmap/hmtx (step 6) | **`skrifa`** + **`read-fonts`** (the `fontations`/"oxidize" stack, Google Fonts) | `allsorts` (0.17), `ttf-parser` (0.25) | Reads metrics + cmap for code→Unicode inference. Replaces REPDF's Python `fonttools`. Fontations is the most actively maintained reader; `allsorts` is the reading fallback. |
| CFF / CIDFont handling | **`read-fonts`/`skrifa`** (CFF/CFF2, charset, FDSelect) | `allsorts`, `cff-parser` | **Reading** CID-keyed CFF (for mapping) is well-covered by fontations. **Writing/subsetting** CFF is the immature area — but we sidestep it (see full-embed note below). |
| Font embedding (step 1) | **full-font embed** — copy raw font-program bytes via `lopdf` | `write-fonts` (`fontations`), `subsetter` (0.2) for optional trimming | REPDF embeds the *whole* font, so no CFF recompile/subset is needed — we only read tables + copy bytes. `write-fonts`/`subsetter` are optional, only for shrinking output. |
| Complex-script shaping | **`harfrust`** (0.12, HarfBuzz org) | — | HarfBuzz port rebuilt on `read-fonts` (rustybuzz's successor — one font ecosystem with skrifa/hayro) — needed for correct **Arabic/Hindi** glyph shaping & widths (the paper's weakest languages). |
| System-font enumeration (output option) | **`fontique`** (0.11, Linebender) | hand-rolled dir scan + `read-fonts` | "Use system fonts" substitution mode: match by family/PS name via DirectWrite/CoreText/fontconfig (dlopen'd) — OS platform-API FFI, the sole FFI exception. |
| Render previews / thumbnails / font-pick preview | **`hayro`** (0.7, pure-Rust rasterizer) + `hayro-syntax`/`hayro-interpret` | — | The `hayro` family (author LaurenzV) is a full pure-Rust PDF stack; `hayro-interpret`/`hayro-syntax` can also cross-check our carve results. No native renderer needed. |
| Image extract/decode (step 5) | **`image`** (0.25, narrow features) + **`hayro-jpeg2000`** (0.4) | — | DCTDecode/JPXDecode streams are complete JPEG/JP2 files — extracted **verbatim, no decode**; only Flate rasters need decode + PNG-encode. (Standalone `jpeg-decoder`/`png` dropped — already bundled in `image`.) |
| Text extraction (benchmark eval) | **`hayro-interpret`** | rasterize (`hayro`) + OCR | To score recovered vs. original text against the corpus. (`pdf-extract` dropped — pins lopdf 0.42, would compile a duplicate lopdf tree + 4 redundant font crates.) |
| Char encoding / Unicode | `encoding_rs`, `unicode-normalization` | — | Normalize during font-inference scoring against word dictionaries. |
| **MD export — glyph extraction** | our **pure-Rust `PdfEngine`** on `hayro-interpret` | — | Feeds per-glyph items (text + bbox + font attrs) into spdf, replacing spdf's PDFium `spdf-pdf`. Keeps export FFI-free. |
| **MD export — layout/tables** | `spdf-projection` + `spdf-types` + `spdf-processing` (MIT) | (our own projection if spdf's API churns) | Engine-agnostic spatial-grid projection: columns, reading order, tables, faux-bold dedup. The hard layout logic, reused not rebuilt. |
| **MD export — Markdown emitter** | our `export/markdown.rs` (GFM) | contribute to `spdf-output` | spdf ships text/JSON only; the Markdown formatter is our value-add (headings by font size, emphasis by flags, GFM tables, image refs, links). |
| Retro cat background — fetch | **`ureq`** (3.3) + **`rustls-graviola`** provider | `minreq` | Pure-Rust sync HTTPS for thecatapi.com — no tokio/reqwest, and no C compiler (ureq's default `ring` provider compiles C/asm; graviola doesn't). Optional/opt-in, off by default. |
| Retro cat background — ASCII | **`artem`** (3.0, lib target, `default-features = false`) | hand-rolled luminance ramp | Image → colored ASCII sized to the terminal; ANSI output parsed into cells. (`rascii_art` dropped — unmaintained since 2023. artem is MPL-2.0: recorded license exception.) |
| Retro cat background — colors | **`palette`** (0.7) | hand-rolled OkLCh (~50 lines) | Oklch: raise lightness + drop saturation + nudge hue → pastels. (`pastel` git dep dropped — unversioned, drags clap/build.rs.) |

**Decision — pure-Rust only:** no C FFI. `qpdf`, `mupdf`, and `pdfium` are **not**
used; the `hayro` family (incl. `hayro-jpeg2000`) covers rendering and JPEG2000 in
pure Rust. For Markdown export we adopt `spdf`'s engine-agnostic crates
(`spdf-projection`/`-types`/`-processing`) but **not** its PDFium (`spdf-pdf`),
LibreOffice (`spdf-convert`), or Tesseract (`spdf-ocr`) backends — we supply a
pure-Rust `PdfEngine` on `hayro-interpret` instead. This keeps a single clean
binary and trivial cross-platform builds, and avoids AGPL (`mupdf`) / large-binary
(`pdfium`) concerns. Trade-off accepted: the worst-case structural-recovery ceiling
rests on our own carver rather than a battle-tested C library.

**Rule refined (2026-07-16):** no *compiled/vendored* C or assembly anywhere in
the build (hence rustls-graviola over ring, JSON over bundled SQLite); OS
platform-API FFI is permitted solely for system-font enumeration (`fontique`).
Licenses: code deps are MIT/Apache except `artem` (MPL-2.0, link-only — recorded
exception); bundled Noto fonts are SIL OFL (license texts ship with the binary).

### Component layout

Single Rust crate — the TUI, job runner, and PDF engine all live in one process.

```
pdfpundit/
├─ Cargo.toml
├─ assets/
│  ├─ templates/          # pre-built template PDFs (full-embedded reference fonts)
│  ├─ fonts/              # bundled font files (Noto family, en/fr/es/ar/hi/zh)
│  ├─ dicts/              # 6-language word dictionaries for font-inference scoring
│  ├─ fontindex.json      # font name → glyphorder/cmap/metrics/template mapping
│  └─ theme/              # retro palette / ASCII banner / bundled default cat (offline)
├─ tools/
│  └─ build-templates/    # offline generator for templates + fontindex (dev-time)
└─ src/
   ├─ main.rs             # terminal init/teardown, event loop, panic-safe restore
   ├─ app.rs              # App state, panel focus, key/mouse routing, dispatch
   ├─ input.rs            # crossterm events + terminal path-paste → actions
   ├─ theme.rs            # retro color scheme, borders, ASCII banner
   ├─ catbg.rs            # thecatapi fetch (ureq+graviola) → artem → palette bg + cache
   ├─ ui/                 # FrankenTUI (ratatui) rendering
   │  ├─ browser.rs       # file queue / history list panel
   │  ├─ report.rs        # diagnostics panel — findings grouped by severity
   │  ├─ actions.rs       # repair-task checklist + progress panel
   │  ├─ fontpick.rs      # interactive font-candidate selector (low-confidence cases)
   │  └─ picker.rs        # in-TUI .pdf-filtered filesystem browser
   ├─ library.rs          # file-history index + persistence (JSON, atomic writes)
   ├─ jobs.rs             # job runner — std threads + mpsc (analyze / repair) + progress events
   └─ pdf/
      ├─ meta.rs          # lopdf metadata (version, pages, title, dimensions)
      ├─ carver.rs        # step 2: raw-byte object/stream scan (obj/endobj/stream)
      ├─ streams.rs       # step 4: inflate + classify streams (image vs content)
      ├─ pages.rs         # step 3/5: page /Contents,/MediaBox + resource analysis
      ├─ rebuild.rs       # xref/trailer rebuild + object-graph reconstruction
      ├─ fontdb.rs        # steps 1/6: template lookup, glyph→Unicode inference, scoring
      ├─ emit.rs          # step 7: assemble recovered objects into template → output
      ├─ diagnose.rs      # C1–C10 detectors → Findings
      ├─ repair.rs        # C1–C10 repair passes → writes <name>.repaired.pdf
      └─ export/          # high-quality PDF → Markdown (fast-follow feature)
         ├─ engine.rs     # pure-Rust PdfEngine (hayro-interpret) → spdf glyph items
         ├─ layout.rs     # drive spdf-projection: columns, reading order, tables
         └─ markdown.rs   # structured layout → GFM (headings, lists, tables, images)
```

**Repair passes** map 1:1 to the C1–C10 taxonomy above. Each is a pluggable unit
exposing `diagnose() -> Vec<Finding>` and `repair(&mut Doc)`, and most are built on
the shared engine primitives (`carver`, `rebuild`, `fontdb`):

- `c1_header`, `c2_xref`, `c3_trailer`, `c4_page_tree`, `c5_object_tag` — structural recovery.
- `c6_font_map`, `c7_font_stream`, `c8_font_resources` — font/Unicode restoration via `fontdb`.
- `c9_zlib`, `c10_truncation` — stream salvage and partial-file reconstruction.

### Key data flow

1. User adds a PDF — drops it onto the terminal (path pasted) or picks it via the
   in-TUI `browse…` picker — `input.rs` captures the path and queues it.
2. The job runner runs the selected **diagnostic** passes off-thread; each pass
   emits `Finding`s (severity, location, description, whether auto-repairable).
3. The report panel streams findings in as they arrive, grouped by severity.
4. User ticks the repair tasks to apply; the runner executes their `repair()`
   passes and writes a **new `<name>.repaired.pdf`** — the original is never mutated.
5. Run metadata (findings + repair log + output path) is recorded to history.

## UI Layout (v1)

**Cat-first layout** (revised 2026-07-16; full spec in the
[technical design](pdfpundit-technical-design.md) §7). The window starts clean —
the pastel ASCII cat on full display — and panels exist only once files do.
Tab/mouse move focus; the footer shows context key hints.

- **Empty state:** no panels — just the cat, an ASCII wordmark, and one dim
  hint: "Drop a PDF on this terminal, or press `b` to browse."
- **Queue panel (floating, top-left):** appears when files are added (drop or
  in-TUI browser); it *is* the batch queue and grows downward as files are
  added. Each row: a status icon (not started / in progress / complete /
  error) + filename + terse result note.
- **Analysis panel (beneath the queue, hideable):** the selected file's
  metadata, C1–C10 findings grouped by severity (expandable to per-object
  evidence), font resolutions, and repair outcomes.
- **Bottom progress bar (while processing):** the current file's gauge +
  filename; when more than one file is queued, a second line shows total batch
  percentage.
- **Context menus (modals):** every decision — per-file actions
  (analyze / repair / export), the repair-pass checklist, low-confidence font
  picks (REPDF's suggested "interactive step," native to our TUI), and
  unresolved font-substitution choices with individual selection plus a
  select-all option.

### Retro aesthetic

The "cute retro" look is a first-class design constraint, driven by `theme.rs`:

- Double-line box borders, a blocky ASCII/ANSI wordmark banner on launch.
- A constrained retro palette (e.g. amber-on-black or teal/magenta DOS-era), with
  a couple of selectable themes ("Amber", "Phosphor Green", "DOS16").
- Chunky progress bars (`█▓▒░`), blinking cursor accents, and a status "ticker".
- Optional CRT-flavored touches (scanline dividers) kept subtle so results stay
  legible — **simple and reliable readouts first, decoration second.**

#### Background: pastel ASCII-art cats 🐱

A dim, cute ASCII-art **cat** sits behind the panels as wallpaper. Pipeline:

1. Fetch a random cat from **thecatapi.com** (`GET /v1/images/search`, `x-api-key`
   header) using **`ureq`** (pure-Rust, sync; rustls with the **`rustls-graviola`**
   provider — no tokio/reqwest, no C compiler).
2. Decode the image (`image` crate) → convert to colored ASCII with **`artem`**
   (lib target, `default-features = false`), sized to the current terminal;
   parse its ANSI output into cells.
3. Shift the palette "into cuteness" with **`palette`** (Oklch: raise lightness,
   lower saturation, nudge hue toward pastels). Rendered as a low-contrast
   background layer so panel text stays fully legible.
4. **Cache** the fetched image + rendered ASCII to the app cache dir; reuse across
   launches and only refresh occasionally (or on a "new cat" keybind).

**Network is optional and off by default** (this is a forensic tool that may run
air-gapped): cats are an opt-in cosmetic. On no-network / disabled / API failure,
fall back to a **bundled default cat** in `assets/theme/`. A setting toggles the cat
background and a global offline mode.

**API-key handling ("somewhat secure"):** the thecatapi key is **not** committed to
source. It's injected at build time (`option_env!("PDFPUNDIT_CATAPI_KEY")`, supplied
via a git-ignored `.env`/CI secret) and stored lightly obfuscated (XOR/base64) to
resist trivial `strings` scraping, with a runtime env/config override. This is
obfuscation, not real secrecy — an embedded client key can't be truly hidden, which
is fine since it only fetches cats. (The actual key lives outside the repo.)

## Milestones

### M1 — Skeleton & input (a TUI that accepts PDFs)
- [ ] Scaffold the ratatui/crossterm crate; panic-safe terminal restore.
- [ ] Event loop, panel focus (Tab), mouse selection, footer key hints.
- [ ] Add-file via terminal path-paste; reject non-`.pdf`.
- [ ] In-TUI `.pdf`-filtered filesystem picker (`browse…`).

### M2 — File history & metadata
- [ ] History index model + local persistence (JSON, atomic write-temp-then-rename).
- [ ] Extract metadata (version, page count, title, dimensions) via lopdf.
- [ ] Queue/history panel: list, status, select, remove.

### M3 — Forensic engine primitives (steps 2–5)
- [ ] Object carver: raw-byte `obj/endobj/stream` scan recovering objects + stream lengths, incl. expanding `/ObjStm` containers (mandatory for PDF ≥1.5).
- [ ] Stream inflate + classifier (image vs content by `/Subtype`/operators).
- [ ] Page extraction (`/Contents`, `/MediaBox`) + resource analysis.
- [ ] XRef/trailer rebuild + object-graph reconstruction (`/Root`→`/Pages`→`/Page`).
- [ ] Job runner (std threads + mpsc channels) streaming progress + findings to the UI.
- [ ] Vendor the REPDF corpus (or a subset) as a test fixture set.

### M4 — Diagnostics (C1–C10 detectors)
- [ ] Detectors for C1–C10 emitting `Finding`s with severity + object location.
- [ ] Diagnostics panel: findings grouped by severity, expandable detail.
- [ ] Correctly classify each corpus file by its known corruption suffix.

### M5 — Template / font database (steps 1, 6)
- [ ] `tools/build-templates`: offline generator producing template PDFs (full-embedded
      Noto fonts + `W`/`Widths`) and `fontindex.json` (glyphorder/cmap/metrics).
- [ ] Bundle 6-language word dictionaries; font-inference scorer (word bonuses, mixed-lang/rare-char penalties).
- [ ] `/BaseFont` name lookup path; glyph-code→Unicode inference path; `/ToUnicode`-as-dictionary path.
- [ ] Interactive font picker modal for low-confidence cases.

### M6 — Repair passes (C1–C10) + emitter (step 7)
- [ ] Structural repairs C1–C5 → renderable `<name>.repaired.pdf` (original never mutated).
- [ ] Font/Unicode repairs C6–C8 via the template DB, inference, and `/ToUnicode` rebuild.
- [ ] Stream/truncation repairs C9–C10 (byte-by-byte inflate to limit loss).
- [ ] Template emitter assembling recovered content/font/image objects into output.
- [ ] Actions panel checklist + run log; re-diagnose after repair to confirm resolution.

### M7 — Benchmark, retro polish & packaging
- [ ] Batch harness measuring text/image recovery across the REPDF corpus vs. the paper's numbers.
- [ ] Retro theme(s), ASCII banner, selectable palettes; about screen.
- [ ] Pastel ASCII-cat background: `catbg.rs` (ureq+rustls-graviola fetch → artem → `palette`),
      disk cache, bundled offline fallback, on/off + offline-mode settings, obfuscated key.
- [ ] `cargo-dist` binaries/installers for macOS, Windows, Linux via CI matrix.
- [ ] Docs / README with usage.

### M8 — High-quality PDF → Markdown export (fast-follow)
- [ ] Spike: confirm `hayro-interpret` exposes per-glyph text + bbox + font attrs.
- [ ] Pure-Rust `PdfEngine` adapter feeding `spdf-types` items (replaces `spdf-pdf`/PDFium).
- [ ] Wire `spdf-projection` for columns, reading order, and best-effort GFM tables.
- [ ] `markdown.rs` emitter: headings (font-size tiers), emphasis (font flags), lists,
      links (`/Annots`), image extraction + refs, code/quote heuristics.
- [ ] `Export → <name>.md` action in the Actions panel; "repair → export" one-shot flow.
- [ ] Quality harness: compare export against reference Markdown for a sample set.

### Future — the "Pundit+" phase (out of v1 scope)
- OCR pass (`spdf-ocr`/Tesseract or pure-Rust) to rebuild text layers on scanned/image-only documents — also unlocks Markdown export of scans.
- Headless CLI / batch mode for scripted folder repair.
- Standards deep-dive: full PDF/A validation & conversion.
- Optional AI-assisted findings explanations / repair suggestions.

## Risks & Open Questions

- **Repair correctness:** a pass could "fix" a file into a worse state. Mitigated by always writing a new `<name>.repaired.pdf`, re-diagnosing after repair, and benchmarking against the REPDF corpus; never mutate the original.
- **Template/font DB is the crux:** REPDF's advantage over other tools (C6–C8) comes entirely from the pre-built font DB. Building good templates + `fontindex` in Rust (font subsetting/full-embed via `skrifa`/`allsorts` vs. Python `fonttools`) is the highest-risk, highest-value work. Fonts must cover en/fr/es/ar/hi/zh — Noto is the natural open-licensed choice; *confirm acceptable bundle size (likely tens of MB).*
- **Known-hard cases:** the paper's own weak spots carry over — Arabic font inference (~33–40% for C6/C8 due to inflection), "Print to PDF" files with mutating font names (`CIDFont+F1`), C9 zlib (~60%), and C10 "Print to PDF" truncation (~35%). The interactive font picker is our lever to beat the automatic-only scores.
- **Pure-Rust ceiling (decided):** no C FFI — carver/rebuilder is entirely ours on `lopdf`, rendering via the `hayro` family. Accepted trade-off: no battle-tested C library (`qpdf`/`mupdf`) to lean on for the nastiest structural cases, so our carver's robustness is the ceiling. Mitigate with the corpus benchmark harness.
- **Font tooling parity (reduced):** REPDF leans on `fonttools`+`pikepdf`. The `fontations`/"oxidize" stack (`read-fonts`/`skrifa`) covers CID/CFF *reading* well; the immature part (CFF *subsetting*/writing) is sidestepped because we embed the *full* font program bytes rather than subsetting. Residual risk is narrow: correctly reading CID charset/FDSelect and generating `/ToUnicode` for composite Type0 fonts. Still worth an early spike on a C7/C8 CJK/CFF corpus file. Arabic/Hindi correctness also needs `harfrust` shaping, not just glyph lookup.
- **Markdown export depends on two unknowns:** (1) `hayro-interpret` must expose per-glyph text + bbox + font attributes for our `PdfEngine` adapter — verify with a spike before committing to M8; if it doesn't, we extend hayro or fall back to our own content-stream interpreter. (2) `spdf` is early (v0.2.0-alpha) — API churn risk; mitigated because its projection core is small and MIT, so we can vendor/fork if needed.
- **Markdown emitter is ours:** spdf outputs text/JSON, not Markdown — the GFM formatter (esp. table rendering and heading inference) is net-new work and where "high quality" is won or lost.
- **Terminal drag-drop UX:** drop-on-terminal behavior varies by emulator (most paste the path). The `browse…` picker is the guaranteed fallback.
- **Retro vs legibility:** CRT/scanline effects and the cat background must never hurt readability of findings — the wallpaper is dimmed and panels stay opaque; decoration is subordinate to reliable results.
- **Cat background = network + secret:** fetching cats reaches the internet, which a forensic tool often shouldn't do unprompted — so it's opt-in, off by default, with an offline mode and bundled fallback. The embedded thecatapi key is obfuscated and kept out of source, but can't be truly secret (acceptable: cats only).

## Success Criteria

- Drop a corrupted PDF (or browse to it); PDFPundit identifies its corruption type(s) against C1–C10 within a second or two, shown in the retro UI.
- Produce a `<name>.repaired.pdf` that renders in Chrome and re-diagnoses clean for the targeted findings — the original file is untouched.
- On the REPDF corpus, match or beat the paper's results (avg text recovery ≈90.67%): ≈100% on C1–C5, ≈99% on C7, ≈90% on C6/C8, with the interactive font picker lifting the known-hard Arabic / "Print to PDF" cases above REPDF's automatic-only scores.
- Extract embedded images to standalone files during repair (REPDF ≈94.75% image recovery) and re-place them when the content stream allows.
- Export a multi-column PDF to Markdown in correct reading order with headings, lists, best-effort GFM tables, and image refs — and on a font-broken PDF, the repaired-then-exported Markdown has correct Unicode where raw extraction would be garbled.
- Runs identically on macOS, Windows, and Linux from a single self-contained binary.
- Close and reopen the app; file history and prior findings are intact.
