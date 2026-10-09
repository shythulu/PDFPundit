# Evidence and report schema: custody mode and image extraction

Date: 2026-10-09. Role: evidence-report architect. Draft for the user and the T-33 and D-114 tickets.
Revision 5: the user's round-3 answers (decision log, commit `dcc0859`, "Answered by the user, 2026-10-09 (round 3 ...)") are applied as decisions; section 15 records them. Three of them changed the design: custody mode needs `output_dir`, the UTC offset is filled on every platform, and verify files are named `.verify-<n>.txt`.
Revision 4: every finding of the final review is addressed, re-checked against the code at `f5ddfa6` (after G-01, G-03, G-04, G-05, G-06, G-07 and G-09). Revisions 2 and 3 fixed the first and second reviews. The review log at the end records each finding and what changed.
Inputs: the four research files (`research-*.md`, kept outside the repository like the fact reports; the plans folder's `README.md` says why), the desktop plan M6b, `decision-log.md`, the UX design (`ux-design.md`, frames 13 and 14), and at `f5ddfa6`: `src/engine/report.rs`, `src/pdf/model.rs`, `src/jobs.rs`, `src/pdf/repair.rs`, `src/pdf/repair/fonts.rs`, `src/pdf/diagnose.rs`, `src/pdf/streams/salvage.rs`, `src/pdf/verify.rs`, `src/ui/input/gate.rs`, `src/ui/app.rs`, `src/pdf/text.rs`.
Machine-checked companions, all under `nimbalyst-local/plans/implementation/evidence-schema/`:

| File | What it is |
|---|---|
| `example/pdfpundit-custody.schema.json` | JSON Schema 2020-12 for the custody record, the log entry and the images index |
| `example/invoice_scan_2024.repaired.pdf.custody.json` | The worked example. Validates against the schema |
| `example/invoice_scan_2024.repaired.pdf.custody.txt` | The text report rendered from that record |
| `example/invoice_scan_2024.repaired.pdf.custody.sha256` | The checksum manifest |
| `example/custody.log` | Three chained log entries: batch opened, record, batch closed |
| `example/f7c39aa7.images/` | `images.json` and `SHA256SUMS` for the two images |
| `example/invoice_scan_2024.repaired.pdf.case.jsonld` | CASE 1.5.0 view built from the record. `case_validate` says `Conforms: True` |
| `example/fixtures/invoice_scan_2024.md.custody.json` | A custody-off repair followed by a custody-on export, on another workstation with its own log. Validates against the schema |
| `example/fixtures/salvage-outcomes.custody.json` | The example plus one stream that decodes but fails its Adler-32 and one cut short, with their images. Validates against the schema |
| `example/fixtures/export-after-custody-off.b7.txt` | The B.7 lines that fixture renders: the answers are marked as given in an earlier run with no record |
| `work/templates.py` | Every fixed sentence, the plural rule, and the engine format strings the example quotes. The example, this document and the validator all read it |
| `work/*.py` | The scripts that generate and check all of the above. `validate.py` runs 22 checks over the record and both fixtures, including jq against Python, the font rule, the C9 counter, grade and window rules, the time order and the fixed text |
| `work/ascii_fixture.py` | Finding 8's fixture: a resource name with 0x7F and non-UTF-8 bytes, raw and escaped |

## 1. Answer first

The design in nine decisions:

1. **One record per job, three files beside the output.** `<output>.custody.json` is the source of truth. `<output>.custody.txt` is the examiner's printable view. `<output>.custody.sha256` lets anyone run `sha256sum -c`. Custody mode writes them only to `[general] output_dir`, and does not start without it or when it is an input's folder, decision Q16.
2. **Two parts in every record.** Part B, `reproducible`, is a pure function of input bytes, build, settings and recorded answers, so it is byte-identical on re-run. Part A, `run`, holds times, host, paths, case details and the log link. This is D-073 carried into the custody file.
3. **Part B has its own hash.** `reproducible_sha256` is the SHA-256 of the `reproducible` member in compact JSON. Anyone can check it with `jq -cj .reproducible x.custody.json | shasum -a 256`. Part B holds printable ASCII only, which is what makes jq, Python and serde_json agree. Tested with jq 1.8.2.
4. **SHA-256 always.** MD5 and SHA-1 are added per `[custody] hashes`, labelled legacy, only in Part A. SHA-256 cannot be switched off because the history store, the log chain and Part B all key on it.
5. **Images get a full provenance record in every mode.** The record lives in `RepairReport` v2, so history has it even with custody off. Files are named by content, `<first 16 hex of sha256>.<ext>`. A file holding samples PDFPundit changed, cut short or could not verify carries its status in the name, such as `<hex16>.repaired.png` or `<hex16>.unverified.png`, so it cannot pass as an as-found extraction. Two hashes per image: the raw stream as found, and the extracted file.
6. **Every answer records who gave it.** `user`, `batched` or `policy`, per D-141, plus which earlier answer a batched one copied. The engine's `UseBest` source is the user's own answer, so it maps to `user` with reply `use_best`. Answers replayed from an earlier run say so and name that run, whether or not that run had a custody record.
7. **Every font slot records why it got its font.** `font_decisions` lists each slot with its basis: name match, auto-accept with the scores and thresholds, an answer, or unsupported.
8. **The custody log is JSON Lines with a SHA-256 chain, and holds no case details.** Each line's `prev` is the SHA-256 of the line before it. Lines carry hashes, counts and outcomes only, so a chain can be shown without exposing other matters. A `batch_closed` entry gives the log head that the examiner copies off screen. At start-up the app compares the log with the last head kept in history, so a deleted or swapped log is caught too. Anyone who can write the file can still rewrite it from an edit onward, so only a head copied outside the log pins it. It is tamper-evident, never called tamper-proof.
9. **CASE/UCO JSON-LD is optional and off by default.** It is generated from the custody record, never the other way. The mapping is proven valid offline.

## 2. Files written

### 2.1 The file set

| File | Written when | Mode | Deterministic | Source of truth |
|---|---|---|---|---|
| `<output>` itself: `.repaired.pdf` or `.md` | Every job with an output | All | Yes | n/a |
| `<hash8>.images/<hex16>[.<status>].<ext>` | Markdown export, or any repair with custody on | All | Yes | n/a |
| `<hash8>.images/images.json` | Whenever the images folder is written | All | Yes | Image records |
| `<hash8>.images/SHA256SUMS` | Whenever the images folder is written | All | Yes | Image file hashes |
| `<primary>.custody.json` | Every job, including failed and clean ones | Custody | Part B only | Yes |
| `<primary>.custody.txt` | With the `.json` | Custody | No | Rendered from the `.json` |
| `<primary>.custody.sha256` | With the `.json`. Lists only files PDFPundit wrote, never the input. See Appendix C | Custody | No | Rendered from the `.json` |
| `<primary>.case.jsonld` | With the `.json`, only if `[custody] case_jsonld = true`, off by default, decision Q5 | Custody | No | Rendered from the `.json` |
| `custody.log` | One line per batch opened, record, batch closed, batch cancelled, batch left unclosed, verify or failure | Custody | No | Yes, for the chain. Holds no case details |

The `.txt`, `.sha256` and `.case.jsonld` are views. If a view and the `.json` ever disagree, the `.json` wins.

### 2.2 Naming

The record takes the full name of the job's primary file and appends `.custody.json`.

| Job | Primary file | Record name |
|---|---|---|
| Repair | the placed PDF | `invoice.repaired.pdf.custody.json` |
| Repair, output got a suffix | the placed PDF | `invoice.repaired (2).pdf.custody.json` |
| Repair plus one-shot export | the placed PDF; the `.md` is a second placed file | `invoice.repaired.pdf.custody.json` |
| Export only, after the job ended (D-125) | the placed `.md` | `invoice.md.custody.json`. Its answers are replayed, see 7.4 |
| No output: clean, failed, encrypted, cancelled | the input | `invoice.pdf.custody.json`, in the destination folder |

Why the full name and not the stem:

- It pairs one record with one file, even after a `(N)` suffix.
- A repair record and an export record for the same input can never collide.
- It sorts next to its file in any folder listing.

The mockup shows `report_2024.custody.json` beside `report_2024.repaired.pdf`. That reading drops `.repaired` and collides with an export record. The user chose the full name, decision Q1.

The three record names are claimed before anything is written, so the record can state its own names. A taken name gets the usual `(N)` suffix, and the record names the file it pairs with by hash, so pairing survives.

Each record file is written the way placement writes outputs (D-044), never in place:

1. Claim the final name with `create_new` as an empty placeholder.
2. Write the bytes to a temporary file in the same folder, and fsync it.
3. Rename the temporary file onto the placeholder, the `ClaimedThenRenamed` path `jobs.rs` already has.

A crash therefore leaves an empty placeholder or a stray temporary file, never a truncated file under a record's name. An empty `.custody.json` with no `record` log entry is what a crash looks like, and Verify says so.

### 2.3 Write order

1. Open the batch: append a `batch_opened` log entry with the file count. Case details go only into the records. A batch never starts if the log cannot be opened, or if the start-up check in 8.4 has not run.
2. Read the clock: this is `started_utc`. Then stat the input, then read it into memory once. Hash that buffer with every configured algorithm. These are the "before" stat and hashes. For a path input this is PDFPundit's first touch of the file's content, once the custody-mode gate below ships. An input that arrived as bytes was read earlier, at the drop: its "before" hashes are of the bytes received, and its stat, when it has a path, is taken now, after that read.
3. Run the job. Place outputs as today. Each answer given on screen gets its asked and answered times, in `answer_times`.
4. Re-read each placed file from disk and hash it. Compare with the engine's product hash.
5. Re-read the input from its path and hash it, then stat it again. These are the "after" hashes and stat. Read the clock: this is `ended_utc`.
6. Claim the record names. Write `.custody.json`, then `.custody.txt`, then `.custody.sha256`, then `.case.jsonld` if on. Fsync each. The `.txt` prints the `.json`'s SHA-256 on every page, which works because the `.json` is already final.
7. Append a `record` log entry holding the SHA-256 of each record file, tagged by role. Fsync.
8. After the last job of the batch, append a `batch_closed` entry. Fsync. The screen then shows the SHA-256 of that line as "log head after this batch", with its entry number, for the examiner to copy into notes.

Step 7 comes after step 6, so the log never names a file that does not exist. Step 8 is the only way to get an anchor that covers the batch's last record, because nothing printed in step 6 can contain a hash of step 7.

`ended_utc` is read before the record files are written, because the `.json` cannot hold a time later than its own write. The `record` log entry's `time_utc` is read after the record files are fsynced. So in every run: `batch_opened` time, then `started_utc`, then the answer times, then `ended_utc`, then the `record` entry's time. `validate.py` check 12 tests that order on the example.

### 2.3.1 What PDFPundit reads before the prompt

What "before the input was first read" means depends on how the input arrived. `run.input_file.path_kind` and `first_read` record it, and the `.txt` words the Started line to match.

| Input, `JobInput` variant | `path_kind` | Read before the prompt | `first_read` | Started line in the `.txt` |
|---|---|---|---|---|
| A path: drop, paste or picker, `File` | `file` | Nothing, with the custody-mode gate. Name and `fs::metadata` only | `job_start` | "before the job first read the input's file state" |
| A kitty drop, `Dropped`: path plus bytes read whole while the drop was active (`take_drop`, T-31) | `dropped` | The whole file, at the drop | `drop` | "after the input was received whole at the drop" |
| A macOS file promise, `Memory`: bytes and a name, no path (D-039) | `memory` | The whole file, at the drop | `drop` | Same |

The custody-mode gate is a build item, section 14 item 23, and the UX design lists it. Today `gate()` in `src/ui/input/gate.rs` opens every dropped or picked file and reads its first KiB for the `%PDF-` sniff, before any prompt. Until the gate change ships, a path input has been opened before the prompt and before `stat_before`, so its `stat_before.accessed` can show that read. In custody mode the gate admits on name and `fs::metadata` only, and the open and the sniff move to job start, after `stat_before`.

### 2.3.2 Cancelling

If the examiner cancels the custody prompt, no job runs and no record is written. The app appends one `batch_cancelled` entry with two counts and nothing else:

- `files`: how many files the batch held.
- `received`: how many of them arrived whole at the drop, as `dropped` or `memory` inputs. Their bytes are discarded unhashed.

It hashes nothing. With the custody-mode gate, the prompt never opened a path input: frame 13 of the UX design lists file names and the configured algorithm names, not hash values. Hashing on cancel would mean reading files the examiner just declined to process, with no stat before or after, which is an unrecorded access to evidence. SWGDE 18-Q-002 5.3.4 lists items "whether examined or not". The counts are what the tool can state without reading more, and the examiner's own notes identify the files. A received input was read by the terminal's drop before the prompt could exist; `received` says so, and frame 13 tells the examiner before they choose.

A job cancelled after `Start batch` is different: the examiner chose to process that file. Its record is written with `job_status = cancelled`. If the job had read the input, the hashes it took are kept. If it had not, `hashes_before` is null and `not_rechecked_reason` says "cancelled before the input was read". PDFPundit never reads an input only to hash it for a cancelled job.

### 2.3.3 A batch the app never closed

If the app quits with jobs of a batch unfinished, `JobRunner::shutdown` cancels them and no `batch_closed` entry is written. Each cancelled job's record is written when shutdown has time for it, within its grace period. At the next start, the 8.4 check finds a `batch_opened` with no `batch_closed` or `batch_cancelled` after it, and appends a `batch_unclosed` entry: the batch's entry number, its file count, and how many `record` and `record_write_failed` entries it has. The screen shows it once.

### 2.4 Failure handling

| Failure | What happens | Why |
|---|---|---|
| Log cannot be opened or locked | The batch does not start. The screen says why | A custody batch with no log breaks the chain. Matches D-062: fail loudly |
| Input has no durable path: a `memory` input, or a `dropped` one whose path is gone | `unchanged = not_rechecked`, reason `no durable path` | D-039. The record says so, it never skips silently |
| Input vanished or unreadable after the run | `unchanged = not_rechecked`, reason gives the OS error | Same |
| Input hash after differs from before | `unchanged = no`. Outputs are kept. The row turns red | The examiner must see it. Deleting outputs would destroy evidence of what ran |
| Placed file re-read differs from product | `matches_product = false`, note gives the reason | Shows disk or placement trouble |
| Images folder renamed after a clash (D-125) | `images_dir.outcome = renamed_after_clash`. The `.md` placed hash differs from the product hash, note says why | The one case where the `.md` bytes depend on disk state |
| Record cannot be written after the PDF was placed | A `record_write_failed` log entry with input hash, output hashes and the OS error kind. The row shows "custody record not written" and the full error | The PDF already exists. The log keeps the hashes the record would have held, and no path |
| Job cancelled or failed before the engine ran | Record written with `reproducible = null` and `job_status` saying why | SWGDE 18-Q-002 5.3.4 lists items "whether examined or not" |
| Custody prompt cancelled | No records. One `batch_cancelled` log entry with `files` and `received`. Path inputs are not read; received bytes are discarded unhashed | The log shows a batch was dropped and how many files it held, without reading more. 2.3.2 |
| Job cancelled after `Start batch` | Record with `job_status = cancelled`. Hashes only if the job had read the input | 2.3.2 |
| App quits with a batch unfinished | No `batch_closed`. A `batch_unclosed` entry at the next start | 2.3.3 |
| `[general] output_dir` unset when custody mode is switched on or a batch starts | Custody mode does not start. Setup refuses to turn it on, and the prompt shows the reason in place of `Start batch`, which cannot be pressed: "custody mode needs an output folder: set one in setup". No job runs, no record or log entry is written | Decision Q16: the evidence folder stays untouched. D-061's default writes beside the input, which custody mode never does |
| `output_dir` is the folder of a pending input | The batch does not start. The prompt names the folder: "the output folder is an evidence folder: <folder>". The examiner sets another folder or removes that file from the batch | Same. The check compares folders after `std::path::absolute` on both sides |

## 3. The split: reproducible part and run context

### 3.1 Rule

Part B holds every fact that is a pure function of four things:

- the input bytes
- the build: tool version, build commit, platform (operating system and processor architecture), font DB
- the settings snapshot
- the recorded answers

Everything else goes in Part A. Part B never holds a path, a time, a host, a file name taken from disk, or a custody setting.

The platform is part of the build on purpose. Cross-platform identity of the engine's output is a design goal (D-041, D-059, D-066), but it has been measured only in part: paint counts and the salvage digest on macOS arm64 and Linux x86_64, and Windows not at all (D-058, D-059). So the record claims identity only for the same build on the same platform. `tool.platform` comes from `std::env::consts::OS` and `ARCH`, which are compile-time constants. A re-run on another platform gets a different Part B and Verify reports `different build`, never a false mismatch. The `reproducibility` statement says this in every record.

The settings snapshot includes every setting that changes Part B, not only the engine's. Two are runner settings: `prompt_unresolved` decides whether an answer is `user` or `policy`, and `derived_image_views` adds `view_file`, `view_sha256` and `image_view` products. Both are in Part B's `settings`.

Today the four inputs are complete because fonts come only from the bundled database, whose SHA-256 is in `tool.font_db_sha256`. `font_policy = "bundled_then_system"` is accepted by `config.rs` but warns `SystemFontsNotBuilt` and uses bundled fonts, so Part B records both the configured value and `font_source_effective`. When system fonts land, the installed fonts become a fifth input. **Before system-font support ships, it must put the SHA-256 of every system font it used into Part B**, or the reproducibility statement becomes false across machines. Section 14 item 21 holds this.

### 3.2 Where each kind of fact lives

| Fact | Category | Lives in | Why there |
|---|---|---|---|
| Tool name, version, build commit, platform, font DB SHA-256 | Build | Part B `tool` | Same build gives the same value. A different build or platform may give a different result, so it belongs in the claim |
| Settings snapshot | Pure input | Part B `settings` | D-073 |
| Input SHA-256 and size | Pure input | Part B `input` | Content, not location |
| Questions and answers, with source | Pure input | Part B `interactions` | Answers are an input to the repair |
| Basis of every font slot: name match, auto-accept scores, answer | Pure output | Part B `font_decisions` | The engine computes it from the input, settings and answers |
| Findings, passes, actions, grades, salvage edits | Pure output | Part B | Engine output |
| Product hashes: repaired PDF, `.md`, image files | Pure output | Part B `products` | Bytes are a function of the four inputs, D-041 and D-060 |
| Image provenance records | Pure output | Part B `images` and `images.json` | Same |
| Limitations and fixed statements | Pure output | Part B | Text templates filled from counts |
| Tool binary SHA-256 | Build and platform | Part A `host` | Differs per platform for one commit |
| UTC start and end, host offset | Run | Part A `clock` | Wall clock. D-076 keeps it out of pure artefacts |
| Host name, OS | Run | Part A `host` | Machine-specific |
| Case reference, examiner, notes | Run | Part A `case` | D-020: UI-only, never in the engine |
| Input file name, path, before and after hashes | Run | Part A `input_file` | Name and path are location. MD5 and SHA-1 depend on a custody setting |
| Input's file-system state before and after: times, size, read-only | Run | Part A `input_file.stat_*` | Disk state, never content |
| Destination folder, which custody mode requires | Run | Part A `destination` | Location. `output_dir` is not a Part B setting |
| When each answer was asked and given | Run | Part A `answer_times` | Wall clock |
| Earlier run whose answers were replayed | Run | Part A `answers_replayed_from` | Names another run, so it cannot be in Part B |
| Placed paths, placement tier, re-read hashes | Run | Part A `placed` | Disk-dependent, D-073 |
| Analysis state reused or rebuilt | Run | Part A `processing` | D-073 |
| Log path, log identity, sequence number, previous entry hash | Run | Part A `log` | Changes every run |
| Custody settings: hashes, prompt, host name, CASE | Run | Part A `custody_settings` | They do not change the repair |

### 3.3 How a second examiner checks Part B

1. Re-run PDFPundit of the same build, on the same platform, on the same input, with the recorded settings and answers. The Verify action does this.
2. Compare the new `reproducible_sha256` with the recorded one.
3. Without PDFPundit, check the recorded value is honest: `jq -cj .reproducible x.custody.json | shasum -a 256`.

Compact form is defined as serde_json's default compact writer: no whitespace, keys in struct order, integers only. Every string in Part B is printable ASCII, 0x20 to 0x7E, per 4.1. Under that rule Python's `json.dumps(v, ensure_ascii=False, separators=(",", ":"))` and jq give the same bytes, because the only escapes left are `\"` and `\\`, which all three write alike. Tested with jq 1.8.2 on the example and both fixtures, `validate.py` check 9. Other jq versions are not tested.

The ASCII rule is needed, not cosmetic. jq writes U+007F as `\u007f`, while serde_json and Python write the raw byte. Other control and non-ASCII characters differ the same way. `work/ascii_fixture.py` proves it: a slot name holding 0x7F and non-UTF-8 bytes gives different jq and Python hashes when stored raw, and the same hash once escaped. jq 1.6 and earlier convert integers above 2^53 to floats, so the schema keeps every integer below 2^53.

## 4. Custody record schema, version 1

### 4.1 Conventions

| Rule | Value |
|---|---|
| Encoding | UTF-8 JSON, LF, written by serde_json's pretty writer with two-space indent, trailing newline |
| Key order | Fixed, as listed in this document. Derived structs, never maps with runtime keys |
| Absent fields | None. Every field is always present. "Optional" means the value may be `null` |
| Numbers | Integers only, below 2^53. No floats anywhere. Ratios are `{"num", "den"}` |
| Hashes | Lowercase hex strings. SHA-256 is 64 chars, SHA-1 40, MD5 32 |
| Byte offsets | Into the input file, from 0. Spans are half-open `{"start", "end"}`. Two exceptions: salvage edit offsets, next row, and image spans in a decoded content stream, 6.2 |
| Edit offsets | Relative to the start of the stream data, in the domain `edit_domain` names. `raw_stream_data` means the bytes as stored, so file offset = stream start + offset. `flate_input` means after the filters before `/FlateDecode` were decoded, as `salvage.rs` works; `filters_before` lists them |
| Object references | Strings `"num gen"`, for example `"14 0"`. Declared numbers from the input unless the field says output. `findings_after` is a diagnosis of the output, so its objects and spans are the output's |
| Pages | Physical position, from 1, in the page order as rebuilt: the page tree's pages in tree order, then any pages appended under D-112. `/PageLabels` are never used, so page "iii" of a preface is page 3. `page_in` is the position in the input's page tree and is null for an appended page. `page_out` and every other `page` field use the rebuilt order, which is also the output's order. When `text.rs` found pages by scanning, its file order is never used for a page number here, and a this-run sentence says counts were taken that way, 12.3 |
| Lengths and areas | `size_bytes`, `*_len` in bytes. `bbox_mpt` in milli-points, `1/1000` of a PDF point |
| Matrices | Six integers: `a b c d` times 10^6, `e f` in micro-points. Rounded with integer arithmetic, never libm |
| Times | `YYYY-MM-DDThh:mm:ssZ`, UTC, whole seconds. Only in Part A and the log |
| Enum values | snake_case strings. Engine enums are mapped, for example `BundledThenSystem` to `bundled_then_system` |
| PDF names and dictionaries | Kept as PDF text, for example `/DeviceRGB` or `<</Predictor 15>>`. Canonical form: keys sorted, single spaces. Name bytes outside 0x21 to 0x7E, `#` and the PDF delimiters are written `#xx`, the PDF's own name escape. So `/F` followed by 0x7F is `/F#7F` |
| Part B strings | Printable ASCII only, 0x20 to 0x7E. Input-derived text that is not a PDF name, such as a `/Version` value or an engine reason quoting the file, writes other bytes as `\xHH`. Engine-written text must be ASCII already: `>=`, never the Unicode sign. The schema's string pattern enforces this |
| File times | `{"sec", "nsec"}`: seconds and nanoseconds since 1970-01-01 UTC, from the OS as raw integers through `rustix` or `windows-sys`. Windows FILETIME is converted with integer arithmetic. Never `std::time` |
| User text | Case reference, examiner, submission reference and notes are stored as typed, in Part A only. The prompt refuses control characters and caps each at 200 characters. Blank prints as "not provided" |

### 4.2 Top level

| Field | Type | Null allowed | Notes |
|---|---|---|---|
| `format` | const `"pdfpundit-custody"` | No | Lets a reader recognise the file |
| `custody_schema_version` | integer, const `1` | No | Bumped on any shape change. See 4.6 |
| `reproducible_sha256` | hex64 | Yes, when `reproducible` is null | SHA-256 of `reproducible` in compact form |
| `reproducible` | object, 4.3 | Yes, when the engine never ran | Part B |
| `run` | object, 4.4 | No | Part A |

### 4.3 `reproducible`, Part B

Members appear in this order.

| Field | Type | Notes |
|---|---|---|
| `job` | enum `repair`, `repair_and_export`, `export` | Which products the job made. A re-run must use the same job kind |
| `tool` | object | `name` const `PDFPundit`; `version` string; `build_commit` hex40 or null; `build_origin` enum `release`, `local`, `unknown`; `platform` `{os, arch}`, `os` enum `macos`, `windows`, `linux`, `arch` enum `aarch64`, `x86_64`, see 3.1; `report_schema_version` integer; `font_db_sha256` hex64. See 4.5 for what `build_origin` can and cannot show |
| `settings` | object | The `SettingsSnapshot` fields, names unchanged, plus `extract_type3_glyph_images`, `prompt_unresolved` and `derived_image_views` booleans and `font_source_effective` enum `bundled`, `bundled_then_system`. `passes` is null for all classes or a list of `C1`..`C10`. See 3.1 |
| `input` | object | `sha256` hex64; `size_bytes`; `pdf_version` string or null; `pages_discovered`; `signed_fields` integer, 0 when unsigned; `encrypted` boolean |
| `interactions` | array of 7.1 | In the order asked |
| `font_decisions` | array of 7.3 | One per font slot the repair touched, in rebuilt page order, then slot name bytes |
| `outcome` | object | `status` enum `repaired`, `partial`, `clean`, `failed`, `encrypted`; `reasons` array of strings, the engine's partial or failure reasons; `chosen_toolpath` enum `resave`, `template_assemble` or null |
| `findings_before`, `findings_after` | arrays of finding | Finding: `id`, as diagnose numbers it, `C9-001` or `NOEMBED-001`; `kind` enum `corruption`, `encrypted`, `signed`, `outlined_text`, `type3_text`, `font_not_embedded`, `pages_outside_tree`, one per `FindingKind` variant; `class` `C1`..`C10` for `corruption`, else null; `severity` enum `info`, `warning`, `error`; `location`; `summary`, the engine's text; `detail`, the variant's payload, below |
| `location` | tagged object | `{"type":"file"}`, `{"type":"span","span"}`, `{"type":"object","object","span"}` or `{"type":"page","page","object"}`. `page` counts from 1, 4.5 |
| `passes` | array | `class`; `outcome` enum `fixed`, `partial`, `skipped`; `note` string or null, the `Partial` or `Skipped` reason; `actions` array of `{object, what, grade}`. `object` is null for a file-wide action. `what` is the engine's text. `grade` is set for every C9 action that has one |
| `candidates` | array | `toolpath`; `chosen` boolean; `verification` with `baseline`, `gates`, `retention`, `plausibility`: `verify.rs`'s `v0`, `v1`, `v2` under their meanings, fields unchanged, ratios as `{num, den}` |
| `stream_salvage` | object | `summary`: `streams_damaged`, `repaired`, `exact`, `accepted`, `ambiguous`, `checksum_mismatch`, `prefix`, `unrecoverable`, `unsearched`. `streams_damaged` is the sum of `repaired`, `checksum_mismatch`, `prefix`, `unrecoverable` and `unsearched`, and `repaired` is the sum of the three grades. `streams` array of `{object, search}`, one per damaged stream, `search` below; `filter_rewrites` array of `{object, before, after}` |
| `shadows` | array | `{object, span}`: earlier copies of an object number that lost to a later copy |
| `reals_narrowed` | integer | D-075 |
| `text_warnings` | array | `{source, warning}`: `source` enum `input`, `output`, the input baseline or the chosen output; `warning` enum `hidden_layers`, `scanned_pages`, `text.rs`'s two file-dependent warnings. Each gives a this-run sentence, 12.3 |
| `products` | array | `{role, file, sha256, size_bytes}`. `role` enum `repaired_pdf`, `markdown`, `image`, `image_view`, `images_index`, `images_checksums`. `file` is the name inside the images folder, or null for the PDF and `.md` because their names come from the input's file name |
| `images` | object | `dir` the images folder name `<hash8>.images` or null; `type3_glyph_images` count; `records` array of 6.2 |
| `limitations` | object | `standing` and `this_run`, arrays of fixed sentences. Section 12 |
| `statements` | object | `role`, `reproducibility`, `validation`, `tool_identity`, `input_file`, `case_details`, `times`, `log`, `legacy_hashes`. Fixed text per schema version. Section 12 |

Each `stream_salvage.streams` entry's `search` records what happened to one damaged stream, enough to check the grade's claim. An image drawn from that stream carries the same object as `integrity.salvage`. Every value comes from `salvage.rs` and `salvage/ttf.rs`; 12.3 gives the search rules in words.

| Field | Type | Notes |
|---|---|---|
| `outcome` | enum `repaired`, `checksum_mismatch`, `prefix`, `unrecoverable`, `unsearched` | The `Salvage` variant, `Clean` aside. `checksum_mismatch`: decoded to the end, Adler-32 wrong, no repair found; or a repair whose output held an invalid predictor row, cut to the rows before it (`entry_of`). `prefix`: only the bytes before the damage decode |
| `grade` | enum `exact`, `accepted`, `ambiguous`, `unsearched`, or null | Set for `repaired` and `unsearched` only, as `Salvage::grade` does. Null for the rest, which is why they have their own counters and image status |
| `adler_rerun` | boolean | The localizer's window gave no accept and the search widened under Adler-32 alone. For a font: the font's own table checksum was wrong |
| `trailer_edit` | boolean | The chosen repair rewrote a byte of the stored Adler-32 trailer, not the data. The decoded data is the damaged stream's own, and no checksum covers it: the trailer was made to fit it. 12.3 says so whatever the grade |
| `predictor_stopped` | boolean | The PNG predictor stopped at an invalid row, so the data ends there |
| `prefix` | object or null | For `prefix`: `in_used` and `in_total`, the input bytes that decoded and the stream's length |
| `edit_domain` | object | `kind` enum `raw_stream_data`, `flate_input`; `filters_before` array of filter names. See 4.1 |
| `passes` | array of pass | One per search pass, in run order. Empty when nothing was searched |
| `passing_edits` | integer | Single-byte replacements that passed the check, in the last pass. Earlier passes ended with none, or the search would have stopped there. Two different edits can give the same output |
| `distinct_outputs` | integer | Different outputs among those, told apart by the SHA-256 of the output bytes, as `salvage.rs` does. Exact and Accepted need exactly 1 |
| `edits` | array of `[offset, from, to]` | The chosen output's edits, offsets in the edit domain, first in the pinned order: body edits before trailer edits, then lowest offset, then lowest replacement byte. Empty unless `repaired` |

A pass is one sweep over one window. `salvage.rs` starts a new one in three places: the first window, the widening after a localizer's window gave nothing (`Pending::exhausted`), and the move from zlib to raw deflate after a rejected header. Each starts its own candidate count (`hunt.pass_start`). The deep draw does not start a pass: it extends the window of the pass it lands in, and the count goes on.

| Pass field | Type | Notes |
|---|---|---|
| `mode` | enum `zlib`, `raw_deflate` | |
| `damage` | object | `kind` enum `inflate_error`, `adler_mismatch`, `header_rejected`; `at`, the offset in the edit domain where the inflate failed, or where the Adler-32 trailer starts. The raw-deflate pass reads the damage again |
| `localizer` | enum `error_window`, `adler_only`, `ttf_tables`, `widened` | Which rule set the window. `error_window`: from 4,096 bytes before `at` to 8 after. `adler_only`: from byte 2 to the trailer. `ttf_tables`: from the sfnt table directory. `widened`: the localizer's positions it had judged with its table check, then the rest of the ladder's own window |
| `check` | enum `adler32`, `adler32_after_ttf_table_checksum`, `none_raw_deflate` | What a candidate had to pass. With `ttf_tables`, a candidate past the directory is first rejected unless the failing table's checksum passes, then must pass Adler-32. Raw deflate has no Adler-32, so its best grade is Ambiguous |
| `deep` | boolean | The pass ran into the deep draw. Its window is then the deep window: for `error_window`, the first window plus 16,384 bytes before the error less the 4,096 already in it |
| `window` | array of spans | The window in force when the pass ended, in the edit domain, in search order |
| `candidates_in_window` | integer | 255 per window byte |
| `candidates_tried` | integer | Candidates this pass tried. Never summed across passes: a widened pass tries some positions an earlier pass judged, under a different check |

Exact needs the last pass's tried to equal its in-window count. `validate.py` check 13 tests that, the error-window arithmetic per pass, and that the counters add up.

`detail`, a finding's payload. Every value is from the `FindingKind` variant; names are PDF names, escaped per 4.1:

| `kind` | `detail` |
|---|---|
| `corruption`, `encrypted` | null |
| `signed` | `{type, fields}`: the signature fields found. `input.signed_fields` is the same number |
| `outlined_text` | `{type, glyph_runs, contours}` |
| `type3_text` | `{type, font}`, an objref |
| `font_not_embedded` | `{type, font, base_font}`: the font a page selects, and its name without a subset tag, empty when it has none |
| `pages_outside_tree` | `{type, pages}`: the count appended under D-112. Each page's object is in the engine's evidence, and its position in `page_out` |

The `.md` product hash is the hash of the Markdown as produced with the canonical folder name `<hash8>.images`. If a clash renamed the folder, the placed `.md` differs and Part A says so.

### 4.4 `run`, Part A

| Field | Type | Notes |
|---|---|---|
| `job_status` | enum `completed`, `cancelled`, `failed_before_engine`, `input_changed`, `record_only` | `input_changed` is the parked-job `INPUT_CHANGED` failure. `record_only` is a file the batch listed but never queued |
| `batch` | object | `log_seq` of the `batch_opened` entry; `position`, `count`: file n of N |
| `case` | object | `prompt_shown` boolean; `case_reference`, `examiner`, `submission_reference`, `notes`: string or null. Null prints as "not provided". None is verified, and the `case_details` statement says so. All four are typed once per batch and apply to every file in it. The `.txt` prints `submission_reference` as "applies to all N files of this batch". The item's unique identifier is the input SHA-256, not a typed field |
| `clock` | object | `started_utc`, read just before the job's first stat of the input, which for a `drop` input comes after its bytes were received, 2.3.1; `ended_utc`, read just after its second stat and before the record files are written. 2.3 gives the order. `host_utc_offset_minutes`, the host's offset at `started_utc`, filled on every platform, decision Q7, and null only when the zone source is missing or does not parse; `host_utc_offset_from` enum `etc_localtime_tzif`, `windows_local_file_time`, or null with the offset; `host_zone_hint` `{name, from}` or null, `from` enum `etc_localtime_link`, `windows_time_zone_info`; `source` const `host_clock_unverified`. The hint is a name only and is printed as unverified. `TZ` is never read: plan section 3.1 does not list it, and `clock.rs` ignores it too. 5.5 says how the offset is read |
| `answer_times` | array | `{seq, shown_utc, answered_utc}` for every interaction answered in this run, keyed by its `seq` in Part B. `shown_utc` is when the question reached the screen, and is null for a `batched` or `policy` answer, which nobody saw. `answered_utc` is when the answer was given, by the user or by the app. Replayed answers have no entry. NIST SP 800-86 3.1.2 asks at what time each action was done |
| `host` | object | `name` string or null, null when `record_host_name` is off; `os` enum `macos`, `windows`, `linux`; `tool_binary_sha256` hex64 or null |
| `custody_settings` | object | `hashes` effective list, always starts with `sha256`; `ask_case_details`; `require_case_details`, which refuses an empty case reference at the prompt, off by default (UX decision Q27); `record_host_name`, on by default, decision Q3; `case_jsonld`, off by default, decision Q5 |
| `input_file` | object | The input as received by PDFPundit. The tool cannot tell whether it is the original evidence or a working copy, so it never calls it the original. `name`; `path` or null; `path_kind` enum `file`, `dropped`, `memory`, one per `JobInput` variant, 2.3.1; `first_read` enum `job_start`, `drop`; `size_bytes`; `hashes_before`, null only for a job cancelled before it read the input, 2.3.2; `hashes_after` or null; `unchanged` enum `yes`, `no`, `not_rechecked`; `not_rechecked_reason` or null; `stat_before`, `stat_after` file stat or null; `stat_changed` array naming every stat field that differs |
| `hashes` | object | `sha256` hex64; `sha1` hex40 or null; `md5` hex32 or null. Null means the algorithm is not in `[custody] hashes` |
| `destination` | object | `path`: the `[general] output_dir` folder, absolute. Custody mode starts only with it set and never equal to an input's folder, 2.4, decision Q16, so there is no "beside the input" case to record |
| `placed` | array | `role` as in products; `name` relative to the record's folder; `path` absolute or null; `size_bytes`; `hashes`; `placement` enum `atomic`, `linked`, `claimed_then_renamed`, `create_new`, `reused_identical` or null; `matches_product` boolean; `note` or null |
| `images_dir` | object or null | `path`; `name`; `outcome` enum `created`, `reused_identical`, `renamed_after_clash` |
| `processing` | object | `analysis_state` enum `reused`, `rebuilt`; `rebuilt_reason` or null; `config_path` or null |
| `record_files` | object | `json`, `text`, `checksums`: the names claimed in 2.3; `case_jsonld` name or null |
| `earlier_records` | array of record ref | Every earlier `record` entry with the same input SHA-256, in this log and in each log before it, followed back through `log_continued.previous_log_id`. SWGDE 18-Q-002 section 6: a new record names the earlier one |
| `earlier_records_logs` | object | `searched`: the `log_id`s searched, this log first; `missing`: the `log_id` of a predecessor whose file was not found, where the search stopped, or null |
| `answers_replayed_from` | tagged object, never null | `{type: none}` when no answer was replayed. Otherwise `custody_record` or `run_without_record`, each with `seqs`, the interactions replayed. `run_without_record.started_utc` may be null. See 7.4 |
| `log` | object | `path`; `log_id`, the SHA-256 of the log file's first line; `seq` of this record's own entry; `prev_entry_sha256`, the SHA-256 of the entry before it |

A record ref is `{record, sha256, log_id, log_seq}`: the record's file name, its `.json` SHA-256, and the log and entry that hold it. `log_id` keeps entry numbers unambiguous across `custody-<n>.log` continuations, because each file restarts its own chain.

A file stat, `stat_before` and `stat_after`, null for a `memory` input or a path that no longer exists:

| Field | Type | Notes |
|---|---|---|
| `size_bytes` | integer | From the stat, not the read |
| `modified` | file time | Unix `st_mtime`; Windows `LastWriteTime` |
| `changed` | file time or null | Unix `st_ctime`; Windows `ChangeTime` from `FILE_BASIC_INFO` |
| `accessed` | file time or null | Unix `st_atime`; Windows `LastAccessTime`. PDFPundit's own read may update it |
| `created` | file time or null | macOS `st_birthtime`; Linux `statx` birth time when the file system has it; Windows `CreationTime` |
| `read_only` | boolean | No write bits for anyone on Unix; `FILE_ATTRIBUTE_READONLY` on Windows |
| `unix_mode` | integer or null | Raw `st_mode`; null on Windows |
| `source` | enum `unix_stat`, `windows_file_info` | Which call produced it |

### 4.5 Mapping from today's engine types

| Custody field | Comes from | Change needed |
|---|---|---|
| `tool.version`, `font_db_sha256` | `RepairReport.engine_version`, `font_db_sha256` | Hex conversion only |
| `tool.build_commit` | New: `option_env!("PDFPUNDIT_BUILD_COMMIT")`. Today nothing sets it: `v-release.yml` must, see section 14 | Null when unset. No `build.rs`, no `RUSTFLAGS`, no date embedded |
| `tool.build_origin` | New: `option_env!("PDFPUNDIT_BUILD_ORIGIN")`, set to `release` by the release workflow only | `release` when that value is present; `local` when a commit is set without it; `unknown` when neither is set |
| `settings` | `RepairReport.settings` plus the runner's config | Add `extract_type3_glyph_images` to `SettingsSnapshot`. `prompt_unresolved` is `[fonts] prompt_unresolved`. `derived_image_views` is a new `[repair]` key, false by default, decision Q6. `font_source_effective` is `bundled` whenever `config.rs` raised `SystemFontsNotBuilt` |
| `interactions` | `RepairReport.interactions` | `InteractionSource` is `User`, `Policy`, `UseBest`, `Batched` since G-03. Mapped per 7.2: `UseBest` is `user` with reply `use_best`. New: `carried_from` and `policy_setting` on `Answer` and `InteractionRecord`, section 14 item 1 |
| `font_decisions` | New `RepairReport.font_decisions`, filled in `fonts.rs` where `decide` returns | Today the basis of a `Named` or `AutoAccept` decision goes only to the log sink as text (`fonts.rs` 182-185, `fontdb/score.rs` 262-286, 341). The `Candidate`, the `InferenceTrace`, the slot's `ToUnicodeState`, the best coverage and the thresholds must be kept as data |
| `stream_salvage.streams[].search` | `Salvage::Repaired` already has `grade`, `edits`, `survivors` (one edit list per distinct output), `trailer_edit` and `adler_rerun`; `Prefix` has `in_used`, `in_total` | New: one pass record per `Pending::start`, with `mode`, `damage`, `localizer`, `check`, `deep`, the window and the pass's tried count; `passing_edits` from `hunt.accepted`; `predictor_stopped` from `entry_of`. Today these stay inside `Pending`. `outcome` is the variant |
| `stream_salvage.summary` | `C9Summary` | New counters `checksum_mismatch` and `prefix`. Today `repair.rs` counts both into `streams_damaged` only, so the counters do not add up |
| `findings_*` | `Finding` | `class: FindingKind` becomes `kind`, `class` and `detail`. `evidence` and `repair` are not mapped. A name in `summary` or `detail` is escaped per 4.1 from the name's bytes, which needs the raw bytes kept: today `diagnose.rs` builds the summary with `from_utf8_lossy` |
| `passes`, `candidates`, `shadows`, `reals_narrowed` | Same-named `RepairReport` fields | View mapping, with the renames below |
| `text_warnings` | `PageText.warnings` from `text.rs`, for the baseline and the chosen output | New: the report keeps which of the two warnings fired, section 14 item 33 |
| `outcome` | `OutcomeStatus` plus whether an output was written | `clean` is `Ok` with no output, D-104 |
| `products` | New: hash of each byte buffer the job hands to placement | Computed in the runner from `RepairOutcome` bytes |
| `images.records` | `RepairReport` v2 `images: Vec<ImageRecord>`, replacing `extracted_images` | `SCHEMA_VERSION` 1 to 2; regenerate `tests/data/contract/` |
| `limitations.standing` | `text.rs` `KNOWN_LIMITS` plus two image and repair sentences | D-038 said no report quotes them yet. This one does |
| All of Part A | `RunRecord` plus new custody fields | `RunRecord` gains fields with serde defaults, or the store moves to version 2 |

Renames and conversions the view makes. Each is a fact of today's types that a reader of the engine's JSON would otherwise trip on:

| Engine value | Custody value | Why |
|---|---|---|
| `Verification.v0`, `v1`, `v2` | `gates`, `retention`, `plausibility` | Named by meaning. Fields inside are unchanged |
| `RepairAction.object == (0, 0)`, the `FILE` sentinel in `repair.rs` | `object: null` | `0 0` is a real-looking objref. Null says file-wide |
| `Location::Page { index }`, counted from 0 | `{"type":"page","page"}`, counted from 1 | 4.1: every page number counts from 1 |
| `Location::Object { id }` | `object` | Field name only |
| `FindingKind` variant with fields | `kind` plus `detail` | 4.3 |
| `PassOutcome::Partial(s)`, `Skipped(s)` | `outcome` plus `note` | A tagged enum as two fields |
| `[u8; 32]` hashes | hex strings | 4.1 |
| `Ratio`, `ObjId` | `{num, den}`, `"num gen"` | 4.1 |
| `SettingsSnapshot.font_policy` `BundledThenSystem` | `bundled_then_system` | snake_case enums |

What `build_commit` and `build_origin` can show is limited. Both come from environment variables at build time, so anyone building from source can set them to anything. A local build from a dirty tree cannot be detected without a `build.rs`, which this design does not add. The only check an examiner can make is the binary hash: `run.host.tool_binary_sha256` against the per-executable SHA-256 the release publishes. The `tool_identity` statement says all of this in every record.

`RepairReport` keeps its number-array hashes. Its contract snapshot is frozen, and the custody file is a view, so only the view uses hex.

### 4.6 Versioning

| Change | Action |
|---|---|
| Add, remove, rename or retype any field | Bump `custody_schema_version`. Ship the new schema file in `docs/` |
| Change any fixed statement or limitation sentence | Bump. Part B hashes change, so re-runs on the new version differ, and the record must say why |
| Add an enum value | Bump |
| `RepairReport` `SCHEMA_VERSION` changes | Recorded in `tool.report_schema_version`. Bump custody too if the view changes |

Readers must refuse a `custody_schema_version` they do not know. A newer record is kept and listed, never parsed with a guess. This matches the history store's rule for newer runs.

## 5. Hashes

### 5.1 What is hashed, with what

| Item | SHA-256 | MD5, SHA-1 | Where recorded |
|---|---|---|---|
| Input, before the run | Always | Per setting | `run.input_file.hashes_before`; SHA-256 also in `reproducible.input` |
| Input, after the run | Always | Per setting | `run.input_file.hashes_after` |
| Each placed file, re-read from disk | Always | Per setting | `run.placed[].hashes` |
| Each product, in memory | Always | Never | `reproducible.products` |
| Image raw stream, decoded samples, repaired stream, salvage outputs | Always | Never | Image record; `salvage.rs` tells outputs apart by SHA-256 |
| Record files | Always | Never | Log entry, `.custody.sha256` |
| Log lines | Always | Never | Next line's `prev` |
| Tool binary | Always | Never | `run.host.tool_binary_sha256`. Compared by the examiner with the published per-executable SHA-256, once releases publish it |

### 5.2 The rules

- SHA-256 is always on. A config of `hashes = ["md5"]` yields an effective `["sha256", "md5"]`. The setup screen shows SHA-256 as locked on.
- MD5 and SHA-1 appear only in Part A. They depend on a custody setting, and Part B must not.
- Legacy hashes cover the input and the placed files only. Those are what a case system indexes.

### 5.3 Why legacy hashes are included

- Older case systems and evidence logs index exhibits by MD5 or SHA-1. An examiner whose exhibit was logged by MD5 at acquisition needs that MD5 to show it is the same file. M6b asks for them "for older case systems".
- NIST SP 800-86, section 4.2.2, names only MD5 and SHA-1. Examiners trained on it still expect them.
- Neither is collision-resistant, so neither is used for integrity anywhere in PDFPundit. Every legacy hash is labelled "legacy" in the `.txt`, and the `legacy_hashes` statement says so.

### 5.4 Dependencies

| Crate | State today | Needed for | Gate check |
|---|---|---|---|
| `sha2` 0.11 | Direct dependency | SHA-256 | none |
| `sha1` 0.11 | Dev-dependency only | SHA-1, and UUIDv5 for CASE | Move to normal deps; `cargo deny check licenses`; purity gate |
| `md-5` 0.11 | In `Cargo.lock` through `lopdf` | MD5 | Add as a direct dep; same checks |

Both are pure-Rust RustCrypto crates under MIT or Apache-2.0, which `deny.toml` allows.

### 5.5 Reading the UTC offset

Decision Q7 fills the offset on every platform. It reuses code the app already has, so no new time-zone crate is needed.

| Platform | How | Null when |
|---|---|---|
| macOS, Linux | Read `/etc/localtime` and pass it, with `started_utc` as Unix seconds, to `tzif_offset` in `src/ui/app/clock.rs`: the app's TZif reader (RFC 8536, with the POSIX-TZ footer for times after the last transition). It returns seconds east of UTC; the record stores minutes | The file is missing or does not parse. `local_hm` falls back to 0 for the status bar; the custody path must not, and records null |
| Windows | `GetSystemTimeAsFileTime`, then `FileTimeToLocalFileTime` on that same value; the difference, rounded to whole minutes, is the offset at that instant. Section 14 item 6 | A call fails |

Today `tzif_offset` is private to `clock.rs`, and `clock.rs` says "nothing here reaches an artefact". Section 14 item 36 makes it `pub(crate)` and amends that comment: the offset reaches Part A of a custody record, never Part B, so D-073 and D-076 still hold. The offset is read for the instant `started_utc`, so a run that crosses a daylight-saving change still has one well-defined offset.

## 6. Image extraction record

### 6.1 What is recorded in each mode

| Mode and job | Image records | Image files | `images.json`, `SHA256SUMS` | Custody record |
|---|---|---|---|---|
| Custody off, repair only | In `RepairReport` v2, so in history | Not written | Not written | None |
| Custody off, Markdown export | In `RepairReport` v2 | Written, linked from the `.md` | Written | None |
| Custody on, repair | In `RepairReport` v2 and Part B | Written | Written | Lists every image |
| Custody on, export | Same | Written | Written | Same |

Custody mode writes the files on a plain repair because D-114 says images feed the evidence report when custody is on. A record of a file that does not exist would prove nothing. A plain repair outside custody writes no image files, decision Q4: the records go to history, and files appear on export or in custody mode.

### 6.2 Image record fields

One record per source. Records are never merged, even when two sources share bytes.

| Group | Field | Type | Notes |
|---|---|---|---|
| Identity | `id` | `img-NNNN` | Ordinal in record order. Local to this record; used by `parent` |
| | `file` | string or null | `<hex16>.<ext>`, or `<hex16>.<status>.<ext>` when the file holds samples PDFPundit changed, cut short or could not verify, 6.4. Null when nothing was written: over cap, glyph images not extracted |
| | `file_sha256`, `file_len` | hex64, integer, or null | The extracted file |
| | `format` | enum `jpg`, `jp2`, `j2k`, `png`, `tif`, `ccitt`, `jb2e`, `jb2g`, `bin`, `none` | From 6.3 |
| | `view_file`, `view_sha256` | string or null | A derived viewable PNG for JPX, CCITT, JBIG2. Null unless `[repair] derived_image_views` is on, which it is not by default, decision Q6. Never the evidence file |
| Source | `kind` | enum `xobject`, `inline`, `smask`, `mask`, `stencil`, `thumb`, `jbig2_globals` | |
| | `copy` | enum `winner`, `shadow`, `headerless`, `unreferenced` | A shadow is an earlier copy of the object number, direct evidence of an incremental change |
| | `object` | objref or null | As declared in the input. Null for inline and headerless images |
| | `output_object` | objref or null | Number in the repaired PDF, through `IdRemap`. Without it, the record cannot be joined to the output |
| | `parent` | `img-NNNN` or null | For masks: the image they belong to |
| | `inline_origin` | object or null | `stream_object`, the content stream holding the `BI`; `bi_ordinal`, its count from 0 in that stream; `bi_offset`, the offset of `BI` in that stream's decoded bytes, after all its filters. For an unfiltered stream that equals the offset from the start of its stream data |
| | `raw` | tagged object | Where the image's bytes are, as found and still filtered. `domain` enum `file` or `decoded_content_stream`; `span` in that domain; `container_object` and `container_file_span`, the containing stream and its raw bytes in the input file, both null in the `file` domain. See below |
| | `raw_len`, `raw_sha256` | integer, hex64 | Length and SHA-256 of the bytes `raw` points at |
| | `length_source` | enum `declared`, `inflate_probe`, `scanned_endstream`, `truncated_at_eof` | How the stream end was decided |
| Encoding | `filters` | array of strings | Full PDF names, abbreviations expanded, in order |
| | `decode_parms` | array of string or null | One per filter, canonical PDF text |
| | `codec` | enum `jpeg`, `jpeg2000`, `ccitt`, `jbig2`, or null | Null for Flate, LZW, RunLength or raw rasters |
| | `jbig2_globals` | `img-NNNN` or null | The globals record |
| Description | `width`, `height` | integer or null | From the dictionary |
| | `actual_width`, `actual_height` | integer or null | From the codestream header or the decoded row count. A mismatch is a warning |
| | `bits_per_component`, `components`, `colour_space` | integer, integer, PDF text | Recorded, never applied |
| | `icc` | `{object, sha256}` or null | ICC profile reference |
| | `palette_entries` | integer or null | Indexed only |
| | `decode`, `image_mask`, `interpolate`, `intent` | PDF text or boolean | Recorded verbatim, applied never |
| | `needs_pdf_context` | boolean | True when the file alone displays wrongly: CMYK JPEG with `/Decode`, JBIG2 needing globals |
| Placement | `placements` | array | `page_in`, `page_out`, `paint_order`, `bbox_mpt`, `matrix_micro`, `form_chain`. Empty means never painted. Page numbers follow 4.1: `page_in` is null on a page appended under D-112 |
| Integrity | `status` | enum `intact`, `repaired`, `partial`, `truncated`, `unverified`, `undecoded`, `unrecoverable`, `over_cap`, `no_dimensions` | From the stream's salvage `outcome`: `repaired` gives `repaired`, `checksum_mismatch` gives `unverified`, `prefix` gives `partial`. `truncated` is a stream cut by the end of the file |
| | `rows_written`, `rows_declared` | integer or null | Rasters only. Partial images keep whole rows, never padding, per D-128 |
| | `salvage` | object or null | The stream's salvage record, the same object as its `stream_salvage` entry, 4.3: outcome, grade, passes, `adler_rerun`, `trailer_edit`, edit domain, `passing_edits`, `distinct_outputs`, edits. Null when the stream was undamaged |
| | `repaired_stream_sha256` | hex64 or null | The stream bytes after repair, as written into the output |
| | `samples_sha256` | hex64 or null | Decoded sample rows. Flate, LZW and raw rasters only, because they do not depend on a decoder. Never for DCT or JPX |
| | `warnings` | array of strings | From a fixed set: dimension mismatch, trailing bytes, no JPEG end marker, palette missing |

Span domains. An image XObject, mask or thumbnail is a stream of its own, so its bytes are a span of the input file: domain `file`. An inline image (`BI ... ID ... EI`) and a Type3 glyph bitmap sit inside a content stream. When that stream has no filters, the bytes are still a span of the file, and the domain is `file`. When it is compressed, the bytes exist only after decoding it: domain `decoded_content_stream`, `span` is in the decoded bytes, and `container_file_span` gives the compressed stream in the file. Only the `file` domain can be checked against the input with `dd` and `shasum` without trusting PDFPundit. For the other domain, a reader decodes the container first.

`bbox_mpt` is `[x0, y0, x1, y1]` in the page's default user space, after the paint-time matrix, before `/Rotate`. A rotated image gets the bounding box of the four mapped corners.

Why two or three hashes per image:

- `raw_sha256` matches the bytes `raw` points at in the input. It proves where the image came from.
- `file_sha256` matches the extracted file. It proves the file was not altered.
- They are equal only for a single image-codec filter with no repair, like the example's JPEG. A Flate wrapper or a repair makes them differ, and `repaired_stream_sha256` then shows the bytes after repair.

### 6.3 Evidence file form

Rule: the file holds the image as stored when a viewer can read it that way: JPEG, JPEG 2000, CCITT and JBIG2 bytes are copied verbatim. Otherwise the file holds the decoded samples, re-encoded by PDFPundit's own encoder as PNG or TIFF with the same sample values. `samples_sha256` lets a reader check that: decode the file, hash the rows, compare. Colour, `/Decode` and masks are never applied. An image whose stream was repaired holds the samples after PDFPundit's byte edits; one whose stream was cut short or failed its check holds the samples that decoded. Its name and record say which.

So `file_sha256` equals `raw_sha256` only for a verbatim copy. A PNG or TIFF is a new encoding, and its bytes depend on PDFPundit's encoder at this build. `samples_sha256` is the hash that compares across tools.

| Last filter | File | Format value | Viewable in Markdown |
|---|---|---|---|
| `DCTDecode` | JPEG bytes, verbatim | `jpg` | Yes |
| `JPXDecode` | Verbatim. `.jp2` when it starts with the JP2 box, `.j2k` for a bare codestream | `jp2`, `j2k` | No |
| `CCITTFaxDecode` | Verbatim. Parameters in `decode_parms` | `ccitt` | No |
| `JBIG2Decode` | Embedded stream verbatim; globals as their own record | `jb2e`, `jb2g` | No |
| Flate, LZW, RunLength, ASCII, or none | Sample-exact PNG, fixed deflate level | `png` | Yes |
| Raster in CMYK, Separation, DeviceN, Lab | Uncompressed TIFF, raw samples | `tif` | No |
| Unknown filter or undecodable | Raw stream bytes | `bin` | No |

### 6.4 Naming and deduplication

| Rule | Detail |
|---|---|
| File name | `<first 16 hex of file_sha256>.<format>`. Independent of renumbering, page order and the input's name, per D-060 and D-136 |
| Status marker | `<hex16>.repaired.<format>`, `<hex16>.partial.<format>`, `<hex16>.truncated.<format>` or `<hex16>.unverified.<format>` when `status` is one of those. The file then holds content PDFPundit changed, cut short or could not verify, and a loose copy must not pass as an as-found extraction. In every mode, so Part B never depends on a custody setting |
| Same bytes, several sources | One file per bytes and marker, several records. An intact source and a repaired source that happen to give the same bytes get two files, `<hex16>.png` and `<hex16>.repaired.png`, because only one of them is an as-found extraction |
| Same bytes, different `/Decode` or colour space | Still one file. The records differ, which is why records are never merged |
| Record order | Byte offset of the object in the input file, then `BI` offset for inline images |
| 16-hex collision with different bytes | Use all 64 hex for both files. Practically never happens |
| Type3 glyph bitmaps | Counted in `type3_glyph_images`, no files, unless `[repair] extract_type3_glyph_images` is on. Off by default, decision Q10 |
| Ambiguous repair survivors | The pinned choice is written. `distinct_outputs` gives the count |

### 6.5 Folder layout and manifest

```
/Volumes/EXHIBIT-JD3/exhibits/
  invoice_scan_2024.pdf                         the input, mode 0444; PDFPundit never writes it

/Cases/2026-0412/work/                          destination, set by output_dir
  invoice_scan_2024.repaired.pdf
  invoice_scan_2024.repaired.pdf.custody.json
  invoice_scan_2024.repaired.pdf.custody.txt
  invoice_scan_2024.repaired.pdf.custody.sha256
  f7c39aa7.images/                              first 8 hex of the input's SHA-256
    1b48e21282963dfb.jpg                        verbatim JPEG, as found
    d013614dc14a37ee.repaired.png               decoded samples after the repair
    images.json                                 image records, deterministic
    SHA256SUMS                                  sha256sum format, sorted by name
```

The example uses a separate work folder because custody mode requires one, decision Q16. Outside custody mode, with `output_dir` unset, D-061 still puts every file above beside the input.

`images.json` holds `format` const `pdfpundit-images`, `images_schema_version` 1, `input_sha256`, `tool` name and version, `settings_sha256`, `type3_glyph_images` and `records`. `settings_sha256` is the SHA-256 of Part B's `settings` object in compact form, 3.3, so a folder can be tied to the settings that made it without the record; `validate.py` check 19 recomputes it. Its records are byte-for-byte the same as Part B's `images.records`. The validator checks that.

`SHA256SUMS` uses GNU format: hex, two spaces, name, LF. It covers the image files and `images.json`. Lines are sorted by name bytes, so the file is deterministic.

The folder still follows D-060's reuse rule. A byte-identical folder is reused and nothing is written. One consequence: `images.json` names the tool version, so a newer version writing to the same place sees a different folder and gets a `(2)` suffix. That is correct, since its provenance differs.

### 6.6 Markdown links

| Image | Markdown line |
|---|---|
| JPEG or PNG | `![image 1240x1754](f7c39aa7.images/1b48e21282963dfb.jpg)` |
| JPX, CCITT, JBIG2, TIFF, bin, with no view file | `<!-- image f7c39aa7.images/0a1b2c3d4e5f6071.jb2e: JBIG2, 2480x3508, not viewable here -->` |
| Same, with the derived-view option on | Links `view_file`. The record says the link is a derived view |
| Repaired, partial, truncated or unverified | Linked by its marked name, `![image 600x200, repaired](f7c39aa7.images/d013614dc14a37ee.repaired.png)`. The alt text adds the status, a fact, not an opinion |
| Unplaced | Placed after the last page, as `export/mod.rs` does today |

Alt text is the dimensions, plus the status when it is not intact. It never describes content, because that would be an opinion.

## 7. Interaction record

### 7.1 Fields

| Field | Type | Notes |
|---|---|---|
| `seq` | integer from 1 | Order asked |
| `kind` | enum `font_pick`, `font_unreproducible` | `InteractionKind` |
| `page`, `slot` | integer or null, string or null | Where |
| `candidates` | array of font ids | What was offered |
| `reply` | tagged object | `pick` with `font_id`; `use_best`; `skip`; `substitute` with `choice`; `text_only` |
| `source` | enum `user`, `batched`, `policy` | Who answered, 7.2 |
| `carried_from` | tagged object or null | For `batched` only: `{type: this_record, seq}`, the earlier answer in this record it copied; or `{type: another_file, input_sha256}`, an answer the user gave to another file's question, "apply best to all" |
| `policy_setting` | string or null | For `policy` only: the setting that answered, `fonts.prompt_unresolved` or `repair.unreproducible` |

One interaction can answer several font slots. A font shared by slots on several pages, or a family's unreproducible question, asks once (`fonts.rs` `family_slots`). Each slot names its interaction in `font_decisions.interaction_seq`, 7.3.

### 7.2 Source mapping, D-141

`InteractionSource` already has four values since G-03 merged: `User`, `Policy`, `UseBest`, `Batched`. `Answer::recorded_source` turns the user's own `UseBest` reply into `UseBest`. The view maps them to three, by who saw the question:

| What happened, as the code records it today | `InteractionSource` | Source in this schema |
|---|---|---|
| The question was on screen and the user picked a font, skipped, or chose text only | `User` | `user` |
| The question was on screen and the user chose the best guess for it | `UseBest` (`engine.rs` `recorded_source`) | `user`, reply `use_best`. The user saw this question, so it is never `batched` |
| A later question of the same file and family, answered from the user's answer (`app.rs` `answers.known`) | `Batched` | `batched`, `carried_from` `{this_record, seq}` |
| The user pressed `b`, best for this file: the file's later questions | `Batched` | `batched`, `carried_from` the question on screen |
| "Apply best to all": parked questions of other files | `Batched` (`app.rs` 984) | `batched`, `carried_from` `{another_file, input_sha256}` |
| `[fonts] prompt_unresolved = false` answered | `Policy` (`app.rs` 1241) | `policy`, `policy_setting` `fonts.prompt_unresolved` |
| A configured `unreproducible` policy answered, or the generic-substitute fallback | `Policy` (`fonts.rs` 982, 1037) | `policy`, `policy_setting` `repair.unreproducible` |
| A guess cleared `auto_accept_confidence` | Not an interaction. The basis goes to the log sink only | Not an interaction, because nothing was asked. Recorded in `font_decisions` with basis `auto_accept` and its scores, 7.3 |

Mapping `UseBest` to `batched`, as revision 3 did, would print "not shown to the user" for an answer the user gave on screen. The code has the source; the record lacks only `carried_from` and `policy_setting`, which are section 14 item 1.

The custody prompt is not an interaction. Its answers live in `run.case`, per D-020.

### 7.3 Font decisions

`interactions` lists questions. `font_decisions` lists every font slot the repair touched, asked or not, so the record says who or what chose each font and on what evidence. Checked: today an auto-accepted slot leaves only a `RepairAction` text and a log-sink line, so this needs engine work before T-33 (section 14).

| Field | Type | Notes |
|---|---|---|
| `page`, `slot` | integer or null, string or null | Where. `slot` is the resource name, escaped per 4.1 |
| `object` | objref | The font dictionary |
| `basis` | enum `named_match`, `auto_accept`, `user`, `batched`, `policy`, `unsupported` | Why this font |
| `font_id` | string or null | The font used; null for `unsupported` and text-only |
| `interaction_seq` | integer or null | For `user`, `batched`, `policy`: the answer in `interactions`. Null otherwise. Several slots may name the same interaction, and every interaction is named by at least one slot. Its `source` equals the slot's `basis`. `validate.py` check 10 |
| `evidence` | object or null | Every input `resolve` used, for every scored slot. Null for `named_match` and `unsupported`. Fields below |
| `reason` | string or null | For `unsupported`: the engine's reason |

`evidence` fields, all from `fontdb/score.rs`:

| Field | Type | Notes |
|---|---|---|
| `top_candidate` | string or null | Null when no database font is a candidate |
| `confidence`, `hit`, `margin` | ratio or null | The top candidate's. Null with no candidate |
| `threshold` | ratio | The `auto_accept_confidence` setting |
| `min_hit` | ratio | `MIN_HIT`, 1/2 |
| `inference` | object | `lang`, `script` (ISO 15924), `n_chars`, `n_runs`, from `InferenceTrace` |
| `sample_floor` | integer | `SAMPLE_FLOOR`, 20 |
| `tounicode` | enum `present`, `missing`, `unparsable` | The slot's `ToUnicodeState` |
| `best_coverage`, `best_coverage_font` | ratio, string or null | The largest share of the slot's code points any database font maps, and that font |
| `min_coverage` | ratio | `MIN_COVERAGE`, 95/100 |
| `decision` | enum `auto_accept`, `ask`, `unreproducible`, `text_only` | What `resolve` returned |

The rule a reader can check, as `resolve` applies it:

1. The top candidate is refused when there is none, or confidence is below `threshold`, or hit is below `min_hit`, or `inference.n_chars` is below `sample_floor`.
2. The slot decodes when `tounicode` is `present` or the top candidate was not refused.
3. The slot is reproducible when there is a candidate and, if it decodes, `best_coverage` is at least `min_coverage`.
4. Not reproducible: the `unreproducible` setting decides. `text_only` gives `text_only`. Otherwise `unreproducible`, which asks, or under `substitute_generic` answers with the first option without asking, as a `policy` answer.
5. Reproducible and not refused: `auto_accept`. Reproducible and refused: `ask`.

`validate.py` check 11 recomputes `decision` from `evidence` with this rule for every slot in the example. A this-run sentence counts the auto-accepted slots, 12.3.

### 7.4 Replayed answers

An export after the job ended (D-125) re-runs analyze and repair in memory with the earlier job's replies (`jobs.rs`: the export-only job copies `done.replies`, and the worker replays them). Part B must match the earlier run, so its interactions keep their recorded `source`. But the person who answered may not be this run's examiner, and the answers were not given in this run. The earlier run may also have had no custody record at all: custody may have been off for the repair and switched on before the export.

`run.answers_replayed_from` is therefore a tagged object, never null. The runner knows the earlier run only through `jobs.rs`'s `Finished`, which keeps the name, path, input SHA-256 and replies. It keeps neither a start time nor the custody record's name, so both are new fields, section 14 item 29:

| `type` | Fields | When | `.txt` B.7 says |
|---|---|---|---|
| `none` | none | No answer in Part B was replayed. Every answer has an `answer_times` entry | Answers with their times |
| `custody_record` | `record`, `sha256`, `log_id`, `log_seq`, `seqs` | The earlier run wrote a custody record | "Answers marked * were given in an earlier run, recorded in `<record>` (custody log entry N), and replayed here. They were not given in this run." |
| `run_without_record` | `input_sha256`, `started_utc` or null, `history_run`, `seqs` | The earlier run had custody off | "Answers marked * were given in an earlier run, started `<time>`, that has no custody record; not given in this run. They were replayed here." With no start time: "...in an earlier run, whose start time was not kept, that has no custody record..." |

`seqs` lists the replayed interactions, so a run that replays some answers and asks a new one stays exact. `history_run` is true when the history store kept that earlier run, so an examiner can find it there. `started_utc` comes from `Finished.started_utc`, the new field. Today the only start time is `ui/app.rs`'s `repair_started`, which is removed when the job is recorded, and `RunRecord.started_at`, which exists only if history kept the run. So `started_utc` is null when neither exists, and the `.txt` says the time was not kept. The `custody_record` variant takes its `record`, `sha256`, `log_id` and `log_seq` from `Finished.custody_record`, also new.

Each replayed row in B.7 is marked `*` and its "answered by" ends ", earlier run". The fixture `example/fixtures/invoice_scan_2024.md.custody.json` is this case: a custody-off repair with two answers, then a custody-on export, for another case on another workstation. It has its own custody log, so its `earlier_records` is empty and its images folder is `created`, both truthfully. `validate.py` check 16 validates it and checks the wording; check 21 checks `earlier_records` across every record that shares a log. Its B.7 lines:

```text
Questions asked
Answers marked * were given in an earlier run, started 2026-10-08 13:41:52Z,
that has no custody record; not given in this run. They were replayed here.
#   Page  Slot  Answer                Answered by
1*  2     F3    font dejavu-sans      user, on screen, earlier run
2*  3     F5    font dejavu-sans      carried from #1, not shown, earlier run
```

Verify check 5 always replays. Its verify file, 9, says "answers replayed from this record, not given again".

Part B stays byte-identical, which is the point of replaying. Part A carries the honesty.

## 8. Custody log

### 8.1 Format

- File: `custody.log` in the data folder, or `[custody] log_path`.
- JSON Lines: one compact JSON object per line, LF after each.
- Content: hashes, counts, times, kinds and outcomes only. No case reference, examiner, notes, file names or paths. The records hold those, and the log binds the records by hash, so nothing is lost. Proving one record's chain then shows nothing about other matters except their times, outcomes and input hashes.
- Opened with append only. Every write is one line followed by fsync.
- An OS advisory lock is held while the app has custody on, through features of `rustix` and `windows-sys`, which are already dependencies. A second instance cannot start a custody batch.

### 8.2 Chain rule

- `prev` of entry N is the SHA-256 of entry N-1's bytes, without the LF.
- The first entry is `log_created`, with `prev` all zeros. It carries a `nonce`, 16 random bytes from the OS, so two logs created in the same second by the same build still get different `log_id`s.
- The chain head is the SHA-256 of the last line.
- The log identity, `log_id`, is the SHA-256 of the file's first line. Records carry it, so "entry 42" always means entry 42 of one particular file.

Anyone can check one link with standard tools: `sed -n 1p custody.log | tr -d '\n' | shasum -a 256` must equal the second line's `prev`. In the full log, line N is entry N. Tested on the example's three lines.

### 8.3 Entry kinds

| Kind | Extra fields | Written when |
|---|---|---|
| `log_created` | `tool`: version, build commit, build origin; `nonce`, hex32 | First use |
| `log_continued` | `reason` enum `chain_broken`, `previous_log_missing`, `head_mismatch`; `previous_log_id`; `previous_seq`; `previous_head_sha256`; `known_from` enum `log_walk`, `history`; `tool` | The start-up check found a broken chain, a missing log, or a log whose head differs from the one history kept. See 8.4 |
| `batch_opened` | `files` count, `prompt_shown`, `tool` | Before the first job of a batch |
| `record` | `batch_seq`, `input_sha256`, `outcome`, `input_unchanged`, `files` array of `{role, sha256}`, role enum `json`, `text`, `checksums`, `case_jsonld` | After a record's files are written |
| `record_write_failed` | `batch_seq`, `input_sha256`, `outputs` array of `{role, sha256}`, `error_kind`: the OS error kind, never a message with a path | Record files could not be written |
| `batch_closed` | `batch_seq`, `records`, `failed` | After the last job of a batch. Its hash is the anchor shown on screen |
| `batch_cancelled` | `prompt_shown`; `files`, the count of files the batch held; `received`, how many of those arrived whole at the drop and were discarded unhashed | The custody prompt was cancelled. Nothing more is read, 2.3.2 |
| `batch_unclosed` | `batch_seq`; `files`; `records` and `failed`, the `record` and `record_write_failed` entries the batch has | At start-up, for a batch opened and never closed or cancelled: the app quit, 2.3.3 |
| `verify` | `record_seq`, `record_json_sha256`, `verify_n`, the `<n>` of the verify file; `text_sha256`, that file's SHA-256; `checks`, `head_after_record` | Each Verify action |

Every entry starts with `seq`, `kind`, `time_utc`, `prev`, in that order. The schema has one closed shape per kind, so a line with a stray field such as a case reference fails validation.

### 8.4 Breaks and limits

- History keeps `(log path, log_id, seq, head)` after every append, not only at batch close. Verify entries move the head too.
- When custody mode starts, the app runs one check before any batch:

| Finding at start-up | Action | `log_continued.reason`, `known_from` |
|---|---|---|
| Chain walks clean; its `log_id` and last line match what history kept | Append as usual. Then append a `batch_unclosed` for any batch left open | none |
| A line's `prev` does not match | Never append to it. Start a new log | `chain_broken`, `log_walk`: last good entry from the walk |
| No log at the path, but history kept a head for it | Start a new log | `previous_log_missing`, `history` |
| Log exists but its `log_id` differs, or its line at the kept `seq` does not hash to the kept head, or it has fewer or more lines than history says | Never append to it. Start a new log | `head_mismatch`, `history` |
| No log and no head in history | First use: `log_created` | none |

- A new log is `custody-<n>.log`, or `custody.log` again when none exists at the path. Its first entry is `log_continued`, naming the previous `log_id`, `seq` and head. The old file is never renamed or deleted. The new file has its own `log_id`, so its entry numbers never get confused with the old file's. The screen shows the reason and the previous head, and the batch-end screen repeats it once.
- Each line is a few hundred bytes, so even 10,000 entries hash in well under a second.
- The chain shows a changed or removed entry only while every entry after it is unchanged. Anyone who can write the file can change an entry and recompute each later `prev`, and the rewritten chain verifies. So the chain alone catches only a careless edit.
- What catches a careful one: a head copied outside the log, 8.5, or an unedited copy of the log. History's kept head is such a copy, so the start-up check catches a rewrite, a removed newest entry and a swapped file, unless history was changed to match. The `log` statement says all of this in every record.

### 8.5 Offline anchors

A copy of a log head held outside the log is what closes the gap the chain leaves: it pins the log up to its entry. Three anchors exist, and each pins a different thing:

| Anchor | Where the examiner gets it | What it pins |
|---|---|---|
| `.json` SHA-256 | Every page header of the `.txt` | The machine record a printed page came from. Editing the `.json` breaks the match with a page already in the case file |
| Log head after this batch | On screen when the batch closes: the hash of the `batch_closed` line and its entry number. Also in history, which the start-up check compares against | The log up to and including every record of the batch |
| Log head after this record | Verify's screen and its `.verify-<n>.txt`: the hash of the record's own entry, labelled "log head after this record", plus the current head | The log up to that record, at the time of the Verify |

The `.txt` cannot hold a head that covers its own record. The record's log entry contains the `.txt`'s hash, so that entry is written after the `.txt`. The `.txt` therefore prints only "Log entry before this record" with its number and hash, under that label.

Why the batch anchor matters. Without it, someone could edit the last record's `.json`, regenerate its `.sha256`, and rewrite its log line and every line after it. The chain still verifies, because each rewritten `prev` matches the rewritten line before it. With the batch head copied into notes, the rewritten `batch_closed` line no longer matches the copy. The printed `.json` hash catches the `.json` edit on its own. The same holds for an edit anywhere earlier: a rewrite from that point on is caught only by a head copied after it.

### 8.6 Showing a chain to someone else

To prove record entry N to opposing counsel or an auditor:

1. Give them the log lines from line 1 to the `batch_closed` line of entry N's batch. Start at line k instead if an anchor for entry k-1 is already in an agreed note.
2. Give them the record files. They check the record's file hashes against entry N and the chain from line 1 or k.
3. If the examiner copied a batch head, they compare it with the hash of the `batch_closed` line.

The lines shown contain hashes, counts, times and outcomes of other batches, and nothing else. An input SHA-256 does let a holder of a known file confirm it was processed. That is the residue, and the review log notes it.

A log per case would remove even that. v1 has none, decision Q12: one log per machine, safe to show because it holds no case details. `[custody] log_path` still allows a separate file.

## 9. Verify action

Verify runs on a finished record and does five checks:

| # | Check | Result values |
|---|---|---|
| 1 | Record files against the hashes in their log entry | `match`, `mismatch`, `missing`, `empty`: a placeholder left by a crash, 2.2 |
| 2 | Input file against `hashes_before` | `match`, `mismatch`, `unavailable` |
| 3 | Each placed file against its recorded hashes | `match`, `mismatch`, `missing`, per file |
| 4 | Walk the log chain up to and past this entry | `intact`, `broken at seq N` |
| 5 | Optional re-run: same build, settings and recorded answers, then compare `reproducible_sha256` | `identical`, `different`, `different build`, `not_run` |

Check 5 is the direct answer to ACPO principle 3: a third party repeats the process and gets the same result. It needs the input bytes and the same build on the same platform, 3.1. A different version, commit or platform reports `different build` instead of a false mismatch.

Output: a `verify` log entry, the result on screen, and `<primary>.custody.verify-<n>.txt`, a one-page text record, every time, decision Q9. The record itself is never changed, per SWGDE 18-Q-002 section 6.

How `<n>` never collides:

1. `<n>` counts the verify files of this one record, from 1. It is not a log entry number, so a continued log, 8.4, cannot repeat it.
2. Verify claims the name `verify-1.txt` with `create_new`. If it exists, it tries `verify-2.txt`, and so on: the first free number. `create_new` is atomic, so two Verify runs, even from two app instances, can never claim the same name.
3. The bytes go to a temporary file and are renamed onto the claimed name, as the record files are, 2.2.
4. The `verify` log entry holds `verify_n` and the file's SHA-256, `text_sha256`. A verify file is tied to its entry by hash, so a renamed or copied file still pairs, and a gap in the numbers after a crash shows as an empty placeholder with no entry.

Revision 4 named the file `.verify-<log_id8>-<seq>.txt` for the same safety. The user's shorter form is just as safe under the rules above. The entry number is safe only within one log, so the counter replaces it.

The screen and the `.verify-<n>.txt` both print:

- the record's `.json` SHA-256 and log identity
- "Log head after this record": the SHA-256 of the record's own entry, with its number
- "Log head now": the SHA-256 of the newest line, with its number
- for check 5: "answers replayed from this record, not given again", per 7.4

## 10. CASE/UCO JSON-LD

### 10.1 Recommendation

Offer it, off by default, after the JSON and text files ship.

| For | Against |
|---|---|
| CASE is the interchange format forensic suites read | It has no slot for object ids, grades, byte spans or salvage edits |
| The mapping is small and validated offline | One more file beside each output |
| Pure `serde_json`, no RDF crate | CASE 1.5.0 is still moving; UCO 2.0 will tighten hash rules |

### 10.2 Mapping

| PDFPundit | CASE 1.5.0 / UCO 1.5.0 |
|---|---|
| Tool, version | `uco-tool:Tool` with `uco-core:name`, `uco-tool:version` |
| Input file | `uco-observable:File` with `FileFacet` and `ContentDataFacet` |
| Repaired PDF | `File`, `case-investigation:wasDerivedFrom` the input file |
| Each image file | `File`, `wasDerivedFrom` the input file, plus `RasterPictureFacet`: `pictureWidth`, `pictureHeight`, `bitsPerPixel`, `pictureType` |
| Hashes | `uco-types:Hash`, method `SHA256`, `SHA1`, `MD5`, value typed `xsd:hexBinary` |
| Repair | `case-investigation:InvestigativeAction` "Repair PDF": `instrument` tool, `object` the input file, `result` repaired PDF, `startTime`, `endTime` |
| Image extraction | A second `InvestigativeAction` "Extract images", `result` the image files |
| Examiner | `uco-identity:Identity` with `uco-core:name`, linked as `uco-action:performer`. Omitted when blank |
| Case reference | `case-investigation:Investigation` with `uco-core:name`, `uco-core:object` both actions. Omitted when blank |
| Grades, edits, findings, interactions | Not mapped. The CASE file names the custody record instead |

### 10.3 Rules

- Pin CASE 1.5.0. Inline the `@context`, so a reader never fetches a remote one.
- IDs are `kb:<kind>-<UUIDv5>` in a fixed PDFPundit namespace, named by content. The file is a pure function of the record.
- Not UUIDv8. UCO's identifier advice matches the version digit against `[0-5]`. Tested: the example with every version digit set to 8 gives 31 Info results and `Conforms: False`. UUIDv5 needs SHA-1, which the legacy hashes add anyway.
- Validate in CI with `case_validate --built-version case-1.5.0`. It runs offline. Never at run time.
- Result on the example: `Conforms: True`, 10.8 kB, including MD5 and SHA-1 hashes, the raster facets, the identity and the investigation.

## 11. Mapping onto the standards

No claim of compliance with any standard appears in any record. The record is built to give an examiner what each source asks for.

### 11.1 ISO/IEC 27037 and 27042

Only the previews were read; the clause text is paywalled. So this section cites 27037 clauses by number and title only, from the preview's table of contents, and claims nothing about what a clause says.

| Fact from the previews | Consequence |
|---|---|
| 27037 covers identification, collection, acquisition and preservation, and "does not extend to the analysis" | PDFPundit is not an acquisition tool. It works on acquired files and makes derivatives. The record must not say "ISO 27037 compliant" |
| 27037 clause 5.3, "Requirements for digital evidence handling", has subclauses titled Auditability, Repeatability, Reproducibility and Justifiability | Mapped below as design goals named by those titles, not as clause compliance |
| 27037 has clauses titled 6.1 "Chain of custody" and 6.6 "Documentation" | Not mapped. Their text was not read, and will not be: the user chose not to buy the standards, decision Q11. The design rests on SWGDE, NIST and ACPO |
| 27042 asks that processes be recorded "to allow such processes to be subjected to independent scrutiny" | Part B, the Part B hash and Verify check 5 |

| 27037 subclause title | Where the schema serves the goal the title names |
|---|---|
| Auditability | Every action with object ids; every answer with its source; the hash-chained log |
| Repeatability, same examiner and tool | Part B is byte-identical on re-run; Verify check 5 |
| Reproducibility, another examiner | Build commit, settings and answers recorded; `jq` check of the Part B hash; CASE view for other tools |
| Justifiability | Grades say how certain each repair is; limitations always present; no opinions |

### 11.2 SWGDE, NIST, ACPO, US rules of evidence

| Requirement | Source | Field or feature |
|---|---|---|
| Tool output is supporting documentation, not the report | SWGDE 18-Q-002 v1.0 s4 | Title "Tool run record"; `role` statement |
| Case identifier, page n of N | SWGDE 18-Q-002 5.1.3 | `run.case.case_reference`; `.txt` header |
| Each item identified | 5.3 | The input SHA-256, computed, unique to the item's content, with the batch position. `submission_reference` is typed once per batch and labelled as applying to all N files, so it never claims to identify one item |
| Acronyms defined | 5.1.5 | `.txt` part C |
| Authority, requestor | 5.2.4 | `run.case.notes`, labelled "Authority/notes" |
| Each item identified by hash, examined or not | 5.3.4 | A record for every queued file, including failed, clean, cancelled. Files dropped and then cancelled at the prompt were never queued: a `batch_cancelled` entry gives their count, and how many arrived whole at the drop, because hashing them would read evidence the examiner declined to process, 2.3.2 |
| Data recovered, extracted and provided | 5.4 | `products`, `placed`, `images.records` |
| No unsupported opinions | 5.5; FRE 702 note | No content opinions; graded wording only |
| Disposition of input and derivatives | 5.6 | `input_file` statement; `destination`; `placed` with paths |
| Authoriser signature | 5.7 | Blank sign-off block in the `.txt` |
| Amendments reference the original | s6 | `earlier_records`; Verify writes new files only |
| Tool digest, version, exact switches | NIST SP 800-86 5.2.1 | `tool_binary_sha256`, `tool`, `settings` |
| Hash before, copy, hash again | NIST 4.2.2 | `hashes_before` and `hashes_after` of the input. PDFPundit makes no copy of the input, only derivatives, so placed-file hashes are the derivatives' own |
| Time and time zone; when each action was done | NIST 3.1.2, 4.4 | `clock` with offset and defined start and end points; `answer_times` per answer; log entry times; `times` statement |
| Logs can be altered | NIST 2.5 | Chain, walk on start, `log` statement |
| Record of all processes; third party gets the same result | ACPO v4 principle 3 | Part B; Verify check 5 |
| No change to original data | ACPO v4 principle 1 | Read-only open of the input; `unchanged` check. The `input_file` statement says PDFPundit cannot tell the original from a working copy. Custody mode never writes into an input's folder: it requires `output_dir` and refuses an evidence folder, decision Q16. `destination` records where outputs went |
| Hash identification of the item | FRE 902(14) note | `run.input_file.hashes_before.sha256`, plus MD5 or SHA-1 when set, for the examiner to compare with their acquisition hash. There is no copy, only derivatives |
| Process produces an accurate result | FRE 902(13), 901(b)(9) | Build, settings, grades, verification gates; `validation` statement says what is not claimed |
| Identified limitations | SWGDE 18-Q-001-2.1 s6 | `limitations`, always present |
| State of the item documented | SWGDE 18-Q-002 5.3 | `stat_before`, `stat_after`, `stat_changed`; `path_kind` and `first_read` say whether PDFPundit read the item before the stat |
| Who made each decision | FRE 702 note; D-141 | `interactions` with source; `font_decisions` with basis |

## 12. Limitations and fixed statements

### 12.1 Statements, in every record

`templates.py` holds every string in this section, and this document prints them from it. The example writes the same strings, and `validate.py` check 18 compares them. So the text here, in the example and in the check cannot drift apart. Changing any of it changes Part B hashes, so it bumps `custody_schema_version`, 4.6.

| Key | Text |
|---|---|
| `role` | This is a tool run record. It supports an examiner's report and is not one. It states no opinion on the document's content, authorship or authenticity. |
| `reproducibility` | Part B is identical, byte for byte, whenever the same input bytes are processed by the same PDFPundit build with the same settings and the same recorded answers. The same build means the same version and build commit, compiled for the same operating system and processor architecture; part B records all four. Identity across platforms is not claimed. This shows the result is repeatable. It does not show the repair is correct. |
| `validation`, with a build commit | PDFPundit has not been validated by an independent body, such as NIST CFTT, and claims no accreditation. Its test suite is in its source repository at the build commit recorded in part B. Its corpus tests use a third-party corpus that the repository names but does not contain. |
| `validation`, no build commit | PDFPundit has not been validated by an independent body, such as NIST CFTT, and claims no accreditation. This build records no build commit, so this record cannot name the source its tests ran on. Its corpus tests use a third-party corpus that the repository names but does not contain. |
| `tool_identity` | Version, build commit and build origin are reported by the program itself, and whoever builds it can set them. Releases are unsigned. Each release publishes the SHA-256 of every executable it ships. To confirm the build, compare the binary SHA-256 in part A with the value published for this version and platform. |
| `input_file` | PDFPundit cannot tell whether its input is the original evidence or a working copy. It opens the input read-only and never writes to it. It is not a write blocker. Unchanged means the SHA-256 of the input's content was the same before and after the run. File-system times before and after the run are recorded in part A. Reading a file can itself update its access time. In custody mode, outputs and records are written only to the output folder named in part A. Custody mode does not start while no output folder is set, or while it is the folder of an input. |
| `case_details` | Case reference, examiner, submission reference and notes are typed at the custody prompt, once per batch. PDFPundit records them as typed and does not verify them. |
| `times` | Times come from the host clock, in UTC, and are not independently verified. Started is read just before the job first reads the input's file state. An input received whole at the drop was read before that, before the custody prompt; part A says how the input was received. Ended is read just after the input's file state is read again, before the record files are written. The log entry for this record is timed after those files are written. |
| `log` | The custody log is hash-chained. It is tamper-evident, not tamper-proof. The chain shows a changed or removed entry only while the entries after it are unchanged: anyone who can write the file can recompute every later entry, and the chain then verifies. Only a log head copied outside the log, such as the one shown when a batch closes, or an unedited copy of the log catches that; such a head pins the log up to its entry. PDFPundit also compares the log with the last head kept in its history when custody mode starts, and starts a new log naming the old one when they differ; a change made to both is not detected. |
| `legacy_hashes` | MD5 and SHA-1 are included only to match older case systems. Neither is collision-resistant. SHA-256 is the integrity hash. |

Each sentence was checked against what exists today:

| Statement | Claim | True today? |
|---|---|---|
| `validation` | Test suite at the build commit | Yes: `tests/` and the unit tests are in the repository |
| `validation` | Corpus named, not contained | Yes: `AGENTS.md` names the REPDF corpus and says it is never inside the repo. The first draft said results were in the release notes; no release step publishes them, so that sentence is gone |
| `validation` | Variant chosen by `tool.build_commit` | Yes: a build with no commit gets the sentence that names no commit |
| `input_file` | Read-only open, never written | Yes: placement never targets the input, and D-044 refuses any output path that is a batch input |
| `input_file` | Custody mode writes only to `output_dir` and does not start without it or when it is an input's folder | Not yet. Section 14 item 37. Must ship with custody mode |
| `log` | History comparison at start-up | Not yet. Section 14 item 20 |
| `log` | Only a head kept outside the log catches a rewrite from an edit onward | Yes: the chain is keyless, so recomputing every later `prev` gives a chain that verifies |
| `times` | Started precedes the job's first read of a path input | Not yet. Today `gate()` opens every file at the drop. True once section 14 item 23 ships, which must ship with custody mode |
| `reproducibility` | Part B records version, commit, OS and architecture | Once item 34 adds `tool.platform` |
| `tool_identity` | Each release publishes the SHA-256 of every executable | Not yet. cargo-dist publishes archive checksums only. Section 14 item 13 adds per-executable hashes, decision Q14, and the first custody-capable release waits for it |

### 12.2 Standing limits, in every record

The four `KNOWN_LIMITS` sentences from `text.rs`, verbatim, then two more:

- text marked with ActualText is extracted as drawn; the ActualText is not used
- glyphs carry an origin and a size but no bounding box
- text drawn in render mode 7 (clip only) is not extracted
- the interpreter gives no warning for a dropped compressed stream or an unresolved font; unmapped glyphs and the glyph count per page are the integrity signals
- image files hold the image as stored, or its decoded samples re-encoded as PNG or TIFF with the same sample values; colour spaces, /Decode arrays and masks are recorded, not applied; an image whose stream was repaired, cut short or left unverified holds those samples, its file name says so, and its record gives the search
- a repaired file is a new document: it is not the input and carries none of the input's signatures

### 12.3 This-run sentences, added only when the condition holds

Each sentence is added only when its count is above zero, in the order below. Every condition reads Part B alone, so the sentences are a pure function of Part B, and `validate.py` check 18 recomputes the list for every record.

Plural rule: in a template, `[n:one|many]` gives `one` when the count `n` is exactly 1 and `many` otherwise; `[m:...]` does the same with `m`. `{n}` and `{m}` print the counts in plain digits. So `1 of 3 font slots was matched`: the noun follows `m`, the verb follows `n`.

| Key | Condition, from Part B | Template |
|---|---|---|
| `c9_exact` | `stream_salvage.summary.exact` > 0 | {n} compressed [n:stream was\|streams were] graded Exact: every single-byte replacement in the searched window was tried, and every one that passed the check gave the same output; damage of other kinds is not searched, and the grade does not show the repaired bytes equal the original bytes |
| `c9_accepted` | `summary.accepted` > 0 | {n} compressed [n:stream was\|streams were] graded Accepted: the work limit ran out before every single-byte replacement in the window was tried; every one that passed the check up to then gave the same output, and a different one may exist |
| `c9_ambiguous` | `summary.ambiguous` > 0 | {n} compressed [n:stream was\|streams were] graded Ambiguous: replacements that passed the check gave two or more different outputs, or the stream is raw deflate with no Adler-32, and a fixed order chose one |
| `c9_trailer` | streams whose `search.trailer_edit` is true > 0 | for {n} compressed [n:stream\|streams] no replacement in the data passed the check, so the stored Adler-32 was rewritten to match the decoded data; that data is the damaged stream's own and no checksum covers it, whatever the grade says |
| `c9_widened` | streams whose `search.adler_rerun` is true > 0 | {n} font [n:stream\|streams] failed [n:its\|their] own table checksum, so the search widened and the repair passed the Adler-32 check alone |
| `c9_unverified` | `summary.checksum_mismatch` > 0 | {n} compressed [n:stream was\|streams were] written decoded with no grade: no replacement passed the Adler-32 check, or one that passed held an invalid predictor row and only the rows before it were kept; these bytes are unverified |
| `c9_prefix` | `summary.prefix` > 0 | {n} compressed [n:stream was\|streams were] cut short: only the bytes before the damage decode, and the rest of [n:the stream is\|each stream is] lost |
| `c9_unrecoverable` | `summary.unrecoverable` > 0 | {n} compressed [n:stream\|streams] could not be repaired and [n:was\|were] written as found |
| `c9_unsearched` | `summary.unsearched` > 0 | {n} compressed [n:stream was\|streams were] over the search size limit and not searched |
| `fonts_substituted` | font objects in `font_decisions` with a `font_id` > 0 | {n} font [n:program was\|programs were] substituted from the bundled font database; glyph shapes are the bundled font's, not those of the font the input embedded |
| `font_substitute_or_text_only` | interactions whose reply is `substitute` or `text_only` > 0 | {n} font [n:question was\|questions were] answered with a substitute or text only, as part B.7 shows |
| `font_auto_accept` | slots with basis `auto_accept` > 0; m = slots in `font_decisions` | {n} of {m} font [m:slot\|slots] [n:was\|were] matched without asking, because [n:its score\|their scores] cleared the auto-accept setting; part B.7 gives the basis for every slot |
| `answers_batched` | interactions with source `batched` > 0; m = interactions | {n} of {m} [m:answer\|answers] [n:was\|were] carried from an earlier answer and not shown to the user |
| `answers_policy` | interactions with source `policy` > 0; m = interactions | {n} of {m} [m:answer\|answers] came from settings, not from the user |
| `signed` | `input.signed_fields` > 0 | the input has {n} digital signature [n:field\|fields]; the repaired file carries none |
| `pages_appended` | sum of `pages` in `pages_outside_tree` findings > 0 | {n} [n:page was\|pages were] found only outside the page tree and [n:was\|were] appended after the tree's pages |
| `scanned_pages` | `text_warnings` holds `scanned_pages` | the page tree is unreadable: pages were found by scanning and are in file order; text counts taken that way follow file order, while every page number in this record follows the rebuilt page order |
| `hidden_layers` | `text_warnings` holds `hidden_layers` | the document has optional content: text in hidden layers is not extracted |
| `reals_narrowed` | `reals_narrowed` > 0 | {n} [n:number\|numbers] lost precision when written as 32-bit values |
| `images_incomplete` | images with status `partial` or `truncated` > 0 | {n} [n:image is\|images are] incomplete; only whole rows that decoded are kept |
| `images_unverified` | images with status `unverified` > 0 | {n} image [n:file holds\|files hold] samples decoded from a stream that failed its check; [n:its name says\|their names say] unverified |
| `images_need_context` | images with `needs_pdf_context` > 0 | {n} image [n:file displays\|files display] wrongly on [n:its\|their] own; the PDF settings recorded here are needed to read [n:it\|them] |
| `type3_counted` | `images.type3_glyph_images` > 0 and `extract_type3_glyph_images` false | {n} glyph [n:image\|images] in Type3 fonts [n:was\|were] counted, not extracted |
| `outcome_reasons` | `outcome.reasons` not empty | each engine reason, verbatim, in order |

What the search covers, from the constants in `salvage.rs` and `salvage/ttf.rs`:

| Damage | Window (in the edit domain) | Check | Fields |
|---|---|---|---|
| Inflate fails at input offset `k` | `[k - 4,096, k + 8)`, clamped to the stream (`ERROR_WINDOW`, `ERROR_SLACK`). The deep draw extends the same pass with `[k - 16,384, k - 4,096)` (`DEEP_ERROR_WINDOW`), searched after the first range | Adler-32 | `localizer = error_window`, `damage.kind = inflate_error`; `deep` when extended |
| zlib header rejected at `at` | As above with `k = at`, so `[0, at + 8)` covers the header. If that finds nothing, the stream is read as raw deflate and searched again, as a new pass with its own damage and window | Adler-32, then none for raw deflate | First pass `damage.kind = header_rejected`; second pass `mode = raw_deflate` |
| Decodes to the end, Adler-32 differs | From byte 2, after the zlib header, to the trailer (`ADLER_FROM`): the whole body | Adler-32 | `localizer = adler_only`, `damage.kind = adler_mismatch` |
| Same, and the output is an sfnt font whose table directory parses | Output bytes before the end of the lowest-offset table whose checksum fails, less every table that verifies, mapped back to input positions. It comes from the font's own directory, not from a fixed distance | Past the directory, the failing table's checksum first, then Adler-32 | `localizer = ttf_tables`, `check = adler32_after_ttf_table_checksum` |
| Font window gives no accept | A new pass, widened: the positions the table check judged, then the rest of the whole-body window. Its count starts again, and some positions are tried a second time under the other check | Adler-32 alone | Pass `localizer = widened`; `adler_rerun = true` |
| Body window gives no accept, Adler-32 off in exactly one byte | No search. Rewriting that trailer byte is a survivor. It runs only after the body window is exhausted, so a stream graded Exact this way had no body edit pass the check. The data is the damaged stream's own, and nothing checks it: the trailer was rewritten to fit it | The rewritten Adler-32, which checks nothing | `trailer_edit = true`; the `c9_trailer` sentence |
| No repair after the search, Adler-32 damage | Written decoded, every byte, unverified | none | `outcome = checksum_mismatch`, image `unverified` |
| No repair after the search, inflate error; or the input runs out | Written decoded up to the damage or the end of input | none | `outcome = prefix`, image `partial` |

In every row a candidate is one byte replaced by each of the 255 other values. It passes only when miniz_oxide ends the inflate `Done`, which means its Adler-32 matched. The search never stops at the first pass. It runs until the window is exhausted or the work budget is spent, and passing candidates are grouped by the SHA-256 of their output. Two different edits that give the same output count as one distinct output, which is why the grades talk about outputs, not edits.

Not searched: insertions, deletions, damage to two or more bytes, and damage outside the window. A 32-bit check means a wrong candidate can pass, which is why Exact never claims the bytes are the original's. `passes` in 4.3 lets anyone check each window and count, and `validate.py` check 13 tests the error-window arithmetic on the example and the fixtures.

None of these sentences says "lossless", "forensically sound", "validated", "court-ready" or "complete". `validate.py` check 22 scans every Part B for those words and for the engine phrases this revision replaces. The artefact deny-list test gains the custody files and `images.json`. This template text must stay out of `ui::strings::ALL`, or it would fail its own scan.

## 13. Text report layout

### 13.1 Rules

| Rule | Value |
|---|---|
| Title | `PDFPUNDIT TOOL RUN RECORD`. Never "report of examination" |
| Width | 80 columns. Labels in a 16-column field. Hashes fit unbroken. Paths wrap at `/` |
| Pages | 60 lines: a 4-line header, then 56 lines. Form feed between pages. A block never splits across pages unless it is longer than one page |
| Header | Title and `Page n of N`; the record's JSON file name; `JSON SHA-256` and that file's hash; a rule. Every page carries the hash, so a loose page still names its record |
| Encoding | UTF-8, LF |
| Blank fields | Printed "not provided", never left out, decision Q13. The UX prompt says "recorded as blank" and never quotes the phrase, so `strings::ALL` cannot fail the artefact scan |
| Order | Preamble; A run context; B reproducible result; C abbreviations; D custody log and sign-off |
| Words | Plain English. Hash names are spelled out. Legacy hashes are labelled legacy on every line |
| Content | No colour, no ANSI, no cat, no theme names |

### 13.2 Section map

```
Preamble        role statement; what parts A and B mean
A  RUN CONTEXT  case, examiner, submission reference "applies to all N files",
                notes, "not verified", batch, times with what they mark (the
                Started line depends on how the input arrived), UTC offset or
                where it came from, zone name hint, host, binary hash
   A.1          input file (as received by PDFPundit): "cannot tell original
                from copy"; name, path, how it was received, size, hashes
                before and after, content verdict; file state before and
                after, with differences named
   A.2          destination, the required output folder; files
                written: one block per file, kind, size, hashes, re-read check
B  REPRODUCIBLE Part B SHA-256 and how to check it
   B.1..B.3     tool, build origin and platform, settings, input and outcome,
                method chosen over the others, output checks, kept counts
                labelled against the damaged input
   B.4..B.6     findings with what became of each; repair actions in the
                engine's words; stream salvage: outcome and grade, then per
                pass the damage, window rule, window, count tried and check,
                then passing edits, distinct outputs, edits, trailer note
   B.7          questions and answers with who answered and when, "not
                shown" for carried and policy answers; replayed answers
                marked; every font slot with its basis, every refusal reason,
                and the coverage and /ToUnicode facts behind it
   B.8          extracted images, one block each
   B.9          limitations: standing, then this run
   B.10         statements
C  ABBREVIATIONS
D  CUSTODY LOG  log path, log identity, entry number, log entry before this
                record; where to get the head after it; blank sign-off
```

The full worked page set is Appendix B. It is 7 pages for this example, mostly because each written file gets its own block.

## 14. Work this design implies

| # | Work | Depends on | Notes |
|---|---|---|---|
| 1 | Answer origin: `carried_from` and `policy_setting` on `Answer` and `InteractionRecord` | none | The source itself is done: G-03 merged `InteractionSource` `User`, `Policy`, `UseBest`, `Batched`, and `app.rs` sets each. Left: the app passes which answer it copied (`answers.known` already returns it as text) and which setting answered. Must land before T-33 |
| 2 | Image extraction pass; `RepairReport` v2 with `ImageRecord`; `images.json`, `SHA256SUMS` | hayro spike for stencils, inline images, form chain | D-114. Bump `SCHEMA_VERSION`, regenerate the contract snapshots |
| 3 | `src/custody.rs`: view mapping, hashing, record writer, `.txt` renderer, manifest | 1, 2 | Pure functions over `RepairReport`, `RunRecord` and product bytes |
| 4 | Custody log: append, lock, chain walk, continuation | none | `rustix` `fs` (`flock`); `windows-sys` `LockFileEx`, which needs the `Win32_System_IO` feature for `OVERLAPPED` |
| 5 | Runner: re-read placed files, re-hash the input, read the clock at the points 2.3 defines, record `answer_times` with `shown_utc` | 3 | Time read only in `jobs.rs` and UI, per D-076 |
| 6 | Host name, UTC offset, zone hint, binary hash | none | `rustix` `system` feature; Windows `GetComputerNameExW`, and `GetDynamicTimeZoneInformation` for the zone hint, which needs the `windows-sys` `Win32_System_Time` feature. Windows offset from one instant: `GetSystemTimeAsFileTime`, then `FileTimeToLocalFileTime` on that same value, difference rounded to whole minutes, so no second or minute boundary can fall between two reads. Unix offset from `clock.rs`'s TZif reader, item 36. Decision Q7 |
| 7 | Legacy hash crates | none | `sha1`, `md-5` as direct deps |
| 8 | Prompt, setup, batch-end and verify screens | UX design, D-016, D-048 | Fields: case reference, examiner, submission reference labelled for the whole batch, notes. The prompt refuses to start with no `output_dir`, or with one that is an input's folder, 2.4. `require_case_details` refuses an empty case reference. Frame 13 shows algorithm names, never hash values. Batch end shows the log head. Verify shows the head after the record |
| 9 | Verify action | 3, 4 | Checks 1 to 5 |
| 10 | CASE writer and CI validation | 3, 7 | Off by default |
| 11 | Artefact deny-list scan over every new file | 3 | D-104 |
| 12 | Ship `docs/custody.schema.json` and a field reference | 3 | Versioned with `custody_schema_version` |
| 13 | Release workflow: set `PDFPUNDIT_BUILD_COMMIT` from the tagged commit and `PDFPUNDIT_BUILD_ORIGIN=release`; publish the SHA-256 of each executable beside the archive checksums | none | Decisions Q8 and Q14. `v-release.yml` sets neither today. cargo-dist's `checksum = "sha256"` covers archives only. The first custody-capable release waits for it. Local builds record a null commit |
| 14 | `RepairReport.font_decisions`: keep each slot's basis, `Candidate` scores and thresholds as data | none | Must land before T-33, like item 1. Today the basis reaches only the log sink |
| 15 | Input stat before and after, raw integers | none | `rustix` `fs` stat and `statx`; `windows-sys` `GetFileInformationByHandleEx` with `FileBasicInfo`. No `std::time` |
| 16 | Part B ASCII rule: `#xx` for names, `\xHH` elsewhere, ASCII-only engine text | 3 | Plus the 0x7F and non-UTF-8 fixture, and a test that jq and Python reproduce `reproducible_sha256`. `work/ascii_fixture.py` is the model |
| 17 | Log changes: no case data in lines, `batch_closed`, `batch_cancelled` with `received`, `batch_unclosed`, `log_id`, the `log_created` nonce, `input_unchanged`, head shown at batch end | 4 | The head goes to the batch-end screen and to history. The nonce needs an OS random source: `getrandom`, already in `Cargo.lock`, as a direct dependency, through the dependency gates |
| 18 | Not in v1: publish a test and corpus summary per tag | 13 | Decision Q14 covers executable hashes only. The `validation` statement does not depend on a summary |
| 19 | C9 report fields: one record per pass (`mode`, `damage`, `localizer`, `check`, `deep`, window, tried), `passing_edits`, `predictor_stopped`; the passes also for `ChecksumMismatch` | none | Lift them out of `Pending` at each `start` and at `conclude`. Must land before T-33, or the Exact claim cannot be checked |
| 20 | Start-up log check against history | 4, 17 | History keeps `(path, log_id, seq, head)` after every append. `log_continued` reasons per 8.4 |
| 21 | Gate on system fonts | none | System-font support must add the SHA-256 of every system font used to Part B before it ships, 3.1 |
| 22 | Image status marker in file names; span domains for inline and Type3 images | 2 | 6.2, 6.4. The Markdown alt text adds the status. Includes `unverified` |
| 23 | Custody-mode drop gate: admit on name and `fs::metadata` only; move the open, the read check and the `%PDF-` sniff to job start, after `stat_before` | none | `src/ui/input/gate.rs`, job start in `src/jobs.rs`. The UX design lists it. Kitty drops stay read at the drop. Must ship with custody mode, or the `times` statement and 2.3.2 are false |
| 24 | C9 engine text: stop "no other repair fits its window" | none | It overclaims: the search tried single-byte replacements in one window, and two outputs outside it could both fit. The text lands in Part B verbatim, in an action and a finding. Exact strings below |
| 25 | `C9Summary` gains `checksum_mismatch` and `prefix`; images from those streams get status `unverified` and `partial` | 2, 19 | Today both count only into `streams_damaged`, so a reader's sum fails, and an image from an unverified stream would get a plain as-found name |
| 26 | `FindingKind` in the view: `kind`, `class`, `detail` for all seven variants | 3 | Revision 3's closed enum had no `font_not_embedded` or `pages_outside_tree`, so a common input produced an invalid record. Keep the font name's raw bytes for the 4.1 escape |
| 27 | Signed-input wording: "original" becomes "input" | none | `report.rs` `SIGNED_NOTE` and `diagnose.rs` `SIGNED_SUMMARY`. Exact strings below. The custody view's own this-run sentence names the field count, 12.3 |
| 28 | Record files and verify files written to a temporary file, then renamed onto a claimed placeholder; verify files named with the log id | 3, 9 | 2.2, 9 |
| 29 | `Finished` in `jobs.rs` keeps `started_utc` and the job's custody record ref | 5 | 7.4. Today the start time lives only in `ui/app.rs`'s `repair_started`, removed when the job is recorded |
| 30 | Config keys this design adds or relies on | 3 | `[custody]`: `hashes`, `ask_case_details`, `log_path`, `require_case_details` (new, default false), `record_host_name` (new, default true), `case_jsonld` (new, default false). `[repair]`: `derived_image_views` (new, default false), `extract_type3_glyph_images` (new, default false). `[fonts] prompt_unresolved` and `[general] output_dir` exist. Each new key goes through `config.rs` validation and the setup screen |
| 31 | `earlier_records` searched back through `log_continued` | 4, 20 | 4.4 |
| 32 | `batch_unclosed` at start-up; records for jobs cancelled at quit | 4, 20 | 2.3.3 |
| 33 | `RepairReport.text_warnings`: which of `HIDDEN_LAYERS` and `SCANNED_PAGES` fired, for the baseline and the chosen output | none | Today they reach only `PageText.warnings` |
| 34 | `tool.platform` in Part B, from `std::env::consts` | 3 | 3.1. Verify check 5 compares it |
| 35 | Map the renames in 4.5: `v0`, `v1`, `v2`; the `FILE` sentinel; page index from 0 | 3 | View code only |
| 36 | Reuse `clock.rs`'s TZif reader for the record's UTC offset | 6 | Make `tzif_offset` `pub(crate)`; the custody path records null where `local_hm` falls back to 0; amend the module comment, 5.5. Decision Q7 |
| 37 | Custody mode requires `[general] output_dir`, never an input's folder | 8, 23 | Setup refuses to switch custody on without it; the prompt refuses `Start batch`, 2.4. Decision Q16. Drops `warned_at_prompt` and the warning rows of frame 13 |
| 38 | Verify files `.verify-<n>.txt`, claimed first-free with `create_new`; `verify_n` and `text_sha256` in the log entry | 9, 28 | 9. Decision Q9 |
| 39 | `[custody] require_case_details` | 8, 30 | Off by default. Refuses an empty case reference at the prompt. UX decision Q27 |

Item 24 and item 27, the exact text. The example in Appendices A and B already uses the new text.

| Where | Today | Revision 4 asks for |
|---|---|---|
| `repair.rs` 690, C9 action, Exact | `Flate data repaired ({edits}); no other repair fits its window` | `Flate data repaired ({edits}); every single-byte replacement in its window was tried, and every one that passed the Adler-32 gave this output` |
| `diagnose.rs` 1726, C9 finding, Exact | `the Flate data of {n} {g} obj is damaged: repaired; no other repair fits its window` | `the Flate data of {n} {g} obj is damaged: repaired; every single-byte replacement in its window that passed the Adler-32 gave the same output` |
| `repair.rs`, C9 action, Exact with `trailer_edit` | same as Exact | `Adler-32 trailer rewritten ({edits}) to match the decoded data; no replacement in the data passed the check, so no checksum covers the data` |
| `diagnose.rs`, C9 finding, Exact with `trailer_edit` | same as Exact | `the Flate data of {n} {g} obj is damaged: its stored Adler-32 was rewritten to match the decoded data, which no checksum covers` |
| `report.rs` 25 `SIGNED_NOTE` | `the original is digitally signed; the repaired file is not` | `the input is digitally signed; the repaired file is not` |
| `diagnose.rs` 134 `SIGNED_SUMMARY` | `the original is digitally signed; a repaired file is not` | `the input is digitally signed; a repaired file is not` |

## 15. Decisions

The user answered every open question on 2026-10-09, each as recommended. The answers are in `decision-log.md`, block "Answered by the user, 2026-10-09 (round 3: custody, evidence records and new screens, all as recommended)", commit `dcc0859`. That block numbers them Q27 to Q47 and names the schema question each settles. The schema's own numbers are kept here so earlier references still resolve.

| # | Question | Decision | Decision log | Where applied |
|---|---|---|---|---|
| Q1 | Record names: full output name, or the mockup's `invoice.custody.json`? | Full output name: `<name>.repaired.pdf.custody.json` | Q35 | 2.2 |
| Q2 | Two more prompt fields, submission reference and authority/notes? | Four fields: case reference, examiner, submission reference, authority/notes; the last two optional | Q36 | 4.4 `case` |
| Q3 | Record the host name by default? | Yes, with a `record_host_name` switch | Q37 | 4.4 `host`, `custody_settings` |
| Q4 | Write image files on a plain repair with custody off? | No. Only on Markdown export or in custody mode | Q38 | 6.1 |
| Q5 | Ship the CASE/UCO file in v1? | After the JSON and text records, off by default | Q39 | 2.1, 10 |
| Q6 | Derived viewable PNGs for JPX, CCITT, JBIG2? | Off, behind `[repair] derived_image_views` | Q40 | 6.2 |
| Q7 | UTC offset on Unix? | Filled on every platform, reusing the app's time-zone reader | Q41 | 4.4 `clock`, 5.5, App. A, B. Revision 4's null offset is gone |
| Q8 | Build commit from the release workflow, null locally? | Yes | Q42 | 4.5, 14 item 13 |
| Q9 | Verify file every time? | Yes, `.verify-<n>.txt` every time | Q43 | 9. `<n>` is a per-record counter claimed with `create_new`, so it never collides |
| Q10 | Type3 glyph bitmaps as files? | Off, behind `[repair] extract_type3_glyph_images`; counted always | Q40 | 6.4 |
| Q11 | Buy ISO/IEC 27037 and 27042? | No. The design rests on SWGDE, NIST and ACPO | Q47 | 11.1 |
| Q12 | A per-case custody log? | Not in v1 | Q44 | 8.6 |
| Q13 | Blank fields: "not given" or "not provided"? | "not provided" | Block preamble | 13.1 |
| Q14 | Per-executable SHA-256 published by releases? | Yes, and custody records cite it | Q45 | `tool_identity` statement, 14 item 13 |
| Q15 | A typed reference per file? | No. One submission reference per batch | Block preamble | 4.4 `case` |
| Q16 | Should custody mode stop writing into the input's folder? | Custody mode refuses to start until `[general] output_dir` is set, so the evidence folder stays untouched | Q29 | 2.4, 4.4 `destination`, 14 item 37. The warn-and-continue path is gone |
| Q17 | A `.bin` copy of a repaired image's damaged stream? | No. The record keeps its hash and span | Q46 | 6.2 `raw` |
| UX Q27 | Refuse an empty case reference? | Blank allowed; `require_case_details` refuses it, off by default | Q27 | 4.4 `custody_settings`, 14 item 39 |
| UX Q30 | Kitty drops send bytes at the drop | The prompt says so; no further restriction | Q30 | 2.3.1, 2.3.2 `received` |

## Appendix A. Worked example: the custody record

One repaired file, two extracted images, custody mode on, all three hashes on, on a macOS host. The input had a missing cross-reference table, three deleted font programs, one Flate image with a flipped byte, and one font left unembedded on purpose.

- The input sits in an exhibit folder on a writable volume, with mode 0444. `output_dir` sends every output and record to a separate work folder, as custody mode requires, so nothing is written beside the input.
- Every finding summary and repair action is the engine's own format string at `f5ddfa6`, filled in, from `work/templates.py`. Two are the corrected text of section 14 item 24: the C9 finding and the C9 action.
- Fonts are substituted only on the template-assemble path (`fonts.rs`), so that candidate was built beside resave and chosen.
- `NOEMBED-001` is a `font_not_embedded` finding with its payload. It is still there after the repair, under the output's object numbers.
- `img-0001` is a JPEG on page 1. Raw stream and file hashes are equal because the file is the stream bytes.
- `img-0002` is a grey image drawn on pages 2 and 3. Its inflate failed at offset 1,294, so the one search pass's window is bytes 0 to 1,302. All 332,010 single-byte replacements were tried, one passed, and it was graded Exact. The file is `d013614dc14a37ee.repaired.png` because it holds samples after PDFPundit's edit. Raw, repaired-stream and file hashes all differ.
- Font slot F1 was auto-accepted: confidence 77/100 against a threshold of 35/100, and the best coverage 100/100 met the 95/100 a decoding slot needs. Slot F3 was shown at 14:03:13 and answered at 14:03:16. Slot F5 was carried from that answer and never shown, so its source is `batched` and its `shown_utc` is null.
- The UTC offset is UTC-4, Toronto's daylight time at Started, read from `/etc/localtime` through the app's TZif reader, decision Q7. The zone name is recorded as a hint.
- The input's access time changed during the run, from the job's own read after `stat_before`. Its content hash did not. The record names the difference.

PDF and image file hashes are stand-ins, because those files do not exist. So are object spans and `log_id`, because entries 1 to 40 are not shown. Every hash that covers a file the scripts wrote is real: Part B's hash, `images.json`, `SHA256SUMS`, the record files, the `.json` hash on each `.txt` page and the log chain. `work/validate.py` checks all of them, and runs jq against Python on Part B.

Display note: short arrays and flat objects are shown on one line here. The real file is serde-style pretty JSON, one element per line. The hashes are the same either way, because Part B's hash is taken over the compact form.

```json
{
  "format": "pdfpundit-custody",
  "custody_schema_version": 1,
  "reproducible_sha256": "6a4b244b215df5007efa4a1d3d1d498e425f520e55d4b9253e3a1768e0a355c3",
  "reproducible": {
    "job": "repair",
    "tool": {
      "name": "PDFPundit",
      "version": "0.1.0",
      "build_commit": "4015b57a143aec5156fd1444a017a32137a3fd0f",
      "build_origin": "release",
      "platform": {"os": "macos", "arch": "aarch64"},
      "report_schema_version": 2,
      "font_db_sha256": "8b85da72b2520c0f15cfb0e0f8b9b9597b47af910c613e90bd81b5e63ff61f30"
    },
    "settings": {
      "salvage_work": 700000000,
      "salvage_deep_work": 10000000000,
      "salvage_deep_pool": 80000000000,
      "max_search_stream": 4194304,
      "passes": null,
      "font_policy": "bundled",
      "font_source_effective": "bundled",
      "prompt_unresolved": true,
      "derived_image_views": false,
      "unreproducible": "ask",
      "default_page_size": "a4",
      "extract_unplaceable_images": true,
      "extract_type3_glyph_images": false,
      "auto_accept_confidence": {"num": 35, "den": 100},
      "max_font_candidates": 5
    },
    "input": {
      "sha256": "f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf",
      "size_bytes": 482113,
      "pdf_version": "1.6",
      "pages_discovered": 3,
      "signed_fields": 0,
      "encrypted": false
    },
    "interactions": [
      {
        "seq": 1,
        "kind": "font_pick",
        "page": 2,
        "slot": "F3",
        "candidates": ["dejavu-sans", "liberation-sans", "noto-sans"],
        "reply": {"type": "pick", "font_id": "dejavu-sans"},
        "source": "user",
        "carried_from": null,
        "policy_setting": null
      },
      {
        "seq": 2,
        "kind": "font_pick",
        "page": 3,
        "slot": "F5",
        "candidates": ["dejavu-sans", "liberation-sans", "noto-sans"],
        "reply": {"type": "pick", "font_id": "dejavu-sans"},
        "source": "batched",
        "carried_from": {"type": "this_record", "seq": 1},
        "policy_setting": null
      }
    ],
    "font_decisions": [
      {
        "page": 1,
        "slot": "F1",
        "object": "21 0",
        "basis": "auto_accept",
        "font_id": "liberation-serif",
        "interaction_seq": null,
        "evidence": {
          "top_candidate": "liberation-serif",
          "confidence": {"num": 77, "den": 100},
          "hit": {"num": 94, "den": 100},
          "margin": {"num": 60, "den": 100},
          "threshold": {"num": 35, "den": 100},
          "min_hit": {"num": 1, "den": 2},
          "inference": {"lang": "en", "script": "Latn", "n_chars": 431, "n_runs": 37},
          "sample_floor": 20,
          "tounicode": "missing",
          "best_coverage": {"num": 100, "den": 100},
          "best_coverage_font": "liberation-serif",
          "min_coverage": {"num": 95, "den": 100},
          "decision": "auto_accept"
        },
        "reason": null
      },
      {
        "page": 2,
        "slot": "F3",
        "object": "27 0",
        "basis": "user",
        "font_id": "dejavu-sans",
        "interaction_seq": 1,
        "evidence": {
          "top_candidate": "dejavu-sans",
          "confidence": {"num": 25, "den": 100},
          "hit": {"num": 48, "den": 100},
          "margin": {"num": 2, "den": 100},
          "threshold": {"num": 35, "den": 100},
          "min_hit": {"num": 1, "den": 2},
          "inference": {"lang": "en", "script": "Latn", "n_chars": 212, "n_runs": 19},
          "sample_floor": 20,
          "tounicode": "missing",
          "best_coverage": {"num": 97, "den": 100},
          "best_coverage_font": "dejavu-sans",
          "min_coverage": {"num": 95, "den": 100},
          "decision": "ask"
        },
        "reason": null
      },
      {
        "page": 3,
        "slot": "F5",
        "object": "33 0",
        "basis": "batched",
        "font_id": "dejavu-sans",
        "interaction_seq": 2,
        "evidence": {
          "top_candidate": "dejavu-sans",
          "confidence": {"num": 26, "den": 100},
          "hit": {"num": 51, "den": 100},
          "margin": {"num": 1, "den": 100},
          "threshold": {"num": 35, "den": 100},
          "min_hit": {"num": 1, "den": 2},
          "inference": {"lang": "en", "script": "Latn", "n_chars": 188, "n_runs": 16},
          "sample_floor": 20,
          "tounicode": "missing",
          "best_coverage": {"num": 97, "den": 100},
          "best_coverage_font": "dejavu-sans",
          "min_coverage": {"num": 95, "den": 100},
          "decision": "ask"
        },
        "reason": null
      }
    ],
    "outcome": {
      "status": "repaired",
      "reasons": [],
      "chosen_toolpath": "template_assemble"
    },
    "findings_before": [
      {
        "id": "C2-001",
        "kind": "corruption",
        "class": "C2",
        "severity": "error",
        "location": {"type": "file"},
        "summary": "no cross-reference table or stream",
        "detail": null
      },
      {
        "id": "C9-001",
        "kind": "corruption",
        "class": "C9",
        "severity": "warning",
        "location": {
          "type": "object",
          "object": "14 0",
          "span": {"start": 232950, "end": 251431}
        },
        "summary": "the Flate data of 14 0 obj is damaged: repaired; every single-byte replacement in its window that passed the Adler-32 gave the same output",
        "detail": null
      },
      {
        "id": "C7-001",
        "kind": "corruption",
        "class": "C7",
        "severity": "error",
        "location": {
          "type": "object",
          "object": "23 0",
          "span": {"start": 252100, "end": 252412}
        },
        "summary": "the font program of CIDFont+F1 is gone: its /FontFile2 names 24 0 R, which is not a stream",
        "detail": null
      },
      {
        "id": "C7-002",
        "kind": "corruption",
        "class": "C7",
        "severity": "error",
        "location": {
          "type": "object",
          "object": "29 0",
          "span": {"start": 255300, "end": 255611}
        },
        "summary": "the font program of CIDFont+F3 is gone: its /FontFile2 names 30 0 R, which is not a stream",
        "detail": null
      },
      {
        "id": "C7-003",
        "kind": "corruption",
        "class": "C7",
        "severity": "error",
        "location": {
          "type": "object",
          "object": "35 0",
          "span": {"start": 258700, "end": 259013}
        },
        "summary": "the font program of CIDFont+F5 is gone: its /FontFile2 names 36 0 R, which is not a stream",
        "detail": null
      },
      {
        "id": "NOEMBED-001",
        "kind": "font_not_embedded",
        "class": null,
        "severity": "info",
        "location": {
          "type": "object",
          "object": "41 0",
          "span": {"start": 262040, "end": 262318}
        },
        "summary": "the font ArialMT is not embedded",
        "detail": {"type": "font_not_embedded", "font": "40 0", "base_font": "ArialMT"}
      }
    ],
    "findings_after": [
      {
        "id": "NOEMBED-001",
        "kind": "font_not_embedded",
        "class": null,
        "severity": "info",
        "location": {
          "type": "object",
          "object": "13 0",
          "span": {"start": 30411, "end": 30690}
        },
        "summary": "the font ArialMT is not embedded",
        "detail": {"type": "font_not_embedded", "font": "12 0", "base_font": "ArialMT"}
      }
    ],
    "passes": [
      {
        "class": "C2",
        "outcome": "fixed",
        "note": null,
        "actions": [
          {"object": null, "what": "C2-001: resolved by re-emission", "grade": null}
        ]
      },
      {
        "class": "C7",
        "outcome": "fixed",
        "note": null,
        "actions": [
          {
            "object": "23 0",
            "what": "font program substituted: liberation-serif",
            "grade": null
          },
          {"object": "29 0", "what": "font program substituted: dejavu-sans", "grade": null},
          {"object": "35 0", "what": "font program substituted: dejavu-sans", "grade": null}
        ]
      },
      {
        "class": "C9",
        "outcome": "fixed",
        "note": null,
        "actions": [
          {
            "object": "14 0",
            "what": "Flate data repaired (byte 1207 0x3a -> 0x7a); every single-byte replacement in its window was tried, and every one that passed the Adler-32 gave this output",
            "grade": "exact"
          }
        ]
      }
    ],
    "candidates": [
      {
        "toolpath": "resave",
        "chosen": false,
        "verification": {
          "baseline": "extracted",
          "gates": {
            "lopdf_reload": true,
            "hayro_render_all_pages": true,
            "rediagnose_clean": true,
            "page_count_ok": true,
            "root_and_pages_present": true
          },
          "retention": {
            "text_ops": {"num": 1412, "den": 1412},
            "path_fills": {"num": 88, "den": 88},
            "images": {"num": 2, "den": 2},
            "content_bytes": {"num": 61230, "den": 61230},
            "blank_pages": 0,
            "glyph_count": {"num": 5120, "den": 5120}
          },
          "plausibility": {
            "unmapped_glyph": {"num": 1840, "den": 5120},
            "fffd": {"num": 0, "den": 5120},
            "script_consistency": {"num": 5120, "den": 5120},
            "dictionary_hit": {"num": 902, "den": 940}
          }
        }
      },
      {
        "toolpath": "template_assemble",
        "chosen": true,
        "verification": {
          "baseline": "extracted",
          "gates": {
            "lopdf_reload": true,
            "hayro_render_all_pages": true,
            "rediagnose_clean": true,
            "page_count_ok": true,
            "root_and_pages_present": true
          },
          "retention": {
            "text_ops": {"num": 1412, "den": 1412},
            "path_fills": {"num": 88, "den": 88},
            "images": {"num": 2, "den": 2},
            "content_bytes": {"num": 61230, "den": 61230},
            "blank_pages": 0,
            "glyph_count": {"num": 5120, "den": 5120}
          },
          "plausibility": {
            "unmapped_glyph": {"num": 0, "den": 5120},
            "fffd": {"num": 0, "den": 5120},
            "script_consistency": {"num": 5120, "den": 5120},
            "dictionary_hit": {"num": 902, "den": 940}
          }
        }
      }
    ],
    "stream_salvage": {
      "summary": {
        "streams_damaged": 1,
        "repaired": 1,
        "exact": 1,
        "accepted": 0,
        "ambiguous": 0,
        "checksum_mismatch": 0,
        "prefix": 0,
        "unrecoverable": 0,
        "unsearched": 0
      },
      "streams": [
        {
          "object": "14 0",
          "search": {
            "outcome": "repaired",
            "grade": "exact",
            "adler_rerun": false,
            "trailer_edit": false,
            "predictor_stopped": false,
            "prefix": null,
            "edit_domain": {
              "kind": "raw_stream_data",
              "filters_before": []
            },
            "passes": [
              {
                "mode": "zlib",
                "damage": {"kind": "inflate_error", "at": 1294},
                "localizer": "error_window",
                "check": "adler32",
                "deep": false,
                "window": [
                  {"start": 0, "end": 1302}
                ],
                "candidates_in_window": 332010,
                "candidates_tried": 332010
              }
            ],
            "passing_edits": 1,
            "distinct_outputs": 1,
            "edits": [[1207, 58, 122]]
          }
        }
      ],
      "filter_rewrites": []
    },
    "shadows": [],
    "reals_narrowed": 0,
    "text_warnings": [],
    "products": [
      {
        "role": "repaired_pdf",
        "file": null,
        "sha256": "915a4fded8dc1ab2e2b2ed7a0fb99cfaf6f9c098c5f079a6abb3bc8cd4be533d",
        "size_bytes": 489302
      },
      {
        "role": "image",
        "file": "1b48e21282963dfb.jpg",
        "sha256": "1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9",
        "size_bytes": 211876
      },
      {
        "role": "image",
        "file": "d013614dc14a37ee.repaired.png",
        "sha256": "d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e",
        "size_bytes": 9871
      },
      {
        "role": "images_index",
        "file": "images.json",
        "sha256": "977e42299a55c62c0c19ce87d4189e4c4b490b2e8cf875d446acf2a04ebb35a7",
        "size_bytes": 6196
      },
      {
        "role": "images_checksums",
        "file": "SHA256SUMS",
        "sha256": "113f8e25bbded892b25c735410384677b8f594f06318727ca2e89c87345fdcf9",
        "size_bytes": 261
      }
    ],
    "images": {
      "dir": "f7c39aa7.images",
      "type3_glyph_images": 0,
      "records": [
        {
          "id": "img-0001",
          "file": "1b48e21282963dfb.jpg",
          "file_sha256": "1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9",
          "file_len": 211876,
          "format": "jpg",
          "view_file": null,
          "view_sha256": null,
          "source": {
            "kind": "xobject",
            "copy": "winner",
            "object": "9 0",
            "output_object": "5 0",
            "parent": null,
            "inline_origin": null,
            "raw": {
              "domain": "file",
              "span": {"start": 20311, "end": 232187},
              "container_object": null,
              "container_file_span": null
            },
            "raw_len": 211876,
            "raw_sha256": "1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9",
            "length_source": "declared"
          },
          "encoding": {
            "filters": ["DCTDecode"],
            "decode_parms": [null],
            "codec": "jpeg",
            "jbig2_globals": null
          },
          "description": {
            "width": 1240,
            "height": 1754,
            "actual_width": 1240,
            "actual_height": 1754,
            "bits_per_component": 8,
            "components": 3,
            "colour_space": "/DeviceRGB",
            "icc": null,
            "palette_entries": null,
            "decode": null,
            "image_mask": false,
            "interpolate": false,
            "intent": null,
            "needs_pdf_context": false
          },
          "placements": [
            {
              "page_in": 1,
              "page_out": 1,
              "paint_order": 0,
              "bbox_mpt": [36000, 51000, 559000, 791000],
              "matrix_micro": [523000000, 0, 0, 740000000, 36000000, 51000000],
              "form_chain": []
            }
          ],
          "integrity": {
            "status": "intact",
            "rows_written": null,
            "rows_declared": null,
            "salvage": null,
            "repaired_stream_sha256": null,
            "samples_sha256": null,
            "warnings": []
          }
        },
        {
          "id": "img-0002",
          "file": "d013614dc14a37ee.repaired.png",
          "file_sha256": "d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e",
          "file_len": 9871,
          "format": "png",
          "view_file": null,
          "view_sha256": null,
          "source": {
            "kind": "xobject",
            "copy": "winner",
            "object": "14 0",
            "output_object": "8 0",
            "parent": null,
            "inline_origin": null,
            "raw": {
              "domain": "file",
              "span": {"start": 233010, "end": 251412},
              "container_object": null,
              "container_file_span": null
            },
            "raw_len": 18402,
            "raw_sha256": "548430417123e2a2ea1f043941e56afbbc529274f270e716a32bfb32ff8d0d10",
            "length_source": "declared"
          },
          "encoding": {
            "filters": ["FlateDecode"],
            "decode_parms": ["<</BitsPerComponent 8 /Colors 1 /Columns 600 /Predictor 15>>"],
            "codec": null,
            "jbig2_globals": null
          },
          "description": {
            "width": 600,
            "height": 200,
            "actual_width": 600,
            "actual_height": 200,
            "bits_per_component": 8,
            "components": 1,
            "colour_space": "/DeviceGray",
            "icc": null,
            "palette_entries": null,
            "decode": null,
            "image_mask": false,
            "interpolate": false,
            "intent": null,
            "needs_pdf_context": false
          },
          "placements": [
            {
              "page_in": 2,
              "page_out": 2,
              "paint_order": 3,
              "bbox_mpt": [400000, 60000, 550000, 110000],
              "matrix_micro": [150000000, 0, 0, 50000000, 400000000, 60000000],
              "form_chain": []
            },
            {
              "page_in": 3,
              "page_out": 3,
              "paint_order": 1,
              "bbox_mpt": [400000, 60000, 550000, 110000],
              "matrix_micro": [150000000, 0, 0, 50000000, 400000000, 60000000],
              "form_chain": []
            }
          ],
          "integrity": {
            "status": "repaired",
            "rows_written": 200,
            "rows_declared": 200,
            "salvage": {
              "outcome": "repaired",
              "grade": "exact",
              "adler_rerun": false,
              "trailer_edit": false,
              "predictor_stopped": false,
              "prefix": null,
              "edit_domain": {
                "kind": "raw_stream_data",
                "filters_before": []
              },
              "passes": [
                {
                  "mode": "zlib",
                  "damage": {"kind": "inflate_error", "at": 1294},
                  "localizer": "error_window",
                  "check": "adler32",
                  "deep": false,
                  "window": [
                    {"start": 0, "end": 1302}
                  ],
                  "candidates_in_window": 332010,
                  "candidates_tried": 332010
                }
              ],
              "passing_edits": 1,
              "distinct_outputs": 1,
              "edits": [[1207, 58, 122]]
            },
            "repaired_stream_sha256": "94c0f68728561aa1dd9996ff3b13da2e9625145c65d11c9b72a837054482ea6e",
            "samples_sha256": "2d46fdb3cf07a1a3d7faa7644202678229c138ac5c866c50934d041fd0465910",
            "warnings": []
          }
        }
      ]
    },
    "limitations": {
      "standing": [
        "text marked with ActualText is extracted as drawn; the ActualText is not used",
        "glyphs carry an origin and a size but no bounding box",
        "text drawn in render mode 7 (clip only) is not extracted",
        "the interpreter gives no warning for a dropped compressed stream or an unresolved font; unmapped glyphs and the glyph count per page are the integrity signals",
        "image files hold the image as stored, or its decoded samples re-encoded as PNG or TIFF with the same sample values; colour spaces, /Decode arrays and masks are recorded, not applied; an image whose stream was repaired, cut short or left unverified holds those samples, its file name says so, and its record gives the search",
        "a repaired file is a new document: it is not the input and carries none of the input's signatures"
      ],
      "this_run": [
        "1 compressed stream was graded Exact: every single-byte replacement in the searched window was tried, and every one that passed the check gave the same output; damage of other kinds is not searched, and the grade does not show the repaired bytes equal the original bytes",
        "3 font programs were substituted from the bundled font database; glyph shapes are the bundled font's, not those of the font the input embedded",
        "1 of 3 font slots was matched without asking, because its score cleared the auto-accept setting; part B.7 gives the basis for every slot",
        "1 of 2 answers was carried from an earlier answer and not shown to the user"
      ]
    },
    "statements": {
      "role": "This is a tool run record. It supports an examiner's report and is not one. It states no opinion on the document's content, authorship or authenticity.",
      "reproducibility": "Part B is identical, byte for byte, whenever the same input bytes are processed by the same PDFPundit build with the same settings and the same recorded answers. The same build means the same version and build commit, compiled for the same operating system and processor architecture; part B records all four. Identity across platforms is not claimed. This shows the result is repeatable. It does not show the repair is correct.",
      "validation": "PDFPundit has not been validated by an independent body, such as NIST CFTT, and claims no accreditation. Its test suite is in its source repository at the build commit recorded in part B. Its corpus tests use a third-party corpus that the repository names but does not contain.",
      "tool_identity": "Version, build commit and build origin are reported by the program itself, and whoever builds it can set them. Releases are unsigned. Each release publishes the SHA-256 of every executable it ships. To confirm the build, compare the binary SHA-256 in part A with the value published for this version and platform.",
      "input_file": "PDFPundit cannot tell whether its input is the original evidence or a working copy. It opens the input read-only and never writes to it. It is not a write blocker. Unchanged means the SHA-256 of the input's content was the same before and after the run. File-system times before and after the run are recorded in part A. Reading a file can itself update its access time. In custody mode, outputs and records are written only to the output folder named in part A. Custody mode does not start while no output folder is set, or while it is the folder of an input.",
      "case_details": "Case reference, examiner, submission reference and notes are typed at the custody prompt, once per batch. PDFPundit records them as typed and does not verify them.",
      "times": "Times come from the host clock, in UTC, and are not independently verified. Started is read just before the job first reads the input's file state. An input received whole at the drop was read before that, before the custody prompt; part A says how the input was received. Ended is read just after the input's file state is read again, before the record files are written. The log entry for this record is timed after those files are written.",
      "log": "The custody log is hash-chained. It is tamper-evident, not tamper-proof. The chain shows a changed or removed entry only while the entries after it are unchanged: anyone who can write the file can recompute every later entry, and the chain then verifies. Only a log head copied outside the log, such as the one shown when a batch closes, or an unedited copy of the log catches that; such a head pins the log up to its entry. PDFPundit also compares the log with the last head kept in its history when custody mode starts, and starts a new log naming the old one when they differ; a change made to both is not detected.",
      "legacy_hashes": "MD5 and SHA-1 are included only to match older case systems. Neither is collision-resistant. SHA-256 is the integrity hash."
    }
  },
  "run": {
    "job_status": "completed",
    "batch": {"log_seq": 41, "position": 1, "count": 1},
    "case": {
      "prompt_shown": true,
      "case_reference": "2026-0412",
      "examiner": "J. Doe",
      "submission_reference": "Lab request 26-0412-07",
      "notes": "Authority: warrant 26-1187."
    },
    "clock": {
      "started_utc": "2026-10-08T14:03:11Z",
      "ended_utc": "2026-10-08T14:03:19Z",
      "host_utc_offset_minutes": -240,
      "host_utc_offset_from": "etc_localtime_tzif",
      "host_zone_hint": {"name": "America/Toronto", "from": "etc_localtime_link"},
      "source": "host_clock_unverified"
    },
    "answer_times": [
      {"seq": 1, "shown_utc": "2026-10-08T14:03:13Z", "answered_utc": "2026-10-08T14:03:16Z"},
      {"seq": 2, "shown_utc": null, "answered_utc": "2026-10-08T14:03:16Z"}
    ],
    "host": {
      "name": "EXAM-WS-07",
      "os": "macos",
      "tool_binary_sha256": "9a3a45d01531a20e89ac6ae10b0b0beb0492acd7216a368aa062d1a5fecaf9cd"
    },
    "custody_settings": {
      "hashes": ["sha256", "sha1", "md5"],
      "ask_case_details": true,
      "require_case_details": false,
      "record_host_name": true,
      "case_jsonld": false
    },
    "input_file": {
      "name": "invoice_scan_2024.pdf",
      "path": "/Volumes/EXHIBIT-JD3/exhibits/invoice_scan_2024.pdf",
      "path_kind": "file",
      "first_read": "job_start",
      "size_bytes": 482113,
      "hashes_before": {
        "sha256": "f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf",
        "sha1": "190bba18a0fd345b373fe63d00764f58539eccb3",
        "md5": "3c7775b0a27a60274cc65fa82c84d37d"
      },
      "hashes_after": {
        "sha256": "f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf",
        "sha1": "190bba18a0fd345b373fe63d00764f58539eccb3",
        "md5": "3c7775b0a27a60274cc65fa82c84d37d"
      },
      "unchanged": "yes",
      "not_rechecked_reason": null,
      "stat_before": {
        "size_bytes": 482113,
        "modified": {"sec": 1790845964, "nsec": 0},
        "changed": {"sec": 1790959205, "nsec": 120000000},
        "accessed": {"sec": 1791397351, "nsec": 4000000},
        "created": {"sec": 1790845964, "nsec": 0},
        "read_only": true,
        "unix_mode": 33060,
        "source": "unix_stat"
      },
      "stat_after": {
        "size_bytes": 482113,
        "modified": {"sec": 1790845964, "nsec": 0},
        "changed": {"sec": 1790959205, "nsec": 120000000},
        "accessed": {"sec": 1791468191, "nsec": 402000000},
        "created": {"sec": 1790845964, "nsec": 0},
        "read_only": true,
        "unix_mode": 33060,
        "source": "unix_stat"
      },
      "stat_changed": ["accessed"]
    },
    "destination": {"path": "/Cases/2026-0412/work"},
    "placed": [
      {
        "role": "repaired_pdf",
        "name": "invoice_scan_2024.repaired.pdf",
        "path": "/Cases/2026-0412/work/invoice_scan_2024.repaired.pdf",
        "size_bytes": 489302,
        "hashes": {
          "sha256": "915a4fded8dc1ab2e2b2ed7a0fb99cfaf6f9c098c5f079a6abb3bc8cd4be533d",
          "sha1": "db578c7a23ab2ee75ecd2439f56a0e0e868e9c51",
          "md5": "fb972408d1eb9128d8f876eacf6d7aed"
        },
        "placement": "atomic",
        "matches_product": true,
        "note": null
      },
      {
        "role": "image",
        "name": "f7c39aa7.images/1b48e21282963dfb.jpg",
        "path": "/Cases/2026-0412/work/f7c39aa7.images/1b48e21282963dfb.jpg",
        "size_bytes": 211876,
        "hashes": {
          "sha256": "1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9",
          "sha1": "014c3808ba7c76eaea56461402e8e7bedf8004ff",
          "md5": "2f6f0eb5cc2c15df39fee0e84285812c"
        },
        "placement": "create_new",
        "matches_product": true,
        "note": null
      },
      {
        "role": "image",
        "name": "f7c39aa7.images/d013614dc14a37ee.repaired.png",
        "path": "/Cases/2026-0412/work/f7c39aa7.images/d013614dc14a37ee.repaired.png",
        "size_bytes": 9871,
        "hashes": {
          "sha256": "d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e",
          "sha1": "01cd9ac66b03257c54e767745c2e9285502761d9",
          "md5": "edbbdc3d21d008abbfa759d1d7f1ea26"
        },
        "placement": "create_new",
        "matches_product": true,
        "note": null
      },
      {
        "role": "images_index",
        "name": "f7c39aa7.images/images.json",
        "path": "/Cases/2026-0412/work/f7c39aa7.images/images.json",
        "size_bytes": 6196,
        "hashes": {
          "sha256": "977e42299a55c62c0c19ce87d4189e4c4b490b2e8cf875d446acf2a04ebb35a7",
          "sha1": "94ef93d7e1cf238359002439816737e413ab4b9b",
          "md5": "a5c67da734313f3ecd5c65bd7fe60dee"
        },
        "placement": "create_new",
        "matches_product": true,
        "note": null
      },
      {
        "role": "images_checksums",
        "name": "f7c39aa7.images/SHA256SUMS",
        "path": "/Cases/2026-0412/work/f7c39aa7.images/SHA256SUMS",
        "size_bytes": 261,
        "hashes": {
          "sha256": "113f8e25bbded892b25c735410384677b8f594f06318727ca2e89c87345fdcf9",
          "sha1": "770f55991e7b04241da720a4eaa3982577fea4cc",
          "md5": "cd358fa1e8f2d42d07e5745734c0a630"
        },
        "placement": "create_new",
        "matches_product": true,
        "note": null
      }
    ],
    "images_dir": {
      "path": "/Cases/2026-0412/work/f7c39aa7.images",
      "name": "f7c39aa7.images",
      "outcome": "created"
    },
    "processing": {
      "analysis_state": "reused",
      "rebuilt_reason": null,
      "config_path": "/Users/examiner/Library/Application Support/PDFPundit/config.toml"
    },
    "record_files": {
      "json": "invoice_scan_2024.repaired.pdf.custody.json",
      "text": "invoice_scan_2024.repaired.pdf.custody.txt",
      "checksums": "invoice_scan_2024.repaired.pdf.custody.sha256",
      "case_jsonld": null
    },
    "earlier_records": [],
    "earlier_records_logs": {
      "searched": ["922810eecc0e7fe58cce3e60b63032561a2b2cad348f3b1d7dcff0926a572c28"],
      "missing": null
    },
    "answers_replayed_from": {"type": "none"},
    "log": {
      "path": "/Users/examiner/Library/Application Support/PDFPundit/custody.log",
      "log_id": "922810eecc0e7fe58cce3e60b63032561a2b2cad348f3b1d7dcff0926a572c28",
      "seq": 42,
      "prev_entry_sha256": "fdd8424fa998f4e9c6f49ccffe7feb0d234cb63dbf257afeedf6d249fd6685f7"
    }
  }
}
```

## Appendix B. Worked example: the text report

Form feeds are shown as a line of `~` here.

```text
PDFPUNDIT TOOL RUN RECORD                                            Page 1 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
This is a tool run record. It supports an examiner's report and is not one. It
states no opinion on the document's content, authorship or authenticity.

Part A, run context, is true of this run only. Part B, reproducible result, is
the same on every re-run with the same input, build, settings and answers.
Abbreviations are listed in part C. JSON SHA-256 in the page header is the
SHA-256 of the record file this text was rendered from. That file is written
first, so the hash ties every printed page to one machine record.

A. RUN CONTEXT
--------------------------------------------------------------------------------
Case reference  2026-0412
Examiner        J. Doe
Submission ref  Lab request 26-0412-07
                applies to the 1 file of this batch; the item itself is
                identified by its input SHA-256
Authority/notes Authority: warrant 26-1187.
                Typed at the custody prompt; not verified by PDFPundit.
Batch           custody log entry 41, file 1 of 1
Started (UTC)   2026-10-08 14:03:11Z, before the job first read the input's file
                state
Ended (UTC)     2026-10-08 14:03:19Z, before the record was written
Host UTC offset UTC-04:00 at Started, from the /etc/localtime zone file
Host zone name  America/Toronto (from /etc/localtime link; unverified)
Host            EXAM-WS-07 (macOS)
Binary SHA-256  9a3a45d01531a20e89ac6ae10b0b0beb0492acd7216a368aa062d1a5fecaf9cd

A.1 Input file (as received by PDFPundit)
PDFPundit cannot tell whether this file is the original evidence or a working
copy.
Name            invoice_scan_2024.pdf
Path            /Volumes/EXHIBIT-JD3/exhibits/invoice_scan_2024.pdf
Received        by path; first opened by the job, after the file state below was
                read
Size            482,113 bytes
SHA-256 before  f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf
SHA-256 after   f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf
MD5 legacy      3c7775b0a27a60274cc65fa82c84d37d
SHA-1 legacy    190bba18a0fd345b373fe63d00764f58539eccb3
Content         UNCHANGED. SHA-256 before equals SHA-256 after.
Opened          read-only; PDFPundit never writes to its input

File state      before                          after
  Modified      2026-10-01 09:12:44.000000000Z  same
  Changed       2026-10-02 16:40:05.120000000Z  same
  Accessed      2026-10-07 18:22:31.004000000Z  2026-10-08 14:03:11.402000000Z
  Created       2026-10-01 09:12:44.000000000Z  same
  Read-only     yes                             same
  Mode          444                             same
Differences     accessed. Reading a file can update its access time. The content
                hash above is unaffected.
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 2 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
A.2 Files written
Destination     /Cases/2026-0412/work
                set by output_dir, which custody mode requires; never an input's
                own folder

File 1          invoice_scan_2024.repaired.pdf
  Kind          repaired pdf
  Size          489,302 bytes
  SHA-256       915a4fded8dc1ab2e2b2ed7a0fb99cfaf6f9c098c5f079a6abb3bc8cd4be533d
  MD5 legacy    fb972408d1eb9128d8f876eacf6d7aed
  SHA-1 legacy  db578c7a23ab2ee75ecd2439f56a0e0e868e9c51
  Check         re-read after writing; matches part B

File 2          f7c39aa7.images/1b48e21282963dfb.jpg
  Kind          image
  Size          211,876 bytes
  SHA-256       1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9
  MD5 legacy    2f6f0eb5cc2c15df39fee0e84285812c
  SHA-1 legacy  014c3808ba7c76eaea56461402e8e7bedf8004ff
  Check         re-read after writing; matches part B

File 3          f7c39aa7.images/d013614dc14a37ee.repaired.png
  Kind          image
  Size          9,871 bytes
  SHA-256       d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e
  MD5 legacy    edbbdc3d21d008abbfa759d1d7f1ea26
  SHA-1 legacy  01cd9ac66b03257c54e767745c2e9285502761d9
  Check         re-read after writing; matches part B

File 4          f7c39aa7.images/images.json
  Kind          images index
  Size          6,196 bytes
  SHA-256       977e42299a55c62c0c19ce87d4189e4c4b490b2e8cf875d446acf2a04ebb35a7
  MD5 legacy    a5c67da734313f3ecd5c65bd7fe60dee
  SHA-1 legacy  94ef93d7e1cf238359002439816737e413ab4b9b
  Check         re-read after writing; matches part B

File 5          f7c39aa7.images/SHA256SUMS
  Kind          images checksums
  Size          261 bytes
  SHA-256       113f8e25bbded892b25c735410384677b8f594f06318727ca2e89c87345fdcf9
  MD5 legacy    cd358fa1e8f2d42d07e5745734c0a630
  SHA-1 legacy  770f55991e7b04241da720a4eaa3982577fea4cc
  Check         re-read after writing; matches part B
Folder          /Cases/2026-0412/work/f7c39aa7.images (created)

B. REPRODUCIBLE RESULT
--------------------------------------------------------------------------------
Part B SHA-256  6a4b244b215df5007efa4a1d3d1d498e425f520e55d4b9253e3a1768e0a355c3
Part B SHA-256 is the hash of the 'reproducible' member of the JSON record in
compact form. A re-run that gives the same hash gave the same result.
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 3 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
B.1 Tool
Tool            PDFPundit 0.1.0
Build commit    4015b57a143aec5156fd1444a017a32137a3fd0f
Build origin    release build, as reported by the program
Platform        macOS, aarch64; part of the build for re-runs
Font DB SHA-256 8b85da72b2520c0f15cfb0e0f8b9b9597b47af910c613e90bd81b5e63ff61f30
Report schema   version 2; custody schema version 1

B.2 Settings
Passes          all (C1 to C10)
Salvage work    700,000,000 (deep 10,000,000,000, pool 80,000,000,000)
Search limit    4,194,304 bytes
Font source     bundled
Font questions  asked on screen
Image views     none
No-match fonts  ask
Page size       a4
Unplaced images extracted
Type3 glyphs    counted only
Font auto-pick  confidence 35/100 or higher
Font candidates at most 5

B.3 Input and outcome
Input SHA-256   f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf
Input size      482,113 bytes, PDF 1.6, 3 pages found
Outcome         REPAIRED
Method          template assemble, chosen over resave
Output SHA-256  915a4fded8dc1ab2e2b2ed7a0fb99cfaf6f9c098c5f079a6abb3bc8cd4be533d
Output checks   reloads; every page renders; re-diagnosis clean;
                page count matches; catalog and page tree present
Kept            compared with what was readable in the damaged input, not with
                the undamaged document (baseline: extracted)
                text operators 1,412/1,412; images 2/2; glyphs 5,120/5,120

B.4 Findings
Objects are named as in the input. A finding still present after the repair is
named as in the output.
ID           Class  Severity  Where      Before -> after
C2-001       C2     error     file       found -> gone
             no cross-reference table or stream
C9-001       C9     warning   obj 14 0   found -> gone
             the Flate data of 14 0 obj is damaged: repaired; every single-byte
             replacement in its window that passed the Adler-32 gave the same
             output
C7-001       C7     error     obj 23 0   found -> gone
             the font program of CIDFont+F1 is gone: its /FontFile2 names 24 0
             R, which is not a stream
C7-002       C7     error     obj 29 0   found -> gone
             the font program of CIDFont+F3 is gone: its /FontFile2 names 30 0
             R, which is not a stream
C7-003       C7     error     obj 35 0   found -> gone
             the font program of CIDFont+F5 is gone: its /FontFile2 names 36 0
             R, which is not a stream
NOEMBED-001  -      info      obj 41 0   found -> found, obj 13 0
             the font ArialMT is not embedded
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 4 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
B.5 Repair actions
Pass  Object   Grade   Action
C2    file     -       C2-001: resolved by re-emission
C7    23 0     -       font program substituted: liberation-serif
C7    29 0     -       font program substituted: dejavu-sans
C7    35 0     -       font program substituted: dejavu-sans
C9    14 0     exact   Flate data repaired (byte 1207 0x3a -> 0x7a); every
                       single-byte replacement in its window was tried, and
                       every one that passed the Adler-32 gave this output

B.6 Compressed-stream salvage
Damaged streams 1: repaired 1 (Exact 1, Accepted 0, Ambiguous 0); unverified 0;
                cut short 0; unrecoverable 0; not searched 0
obj 14 0        repaired, graded Exact
  Pass 1        zlib; inflate error at offset 1,294 of the stream data
  Window        error window: 4,096 bytes before the inflate error to 8 after
  Searched      bytes 0 to 1,302 of the stream data; single-byte replacements
                tried 332,010 of 332,010
  Check         Adler-32
  Passed check  1 replacement, 1 distinct output
  Edit          offset 1,207 in the stream data (file offset 234,217), 0x3a ->
                0x7a

B.7 Font slots and answers
Questions asked
#   Page  Slot  Answer                Answered by
1   2     F3    font dejavu-sans      user, on screen, 14:03:16
2   3     F5    font dejavu-sans      carried from #1, not shown, 14:03:16
Times are UTC, when the answer was given in this run.

Every font slot
Page  Slot  Object  Font               Basis
1     F1    21 0    liberation-serif   chosen without asking: confidence 77/100
                                       >= 35/100, hit 94/100 >= 1/2, 431 chars
                                       >= 20; coverage 100/100 (needs 95/100);
                                       /ToUnicode missing
2     F3    27 0    dejavu-sans        answer #1; asked because confidence
                                       25/100 < 35/100 and hit 48/100 < 1/2;
                                       /ToUnicode missing, so coverage not
                                       required
3     F5    33 0    dejavu-sans        answer #2, not asked: carried from #1;
                                       the score alone would have asked,
                                       confidence 26/100 < 35/100; /ToUnicode
                                       missing, so coverage not required
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 5 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
B.8 Extracted images
img-0001        f7c39aa7.images/1b48e21282963dfb.jpg
  Source        obj 9 0 (obj 5 0 in repaired PDF), xobject
  Pages         1
  Filters       DCTDecode
  Image         1240 x 1754, 8 bit, /DeviceRGB
  In input file offset 20,311, 211,876 bytes
  Raw SHA-256   1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9
  File SHA-256  1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9
  Status        intact
  File holds    the stream bytes as found, unchanged

img-0002        f7c39aa7.images/d013614dc14a37ee.repaired.png
  Source        obj 14 0 (obj 8 0 in repaired PDF), xobject
  Pages         2, 3
  Filters       FlateDecode
  Image         600 x 200, 8 bit, /DeviceGray
  In input file offset 233,010, 18,402 bytes
  Raw SHA-256   548430417123e2a2ea1f043941e56afbbc529274f270e716a32bfb32ff8d0d10
  File SHA-256  d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e
  Status        repaired, graded exact
  Search        see B.6, obj 14 0
  File holds    decoded samples after PDFPundit's salvage, as PNG; not an
                as-found extraction. The name says 'repaired'

B.9 Limitations
Standing limits of this version:
- text marked with ActualText is extracted as drawn; the ActualText is not used
- glyphs carry an origin and a size but no bounding box
- text drawn in render mode 7 (clip only) is not extracted
- the interpreter gives no warning for a dropped compressed stream or an
  unresolved font; unmapped glyphs and the glyph count per page are the
  integrity signals
- image files hold the image as stored, or its decoded samples re-encoded as PNG
  or TIFF with the same sample values; colour spaces, /Decode arrays and masks
  are recorded, not applied; an image whose stream was repaired, cut short or
  left unverified holds those samples, its file name says so, and its record
  gives the search
- a repaired file is a new document: it is not the input and carries none of the
  input's signatures
Limits of this run:
- 1 compressed stream was graded Exact: every single-byte replacement in the
  searched window was tried, and every one that passed the check gave the same
  output; damage of other kinds is not searched, and the grade does not show the
  repaired bytes equal the original bytes
- 3 font programs were substituted from the bundled font database; glyph shapes
  are the bundled font's, not those of the font the input embedded
- 1 of 3 font slots was matched without asking, because its score cleared the
  auto-accept setting; part B.7 gives the basis for every slot
- 1 of 2 answers was carried from an earlier answer and not shown to the user
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 6 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
B.10 Statements
Part B is identical, byte for byte, whenever the same input bytes are processed
by the same PDFPundit build with the same settings and the same recorded
answers. The same build means the same version and build commit, compiled for
the same operating system and processor architecture; part B records all four.
Identity across platforms is not claimed. This shows the result is repeatable.
It does not show the repair is correct.

PDFPundit has not been validated by an independent body, such as NIST CFTT, and
claims no accreditation. Its test suite is in its source repository at the build
commit recorded in part B. Its corpus tests use a third-party corpus that the
repository names but does not contain.

Version, build commit and build origin are reported by the program itself, and
whoever builds it can set them. Releases are unsigned. Each release publishes
the SHA-256 of every executable it ships. To confirm the build, compare the
binary SHA-256 in part A with the value published for this version and platform.

PDFPundit cannot tell whether its input is the original evidence or a working
copy. It opens the input read-only and never writes to it. It is not a write
blocker. Unchanged means the SHA-256 of the input's content was the same before
and after the run. File-system times before and after the run are recorded in
part A. Reading a file can itself update its access time. In custody mode,
outputs and records are written only to the output folder named in part A.
Custody mode does not start while no output folder is set, or while it is the
folder of an input.

Case reference, examiner, submission reference and notes are typed at the
custody prompt, once per batch. PDFPundit records them as typed and does not
verify them.

Times come from the host clock, in UTC, and are not independently verified.
Started is read just before the job first reads the input's file state. An input
received whole at the drop was read before that, before the custody prompt; part
A says how the input was received. Ended is read just after the input's file
state is read again, before the record files are written. The log entry for this
record is timed after those files are written.

The custody log is hash-chained. It is tamper-evident, not tamper-proof. The
chain shows a changed or removed entry only while the entries after it are
unchanged: anyone who can write the file can recompute every later entry, and
the chain then verifies. Only a log head copied outside the log, such as the one
shown when a batch closes, or an unedited copy of the log catches that; such a
head pins the log up to its entry. PDFPundit also compares the log with the last
head kept in its history when custody mode starts, and starts a new log naming
the old one when they differ; a change made to both is not detected.

MD5 and SHA-1 are included only to match older case systems. Neither is
collision-resistant. SHA-256 is the integrity hash.
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
PDFPUNDIT TOOL RUN RECORD                                            Page 7 of 7
Record invoice_scan_2024.repaired.pdf.custody.json
JSON SHA-256    7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804
================================================================================
C. ABBREVIATIONS
--------------------------------------------------------------------------------
C1..C10     the ten damage classes PDFPundit repairs; listed in its manual
Exact       every single-byte replacement in the searched window was tried, and
            every one that passed the check gave the same output. Damage of
            other kinds is not searched. The grade does not show the repaired
            bytes equal the original bytes
Accepted    the work limit ran out before every single-byte replacement in the
            window was tried; every one that passed the check up to then gave
            the same output
Ambiguous   replacements that passed the check gave two or more different
            outputs, or the stream is raw deflate with no Adler-32; a fixed
            order chose one
unverified  decoded bytes no checksum confirms; written decoded, with no grade
obj N G     PDF object number N, generation G
SHA-256     integrity hash; MD5 and SHA-1 are legacy hashes
resave      one of two ways PDFPundit builds an output: the damaged file
            re-written; see its manual
template assemble
            the other way: the same, with damaged font programs substituted from
            the bundled fonts
UTC         Coordinated Universal Time

D. CUSTODY LOG AND SIGN-OFF
--------------------------------------------------------------------------------
Custody log     /Users/examiner/Library/Application Support/PDFPundit/
                custody.log
Log identity    922810eecc0e7fe58cce3e60b63032561a2b2cad348f3b1d7dcff0926a572c28
This record     entry 42
Log entry before this record
  Entry         41, batch opened
  SHA-256       fdd8424fa998f4e9c6f49ccffe7feb0d234cb63dbf257afeedf6d249fd6685f7
This page cannot show the log head after this record. The log entry for this
record holds this page's hash, so it is written after this page. When the batch
closes, PDFPundit shows the log head on screen, and Verify prints it later. Copy
it into your notes. It pins the log, this record included, at that point.

Reviewed by (name): ______________________________________________

Signature: ___________________________   Date: ____________________
```

## Appendix C. Worked example: log and manifests

`custody.log`, entries 41 to 43. Entry 40 is from an earlier batch and not shown. No line holds a case detail, a file name or a path. The SHA-256 of line 43 is the log head the batch-end screen shows: `93c1e7f493c7277c9548ce6d7dbfc488dc5fb501c94bf0243bcd9162ddee761f`. The SHA-256 of line 42 is what Verify prints as the log head after this record: `52ea592f0e8cd01c29d41a580e8b4153c5bcb9bbe84227c49657799c2df9d1a9`.

```text
{"seq":41,"kind":"batch_opened","time_utc":"2026-10-08T14:03:09Z","prev":"f50af742dc15c01702a6491818aaa57ddb8ee07ce4454110615f5a882a2d6bad","files":1,"prompt_shown":true,"tool":{"version":"0.1.0","build_commit":"4015b57a143aec5156fd1444a017a32137a3fd0f","build_origin":"release"}}
{"seq":42,"kind":"record","time_utc":"2026-10-08T14:03:19Z","prev":"fdd8424fa998f4e9c6f49ccffe7feb0d234cb63dbf257afeedf6d249fd6685f7","batch_seq":41,"input_sha256":"f7c39aa7e478d51b7d49669703d94df49f158ea1d73b58760601f9c1857c4bdf","outcome":"repaired","input_unchanged":"yes","files":[{"role":"json","sha256":"7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804"},{"role":"text","sha256":"4aee9448ef82b207c7b701f3d6faed47f32a9b8a18210c2b5f2c69f83f33a219"},{"role":"checksums","sha256":"4c8028fc2f719d84f071e623371d63f96f46a4ccf03a55c50fb9580d5c33eac5"}]}
{"seq":43,"kind":"batch_closed","time_utc":"2026-10-08T14:03:20Z","prev":"52ea592f0e8cd01c29d41a580e8b4153c5bcb9bbe84227c49657799c2df9d1a9","batch_seq":41,"records":1,"failed":0}
```

`invoice_scan_2024.repaired.pdf.custody.sha256`. Run `sha256sum -c` or `shasum -a 256 -c` from the destination folder. Rules:

- It lists only files PDFPundit wrote, by name relative to the destination, with `/` as the separator on every OS. The `.case.jsonld`, written after it, is covered by the log entry instead.
- It never lists the input. The input may sit in another folder, as here, or have no durable path at all, as a file promise has. Its hashes are in A.1, and Verify check 2 compares them.
- Names are UTF-8, because output names come from the input's stem through `to_string_lossy`, D-044. A name containing a backslash, a line feed or a carriage return is written the GNU way: the line starts with `\`, and those characters become `\\`, `\n` and `\r`. `example.py` has a test of that rule.
- Lines are sorted by the UTF-8 bytes of the name.

```text
1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9  f7c39aa7.images/1b48e21282963dfb.jpg
113f8e25bbded892b25c735410384677b8f594f06318727ca2e89c87345fdcf9  f7c39aa7.images/SHA256SUMS
d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e  f7c39aa7.images/d013614dc14a37ee.repaired.png
977e42299a55c62c0c19ce87d4189e4c4b490b2e8cf875d446acf2a04ebb35a7  f7c39aa7.images/images.json
915a4fded8dc1ab2e2b2ed7a0fb99cfaf6f9c098c5f079a6abb3bc8cd4be533d  invoice_scan_2024.repaired.pdf
7c569cca28868f73dcabc8a462bed08847c9b421f437f8c881f0390782b15804  invoice_scan_2024.repaired.pdf.custody.json
4aee9448ef82b207c7b701f3d6faed47f32a9b8a18210c2b5f2c69f83f33a219  invoice_scan_2024.repaired.pdf.custody.txt
```

`f7c39aa7.images/SHA256SUMS`:

```text
1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9  1b48e21282963dfb.jpg
d013614dc14a37ee20fe92005737ab7d3427e7e93580ad56ef8a42205e7f7a4e  d013614dc14a37ee.repaired.png
977e42299a55c62c0c19ce87d4189e4c4b490b2e8cf875d446acf2a04ebb35a7  images.json
```

## Appendix D. Worked example: CASE excerpt

The image node and the extraction action from `invoice_scan_2024.repaired.pdf.case.jsonld`:

```json
[
  {
    "@id": "kb:file-735fd1ac-72cd-5ec6-a1ff-bd340f4b632e",
    "@type": "uco-observable:File",
    "uco-core:hasFacet": [
      {
        "@id": "kb:filefacet-c20f185f-40a8-512f-b73a-c2429ac7ad57",
        "@type": "uco-observable:FileFacet",
        "uco-observable:fileName": "f7c39aa7.images/1b48e21282963dfb.jpg",
        "uco-observable:sizeInBytes": 211876
      },
      {
        "@id": "kb:contentfacet-e918de20-cd12-51b0-9d53-8beb3f554f7d",
        "@type": "uco-observable:ContentDataFacet",
        "uco-observable:hash": [
          {
            "@id": "kb:hash-98ad8e0f-9af9-52d7-aacb-853f3e945e93",
            "@type": "uco-types:Hash",
            "uco-types:hashMethod": "SHA256",
            "uco-types:hashValue": {
              "@type": "xsd:hexBinary",
              "@value": "1b48e21282963dfba2ffff3a4c331471242fe42fd0a51161e56df72085c445c9"
            }
          },
          {
            "@id": "kb:hash-3ac0514e-f7a4-58e0-9a5d-3e207fbe0a16",
            "@type": "uco-types:Hash",
            "uco-types:hashMethod": "SHA1",
            "uco-types:hashValue": {
              "@type": "xsd:hexBinary",
              "@value": "014c3808ba7c76eaea56461402e8e7bedf8004ff"
            }
          },
          {
            "@id": "kb:hash-8cd70b56-8332-514a-b554-b227c23c82cf",
            "@type": "uco-types:Hash",
            "uco-types:hashMethod": "MD5",
            "uco-types:hashValue": {
              "@type": "xsd:hexBinary",
              "@value": "2f6f0eb5cc2c15df39fee0e84285812c"
            }
          }
        ]
      },
      {
        "@id": "kb:rasterfacet-0af7bd0a-667d-5939-ab13-fb862a669c50",
        "@type": "uco-observable:RasterPictureFacet",
        "uco-observable:pictureWidth": 1240,
        "uco-observable:pictureHeight": 1754,
        "uco-observable:pictureType": "jpg",
        "uco-observable:bitsPerPixel": 24
      }
    ],
    "case-investigation:wasDerivedFrom": {
      "@id": "kb:file-bf174b93-17b0-5a49-96be-b458a6a1b500"
    }
  },
  {
    "@id": "kb:action-3e9e6492-fd8f-5b6c-9338-52c23b53dca1",
    "@type": "case-investigation:InvestigativeAction",
    "uco-core:name": "Extract images",
    "uco-action:startTime": {
      "@type": "xsd:dateTime",
      "@value": "2026-10-08T14:03:11Z"
    },
    "uco-action:endTime": {
      "@type": "xsd:dateTime",
      "@value": "2026-10-08T14:03:19Z"
    },
    "uco-action:instrument": {
      "@id": "kb:tool-f0a9bb41-0771-5ea2-b3b4-9c04a92da237"
    },
    "uco-action:object": [
      {
        "@id": "kb:file-bf174b93-17b0-5a49-96be-b458a6a1b500"
      }
    ],
    "uco-action:result": [
      {
        "@id": "kb:file-735fd1ac-72cd-5ec6-a1ff-bd340f4b632e"
      },
      {
        "@id": "kb:file-a5eaaa33-b289-56c2-9bb5-c015a3ca9ce6"
      }
    ],
    "uco-action:performer": {
      "@id": "kb:identity-75650e83-83df-58e3-8c49-43b09813573c"
    }
  }
]
```

## Sources

- Research: `research-standards.md`, `research-formats.md`, `research-images.md`, `research-repo.md`, and the sources each one cites. These four stay outside the repository like the fact reports (the plans folder's `README.md` says why).
- Repo: `nimbalyst-local/plans/pdfpundit-desktop-app.md` M6b; `nimbalyst-local/plans/implementation/decision-log.md` D-016, D-020, D-038, D-039, D-041, D-060, D-062, D-073, D-075, D-076, D-104, D-112, D-114, D-125, D-128, D-136, D-141 and the 2026-10-08 answered blocks; `src/engine/report.rs`; `src/pdf/model.rs`; `src/pdf/verify.rs`; `src/pdf/text.rs` `KNOWN_LIMITS`; `src/jobs.rs` `Placed`.
- CASE 1.5.0 and UCO 1.5.0 ontologies at tag 1.5.0, in local downloads outside the repository: `RasterPictureFacet` in `observable.ttl`; `wasDerivedFrom` in `investigation.ttl`; the identifier advice regex in `core.ttl`.
- Repo at `f5ddfa6`, revision 4: `src/pdf/diagnose.rs` (finding texts, ids, locations), `src/pdf/rebuild.rs` `outside_tree_note`, `src/ui/input/gate.rs`, `src/ui/app.rs` (`take_drop`, answer sources, `repair_started`), `src/engine.rs` `Answer::recorded_source`, `src/library.rs` `RunRecord`, `Cargo.toml`.
- Tools run: `jsonschema` 4.26.0, `case_validate` from case-utils 0.18.0, jq 1.8.2, `shasum`.

## Review log

### User decisions, revision 5

The user answered all open questions as recommended (decision log, commit `dcc0859`). Revision 5 applies them; section 15 lists each. Three needed design changes, because revision 4 conflicted with them:

| Decision | Revision 4 said | Revision 5 does | Where |
|---|---|---|---|
| Q16 (log Q29): custody mode refuses to start until `output_dir` is set | The prompt warned when outputs would go into the evidence folder, and `destination` recorded `mode`, `same_folder_as_input` and `warned_at_prompt` | Setup refuses to switch custody on without `output_dir`. The prompt refuses `Start batch` when it is unset, or when it is the folder of a pending input. `destination` is the folder alone. The `input_file` statement says custody writes only there. Check 23 | 1, 2.4, 3.2, 4.4, 6.5, 11.2, 12.1, 14 items 8, 37 |
| Q7 (log Q41): UTC offset filled on every platform | Null on Unix, with the zone name as a hint | Read at `started_utc` through `clock.rs`'s TZif reader on Unix and from one `FILETIME` on Windows. New `host_utc_offset_from`. The example shows UTC-4, and the `.txt` prints its source. `TZ` is dropped from the hint sources, since the app never reads it. Check 24 | 4.4, 5.5, 14 items 6, 36, App. A, B |
| Q9 (log Q43): `.verify-<n>.txt` every time | `.verify-<log_id8>-<seq>.txt`, for finding m15 | The user's form, with `<n>` a per-record counter claimed first-free with `create_new`, so it cannot collide, even across continued logs or two app instances. The `verify` entry gains `verify_n` and `text_sha256`, so a file pairs with its entry by hash | 8.3, 8.5, 9, 14 item 38 |

The rest confirmed the recommendations and are now worded as decisions: Q1, Q2, Q3, Q4, Q5, Q6, Q8, Q10, Q11, Q12, Q13, Q14, Q15, Q17, and UX Q27 (`require_case_details`, off by default, new in 4.4 and item 39) and Q30.

Two statements changed text, which changes every Part B hash:

- `input_file`: custody mode writes only to the output folder and does not start without one.
- `tool_identity`: releases publish each executable's SHA-256, per decision Q14.

The UX design (`ux-design.md`, now merged at `096984f`) needs these edits to match:

- Frame 13 rows 26 to 28 show a yellow destination warning, with `warned_at_prompt`. Under decision Q16 they show the refusal instead, and `Start batch` cannot be pressed.
- The UX names the verify file `.verify-<seq>.txt` in three places (lines 770, 827, 885). It becomes `.verify-<n>.txt`, with `<n>` the per-record counter of section 9, not a log entry number. This replaces revision 4's flag about the name.
- Frame 13's kitty state should say `path_kind = dropped`, as revision 4 flagged.

### Final review, revision 4

29 findings received: 6 major, 17 minor, 6 nits. All are addressed, each re-checked against the code at `f5ddfa6`.

| # | Severity | Finding | Disposition | Where |
|---|---|---|---|---|
| M1 | Major | Kitty drops and file promises carry bytes read at the drop, and `gate()` opens every file to sniff `%PDF-`, so "before the input was first read" and "the files are not read" were false for them. `path_kind` had no dropped value; section 14 omitted the gate change | Fixed. `path_kind` is `file`, `dropped`, `memory`, one per `JobInput` variant, plus `first_read` `job_start` or `drop`. New 2.3.1 says what each kind reads before the prompt and how the `.txt` Started line reads for it. `batch_cancelled` gains `received`. The `times` statement no longer claims a first read it cannot see. The custody-mode gate is item 23, and 12.1 marks the claim "not yet" until it ships | 2.3.1, 2.3.2, 2.4, 4.4, 8.3, 11.2, 12.1, 14, App. B |
| M2 | Major | `repair.rs` writes the Exact C9 action as "no other repair fits its window", an overclaim that lands in Part B; the example used invented text | Fixed. Item 24 gives the replacement strings for the action and for `diagnose.rs`'s finding summary, plus a trailer-edit variant. The example's findings and actions are now built from the engine's own format strings, quoted in `templates.py` with file and line, and use the corrected C9 text. Check 22 refuses the old phrase in any Part B | 14, App. A, B, `templates.py` |
| M3 | Major | `ChecksumMismatch` and `Prefix` had no grade, counter or image status; such an image got a plain name; B.6 counts did not add up | Fixed. `search.outcome` names the variant. `summary` gains `checksum_mismatch` and `prefix`, and the counters add up by definition. Image status `unverified`, name marker `.unverified.`, `prefix` maps to `partial`. Two this-run sentences and an image sentence. New fixture `salvage-outcomes.custody.json` has both cases; checks 13, 14 and 18 cover them. Item 25 | 4.3, 6.2, 6.3, 6.4, 12.3, 14 |
| M4 | Major | `InteractionSource` is already `User`, `Policy`, `UseBest`, `Batched`; mapping `UseBest` to `batched` would print "not shown to the user" for an answer the user gave | Fixed. `UseBest` maps to `user` with reply `use_best`. 7.2 now maps every source as the code sets it, with file references. Item 1 is marked done for the source, and keeps `carried_from` and `policy_setting` as the remaining work. `carried_from` is a tagged object, so "apply best to all" can name another file | 1, 4.5, 7.1, 7.2, 14 |
| M5 | Major | The engine also emits `FontNotEmbedded` and `PagesOutsideTree`; the closed enum made a common input produce an invalid record, and payloads were dropped | Fixed. `kind` covers all seven `FindingKind` variants, and `detail` keeps each payload: signed field count, outline counts, Type3 font, font and `base_font`, page count. Names are escaped per 4.1. The example has a `font_not_embedded` finding. Check 20. Item 26 | 4.3, 4.5, 14, App. A, B |
| M6 | Major | The keyless chain shows a changed middle entry only if nobody recomputed the later entries | Fixed. The `log` statement, 1, 8.4 and 8.5 say anyone who can write the file can rewrite it from the edit onward, and only a head copied outside the log or an unedited copy catches that | 1, 8.4, 8.5, 12.1 |
| m1 | Minor | B.7 F5 printed "asked because..." and `asked_utc` gave an ask time for a question nobody saw | Fixed. F5 prints "not asked: carried from #1". `asked_utc` is now `shown_utc`, null for `batched` and `policy`. Check 12 | 4.4, App. A, B |
| m2 | Minor | Template and example text differed, with no plural rule, though the text is hashed into Part B | Fixed. `templates.py` holds every fixed string; this document prints its tables from it and the example writes from it. A stated plural rule, `[n:one|many]`. Check 18 recomputes the this-run list from Part B | 12, `templates.py` |
| m3 | Minor | The fixture shared the example's `log_id` after record 42 for the same input, with empty `earlier_records` and a `created` folder | Fixed by giving the fixture its own case, workstation, log and work folder, so both values are true. Check 21 tests `earlier_records` across any records that share a log, and that a reused folder has reused placements | 7.4, fixture |
| m4 | Minor | A "read-only exhibit volume" showed a changed access time | Fixed. The input has mode 0444 on a writable volume. The `.txt` prints the mode | 6.5, App. A, B |
| m5 | Minor | "re-encoded losslessly" contradicted the no-"lossless" rule | Fixed. "re-encoded as PNG or TIFF with the same sample values", and 6.3 says `samples_sha256` lets a reader check it. Check 22 | 6.3, 12.2, App. B |
| m6 | Minor | `record.original_unchanged` and `SIGNED_NOTE` still said "original" | Fixed. The log field is `input_unchanged`. Item 27 gives the new `SIGNED_NOTE` and `SIGNED_SUMMARY` text. The view's own signed sentence names the field count | 8.3, 12.3, 14 |
| m7 | Minor | Renames unlisted: `v0`/`v1`/`v2`, the `FILE` sentinel, `Location::Page.index` from 0 | Fixed. A renames table in 4.5, item 35 | 4.3, 4.5, 14 |
| m8 | Minor | One interaction can cover several slots, so "every interaction referenced once" was wrong | Fixed. Many slots to one interaction; every interaction covers at least one slot; the slot's basis equals the interaction's source. Check 10 | 7.1, 7.3 |
| m9 | Minor | `run_without_record.started_utc` had no source when history did not keep the run; `Finished` had no record ref | Fixed. `started_utc` may be null and the `.txt` says the time was not kept. Item 29 adds both fields to `Finished` | 4.4, 7.4, 14 |
| m10 | Minor | Window count undefined for the deep phase, widened windows overlapped, zlib-then-raw had one mode field | Fixed with one record per pass. A pass starts at each `Pending::start`; the deep draw extends its pass. Counts are per pass, never summed. Check 13 tests per-pass arithmetic | 4.3, 12.3, App. B |
| m11 | Minor | Quitting with parked jobs left no `batch_closed`; whether a job cancelled after the prompt is hashed was unstated | Fixed. New `batch_unclosed` entry at next start, 2.3.3. Rule in 2.3.2: such a job keeps hashes only if it had read the input; `hashes_before` may be null | 2.3.2, 2.3.3, 2.4, 4.4, 8.3, 8.4 |
| m12 | Minor | Section 14 missed the new config keys and the `Win32_System_Time` feature | Fixed. Item 30 lists every key. Items 4 and 6 name `Win32_System_IO` and `Win32_System_Time` | 14 |
| m13 | Minor | ISO 27037 6.1 and 6.6 were cited for content never read | Fixed. Cited by number and title only; the rows mapping content to 6.1 and 6.6 are gone; Q16 no longer cites it | 11.1, 11.2, 15 |
| m14 | Minor | "Same build" was ambiguous and Part B had no platform field | Fixed. Same build means same version, commit, OS and architecture; `tool.platform` is in Part B; cross-platform identity is not claimed, with the reason. Item 34 | 3.1, 3.3, 4.3, 9, 12.1 |
| m15 | Minor | Records claimed with `create_new` and written in place; `verify-<seq>` names could collide across logs; `earlier_records` covered one log | Fixed. Temporary file, then a rename onto the claimed placeholder. Verify files carry the first 8 hex of `log_id`. `earlier_records` follows `log_continued` back, and `earlier_records_logs` says how far | 2.2, 4.4, 9, 14 |
| m16 | Minor | 12.3 missed `text.rs`'s `SCANNED_PAGES` and `HIDDEN_LAYERS`, and scanned order contradicts 4.1 | Fixed. Both are this-run sentences, quoted verbatim, from a new Part B `text_warnings`. The scanned-pages sentence says page numbers in the record still follow the rebuilt order. Item 33 | 4.1, 4.3, 12.3, 14 |
| m17 | Minor | A trailer-only repair is graded Exact and its decoded data has no checksum | Fixed. The trailer sentence, the search table and `trailer_edit` say no checksum covers the data, whatever the grade. Item 24 has a trailer action text | 4.3, 12.3, 14 |
| n1 | Nit | "fixes the log" | Now "pins" | 8.5, 12.1, App. B |
| n2 | Nit | "jq 1.7 or later" was untested | "Tested with jq 1.8.2" | 1, 3.3 |
| n3 | Nit | `settings_sha256` undefined | SHA-256 of compact Part B `settings`. Check 19 | 6.5 |
| n4 | Nit | Two logs created in one second could share a `log_id` | `log_created.nonce`, 16 random bytes. Check 17 | 8.2, 8.3, 14 |
| n5 | Nit | B.7 F3 listed one refusal reason of two | Every failing condition is listed: confidence 25/100 < 35/100 and hit 48/100 < 1/2 | App. B |
| n6 | Nit | The validation statement claimed a build commit when none is recorded | Two variants, chosen by `tool.build_commit`. Check 18 | 12.1 |

Changes beyond the findings:

- Re-reading `fonts.rs` showed fonts are substituted only on the template-assemble path. Revision 3's example chose resave and still rebuilt three fonts. The example now builds both candidates and chooses template assemble.
- Finding ids, severities and C7 locations now follow `diagnose.rs`: ids like `C9-001`, the Exact C9 finding is a warning, and C7 findings sit on the font descriptor.
- `findings_after` is a diagnosis of the output, so its objects are the output's; 4.1 says so, and B.4 prints which findings remain.
- Validator checks grew from 17 to 22 and now run over the example and both fixtures.

The UX design (`ux-design.md`) needs three small changes to match:

- Frame 13 kitty state says a kitty input's record has `path_kind = file_promise`. It is now `dropped`, with `first_read = drop`.
- Frame 14 and the verify rows name `.custody.verify-<seq>.txt`. It is now `.custody.verify-<log_id8>-<seq>.txt`.
- Its three "flags for the schema owner" are all adopted: 2.3.1 and 2.3.2, `stat_before` before any read by PDFPundit for path inputs, and `received`.

### Second review, revision 3

12 findings received: 5 major, 7 minor. All are addressed. The review text was cut off inside finding 12, after "GNU escaping for names containing a backslash or newline is not spec", so its fix is inferred from the issue. Any findings after it were not received.

| # | Severity | Finding | Disposition | Where |
|---|---|---|---|---|
| 1 | Major | The Exact sentence said "exactly one replacement passed". The code means the window was exhausted and one distinct output survived; two edits with the same output are still Exact. The coverage paragraph left out the +8 slack, the trailer rewrite, the widened search and the TTF localizer | Fixed. Exact now reads "every one that passed the check gave the same output", in 12.3, part C and the example. `survivors` became `distinct_outputs`, beside a new `passing_edits`. New fields `mode`, `damage`, `localizer` (`error_window`, `adler_only`, `ttf_tables`), `adler_rerun`, `trailer_edit`; `check` gains `adler32_after_ttf_table_checksum`. One `salvage_search` object serves both the stream entry and the image's `integrity.salvage`. New this-run sentences for trailer edits and widened searches. The coverage paragraph is now a table built from `ERROR_WINDOW`, `ERROR_SLACK`, `DEEP_ERROR_WINDOW`, `ADLER_FROM`, `trailer_survivor` and `ttf.rs`. `validate.py` check 13 tests the grade and window rules. Engine work item 19 | 4.3, 4.5, 6.2, 12.3, 14, App. A and B |
| 2 | Major | The standing sentence said image files keep the bytes the PDF holds. PNG and TIFF files are re-encoded, and a repaired image holds edited samples, under a plain name | Fixed. New standing sentence: stored bytes or decoded samples re-encoded losslessly; repaired images hold repaired samples, are named so, and the record gives the edits. 6.3's rule rewritten. Repaired, partial and truncated files are named `<hex16>.<status>.<ext>` in every mode. The as-found `.bin` sibling was not adopted: in custody mode only it would put a custody setting into Part B, Q17. `validate.py` check 14 | 1, 6.2, 6.3, 6.4, 6.5, 6.6, 12.2, App. A and B |
| 3 | Major | `answers_replayed_from` was null for an export whose earlier run had no custody record, which reads as answered in this run | Fixed. Now a tagged object, never null: `none`, `custody_record` or `run_without_record`, each replay variant with `seqs`. B.7 marks replayed rows and prints "that has no custody record; not given in this run". Fixture for the custody-off repair then custody-on export, `validate.py` check 16 | 4.4, 7.4, App. A, `example/fixtures/` |
| 4 | Major | One "item reference" per batch, printed on every record, misidentifies all but one item in a multi-file batch | Fixed. Renamed `submission_reference`, printed as "applies to all N files of this batch". The item's unique identifier is the input SHA-256. 11.2's 5.3 row says so. Q15 rewritten: a per-file field would be the item reference | 4.1, 4.4, 11.2, 12.1, 13.2, 14, 15, App. B |
| 5 | Major | The record called the input "Original", though the tool cannot tell an original from a copy, and the default writes nine files into the evidence folder without saying so | Fixed. `run.original` is now `run.input_file`, labelled "Input file (as received by PDFPundit)". The statement, now `input_file`, says PDFPundit cannot tell original from copy and that writing beside the input changes that folder. New `run.destination` with `mode` `beside_input` or `output_dir`. The prompt warns when the destination is the input's folder. The worked example now writes to a separate work folder. Q16 asks whether custody mode should require `output_dir` | 2.2, 2.4, 3.2, 4.4, 6.5, 11.2, 12.1, 13.2, 15, App. A and B |
| 6 | Minor | Logging hashes on a cancelled prompt claimed the prompt had already hashed the files; it had not, so cancelling would read declined evidence | Fixed with option (a). `batch_cancelled` holds a count only, and the doc says the files are not read. Frame 13 shows algorithm names, not hash values, and item 8 says so. `validate.py` check 17 rejects a hash list | 2.3, 2.4, 8.3, 11.2, 14 |
| 7 | Minor | `raw_span` assumed every image has a span in the file. Inline images in compressed content streams and Type3 bitmaps do not, and `bi_offset`'s domain was unstated | Fixed. `raw` is tagged by `domain`, `file` or `decoded_content_stream`, with the container stream and its file span. `bi_offset` is in the container's decoded bytes. The `dd` claim is limited to the `file` domain | 4.1, 6.2, App. B |
| 8 | Minor | The font evidence left out coverage, its 95/100 threshold and the ToUnicode state, so the rule was incomplete | Fixed. `evidence` gains `inference`, `tounicode`, `best_coverage`, `best_coverage_font`, `min_coverage` and `decision`. 7.3 restates `resolve` in five steps. `validate.py` check 11 recomputes every decision | 4.5, 7.3, App. A and B |
| 9 | Minor | Settings that change Part B were missing: `prompt_unresolved`, the derived-view option, and the effective font source | Fixed. All three are in Part B `settings`, with `font_policy` kept as configured. 3.1 says system-font support must hash the system fonts it used into Part B before it ships, item 21 | 3.1, 4.3, 4.5, 14, App. B |
| 10 | Minor | Start and end times were not tied to events, answers had no times, and the Windows offset could straddle a boundary | Fixed. Start is read just before the first stat, end just after the second, before the record is written; the `times` statement says so. New `answer_times`. Windows offset from one `FILETIME` and its local conversion. `validate.py` check 12 tests the order | 2.3, 4.4, 11.2, 12.1, 14, 15 Q7 |
| 11 | Minor | A deleted or swapped `custody.log` just started a fresh chain with nothing pointing back | Fixed. History keeps the head after every append. At start-up the app compares; a missing log or a mismatch starts a new log whose `log_continued` names the previous `log_id` and head, with `reason` and `known_from`, shown on screen. The `log` statement says what this adds and what it still cannot catch | 8.3, 8.4, 8.5, 12.1, 14 |
| 12 | Minor | The manifest listed the input by a name relative to the destination, which fails with `output_dir` or a file promise, and name escaping was unspecified | Fix inferred. The manifest never lists the input. Names are relative to the destination, `/`-separated, UTF-8, sorted by bytes, and GNU-escaped for backslash, line feed and carriage return, with a test in `example.py`. `validate.py` check 15 | 2.1, App. C |

Changes beyond the findings:

- The `.txt` grew from 6 pages to 7.
- First-review dispositions 2 and 15 are superseded where they conflict: the Exact wording, and `batch_cancelled` listing hashes.

### First review, revision 2

15 findings: 7 major, 8 minor. All are addressed. The review text was cut off partway through finding 15, after the word "Add", so its fix is inferred from the issue text. Any findings after it were not received.

| # | Severity | Finding | Disposition | Where |
|---|---|---|---|---|
| 1 | Major | The only anchor the examiner copies is the entry before the record. The last record of a log could be edited with its log line, and the chain would still verify. The `.txt` called that hash "Chain head" | Fixed. New `batch_closed` entry, whose hash the batch-end screen shows as the anchor. Verify prints "log head after this record". Every `.txt` page header carries the `.json` SHA-256. The `.txt` line is now "Log entry before this record". `log_id` in `run.log` and in record refs | 1, 2.3, 4.4, 8.2, 8.3, 8.5, 9, 13, App. B and C |
| 2 | Major | "Exact" claimed one output "exists" after a "complete search". The search covers single-byte replacements in a bounded window, checked by a 32-bit Adler-32 | Fixed. New wording in 12.3, the abbreviations and the example. `stream_salvage.streams` now records `check`, `searched.window`, `candidates_in_window` and `candidates_tried` | 4.3, 12.3, App. A and B |
| 3 | Major | A D-125 export replays answers, but its record shows them as given on screen by this run's examiner | Fixed. Part B unchanged. New `run.answers_replayed_from`. The `.txt` and Verify say "replayed here" and name the earlier record | 2.2, 4.4, 7.4, 9 |
| 4 | Major | Auto-accepted font slots had no attributable record; their basis went only to the log sink | Fixed with a `font_decisions` array, not with policy interactions. An auto-accept asks nothing, so calling it a policy answer would invent a question. Every slot gets a basis and, when scored, its confidence, hit, margin, threshold, min hit and sample floor. New this-run sentence. Engine work item 14 before T-33 | 1, 3.2, 4.3, 4.5, 7.2, 7.3, 12.3, 14 |
| 5 | Major | Tool identity cannot be checked: no published executable hash, a forgeable build commit, and nothing sets it | Fixed in the design. `build_origin` field, `tool_identity` statement, explanation in 4.5. Work item 13 changes the release workflow. Q14 asks whether to hold the release until it lands | 4.3, 4.5, 5.1, 12.1, 14, 15 |
| 6 | Major | The `validation` statement pointed at release notes that publish no test results | Reworded to what is true: test suite in the repository at the build commit; corpus named there, not contained. Publishing a summary is optional item 18 | 12.1, 14 |
| 7 | Major | The one log per machine held case details and file names in plain text, so proving one chain disclosed other matters | Fixed. Log lines hold hashes, counts, times, kinds and outcomes only. The schema gives each kind a closed shape, so a stray field fails validation. 8.6 says how to disclose a segment. Residue: an input SHA-256 lets a holder of a known file confirm it was processed. Q12 offers a per-case log | 2.1, 2.3, 8.1, 8.3, 8.6, 15 |
| 8 | Minor | jq and serde_json disagree on U+007F and other non-printable characters, so the jq check could fail on real input | Fixed. Part B strings are printable ASCII: `#xx` for PDF names, including delimiters, and `\xHH` elsewhere. The schema enforces it. `work/ascii_fixture.py` shows raw bytes give different hashes and escaped bytes match. `validate.py` check 9 runs jq against Python | 3.3, 4.1, 14 item 16 |
| 9 | Minor | Edit offsets were described as from the stream data, but the code uses the Flate stage's input | Fixed. `edit_domain` on every salvage stream and image record. The `.txt` says which domain, and gives the file offset when the domain is the raw stream data | 4.1, 4.3, 6.2, App. B |
| 10 | Minor | FRE 902(14) was mapped to two hashes from the same buffer, called "original and copy" | Fixed. 902(14) maps to the original's hashes for comparison with the acquisition hash. The NIST row says there is no copy, only derivatives | 11.2 |
| 11 | Minor | Q7 recommends a null UTC offset on Unix, but the macOS example showed -300 | Fixed. The example shows null, printed "not recorded", with the zone name from the `/etc/localtime` link as an unverified hint, new field `host_zone_hint` | 4.4, 15 Q7, App. A and B |
| 12 | Minor | "As a viewer shows them" was ambiguous for page numbers | Fixed. Physical position in the rebuilt order; `/PageLabels` never used; `page_in` null for D-112 appended pages | 4.1, 6.2 |
| 13 | Minor | "Kept vs input" could read as nothing lost | Fixed. The `.txt` says the counts compare with what was readable in the damaged input, and prints the baseline | App. B, 13.2 |
| 14 | Minor | The record did not capture the original's file-system state before and after | Fixed. `stat_before`, `stat_after`, `stat_changed` as raw integers; the `.txt` names any difference. The `original` statement now says reading can update the access time | 2.3, 4.1, 4.4, 11, 12.1, 14 item 15, App. B |
| 15 | Minor | Case details are unverified but no statement says so; "not given" against "not provided"; cancelling writes nothing; no item field | Fix inferred, because the text was cut off. New `case_details` statement and a "not verified" line in the `.txt`. "not provided" kept, and Q13 explains why the prompt must not quote it. Cancelling the prompt appends a `batch_cancelled` entry with each file's hash and size. New `item_reference` field, Q2 and Q15 | 2.3, 2.4, 4.4, 11.2, 12.1, 13.1, 15 |

Changes beyond the findings:

- Record log entries tag file hashes by role instead of file name. A record's file name is built from the input's name, so a name would have leaked it.
- The `verify` log entry carries `head_after_record`.
- The worked example gained a third font slot, the auto-accepted one, so findings, actions and the this-run sentences changed. The `.txt` grew from 5 pages to 6.
