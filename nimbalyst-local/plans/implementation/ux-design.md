# UX design: history, setup, help, custody prompt, evidence page, frames 03 and 05

Design proposal, revision 4 of 2026-10-09. Nothing here is built yet. The frames live in `nimbalyst-local/mockups/pdfpundit-ansi-bbs/generate.py`, and their goldens in `tests/data/ui/`.

Revision 4 answers the third review. The custody prompt now says only what is true about what the app has read, its buttons have one focus marker, no screen tells two keys apart by case, and every screen says what a drop does. Revision 3 answered the second review, which aligned the custody screens with evidence schema revision 3. Revision 2 answered the first review. The review log at the end lists each finding and what changed.

## What this adds

| Still | Screen | Key | Decision it answers | Branch |
| --- | --- | --- | --- | --- |
| `10-history.png` | History | `H` | D-048 | `design/ux` |
| `10b-history-empty.png` | History, empty | `H` | D-048 | `design/ux` |
| `11-setup.png` | Setup | `S` | D-048, D-016 | `design/ux` |
| `12-help.png` | Help | `?` | D-048 | `design/ux` |
| `13-custody-prompt.png` | Case details prompt | on drop | D-016 | `design/ux` |
| `13b-widget-custody.png` | Case details prompt in the widget | on drop | D-016 | `design/ux` |
| `14-evidence.png` | Evidence page of a finished file | `v` | D-016, D-114 | `design/ux` |
| `03-batch.png` | Batch view, changed | none | D-117 | `design/ux-d117` |
| `05-result.png` | Result view, changed | none | D-117 | `design/ux-d117` |

Revision 4 changed `11-setup.png`, `12-help.png`, `13-custody-prompt.png` and `14-evidence.png` on `design/ux`. `design/ux-d117` was rebased onto it, so its 03 and 05 are unchanged from revision 3.

### Two branches, so the integration branch stays green

| Branch | Holds | Tests | Merge when |
| --- | --- | --- | --- |
| `design/ux` | Frames 10 to 14, their goldens, this doc. Frames 03 and 05 are untouched | All pass. The new goldens are extra files, and `load_all` only requires the canvases it lists | Any time |
| `design/ux-d117` | `design/ux` plus the D-117 changes to frames 03 and 05 and their re-dumped goldens | Three fail until the Rust follows: `batch_matches_frame_03`, `result_matches_frame_05`, `queue_text_matches_the_batch_golden` | In the same change that updates `src/ui/view.rs`, `src/ui/layout/full.rs` and the fixtures |

On `design/ux`, every still is byte-identical to `claude/wayfinder-implementation`, checked with `cmp`: all 42. On `design/ux-d117`, every still but 03 and 05 is. That includes `04-font-pick.png`, a dimmed copy of frame 03: every frame 03 change sits under its modal.

### What the build does first

1. Merge `design/ux`. Nothing breaks.
2. Build frames 10 to 14 as new screens behind the keys that answer "not yet" today.
3. With the frame 03 and 05 Rust change, merge `design/ux-d117`. Remove the `BATCH_UNFILLED` and `RESULT_UNFILLED` masks in `src/ui/layout/full.rs`: every masked span now has a named source, listed below.

### Custody screens follow the evidence schema

The custody screens (frames 11, 12, 13 and 14) follow `/tmp/pdfpundit-run/design/evidence-schema.md`, revision 3. Where the two documents could disagree, the schema wins, because it defines the files. The table under "Agreement with the evidence schema" maps each point.

## Rules every screen follows

These hold for frames 03, 05 and 10 to 14.

### Grid

| Row | Holds |
| --- | --- |
| 0 | Header: the logo mark and a crumb, as frames 03 and 05 draw it |
| 2 to 34 | Content boxes |
| 35 | Hint line, or a one-off message |
| 36 | Hotkeys for this screen |
| 37 | Status bar |

Help is the one exception. It is a modal over the dimmed idle screen, like the theme chooser.

### Colour roles

No new colours. Every cell uses a role the theme already has.

| Role | Used for |
| --- | --- |
| `W` heading, `w` body, `D` dim | Text levels |
| `C` file names | Names and paths the user owns |
| `Y` hotkeys | Keys, the focus marker `►`, the custody-on flag |
| `G` ok, `R` error, `M` needs input | States, always next to a word or glyph |
| `b` lightbar | The selected row of the focused list, the focused field |
| modal gradient | Boxes that ask the user something: case details, setup's detail panel, help |

### Focus and state survive without colour

One rule for every list, and one for screens with two lists:

| Row | Marker | Background | Under `NO_COLOR` |
| --- | --- | --- | --- |
| Selected row of the focused list | `►` in `Y` | lightbar | `►` and reverse video |
| Selected row of the other list | `►` in `D` | none | `►`, no reverse video |
| Any other row | none | none | plain |

- Frame 10 draws it. The files list has focus: `thesis_ar.pdf` has the yellow `►` and the lightbar. The runs list's selected row has a dim `►` and no lightbar.
- The hotkey row names where `tab` goes, not where focus is: `[tab] go to runs` while files have focus, `[tab] go to files` while runs have it.
- Every state is a word or a glyph: `√ repaired`, `~ partial`, `× failed`, `· pending`, `‼`, `[x] on`, `[ ] off`, `OFF`.
- Toggles always print `on` or `off`. Choices print `‹ value ›`, so the arrows say "←→ changes this".

### Text the user or a file supplied

File names, paths, finding summaries and the four case fields are drawn verbatim. Control characters are escaped, as the existing hostile-name tests require. A value too long for its column is cut at the column and ends in `…`. Frame 10 shows one: `payroll_2025_redacted.p…`.

### Typing goes to the text, never to a hotkey

Three places take text: the custody prompt's fields, the history filter and a setup value being edited. While one has focus:

| Key | Does |
| --- | --- |
| `enter` | Confirms. In the prompt it starts the batch |
| `esc` | Cancels the edit. In the prompt it cancels the batch |
| `tab`, `shift-tab` | Next or previous field, where the screen has several |
| `←→`, `home`, `end`, `backspace`, `delete` | Edit the text |
| paste | Inserts the text, unless the paste is a drop (see "A drop works on every screen"). A pasted newline does not confirm |
| Every other key | Is text, including `x`, `o`, `f`, `s`, `r`, `q`, `Q`, `H`, `S`, `T`, `?` and digits |

The tui-design guidance lists "global mnemonic shortcuts consume text input" as a known failure. This rule prevents it. The key dispatcher in `src/ui/app.rs` checks for a focused text field before any hotkey.

### Letter keys ignore case

Every letter hotkey matches both cases today: `q|Q`, `t|T`, `b|B`, `e|E`, `h|H` and `s|S` in `src/ui/app.rs` and `src/ui/layout/full.rs`. These screens keep that, so caps lock never changes what a key does. No screen gives `x` and `X` different jobs.

A key listed for a screen or a menu wins over a global key while that screen or menu has focus. The global keys `B`, `H`, `S`, `T`, `?` and `Q` keep their job everywhere, with one exception: `s` on setup.

| Key | Scope | Does | Why it does not clash |
| --- | --- | --- | --- |
| `s` | Setup | Save | `S` opens setup, and setup is already open |
| `r` | Setup | Put the selected setting back to its default | No global `r` |
| `r` | Evidence page | Open the Verify panel. The slow re-run is a toggle inside it, not a capital `R` | No global `r` |
| `r` | File menu, while open | `Repair options…`, drawn as `R` on frame 05 | The menu takes every letter while it is open, so the evidence page's `r` never reaches it |
| `f` | History, evidence page, file menu | Reveal the folder | The same job in all three |

### A drop works on every screen

A drop is never refused because of the screen, and it never changes the screen. The user decides when to look at the batch.

| Where the drop lands | What happens |
| --- | --- |
| Batch, result or evidence page | The files join the queue, as today |
| History, frames 10 and 10b; setup, frame 11 | The chomp plays on that screen's cat. The hint row says `N pdfs queued · esc to watch them`, and the status bar counts them. The screen stays |
| Help, theme chooser, font pick | The modal stays open. The hint row, under the modal, says the same |
| Setup with a value being edited | A kitty drop is a drop, and the edit keeps focus. A paste follows the paste rule below |
| Forget confirmation open | The files are taken as above. The confirmation stays open and keeps its keys, so a drop never answers it |
| Custody prompt open | The files join the pending batch, step 4 under frame 13 |

With custody mode on, the files go to the pending batch instead of the queue. The hint row says `N pdfs wait for case details · esc`. The prompt opens on the batch view only: when the user leaves history or setup, or closes a modal. It never opens over setup, so a half-edited setting cannot change under a batch. The batch uses the settings saved when `Start batch` is pressed.

Outside kitty, a drop arrives as a paste. A paste in which every item parses as a `.pdf` path is a drop, wherever focus is. The test is `paste.rs` on the names alone, with no file access. Any other paste into a focused field is text. So a drop on the custody prompt adds files instead of typing their paths into the case reference.

### Strings and the artefact deny-list

`ui::strings::ALL` is already the deny-list that `src/engine/artefact_tests.rs` scans every artefact with, case-sensitive and by whole word. It is curated: lone labels and column heads stay out, so ordinary report words do not hit. These screens keep that rule.

| Kind of copy | Goes into `ALL`? | Examples |
| --- | --- | --- |
| A phrase the UI draws | Yes, as drawn | `custody mode is on in setup`, `case details first.`, `the cat kept the receipts.` |
| A keyed label | Yes, with its key | `[v] evidence`, `[r] verify` |
| A lone label or column head | No | `examiner`, `sha256`, `case reference`, `submission reference`, `submission`, `notes`, `legacy`, `placed`, `after` |
| A phrase a custody template also prints | No. The screen takes it from the custody template module, not from `strings.rs` | `not provided`, `applies to all N files of this batch` |

The custody records use the lone labels, so putting them in `ALL` would fail every record. The custody templates are never added to `ALL` either. A phrase the evidence page shares with a custody template is sourced from the template module and never goes into `ALL`; otherwise every `.custody.txt` would fail the artefact scan.

### The widget

The 32×16 widget has no menus and never shows a form.

| Event in the widget | What it shows |
| --- | --- |
| `H`, `S` or `?` | The status line says `zoom me`; nothing opens |
| Something needs the user | The needs-you pose, `‼` blinking at the top right, one item on the status line, `‼ N` on the status bar |
| Zoomed to 112×38 | The full screen opens on what was asked for |

`‼ N` counts every open question: case details prompts and parked files together. The status line names one, in this order:

1. A case details prompt. It holds a whole batch, so it goes first (frame 13b).
2. Parked files, in queue order (frame 07, needs-you).

### How sources are written below

`Type.field` is a Rust field that exists on `claude/wayfinder-implementation` today. "Planned" marks a field that T-33, the D-114 extractor or this design must add; its name is a proposal. A custody record field is written as schema revision 3 names it, for example `run.input_file.unchanged`.

## Frame 03: batch view (D-117, branch `design/ux-d117`)

### Purpose

Show the batch while it runs: the queue, the file under the cursor, the parked file, the cat.

### What changed, row by row

| Row | Before | After | Source |
| --- | --- | --- | --- |
| 8 | `repairing · C9 salvage` | `repairing · verifying` | `EntryState::Repairing.phase`, from `JobEvent::Phase.name` |
| 14 | `PDF 1.4 · 6 pages · 1.1 MB · Microsoft: Print To PDF` | `PDF 1.4 · 6 pages · 1.1 MB · 212 objects carved` | `FileMeta.version`, `FileMeta.pages`, `QueueEntry.bytes`, `CarveSummary.objects`. The producer is dropped: no engine type has it |
| 16 | `carve ─ diagnose ─ plan ─ repair ─ emit ─ verify` | `carve diagnose salvage measure repair verify` | The six phase names the engine emits: `PHASES` in `src/engine/pipeline.rs`, then `"repairing"` and `"verifying"` in `src/pdf/repair.rs`. `■` done, `☼` current and blinking, `∙` to come |
| 17 | `toolpath Resave · structural only · keeps outlines` | `toolpath chosen after verify · candidate 1 of 2` | `JobEvent::Phase.index` and `.total` for `"repairing"` and `"verifying"`. The toolpath is unknown until `RepairReport.chosen` arrives |
| 20 | `obj 14 0 · p.3 contents` | `obj 14 0` | `Finding.location`, through `findings::location` |
| 21 | `rebuild from carved /Root` | nothing | `Location::File` has no detail |
| 22 | `[iNF] header intact %PDF-1.4` | `[iNF] text as outlines p.1` | `FindingKind::OutlinedText`. "header intact" is not a finding kind |
| 23 | `[iNF] 212 objects carved 6/6 pages found` | `[iNF] digitally signed` | `FindingKind::Signed`. The carve count moved to row 14 |
| 26 to 29 | Timed inflate and xref lines | Phase changes and engine log lines | See below |

### The log

Each line is one event of the selected queue entry, newest at the bottom, last four kept.

| Line kind | Format | Source |
| --- | --- | --- |
| Phase change | `11:42 measuring · step 4 of 4` | `JobEvent::Phase { name, index, total }` |
| Engine note | `11:42 warn 3 NaN or infinite numbers were written as 0` | `JobEvent::Log(LogLevel, String)`. `info` prints no level word; `warn` in `Y`, `error` in `R` |
| Time | `hh:mm` | The UI clock when the event arrived, as the status bar reads it. Seconds are dropped: the clock has none |

The ring belongs to the queue entry, not to a job. An entry runs a `JobKind::Analyze` job and then a `JobKind::Repair` job, and `QueueEntry.job` holds only the current or last one. Frame 03's log mixes the analysis job's `measuring` line with the repair job's lines, so:

- events of every job of the entry go into the entry's ring, in arrival order;
- the ring is cleared when the entry is re-queued (re-diagnose, or a retry after `INPUT_CHANGED`).

The events already exist; today `src/ui/app.rs` only writes them to the debug log.

Row 4's `214 pages · arabic` stays. Its sources are `FileMeta.pages` and `FontPickRequest.candidates[0].language`.

### Keys

Unchanged: `↑↓ select`, `enter file menu`, `i resolve`, `+ add files`, `p pause`, `q quit`.

### States

| State | What shows |
| --- | --- |
| Analysis not finished | Row 14 shows what is known; the rest stays blank |
| No findings | Rows 20 to 23 blank |
| More than four findings | The fourth row becomes `… N more`, as built |
| No log yet | Rows 26 to 29 blank |
| Job failed | Row 29 holds the failure reason in `R` |

## Frame 05: result view (D-117, branch `design/ux-d117`)

### Purpose

Show one finished file: where the copy went, what was wrong, what the copy kept, how fonts were resolved.

### What changed, row by row

| Row | Before | After | Source |
| --- | --- | --- | --- |
| 4 | `out thesis_ar.repaired.pdf` | same | `RepairRun.output_path`, file name |
| 5 | `in ~/cases/2026-091/evidence/` | same | `RepairRun.output_path`, parent folder, home shown as `~` |
| 6 | `path TemplateAssemble · 3.8 s · re-diagnosed clean` | `path TemplateAssemble · re-diagnosed clean` | `RepairReport.chosen`; the chosen `CandidateReport.verification.v0.rediagnose_clean`. The time is dropped: no engine type has it |
| 9 to 11 | after figures `1,842 obj`, `2 fonts`, `12 pages` | the finding's location: blank, `p.12`, `p.12` | `Finding.location` |
| 12 | `[iNF] header intact` | `[ERR] C9 zlib stream damaged × √ obj 14 0` | `RepairReport.findings_before`. Gives the C9 count line a reason to show |
| 14 | `RECOVERY` | `RECOVERY kept in the output, against the input` | Label only |
| 15 | `pages 214/214` bar | `text 96%` bar | Chosen `CandidateReport.verification.v1.glyph_count` |
| 16 | `text 96%` bar | `images 31/32` bar | Chosen `CandidateReport.verification.v1.images`, printed as `num/den` |
| 17 to 18 | `images 31/32` bar, `1 unplaceable → thesis_ar.images/p041-im2.jp2` | The C9 count line, wrapped at spaces | `C9Summary.repaired`, `.exact`, `.accepted`, as `c9_line` in `src/ui/view.rs` builds it |
| 23 | `F7 unknown bundled Noto Sans partial bold→reg` | `F7 unknown bundled Noto Sans (picked)` | `SubstituteChoice.label`, and the slot's `InteractionRecord.source` is `User` |
| 8, queue box | `invoice_scan.pdf partial · 88% salvaged` | `invoice_scan.pdf partial · C9` | The class code of the first `RepairReport.partial_reasons` entry, the text before its first `:`. The engine writes each entry as `PassReport.class.code()`, `: `, then the pass's reason. No pass emits a percentage. The file menu covers this row's status in frame 05, so its golden does not change; frame 14 shows it |

Row 22's `Noto Naskh Arabic (picked)` stays. The family comes from the font database, looked up by `FontResolutionKind::Picked.font_id`. "(picked)" means the matching `InteractionRecord.source` is `User`.

### Why F7 is picked, not a best guess

The sample has to obey the engine's rules. `src/pdf/repair/fonts.rs` marks a `UseBest` reply `Partial`, and a substitute for an unreproducible font too. A best-guess F7 would make thesis_ar `~ partial`, not `√ repaired`. So in the sample the user picked both fonts: frame 05 shows `(picked)` on F3 and F7, and frame 10's ANSWERS shows `you` for both.

### Partial results

When `RepairRun.status` is `OutcomeStatus::Partial`:

- the box note reads `~ partial` in `Y`;
- a FONTS row whose slot was answered `UseBest` or `Skip` ends in `partial` in `Y`;
- the hint line, row 35, shows the first of `RepairReport.partial_reasons` while the file is selected, with `+N more` when there are several.

The reasons come from `PassOutcome::Partial(String)` through `RepairReport.partial_reasons`.

The queue's short form prints `partial · <class>`, for example `partial · C9`, because a real reason does not fit 24 cells. C9 gives `C9: 31 0 obj: unrecoverable stream`. The full first reason goes on the hint line when the file is selected. The long form, in the batch view's queue, prints the reason cut at the column with `…`.

### Recovery rules

- The two bars are V1 retention ratios of the chosen candidate. They compare the output with the input's text baseline.
- With `Verification.baseline == BaselineKind::None`, both rows say `no baseline` in `D` and draw no bar.
- With `CarveProxy`, the rows draw as normal and the header note reads `kept, against a carve estimate`.
- A zero denominator prints `none in the input` and draws no bar.
- The C9 line shows only when `C9Summary.streams_damaged > 0`. Otherwise rows 17 and 18 stay blank.
- The C9 line fits two rows of 55 cells up to four-digit counts. Longer counts are cut with `…`, as `wrap` does today.

The extracted-images line left frame 05. It now lives on the evidence page, frame 14.

### With a custody record

Row 26 is blank in frame 05. When the selected run has a custody record, row 26 holds `[v] evidence`, so the evidence page can be found without help. The file menu gains an `Evidence` item too. Row 25 is full, which is why the key goes on its own row.

### States

| State | What shows |
| --- | --- |
| Clean file, no output written | Rows 4 to 6 say `clean · nothing written`; no bars; no C9 line |
| Partial | See "Partial results" above |
| Failed | The reason takes the progress box's bottom edge, as built |
| Encrypted | Findings show `[ERR] the file is encrypted`; everything else blank |

## Frame 10: history (`H`)

### Purpose

Find a file the cat has seen, see every run of it, and get back to what each run wrote. The store is `JsonStore` in `src/library.rs`.

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `history` |
| FiLES box | x 1, y 2, 58×20 | Filter line, column heads, 14 file rows, totals |
| Cat | x 1, y 22, scale 0.465, idle squint | Judging your history |
| Cat's note | x 32, rows 24 to 32 | Where history lives, what forgetting does |
| FiLE box | x 60, y 2, 51×31 | The selected file, its runs, the selected run |
| Hotkeys | row 36 | `↑↓`, `tab`, `/`, `o`, `f`, `e`, `x`, `esc`. `v` joins when the selected run has a custody record |
| Status bar | row 37 | `node 1 │ history │ offline` |

FiLES box rows:

| Row | Holds |
| --- | --- |
| 3 | `[/] filter by name or path` |
| 4 | Column heads at x 6, 31, 37, 49: file, runs, last seen, status |
| 5 to 18 | One file per row: focus `►` x 2, glyph x 4, name x 6 cut at 24, runs right-aligned to x 34, date x 37, status word x 49 |
| 19 | Separator |
| 20 | `57 files · 91 runs`, and `↓ 43 more` from x 46 when the list scrolls |

FiLE box rows:

| Row | Holds |
| --- | --- |
| 3 to 4 | `sha256` in groups of 8 hex digits, two rows |
| 5 | `path`, or `no durable path (kitty drop)` when there is none |
| 6 | `size · first fed` |
| 8 | `RUNS newest first`, heads `path` at x 81 and `findings` at x 98 |
| 9 to 11 | One run per row: `►` x 62, start time x 64, toolpath x 81, `before → after` x 98 |
| 13 | `RUN <time> · engine <version>` |
| 14 to 16 | `out`, `placed`, `config` |
| 18 to 22 | Findings with before and after, as the result view draws them |
| 24 to 26 | `ANSWERS`: who decided each question |
| 28 | Keys for the selected run |

### Keys

| Key | Does |
| --- | --- |
| `↑↓` | Move in the focused list |
| `tab` | Move focus between files and runs. The hotkey row says where it goes |
| `/` | Focus the filter. Typing then follows "Typing goes to the text"; `enter` keeps the filter, `esc` clears it |
| `o` | Open the selected run's output |
| `f` | Reveal the output's folder |
| `e` | Export Markdown. Needs the input file at its recorded path; otherwise the hint says why not |
| `v` | Open the selected run's evidence page, when it has a custody record |
| `x` | Forget the file and its runs, after the confirmation below |
| `esc` | Back to where `H` was pressed |
| a drop | Taken; history stays open, per "A drop works on every screen". Frame 10b's `Drop a pdf on the cat` works here too |

### Forget confirmation

`x` opens a small modal over the FiLE box. It is not drawn yet.

| Part | Content |
| --- | --- |
| Title | `FORGET` |
| Text | `thesis_ar.pdf and its 3 runs leave the history. The files it wrote stay on disk.` |
| Buttons | `[y] forget` and `[n] keep`, with `keep` selected |

| Key | Does |
| --- | --- |
| `y` | Forgets, through `HistoryStore::delete_file(sha256)` |
| `n`, `esc`, `enter` | Keeps. `enter` keeps because `keep` is selected, so a stray `enter` loses nothing |
| a drop | Taken, and the confirmation stays open. A drop never answers it |
| Anything else | Nothing |

### Data sources

| Shown | Source |
| --- | --- |
| File rows | `HistoryStore::list_files(filter)` → `Vec<FileSummary>`, newest first |
| Glyph and status word | `FileSummary.status` (`RecentStatus`), through `RecentStatus::glyph` and `::role` |
| Runs count | `FileSummary.runs` |
| Last seen | `FileSummary.last_at`, unix seconds, as a local date |
| Totals | `HistorySummary.files`, `HistorySummary.runs` |
| sha256 | `FileSummary.sha256` |
| path, size, first fed | `FileSummary.path`, `.size`, `.added_at` |
| Run rows | `HistoryStore::runs_for(sha256)` → `Vec<RunRecord>`, shown newest first |
| Run time | `RunRecord.started_at` |
| Toolpath | `RunRecord.report.chosen`; `none` when `None` |
| Findings before → after | `report.findings_before.len()` → `report.findings_after.len()` |
| engine | `report.engine_version` |
| out | `RunRecord.output_path`; `nothing written` when `None` |
| placed | `RunRecord.placed`: `Atomic` → `atomic`, `Linked` → `linked`, `ClaimedThenRenamed` → `claimed, then renamed` |
| config | `RunRecord.config_path` |
| Findings rows | `report.findings_before` against `report.findings_after`, matched by class as `finding_rows` does |
| ANSWERS | `report.interactions`: `request.slot`, `request.page`, the reply font's family, and `source` |
| Custody record of a run | Planned: `RunRecord` gains the record's file names, as schema 4.5 says |

ANSWERS maps `InteractionSource`, which G-03 extended (D-141):

| `source` | Prints |
| --- | --- |
| `User` | `you` |
| `UseBest` | `best guess` |
| `Policy` | `setting` |
| `Batched` | `carried over` |

### States

| State | What shows |
| --- | --- |
| Empty | Frame 10b: "Nothing here yet." on the left, a pointer on the right, hotkeys cut to `[B] browse for pdfs` and `[esc] back` |
| Many files | 14 rows, `↓ N more`, the selection stays visible on scroll |
| Many runs | Three rows, then `… N more`; the list scrolls with focus |
| Filter matches nothing | `no file matches "<filter>"` in the list |
| No store | The FiLES box says `history is off: <reason>` in `R`. Reasons come from the startup log lines in `src/ui/app.rs`: no home folder, or the store failed to open |
| A record a newer build wrote | Not listed. The FiLES footer adds `1 skipped: written by a newer build` in `Y`. From `HistoryStore::take_warnings` |
| Input file moved or gone | `path` row shows the old path with `(missing)` in `Y`; `e` explains |
| Run has a custody record | The RUN block's `config` row is followed by `custody <record name>`, and the key row adds `[v] evidence` |

The samples on frames 10 and 14 are independent moments. Frame 10's session has custody mode off, so its runs have no record.

## Frame 11: setup (`S`)

### Purpose

Change the settings in `config.toml` without opening an editor, and see which ones change the output.

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `setup · config.toml` |
| SETTiNGS box | x 1, y 2, 64×33 | Five sections, one setting per row, rows 3 to 33. The list is one row taller than the box, so it scrolls: `↓ 1 more` sits on the bottom edge |
| Detail box | x 66, y 2, 45×20, modal gradient | What the selected setting does |
| Cat | x 73, y 22, scale 0.465 | Idle squint |
| Save line | row 35 | `saves to <config path> · applies to the next batch` |
| Hotkeys | row 36 | Below |

Each setting row: `►` at x 3 when selected, `◆` at x 5 when the setting is in Part B, label at x 7 cut at 24, value at x 31. The box title's note reads `◆ in the reproducible record`.

### Settings shown

| Section | Label | Config field | ◆ |
| --- | --- | --- | --- |
| GENERAL | output folder | `General.output_dir`: `Beside` shows `beside each input` | |
| | default page size | `General.default_page_size` | ◆ |
| REPAIR | auto-accept a font at | `Repair.auto_accept_confidence`, as a percentage | ◆ |
| | font candidates | `Repair.max_font_candidates` | ◆ |
| | save unplaceable images | `Repair.extract_unplaceable_images` | ◆ |
| | derived image views | Planned: `Repair.derived_image_views`, a new key, default off (schema 4.5, Q6) | ◆ |
| | Type3 glyph images | Planned: `Repair.extract_type3_glyph_images`, default off (schema 6.4, Q10) | ◆ |
| | salvage work | `Repair.salvage_work` | ◆ |
| | deep salvage work | `Repair.salvage_deep_work` | ◆ |
| | deep salvage pool | `Repair.salvage_deep_pool` | ◆ |
| | max search stream | `Repair.max_search_stream` | ◆ |
| FONTS | font source | `Fonts.source` | ◆ |
| | ask on unknown fonts | `Fonts.prompt_unresolved` | ◆ |
| | unreproducible fonts | `Fonts.unreproducible` | ◆ |
| CUSTODY | custody mode | `Custody.enabled` | |
| | hashes | `Custody.hashes`. SHA-256 is always on and prints `sha256 always`; SHA-1 and MD5 are toggles | |
| | ask case details | `Custody.ask_case_details` | |
| | record host name | Planned: `Custody.record_host_name`, schema Q3, default on | |
| | custody log | `Custody.log_path`; `None` shows `data folder/custody.log` | |
| LOOK | theme | `Ui.theme` | |
| | layout | `Ui.layout` | |
| | mouse | `Ui.mouse` | |
| | grow when needed | `Ui.request_resize` | |

### What ◆ means

`◆` marks a setting recorded in Part B, the custody record's `reproducible.settings` (schema 3.1, 4.3). A `◆` setting changes Part B, so a re-run must use the same value to get the same bytes.

| ◆ setting | Recorded today | Recorded once planned work lands |
| --- | --- | --- |
| The other ten, all fields of `SettingsSnapshot` | `RepairReport.settings`, every run | Part B too |
| `ask on unknown fonts` | Not yet | Part B: it decides whether an answer is `user` or `policy` |
| `derived image views` | Not yet | Part B: it adds view files and their hashes |
| `Type3 glyph images` | Not yet | Part B, and `SettingsSnapshot` gains it (schema 4.5) |

`SettingsSnapshot.passes` is the one Part B setting with no row: `config.rs` always sets it to `None`, all classes, so no config key exists to edit.

No custody setting has `◆`. Schema 3.1 keeps custody settings out of Part B, because they do not change the repair.

### Image settings, and what custody mode does

`save unplaceable images` maps to `Repair.extract_unplaceable_images`. It covers only images the repair could not put back on a page. The label says so, because "extract images" read like a master switch.

| Situation | Image files written |
| --- | --- |
| Custody off, repair only | Unplaceable images, if this setting is on |
| Custody off, Markdown export | Every image, linked from the `.md` |
| Custody on, any job | Every image, whatever this setting says (schema 6.1, D-114) |

The detail box for this row says the last line: `custody mode writes every image to <hash8>.images/ anyway`.

### Rows not shown yet

The schema's `[custody] case_jsonld` is not shown yet. It waits on schema Q5. When it ships, the list is two rows taller than the box and scrolls by two.

### Custody is off, and says so

- The CUSTODY heading carries `mode OFF` in a grey pill: base text on the dim role. Grey, because off is the default and not a fault.
- With custody off, the custody rows draw in `D`. They stay editable.
- With custody on, the pill reads `ON` in `Y`, and every screen's status bar shows `custody ON` in `Y`.
- Frame 11 shows custody mode selected, so the detail box explains it: the case details asked once per batch, the hashing, the three record files for every file and the log entries.

### The hashes row

`sha256 always · [x] sha1 [x] md5`. SHA-256 cannot be switched off, because the history store, the log chain and Part B all key on it (schema 5.2). A config with `hashes = ["md5"]` loads as SHA-256 plus MD5.

On this row, `←→` moves between `sha1` and `md5`, and `space` toggles the one under the cursor. The cursor never lands on `sha256`. The detail box calls SHA-1 and MD5 legacy and says what they are for: older case systems that index exhibits by them.

### Keys

| Key | Does |
| --- | --- |
| `↑↓` | Move |
| `space` | Toggle a `[x]` setting |
| `←→` | Step a `‹ choice ›`; on the hashes row, move between `sha1` and `md5` |
| `enter` | Edit a number or a path in place. Typing then follows "Typing goes to the text" |
| `s` | Save to `config.toml`. Setup's own key: `S` opens setup elsewhere, and here it saves too |
| `r` | Put the selected setting back to its default. Setup's own key |
| `esc` | Back. With unsaved changes it asks: `[s] save`, `[d] discard`, `[esc] stay` |
| a drop | Taken; setup stays open, and an open edit keeps focus. With custody on, the prompt waits until setup closes |

### Data sources

| Shown | Source |
| --- | --- |
| Every value | The loaded `Config` |
| Defaults for `r` | `Config::default()` and each section's `Default` |
| Config path | The `PathBuf` that `Config::load` returns |
| Saving | `Config::to_toml`, written temp file, fsync, rename |
| ◆ | Membership in `SettingsSnapshot`, `src/engine/report.rs` |

### States

| State | What shows |
| --- | --- |
| Unsaved changes | Save line reads `● N unsaved changes` in `Y` |
| Invalid number | The value turns `R` with the allowed range on the save line; `s` refuses |
| `font source = system` | The value shows `system (not built yet, acts as bundled)`, from `Warning::SystemFontsNotBuilt` |
| Config unreadable at start | Save line in `R`: `config.toml could not be read; saving replaces it`, from `Warning::Unreadable` |
| Unknown keys in the file | Save line warns they will be dropped on save, from `Warning::UnknownKey` |
| Batch running | Save line adds `the running batch keeps its settings` |

## Frame 12: help (`?`)

### Purpose

One screen that answers: what keys do what, what is the cat doing, where did my files go, and what does this app promise.

### Layout

A modal box at x 4, y 2, 104×33 over the idle screen dimmed by 0.8. A divider at x 56 splits it.

| Column | Sections |
| --- | --- |
| Left, from x 6 | KEYS: anywhere, while a batch runs, on a finished file. THE CAT: what it does and what each pose means |
| Right, from x 58 | WHERE THiNGS GO, WHAT iT PROMiSES, WHAT iT WON'T DO, CUSTODY MODE |

Row 18 lists `[v] evidence` with `when the run has a custody record`. Row 9 lists `<hash8>.images/`, as `src/place.rs` names it. Row 10 lists `<output>.custody.*`, matching the schema's names. Rows 28 to 31 say the prompt asks once per batch, every input and output is hashed, and `.custody.json`, `.txt` and `.sha256` are written for every file, each with an entry in `custody.log`.

### Keys

`esc` or `?` closes it. No other key does anything. A drop is taken and help stays open, per "A drop works on every screen".

### Data sources

Almost everything is fixed copy in `strings.rs`. The live parts:

| Shown | Source |
| --- | --- |
| Version | `env!("CARGO_PKG_VERSION")` |
| History path | `AppDirs.data_dir` joined with `history/` |
| Config path | The path from `Config::load` |
| `or in [general] output_dir` | Shown as the actual folder when `General.output_dir` is `Dir` |

### What it promises, and why each line is true

| Line | Backed by |
| --- | --- |
| Inputs are never opened for writing | D-044, `BatchInputs` in `src/place.rs` |
| Same file, version, settings and answers: the same output, byte for byte | DA:423-425, D-041, the `clippy.toml` time and libm bans. The answers are part of the contract: `RepairReport.interactions`, the `RunRecord` doc comment in `src/library.rs`, and Part B in the evidence schema. The same file with a different font pick gives different bytes |
| No network code | D-050, `tools/purity-gate.py --network` |
| Every fix is listed in the report | `RepairReport.passes[].actions` |
| No cat in any file it writes | `src/engine/artefact_tests.rs` |

## Frame 13: custody prompt

### Purpose

When custody mode is on and files are dropped, get the case details once for the whole batch, before any of its jobs start (DA M6b, D-020, schema 4.4 `run.case`).

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `new batch · custody mode ON` |
| Cat | x 4, y 8, scale 0.68, needs-you pose | Wide eyes on the viewer |
| Cat's line | x 8, y 28 | `case details first. then food.` |
| Reminder | x 5, y 31 | `custody mode is on in setup`, plain `D`. No `[S]`: a field always has focus here, so `S` is text |
| CASE DETAiLS box | x 48, y 2, 63×33, modal gradient | Below |
| Hint | row 35 | `the cat asks once per batch` |
| Status bar | row 37 | `custody ON │ case details` |

Box rows:

| Row | Holds |
| --- | --- |
| 4 to 5 | `3 pdfs are waiting in custody mode`, and that the cat asks once, before any of them runs |
| 7 to 8 | `case reference` and its field: brackets at x 52 and 107, text from x 54 |
| 9 to 10 | `examiner` and its field |
| 11 to 12 | `submission reference` and its field |
| 13 to 14 | `authority / notes` and its field |
| 16 to 18 | Any field may stay blank and is recorded as blank; 200 characters each; the values go only into the custody records, never into `custody.log`, the PDF or the Markdown |
| 20 to 24 | THiS BATCH: file names, the hash algorithms, `records <output>.custody.json, .txt and .sha256 per file`, log entries. No hash values: in custody mode a path input is not opened before its job runs |
| 26 to 28 | The destination warning in `Y`, when it applies: `! outputs and records will be written into the evidence folder <folder>`, then `set an output folder in setup to leave it untouched` in plain `D`. Blank otherwise |
| 30 | `‹ Start batch ›` in the modal pill at x 51, and `[ Cancel batch ]` at x 70 |
| 32 to 33 | `Cancel repairs nothing and writes no record. The files were listed, never opened: custody.log notes only the count.` See "What the app reads before the prompt" |
| 36 | `[tab] next field  [enter] start batch  [esc] cancel batch` |

One control has focus at a time, and only it has the `►`:

| Control | Unfocused | Focused |
| --- | --- | --- |
| Text field | Brackets, text in `w` | `►` in `Y` at x 50, lightbar, blinking cursor |
| `Start batch`, the default button | `‹ Start batch ›` in the pill | `►` in `Y` at x 49 and the pill in the lightbar |
| `Cancel batch` | `[ Cancel batch ]` | `►` in `Y` at x 68 and the brackets on the lightbar |

`‹ ›` marks the default button, the one `enter` in a field presses. It survives `NO_COLOR`. Frame 13 has focus on `case reference`, so neither button has a `►`. It leaves `authority / notes` blank, to show an empty field.

### Keys

| Key | Does |
| --- | --- |
| `tab`, `shift-tab` | Next or previous control: the four fields, then `Start batch`, then `Cancel batch`, then back to the first field |
| typing | Edits the focused field, per "Typing goes to the text". On a button, letters do nothing |
| `enter` in a field | Presses the default button, `Start batch` |
| `enter` or `space` on `Start batch` | Starts the batch with what is typed |
| `enter` or `space` on `Cancel batch` | Cancels the batch, as `esc` does. It never starts it |
| `esc` | Cancels the batch from any control: its files leave, nothing is repaired, no record is written, and one `batch_cancelled` entry holds the file count only (schema 2.3, 8.3) |
| paste | Text, unless every item is a `.pdf` path: then it is a drop and joins the pending batch. A pasted newline does not start the batch |

The prompt cannot be skipped while `ask_case_details` is true. The user must answer it, but every field may stay blank (DA:433). Setting `ask_case_details` to false is the only way to never see it; the records then have `run.case.prompt_shown = false`.

### Batches and what else is on screen

A custody batch has no data model today: `BatchState` in `src/jobs.rs` is one growing `Vec<QueueEntry>` with no batch id. This design adds one, marked Planned.

| Step | What happens |
| --- | --- |
| 1 | Files are dropped, or added with `+` through the browse picker, while no prompt is open. The gate admits path inputs on name and `fs::metadata` only, per "What the app reads before the prompt". The chomp plays as usual |
| 2 | A new custody batch opens. Planned: `AppState.pending_batch` holds the files. They are not enqueued yet, so the runner cannot start them |
| 3 | The prompt opens at once on the batch view. On history or setup, or with a modal open, it waits until the user is back on the batch view, per "A drop works on every screen" |
| 4 | Files dropped while the prompt is open join the pending batch. The list updates and the question is not asked again. `+` cannot be pressed: it would be text |
| 5 | `Start batch`: one `batch_opened` log entry, then the files are enqueued. Planned: each gets `QueueEntry.custody_batch`, holding the batch's index in `AppState.custody_batches` and its `batch_opened` log sequence number |
| 6 | `Cancel batch`: the pending files leave unrepaired, and one `batch_cancelled` entry logs how many there were. Path inputs were never opened. Kitty inputs were received whole at the drop; their bytes are discarded, and the entry counts them too |
| 7 | Files dropped after step 5 open a new batch, back at step 2. The new prompt is pre-filled with the last batch's four values, so `enter` alone carries them over |

While the prompt is open:

- The runner keeps working on the entries of earlier batches.
- A parked file of an earlier batch stays parked. Its `‼` question waits; `i` reaches it after the prompt closes.
- A new font question from an earlier batch parks as usual. It does not open over the prompt.
- The widget follows the order under "The widget": case details first, then parked files.

A new prompt after step 5 is the price of "asked once per batch": a later drop may belong to a different case. Pre-filling keeps it to one key when it does not. Open question 2 asks whether the user wants this.

### Data sources

| Shown | Source |
| --- | --- |
| Number of files, names | Planned: `AppState.pending_batch`, the files dropped or added since the prompt opened |
| Hashes list | `Custody.hashes`, with SHA-256 always first; MD5 and SHA-1 marked legacy |
| Record names | Schema 2.2: `<output>.custody.json`, `.txt`, `.sha256` |
| Log entries named | Schema 8.3: `batch_opened`, one `record` per file, `batch_closed` |
| Typed values | UI state only. Planned: held per batch in `AppState.custody_batches`, handed to the custody writer, never to the engine (D-020). They become `run.case.case_reference`, `.examiner`, `.submission_reference`, `.notes` |
| Pre-filled values | The previous batch's typed values |
| Destination warning | `General.output_dir`, against each pending file's path. Shown when at least one file would be written into its own folder. Recorded as `run.destination.warned_at_prompt` (schema 2.4, 4.4) |

### What the app reads before the prompt

Today every dropped or picked file passes `gate()` in `src/ui/input/gate.rs` before the chomp. `gate()` stats the file, then opens it and reads its first KiB for the `%PDF-` sniff. Kitty drops go further: `take_drop` in `src/ui/app.rs` reads each admitted file whole into memory before the drop completes (D-039, `JobInput::Dropped`, which keeps the path and the bytes). So as built, every file has been opened before a prompt could show. That read also comes before `started_utc` and `stat_before`, so `stat_before.accessed` can show it.

Custody mode changes the gate. This is a build item.

| Check | Custody off, as today | Custody on |
| --- | --- | --- |
| Name ends in `.pdf`, not UNC | At the drop | At the drop |
| `fs::metadata`: a regular file, at most 4 GiB, the size warning | At the drop | At the drop. It reads no content |
| Opens and reads | At the drop: unreadable is refused | At job start, after `stat_before`. Unreadable fails the job, and its record says so |
| `%PDF-` sniff, `drop_sniff` | At the drop | At job start, from the same read |
| Kitty drop read whole (D-039, `JobInput::Dropped`) | At the drop | At the drop. A file promise cannot wait, so this stays |

So in custody mode:

- A path input is first opened when its job runs, after `Start batch`. Its `stat_before.accessed` is untouched by PDFPundit.
- A kitty input was received in full at the drop. Its record has `path_kind = file_promise` and no stat (schema 4.4). The prompt says so on rows 32 and 33 in place of the cancel line: `kitty sent these files whole at the drop. Cancel repairs nothing and writes no record; custody.log counts them.`
- Hashing at the prompt stays out. It would make a cancel an unrecorded read of evidence the examiner declined to process.

The schema owner needs to know two things. Schema 2.3 says the prompt never read the files, which is true only after this gate change. `batch_cancelled` needs a second count, `received`, for kitty inputs whose bytes were held and discarded. See "Flags for the schema owner".

### The submission reference

One value covers the whole batch, so it cannot name one item. Each item's identifier is its input SHA-256. The prompt labels the field `submission reference`, for the request or submission the batch belongs to. The evidence page prints it with `applies to all N files of this batch`, as the `.txt` does (schema 4.4, finding 4).

### Frame 13b: the widget

The widget never shows the form. It shows the needs-you pose, `‼` blinking at the top right, `‼ case details · zoom me` on the status line, and `custody ‼ 1` on the status bar: one open question. Zooming the tile opens frame 13.

### States

| State | What shows |
| --- | --- |
| One file | `1 pdf is waiting in custody mode` |
| Many files | Names until the line is full, then `+N more` |
| Field longer than the box | The field scrolls. At 200 characters it takes no more, and the hint says `200 characters at most` |
| Control characters typed or pasted | Never enter the field (schema 4.1) |
| Some or all files came by kitty drop | Rows 32 and 33 say `kitty sent N files whole at the drop` before the cancel line, and the `batch_cancelled` entry counts them as `received` |
| A path input is unreadable | Not caught at the drop in custody mode. Its job fails at start with `unreadable`, and its record says so |
| Outputs would go into the input's own folder | Rows 26 to 28, in `Y`, as frame 13 draws them. Source: `General.output_dir == OutputDir::Beside` and the file has a durable path, or `OutputDir::Dir` names the input's own folder. `Start batch` still works; the warning is recorded as `run.destination.warned_at_prompt = true` (schema 2.4) |
| Several input folders affected | Row 27 says `N evidence folders` instead of one path |
| Output folder set elsewhere | Rows 26 to 28 blank, `warned_at_prompt = false` |
| Custody log cannot be opened or locked | The box shows the reason in `R` on rows 26 to 28, replacing the destination warning, and `Start batch` is refused (schema 2.4) |
| Log chain found broken | The box says `custody.log was damaged: a new log continues it` in `Y`, naming the new file (schema 8.4) |

## Frame 14: evidence page (`v`)

### Purpose

Show what a custody record holds for a finished file: the case details, the hashes before and after, the files written beside the output, the extracted images, the log entry. It is a page of the result panel, so the queue and the cat stay.

### Layout

Frame 05's layout, with the result panel replaced by the EViDENCE box at x 52, y 2, 59×30. `tab` or `v` flips back to the RESULT page.

| Row | Holds |
| --- | --- |
| 3 to 5 | `case`, `examiner`, `submission`. Labels 11 cells wide. A blank field prints `not provided` in `D`, as the `.txt` does. The phrase comes from the custody template module and stays out of `ALL` |
| 6 | Under the submission value: `applies to all N files of this batch` |
| 7 | `notes` |
| 9 | `iNPUT`, the file name, and `sha1, md5: legacy` from x 92 |
| 10 to 13 | sha256 on two rows, sha1, md5, in groups of 8 hex digits |
| 14 | `after √ unchanged · hashed again after the run` |
| 16 | `OUTPUT`, its name, and the legacy note |
| 17 to 20 | The same hash rows |
| 21 | `placed √ same bytes as the engine made · atomic` |
| 23 | `WRiTTEN BESiDE iT`, then `· in the input's folder` in `Y` when `run.destination.same_folder_as_input` is true |
| 24 to 26 | `<output>.custody.json`, `.txt`, `.sha256` |
| 27 | `<hash8>.images/`, the image count, `images.json`, `SHA256SUMS`. Frame 14 counts 32: frame 05's 31 placed images and the unplaceable one, because custody mode writes every image (schema 6.1) |
| 28 | `custody.log`, this record's entry and the batch's `batch_opened` entry |
| 30 | `[r] verify  [f] folder  [c] copy hashes  [tab] result` |
| 36 | Every live key: `[↑↓] select  [tab] result  [r] verify  [f] folder  [c] copy hashes  [esc] back  [q] quit` |

The queue on the left is frame 05's, except `invoice_scan.pdf`, which reads `partial · C9`. Frame 05 itself moves to that row on `design/ux-d117`.

Hashes print in groups of 8 so an examiner can compare them by eye against another tool's output. SHA-256 always prints. SHA-1 and MD5 print only when the record has them; otherwise their rows close up.

With `[custody] case_jsonld = true`, a row `<output>.case.jsonld  CASE/UCO view` follows the `.sha256` row, and the rows below move down one. Frame 14 has it off, the default.

### Keys

| Key | Does |
| --- | --- |
| `v` | From the result page, open this page. Works whenever the selected run has a custody record, whatever `Custody.enabled` says now |
| `tab`, `v` | Back to the result page |
| `r` | Open the Verify panel, schema section 9. Checks 1 to 4 are on. `[5] also re-run` is off: it re-runs the repair with the recorded settings and answers and compares `reproducible_sha256`, which takes as long as a repair. `5` toggles it, `enter` runs, `esc` closes |
| `f` | Reveal the output folder |
| `c` | Copy the hashes as plain text |
| `esc` | Back |

### What Verify does

Verify follows schema section 9. It never changes the record.

| # | Check | Results |
| --- | --- | --- |
| 1 | Record files against the hashes in their log entry | `match`, `mismatch`, `missing` |
| 2 | Input file against `run.input_file.hashes_before` | `match`, `mismatch`, `unavailable` |
| 3 | Each placed file against its recorded hashes | `match`, `mismatch`, `missing`, per file |
| 4 | The log chain up to and past this entry | `intact`, `broken at seq N` |
| 5 | Only with `[5] also re-run` on: re-run and compare `reproducible_sha256` | `identical`, `different`, `different build`, `not_run` |

Each Verify writes `<output>.custody.verify-<seq>.txt` beside the record and a `verify` entry in `custody.log`. The page then shows the five results in place of rows 9 to 21, with "Log head after this record" and "Log head now", each with its entry number. The WRiTTEN rows add the verify file. The Verify panel and this result view are not drawn yet.

The re-run is a toggle, not a capital `R`, because letters ignore case: with caps lock on, `R` would start a repair-length re-run by accident. The file menu's `R` stays `Repair options…`, scoped to the open menu.

### Data sources

| Shown | Source |
| --- | --- |
| case, examiner, submission, notes | `run.case.case_reference`, `.examiner`, `.submission_reference`, `.notes`. Null prints the template's `not provided` |
| N in `applies to all N files` | `run.batch.count` |
| Input sha256 | `RepairReport.input_sha256`, also `run.input_file.hashes_before.sha256` |
| Input sha1, md5 | `run.input_file.hashes_before.sha1`, `.md5`; null closes the row |
| after | `run.input_file.unchanged`: `yes`, `no`, `not_rechecked` with `not_rechecked_reason` |
| Output name | `RepairRun.output_path`, also `run.placed[role = repaired_pdf].name` |
| Output hashes | `run.placed[role = repaired_pdf].hashes` |
| placed | `run.placed[].matches_product` and `.placement` |
| Record file names | `run.record_files.json`, `.text`, `.checksums`, `.case_jsonld` |
| Images folder | `run.images_dir.name`, `<first 8 hex of the input sha256>.images` as `src/place.rs` names it |
| Images count | `reproducible.products` with role `image`. Empty until the D-114 extractor exists |
| Log entry | `run.log.seq`, and `run.batch.log_seq` for `batch_opened` |
| Box note | `run.input_file.unchanged` |
| `· in the input's folder` after WRiTTEN BESiDE iT | `run.destination.same_folder_as_input` |
| Whether `v` works | Planned: `QueueEntry` gains the record's path when T-33 writes it; `RunRecord` gains the record's names for history |

### States

| State | What shows |
| --- | --- |
| Input changed after the run | `after × CHANGED since it was read` in `R`; the box note turns `× changed`; the status bar adds it |
| Input not re-checked | `after not re-checked · <reason>` in `Y`, for example `no durable path` for a kitty drop (D-039) |
| Placed file differs from the engine's bytes | `placed × differs from what the engine made` in `R`, with `run.placed[].note` on the hint line |
| Clean file, no output | The OUTPUT block reads `nothing written: the file was clean` |
| No images | `no images to extract` |
| Record write failed | The WRiTTEN rows read `custody record not written` in `R` with the error; the log holds a `record_write_failed` entry (schema 2.4) |
| Batch still running | Row 27 names the `batch_opened` entry, as frame 14 does |
| Batch closed | Row 27 names the `batch_closed` entry too. The progress box shows `log head after this batch`, the entry number and its SHA-256 in groups of 8, for the examiner to copy (schema 2.3 step 8). Not drawn yet |
| No custody record | `v` does nothing; the hint says `no custody record for this run` |
| Written into the input's folder | Row 23 adds `· in the input's folder` in `Y`, as frame 14 draws it: its outputs sit in `~/cases/2026-091/evidence/`, the default |
| Written to an output folder | Row 23 reads `WRiTTEN TO <folder>`, from `run.destination.path`, in `C` |
| Input had no durable path | Row 23 reads `WRiTTEN TO <folder>`; `same_folder_as_input` is null, so no folder note |

## Agreement with the evidence schema

| Point | Schema says | These screens now |
| --- | --- | --- |
| Record names | `<output>.custody.json`, after the full name of the primary file (2.2) | Frames 11, 12, 13 and 14 use `<output>.custody.*`; frame 14 shows `thesis_ar.repaired.pdf.custody.json` |
| Files | `.custody.json`, `.custody.txt`, `.custody.sha256`, optional `.case.jsonld` (2.1) | Frame 14 lists all three; the `.case.jsonld` row appears when it is on |
| SHA-256 | Always on; MD5 and SHA-1 are legacy (5.2) | Setup prints `sha256 always`; frames 13 and 14 mark the other two legacy |
| Prompt fields | Case reference, examiner, submission reference, notes (4.4, Q2) | Frame 13 has all four, so Q2's "yes" is drawn. If the user says no, rows 11 to 14 go |
| Submission reference | One value per batch; the `.txt` prints "applies to all N files of this batch"; the item's identifier is the input SHA-256 (4.4, finding 4) | Frame 13 labels it `submission reference`. Frame 14 prints `applies to all 3 files of this batch` under it |
| Input wording | `run.input_file`, "Input file (as received by PDFPundit)"; never called the original (4.4, 12.1) | Frame 14 says `iNPUT`. Setup says `beside each input`; help says `Inputs are never opened for writing` |
| Destination | `run.destination`; the prompt warns when outputs go into the input's folder, recorded as `warned_at_prompt` (2.4, 4.4) | Frame 13 rows 26 to 28 warn. Frame 14 row 23 adds `· in the input's folder` |
| Blank values | Stored as null, printed as "not provided"; the prompt should not quote it (Q13) | The prompt says `recorded as blank`. The evidence page prints `not provided`, taken from the custody template module and kept out of `ALL` |
| Log entries | `batch_opened`, `record`, `batch_closed`, `batch_cancelled`, `verify` (8.3) | Setup, help and frame 13 name them |
| Cancel | `batch_cancelled`: count only; the files are never read (2.3, 8.3) | Agrees once the custody gate change is built: path inputs are listed, never opened. Frame 13 rows 32 and 33 say so. Kitty inputs are read at the drop, so the schema needs a `received` count. The prompt shows algorithm names, never hash values |
| Part B settings | Every setting that changes Part B, including `prompt_unresolved`, `derived_image_views`, `extract_type3_glyph_images` (3.1, 4.3) | Setup marks all of them `◆`; the last two are Planned rows |
| Image files | Custody mode writes every image on any job (6.1) | Setup's `save unplaceable images` detail box says custody writes every image anyway. Frame 14 row 27 counts all 32, the unplaceable one included |
| Verify | Five checks, a `.verify-<seq>.txt` and a log entry (9) | `r` opens the Verify panel: checks 1 to 4, and check 5 behind the `[5] also re-run` toggle. Each run writes the file and the entry |
| One record per job | Every job gets a record, named after the input when nothing was output (2.1, 2.2) | Frame 13 says `per file`; setup and help say `for every file` |
| Field limit | 200 characters, no control characters (4.1) | The prompt stops at 200 and refuses control characters |
| Host name | `record_host_name` setting (Q3) | A setup row, Planned |
| Log head | Shown when the batch closes (2.3 step 8, 8.5) | Specified for the progress box; not drawn yet |

## Accessibility

### NO_COLOR

Today the app does not read `NO_COLOR`: `color_caps` in `src/ui/term.rs` picks only truecolor, 256 or 16. The build list adds it. The screens above stay usable without colour:

- Selection keeps the `►` marker; the lightbar becomes reverse video. The other list's selection keeps its `►` without reverse video.
- Every state has a word or glyph next to it.
- Box borders, separators and the grid stay.
- Blinking is limited to three things, never a whole screen: one text cursor, the `‼` marker, and the widget's needs-you status line (frames 07 needs-you and 13b). That line is one row of a 32×16 tile, and the tile has nowhere else to say it needs the user.

The cat is the hard part. It is drawn in half-block colour pixels. A no-colour cat needs its own rendering, which is open question 4.

### 16 colours

- Every role in the tables above maps to one ANSI slot through the theme's `ansi16`, as `Theme::downgrade` does today.
- Gradients collapse to their nearest slot, so box borders become one colour. That loses nothing these screens rely on.
- The bars on frame 05 keep their numbers beside them, so a flat bar still reads.
- Hash rows use body text only, so 16 colours change nothing there.
- The two focus markers differ in colour (`Y` against `D`) and in the lightbar, so they stay apart in 16 colours.

### Reading order

Each screen reads top to bottom, left to right, in the order the user acts: list, then detail, then keys. The status bar always says which screen is open.

## What the build needs

| Item | Where | For |
| --- | --- | --- |
| History screen, store queries, local date formatting | New screen; `src/ui/app/clock.rs` for the offset | Frame 10 |
| Forget confirmation modal | New modal | Frame 10 |
| Setup screen and its editor, save through `Config::to_toml` | New screen | Frame 11 |
| `Custody.record_host_name`, default on; `hashes` always loads with SHA-256 | `src/config.rs` | Frame 11 |
| `Repair.derived_image_views` and `Repair.extract_type3_glyph_images`, both default off; the latter into `SettingsSnapshot` | `src/config.rs`, `src/engine/report.rs` | Frame 11 |
| Setup list scrolling, with `↓ N more` on the box's bottom edge | New screen | Frame 11 |
| Help screen | New screen | Frame 12 |
| Text-field focus checked before any hotkey | Key dispatcher, `src/ui/app.rs` | Frames 10, 11, 13 |
| Custody-mode gate: name and `fs::metadata` only; the open, the read check and the `%PDF-` sniff move to job start, after `stat_before` | `src/ui/input/gate.rs`, the job start in `src/jobs.rs` | Frame 13 rows 32 and 33 |
| `batch_cancelled` counts kitty inputs received and discarded | `src/custody.rs`, schema 8.3 | Frame 13 kitty state |
| A paste whose every item is a `.pdf` path is a drop, even with a field focused | `src/ui/app.rs`, `pasted` | Frames 11 and 13 |
| A drop on history, setup, help or a modal is taken and the screen stays; the custody prompt waits for the batch view | `src/ui/app.rs` | Frames 10 to 13 |
| Prompt buttons: one focus, `‹ ›` on the default, `enter` on a focused button presses it | New screen | Frame 13 |
| Verify panel with the `[5] also re-run` toggle | New panel | Frame 14 |
| Pending custody batch, held before enqueue | Planned `AppState.pending_batch` | Frame 13 |
| Custody batch id per entry | Planned `QueueEntry.custody_batch`, indexing `AppState.custody_batches` (case values, `batch_opened` seq, state) | Frames 13, 14 |
| Case details prompt, and the order against open modals and parked files | New screen; the enqueue in `src/ui/app.rs` | Frame 13 |
| Destination check before the prompt: does any pending file's output folder equal its input folder | `src/place.rs` naming plus `General.output_dir` | Frame 13 rows 26 to 28, `run.destination` |
| Widget `‼ N` counting prompts and parked files, case details named first | `src/ui/layout/widget.rs` | Frame 13b |
| Custody record, hashes, log, Verify | T-33, `src/custody.rs`, per the evidence schema | Frame 14 |
| Record path on the queue entry and in `RunRecord` | `src/jobs.rs`, `src/library.rs` | `v` on frames 05, 10, 14 |
| `[v] evidence` on row 26 of the result page and in the file menu | `src/ui/layout/full.rs` | Frame 05 with a record |
| Image extraction | D-114 | Frame 14 images count |
| Artefact scan of every custody file: `.custody.json`, `.txt`, `.sha256`, `.case.jsonld`, `.verify-<seq>.txt`, `custody.log`, `images.json`, `SHA256SUMS` | `src/engine/artefact_tests.rs` | No cat in artefacts |
| UI phrases into `ALL` as drawn; lone labels left out | `src/ui/strings.rs` | Same |
| `NO_COLOR`: a non-empty value is honoured before colour detection; the lightbar becomes reverse video, roles become attributes | `color_caps` in `src/ui/term.rs` | Accessibility |
| Carry `CarveSummary.objects` into `QueueEntry` | `src/jobs.rs`, `src/ui/view.rs` | Frame 03 row 14 (`design/ux-d117`) |
| Per-entry ring of the last four phase and log events, kept across the entry's jobs, cleared on re-queue | `src/ui/app.rs`, `ViewModel` | Frame 03 log (`design/ux-d117`) |
| Phase index and total in the view model | `ViewModel.current` | Frame 03 rows 16 and 17 (`design/ux-d117`) |
| Short labels for info findings | `strings.rs` | Frame 03 rows 22 and 23 (`design/ux-d117`) |
| Output name and folder, chosen toolpath, rediagnose flag, V1 ratios, baseline kind, partial reasons | `ViewModel` from `RepairRun` | Frame 05 (`design/ux-d117`) |
| Queue short form `partial · <class>`; fixture reason `31 0 obj: unrecoverable stream`, status `C9: 31 0 obj: unrecoverable stream`, in place of `88% salvaged` | `src/ui/view.rs` `RowStatus`, `src/ui/state.rs` `mockup_result`, the view tests | Frames 05 and 14 (`design/ux-d117`) |
| Picked font's family | Font database lookup by `font_id` | Frame 05 rows 22 and 23 (`design/ux-d117`) |

## Flags for the schema owner

| # | Schema text | Problem | Proposed fix |
| --- | --- | --- | --- |
| 1 | 2.3: "The prompt never read them" | False for the app as built: `gate()` opens every file and reads its first KiB before the prompt | True for path inputs once the custody-mode gate ships. Say so, and name kitty drops as the exception |
| 2 | 2.3 order: `started_utc`, then `stat_before` | Today the gate's read comes earlier, so `stat_before.accessed` can show PDFPundit's own read | With the custody gate, the first open is after `stat_before`. The schema could say the stat is taken before any read by PDFPundit |
| 3 | 8.3 `batch_cancelled`: `files` only | Kitty inputs were received whole and then discarded | Add `received`: how many of `files` were held in memory and discarded |

## Open questions for the user

1. **Prompt strictness.** As designed, the prompt must be answered whenever `ask_case_details` is true, and every field may be blank, as DA:433 says. Do you also want a `require_case_details` setting that refuses an empty case reference?
2. **A drop after a custody batch has started.** It opens a new prompt, pre-filled with the last batch's answers, so `enter` alone carries them over. The other choice is to join the running batch silently, which risks recording the wrong case. Is the pre-filled prompt right?
3. **Times in history.** Frame 10 shows local dates and times, matching the status bar clock. Custody files use UTC. Is local right for the history screen? On Windows the clock code reads only the current time, so past dates need new code either way.
4. **NO_COLOR and the cat.** The cat is not optional, and it is drawn in colour pixels. Under `NO_COLOR`, should the cat become shade-block art (`░▒▓█`), or should the app keep colour for the cat only?
5. **Setup rewrites comments.** `Config::to_toml` writes the whole file, so saving from setup drops comments and unknown keys in a hand-edited `config.toml`. Is that acceptable with a warning, or should setup refuse to save over a file it did not write?
6. **Frame 05 recovery wording.** RECOVERY now shows V1 retention, labelled `kept in the output, against the input`. The old frame showed recovery percentages without a source. Is "kept" the word you want?
7. **Frame 03's info findings.** Rows 22 and 23 now use short labels for info findings, such as `text as outlines` and `digitally signed`, instead of the engine's long summary cut at 21 cells. The full summary stays in history and the report. Agreed?
8. **The schema's questions decide what these frames draw.** Frames 11, 13 and 14 assume the schema's recommended answers: Q1 (full output name in record names), Q2 (submission reference and notes fields), Q3 (host name recorded, with a setup switch), Q5 (CASE file off by default) and Q13 (blank wording). A different answer changes those frames.

9. **Custody mode and the evidence folder, schema Q16.** By default outputs go beside each input, so a custody batch writes nine or more files into the evidence folder. Frame 13 warns before the batch starts, and the record notes the warning. Should custody mode instead refuse to start until `output_dir` is set? That would make the frame 13 warning a blocking message with a third button, `Open setup`, reached with `tab` like the others, not a letter key. It would open setup with the output folder row selected; `esc` from setup would return to the prompt with the batch still pending and the typed values kept.
10. **The queue's partial label.** The result view's queue now prints `partial · C9`, the class of the first partial reason, because a real reason such as `C9: 31 0 obj: unrecoverable stream` does not fit 24 cells. The full reason goes on the hint line. Is the class code enough there?
11. **Kitty drops in custody mode.** Kitty hands over the file's bytes at the drop, so the app reads them before the prompt. The prompt says so on screen. Is that enough, or should custody mode refuse kitty drops and ask for the picker (`B`) or a paste instead?

## Review log

| # | Finding | Severity | What changed |
| --- | --- | --- | --- |
| 1 | Custody screens contradicted the evidence schema | major | Frames 11, 12, 13 and 14 redrawn to the schema: names, three files, SHA-256 always, four prompt fields, blank wording, log entry kinds, Verify, 200 characters. New section "Agreement with the evidence schema". Old open question 2 replaced by question 8 |
| 2 | The custody batch had no data model; concurrency undesigned | major | "Batches and what else is on screen" under frame 13. Planned `AppState.pending_batch` and `QueueEntry.custody_batch` in the build list |
| 3 | Merging turned the integration branch red | major | Split: `design/ux` keeps frames 03 and 05 as they are and passes; `design/ux-d117` holds the D-117 change and its goldens |
| 4 | History did not show which list had focus | minor | Focus rule drawn and written: yellow `►` and lightbar on the focused list, dim `►` on the other; hotkey row says `[tab] go to runs` |
| 5 | Determinism promise left out the answers | minor | Help and its backing table now say "settings and answers" |
| 6 | Sample data broke the partial rule | minor | F7 is picked by the user on frames 05 and 10; frame 05 specifies how a partial result shows its reasons |
| 7 | The log ring was per job | minor | Per queue entry, kept across its jobs, cleared on re-queue |
| 8 | The deny-list rule was wrong | minor | Restated: phrases in, lone labels out; artefact scan of every custody file added to the build list |
| 9 | `v` depended on the current setting | minor | `v` works whenever the run has a record |
| 10 | No build item for `NO_COLOR` | minor | Added for `color_caps` |
| 11 | `v` was only discoverable through help | minor | Result page row 26 and the file menu show it when there is a record |
| 12 | Typing could trigger hotkeys | minor | "Typing goes to the text" rule; the forget confirmation's keys specified |

### Second review, revision 3

| # | Finding | Severity | What changed |
| --- | --- | --- | --- |
| 1 | Cancel logged hashes, and the prompt hashed files in the background | major | Frame 13 row 33 says cancel reads nothing and logs the count. Background hashing removed. `esc` row and agreement table say `batch_cancelled`: count only (schema 2.3) |
| 2 | `item reference` misidentified items in a multi-file batch | major | Renamed `submission reference` on frames 13 and 14 and in the doc, source `run.case.submission_reference`. Frame 14 prints `applies to all 3 files of this batch` |
| 3 | No warning when outputs go into the evidence folder | major | Frame 13 rows 26 to 28 warn, with an `[S]` pointer. States table names the source and `run.destination.warned_at_prompt`. Open question 9 asks schema Q16. Frame 14 row 23 notes the folder too |
| 4 | Doc cited schema revision 2 and `run.original` | minor | Rebased on revision 3: `run.input_file.*`, `run.destination`, agreement table re-checked row by row. Setup and help copy say "input", not "original" |
| 5 | ◆ undefined against Part B; two image settings missing | minor | ◆ means "in Part B settings", and `ask on unknown fonts` has it. `derived image views` and `Type3 glyph images` added as Planned rows. The list scrolls: `↓ 1 more` on the box's bottom edge |
| 6 | `extract images` read like a master switch | minor | Relabelled `save unplaceable images`. New section says custody mode writes every image whatever it says (schema 6.1) |
| 7 | Queue row showed `88% salvaged` | minor | `partial · C9` on frame 14 now, and on frame 05 on `design/ux-d117`. Build list updates the `state.rs` fixture. Open question 10 |
| 8 | `queued` contradicted `pending_batch` | nit | Frame 13 says `3 pdfs are waiting` |
| 9 | Frame 13 caption said two fields | nit | Caption says the cat asks once for the case details, any of which may stay blank |
| 10 | Extra blank line; titles read by index | nit | Blank line dropped. `still_frames` finds titles with `title(fn)` for every frame drawn by a named function |
| 11 | History hotkeys left out `[e]`; blink rule too narrow | nit | `[e] export .md` on row 36. The blink rule names the widget's needs-you status line |

### Third review, revision 4

| # | Finding | Severity | What changed |
| --- | --- | --- | --- |
| 1 | "Cancel reads and runs nothing" was false: `gate()` and `take_drop` read every file before the prompt | major | Option (a). In custody mode the gate admits on name and `fs::metadata` only; the open and the `%PDF-` sniff move to job start, after `stat_before`. Kitty inputs are still read at the drop, and the prompt says so. Frame 13 rows 32 and 33: `The files were listed, never opened`. New section "What the app reads before the prompt", build items, and "Flags for the schema owner" |
| 2 | `enter` on a focused Cancel started the batch; two `►` at once | major | `enter` in a field presses the default button; `enter` or `space` on a button presses that button. The default is `‹ Start batch ›`, without `►`. Keys table has a row for Cancel focused; a focus table names each control's look |
| 3 | `[S]` hints on the prompt would type an `S` into the case reference | minor | Both hints are plain dim text, `in setup`. No key leaves the prompt except `esc` and the buttons |
| 4 | Evidence footer missed `f` and the re-run; `r`/`R` by case; `R` clashed with the file menu; setup's `s` | minor | Footer lists every live key. The re-run is a `[5] also re-run` toggle in the Verify panel. New rule "Letter keys ignore case" scopes setup's `s` and `r` and the menu's `R` |
| 5 | Evidence page printed `blank`; the `.txt` prints "not provided" | minor | The page prints `not provided`, from the custody template module, out of `ALL`. Agreement table corrected |
| 6 | Shared template phrases had no bucket in the deny-list rule | minor | New row: a phrase a custody template also prints comes from the template module and never goes into `ALL` |
| 7 | No drop behaviour on history, setup or help | minor | New rule "A drop works on every screen", with a drop row in the keys of history, the forget confirmation, setup and help. A paste of `.pdf` paths is a drop even with a field focused |
| 8 | Frame 14 counted 31 images | nit | 32, the unplaceable one included |
| 9 | "per output" for records | nit | `per file` on frame 13; `for every file` on setup and help |
| 10 | Help said `<hash>.images/` | nit | `<hash8>.images/` |
| 11 | `04-font-pick` title read by position | nit | Frame 4 is drawn by `frame_fontpick_on_batch`, frame 1 by `frame_main`; every still finds its title with `title()` |
