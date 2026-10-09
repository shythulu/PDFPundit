# Implementation status, 2026-10-08 (end of the autonomous implementation session)

Branch: `claude/wayfinder-implementation` at 5960eda (merge of T-38b) plus the two documentation
commits that follow it. The session ran the plan's scheduler over `tickets.json` in three runs:
the first stalled after T-01 was built, the second crashed during T-11b/T-12a/T-23a, and the third
finished the plan. Every one of the 48 buildable tickets is merged. T-33, T-34 and T-35 were never
scheduled, because the plan lists them as blocked on user decisions. The facts behind every row are
in `decision-log.md` (implementation addendum, D-078 onward) and in the per-ticket records the
scheduler kept.

## What happened, in short

| Fact | Value |
|---|---|
| Tickets merged | 48 of 51 (every buildable ticket); 0 failed; 3 not started (T-33, T-34, T-35) |
| Review rounds | 88 reviews over 48 tickets; every ticket ended with an `approve` verdict; 503 findings recorded, none blocker or major left open |
| Gates on the merged tree (authoring Mac only) | `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test` pass: 1,150 lib tests + 9 integration tests, 4 ignored (the three corpus runs and the exFAT placement case), plus the `#[ignore]` pty/console cases in `tests/guard.rs` |
| Hosted CI | Never run. Nothing was pushed, so `.github/workflows/ci.yml`, `corpus-smoke.yml` and `corpus-ocr.yml` have never executed; every Linux and Windows leg is unverified (D-058) |
| Commits | 144 unsigned commits after 9439a30 on this branch (D-079); 48 `impl/T-xx` branches kept locally |
| Fact tasks | A-05, A-06, A-07 done and folded into D-069, D-037, D-011; A-04 (the OCR Python lock) was produced by T-38b itself (`tools/ocr-recall/requirements.lock`, `models.toml`) |
| User decisions still open | the 19 of revision 9, plus D-079 (re-signing) and D-091 (the DarkBerry palette licence); 20 defaults are flagged for the user in the addendum |

## Ticket table

Status words: `merged` (merged, acceptance met as the ticket is written, apart from legs that only a
hosted runner can show); `merged-partial` (merged, but a stated acceptance criterion or interface item
is knowingly not met, or a data path the ticket needs does not exist); `not started` (blocked in the
plan, never scheduled). No ticket is `failed`, `blocked` or `skipped`. "Still missing" lists only
things a user or a later ticket will notice; review nits are in the section after the table.

| Ticket | Title | Status | Merge commit | Still missing | Why |
|---|---|---|---|---|---|
| T-01 | Crate scaffold and CI gates | merged | 14b65af | A first hosted CI run; `f32/f64::sin_cos` is not in the clippy ban list (D-076); the purity gate skips the project's own tree (D-082); ratatui's default features keep `time` in the graph (D-083); the facade gate accepts E0603 without the "module `engine` is private" text (D-080) | Nothing was pushed; the four items were reviewer notes the later tickets did not pick up |
| T-02 | Core types | merged | fa23b12 | `MetricValue` (untagged), `HexWindow` newtype and `Ratio` value-equality are frozen into T-02b's snapshots (D-105) | The plan left these field types open |
| T-02b | Facade, jobs and report contract | merged | 7422280 | `RepairOptions` has eight fields, not five; `RepairRun` carries the run facts (D-105) | `repair()` needs the salvage budget and the two font knobs to fill `SettingsSnapshot` |
| T-00 | Golden cell dumps | merged | 9624a0b | The ubuntu leg of "generate.py reproduces the committed set" (D-055) | Linux glibc libm never ran the generator |
| T-03a | Writer first cut, goldens, structural corruptors | merged | 5afc6f7 | Nothing | C4 spans run into neighbouring objects by design (D-106) |
| T-03b | ObjStm golden, C9 corruptor, adversarial builders | merged | cb2bfe0 | Nothing | C9 never touches ObjStm containers; the outside-stream rate is the ticket's 1/3, not the measured 1/4 (D-106) |
| T-04 | Tolerant lexer | merged | 2acbca7 | `endob` + EOL + next header is not recognised as a near-miss keyword (review note) | The deletion reading fires only before a delimiter |
| T-36 | Text extractor and paint counts | merged | fc40c76 | Windows/Linux dump hashes; Type3 and inline-image fixtures | No hosted runner; reviewer nits |
| T-06 | Streams A | merged | a9c405f | A non-integer `/DecodeParms` value silently takes its default (review note); `q cm Do Q` with no text classifies as Form (D-107) | Reviewer notes left open |
| T-05 | Carver A | merged | ebb9383 | Rung (f) of a last stream without `endstream` swallows the trailer; lexer container cap is not a `CapHit` (review notes) | Rules 3 and 6 as built (D-108) |
| T-07 | Carver B | merged | b7ac30f | Two stream orphans in one gap with bad lengths merge; a C9 xref stream with an Adler mismatch gives no rows; a headerless ObjStm is never expanded (review notes) | Known limits, recorded |
| T-08 | Streams B: C9 salvage ladder | merged | 5f00962 | W does not count output volume, so a flat 16 MiB stream runs `work` in minutes (D-110, research-pending) | Plan-level definition of W |
| T-08b | TrueType-checksum localizer | merged-partial | 7e30de5 | glyf damage is `Accepted` under `work`, not `Exact`; the gaps-only window reaches the trailer gate without a body search (D-111) | The complete glyf window is 5,732 positions, not "a few hundred" |
| T-09 | Object graph | merged | 226022f | A null or dangling `/Resources` stops inheritance (review note) | Pages found by byte order are appended even when the tree is intact (D-112) |
| T-10 | Rebuild | merged | fa428d4 | Non-page referrers can claim orphans by key-path tail; a form field's dangling `/Parent` is never reported; references inside orphans are not reconciled (review notes) | TD's table names only the tail |
| T-11a | Diagnose: structural classes | merged | 1da3138 | A C5 cut of the last object also reports a spurious C3; one wild xref offset in an intact file reports C10 (review notes) | Rules as written in the #14 table |
| T-11b | Diagnose: C6-C9, outlined and Type3 text | merged | 8c210b8 | Never-embedded fonts are C7/C8 Errors that park on a font pick (D-084); C6 wording for a dangling reference (D-085); C9-outside-stream never fires because the carver never enables near-miss mode | The #14 table's "not embedded" Warning has no `FindingKind` |
| T-12a | Emit (Resave) and writer rules | merged | 733123b | Old `/Pages` nodes are still written beside the flat tree (D-086); the "no compression" tripwire test scans nothing in emit.rs (review note) | Deviations recorded |
| T-12b | Verification gates | merged | 3f38034 | `content_bytes` measures stored bytes; the Extracted baseline double-counts glyphs under hayro's fallback font (D-088); `show_counts` decodes every content stream at once (review note) | Frozen `Baseline` shape; T-13a's C5 seeds 2-4 are excluded from the V1 check |
| T-13a | Planner, generate-and-validate, structural passes | merged-partial | ea07624 | C5 seeds 2-4 fail V1 on a perfect output (D-088); an emit failure gives no `NoCandidatePassed`; `passes = Some(..)` with no matching class logs nothing (review notes) | T-12b baseline bias |
| T-13b | C9 swap and C6 re-link passes | merged | da55861 | `widths_fit` overflows on a crafted `/FirstChar` near `i64::MIN` (debug panic, caught by the runner); `c9_summary` is kept when no candidate passed (review notes) | Re-link ranking order recorded (D-113) |
| T-14 | Facade bodies and end-to-end tests | merged-partial | c05773f | No images are extracted (`images` and `extracted_images` stay empty); a clean file returns `Ok` with no output; a rebuilt state is repaired against the stale plan (review note) | No pass extracts images (D-114) |
| T-15 | Job runner, placement, panic guard | merged | 41f70e6 | Evicted re-runs are unbounded (D-005 amended); the temp file is reopened by path (hard-link window, D-044 amended); non-UTF-8 input names are lossy in the output stem (review note) | `Config` was not a dependency, so `destination_for` takes `Option<&Path>` |
| T-16 | Config, app dirs, history store | merged | 000900e | Two processes share `index.json` with no lock (review note); Windows drive-relative `output_dir` is not caught | Linux/Windows `resolve()` unrun |
| T-17 | Themes and colour math | merged | 2ebfd86 | The 16-colour downgrade merges `dim` with `error` (Blackwater) and `heading` with `error` (Mono Ink) (D-090); the DarkBerry palette has no licence text (D-091) | `ColorCaps` defined here (D-090) |
| T-18 | Cat renderer | merged | c7dd86e | Nothing | Plate rule and model-space gaze recorded (index) |
| T-19 | Director | merged | 2ad403c | The full-layout drag hint has no file count; `NeedsYou` uses gy 0.1 from the golden, not the ticket's 0 (D-116) | `DragAt` carries no count |
| T-20 | View model | merged | 5297c3f | Frames 03 and 05 draw data the view model does not carry (D-117); an export on a repaired file reclasses it as working (review note) | ViewModel extension is a plan-level gap |
| T-21 | Canvas, layout seam, widget, one-line fallback | merged | d800ddf | The drag count on the widget's status bar is blank at runtime (D-116) | Same |
| T-22a | Full layout: chrome and idle | merged | d02472d | The file label and "dragging n files" are not drawn (D-116); bidi-control and wide characters reach cells (D-118) | Same, plus a Canvas cell-width policy is undecided |
| T-22b | Full layout: batch and result views | merged-partial | 5c6b17f | Cell-exact equality for frames 03 and 05 holds only with masked spans; the C9 count line sits in the RECOVERY rows; recovery bars are not drawn (D-117) | The view model has no such data |
| T-24 | Modals: font pick and theme chooser | merged | c2fed69 | "[tab] switch", "[e] copy & edit", "load theme file" are inert (D-048 amended); "n of m" counts per (page, slot), not per font (review note); previews draw one char per cell (D-118) | System fonts wait on fontique (M5) |
| T-23a | Terminal guard, loop, blit, flush | merged | ec102e5 | The terminal is restored twice on a UI panic; a dead input source cannot be quit; a resize during start-up is lost (review notes); ratatui still has default features (D-083); the ConPTY true case is `#[ignore]` | Windows runtime unverified (D-058) |
| T-23b | Paste parser, Windows collector, drop gate | merged | 0b1aa01 | On Windows the collector's idle flush runs only on a Tick, so held keys wait while a job reports progress; `\\.\UNC\` and `\??\UNC\` forms pass the gate (review notes); `drop_sniff` reaches no engine type (D-089) | No field to carry the sniff |
| T-37 | Browse picker | merged | 239cd6a | The widget picker inherits blinking cells (review note); five Windows-only tests never ran | Self-goldens until the user supplies a frame (D-048) |
| T-31 | kitty OSC 72 drag tracking | merged-partial | 177ca0c | FR-12's live kitty capture (the "verified" acceptance); a new drag without a leave is not answered; a `t=R` that interrupts a chunked `t=r` loses its hint (review notes); files are read on the UI thread; `t=R` shows fixed hints, not the terminal's text (D-119) | No kitty session ran on the authoring Mac |
| T-25 | Text-recovery metrics | merged | 14f74d9 | Nothing | RawMyers, not `from_slices` (index) |
| T-26 | Corpus harness and CI smoke job | merged-partial | 037d7e4 | The paper's reference columns (D-070: `bench/repdf_paper_tables.toml` is committed but never read); the first hosted smoke run; the full 1,000-file run was not completed here | Blocked on D-070; no push |
| T-38a | Corpus `ocr` mode | merged | 178025b | `write_ocr` has no unit test outside the ignored corpus run (review note) | Reviewer note |
| T-38b | OCR word-recall harness | merged-partial | 5960eda | The first nightly run (uv, cache, runner latency, OCR text equality); the Document AI leg (D-014); an unopenable original silently drops its repairs from the CSV (review note) | Hosted runner needed; user decision |
| T-27a | AGL and the glyph-name ladder | merged | 638e87c | Per-name hex fallback for `cNN` mixes encodings inside one font; five `_` names in the pdf.js table are unreachable (review notes) | Font-wide radix needs T-27c's context (D-120) |
| T-27b | .gmap, GmapTable, ToUnicode builder | merged | da5c9c9 | `scripts` lists only the languages' scripts, so Greek and Cyrillic coverage is penalised by T-28 (review note) | `IndexEntry` and `BuildError` widened the public surface (D-047 amended) |
| T-27c | Decode-through-own-fonts ladder | merged | b5dbc1e | A CIDFontType0 with an OpenType program is read as gid = CID; a damaged charset hides the collection rung; an implied StandardEncoding is tagged `Agl` (review notes) | `CarvedFont` is undefined; the ladder takes `&lopdf::Dictionary` (D-120) |
| T-29 | tools/build-templates and the runtime template builder | merged | f501a45 | Nothing user-visible | `template::build` returns `Result`; aliases curated (D-121) |
| T-28 | Inference scorer and FontResolution policy | merged-partial | 06e1282 | Word lists (D-011): `dicts` is empty in production, so C8 inference always asks; the 95% coverage check uses the best-covering font, not the accepted one (review note); the `zh` bigram path is untested | Assets blocked on D-010/D-011 |
| T-30 | C7/C8 passes, TemplateAssemble, font pick | merged-partial | eadec45 | An unconfirmed C8 guess under `UseBest` beats Resave with wrong text (D-122); ligature `/ToUnicode` entries keep one character (D-123); no LayoutNote for text-only fonts (D-124); a form sharing the page's resources keeps the damaged font while the pass says Fixed (review note) | No word lists; interface gaps |
| T-32a | Layout analysis for Markdown | merged-partial | c32b25c | Images are not placed by bbox (D-114); RTL glyphs drawn in visual order come out reversed (D-115); a late-drawn superscript in a mixed-direction stretch moves to the end (review note) | `ExtractedImage` has no position |
| T-32b | Markdown emitter and export action | merged-partial | f896ea1 | `!` before a link is not escaped, so a crafted PDF can make the `.md` fetch a remote image when opened (review note, must fix before any release); page notes use input page indices (wrong page after a C4 rebuild); a re-export of a vanished file promise marks a repaired row Failed (review notes) | Re-export re-runs the repair in memory (D-125) |
| T-33 | Custody mode (M6b) | not started | none | Everything | Blocked in the plan: schema design (D-016) after T-14 |
| T-34 | OCR (M9) | not started | none | Everything | Blocked in the plan: model delivery (D-050) |
| T-35 | Release packaging, docs, notices | not started | none | Everything | Blocked in the plan: licence (D-046) and the Smudge likeness (D-065); the DarkBerry palette licence joins the list (D-091) |

## What a user can run today

On this branch, on the authoring Mac (macOS, arm64), in a real terminal:

| Step | What happens |
|---|---|
| `cargo run --release` | The guard refuses a non-terminal with exit 2; on a terminal it handshakes (kitty `t=q` + DA1, 300 ms), flushes stale input, draws the full layout at 112×38 or more, the widget below that, and the one-line face below 32×16 (D-064: no input there). Argv is ignored. |
| Drop a PDF | kitty: OSC 72 drag tracking, the cat looks at the file, the chomp runs, bytes are read before `t=r:o=1`. Any Unix terminal: a bracketed paste of paths. Windows: the typed-path collector (150 ms burst rule). `b`: the `browse…` picker. The drop gate takes `.pdf` only, refuses UNC paths, warns above 512 MiB, refuses above 4 GiB. |
| The batch runs | Analyze → plan → repair per file, autonomously (GG §1). Classes C1–C10, Encrypted, Signed, OutlinedText and Type3Text are diagnosed; C1–C5, C10 (Resave), C9 (salvage swap), C6 (re-link) and C7/C8 (TemplateAssemble with the bundled Noto Sans/Serif) are repaired; every candidate passes V0 (lopdf reload, hayro render, re-diagnosis) and is ranked by V1/V2/V3. Output is `<name>.repaired.pdf` beside the input or in `[general] output_dir`, never replacing a file; a read-only destination fails with the fixed message (D-061). |
| A font question | C7/C8 fonts the database cannot confirm park the job with the font-pick modal (`↑↓`, Enter, `u` use best, `s` skip); the batch continues (D-005). With no word lists shipped, every C8 inference asks (D-011, D-122). |
| `e` | Exports `<stem>.md` beside the output, with page separators, headings, lists, tables, links (http/https/mailto/ftp only) and per-page notes; `<stem>.<hash8>.images/` is created only when images exist, and no pass extracts images yet (D-114). A second export of the same input gives byte-identical Markdown. |
| `T` | The theme chooser (DarkBerry Blackwater default, 7 themes, 256- and 16-colour downgrade). |
| `H`, `S`, `?` | The "not yet" hint (D-048). History is recorded under the platform data dir (T-16) but has no screen. |
| `q` | Quits and restores the terminal; a UI panic restores it from the hook and sends kitty `t=A`. |
| Config | `config.toml` under the platform config dir: `[general] output_dir`, `[repair]` knobs (`salvage_work`, `salvage_deep_work`, `max_search_stream`, `auto_accept_confidence`, …), `[ui]` theme and layout pin. `PDFPUNDIT_ASSETS` and `PDFPUNDIT_PANIC` exist in debug builds only. |
| Dev-only | `PDFPUNDIT_CORPUS=~/corpora/repdf cargo test --release -- --ignored smoke` (44-file smoke run against `bench/golden_smoke.csv`, 2% slack), `ocr` (page rasters for `tools/ocr-recall`), `full`. `tools/ocr-recall/ocr-recall` scores REPDF-style OCR word recall with PaddleOCR (tiny tier per script). `cargo run -p build-templates` regenerates the gmaps and `assets/fontindex.json` and checks them. |

What is not there:

- No binary has run on Windows or Linux. Every Windows path (console flush, collector, ConPTY guard case, `SHGetKnownFolderPath`, `MoveFileExW`, junction handling) and every Linux leg (glibc generate.py, placement tiers, app dirs) is compile-checked only (D-058).
- No CI run has ever happened; `cargo deny`, the purity gate on the Linux and Windows targets, the ui-goldens job, the assets job and both corpus jobs have only been run by hand on the authoring Mac where they could be.
- No word lists, so C8 inference never auto-accepts (D-011); no system fonts (fontique, M5); no custody mode (T-33); no OCR (T-34); no release artefact, licence file, or `license` field (T-35, D-046); no history, setup, error or export screens (D-048).
- Markdown export carries no images (D-114) and has the unescaped-`!` link defect (T-32b row above).
- The repaired file resurrects pages an incremental update had deleted (D-112) and keeps the old `/Pages` nodes (D-086); both are visible to a forensic reader.

Goal guards that exist: `publish = false`; `engine` is `pub(crate)` and the dependant test proves it is unreachable; `run()` ignores argv and refuses a non-terminal; pre-frame bytes are discarded and the input buffer flushed; the one-line fallback accepts nothing; the corpus harness refuses any file not in the 1,100-entry manifest; `clippy.toml` bans the clock types and the libm-lowered float methods crate-wide (except `sin_cos`, D-076); the purity, network and licence gates exist (unrun on a hosted runner); the artefact scan proves no UI string reaches a repaired PDF, a report or an export of the fixtures.

## Open review notes by ticket (minor and nit items from each ticket's final approving review, none fixed)

Items that became a decision-log entry are marked with their D-id; the rest are recorded here only.

- **T-01:** `sin_cos` ban (D-076); unpinned `stable` toolchain and no `rust-version`; `bytemuck` `derive` feature unused; the scratch dependant's manifest writes the repo path as a TOML literal string.
- **T-02:** `MetricValue` untagged (D-105); `Ratio` value-equality vs serialised bytes; `ObjectKind::Other` name convention (with or without `/`); `page_sizes` rounding rule unstated.
- **T-00:** ubuntu generate.py leg (D-055); `float_roundtrip` on the runtime dependency; `dump_goldens.py` clears any OUTDIR; `Golden::mismatches` size-mismatch sentinel.
- **T-04:** `endob`+EOL not a near-miss; `LIT_SCANNED` counter over-documented; `RealNarrowed` added at the tokenizer (D-075); `+-5` sign reading.
- **T-02b:** budget invariant for state reuse (built by T-14, D-073); `BatchState.current` is positional; `signed_note` not set by `default_for`; `THEME_NAMES` duplicates theme.rs; `Done` with `run == None` is ambiguous.
- **T-03a:** no sha256 pin of the golden bytes; C4 spans run into neighbours (D-106); C5 picks objects 1–9 only; the first-cut `Writer` accepts id 0.
- **T-17:** 16-colour downgrade merges roles (D-090); gradients outside `Theme` (added by T-21/T-22a since); `Palette::flavour` catch-all; two `Rgb` types; `mix` allocates per call.
- **T-36:** `is_stroke_of_last_fill` can drop a different glyph stroked at the fill's position (lost evidence); no Type3 or inline-image test; `KNOWN_LIMITS` reaches no report yet (D-038); `font_table` parses the file a second time.
- **T-03b:** outside-stream rate 1/3 vs measured 1/4 (D-106); parametrised builders are not in the repeat-build check; `fixtures.rs` is 2,156 lines.
- **T-06:** `int_parm` silently defaults on a non-integer value (silent wrong decode); explicit `/Subtype /Image` with a Flate chain takes the codec from magic bytes; `q cm Do Q` is Form (D-107); the `UNRESOLVED_PARMS` sentinel (emit restores the original entry, verified); a headerless stream past the cap is reported as a header failure.
- **T-15:** temp file reopened by path (D-044); unbounded evicted re-runs (D-005); lossy output stem and `\` refused on Unix; `Resumed` sent after `spawn`; no `JobRunner::active()`.
- **T-16:** `$HOME//Docs` in `user-dirs.dirs`; drive-relative `output_dir` on Windows; the `dead_code` allow moved to `lib.rs`; no inter-process lock on the history store.
- **T-18:** the whisker test only checks the head region; the min-line-width test checks less than its name; `plate` rule by width (index); `gaze_to` takes model space; `FOOD` index can hit 5 on a tiny negative hash.
- **T-05:** rung (f) swallows the trailer of a last stream without `endstream`; container cap is not a `CapHit`; `HeaderOutOfRange` fires on non-header digit runs; a failing classify stage is charged its whole cap.
- **T-25:** 0/1 only for recall's zero denominator; RawMyers is quadratic on unrelated inputs; `char_f1` counts spaces for Zh.
- **T-19:** full-layout Working/NeedsYou poses differ from frame 03 (T-22b uses the idle pose); `gy == 0.1` (D-116); no drag count (D-116); undecorated phrases missing from `ALL`; `CHOMP_FX[0]` carries sparkles; a `DragAt` during a chomp is dropped.
- **T-08:** trailer rewrite only after the body window (D-041); W ignores output volume (D-110); admission `+4·out` (D-072); edits identical only for Exact (D-041); the test allocator is the crate's one global allocator; damaged streams are inflated three times in phase 1; only the first `/FlateDecode` in a chain is salvaged; localizers register in `LOCALIZERS`; raw-retry slack of 2 bytes changes T-06.
- **T-27a:** per-name `cNN` hex fallback (D-120); five `_` names unreachable; `HEURISTICS_VERSION` test is tautological; NUL handling differs between layers.
- **T-20:** an export or re-analysis reclasses a repaired file as working; encryption read from two sources; "clean · nothing to fix" after all-Skipped passes with findings; `c9_line` duplicates the engine's line; Pending uses Body not dim and Skipped is neutral (index).
- **T-27b:** `IndexEntry`/`BuildError` public (D-047); `scripts` from languages only; the shaped-ligature skip branch is untested; width tests run at upem 1000 only; `FLAG_ITALIC` after truncation; `GmapTable` re-checks per construction; non-Latin language rows.
- **T-07:** two orphans in one gap merge; rung (c) before (b) (D-108); xref streams decoded all-or-nothing; cap-reached sweep still runs `orphan_at`; headerless ObjStm never expanded.
- **T-27c:** `mac_reverse` keeps empty MacRoman names; CIDFontType0 + OpenType read as gid = CID; a damaged charset hides the collection rung; implied StandardEncoding tagged `Agl`; `FontFile3` Subtype trusted over sniffing; `CarvedFont` (D-120).
- **T-21:** `OneLine::min_size` is (1,1) while it draws 12 cells; `FACE` literal duplicated in `ALL`.
- **T-09:** byte-order page candidates appended (D-112); null `/Resources` stops inheritance; depth-100 test does not pin 100; `MAX_FORM_DEPTH` asserted by itself; DFS not BFS (index); D-032 applied in two places (moved to rebuild.rs by T-10); no test for indirect `/Kids` or `/Contents` arrays.
- **T-22a:** `Canvas::text` replaces Cc but not Cf (RLO spoofing) (D-118); `menu_key` mutates state from a layout; the status bar branches on a hint string; `ALL` gains bare words ("MENU", "SYSTEM", "busy", "v0.1").
- **T-08b:** gaps-only window reaches the trailer gate (D-111); `adler_rerun` wording claims the original's checksum was wrong; glyf `Accepted` not `Exact` (D-111); admission taken for every damaged stream once a localizer exists; the no-inflate reject path is unreachable in production.
- **T-10:** `Want::of` by tail only; `/Parent` always owned by the page tree; orphan references not reconciled; deleted pages resurrected (D-112); integer `/Rotate 45` kept; dense renumbering past 8.4 M objects unreported.
- **T-24:** "n of m" per (page, slot); wide and Cf characters in previews (D-118); modal strings missing from `ALL`; inert hotkeys (D-048); `contains` deny-list check.
- **T-22b:** C9 line in the RECOVERY rows (D-117); masked spans (D-117); `ALL` conventions differ between T-24 and T-22b; `header()` slices `APP_NAME` by fixed ranges; `file_menu` added to T-20's types.
- **T-11a:** spurious C3 beside a last-object C5; a wild xref offset reports C10; wrong-`/Count` C4 counts as a cut; a `/Kids` cycle to the root gives two C4 findings at one location; `meta.rs` imports `trailer_dict` from diagnose.
- **T-23a:** double terminal restore on a UI panic; a dead input source cannot be quit; a resize during start-up is lost; `modal_status` duplicates the status bar; `handshake()` not `cfg(unix)`; `repair_started` leaks on failure; `started_at` read in `ui/app.rs` not `jobs.rs`; the hook-wiring test repeats T-15's; the merge commit e56b534 has no co-author line.
- **T-12a:** the "no compression" tripwire scans only emit.rs's module doc; `RebuildDoc` seam (closed by T-13a's `new`/`emit_doc`); old `/Pages` nodes written (D-086); silent raw-copy fallback in `stream()`; `reals_narrowed` counts tokens not values; `/Size 32` through emit.
- **T-11b:** C6 wording for a dangling reference and C6 beside C10 (D-085); `/Font null` asymmetry; the Ambiguous C9 test uses a hand-built index; `CarveSource::last_index` added in salvage.rs.
- **T-29:** `template::build` returns `Result` (D-121); the tool duplicates the coverage ranges and gmap record layout; OFL fetched from `main`; three spellings of the `F1` resource name.
- **T-23b:** idle flush only on Tick; `\\.\UNC\` and `\??\UNC\` pass the gate; `drop_sniff` (D-089); `DROP_BIG` names the threshold, not the size.
- **T-12b:** the Unrecoverable test's C9 assertion is conditional; `show_counts` holds every decoded stream at once (OOM on a hostile file); `partial` argument (D-087); `work_total` stands in for a Progress counter; no warning sink on the render gate.
- **T-28:** 95% coverage check uses the best-covering font; empty `options` under `SubstituteGeneric`; two template caches; silent `PDFPUNDIT_ASSETS` fallback; the confident-pick Unreproducible path untested.
- **T-37:** widget picker inherits blinks; five Windows-only tests unrun; the same file picked through a link and its folder is fed twice.
- **T-32a:** late-drawn superscripts in mixed-direction stretches (D-115); the per-ticket rows in the decision log (kept); `link_at` is glyphs × annotations; images after text (D-114); test name "mapped ink".
- **T-13a:** emit failure gives no escalation; `passes` with no matching class logs nothing; C5 seeds 2–4 (D-088); a kept cut dictionary gets no action; edits outside the paths (index); `load_mem` is not strict in a test helper; a duplicated fixture helper; `FontTrack` row refined by T-30.
- **T-31:** a new drag without a leave is unanswered; a `t=R` mid-chain loses its hint; signal-hook and rustix `event` (D-109); fixed `t=R` hints (D-119); the promise heuristic (D-119); split OSC prefixes can type keys; reading on the UI thread.
- **T-13b:** `i64` overflow in `widths_fit`; ranking order (D-113); `c9_summary` kept when nothing passed; the slot resets per content piece; `edit_list` duplicated from diagnose; per-pair width walks.
- **T-32b:** `!` before a link (fix before release); a vanished promise's re-export marks the row Failed; notes keyed by input page index; the one-shot tests are timing-dependent; `#` escaping inside code spans; a leading `|` can make a GFM table; the "text not extracted" note is unconditional (D-125); `finished` never shrinks.
- **T-14:** a rebuilt state is repaired against the stale plan; `pattern("")` would panic the scan.
- **T-30:** `UseBest` gives Partial (T-30-2, D-122); a shared-resources form keeps the damaged font while Fixed; the T-30 deny-list test is weaker than T-14's; `Generated.resolutions` reaches no report (D-124); pipeline docs stale; `slots_of` with `target == None`.
- **T-26:** full-run header variant (D-063); file-level CSV columns repeat per page row; `read_checked` has no unit test; the golden is Mac-only.
- **T-38a:** `write_ocr` untested; all PNGs held in memory; same-stem collisions; originals' `lcs_f1` 0/1 on empty pages.
- **T-38b:** an unopenable original drops its repairs silently; `isdigit` accepts non-ASCII digits (traceback); structural pairing checked after OCR; engine interface shape (index); `pdiparams_bytes` unchecked; `os.environ` writes leak into tests; `ocr_pages` counts unscored pages; README timing sentence; model cache key ignores the tier.

## Outside the repository, observed on disk

These live in the session's scratch folder on the authoring Mac and in `~/corpora`; they are lost on a reboot and nothing from them is committed.

- A-01: REPDF clone at `~/corpora/repdf` at e547d4d (`original/`, `corrupted/`), read through `PDFPUNDIT_CORPUS`.
- A-02: the staged font and AGL assets with `SHA256SUMS`; the committed copies are under `assets/` (D-021 as built).
- A-03: `tests/data/tiny.jpg`, `bench/repdf_manifest.txt` and `bench/smoke_subset.txt` are committed.
- A-05, A-06, A-07: fact reports `eng-r4-fr1.md`, `fr-13.md`, `fr-11.md`; conclusions restated in D-069, D-037, D-011.
- The 48 `impl/T-xx` branches and the per-slot `CARGO_TARGET_DIR`s.

## To resume

1. Decide D-079: re-sign the 144 commits before any push (command in the row), or push unsigned.
2. Untrack the two `.DS_Store` files and ignore the pattern (D-078).
3. Fix the two defects marked "before any release": the unescaped `!` before a Markdown link (T-32b) and the `i64` overflow in `widths_fit` (T-13b).
4. Push, and read the first CI run as the measurement for every Windows and Linux leg (D-058), the ui-goldens ubuntu leg (D-055), the corpus smoke golden (D-077) and the nightly OCR job (D-067).
5. Answer the user decisions: the 19 of revision 9, D-079, D-091, and the flagged defaults D-084, D-088, D-112, D-117, D-118, D-122.
6. T-33 (custody schema), T-34 (OCR) and T-35 (release) wait on D-016, D-050, and D-046/D-065/D-091.
