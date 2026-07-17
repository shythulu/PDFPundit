---
planStatus:
  planId: plan-pdfpundit-technical-design
  title: PDFPundit — Technical Design (v1)
  status: draft
  planType: system-design
  priority: medium
  owner: shythulu
  stakeholders: []
  tags:
    - rust
    - pdf
    - forensics
    - tui
    - design
  created: "2026-07-16"
  updated: "2026-07-17T06:40:00.000Z"
  progress: 0
---
# PDFPundit — Technical Design (v1)

## Context

The feature plan [pdfpundit-desktop-app.md](/Users/shylo/source/PDFPundit/nimbalyst-local/plans/pdfpundit-desktop-app.md)
defines *what* PDFPundit is: a pure-Rust, REPDF-grounded forensic PDF repair
tool with a terminal UI, targeting the C1–C10 corruption taxonomy, plus a
fast-follow PDF→Markdown export. This document is the level below — the
technical design an implementer codes from: module tree, core data model,
concurrency/event architecture, carver and rebuild algorithms, font database
and substitution policy, persistence, the engine↔UI contract, and testing.
(The UI framework, layout, and aesthetic are designed separately in
[pdfpundit-ui-design.md](pdfpundit-ui-design.md).)

Plan decisions treated as fixed constraints: pure Rust, no C FFI, `lopdf` as
sole emitter, full-font embedding (no CFF subsetting), originals never mutated,
single crate, MIT/Apache code deps.

## 1. Plan review — decisions & refinements

### Resolved with the user (supersede the plan where they conflict)

1. **Persistence**: **JSON store** via serde — it's just the recent-file list
   plus per-run findings; nothing fancy. (Bonus: `rusqlite --features bundled`
   compiles SQLite's C source, which would have violated the no-C-FFI rule the
   plan itself set. JSON dodges that entirely; the store sits behind a small
   trait if scale ever demands more.)
2. **UI is designed separately**: the terminal-UI framework, layout (the
   "cat-first" idea), theme, and copy are parked in
   [pdfpundit-ui-design.md](pdfpundit-ui-design.md). This document defines only
   the engine and the UI-agnostic event/interaction contract (§7). *(supersedes
   the plan's three-always-visible-panels UI section — now a UI-plan concern.)*
3. **Font substitution is an output option**: default = bundled open-source
   "sister" fonts (Unicode-correct Noto-style replacements from the template
   DB); switchable to **system fonts matched by name**. Analysis precedes
   output, so PDF fonts with no equivalent become per-font user choices in a
   user prompt — individual selection plus select-all. (§5.5)

### Design refinements over the plan (found during design)

4. **Job runner = `std::thread` + `std::sync::mpsc`** — no tokio, no crossbeam.
   Jobs are CPU-bound byte crunching that stream typed events to whatever UI
   consumes them; ureq is sync. The engine is **UI-agnostic** — it emits a
   `JobEvent` stream and nothing more. (§3)
   *(The TUI framework, layout, and aesthetic are decided separately in
   [pdfpundit-ui-design.md](pdfpundit-ui-design.md) — not in this document.)*
5. **Batches are first-class and sequential**: one PDF job at a time
   (deterministic "current file" progress, interaction prompts never
   interleave); the runner's queue makes parallelism a config knob later. (§3)
6. **ObjStm expansion is mandatory** — PDFs ≥1.5 pack objects into
   `/ObjStm` streams; a carver seeing only top-level `N G obj` misses most of
   such files. The plan omitted this. (§4.2)
7. **Two rebuild strategies** — always rebuilding into a template (REPDF-style)
   flattens outlines/annotations/metadata on lightly-damaged files. Add
   `Resave` (copy all carved objects, fix structure) vs `TemplateAssemble`
   (font work / heavy loss). (§4.4)
8. **Encrypted PDFs**: cheap `/Encrypt` detector → Finding "Unrepairable in v1,
   decrypt first" — avoids garbage repairs.
9. **Chinese scoring needs a character-bigram table** (no word spaces), and
   **Arabic needs shaping-derived glyph maps** (content streams reference
   positional-form gids absent from `cmap`) — without these the plan's C6/C8
   targets are unreachable for those languages. (§5.2, §5.1)
10. **Licensing nits**: Noto is SIL OFL (fine to bundle; ship license texts —
    the plan's "everything MIT/Apache" is true only of code). Word dictionaries:
    Hermit Dave's FrequencyWords (MIT). Verify `rascii_art` + `hayro-*` licenses
    when locking Cargo.toml. **v1 bundles Regular weights only** (Bold/Italic
    substituted by Regular, noted `Partial`) to contain bundle size.
11. **Memory model**: read whole file into `Vec<u8>` with a size guard (warn
    over 512 MiB); no mmap in v1.
12. **Milestone fix**: the event/channel skeleton must exist in **M1** (plan had
    the job runner in M3) or the M1 event loop gets retrofitted. (§9)
13. **M8 churn containment**: `spdf-*` and all export code behind a cargo
    feature `export` so alpha-dep churn can never break the core build.
14. **UI/aesthetic deferred**: the terminal-UI framework, the cat-first layout,
    themes, and copy are **out of scope here** — parked in
    [pdfpundit-ui-design.md](pdfpundit-ui-design.md). This document specifies
    only the engine and the UI-agnostic `JobEvent` contract it exposes.

## 2. Module tree (delta from plan marked `+`)

```
pdfpundit/                       # single crate (+ tools/ workspace member)
├─ src/
│  ├─ main.rs                    # entry: init, wire engine↔UI, run; panic-safe restore
│  ├─ jobs.rs                    # JobRunner, AppEvent/JobEvent, CancelToken, interaction rendezvous
│  ├─ config.rs                + # Config (config.toml), app-dirs resolution
│  ├─ library.rs                 # HistoryStore trait + JSON backend
│  ├─ catbg.rs                   # optional cat backdrop: fetch/convert/cache (cosmetic; see UI plan §D4)
│  ├─ ui/                        # ← framework/layout/theme designed in pdfpundit-ui-design.md
│  └─ pdf/
│     ├─ model.rs              + # shared types: CorruptionClass, Finding, ObjId…
│     ├─ lexer.rs              + # tolerant byte-level PDF object parser
│     ├─ carver.rs               # landmark scan + object-assembly state machine
│     ├─ streams.rs              # inflate (flate2), C9 salvage (miniz_oxide), classifier
│     ├─ graph.rs              + # ObjectGraph (hand-rolled)
│     ├─ pages.rs  rebuild.rs  fontdb.rs  emit.rs  diagnose.rs  repair.rs  meta.rs
│     └─ export/                 # M8, behind cargo feature "export"
├─ src/bin/corpus.rs           + # dev-only REPDF-corpus benchmark harness
├─ tools/build-templates/        # dev-time generator: templates, fontindex, gmaps
└─ tests/fixtures.rs           + # programmatic C1–C10 corruptors + golden PDFs
```

## 3. Concurrency & event architecture (`jobs.rs`)

**Decision: `std::thread` workers + `std::sync::mpsc`** — no async runtime in the
engine. Jobs are CPU-bound byte crunching; ureq is sync. The engine emits a
merged event stream that the UI consumes; the UI layer (chosen separately, see
[pdfpundit-ui-design.md](pdfpundit-ui-design.md)) owns the render loop and how it
bridges these events into its own model. `AppEvent`/`JobEvent` below are the
**engine↔UI contract**; interactive prompts use a bounded `sync_channel(1)`
rendezvous. crossbeam-channel is a drop-in if mpsc ever shows contention.

```rust
pub enum AppEvent {                          // what the UI's event source yields
    Input(/* terminal input, framework-specific */),
    Job(JobId, JobEvent),
    Tick,
}

pub enum JobKind { Analyze, Repair { passes: Vec<CorruptionClass> }, ExportMarkdown, FetchCat }

pub enum JobEvent {
    Started { kind: JobKind, file: PathBuf },
    Phase { name: &'static str, index: u32, total: u32 },   // "carving", "diagnosing C4", …
    Progress { done: u64, total: Option<u64> },
    Finding(Finding),                        // streamed as discovered
    Log(LogLevel, String),
    NeedsInteraction(InteractionRequest),    // carries the reply channel
    AnalyzeDone(Box<AnalysisResult>),
    RepairDone(Box<RepairReport>),
    Failed { error: String, panicked: bool },
    Cancelled,
}

pub struct InteractionRequest {
    pub kind: InteractionKind,               // FontPick | FontSubstitution (§5.5)
    pub reply: std::sync::mpsc::SyncSender<InteractionReply>,
}
```

**Ownership.** The UI thread owns the app state, the `Receiver<AppEvent>`, and
the `JobRunner`; it is the single consumer of the merged channel (`Sender` is
`Clone`, shared by the input source and every job thread). Job workers each own
their inputs (`PathBuf`, a cloned `JobOptions` snapshot, `Arc<FontDb>`) and
return data only by sending `AppEvent::Job(id, ev)` — no shared mutable state.

**Batch semantics (sequential).** PDF jobs (analyze/repair/export) run **one at
a time**: `JobRunner` holds a `VecDeque` of queued jobs and starts the next when
the current finishes. This makes the bottom bar's "current file" line exact and
guarantees decision prompts arrive one file at a time. `FetchCat` runs on a
separate slot (never queued behind PDF work). Parallelism stays a future config
knob (`[jobs] parallel`).

```rust
pub struct CancelToken(Arc<AtomicBool>);     // .check() -> Result<(), Cancelled> in every loop

pub struct JobRunner {
    tx: Sender<AppEvent>,                     // clone of the UI's merged channel
    current: Option<JobHandle>,              // { id, cancel, join }
    pending: VecDeque<QueuedJob>,
    next_id: u64,
}
impl JobRunner {
    pub fn submit(&mut self, kind: JobKind, path: PathBuf, opts: JobOptions) -> JobId;
    pub fn cancel(&mut self, id: JobId);     // also removes from pending
    pub fn on_job_finished(&mut self, id: JobId);  // reap, start next pending
    pub fn shutdown(self, grace: Duration);
}
```

**Panic isolation.** Job bodies run inside `catch_unwind`; a panic becomes
`JobEvent::Failed { panicked: true }` — a bad PDF can never kill the app. The
UI layer is expected to install a panic hook that restores the terminal before
printing (covers UI-thread panics); that hook is a UI-plan concern.

**Blocking interaction from a job** (used by both font prompts):

```rust
fn interact(&self, kind: InteractionKind) -> Result<InteractionReply, Cancelled> {
    let (tx, rx) = mpsc::sync_channel(1);
    self.tx.send(AppEvent::Job(self.id, JobEvent::NeedsInteraction(
        InteractionRequest { kind, reply: tx })));   // stream to the UI
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(reply) => return Ok(reply),
            Err(Timeout) => self.cancel.check()?,       // cancellable while the menu is up
            Err(Disconnected) => return Err(Cancelled), // UI dismissed without answering
        }
    }
}
```

The `Disconnected ⇒ Cancelled` arm plus token polling is the deadlock guard for
"user quits while a job waits on a prompt" — covered by an explicit test.

## 4. Core engine: data model & algorithms

> §4–§5 are the overview. **Part II (§13–§20)** expands the carver, stream
> salvage, rebuild, and font inference to implementation-ready pseudocode with
> byte-level edge cases — that is the spec the modules are coded/tested against.

### 4.1 Core types (`pdf/model.rs`)

```rust
pub type ObjId = (u32, u16);                 // == lopdf::ObjectId

pub enum CorruptionClass {                   // + code() "C1".."C10", label(), const ALL
    C1Header, C2XrefMissing, C3TrailerDamaged, C4PageTreeBroken,
    C5ObjectTagStripped, C6FontMapLost, C7FontStreamDeleted,
    C8FontResourcesDeleted, C9ZlibTampered, C10Truncated,
}

pub enum Severity { Info, Warning, Error }
pub struct ByteSpan { pub start: u64, pub end: u64 }    // half-open, original-file offsets

pub enum Location {
    File,
    Span(ByteSpan),
    Object { id: ObjId, span: Option<ByteSpan> },
    Page { index: u32, obj: Option<ObjId> },
}

pub struct Finding {
    pub id: String,                          // stable per run, e.g. "C2-001"
    pub class: CorruptionClass,
    pub severity: Severity,
    pub location: Location,
    pub summary: String,
    pub evidence: Vec<Evidence>,             // Text | HexWindow(≤64 B) | ObjectRef | Metric
    pub repair: Repairability,
}

pub enum Repairability {
    Auto,
    Interactive(InteractionKind),            // FontPick / FontSubstitution
    Partial(String),                         // best-effort; explains the loss
    Unrepairable(String),
}
```

### 4.2 Carver (`pdf/carver.rs`, `pdf/lexer.rs`)

**Phase A — landmark scan.** `memchr::memmem::find_iter` over the raw bytes for
`%PDF-`, `obj`, `endobj`, `stream`, `endstream`, `xref`, `trailer`,
`startxref`, `%%EOF` → one offset-sorted landmark list. Disambiguate at
collection: drop `obj` hits that are the tail of `endobj` (same for
`stream`/`endstream`); an `obj` hit must backtrack-parse as `N G obj` within
24 bytes or be discarded. memchr runs at GB/s — well inside the "diagnose in a
second or two" budget.

**Phase B — object assembly.** Walk landmarks in offset order; per `N G obj`:

1. Parse the body with a **tolerant recursive-descent parser** (`lexer.rs`)
   producing `lopdf::Object`; on malformed dict content, error-recover by
   scanning to the matching `>>` with a string/hexstring-aware depth counter
   (note `DictRecovered`).
2. If followed by `stream`, resolve the data extent — the crux, since
   compressed data legitimately contains keyword bytes. Resolution ladder,
   recorded as `LengthSource`:
  - `/Length` direct int → verify `endstream` lands within ±2-byte EOL slack → `Declared`.
  - `/Length` indirect ref → defer to a second pass (the length object may live later) → `DeclaredIndirect`.
  - Missing/mismatched length + FlateDecode → **inflate probe**: stream-decode
     with `miniz_oxide`; `bytes_consumed` at `Done` is the true length →
     `InflateProbe` (survives a destroyed `endstream` too).
  - Else next `endstream` landmark *followed by a plausible continuation*
     (`endobj` / next header / `xref` / EOF) → `ScannedEndstream`.
  - Extends past EOF (C10) → clamp → `TruncatedAtEof`.
3. Mark landmarks inside a confirmed stream span dead (spurious keyword bytes).
4. Missing `endobj` → object ends at next header/EOF (note `NoEndobj`).
5. Classify `ObjectKind` from `/Type`//`/Subtype`; typeless streams classify by
   content (`BT/ET/Tf/Tj` → content stream; image magic / `/Image` → image).
6. **Expand `/ObjStm` containers** (inflate, parse `First`/`N`, re-carve
   embedded objects); decode xref streams for diagnosis only.

**Gap sweep (C5/C10):** scan residual gaps between spans; `<<`-led gaps →
`Orphan::Dict`, header-less `stream…endstream` (or inflate-probe hits) →
`Orphan::Stream`; other non-whitespace runs ≥16 bytes become Finding evidence.

Carve output: `CarveReport { header, objects: Vec<CarvedObject>, orphans,
xref_spans, trailer_spans, startxref, eof_markers }`, where` CarvedObject`
tracks `declared_id`, `assigned_id`, span, body (`Dict`/`Stream`/`Primitive`/
`Unparsed`), kind, and anomaly notes.

### 4.3 Object graph (`pdf/graph.rs`) — hand-rolled

Nodes keyed by `ObjId`; edges carry the referencing key-path (e.g.
`[Resources, Font, F1]`). Queries: `catalog_candidates()`, `reachable(root)`,
`referrers(id)`, `dangling_refs()`, `pages_document_order()`. petgraph is
rejected — it would force `ObjId↔NodeIndex` bookkeeping for algorithms (SCC,
shortest path) we never use; everything here is a map lookup or BFS.

### 4.4 Rebuild (`pdf/rebuild.rs`, `pdf/emit.rs`)

**Renumbering (C5):** duplicated declared ids → last-in-byte-order wins
(mirrors incremental-update semantics); orphans get fresh ids from
`max_declared + 1`. **Dangling-ref reconciliation:** infer the expected kind
from the referencing key path (`/Contents` → content stream, `/FontFile2` →
font file, `/Kids` entry → page…), then match the nearest unclaimed orphan of
that kind in byte order (REPDF-style positional matching); each match is
recorded as Finding evidence with its distance, so the report shows its work.

**Xref/trailer by construction:** repairs populate a fresh `lopdf::Document`
(wrapped as `RebuildDoc` with an id-remap table); `save_to()` emits a correct
header, xref, and trailer — no hand-formatted xref code to get wrong. C1/C2/C3
"repairs" therefore validate inputs and report `Fixed { notes: ["resolved by
re-emission"] }`; the original xref/trailer are parsed only for diagnosis.

```rust
pub enum RebuildStrategy {
    Resave,            // only C1–C5 structural damage: copy ALL carved objects,
                       // preserving annotations/outlines/metadata; fix tree; save.
    TemplateAssemble,  // REPDF-style: seed from template PDFs, inject recovered
                       // content/images, substitute fonts (C6–C8, heavy C10).
}
// selector: TemplateAssemble iff selected passes ∩ {C6,C7,C8} ≠ ∅
//           or (C10 and <60% of referenced objects recovered); else Resave.
```

**Page tree (C4):** collect Page nodes (order: surviving `/Kids` order →
`/Parent` grouping → byte order); emit a single flat `/Pages` node with
rewritten `/Parent`s; `/MediaBox` fallback chain: own → inherited → modal
sibling value → config default. Catalog: reuse surviving with `/Pages` rewired,
else synthesize.

**Repair pass interface:** each pass implements
`RepairPass { class(), diagnose(&DiagnoseCtx) -> Vec<Finding>, repair(&mut RepairCtx, &[Finding]) -> PassOutcome }`
where the contexts carry `bytes`, `CarveReport`, `ObjectGraph`, the `RebuildDoc`
under construction, `Arc<FontDb>`, the interaction bridge, progress sink, and
cancel token. Execution order: **C9 → C10 → C5 → C4 → C6 → C7 → C8** (stream
salvage first so later passes see decodable content; C1–C3 realized by
emission). The `RepairReport` records findings before/after, per-pass outcomes,
verification results (lopdf reload + hayro smoke-render), extracted images, and
stats.

### 4.5 C9 zlib salvage (`pdf/streams.rs`)

Ladder: (1) normal `flate2` inflate; (2) on error at input offset `k`, keep the
decoded prefix (miniz_oxide streaming gives exact consumed/produced counts);
(3) **single-byte-flip brute force** — the corpus's C9 flips exactly one byte,
so for streams ≤256 KiB try all 8 bit-flips per byte in `[k − 4 KiB, k + 64]`
and accept the first candidate that inflates to completion and passes Adler-32;
(4) resync to a later stored-block boundary for a best-effort suffix fragment
(`Partial`). Result enum: `Clean | Prefix | RepairedByteFlip | Unrecoverable`.

## 5. Font database, inference & substitution policy

### 5.1 Bundled font DB (`assets/`, built by `tools/build-templates`)

`fontindex.json` (per font): id, family, PostScript name, `base_font_aliases`
(e.g. Arial/Helvetica/LiberationSans → Noto Sans), scripts/languages, flavor
(glyf vs cff), descriptor metrics, template PDF path + font object number, and
a **sidecar `.gmap` binary** — `(gid, unicode, width, source)` records sorted
by gid (JSON would balloon at CJK glyph counts). `source` distinguishes
`cmap`-direct entries from **shaping-derived** ones: at build time `harfrust`
(the HarfBuzz-org shaper built on read-fonts — one font ecosystem with skrifa
and hayro) shapes every dictionary character in isolated/initial/medial/final
context and records the GSUB-produced gids. This is what makes Arabic inference viable —
content streams reference positional-form gids that never appear in `cmap`.

Template PDFs (one per font, emitted with lopdf + read-fonts): single page,
`/Type0` + `Identity-H` → `CIDFontType2` descendant with `/CIDToGIDMap
/Identity`, full` /W `array, and a **full, non-subset**` /FontFile2`, plus a
full-coverage `/ToUnicode`. Repair harvests the font subtree via lopdf object
copy + renumber. Assets embedded via `include_bytes!`/`rust-embed` (single
binary), with `PDFPUNDIT_ASSETS`/`asset_dir` dev overrides. Dictionaries:
FrequencyWords (MIT) for en/fr/es/ar/hi + a Chinese character-bigram table.

### 5.2 Inference scoring (`pdf/fontdb.rs`)

Inputs: hex-code runs (`<…> Tj`, `[<…>…] TJ`) per unknown font slot. Per
candidate font × language: decode code → gid → Unicode via gmap, tokenize
(space glyphs / TJ offsets / `Td` breaks), then

```
score = Σ +len(w)·2.0 for dictionary words  |  +len(p)·0.5 for longest dict-prefix
        − unmapped·3.0 − off-script·1.5 − mixed-script tokens·2.0     (weights tuned on corpus)
```

Chinese uses mean bigram log-probability instead (no word spaces). If
`/ToUnicode` survives, decode the true text through it and use its words as a
custom dictionary — pins the correct font even for inflected languages.
Confidence = `0.5·hit_rate + 0.5·margin(best, second)`; auto-accept iff
`conf ≥ 0.35 && hit ≥ 0.5` (config `repair.auto_accept_confidence`), else
`Repairability::Interactive(FontPick)`.

### 5.3 Interactive font pick (low confidence)

`FontPickRequest { page, font_slot, sample codes, candidates: top 5 }`, each
candidate carrying family, language, score, confidence, and a ~200-char decoded
**preview** — the modal shows the previews as terminal text (correct Unicode is
exactly what's being judged) with retro bar meters for scores. Replies:
`Pick(font_id) | UseBest | Skip` (Skip keeps the best candidate but flags the
Finding `Partial`).

### 5.4 `/ToUnicode` rebuild

Generate a CMap covering only codes actually used: standard `CIDInit` wrapper,
consecutive runs batched into `bfrange`, singletons into `bfchar` (≤100 per
block per spec), FlateDecoded via lopdf.
`fn build_tounicode(used: &BTreeMap<u16, char>) -> lopdf::Stream`.

### 5.5 Font substitution policy (user decision — output option)

```rust
pub enum FontSourcePolicy { Bundled, SystemByName }   // config [fonts].source + per-run override

pub struct FontResolution {                // produced during ANALYSIS, consumed at output time
    pub pdf_font: String,                  // /BaseFont or slot name
    pub status: ResolutionStatus,
}
pub enum ResolutionStatus {
    Embedded,                              // intact in the PDF — nothing to do
    Bundled { font_id: String },           // sister-font match in the template DB
    System { family: String, path: PathBuf },
    Unresolved { candidates: Vec<FontCandidate> },   // → TUI prompt at output trigger
}
```

- **Bundled (default):** name lookup via `base_font_aliases`, else inference
  (§5.2). Always Unicode-correct — the template DB carries full gmaps.
- **SystemByName:** enumerate installed fonts with **`fontique`** (Linebender;
  fontations-native so no second font parser; queries by family name via
  DirectWrite/CoreText/fontconfig — fontconfig dlopen'd, so builds stay clean
  on machines without it). A matched system font is full-embedded at output:
  raw bytes copied via lopdf, descriptor built from its tables via read-fonts,
  runtime cmap-based gmap for `/ToUnicode`. Check the OS/2 `fsType` embedding
  bits first — a restricted-license font is refused with a Finding, falling
  back to bundled. Name-match only; the inference path always draws candidates
  from the bundled DB (it alone has shaped gmaps + dictionaries).
- **Unresolved fonts:** because analysis precedes output, the set is known
  before any file is written. At the output trigger the TUI presents one
  context menu listing every unresolved font — each row cycles through its
  candidates (with decoded previews), plus **"Apply best to all"** as the
  select-all bulk action. Delivered through the same interaction rendezvous
  (`InteractionKind::FontSubstitution`).

## 6. Persistence & config (`library.rs`, `config.rs`)

**JSON store** (user decision) behind a minimal trait (`upsert_file`,
`record_run`, `list_files(filter)`, `runs_for`, `delete_file`):

```
<data_dir>/history/
  index.json                  # Vec<FileSummary> — a cache; rebuilt by scanning files/ if corrupt
  files/<sha256>.json         # { meta: FileMeta, runs: Vec<RunRecord> }  (findings inline)
```

Atomic writes: serialize to `<name>.json.tmp` in the same directory → fsync →
`fs::rename` (→ fsync dir on Unix). Search is an in-memory substring filter —
plenty for a recent-file list.

App dirs via the `directories` crate:
`ProjectDirs::from("dev", "shythulu", "PDFPundit")` → `config.toml`,
`data_dir()/history/`, `cache_dir()/catbg/`. Missing config ⇒ defaults; unknown
keys ⇒ startup warning, not an error.

```toml
[general]
offline = true                    # global network kill-switch — default ON (forensic tool)
output_dir = ""                   # "" = write outputs beside the input
default_page_size = "A4"

[repair]
auto_accept_confidence = 0.35
max_font_candidates = 5
extract_unplaceable_images = true

[fonts]
source = "bundled"                # bundled | system   (per-run override in the TUI)
prompt_unresolved = true          # false ⇒ auto-pick best candidate silently

[ui]
theme = "default"                 # theme names defined by the UI plan (deferred)
mouse = true
cat_background = true             # optional cosmetic backdrop; obeys [general].offline
cat_refresh_hours = 24

[catapi]
api_key = ""                      # overrides the embedded obfuscated key
```

## 7. UI — deferred (see separate plan)

The terminal-UI framework, layout, theme, and copy are **out of scope for this
engine document** and are designed separately in
[pdfpundit-ui-design.md](pdfpundit-ui-design.md) (framework candidates —
ratatui vs. the Charm/lipgloss stack vs. others; the cat-first layout; pastel
vs. retro; "furensic" copy — all still open).

What the engine guarantees to any UI (the contract the UI plan builds on):

- **Event stream** — `AppEvent`/`JobEvent` (§3): started, phase, progress,
  streamed findings, needs-interaction, done, failed, cancelled.
- **View data** — per-file `QueueEntry { path, status (NotStarted | InProgress |
  Done | Error), meta, findings, font_resolutions, job }` and batch progress
  `BatchState { current, current_progress, completed, total }` (engine-provided
  shapes; the UI owns its own widget/model state).
- **Interaction requests** — `FontPickRequest` (§5.3) and the font-substitution
  rows (§5.5) arrive over the stream, each carrying a reply channel; the UI
  presents them however it likes and sends the choice back.
- **Input the engine needs** — added file paths (from paste/drop or a picker),
  validated `.pdf` + `%PDF-` sniff (§4), and per-run job options
  (selected passes, font policy). How those are captured is a UI concern.

## 8. Testing & benchmark harness

**Unit fixtures (`tests/fixtures.rs`):** build a tiny known-good PDF
programmatically with lopdf (2 pages, 1 Type0 font, 1 image, 1 flate content
stream), then apply corruptors mirroring REPDF's induction exactly — C1
overwrite first 12 bytes … C9 flip one mid-stream byte, C10 truncate to 70%.
Per-layer assertions: **carver** (spans, `LengthSource`, orphans, plus
adversarial cases: stream data containing literal `endobj`, wrong/indirect
`/Length`, CRLF vs LF, junk-prefixed header), **graph** (edges, dangling refs),
**diagnose** (each corruptor yields exactly its class), **repair** (output
reloads in strict lopdf, hayro smoke-renders, re-diagnoses clean), **scorer**
(golden score tables guarding weight tuning), **interaction** (quit-while-
prompted cancels cleanly).

**Corpus harness (`src/bin/corpus.rs`):** headless run over the REPDF corpus
(1,000 files). Text-recovery metric: extract text from repaired vs pristine
via `hayro-interpret` (also serving as the M8 glyph-API spike), normalize (NFC, casefold, collapse
whitespace), word-level Myers diff via the `similar` crate;
`recovery = 2·matched / (len_orig + len_repaired)`. Image recovery:
decoded-pixel hash matches / originals. Emits `corpus_results.csv` and an
aggregate table against the paper's baselines (C1–C5 ≈100%, C7 ≈99%, C6/C8
≈90%, C9 ≈60%, C10 35–99%).

**CI (GitHub Actions):** `lint` (fmt + clippy -D warnings); `test` on
{ubuntu, macos, windows}; `corpus-smoke` PR gate (~30 cached corpus files,
threshold `recovery ≥ baseline − 2%`); `corpus-full` nightly; `dist`
(cargo-dist) on tags.

## 9. Build order (dependency-ordered, mapped to plan milestones)

| Step | Work | Milestone |
| --- | --- | --- |
| 1 | Crate scaffold; `pdf/model.rs` core types; `jobs.rs` event/channel skeleton (`AppEvent`/`JobEvent`/`CancelToken`); `main.rs` wiring + panic-safe restore; minimal shell that lists added files (**UI framework/layout per** [pdfpundit-ui-design.md](pdfpundit-ui-design.md)); add-file via paste/picker | M1 |
| 2 | `config.rs` (dirs + toml); `library.rs` JSON store; `pdf/meta.rs`; queue/history wiring; `ui/analysis.rs` shell | M2 |
| 3 | `pdf/lexer.rs` → `pdf/carver.rs` (landmarks, assembly, ObjStm, gap sweep) → `pdf/streams.rs` (inflate, classify, C9 salvage) → `pdf/graph.rs` → `pdf/rebuild.rs`; `tests/fixtures.rs` corruptors; JobRunner completed (sequential batch, cancel, panic isolation); `ui/progress.rs`; vendor corpus subset | M3 |
| 4 | `pdf/diagnose.rs` C1–C10 detectors + `/Encrypt` detector; analysis panel findings tree; corpus classification test | M4 |
| 5 | **Parallel track from step 2:** `tools/build-templates` (read-fonts extraction, harfrust shaped gmaps, lopdf template emit, fontindex); runtime `pdf/fontdb.rs` loader + scorer; system-font enumeration (fontique) + `FontResolution`; `ui/fontpick.rs` + substitution menu + rendezvous | M5 |
| 6 | `pdf/emit.rs` (RebuildDoc, strategy selector, template harvest, verification); `pdf/repair.rs` passes in order C9→C10→C5→C4→C6→C7→C8; `/ToUnicode` rebuild; image extraction; context-menu actions + pass checklist; re-diagnose loop | M6 |
| 7 | `src/bin/corpus.rs` + scoring; scorer weight tuning; cat fetch pipeline (ureq+graviola → artem → palette) + themes + banner; third-party-viewer spot-check of a corpus sample; cargo-dist CI; docs | M7 |
| 8 | **Spike `hayro-interpret` glyph API first**; then `pdf/export/*` behind `feature = "export"`; spdf wiring; Markdown emitter; export action + quality harness | M8 |

Critical path: 1 → 3 → 4 → 6. The font DB (step 5) is the long pole for M6's
font passes and starts as soon as step 2 finishes — `tools/build-templates`
needs only lopdf + read-fonts, not the carver.

## 10. Verification

- **Per-layer:** `cargo test` — fixture corruptors round-trip through
  carve → diagnose → repair; each Cn fixture must re-diagnose clean after its
  pass; adversarial carver cases pass.
- **End-to-end (manual):** `cargo run`; confirm the cat-first empty state;
  drop/browse a corrupted sample (single + multi-file batch); watch queue
  icons, analysis panel findings, and both progress-bar lines; trigger a repair
  needing a font decision and exercise the context menu incl. select-all; open
  `<name>.repaired.pdf` in a viewer; relaunch and confirm history persists.
- **Corpus:** `cargo run --bin corpus -- <corpus-dir>` — compare the aggregate
  table against the paper's baselines.
- **Cross-platform:** CI matrix (ubuntu/macos/windows) runs the full test
  suite; cargo-dist artifacts smoke-launched per OS.

## 11. Post-approval follow-ups

- [x] Renamed this file to `pdfpundit-technical-design.md` (2026-07-16).
- [x] UI/aesthetic decisions spun out to
  [pdfpundit-ui-design.md](pdfpundit-ui-design.md) (2026-07-17) — framework,
  cat-first layout, pastel/retro theme, and "furensic" copy are parked there as
  open decisions; this document is now engine-only. The TUI mockup is a UI-plan
  task, built once its aesthetic is locked.
- [x] Feature plan patched (2026-07-16): JSON persistence, threads-not-async
  wording, ObjStm carving note, refined FFI/license rules, and the §12 crate
  decisions (harfrust, fontique, hayro-interpret only, ureq+graviola, artem,
  palette, image trim).

## 12. Backend crate-stack structural review

Research pass over the plan's crate table (crates.io / upstream manifests,
verified 2026-07-16) to find duplication and settle the big "which crate,
where, and why" questions.

### 12.1 Dependency-overlap map

| Capability | In the plan | What research showed |
| --- | --- | --- |
| PDF parsing | lopdf + our carver + hayro + pdf-extract | `pdf-extract` rides **lopdf 0.42** (pinned) — with our lopdf 0.44 that compiles **two lopdf copies**, plus 4 small font/cmap crates (`adobe-cmap-parser`, `postscript`, `type1-encoding-parser`, `cff-parser`) duplicating hayro's machinery. |
| Font parsing | read-fonts/skrifa **and** rustybuzz (ttf-parser inside) **and** maybe fontdb (ttf-parser) | `hayro-interpret` already depends on **skrifa** — fontations is in the binary regardless; every ttf-parser crate is a guaranteed *second* font parser. **harfrust** (HarfBuzz org, v0.12.0, July 2026, 2.4M recent dl) is rustybuzz rebuilt on read-fonts *specifically* to kill this duplication; rustybuzz last shipped Nov 2024. ⚠ hayro pins **skrifa 0.42** (current 0.44) — match hayro's pin or two skrifas compile. |
| Inflate | flate2 + miniz_oxide | Non-issue: flate2's default rust backend **is** miniz_oxide — one implementation; hayro uses flate2 too. Keep flate2 (high-level) + miniz_oxide streaming API (C9 salvage); pin flate2 `default-features = false, features = ["rust_backend"]`. |
| Image codecs | image + jpeg-decoder + png + hayro-jpeg2000 | `jpeg-decoder` and `png` standalone are **redundant** — image 0.25 bundles zune-jpeg and wraps the png crate. Also: DCTDecode/JPXDecode streams are complete JPEG/JP2 files — extraction dumps them **verbatim, no decoder needed**; only Flate raster data needs decode + PNG-encode. `image` (narrow features: jpeg, png, gif) is otherwise only needed to decode the downloaded cat photo. |
| HTTPS (cat fetch) | ureq 3 + rustls | ⚠ ureq's default `rustls` feature uses the **ring** provider — ring compiles C/assembly, silently breaking the no-C-FFI rule. Escape hatches: `rustls-no-provider` + **rustls-graviola** (v0.4.0, June 2026, by the rustls maintainer; pure-cargo build, no C compiler; x86_64/aarch64 only — fine for our targets), or no runtime fetch at all. |
| ASCII art | rascii_art 0.4 | **Effectively dead** — last release Aug 2023, ~1.7k recent downloads. Replaced by **`artem` 3.0.0** (user decision): has a real lib target, shares our `image` 0.25; caveats — **MPL-2.0** (recorded exception to the MIT/Apache rule; link-only use is fine), last release Mar 2024, drags clap/env_logger/build.rs, and outputs an ANSI string we parse into cells. Build with `default-features = false` (drops its ureq). |
| Pastel color math | sharkdp/pastel as git dep | Unversioned git dep that drags clap/build.rs. **palette 0.7.6** (stable, 3.7M recent dl) has Oklch — lighten/desaturate is a few lines; or hand-roll (~50 lines). |
| Bonus finds | — | hayro workspace also ships **hayro-write** (PDF emission — future alternative to lopdf, not adopted now), embedded standard-14 substitute fonts (~240KB) and predefined CMaps (~250KB) via `hayro-interpret` default features, `hayro-jbig2`/`hayro-ccitt` decoders, and `hayro-postscript`. memchr shared with hayro-syntax. |

### 12.2 Weighted options on the four open decisions

**A. Shaping / font-parsing ecosystem** (template build-time Arabic/Indic gid maps, §5.1)
- **A1 — harfrust (fontations-everywhere)** ✅ recommended: one font parser in the whole binary (shared with hayro); HarfBuzz-org project matching HarfBuzz 13.0.0, actively released; caveat: errors on malformed fonts (irrelevant — we shape well-formed Notos at asset-build time) and younger than rustybuzz.
- **A2 — rustybuzz (plan as written)**: most battle-tested shaper; but stale since Nov 2024, effectively superseded upstream, and adds ttf-parser as a second parser.

**B. System-font enumeration** (SystemByName policy, §5.5)
- **B1 — fontique** (0.11, Linebender, active): fontations-native (read-fonts, no second parser); but reaches fonts via **OS FFI** — DirectWrite/CoreText/fontconfig (fontconfig optionally dlopen'd, so builds stay clean); best fidelity to what apps actually see.
- **B2 — fontdb** (0.23, stable, 9M dl): zero FFI, pure-Rust dir scanning + fontconfig-config parsing; but ttf-parser inside = second font parser (~small), last release Oct 2024.
- **B3 — hand-rolled dir scan + read-fonts name tables**: zero new deps, fully pure; ~150 lines; misses exotic fontconfig setups and registry-only Windows fonts.

**C. Benchmark text extraction** (corpus harness §8; also feeds the M8 engine spike)
- **C1 — hayro-interpret only** ✅ recommended: drops pdf-extract's duplicate lopdf 0.42 + 4 crates; skrifa-based and active; doubles as the M8 `PdfEngine` spike (corpus harness becomes the spike). Risk: same interpreter for repair verification and scoring could share blind spots.
- **C2 — keep pdf-extract as independent scorer**: a second opinion decorrelates measurement from our stack; costs the duplication (could be contained behind a `corpus` cargo feature).
- **C3 — both, feature-gated**: hayro for extraction+spike, pdf-extract cross-check only inside the corpus bin.

**D. Cat-fetch network purity** (`catbg.rs`, §7)
- **D1 — bundled cat pack only**: zero network code in a forensic tool; several cats shipped in assets, "new cat" rotates the pack; simplest and purest; loses live fetch.
- **D2 — ureq + rustls-graviola**: keeps live thecatapi fetch with a pure-cargo, no-C build; graviola is young (0.4) but authored by the rustls maintainer; x86_64/aarch64 only.
- **D3 — ureq default (ring)**: most battle-tested; compiles C/asm — breaks the plan's stated constraint for a cosmetic feature.

**Settled without a question** (low stakes / one obvious answer): drop
rascii_art (dead) — ASCII conversion via `artem` per user decision (see 12.1);
use `palette` for Oklch pastelization (drop the pastel git dep); keep lopdf
strict-parse of inputs for diagnosis (free — lopdf
is compiled anyway; carve remains authoritative); repo = cargo **workspace**
(`pdfpundit` app package with lib + `corpus` bin, `tools/build-templates` as
member — the plan already implied this); pin skrifa to hayro's version.

### 12.3 Decisions

All four resolved with the user (2026-07-16):

- **A. Shaper: harfrust.** One font ecosystem (fontations) across the whole
  binary; rustybuzz dropped everywhere.
- **B. System fonts: fontique.** Fontations-native enumeration via OS APIs
  (fontconfig dlopen'd on Linux). This refines the purity rule to: **no
  compiled/vendored C or assembly in the build**; OS platform-API FFI is
  acceptable, for system-font enumeration only.
- **C. Benchmark extraction: hayro-interpret only.** pdf-extract dropped; the
  corpus harness doubles as the M8 glyph-API spike. Shared-blind-spot risk
  mitigated by spot-checking a corpus sample in a third-party viewer during M7.
- **D. Cat fetch: ureq + rustls-graviola.** `ureq` built with
  `rustls-no-provider`, graviola configured as the Agent's CryptoProvider —
  live thecatapi fetch with no C compiler anywhere in the build.

Plus: **ASCII conversion via `artem`** (user decision; rascii_art dropped —
note artem is MPL-2.0, a recorded exception to the MIT/Apache code-deps rule,
alongside the OFL fonts), `palette` for Oklch pastelization (pastel git dep
dropped), lopdf strict-parse of inputs kept for diagnosis, cargo workspace
(`pdfpundit` lib + `corpus` bin, `tools/build-templates` member), skrifa
pinned to hayro's version.

**Frontend crates are out of this ledger** — the TUI framework/styling stack is
selected in [pdfpundit-ui-design.md](pdfpundit-ui-design.md) (D1). This backend
ledger stands regardless of that choice; the engine is UI-agnostic.

---

# Part II — Engine deep-dive (implementation-ready)

This part expands §4–§5 from sketches to buildable detail: exact grammars, state
machines, byte-level edge cases, and function signatures. It is the spec the
carver/rebuild/fontdb modules are coded and unit-tested against. PDF facts follow
ISO 32000-1/2; where the spec and real-world files disagree, we follow
**observed bytes**, not the spec's ideal.

## 13. Lexer — tolerant PDF tokenizer (`pdf/lexer.rs`)

The carver cannot use lopdf's parser for damaged input (it bails on the first
structural error). We hand-roll a byte-slice tokenizer that never allocates for
tokens and always makes forward progress.

### 13.1 Character classes (PDF §7.2)

```rust
#[inline] fn is_ws(b: u8) -> bool   { matches!(b, b'\0'|b'\t'|b'\n'|b'\x0c'|b'\r'|b' ') }
#[inline] fn is_delim(b: u8) -> bool{ matches!(b, b'('|b')'|b'<'|b'>'|b'['|b']'|b'{'|b'}'|b'/'|b'%') }
#[inline] fn is_reg(b: u8) -> bool  { !is_ws(b) && !is_delim(b) }   // "regular" char
```

EOL is `\r`, `\n`, or `\r\n`. Comments run `%` → next EOL (skipped as whitespace,
except the `%PDF-`/`%%EOF` landmarks the carver treats specially).

### 13.2 Token grammar

```rust
pub enum Tok<'a> {
    Int(i64), Real(f64),
    Name(Cow<'a,[u8]>),          // '/' consumed; #xx unescaped (lazily → Cow)
    LitStr(Vec<u8>),             // ( ... ) escapes + balanced parens resolved
    HexStr(Vec<u8>),             // < ... > ; odd final nibble padded with 0
    ArrOpen, ArrClose,           // [ ]
    DictOpen, DictClose,         // << >>
    Kw(&'a [u8]),                // bareword: obj endobj stream endstream R true false null xref trailer startxref
    Eof,
}

pub struct Lexer<'a> { buf: &'a [u8], pub pos: usize }
impl<'a> Lexer<'a> {
    pub fn new(buf: &'a [u8], pos: usize) -> Self;
    pub fn next(&mut self) -> Tok<'a>;         // skips leading ws/comments; never panics
    pub fn peek(&mut self) -> Tok<'a>;         // next() without advancing (save/restore pos)
    fn read_number(&mut self) -> Tok<'a>;      // handles +/-/. ; bare '.' → Real(0); "--" → recover
    fn read_lit_string(&mut self) -> Tok<'a>;  // depth-counted (); \n \r \t \b \f \( \) \\ \ddd ; \<EOL> line-continue
    fn read_hex_string(&mut self) -> Tok<'a>;  // ignores interior ws; non-hex byte → stop + note
    fn read_name(&mut self) -> Tok<'a>;        // reads is_reg run; resolves #xx
}
```

**Robustness rules (every one has a fixture):** unterminated `(` → consume to
EOF-or-next-`endobj`, return what we have + `LexNote::UnterminatedString`;
unterminated `<` that is not `<<` → treat as hex string to next `>` or delimiter;
a number with a second sign/point ends at the anomaly; an unknown byte where a
token is expected is skipped (never an infinite loop — `pos` strictly advances).

### 13.3 Object-value parser

```rust
pub struct ParsedObj { pub value: lopdf::Object, pub end: usize, pub notes: Vec<LexNote> }

/// Parse one object *value* starting at `pos` (after "N G obj" or inside a container).
/// Recursively builds arrays/dicts; resolves `N G R` when three tokens read as int int 'R'.
pub fn parse_value(buf: &[u8], pos: usize, depth: u8) -> Result<ParsedObj, LexErr>;
```

- **Depth guard:** `depth > 100` → `LexErr::TooDeep` (malicious nesting bomb).
- **Dict recovery:** parse `<< key value key value … >>`; a value that fails to
  parse is skipped to the next `/Name`-or-`>>` and the key dropped with
  `DictRecovered`. Track `<<`/`>>` balance with a counter that ignores `<`/`>`
  inside strings, so a stray `>>` in a `(literal)` value never closes the dict.
- **Ref detection:** on `Int a`, save pos; if next two tokens are `Int b` then
  `Kw("R")`, emit `Reference((a as u32, b as u16))`; else restore and treat `a`
  as a number.

## 14. Carver state machine (`pdf/carver.rs`)

Top-level entry:

```rust
pub fn carve(buf: &[u8]) -> CarveReport;   // never fails; degrades to more `Unparsed`/`Orphan`s
```

### 14.1 Phase A — landmark scan

```rust
enum Landmark { Pdf, ObjHdr{num:u32, gen:u16, hdr_start:usize}, EndObj,
                Stream, EndStream, Xref, Trailer, StartXref, Eof }
```

For each keyword run one `memchr::memmem::Finder`, collect `(offset, kind)`,
merge-sort by offset. Disambiguation **at collection time**:

1. Drop `obj` whose preceding 3 bytes are `end` (it's `endobj`'s tail); same for
   `stream`/`endstream`.
2. Promote an `obj` hit to `ObjHdr` only if backtracking over ≤24 bytes matches
   `\d+\s+\d+\s+obj` (regex done by hand: skip ws left, read gen digits, skip ws,
   read num digits). Record `hdr_start` at the object number's first digit. A hit
   that fails this (e.g. `globj` in a stream) is discarded.

Complexity: 8 linear scans; on a 100 MB file, memchr keeps this well under the
"second or two" diagnose budget.

### 14.2 Phase B — assembly loop

```rust
let mut cur = 0usize;                       // byte cursor
let mut dead: RangeSet = empty();           // landmark offsets inside stream bodies
for lm in landmarks.filter(kind == ObjHdr) {
    if dead.contains(lm.hdr_start) { continue; }        // spurious hit inside a stream
    let (num, gen) = (lm.num, lm.gen);
    let body_start = end_of("obj", lm);                 // just past the 'obj' keyword
    let ParsedObj{ value, end: after_val, notes } = parse_value(buf, body_start, 0)?;

    // Is it a stream?  dict immediately followed by the `stream` keyword.
    if value.is_dict() && next_kw(buf, after_val) == Some("stream") {
        let dict = value.as_dict();
        let data_start = after_stream_eol(buf, kw_end);     // skip the single CRLF|LF (14.3)
        let (data_end, src) = resolve_stream_extent(buf, dict, data_start, &landmarks);
        mark_dead(&mut dead, data_start..data_end);         // keywords inside are inert
        let end = consume_endstream_endobj(buf, data_end);  // tolerate missing either
        push_stream_object(num,gen, dict, data_start..data_end, src, end, notes);
        cur = end;
    } else {
        let end = consume_endobj(buf, after_val);           // to `endobj` or next ObjHdr/EOF
        push_object(num,gen, value, end, notes);            // Dict | Primitive | Unparsed
        cur = end;
    }
}
resolve_deferred_lengths();     // second pass: /Length was an indirect ref (14.3c)
expand_objstms();               // 14.4
gap_sweep();                    // 14.5
detect_xref_and_trailer();      // classic xref/trailer/startxref spans for diagnosis only
```

Key invariant: **the loop visits ObjHdr landmarks in byte order and skips any
that fall inside an already-claimed stream span**, so binary payloads that happen
to contain `1 0 obj` can never spawn phantom objects.

### 14.3 Stream-extent resolution ladder — `resolve_stream_extent`

The crux of the whole carver. Returns `(data_end, LengthSource)`. Try in order;
first success wins:

| # | Condition | Method | Source |
| --- | --- | --- | --- |
| a | `/Length` is a direct positive `Int L` | candidate `e = data_start + L`; **verify** bytes at `e..e+~2` are `EOL? endstream` within a 2-byte slack | `Declared` |
| b | (a) failed but `L` present | still record mismatch; fall through | note `LengthMismatch` |
| c | `/Length` is `Reference(id)` | **defer**: provisional end via (d)/(e); after the main loop, once `id`'s value is known, re-resolve and re-verify | `DeclaredIndirect(id)` |
| d | filter chain starts with `FlateDecode` (or none + looks like zlib `0x78`) | **inflate-probe**: stream-inflate from `data_start`; the input offset consumed at `StreamResult::Done` is the exact length | `InflateProbe` |
| e | otherwise | scan forward to the next `EndStream` landmark whose following non-ws keyword ∈ {`endobj`, an `ObjHdr`, `xref`, `trailer`} or EOF | `ScannedEndstream` |
| f | candidate end > `buf.len()` (truncation, C10) | clamp to `buf.len()`; mark object | `TruncatedAtEof` |

`after_stream_eol`: per spec the `stream` keyword is followed by CRLF or LF (not
bare CR). We accept CRLF, LF, **and** tolerate a stray single space before the
EOL (seen in the wild); data begins after that.

Inflate-probe detail (also the C9 hook): use `miniz_oxide::inflate::stream::inflate`
with a reusable `InflateState`; feed the whole tail, 64 KiB output window at a
time, until `MZStatus::StreamEnd` (record `total_in` = length) or `Err` (record
the consumed-so-far as the C9 truncation point — §16).

### 14.4 Container expansion — `expand_objstms`

For each carved object with `/Type /ObjStm`:

```
data = salvage_inflate(stream)          // §16; may be Clean or Prefix
N    = dict[/N] as usize
first= dict[/First] as usize
header = parse N pairs (objnum, rel_offset) from data[0..first]   // ints, ws-separated
for k in 0..N:
    start = first + rel_offset[k]
    end   = if k+1<N { first + rel_offset[k+1] } else { data.len() }
    obj   = parse_value(&data, start, 0)      // compressed objs are never streams
    push carved object (objnum, gen=0) with note FromObjStm(container_id), origin=Compressed
```

Malformed offsets (non-monotonic, out of range) → skip that entry, note
`ObjStmEntryBad`, keep the rest. **Xref streams** (`/Type /XRef`) are decoded the
same way but only to cross-check declared offsets in diagnosis (C2/C3) — their
entries are never treated as authoritative.

### 14.5 Gap sweep (C5 orphans / C10 tails)

Compute `covered = union(object spans ∪ xref spans ∪ trailer spans)`. For each
maximal uncovered run `g` with ≥16 non-ws bytes:

```
if g starts (after ws) with '<<'                      → parse_value → Orphan::Dict{kind by /Type}
else if g contains 'stream' … ('endstream' | inflate-Done) with no ObjHdr before it
                                                       → Orphan::Stream (classify by content §15)
else                                                  → record UnexplainedSpan (Finding evidence only)
```

Orphans carry no id yet; `rebuild` assigns ids and tries to place them (§17.2).

## 15. Stream classification (`pdf/streams.rs`)

```rust
pub enum StreamClass { Content, Image{codec:ImgCodec}, Form, FontFile, CMap, Metadata, ObjStm, Other }
pub fn classify(dict:&Dictionary, inflated:&[u8]) -> StreamClass;
```

Priority: (1) explicit `/Subtype`/`/Type` if present and sane; else (2) **content
sniff** of the (inflated) bytes:

- JPEG SOI `FF D8 FF`, PNG `89 50 4E 47`, JP2 `00 00 00 0C 6A 50` → `Image`.
- Contains text operators `BT`…`ET` with `Tj`/`TJ`/`Tf` → `Content`.
- Only `Do` XObject invocations, no text → `Form`.
- Starts with `%!PS` or has `begincmap`/`endcmap` → `CMap`.
- Font table magic `00 01 00 00` / `OTTO` / `true`/`ttcf` → `FontFile`.
- `<?xpacket`/`<x:xmpmeta` → `Metadata`.

Codec for image extraction comes from the filter, not the sniff: `DCTDecode`→JPEG
(dump verbatim), `JPXDecode`→JP2 (dump verbatim), `CCITTFax`/`JBIG2` (dump +
note), `FlateDecode` raster → decode + PNG-encode via `image`.

## 16. Stream salvage — C9 & partial inflate (`pdf/streams.rs`)

```rust
pub enum Salvage { Clean(Vec<u8>), Prefix{data:Vec<u8>, in_used:usize, in_total:usize},
                   ByteFlip{data:Vec<u8>, at:usize}, Unrecoverable }
pub fn salvage_inflate(raw:&[u8], filters:&[Filter]) -> Salvage;
```

Ladder:

1. `flate2::read::ZlibDecoder` (or `Deflate` if header absent) → `Ok` ⇒ `Clean`.
2. On error at input offset `k`: keep the decoded prefix (miniz streaming gives
   exact `in`/`out` counts) ⇒ candidate `Prefix`.
3. **Single-byte-flip brute force** (corpus C9 flips exactly one byte), gated to
   streams ≤ 256 KiB: for each byte `i` in `[k−4096 .. min(k+64, len)]`, for each
   of 8 bit flips, re-inflate; accept the first that reaches `StreamEnd` **and**
   (has a valid trailing Adler-32, or classifies as valid content) ⇒ `ByteFlip{at:i}`.
   Budget-capped at ~256 K attempts; abort → step 4.
4. Resync: scan past `k` for a plausible deflate **stored-block** header
   (`00`/`01` final-block bit + `LEN`/`~LEN` complement match) and recover the
   suffix fragment ⇒ `Prefix` (best-effort, `Partial`). Else `Unrecoverable`.

Multi-filter chains apply remaining filters (e.g. `/ASCII85Decode` before
`/FlateDecode`) left-to-right before/after inflate as declared.

## 17. Object graph & rebuild (`pdf/graph.rs`, `pdf/rebuild.rs`)

### 17.1 Graph build

```rust
impl ObjectGraph {
  pub fn from_carve(r:&CarveReport) -> Self {   // O(total refs)
    // node per assigned_id; walk each object value, emitting a RefEdge for every
    // Reference found, tagged with the key-path stack (["Resources","Font","F1"]).
  }
}
```

Catalog candidates = nodes with `/Type /Catalog`, else nodes carrying a `/Pages`
ref. `pages_in_doc_order` = BFS from the chosen `/Pages` following `/Kids`,
falling back to byte order for unreferenced `/Type /Page` nodes.

### 17.2 Renumber + dangling-ref reconciliation

```rust
pub fn plan_ids(carve:&CarveReport, graph:&ObjectGraph) -> IdRemap;
```

1. **Duplicate declared ids** (incremental updates / corruption): group by
   `(num,gen)`; the **last in byte order wins** the id; earlier copies become
   shadows (kept, unreferenced unless step 3 claims them).
2. **Orphans** get fresh ids from `max_declared_num + 1`, assigned in byte order.
3. **Dangling refs:** for each edge → missing id, infer expected `ObjectKind`
   from the key-path tail:

   | key path tail | expected kind |
   |---|---|
   | `/Contents` | ContentStream |
   | `/FontFile`,`/FontFile2`,`/FontFile3` | FontFile |
   | `/ToUnicode` | ToUnicode / CMap |
   | `/Kids[*]` | Page |
   | `/Font/*` | Font |
   | `/XObject/*` (+`/Subtype`) | Image or Form |

   Match to the **nearest unclaimed orphan of that kind in byte order**; on match,
   rewrite the ref to the orphan's new id and record `Finding` evidence
   (`matched by position, Δ=<bytes>`). Unmatched dangling refs → `Finding`
   (severity Warning; the object may be genuinely gone).

### 17.3 Page-tree reconstruction (C4)

```
pages   = graph.pages_in_doc_order()
for p in pages: set /Parent → PAGES_ID
mediabox(p) = p./MediaBox
            ?? nearest ancestor /Pages./MediaBox
            ?? modal MediaBox across sibling pages
            ?? config.default_page_size
build PAGES = << /Type/Pages /Kids [pages...] /Count pages.len() >>   // single flat node
catalog = reuse surviving /Catalog (rewire /Pages→PAGES_ID)
        ?? << /Type/Catalog /Pages PAGES_ID >>
```

Inheritable attributes actually resolved and pinned per-page before flattening:
`/Resources`, `/MediaBox`, `/CropBox`, `/Rotate` (so discarding intermediate
`/Pages` nodes loses nothing).

### 17.4 Strategy selection & emit (`pdf/emit.rs`)

```rust
fn choose_strategy(sel:&[CorruptionClass], carve:&CarveReport, graph:&ObjectGraph) -> RebuildStrategy {
    let font_work = sel.iter().any(|c| matches!(c, C6|C7|C8));
    let heavy_loss = sel.contains(&C10)
        && graph.reachable_fraction(root) < 0.60;
    if font_work || heavy_loss { TemplateAssemble } else { Resave }
}
```

- **Resave:** copy every carved object into a fresh `lopdf::Document` under the
  `IdRemap`, fix header/page-tree/catalog, `set_max_id`, `save_to`. Preserves
  outlines/annots/metadata. lopdf writes a valid classic xref + trailer — C1/C2/C3
  are resolved *by construction*.
- **TemplateAssemble:** seed the doc from the template PDF(s) (§18), inject
  recovered content/image objects, substitute fonts, then the same emit tail.

Post-emit **verification** (always): reload output with strict `lopdf`
(`Document::load_mem`) — must succeed; `hayro` render page 1..=N to a null canvas
— must not error; re-run `diagnose` on the output — targeted classes must be gone.
Results populate `RepairReport.verification`.

## 18. Font DB, inference & `/ToUnicode` (`pdf/fontdb.rs`)

### 18.1 `.gmap` sidecar (built offline by `tools/build-templates`)

Little-endian, sorted by gid, one 9-byte record: `gid:u16, unicode:u32,
width:u16, source:u8 `(`source`: 0=cmap, 1=shaped). Loaded zero-copy via
`bytemuck::cast_slice` behind a bsearch on gid. Reverse index (unicode→gid) built
lazily into a `HashMap` on first inference use.

### 18.2 Inference scoring — full algorithm

```rust
pub struct Candidate { pub font_id:String, pub lang:Lang, pub score:f32, pub hit:f32,
                       pub preview:String }
pub fn infer(codes:&[CodeRun], fonts:&[&FontRecord], dicts:&Dicts) -> Vec<Candidate>;  // sorted desc
```

```
for font in candidate_fonts(filtered by code-range priors):
  for lang in font.languages:
     decoded = []
     for run in codes:                        // each run = Vec<u16> hex codes in one Tj/TJ
        for code in run:
           gid = map_code_to_gid(code, surviving_encoding)   // Identity-H ⇒ gid==code
           decoded.push(gmap(font).unicode(gid).unwrap_or('\u{FFFD}'))
     text  = normalize(decoded)               // NFC + casefold + collapse ws
     toks  = tokenize(text, run_boundaries)   // space glyphs / TJ (+kern) offsets / Td line breaks
     s = 0.0; matched = 0; total = text.chars().count()
     for w in toks:
        if dict[lang].contains(w)          { s += W_WORD*len(w);  matched += len(w) }
        else if let p = dict[lang].longest_prefix(w) { s += W_PREFIX*len(p); matched += len(p) }
     s -= P_UNMAPPED * count(decoded == FFFD)
     s -= P_RARE     * count(chars outside font.scripts)
     s -= P_MIXED    * count(tokens mixing scripts)
     record (font,lang, score=s, hit=matched/total, preview=text[..200])
// Chinese: replace the word loop with mean bigram log-prob over dicts["zh"].bigram
// /ToUnicode survives: build a per-doc dict from the true decoded text; score against it too.
```

Weights (start; tuned on corpus in M7): `W_WORD=2.0 W_PREFIX=0.5 P_UNMAPPED=3.0
P_RARE=1.5 P_MIXED=2.0`. Confidence` conf = 0.5*hit + 0.5*margin`, where
`margin = (best.score − second.score)/max(best.score, ε)`. **Auto-accept iff
`conf ≥ cfg.auto_accept_confidence (0.35) && best.hit ≥ 0.5`; else emit
`Interactive(FontPick)`** carrying the top-`cfg.max_font_candidates` with previews.

### 18.3 `/ToUnicode` rebuild

```rust
pub fn build_tounicode(used:&BTreeMap<u16,char>) -> lopdf::Stream;
```

Emit a Type0 `CIDInit` CMap: header, `1 begincodespacerange <0000><FFFF>
endcodespacerange`, then batch consecutive` (code, unicode)` pairs into
`bfrange` blocks and singletons into `bfchar` blocks, **≤100 entries per block**
(spec limit), FlateDecoded via lopdf. Only codes actually used in the document
are included (keeps the CMap small).

## 19. Diagnosis mapping (`pdf/diagnose.rs`) — detectors → `Finding`

| Class | Detector (over `CarveReport`/`ObjectGraph`, cheap) |
| --- | --- |
| C1 | first `%PDF-` absent in the leading 1 KiB, or version bytes non-numeric |
| C2 | no classic `xref` span **and** no `/Type /XRef` stream, but objects exist |
| C3 | no `trailer`/`startxref`/`%%EOF`, or `startxref` offset doesn't land on `xref`/xref-stream |
| C4 | no reachable `/Type /Pages`, or `/Count` ≠ discovered `/Page` count, or broken `/Kids` |
| C5 | ≥1 `Orphan` with dict/stream shape but no `N G obj` header |
| C6 | a `/Page` `/Resources` lacks `/Font` while its content stream uses `Tf` |
| C7 | a `/FontDescriptor` lacks `/FontFile[23]` (or it's a dangling/empty ref) |
| C8 | C7 **and** the font's `/ToUnicode` is missing |
| C9 | any stream whose `salvage_inflate` returns `Prefix`/`ByteFlip`/`Unrecoverable` |
| C10 | `file_len` < declared/`startxref` expectations, or a stream span clamped `TruncatedAtEof` |
| — (extra) | `/Encrypt` present in trailer/carve ⇒ `Unrepairable("decrypt first")` |

Each detector yields `Finding{ class, severity, location, evidence, repair }`;
`repair` is set from the class's `Repairability` (Auto / Interactive / Partial /
Unrepairable) so the UI knows which need a prompt before running.

## 20. Edge-case catalogue (each ⇒ a `tests/fixtures.rs` case)

- Stream body literally containing `endstream`/`endobj`/`1 0 obj` byte sequences
  (verified by inflate-probe length, not keyword scan).
- `/Length` correct · wrong-too-short · wrong-too-long · indirect (fwd ref) ·
  missing entirely.
- `stream` followed by LF · CRLF · CR-only(reject) · space+EOL.
- Object with no `endobj`; two objects sharing a number; gen ≠ 0.
- `%PDF` preceded by junk bytes (C1 + valid body) — header rewrite path.
- ObjStm holding the page dicts (objects only reachable after §14.4).
- Cross-reference **stream** file (PDF 1.5+) with no classic xref.
- Truncation cutting mid-stream (C10) vs. cutting the trailer only.
- Type0/Identity-H CID font (code==gid) vs. simple font with `/Differences`
  encoding; Arabic positional-form gids resolved only via shaped `.gmap` entries.
- Multi-filter stream (`[/ASCII85Decode /FlateDecode]`).
