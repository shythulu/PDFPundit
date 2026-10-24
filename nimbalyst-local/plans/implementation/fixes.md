# Fix tickets from the architecture audit (2026-10-08)

The audit of the implementation session found wiring gaps and defects that no
ticket owned. Each ticket below is one agent session, built test-first, code
reviewed, and merged through the same gates as the plan's tickets. Evidence for
every item is in the audit (kept outside the repo) and is restated here.

Rules from `plan.md` §9 apply to every ticket: clean-room, contract check,
determinism. None of these tickets may change an artefact's bytes except where
the ticket says so.

**F-01 Answer font questions from the UI** · fixes D-126 (blocker) and D-131 · determined (GG §1, TD:427-429, TD:469-471, TD:602, mockup frames 03 and 04)

- Problem: a job that asks a font question parks forever. `ui/app.rs:976-979` sets `WaitingOnUser` and drops the reply sender. `Screen::FontPick` is never built, `FontPickModal` is drawn only in tests, `JobRunner::reply` has no production caller, and `jobs.rs:13` still says `TODO(T-24)`.
- Build:
  - Keep each parked job's request in app state. The full layout's NEEDS iNPUT panel offers `[i] resolve now` and `[l] later` (frame 03). `i` opens `Screen::FontPick` on the first parked job; the modal's "n of m" counts parked questions.
  - Modal keys as built in `modals.rs` (`↑↓`, Enter = pick, `b` = use best for both, `s` = skip, `esc` = later, the job stays parked). Each answer calls `JobRunner::reply`, so the job resumes and writes its output.
  - "Apply best to all" (TD:469-471): one key in the modal that answers every parked font question with `UseBest`.
  - `[fonts] prompt_unresolved = false` (TD:602) answers font questions with `UseBest` without asking; `true` (default) asks.
  - `[fonts] source = "system"`: system fonts are not built (M5), so warn once at start-up, in the config warnings, that it behaves as `bundled`.
  - Batch font prompts per family: strip the subset tag and the style suffix (`-Regular`, `-Bold`, `-Italic`, `-BoldItalic` and the usual spellings) so one family asks once.
  - The widget layout never shows prompts (D5). It keeps the ‼ and "zoom me".
  - Remove the `TODO(T-24)` allow and the `state.rs` dead-code allow if nothing else needs them.
- Acceptance: an app test with `FakeEngine` drops a file whose job asks, opens the modal with `i`, picks a font, and sees the runner reply and a finished row; `esc` leaves it parked; "apply best to all" answers two parked jobs; `prompt_unresolved = false` never opens the modal; the per-family batching test (`NotoSans-Regular` + `NotoSans-Bold` ask once); the config warning test for `system`.

**F-02 Markdown export cannot be made to embed or fetch anything** · fixes the T-32b `!` defect and D-136 · determined (DA:111-112, plan T-32b)

- Problem: `markdown.rs` does not escape `!`, so text ending in `!` before a linked run becomes an image that fetches a remote URL. Raw `<` in PDF text becomes inline HTML in GFM viewers. Image directory names include the file stem, so the same bytes under another name give a different `.md`.
- Build: escape every character that can start Markdown or HTML syntax in text runs: `\`, `` ` ``, `*`, `_`, `[`, `]`, `<`, `>`, `!`, `#` and `+`/`-`/digits+`.` at line start, `|` inside table cells, `&` before an entity. Name the images directory `<hash8>.images` (hash of the input bytes only).
- Acceptance: a text run `Look!` followed by a link run renders no `![`; text `<img src=x>` renders as literal text; a fuzz test over random runs asserts the output contains no `![`, no raw `<` and no autolink except those the writer created from `/Annots`; renaming the input does not change the `.md` bytes; the existing export goldens are regenerated only for the escaping change, with the diff explained in the commit.

**F-03 Untrusted text never reaches the terminal raw** · fixes D-135, part of D-118 · defaulted-reversible

- Problem: `ui/layout/widget.rs:175-177, 271` formats file names into `rich` colour markup, so a name with `{R}` mis-draws. Control characters bypass `Canvas::text`, and `term.rs` `blit` does not filter.
- Build: every string from a file name, path, PDF, font name or finding is drawn with `Canvas::text`, never through markup. `Canvas::text` replaces C0, C1 and DEL, and also the bidi controls U+200E, U+200F, U+202A–U+202E and U+2066–U+2069. The blit replaces any remaining control character as a second guard.
- Acceptance: tests with names containing `{R}`, ESC, a CSI sequence, BEL and RLO draw literally (or as the replacement glyph) in the full layout, the widget and the picker; a blit test proves no byte below 0x20 other than the escape sequences the blit itself writes reaches the output.

**F-04 Salvage never keeps bytes a raw-deflate guess invented** · fixes D-128 · defaulted-reversible (flagged to the user)

- Problem: `raw_fallback` accepts a `Failed` raw decode with any output (`salvage.rs:1001-1004`), and `conclude` returns that as a `Prefix` (`salvage.rs:1108-1113`). On noise, 13 of 59 streams kept 1–8 invented bytes.
- Build: a raw-deflate fallback counts only when it ends `Done`, or when it ends within `RAW_END_SLACK` of the stream end, the rule T-06 already uses. Otherwise the stream is `Unrecoverable` and is written raw (D-074). Fix the doc comment that says the opposite.
- Acceptance: the 59-stream noise test yields no `Prefix` from a raw guess; the existing salvage tests keep their grades; the C9 smoke numbers do not drop (run `bench::corpus::smoke`, report the per-class result in the commit).

**F-05 The history store carries a format version** · fixes D-129 · defaulted-reversible until a release

- Build: `index.json`, each file record and each run record carry `"version": 1`. A record with no version reads as version 1. A record with a higher version is left untouched and skipped with a warning; it is never moved aside. A record that fails to parse is moved aside to `<name>.corrupt` with a `(N)` suffix, never overwriting an earlier one.
- Acceptance: round-trip tests; a future-version record survives a load and a save untouched; two corrupt records give `.corrupt` and `.corrupt (2)`.

**F-06 Shell fixes** · fixes D-132, D-134, D-139 and three review notes · defaulted-reversible

- Panic restore runs once: `TermGuard`'s drop does nothing if the panic hook already restored the terminal (T-23a review major).
- Centre the layout in a terminal larger than its minimum size (the deviations index says the blit centres; nothing does).
- One-line fallback: when a drop is refused, draw the refusal hint in place of the counts until the next key or resize (D-134).
- Temp files: at start-up and before each placement, remove `.pdfpundit-<pid>-<n>.tmp` files whose pid is not running, in the destination folder only (D-139).
- Debug log: write the in-memory log to `<cache dir>/debug.log`, rotated by size with no-clobber, and show the D-125 images-directory clash on the status line (D-132).
- Remove stale `allow(dead_code)` lines and TODOs in `config.rs`, `appdirs.rs`, `place.rs` and `ui/state.rs`; delete code that is truly unused (`place.rs` `write_new`, `place_no_replace`, `TempFile::path` if still unused).
- Acceptance: a test per item.

**F-07 C9 damage outside Flate data is detected** · fixes D-130 · defaulted-reversible

- Problem: no production code calls `with_near_miss(true)`, so the C9-outside-stream rule never fires.
- Build: after the main carve, run a near-miss lex only over gaps and unexplained spans, never over carved objects, and feed its `NearMissKeyword` notes to diagnose. The main carve does not change.
- Acceptance: a C9 fixture with a keyword hit outside a stream gives the Warning; every existing carve, diagnose, contract and golden test is unchanged; the C9 corruptor's outside-stream seeds now report it.

**F-08 Repairs to references are reported, and V0's Partial excuse is class-qualified** · fixes D-133 and D-137 · defaulted-reversible

- Unmatched and position-matched references (`rebuild.rs:105, 146`) become report actions, so the report says which references were nulled or re-linked and how. No output byte changes.
- `verify`'s `partial` becomes `&[(CorruptionClass, Location)]`; a Partial excuses only a finding of its own class at its own location.
- Acceptance: a fixture with a dangling reference lists it in the report; a test where a C9 Partial no longer excuses a C6 finding at the same page.

**F-09 Findings appear within seconds on C9 files** · fixes D-127 (DA:517) · defaulted-reversible; no artefact changes

- Problem: `pipeline.rs:93-112` runs the whole salvage search before diagnose, so a C9 file shows nothing for 32–243 s.
- Build: analyze emits the structural findings (C1–C8, C10, info kinds) and a provisional C9 finding per damaged stream from the cheap probe first, then runs salvage and replaces the provisional C9 findings with the graded ones. The `Progress` events and the queue rows show the provisional state. The final `AnalysisResult` and every artefact are byte-identical to today.
- Acceptance: a test that the first finding event on a C9 fixture arrives before any salvage work is charged; the final analysis equals today's (contract snapshots unchanged).

**F-10 More dependency gates in CI** · fixes D-138 (a) · defaulted-reversible

- Add `cargo deny check advisories sources bans` to CI, with `bans` allowing the known duplicates (`miniz_oxide` 0.8/0.9, `skrifa`, `read-fonts`). Do not change the licence allow-list (that is D-046, the user's). Run all three locally and record the result in the commit.

## Release ticket (unblocked 2026-10-08)

**T-35 Release packaging (cargo-dist), usage docs, notices completeness** · unblocked by D-046 (MIT) and D-065 (ship as designed) · defaulted-reversible where noted; signing and notarisation are the user's (D-142)

- Packaging with cargo-dist, pinned: `cargo-dist-version = "0.32.0"` in `[workspace.metadata.dist]` (eng-r4-fr3 measured this version). Install the tool for this session with `cargo install cargo-dist --version 0.32.0 --locked` (dev tool, outside the build's purity rule). Only the `pdfpundit` package is distributed; `tools/build-templates` is not.
- Targets: the three CI targets, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`. Default: add `x86_64-apple-darwin` only if dist builds it on the macOS runner without extra setup; otherwise leave it out and say so.
- Artefacts (defaulted): plain archives only (`.tar.xz` on Unix, `.zip` on Windows), each with `pdfpundit[.exe]`, `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.md` and `THIRD_PARTY_CRATES.md`, plus a SHA-256 checksum per archive and a combined `sha256.sum`. No shell/PowerShell/Homebrew/MSI installers in this ticket (an installer fetches over the network and is a separate choice; log it).
- `.github/workflows/release.yml` from `dist generate`, triggered only by pushing a `v*` tag, publishing a GitHub Release. Add a job step that runs the shipped binary's guard checks on each built archive: unpack, run the binary with stdin and stdout redirected, assert exit code 2 and the fixed refusal line; and the release-strings check (no `PDFPUNDIT_ASSETS`, no `PDFPUNDIT_PANIC` in the binary). The workflow must not set `RUSTFLAGS` or any cfg (AGENTS.md "Build"). `publish = false` stays; nothing goes to crates.io.
- `THIRD_PARTY_CRATES.md`: generated by a new `tools/crate-notices.py` (Python stdlib, `python3 -I`) from `cargo metadata --locked` over the shipped target set: one section per crate in the shipped graph (normal dependencies only, all three targets) with name, version, SPDX licence and the crate's own licence/notice file text from the registry source (`LICENSE*`, `COPYING*`, `NOTICE*`); where a crate ships no licence file, say so and give the SPDX expression. Deterministic output (sorted). CI regenerates it and fails on a diff, like the `assets` job.
- `THIRD_PARTY_NOTICES.md` completeness: every data file compiled into the binary is covered (Foxit fonts, Adobe CMaps, ICC profiles, AGL, pdf.js names, DarkBerry palette, Noto). The audit found `assets/licenses/` lacks the AGL BSD-3 and Foxit texts as separate files; the notices file carries them inline, which is enough for the archive. Add a test that every file under `assets/` and every `include_bytes!` target is named in the notices file.
- `README.md` usage section: what PDFPundit is (keep the existing intro), install from a release archive, run it in a terminal (`pdfpundit`; it refuses to run without one), drop or paste PDFs on the cat, `b` browse, `e` export Markdown, `T` themes, font questions (`i`, the modal keys), where outputs go (`<name>.repaired.pdf` beside the input or `[general] output_dir`, never replacing a file), config file location per platform, the widget layout below 112×38, kitty drag tracking vs the chomp elsewhere, offline and deterministic guarantees, limits (no OCR, no images in Markdown, no word lists yet). Plain, short, scannable.
- Acceptance: `dist plan` lists exactly the chosen targets and artefacts; `dist build --artifacts=local --target aarch64-apple-darwin` produces the archive and checksum on this Mac; the unpacked binary passes the guard check and the release-strings check; `tools/crate-notices.py` output is committed, deterministic (two runs identical) and complete (every crate in the shipped graph has a section); the notices-coverage test passes; all existing gates pass. Windows and Linux archives are built only by the release workflow on hosted runners (not run in this session; say so).
- Log for the user, do not decide: macOS code signing and notarisation (needs an Apple Developer account, about USD 99/year; unsigned archives trip Gatekeeper on first run), Windows Authenticode signing (needs a certificate), installers and a Homebrew tap, and the first release tag (`v0.1.0`). Record these as D-142 in the decision log with options and recommendations.

## Follow-up tickets from the user's answers (2026-10-08)

The user answered Tiers 2–4 of the decision log on 2026-10-08 (see the "Answered by the user" blocks in `decision-log.md`). These tickets build the answers that need code. Same rules as above.

**G-01 A never-embedded font is a warning, not damage** · D-084 (b)

- Add `FindingKind::FontNotEmbedded { font: ObjId, base_font: String }` (Info severity, `Repairability::NotApplicable`). `diagnose` emits it, instead of C7/C8, for a font whose descriptor has no `/FontFile*` key at all; a descriptor whose font program was present and is now blank or dangling stays C7/C8 (that is damage). Standard-14 names keep emitting nothing.
- Acceptance: a Word-style `/TrueType /Arial /WinAnsiEncoding` with no `/FontFile2` gives `FontNotEmbedded` and no C7/C8 and no font question; the C7/C8 corruptor fixtures still give C7/C8; contract snapshots updated with the new variant only; the view model shows it as an info row.

**G-02 "Use best" on a weak guess recovers text without substituting a font** · D-122 (b)

- When the reply is `UseBest` (from a user, `a`, or `prompt_unresolved = false`) and the top candidate's dictionary hit rate is below 1/2, resolve to `TextOnly`: rebuild `/ToUnicode` from the inference so the text is recoverable, keep the original font slot, substitute no font, and mark the finding Partial with the reason "text recovered; font not confirmed". At or above 1/2, substitute as today.
- Acceptance: a C8 fixture with a weak top hit and `UseBest` produces no substituted font program and a `/ToUnicode` that extracts the expected text; a strong hit still substitutes; the report line states which happened.

**G-03 Answers the app gives itself are recorded as automatic** · D-141 (a)

- Carry an answer source through the reply path (`JobRunner::reply(job, reply, source)` or a wrapper type). Record `InteractionSource::Batched` for answers carried within a font family or given by `a` (apply best to all), and `InteractionSource::Policy` for `[fonts] prompt_unresolved = false`; `User` only for an answer the user gave to that question. Update the report and run record types and the contract snapshots.
- Acceptance: one test per source; the interaction record of a batch with a family-carried answer shows `Batched`.

**G-04 Pages outside the page tree are reported** · D-112 (c)

- Keep appending pages found only outside the page tree, and add a report action and an info finding "n pages not reachable from the page tree were appended" listing their object ids.
- Acceptance: an incremental-update fixture where a page was removed from `/Kids` gets it back and the report says so; an intact file reports nothing.

**G-05 Evicted jobs re-run one at a time** · D-103 (b)

- When parked jobs whose analysis state was evicted get their answers, re-run them through a single resume slot, in answer order, so memory stays bounded.
- Acceptance: with three evicted jobs answered at once, at most one re-analysis runs at a time and all three finish.

**G-06 Fix V1's glyph baseline** · D-088 (a)

- First run V1 over the corpus C5 files (`PDFPUNDIT_CORPUS`, ~/corpora/repdf) and record whether the hayro-fallback double count happens on real files. Then fix the baseline so a perfect repair scores retention 1 (count glyphs once per drawn run regardless of fallback font), and remove the C5 seed exclusions from the repair tests if the fix makes them pass.
- Acceptance: the C5 seeds 2–4 pass V1 on a perfect output; a corpus C5 sample is reported in the commit message; no other selection changes on the smoke subset (re-run `bench::corpus::smoke`).

**G-07 Cell widths for wide and zero-width characters** · D-118 (a)

- Add `unicode-width` (MIT/Apache, pure Rust) and make `Canvas` place a wide character across two cells and drop zero-width ones (after F-03's control replacement), with truncation that never splits a wide character.
- Acceptance: CJK and emoji file names align in the full layout, the widget and the picker; existing goldens unchanged.

**G-08 T-35 review notes**

- Fix the `Cargo.toml` comment that names `release.yml` (the file is `v-release.yml`). Keep dist's static CRT on Windows and record that the MSVC artefact gets `+crt-static` (so it differs from CI's release build there). Build the release-guard matrix from the dist plan input (or fail when the plan's targets differ from the guard's), so a new target cannot ship unguarded. Reword the crate-notices header to "the normal dependency graph of pdfpundit (including compile-time proc-macros)".
- Acceptance: the guard workflow fails a synthetic plan with an extra target; comments accurate.

**G-09 Word lists for C8 inference** · D-011

- Fetch the Leipzig Corpora Collection news lists for English, French and Spanish (most recent year available, 1M-sentence size), take the word-frequency file, keep the 50,000 most frequent words (lower-case, NFC), and commit them as `assets/dicts/{en,fr,es}.txt` with a SHA256SUMS entry and a `fetch-assets.sh` step that re-downloads and verifies. Confirm on the download page that these downloads are CC BY 4.0 and add the attribution text to `THIRD_PARTY_NOTICES.md` (and the tests that check notices coverage). Wire them into the font inference (replace the hard-coded empty `dicts`).
- Acceptance: C8 inference on the golden fixture auto-accepts with the dictionaries; re-run `bench::corpus::smoke` and report the per-class change (C8 especially) in the commit; update `bench/golden_smoke.csv` only if scores improve, explaining it.

**G-10 Gmaps for the corpus's other open-licensed faces** · D-010 (b), after G-09

- List the 44 distinct `/BaseFont` names in the REPDF originals (strip subset tags). For each family under the SIL OFL (or Apache-2.0) available from the google/fonts repository, fetch the Regular TTF at a pinned commit, generate its `.gmap` and index entry with `tools/build-templates`, and commit the gmaps and index (not the TTFs: output substitution stays Noto, D-010). Add each family's licence notice. Skip anything not OFL/Apache (e.g. Cambria) and list what was skipped.
- Acceptance: the font index covers the new families; inference on a corpus C8 sample matches more fonts by name; re-run the smoke subset and report the change.

**G-11 Per-script Tesseract scoring leg** · D-014 (Tesseract replaces Document AI)

- Implement `tools/ocr-recall/engines/tesseract.py` with per-script `tessdata_best` models (eng, fra, spa, ara, hin, chi_sim), pinned by hash, routed by the page's language label like the Paddle engine. Update the nightly workflow to install Tesseract on the runner. Do not install anything system-wide on this Mac; if Tesseract is not installed locally, test with the existing pytest fakes and say the real run is the nightly job's first run.
- Acceptance: the engine's pytest cases pass; `--engine tesseract --dry-run` works; the D-063 header stays on the output.

## CI ticket (first hosted run, 2026-10-09)

**CI-01 Make the first CI run green on all three OSes** · D-058 (the first windows-latest run is the measurement) · defaulted-reversible

- Problem: run 37907209463 (the branch's first push) failed in two ways. Logs: `/tmp/pdfpundit-run/ci-failed.clean.log` (all failed jobs) and `/tmp/pdfpundit-run/ci-win-test.clean.log` (the full Windows test job).
  1. `lint` on all three OSes: clippy `needless_borrows_for_generic_args` at `src/pdf/repair.rs:1627` and `:1642`. The runners use a newer stable Rust than the authoring Mac's 1.98.1, so local clippy did not see it.
  2. `test (windows-latest)`: 11 failures. Contract snapshots (`src/engine/contract_tests.rs:389`), the source-scanning tripwires (`pdf::verify::tests::nothing_outside_the_render_gate_touches_pixels`, `pdf::write::tests::rules_3_and_5_no_compression_no_streams_no_resave`), three `ui::app` tests (kitty drop, drop with nothing to take, `e` on a pasted pdf), and `ui::input::paste::tests::a_unc_path_is_refused_on_windows`. Read each panic in the log before changing anything.
- Build:
  - Fix the two clippy findings.
  - Pin the toolchain so local and CI lint and build with the same Rust: add `rust-toolchain.toml` (`channel = "1.98.1"`, components rustfmt and clippy), and make CI install that version (`dtolnay/rust-toolchain` or `rustup show` honouring the file). Record this as a new decision-log entry (defaulted-reversible): reproducible builds and stable lint results.
  - Line endings: add `.gitattributes` rules so text files that tests read byte-for-byte (`*.rs`, the contract and golden JSON under `tests/data/`, `*.md` read by tests, `*.csv`, `*.txt` manifests) check out with LF on every OS (`text eol=lf`), keeping existing `-text` rules for binary assets. Where a test reads its own source or a data file, make it robust to CRLF as well (normalise `\r\n` before comparing), so a user checkout with `core.autocrlf` still passes.
  - The ui::app and paste failures: find the real cause from the panic text (paths with `\`, drive letters, UNC forms, temp-dir layout on Windows) and fix the code or the test, whichever is wrong. Do not weaken a guard to make a test pass; if a guard's behaviour on Windows is wrong, fix the guard and say so.
  - Windows cannot be run locally. Verify what you can on the Mac (`cargo test`, `cargo clippy` with the pinned toolchain, and `cargo build --target x86_64-pc-windows-gnu` / `cargo clippy --target x86_64-pc-windows-gnu` if the target is installed), and write in your report exactly which Windows failures you expect fixed and why. The hosted re-run is the real check.
- Acceptance: local gates green on the pinned toolchain; the two clippy sites fixed; every Windows failure has a stated cause and fix; tests that read files are CRLF-robust (a unit test feeds CRLF content to each helper).

**C8-01 The C8 detector finds REPDF's C8 damage** · priority defect found by the G-09/G-10 corpus runs

- Problem: the four C8 files in the 44-file smoke subset diagnose `nothing_to_repair`, so the C8 pass never runs and C8 scores 0.260 (text-layer LCS-F1) regardless of the word lists (G-09) and the extra font maps (G-10). The detector misses the damage REPDF's C8 class induces on real files.
- Investigate first, with the corpus (`PDFPUNDIT_CORPUS=~/corpora/repdf`; never copy corpus files into the repo): for each C8 file in `bench/smoke_subset.txt` and a sample of the 100 corpus C8 files, compare against its original (same base name under `original/`) and state exactly what bytes REPDF changed (the measured induction table in plan.md T-03a / the decision log says C7/C8 overwrite in place with 0x20). Then trace why `diagnose` does not report C8: check in particular whether the in-place overwrite removes or garbles the `/FontFile*` key or the `/ToUnicode` key so that G-01's never-embedded rule (`FindingKind::FontNotEmbedded`) or another rule now claims the font, and whether the C8 rule's conditions match what REPDF actually leaves behind.
- Fix the detector so REPDF C8 files report C8 (and C7 files still report C7), without making a genuinely never-embedded font look damaged: distinguish "key absent in an intact dictionary" from "key region overwritten" using the evidence the carve keeps (blanked spans, dangling references, a descriptor whose byte span contains a run of 0x20 where a key/value was). Update the T-03a C7/C8 corruptors if they do not reproduce the measured damage.
- Acceptance: a fixture that reproduces the measured REPDF C8 damage diagnoses C8; the G-01 never-embedded fixture still gives `FontNotEmbedded` only; the four smoke C8 files diagnose C8 (a corpus test, ignored like the others, asserts it); re-run `bench::corpus::smoke` and report the per-class result, and update `bench/golden_smoke.csv` (same header line) only if C8 or another class improves and nothing drops, explaining it in the commit.
