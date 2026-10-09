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
