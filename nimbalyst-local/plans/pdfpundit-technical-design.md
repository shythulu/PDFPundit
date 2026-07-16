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
  updated: "2026-07-16T13:19:32.000Z"
  progress: 0
---
# PDFPundit — Technical Design (v1)

## Context

The feature plan [pdfpundit-desktop-app.md](/Users/shylo/source/PDFPundit/nimbalyst-local/plans/pdfpundit-desktop-app.md)
defines *what* PDFPundit is: a pure-Rust, REPDF-grounded forensic PDF repair
tool with a retro ratatui TUI, targeting the C1–C10 corruption taxonomy, plus a
fast-follow PDF→Markdown export. This document is the level below — the
technical design an implementer codes from: module tree, core data model,
concurrency/event architecture, carver and rebuild algorithms, font database
and substitution policy, persistence, TUI architecture, and testing.

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
2. **UI layout**: **cat-first**. The window starts clean — pastel ASCII cat on
   full display, no panels. Adding files (drop or in-app browser) summons a
   floating queue panel that grows downward; a hideable analysis panel sits
   beneath it; a bottom progress bar shows current-file progress plus a batch
   total line when more than one file is queued. Decisions (font selection etc.)
   are presented as TUI context menus. Replaces the plan's three-always-visible
   panels. (§7)
3. **Font substitution is an output option**: default = bundled open-source
   "sister" fonts (Unicode-correct Noto-style replacements from the template
   DB); switchable to **system fonts matched by name**. Analysis precedes
   output, so PDF fonts with no equivalent become per-font user choices in a
   TUI prompt — individual selection plus select-all. (§5.5)

### Design refinements over the plan (found during design)

4. **"Async job runner" resolved as `std::thread` + `std::sync::mpsc`** — no
   tokio, no crossbeam. Jobs are CPU-bound byte crunching; ureq is sync. (§3)
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

## 2. Module tree (delta from plan marked `+`)

```
pdfpundit/                       # single crate (+ tools/ workspace member)
├─ src/
│  ├─ main.rs                    # terminal init/teardown, event loop, panic hook
│  ├─ app.rs                     # App state, Action enum, modal stack, dispatch
│  ├─ input.rs                   # crossterm events, bracketed paste, path normalization
│  ├─ config.rs                + # Config (config.toml), app-dirs resolution
│  ├─ theme.rs                   # Theme struct, palettes, color-capability detection
│  ├─ catbg.rs                   # cat fetch/render/cache; CatLayer buffer
│  ├─ jobs.rs                    # JobRunner, AppEvent/JobEvent, CancelToken, interaction rendezvous
│  ├─ library.rs                 # HistoryStore trait + JSON backend
│  ├─ ui/
│  │  ├─ mod.rs                + # render(frame, &App); layout; CatBackdrop widget
│  │  ├─ queue.rs              + # floating file-queue panel (replaces browser.rs)
│  │  ├─ analysis.rs           + # hideable findings panel (replaces report.rs/actions.rs)
│  │  ├─ progress.rs           + # bottom progress bar (current file + batch total)
│  │  ├─ menu.rs               + # context-menu modal (actions, decisions)
│  │  ├─ fontpick.rs             # font-candidate / substitution prompts
│  │  └─ picker.rs               # in-TUI .pdf-filtered filesystem browser
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

## 3. Concurrency & event architecture (`jobs.rs`, `main.rs`)

**Decision: `std::thread` workers + `std::sync::mpsc`.** The UI thread is the
single consumer of one merged channel (`Sender` is `Clone` — input thread and
job threads share it); `recv_timeout` synthesizes ticks; interactive prompts use
a bounded `sync_channel(1)` rendezvous. crossbeam-channel is a drop-in if mpsc
ever shows contention (it won't at these event rates).

```rust
pub enum AppEvent {
    Input(crossterm::event::Event),          // Key, Mouse, Paste, Resize
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

**Threads & ownership.** The UI thread (main) owns `App`, the `Terminal`, the
`Receiver<AppEvent>`, and the `JobRunner`. An input thread loops
`crossterm::event::poll(50ms)`. Job workers each own their inputs (`PathBuf`, a
cloned `JobOptions` snapshot, `Arc<FontDb>`) and return data only through
`JobEvent`s — no shared mutable state.

**Batch semantics (sequential).** PDF jobs (analyze/repair/export) run **one at
a time**: `JobRunner` holds a `VecDeque` of queued jobs and starts the next when
the current finishes. This makes the bottom bar's "current file" line exact and
guarantees decision prompts arrive one file at a time. `FetchCat` runs on a
separate slot (never queued behind PDF work). Parallelism stays a future config
knob (`[jobs] parallel`).

```rust
pub struct CancelToken(Arc<AtomicBool>);     // .check() -> Result<(), Cancelled> in every loop

pub struct JobRunner {
    tx: Sender<AppEvent>,
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
`JobEvent::Failed { panicked: true }` — a bad PDF can never kill the app. A
process-level panic hook restores the terminal (leave alt screen, disable raw
mode/mouse/paste) before printing, covering UI-thread panics.

**UI loop:** `recv_timeout` until next 100 ms tick → `app.handle(ev)` → drain
`try_recv` burst → redraw once if `app.dirty`.

**Blocking interaction from a job** (used by both font prompts):

```rust
fn interact(&self, kind: InteractionKind) -> Result<InteractionReply, Cancelled> {
    let (tx, rx) = mpsc::sync_channel(1);
    self.send(JobEvent::NeedsInteraction(InteractionRequest { kind, reply: tx }));
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
xref_spans, trailer_spans, startxref, eof_markers }`, where `CarvedObject`
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
/Identity`, full `/W` array, and a **full, non-subset** `/FontFile2`, plus a
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
theme = "amber"                   # amber | phosphor | dos16
mouse = true
cat_background = true             # the star of the empty state; obeys [general].offline
cat_refresh_hours = 24

[catapi]
api_key = ""                      # overrides the embedded obfuscated key
```

## 7. TUI architecture (`app.rs`, `ui/`) — cat-first layout

**The window starts clean: no panels, the pastel ASCII cat on full glorious
display**, plus one dim footer hint ("drop a PDF here · b browse · q quit").
Panels exist only once files do.

```rust
pub struct App {
    pub config: Config, pub theme: Theme,
    pub queue: Vec<QueueEntry>,            // empty ⇒ cat-first empty state
    pub selected: Option<usize>,
    pub show_analysis: bool,               // analysis panel under the queue (toggle: Enter/a)
    pub focus: Focus,                      // Queue | Analysis
    pub modals: Vec<Modal>,                // stack — top owns input
    pub pending_interactions: VecDeque<InteractionRequest>,
    pub batch: Option<BatchState>,         // drives the bottom progress bar
    pub runner: JobRunner,
    pub store: JsonHistoryStore,
    pub fontdb: Arc<FontDb>,
    pub catbg: Option<CatLayer>,           // pre-rendered cells at current size
    pub layout: LayoutRects,               // last-frame Rects for mouse hit-testing
    pub dirty: bool, pub should_quit: bool,
}

pub struct QueueEntry {
    pub path: PathBuf,
    pub status: EntryStatus,               // NotStarted | InProgress | Done | Error(String)
    pub meta: Option<FileMeta>,
    pub findings: Vec<Finding>,
    pub font_resolutions: Vec<FontResolution>,   // §5.5 — filled by analysis
    pub job: Option<JobId>,
}

pub struct BatchState {                    // total % = (completed + current_progress) / total
    pub current: usize, pub current_progress: f32,
    pub completed: usize, pub total: usize,
}

pub enum Modal {
    FilePicker(PickerState),               // .pdf-filtered fs browser
    ContextMenu(MenuState),                // per-file actions: Analyze / Repair… / Export…
    PassChecklist(ChecklistState),         // tick C1–C10 passes before Repair
    FontPick { req: FontPickRequest, reply: SyncSender<InteractionReply>, cursor: usize },
    FontSubstitution { rows: Vec<SubstRow>, reply: SyncSender<InteractionReply> }, // + select-all
    Confirm { title: String, body: String, on_yes: Action },
    About,
}
```

**Layout & widgets:**

- **Queue panel** (`ui/queue.rs`) — floating, anchored top-left with a 2-cell
  margin, ~45% width (min 40 cols). Height = entries + chrome; **grows downward
  as files are added**, scrolling past ~60% of screen height. Each row: status
  icon + filename + terse result note ("3 findings", "repaired ✓"). Icons:
  `·` not started, spinner frames `◴◷◶◵` in progress, `✓` complete, `✗` error
  (ASCII fallbacks at 16 colors).
- **Analysis panel** (`ui/analysis.rs`) — directly beneath the queue, same
  width, **hideable**; shows the selected file's info: metadata line, findings
  grouped by severity (expandable to evidence rows incl. hex windows), font
  resolutions, and after a repair, the pass outcomes + output path.
- **Bottom progress bar** (`ui/progress.rs`) — full-width strip overlaying the
  bottom edge while a batch runs. Line 1: chunky `█▓▒░` gauge + "repairing
  report_2024.pdf 63%". Line 2 (only when `batch.total > 1`): "batch 3/7 — 38%
  total". Disappears when idle.
- **Context menus** (`ui/menu.rs`) — every decision is a modal popup near the
  selected row: the per-file action menu (Enter/right-click), the repair-pass
  checklist, and the two font prompts (§5.3, §5.5).
- **Cat backdrop** — a custom widget writing pre-rendered cells (char +
  pastel-dimmed fg) straight into the frame `Buffer` first; panels then render
  opaque on top (`Clear` + solid `bg`) so the cat never bleeds through text.
  `CatLayer` is produced off-thread by the FetchCat job (ureq with the
  rustls-graviola provider → `image` decode → **`artem`** lib
  (`default-features = false` to drop its bundled ureq; sized to the terminal)
  → parse the ANSI output into cells (`ansi-to-tui` or a ~30-line parser) →
  `palette` Oklch lighten/desaturate per cell), re-rendered on `Resize`
  (debounced 200 ms), cached on disk, bundled default when offline.

**Input routing:** events translate to an `Action` enum (`AddFile`,
`StartAnalyze`, `ApplyRepairs(Vec<CorruptionClass>)`, `ToggleAnalysis`,
`OpenMenu`, `Quit`, …). Dispatch: top-of-modal-stack first, then focused panel,
then globals. Mouse clicks hit-test `App.layout`.

**Drop/paste:** enable bracketed paste; `Event::Paste` → strip quotes (macOS),
unescape `\ ` (Linux), normalize; accept multiple whitespace/newline-separated
paths in one paste (multi-file drop). Verify `.pdf` extension and sniff `%PDF-`
in the first 1 KiB — a missing magic only downgrades to a confirm dialog, since
C1 files are our business. Non-bracketed terminals get a key-burst detector
fallback; the picker is the guaranteed path.

**Theme system:** `Theme { bg, panel_bg, fg, dim, accent, severity[3], border,
border_focused, border_set (DOUBLE), progress_chars, banner }`; three palettes
(amber / phosphor / dos16) defined in RGB with automatic quantization to 256/16
colors; the cat degrades to dim monochrome at 16.

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
|---|---|---|
| 1 | Crate scaffold; `pdf/model.rs` core types; `jobs.rs` **event/channel skeleton** (AppEvent/JobEvent/CancelToken); `main.rs` loop + panic-safe restore; `theme.rs` minimal; `app.rs` + cat-first empty state + `ui/queue.rs`; `input.rs` paste (multi-path); `ui/picker.rs`; `catbg.rs` with bundled cat (network fetch later) | M1 |
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
- [ ] TUI mockup (cat-first layout: empty state, floating queue + analysis
  panels, bottom progress pair, font context menu) — in progress; link here
  when done.
- [x] Feature plan patched (2026-07-16): JSON persistence, threads-not-async
  wording, ObjStm carving note, refined FFI/license rules, cat-first UI
  layout, and the §12 crate decisions (harfrust, fontique, hayro-interpret
  only, ureq+graviola, artem, palette, image trim).

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
