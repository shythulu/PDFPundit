# UX design: history, setup, help, custody prompt, evidence page, frames 03 and 05

Design proposal of 2026-10-08, branch `design/ux`. Nothing here is built yet. The frames live in `nimbalyst-local/mockups/pdfpundit-ansi-bbs/generate.py`, and their goldens in `tests/data/ui/`.

## What this adds

| Still | Screen | Key | Decision it answers |
| --- | --- | --- | --- |
| `03-batch.png` | Batch view, changed | none | D-117 |
| `05-result.png` | Result view, changed | none | D-117 |
| `10-history.png` | History | `H` | D-048 |
| `10b-history-empty.png` | History, empty | `H` | D-048 |
| `11-setup.png` | Setup | `S` | D-048, D-016 |
| `12-help.png` | Help | `?` | D-048 |
| `13-custody-prompt.png` | Case details prompt | on drop | D-016 |
| `13b-widget-custody.png` | Case details prompt in the widget | on drop | D-016 |
| `14-evidence.png` | Evidence page of a finished file | `v` | D-016, D-114 |

Every other still is byte-identical to `claude/wayfinder-implementation`, checked with `cmp`. That includes `04-font-pick.png`, which is a dimmed copy of frame 03: every frame 03 change sits under its modal.

### What the build has to do first

1. Re-run the three UI tests that now fail. `batch_matches_frame_03`, `result_matches_frame_05` and `queue_text_matches_the_batch_golden` compare against the new goldens, so the Rust fixtures and layout must follow frames 03 and 05.
2. Remove the `BATCH_UNFILLED` and `RESULT_UNFILLED` masks in `src/ui/layout/full.rs`. Every masked span now has a named source, listed below.
3. Build frames 10 to 14 as new screens behind the keys that answer "not yet" today.

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
| `b` lightbar | The selected row, the focused field |
| modal gradient | Boxes that ask the user something: case details, setup's detail panel, help |

### Focus and state survive without colour

- The selected row always has the lightbar and, where the row is an action or a field, a `►` marker.
- Every state is a word or a glyph: `√ repaired`, `~ partial`, `× failed`, `· pending`, `‼`, `[x] on`, `[ ] off`, `OFF`.
- Toggles always print `on` or `off`. Choices print `‹ value ›`, so the arrows say "←→ changes this".

### Text the user or a file supplied

File names, paths, finding summaries, case reference and examiner are drawn verbatim. Control characters are escaped, as the existing hostile-name tests require. A value too long for its column is cut at the column and ends in `…`. Frame 10 shows one: `payroll_2025_redacted.p…`.

### Strings and the artefact deny-list

All UI copy goes into `src/ui/strings.rs`. `ui::strings::ALL` becomes the deny-list that `src/engine/artefact_tests.rs` scans every artefact with, case-sensitive and by whole word. So:

- The custody `.json` and `.txt` templates must not reuse a phrase that is in `ALL`. If setup says "custody mode" and the `.txt` also says "custody mode", the artefact test fails.
- The custody templates must never be added to `ALL` themselves.
- Recommendation: the files use their own capitalised wording, for example "Chain of custody record", "Case reference", "Examiner".

### The widget

The 32×16 widget has no menus and never shows a form.

| Event in the widget | What it shows |
| --- | --- |
| `H`, `S` or `?` | The status line says `zoom me`; nothing opens |
| Custody prompt due | `‼ case details · zoom me`, blinking, the needs-you pose, `‼` in the corner (frame 13b) |
| Zoomed to 112×38 | The full screen opens on the screen that was asked for |

### How sources are written below

`Type.field` is a Rust field that exists today. "Planned" marks a field that T-33 or the D-114 extractor must add; its name is a proposal.

## Frame 03: batch view (D-117)

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

Each line is one event of the selected file's job, newest at the bottom, last four kept.

| Line kind | Format | Source |
| --- | --- | --- |
| Phase change | `11:42 measuring · step 4 of 4` | `JobEvent::Phase { name, index, total }` |
| Engine note | `11:42 warn 3 NaN or infinite numbers were written as 0` | `JobEvent::Log(LogLevel, String)`. `info` prints no level word; `warn` in `Y`, `error` in `R` |
| Time | `hh:mm` | The UI clock when the event arrived, as the status bar reads it. Seconds are dropped: the clock has none |

The view model needs a ring of the last four log lines per job. The events already exist; today `src/ui/app.rs` only writes them to the debug log.

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

## Frame 05: result view (D-117)

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
| 23 | `Noto Sans partial bold→reg` | `Noto Sans` | `SubstituteChoice.label`. The caveat has no field |

Row 22's `Noto Naskh Arabic (picked)` stays. The family comes from the font database, looked up by `FontResolutionKind::Picked.font_id`. "(picked)" means the matching `InteractionRecord.source` is `User`.

### Recovery rules

- The two bars are V1 retention ratios of the chosen candidate. They compare the output with the input's text baseline.
- With `Verification.baseline == BaselineKind::None`, both rows say `no baseline` in `D` and draw no bar.
- With `CarveProxy`, the rows draw as normal and the header note reads `kept, against a carve estimate`.
- A zero denominator prints `none in the input` and draws no bar.
- The C9 line shows only when `C9Summary.streams_damaged > 0`. Otherwise rows 17 and 18 stay blank.
- The C9 line fits two rows of 55 cells up to four-digit counts. Longer counts are cut with `…`, as `wrap` does today.

The extracted-images line left frame 05. It now lives on the evidence page, frame 14.

### States

| State | What shows |
| --- | --- |
| Clean file, no output written | Rows 4 to 6 say `clean · nothing written`; no bars; no C9 line |
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
| 5 to 18 | One file per row: glyph x 3, name x 6 cut at 24, runs right-aligned to x 34, date x 37, status word x 49 |
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
| 28 | Keys for this file |

### Keys

| Key | Does |
| --- | --- |
| `↑↓` | Move in the focused list |
| `tab` | Move focus between files and runs |
| `/` | Type a filter; `esc` clears it |
| `o` | Open the selected run's output |
| `f` | Reveal the output's folder |
| `e` | Export Markdown. Needs the original at its recorded path; otherwise the hint says why not |
| `x` | Forget the file and its runs, after a confirmation that names the file and says the outputs stay on disk |
| `esc` | Back to where `H` was pressed |

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
| ANSWERS | `report.interactions`: `request.slot`, `request.page`, the `reply` font's family, and `source` (`User` → `you`, `Policy` → `policy`, `UseBest` → `best guess`; D-141's `Batched` → `carried over`) |

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

## Frame 11: setup (`S`)

### Purpose

Change the settings in `config.toml` without opening an editor, and see which ones change the output.

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `setup · config.toml` |
| SETTiNGS box | x 1, y 2, 64×33 | Five sections, one setting per row |
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
| | hashes | `Custody.hashes` | |
| | ask case details | `Custody.ask_case_details` | |
| | custody log | `Custody.log_path`; `None` shows `data folder/custody.log` | |
| LOOK | theme | `Ui.theme` | |
| | layout | `Ui.layout` | |
| | mouse | `Ui.mouse` | |
| | grow when needed | `Ui.request_resize` | |

A `◆` setting is one `SettingsSnapshot` holds, so it is in every `RepairReport.settings`.

### Custody is off, and says so

- The CUSTODY heading carries `mode OFF` in a grey pill: base text on the dim role. Grey, because off is the default and not a fault.
- With custody off, the four custody rows draw in `D`. They stay editable.
- With custody on, the pill reads `ON` in `Y`, and every screen's status bar shows `custody ON` in `Y`.
- Frame 11 shows custody mode selected, so the detail box explains it.

### Keys

| Key | Does |
| --- | --- |
| `↑↓` | Move |
| `space` | Toggle a `[x]` setting |
| `←→` | Step a `‹ choice ›` |
| `enter` | Edit a number or a path in place; `enter` confirms, `esc` cancels |
| `s` | Save to `config.toml` |
| `r` | Put the selected setting back to its default |
| `esc` | Back. With unsaved changes it asks: save, discard, or stay |

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
| Same file, version and settings give the same bytes | DA:423-425, D-041, the `clippy.toml` time and libm bans |
| No network code | D-050, `tools/purity-gate.py --network` |
| Every fix is listed in the report | `RepairReport.passes[].actions` |
| No cat in any file it writes | `src/engine/artefact_tests.rs` |

## Frame 13: custody prompt

### Purpose

When custody mode is on and files are dropped, get a case reference and an examiner once for the whole batch, before any job starts (DA M6b, D-020).

### When it shows

1. The user drops files, and the chomp plays as usual.
2. If `Custody.enabled` and `Custody.ask_case_details` are both true, this screen opens before the runner starts any job.
3. Files dropped while it is open join the same batch. The list updates and the question is not asked again.
4. Files dropped after the batch has started form a new batch, so they get a new prompt.

The prompt cannot be skipped while `ask_case_details` is true. The user must answer it, but both fields may stay blank (DA:433). Setting `ask_case_details` to false is the only way to never see it.

### Layout

| Region | Cells | Contents |
| --- | --- | --- |
| Header | row 0 | `new batch · custody mode ON` |
| Cat | x 4, y 8, scale 0.68, needs-you pose | Wide eyes on the viewer |
| Cat's line | x 8, y 28 | `case details first. then food.` |
| Reminder | x 5, y 31 | `custody mode is on in setup [S]` |
| CASE DETAiLS box | x 48, y 2, 63×27, modal gradient | Below |
| Hint | row 35 | `the cat asks once per batch` |
| Status bar | row 37 | `custody ON │ case details` |

Box rows:

| Row | Holds |
| --- | --- |
| 4 to 5 | How many files, and why the cat asks |
| 7 to 8 | `case reference` and its field: brackets at x 52 and 107, text from x 54 |
| 10 to 11 | `examiner` and its field |
| 13 to 15 | Both may stay blank, the record then says "not given", and the values go nowhere else |
| 17 to 21 | THiS BATCH: file names, hashes, record names, log |
| 24 | `► Start batch` and `[ Cancel batch ]` |
| 26 | `tab next field · enter start · esc cancel: nothing is lost` |

The focused field has the lightbar, a `►` at x 50 and a blinking cursor. The unfocused one has brackets only.

### Keys

| Key | Does |
| --- | --- |
| `tab`, `shift-tab` | Next or previous field, then the buttons |
| typing | Edits the focused field. Single-letter app keys do nothing here |
| `enter` | Start the batch with what is typed |
| `esc` | Cancel the batch: the files leave the queue, nothing is repaired, no record is written |
| paste | Inserted as text; a pasted newline does not start the batch |

### Data sources

| Shown | Source |
| --- | --- |
| Number of files, names | `QueueEntry.name` for the queued entries of this batch |
| Hashes list | `Custody.hashes` |
| Record names, log name | Planned, T-33. The log path is `Custody.log_path` or `custody.log` in `AppDirs.data_dir` |
| Typed values | UI state only. Planned: held per batch in `AppState`, handed to the custody writer, never to the engine (D-020) |

### Frame 13b: the widget

The widget never shows the form. It shows the needs-you pose, `‼` blinking at the top right, `‼ case details · zoom me` on the status line, and `custody ‼ 3` on the status bar. Zooming the tile opens frame 13.

### States

| State | What shows |
| --- | --- |
| One file | `1 pdf is queued in custody mode` |
| Many files | Names until the line is full, then `+N more` |
| Field longer than the box | The field scrolls; the record keeps the full text, up to a limit the build sets |
| Control characters typed or pasted | Dropped from the field |
| Custody log not writable | The box shows the reason in `R` above the buttons, and `Start batch` is refused |

## Frame 14: evidence page (`v`)

### Purpose

Show what custody mode adds to a finished file: the case details, the hashes before and after, the files written beside the output, the extracted images. It is a page of the result panel, so the queue and the cat stay.

### Layout

Frame 05's layout, with the result panel replaced by the EViDENCE box at x 52, y 2, 59×30. `tab` flips between the RESULT and EViDENCE pages.

| Row | Holds |
| --- | --- |
| 4 to 5 | `case`, `examiner` |
| 7 | `iNPUT` and the file name |
| 8 to 11 | sha256 on two rows, sha1, md5, in groups of 8 hex digits |
| 12 | `after √ unchanged · hashed again when the run ended` |
| 14 to 18 | `OUTPUT`, its name and the same hash rows |
| 20 to 24 | WRiTTEN BESiDE iT: the two record files, the images folder and its count, the log entry |
| 26 | `[r] re-verify  [f] folder  [c] copy hashes  [tab] result` |
| 29 | `receipts kept · not one cat in them` |

Hashes print in groups of 8 so an examiner can compare them by eye against another tool's output. Only the algorithms in `Custody.hashes` print; the rows close up.

### Keys

| Key | Does |
| --- | --- |
| `v` | From the result page, open this page. Only in custody mode |
| `tab` | Back to the result page |
| `r` | Re-verify: hash the original and the output again and compare with the record (DA:447-448) |
| `f` | Reveal the output folder |
| `c` | Copy the hashes as plain text |
| `esc` | Back |

### Data sources

| Shown | Source |
| --- | --- |
| case, examiner | Planned: the custody record's case reference and examiner, from frame 13 |
| Input sha256 | `RepairReport.input_sha256` |
| Input sha1, md5 | Planned, T-33. Needs `sha1` and `md-5` as direct dependencies |
| after | Planned: the custody record's re-hash verdict, unchanged, changed or not re-verifiable |
| Output hashes | Planned: hashed after the output is placed |
| Output name | `RepairRun.output_path` |
| Record file names | Planned: the paths T-33 writes |
| Images folder | `<first 8 hex of the input sha256>.images/`, as `src/place.rs` names it |
| Images count | `RepairReport.extracted_images.len()`. Empty until the D-114 extractor exists |
| Log entry | Planned: the custody log's entry number and the previous entry's number |

### States

| State | What shows |
| --- | --- |
| Original changed after the run | `after × CHANGED since it was read` in `R`; the box note turns `× changed`; the status bar adds it |
| Original not re-verifiable | `after not re-verifiable · <reason>` in `Y`, for example a kitty drop with no path (D-039) |
| Clean file, no output | The OUTPUT block reads `nothing written: the file was clean` |
| No images | `no images to extract` |
| Record write failed | The WRiTTEN rows show the failed file in `R` with the reason |
| Custody mode off | `v` does nothing; the hint says `custody mode is off` |

## Accessibility

### NO_COLOR

The app does not read `NO_COLOR` today: `color_caps` in `src/ui/term.rs` picks only truecolor, 256 or 16. The screens above are designed to stay usable without colour:

- Selection keeps the `►` marker; the lightbar becomes reverse video.
- Every state has a word or glyph next to it.
- Box borders, separators and the grid stay.
- Blinking is limited to one cursor and the `‼` marker, never a whole screen.

The cat is the hard part. It is drawn in half-block colour pixels. A no-colour cat needs its own rendering, which is an open question below.

### 16 colours

- Every role in the tables above maps to one ANSI slot through the theme's `ansi16`, as `Theme::downgrade` does today.
- Gradients collapse to their nearest slot, so box borders become one colour. That loses nothing these screens rely on.
- The bars on frame 05 keep their numbers beside them, so a flat bar still reads.
- Hash rows use body text only, so 16 colours change nothing there.

### Reading order

Each screen reads top to bottom, left to right, in the order the user acts: list, then detail, then keys. The status bar always says which screen is open.

## What the build needs

| Item | Where | For |
| --- | --- | --- |
| Carry `CarveSummary.objects` into `QueueEntry` | `src/jobs.rs`, `src/ui/view.rs` | Frame 03 row 14 |
| Per-job ring of the last four phase and log events, with arrival time | `src/ui/app.rs`, `ViewModel` | Frame 03 log |
| Phase index and total in the view model | `ViewModel.current` | Frame 03 rows 16 and 17 |
| Short labels for info findings | `strings.rs` | Frame 03 rows 22 and 23 |
| Output name and folder, chosen toolpath, rediagnose flag, V1 ratios, baseline kind | `ViewModel` from `RepairRun` | Frame 05 |
| Picked font's family | Font database lookup by `font_id` | Frame 05 row 22 |
| History screen, store queries, local date formatting | New screen; `src/ui/app/clock.rs` for the offset | Frame 10 |
| Setup screen and its editor, save through `Config::to_toml` | New screen | Frame 11 |
| Help screen | New screen | Frame 12 |
| Case details prompt, batch gate before the runner starts | New screen; the runner start in `src/ui/app.rs` | Frame 13 |
| Custody record, hashes, log, re-verify | T-33, `src/custody.rs` | Frame 14 |
| Image extraction | D-114 | Frame 14 images count |

## Open questions for the user

1. **Prompt strictness.** As designed, the prompt must be answered whenever `ask_case_details` is true, and both fields may be blank, as DA:433 says. Do you also want a `require_case_details` setting that refuses an empty answer?
2. **Custody file names.** The mockup's desktop frame shows `report_2024.custody.json` beside `report_2024.repaired.pdf`, so frame 14 uses the input's stem: `thesis_ar.custody.json`. The other choice is the output's stem, `thesis_ar.repaired.custody.json`, which pairs cleanly with `(2)` suffixes. Which?
3. **Images without export.** In custody mode, should the images folder be written beside the output even when no Markdown is exported? Frame 14 assumes yes, because D-114 says images feed the evidence report.
4. **Times in history.** Frame 10 shows local dates and times, matching the status bar clock. Custody files would use UTC. Is local right for the history screen? On Windows the clock code reads only the current time, so past dates need new code either way.
5. **NO_COLOR and the cat.** The cat is not optional, and it is drawn in colour pixels. Under `NO_COLOR`, should the cat become shade-block art (`░▒▓█`), or should the app keep colour for the cat only?
6. **Setup rewrites comments.** `Config::to_toml` writes the whole file, so saving from setup drops comments and unknown keys in a hand-edited `config.toml`. Is that acceptable with a warning, or should setup refuse to save over a file it did not write?
7. **Frame 05 recovery wording.** RECOVERY now shows V1 retention, labelled `kept in the output, against the input`. The old frame showed recovery percentages without a source. Is "kept" the word you want?
8. **Frame 03's info findings.** Rows 22 and 23 now use short labels for info findings, such as `text as outlines` and `digitally signed`, instead of the engine's long summary cut at 21 cells. The full summary stays in history and the report. Agreed?
