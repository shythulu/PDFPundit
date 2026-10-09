# UX design: history, setup, help, custody prompt, evidence page, frames 03 and 05

Design proposal, revision 2 of 2026-10-09. Nothing here is built yet. The frames live in `nimbalyst-local/mockups/pdfpundit-ansi-bbs/generate.py`, and their goldens in `tests/data/ui/`.

Revision 2 answers the first review. The review log at the end lists each finding and what changed.

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

The custody screens (frames 11, 12, 13 and 14) follow `/tmp/pdfpundit-run/design/evidence-schema.md`, revision 2. Where the two documents could disagree, the schema wins, because it defines the files. The table under "Agreement with the evidence schema" maps each point.

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
| paste | Inserts the text. A pasted newline does not confirm |
| Every other key | Is text, including `x`, `o`, `f`, `s`, `r`, `q`, `Q`, `H`, `S`, `T`, `?` and digits |

The tui-design guidance lists "global mnemonic shortcuts consume text input" as a known failure. This rule prevents it. The key dispatcher in `src/ui/app.rs` checks for a focused text field before any hotkey.

### Strings and the artefact deny-list

`ui::strings::ALL` is already the deny-list that `src/engine/artefact_tests.rs` scans every artefact with, case-sensitive and by whole word. It is curated: lone labels and column heads stay out, so ordinary report words do not hit. These screens keep that rule.

| Kind of copy | Goes into `ALL`? | Examples |
| --- | --- | --- |
| A phrase the UI draws | Yes, as drawn | `custody mode is on in setup`, `case details first.`, `the cat kept the receipts.` |
| A keyed label | Yes, with its key | `[v] evidence`, `[r] verify` |
| A lone label or column head | No | `examiner`, `sha256`, `case reference`, `item reference`, `notes`, `legacy`, `blank`, `placed`, `after` |

The custody records use the lone labels, so putting them in `ALL` would fail every record. The custody templates are never added to `ALL` either.

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

`Type.field` is a Rust field that exists on `claude/wayfinder-implementation` today. "Planned" marks a field that T-33, the D-114 extractor or this design must add; its name is a proposal. A custody record field is written as the schema names it, for example `run.original.unchanged`.

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

Row 22's `Noto Naskh Arabic (picked)` stays. The family comes from the font database, looked up by `FontResolutionKind::Picked.font_id`. "(picked)" means the matching `InteractionRecord.source` is `User`.

### Why F7 is picked, not a best guess

The sample has to obey the engine's rules. `src/pdf/repair/fonts.rs` marks a `UseBest` reply `Partial`, and a substitute for an unreproducible font too. A best-guess F7 would make thesis_ar `~ partial`, not `√ repaired`. So in the sample the user picked both fonts: frame 05 shows `(picked)` on F3 and F7, and frame 10's ANSWERS shows `you` for both.

### Partial results

When `RepairRun.status` is `OutcomeStatus::Partial`:

- the box note reads `~ partial` in `Y`;
- a FONTS row whose slot was answered `UseBest` or `Skip` ends in `partial` in `Y`;
- the hint line, row 35, shows the first of `RepairReport.partial_reasons` while the file is selected, with `+N more` when there are several.

The reasons come from `PassOutcome::Partial(String)` through `RepairReport.partial_reasons`.

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
| Hotkeys | row 36 | Below |
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
| `e` | Export Markdown. Needs the original at its recorded path; otherwise the hint says why not |
| `v` | Open the selected run's evidence page, when it has a custody record |
| `x` | Forget the file and its runs, after the confirmation below |
| `esc` | Back to where `H` was pressed |

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
| Original moved or gone | `path` row shows the old path with `(missing)` in `Y`; `e` explains |
| Run has a custody record | The RUN block's `config` row is followed by `custody <record name>`, and the key row adds `[v] evidence` |

The samples on frames 10 and 14 are independent moments. Frame 10's session has custody mode off, so its runs have no record.

## Frame 11: setup (`S`)

### Purpose

Change the settings in `config.toml` without opening an editor, and see which ones change the output.

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `setup · config.toml` |
| SETTiNGS box | x 1, y 2, 64×33 | Five sections, one setting per row, rows 3 to 33 |
| Detail box | x 66, y 2, 45×20, modal gradient | What the selected setting does |
| Cat | x 73, y 22, scale 0.465 | Idle squint |
| Save line | row 35 | `saves to <config path> · applies to the next batch` |
| Hotkeys | row 36 | Below |

Each setting row: `►` at x 3 when selected, `◆` at x 5 when every report records it, label at x 7 cut at 24, value at x 31. The box title's note explains `◆ changes the output`.

### Settings shown

| Section | Label | Config field | ◆ |
| --- | --- | --- | --- |
| GENERAL | output folder | `General.output_dir`: `Beside` shows `beside the original` | |
| | default page size | `General.default_page_size` | ◆ |
| REPAIR | auto-accept a font at | `Repair.auto_accept_confidence`, as a percentage | ◆ |
| | font candidates | `Repair.max_font_candidates` | ◆ |
| | extract images | `Repair.extract_unplaceable_images` | ◆ |
| | salvage work | `Repair.salvage_work` | ◆ |
| | deep salvage work | `Repair.salvage_deep_work` | ◆ |
| | deep salvage pool | `Repair.salvage_deep_pool` | ◆ |
| | max search stream | `Repair.max_search_stream` | ◆ |
| FONTS | font source | `Fonts.source` | ◆ |
| | ask on unknown fonts | `Fonts.prompt_unresolved` | |
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

A `◆` setting is one `SettingsSnapshot` holds, so it is in every `RepairReport.settings`. No custody setting has `◆`: schema 3.1 keeps custody settings out of Part B, because they do not change the repair.

The schema's `[custody] case_jsonld` is not shown yet. It waits on schema Q5. When it ships, setup's list is one row too long for the box, and the list scrolls by one row.

### Custody is off, and says so

- The CUSTODY heading carries `mode OFF` in a grey pill: base text on the dim role. Grey, because off is the default and not a fault.
- With custody off, the custody rows draw in `D`. They stay editable.
- With custody on, the pill reads `ON` in `Y`, and every screen's status bar shows `custody ON` in `Y`.
- Frame 11 shows custody mode selected, so the detail box explains it: the case details asked once per batch, the hashing, the three files beside each output and the log entries.

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
| `s` | Save to `config.toml` |
| `r` | Put the selected setting back to its default |
| `esc` | Back. With unsaved changes it asks: `[s] save`, `[d] discard`, `[esc] stay` |

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

Row 18 lists `[v] evidence` with `when the run has a custody record`. Row 10 lists `<output>.custody.*`, matching the schema's names. Rows 28 to 31 say the prompt asks once per batch, every input and output is hashed, and `.custody.json`, `.txt` and `.sha256` go beside each output with an entry in `custody.log`.

### Keys

`esc` or `?` closes it. Nothing else.

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
| Originals are never opened for writing | D-044, `BatchInputs` in `src/place.rs` |
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
| Reminder | x 5, y 31 | `custody mode is on in setup [S]` |
| CASE DETAiLS box | x 48, y 2, 63×31, modal gradient | Below |
| Hint | row 35 | `the cat asks once per batch` |
| Status bar | row 37 | `custody ON │ case details` |

Box rows:

| Row | Holds |
| --- | --- |
| 4 to 5 | How many files, and that the cat asks once, before any of them runs |
| 7 to 8 | `case reference` and its field: brackets at x 52 and 107, text from x 54 |
| 9 to 10 | `examiner` and its field |
| 11 to 12 | `item reference` and its field |
| 13 to 14 | `authority / notes` and its field |
| 16 to 18 | Any field may stay blank and is recorded as blank; 200 characters each; the values go only into the custody records, never into `custody.log`, the PDF or the Markdown |
| 20 to 24 | THiS BATCH: file names, hashes, record names, log entries |
| 27 | `► Start batch` and `[ Cancel batch ]` |
| 29 | `tab next field · enter start · esc cancel batch` |
| 30 | `Cancel runs nothing. custody.log lists the files by hash.` |

The focused field has the lightbar, a `►` at x 50 and a blinking cursor. The others have brackets only. Frame 13 leaves `authority / notes` blank, to show an empty field.

### Keys

| Key | Does |
| --- | --- |
| `tab`, `shift-tab` | Next or previous field, then the buttons |
| typing | Edits the focused field, per "Typing goes to the text" |
| `enter` | Start the batch with what is typed |
| `esc` | Cancel the batch: its files leave, nothing is repaired, no record is written, and one `batch_cancelled` entry lists each file's SHA-256 and size (schema 2.3) |
| paste | Inserted as text; a pasted newline does not start the batch |

The prompt cannot be skipped while `ask_case_details` is true. The user must answer it, but every field may stay blank (DA:433). Setting `ask_case_details` to false is the only way to never see it; the records then have `run.case.prompt_shown = false`.

### Batches and what else is on screen

A custody batch has no data model today: `BatchState` in `src/jobs.rs` is one growing `Vec<QueueEntry>` with no batch id. This design adds one, marked Planned.

| Step | What happens |
| --- | --- |
| 1 | Files are dropped, or added with `+` through the browse picker, while no prompt is open. The chomp plays as usual |
| 2 | A new custody batch opens. Planned: `AppState.pending_batch` holds the files. They are not enqueued yet, so the runner cannot start them |
| 3 | The prompt opens at once, unless another modal is open (font pick, theme chooser, help, a setup edit). Then it opens when that modal closes |
| 4 | Files dropped while the prompt is open join the pending batch. The list updates and the question is not asked again. `+` cannot be pressed: it would be text |
| 5 | `Start batch`: one `batch_opened` log entry, then the files are enqueued. Planned: each gets `QueueEntry.custody_batch`, holding the batch's index in `AppState.custody_batches` and its `batch_opened` log sequence number |
| 6 | `Cancel batch`: the pending files are dropped, and one `batch_cancelled` entry is logged |
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
| Typed values | UI state only. Planned: held per batch in `AppState.custody_batches`, handed to the custody writer, never to the engine (D-020). They become `run.case.case_reference`, `.examiner`, `.item_reference`, `.notes` |
| Pre-filled values | The previous batch's typed values |

The prompt hashes each listed file in the background, so a cancel can log the hashes without reading the files again (schema 2.3).

### Frame 13b: the widget

The widget never shows the form. It shows the needs-you pose, `‼` blinking at the top right, `‼ case details · zoom me` on the status line, and `custody ‼ 1` on the status bar: one open question. Zooming the tile opens frame 13.

### States

| State | What shows |
| --- | --- |
| One file | `1 pdf is queued in custody mode` |
| Many files | Names until the line is full, then `+N more` |
| Field longer than the box | The field scrolls. At 200 characters it takes no more, and the hint says `200 characters at most` |
| Control characters typed or pasted | Never enter the field (schema 4.1) |
| Custody log cannot be opened or locked | The box shows the reason in `R` above the buttons, and `Start batch` is refused (schema 2.4) |
| Log chain found broken | The box says `custody.log was damaged: a new log continues it` in `Y`, naming the new file (schema 8.4) |

## Frame 14: evidence page (`v`)

### Purpose

Show what a custody record holds for a finished file: the case details, the hashes before and after, the files written beside the output, the extracted images, the log entry. It is a page of the result panel, so the queue and the cat stay.

### Layout

Frame 05's layout, with the result panel replaced by the EViDENCE box at x 52, y 2, 59×30. `tab` or `v` flips back to the RESULT page.

| Row | Holds |
| --- | --- |
| 3 to 6 | `case`, `examiner`, `item`, `notes`. A blank field prints `blank` in `D` |
| 8 | `iNPUT`, the file name, and `sha1, md5: legacy` from x 92 |
| 9 to 12 | sha256 on two rows, sha1, md5, in groups of 8 hex digits |
| 13 | `after √ unchanged · hashed again after the run` |
| 15 | `OUTPUT`, its name, and the legacy note |
| 16 to 19 | The same hash rows |
| 20 | `placed √ same bytes as the engine made · atomic` |
| 22 | WRiTTEN BESiDE iT |
| 23 to 25 | `<output>.custody.json`, `.txt`, `.sha256` |
| 26 | `<hash8>.images/`, the image count, `images.json`, `SHA256SUMS` |
| 27 | `custody.log`, this record's entry and the batch's `batch_opened` entry |
| 29 | `[r] verify  [f] folder  [c] copy hashes  [tab] result` |

Hashes print in groups of 8 so an examiner can compare them by eye against another tool's output. SHA-256 always prints. SHA-1 and MD5 print only when the record has them; otherwise their rows close up.

With `[custody] case_jsonld = true`, a row `<output>.case.jsonld  CASE/UCO view` follows the `.sha256` row, and the rows below move down one. Frame 14 has it off, the default.

### Keys

| Key | Does |
| --- | --- |
| `v` | From the result page, open this page. Works whenever the selected run has a custody record, whatever `Custody.enabled` says now |
| `tab`, `v` | Back to the result page |
| `r` | Verify, schema section 9: checks 1 to 4 |
| `R` | Verify with check 5 as well: re-run the repair with the recorded settings and answers and compare `reproducible_sha256`. It takes as long as a repair |
| `f` | Reveal the output folder |
| `c` | Copy the hashes as plain text |
| `esc` | Back |

### What Verify does

Verify follows schema section 9. It never changes the record.

| # | Check | Results |
| --- | --- | --- |
| 1 | Record files against the hashes in their log entry | `match`, `mismatch`, `missing` |
| 2 | Original against `hashes_before` | `match`, `mismatch`, `unavailable` |
| 3 | Each placed file against its recorded hashes | `match`, `mismatch`, `missing`, per file |
| 4 | The log chain up to and past this entry | `intact`, `broken at seq N` |
| 5 | Only with `R`: re-run and compare `reproducible_sha256` | `identical`, `different`, `different build`, `not_run` |

Each Verify writes `<output>.custody.verify-<seq>.txt` beside the record and a `verify` entry in `custody.log`. The page then shows the five results in place of rows 8 to 20, with "Log head after this record" and "Log head now", each with its entry number. The WRiTTEN rows add the verify file. This result view is not drawn yet.

### Data sources

| Shown | Source |
| --- | --- |
| case, examiner, item, notes | `run.case.case_reference`, `.examiner`, `.item_reference`, `.notes` |
| Input sha256 | `RepairReport.input_sha256`, also `run.original.hashes_before.sha256` |
| Input sha1, md5 | `run.original.hashes_before.sha1`, `.md5`; null closes the row |
| after | `run.original.unchanged`: `yes`, `no`, `not_rechecked` with `not_rechecked_reason` |
| Output name | `RepairRun.output_path`, also `run.placed[role = repaired_pdf].name` |
| Output hashes | `run.placed[role = repaired_pdf].hashes` |
| placed | `run.placed[].matches_product` and `.placement` |
| Record file names | `run.record_files.json`, `.text`, `.checksums`, `.case_jsonld` |
| Images folder | `run.images_dir.name`, `<first 8 hex of the input sha256>.images` as `src/place.rs` names it |
| Images count | `reproducible.products` with role `image`. Empty until the D-114 extractor exists |
| Log entry | `run.log.seq`, and `run.batch.log_seq` for `batch_opened` |
| Box note | `run.original.unchanged` |
| Whether `v` works | Planned: `QueueEntry` gains the record's path when T-33 writes it; `RunRecord` gains the record's names for history |

### States

| State | What shows |
| --- | --- |
| Original changed after the run | `after × CHANGED since it was read` in `R`; the box note turns `× changed`; the status bar adds it |
| Original not re-checked | `after not re-checked · <reason>` in `Y`, for example `no durable path` for a kitty drop (D-039) |
| Placed file differs from the engine's bytes | `placed × differs from what the engine made` in `R`, with `run.placed[].note` on the hint line |
| Clean file, no output | The OUTPUT block reads `nothing written: the file was clean` |
| No images | `no images to extract` |
| Record write failed | The WRiTTEN rows read `custody record not written` in `R` with the error; the log holds a `record_write_failed` entry (schema 2.4) |
| Batch still running | Row 27 names the `batch_opened` entry, as frame 14 does |
| Batch closed | Row 27 names the `batch_closed` entry too. The progress box shows `log head after this batch`, the entry number and its SHA-256 in groups of 8, for the examiner to copy (schema 2.3 step 8). Not drawn yet |
| No custody record | `v` does nothing; the hint says `no custody record for this run` |

## Agreement with the evidence schema

| Point | Schema says | These screens now |
| --- | --- | --- |
| Record names | `<output>.custody.json`, after the full name of the primary file (2.2) | Frames 11, 12, 13 and 14 use `<output>.custody.*`; frame 14 shows `thesis_ar.repaired.pdf.custody.json` |
| Files | `.custody.json`, `.custody.txt`, `.custody.sha256`, optional `.case.jsonld` (2.1) | Frame 14 lists all three; the `.case.jsonld` row appears when it is on |
| SHA-256 | Always on; MD5 and SHA-1 are legacy (5.2) | Setup prints `sha256 always`; frames 13 and 14 mark the other two legacy |
| Prompt fields | Case reference, examiner, item reference, notes (4.4, Q2) | Frame 13 has all four, so Q2's "yes" is drawn. If the user says no, rows 11 to 14 go |
| Blank values | Stored as null, printed as "not provided"; the prompt should not quote it (Q13) | The prompt says `recorded as blank`. The evidence page prints `blank`, a lone label kept out of `ALL` |
| Log entries | `batch_opened`, `record`, `batch_closed`, `batch_cancelled`, `verify` (8.3) | Setup, help and frame 13 name them; cancel logs `batch_cancelled` |
| Verify | Five checks, a `.verify-<seq>.txt` and a log entry (9) | `r` runs checks 1 to 4, `R` adds check 5, both write the file and the entry |
| Field limit | 200 characters, no control characters (4.1) | The prompt stops at 200 and refuses control characters |
| Host name | `record_host_name` setting (Q3) | A setup row, Planned |
| Log head | Shown when the batch closes (2.3 step 8, 8.5) | Specified for the progress box; not drawn yet |

## Accessibility

### NO_COLOR

Today the app does not read `NO_COLOR`: `color_caps` in `src/ui/term.rs` picks only truecolor, 256 or 16. The build list adds it. The screens above stay usable without colour:

- Selection keeps the `►` marker; the lightbar becomes reverse video. The other list's selection keeps its `►` without reverse video.
- Every state has a word or glyph next to it.
- Box borders, separators and the grid stay.
- Blinking is limited to one cursor and the `‼` marker, never a whole screen.

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
| Help screen | New screen | Frame 12 |
| Text-field focus checked before any hotkey | Key dispatcher, `src/ui/app.rs` | Frames 10, 11, 13 |
| Pending custody batch, held before enqueue | Planned `AppState.pending_batch` | Frame 13 |
| Custody batch id per entry | Planned `QueueEntry.custody_batch`, indexing `AppState.custody_batches` (case values, `batch_opened` seq, state) | Frames 13, 14 |
| Case details prompt, and the order against open modals and parked files | New screen; the enqueue in `src/ui/app.rs` | Frame 13 |
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
| Picked font's family | Font database lookup by `font_id` | Frame 05 rows 22 and 23 (`design/ux-d117`) |

## Open questions for the user

1. **Prompt strictness.** As designed, the prompt must be answered whenever `ask_case_details` is true, and every field may be blank, as DA:433 says. Do you also want a `require_case_details` setting that refuses an empty case reference?
2. **A drop after a custody batch has started.** It opens a new prompt, pre-filled with the last batch's answers, so `enter` alone carries them over. The other choice is to join the running batch silently, which risks recording the wrong case. Is the pre-filled prompt right?
3. **Times in history.** Frame 10 shows local dates and times, matching the status bar clock. Custody files use UTC. Is local right for the history screen? On Windows the clock code reads only the current time, so past dates need new code either way.
4. **NO_COLOR and the cat.** The cat is not optional, and it is drawn in colour pixels. Under `NO_COLOR`, should the cat become shade-block art (`░▒▓█`), or should the app keep colour for the cat only?
5. **Setup rewrites comments.** `Config::to_toml` writes the whole file, so saving from setup drops comments and unknown keys in a hand-edited `config.toml`. Is that acceptable with a warning, or should setup refuse to save over a file it did not write?
6. **Frame 05 recovery wording.** RECOVERY now shows V1 retention, labelled `kept in the output, against the input`. The old frame showed recovery percentages without a source. Is "kept" the word you want?
7. **Frame 03's info findings.** Rows 22 and 23 now use short labels for info findings, such as `text as outlines` and `digitally signed`, instead of the engine's long summary cut at 21 cells. The full summary stays in history and the report. Agreed?
8. **The schema's questions decide what these frames draw.** Frames 11, 13 and 14 assume the schema's recommended answers: Q1 (full output name in record names), Q2 (item reference and notes fields), Q3 (host name recorded, with a setup switch), Q5 (CASE file off by default) and Q13 (blank wording). A different answer changes those frames.

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
