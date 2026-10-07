---
planStatus:
  planId: plan-pdfpundit-ui-design
  title: PDFPundit — TUI / UI Design
  status: draft
  planType: system-design
  priority: low
  owner: shythulu
  stakeholders: []
  tags:
    - rust
    - tui
    - design
    - ui
  created: "2026-07-17"
  updated: "2026-09-26T13:15:00.000Z"
  progress: 0
---
# PDFPundit — TUI / UI Design

## Context

The terminal-UI framework, layout, and aesthetic are being **decided
separately** from the engine so the core application design
([pdfpundit-technical-design.md](pdfpundit-technical-design.md)) can stay focused
on the forensic PDF work. As of 2026-09-26, D2 (layout), D3 (aesthetic) and D5
(widget mode) are decided, and D1 (framework) is still open. The engine is deliberately
UI-agnostic: it runs analyze/repair/export jobs on background threads and emits
a stream of typed events (`JobEvent`: progress, findings, needs-interaction,
done, failed); *any* UI layer consumes those events. So the choices below can be
made — and changed — without touching the engine.

> This plan absorbs the UI/aesthetic ideas that were floated during engine
> design (cat-first layout, pastel palette, "furensic" copy, Bubble Tea). Each
> decision below says whether it is decided or still open.

## Guiding requirement

**A ready-made library of styles/components to pick from.** The priority is a
framework where common pieces (lists, viewports, progress bars, spinners, file
pickers, styled/bordered boxes, key-hint bars) come prebuilt and are easy to
theme — minimize hand-building widgets. Secondary: pure-Rust, single
self-contained binary, cross-platform (macOS/Windows/Linux), mouse support.

## Decisions

### D1 — UI framework / styling stack

Candidates to investigate (verify current version, maintenance, license,
widget/style coverage, and how each reaches the "library of styles" goal):

| Candidate | What it is | Notes to check |
| --- | --- | --- |
| **ratatui** (+ `tui-widgets`, `tui-big-text`, etc.) | The dominant Rust TUI; immediate-mode, huge widget/community ecosystem | Most mature + best perf; styling is manual (`Style`) — does the ecosystem give enough ready-made styled components, or is it too low-level for the "pick a style" goal? |
| **bubbletea-rs** | Rust reimplementation of Charm's Bubble Tea (Elm architecture) | Young (v0.0.9), single-maintainer, async-first (pulls tokio). Closest to the Charm feel. |
| **bubbletea-widgets** | Prebuilt components for bubbletea-rs (list, viewport, progress, spinner, filepicker, help, table…) | This is the "library of components" for the bubbletea-rs path — assess coverage + maturity. |
| **lipgloss** (lipgloss-rs) | Rust port of Charm's lipgloss: declarative styles, borders, layout, color | This is the "library of styles" candidate — CSS-like style definitions. Usable standalone or with bubbletea-rs. |
| **charmed-bubbles** | (user-suggested) — investigate what it is / a Rust bubbles port? | Confirm it exists, scope, maintenance, license. |

Key question: **ratatui (mature, manual styling) vs. the Charm/lipgloss stack
(prebuilt styles + components, younger).** The "library of styles" requirement
leans toward lipgloss-style declarative styling — evaluate whether that's
lipgloss-rs on ratatui, the full bubbletea-rs stack, or a ratatui styling helper
crate. Consequence to weigh: bubbletea-rs is async-first (introduces a UI-side
runtime); the engine stays sync regardless.

### D2 — Layout — decided: cat-first, with the cat face as the drop target

**Decided on 2026-09-26** (see the `pdfpundit-ansi-bbs.mockup.html` mockup and its
README). The window is fixed at 112×38. A large white cat face in the middle is
the main way in: you drop PDFs on it. Last callers and the menu sit on the left;
how-it-works and system info sit on the right. The panels below are how a batch
is shown once files are dropped. Terminals smaller than 112×38 get the widget
layout (D5).

**Drop reaction, decided on 2026-10-07.** There are two, because most terminals
can't report a drag:

- **The chomp, everywhere.** Most terminals send the app nothing while a file is
  dragged over the window; the paths arrive as a paste on drop, with no position.
  So on drop the files land on the cat's head and it eats them: plop, looks up,
  jaw drops, chomp, nom, nom nom, gulp, burp. Mockup frame 2b (8 frames).
- **Tracking, in kitty.** kitty 0.47+ reports drags through its drag-and-drop
  protocol (`OSC 72`): the cell under the drag, then leave or drop. There the cat
  watches the file come in and opens wide (frame 2), then carries on from the
  chomp's "chomp" step on release. It only knows MIME types until the drop, so
  the file name label has to wait for the drop. crossterm has no event for
  `OSC 72`, so the app parses it itself. It sends the opt-in at start, so other
  terminals get tracking once they ship the protocol; terminals that don't know
  the escape should ignore it (check that in each target terminal).

Terminal support as of 2026-10-07: only kitty ships it. Ghostty has drop-only
support in libghostty-vt, not yet in the app; WezTerm (#7908) and foot (#2407)
have open requests; Alacritty's maintainer declined it (#8959); Windows
Terminal, iTerm2, Konsole, GNOME Terminal, Rio and Contour have no request. The
`ratatui-dnd` crate is unrelated: it reorders items dragged inside the TUI, not
files dragged in from the OS.

Original notes (the panel ideas still apply):

- **Empty state:** clean window, an ASCII-art backdrop on full display, one dim
  footer hint ("drop a PDF here · browse · quit").
- **Queue panel (floating, appears on file add):** the batch queue; grows
  downward as files are added; per-row status icon (not-started / in-progress /
  complete / error) + filename + terse result note.
- **Analysis panel (beneath queue, hideable):** selected file's metadata,
  C1–C10 findings grouped by severity (expandable), font resolutions, repair
  outcomes.
- **Bottom progress bar (while processing):** current-file gauge + name; a
  second line with batch-total % when more than one file is queued.
- **Context menus (modals):** per-file actions, repair-pass checklist, and the
  font-decision prompts (low-confidence picks + no-system-equivalent
  substitution, individual + select-all).

Data the UI needs from the engine (already defined engine-side): `QueueEntry`
(path, status, meta, findings, font resolutions), `BatchState` (current /
completed / total + current progress), `FontPickRequest` / substitution rows,
and the `JobEvent` stream.

### D3 — Aesthetic / theme — decided: full-colour ANSI BBS, DarkBerry

**Decided on 2026-09-26.**
- "Retro" means full-colour ANSI 0day BBS art: block-pixel logo, gradient
  double-line borders, lightbar selection and a BBS status line. It does not
  mean an amber or green CRT; that version was rejected.
- Truecolor with smooth OKLab gradients.
- Themes can be selected in a chooser (`T`). The default is DarkBerry Blackwater,
  from <https://darkberry.slacklab.ca/>. The Mire, Fen and Wisp flavours, ACiD
  Classic, Pastel Parlour and Mono Ink are also in the chooser.
- The "furensic" pun is used in the tagline.

Original options:

- **Retro** — double-line borders, ASCII/ANSI wordmark banner, chunky
  `█▓▒░` progress, DOS-era palettes (amber / phosphor / dos16).
- **Pastel** — soft low-saturation palette cohesive with a pastel ASCII-art
  backdrop; example source: [schemecolor Pastel](https://www.schemecolor.com/palettes/pastel)
  (lilac `#E1C0E9`, blush `#FCBBDB`, mint `#DCF1F0`, periwinkle `#C0C4E6`,
  peach `#F4DDC7`, cream `#F9F6ED`).
- **Copy pun:** optionally style user-facing "forensic" wording as **"furensic"**
  (banner subtitle / about / labels). Engine + internal docs keep "forensic".

Decide: one fixed theme vs. a few selectable variants; retro vs. pastel vs. both.
Whatever framework wins D1 should make theme swaps cheap (ties back to the
"library of styles" requirement).

### D4 — ASCII-art backdrop — superseded, then shelved

Superseded by the cat face, which is now the centre of the layout and the drop
target. It is bundled, drawn by the app and always on (see the
`pdfpundit-ansi-bbs.mockup.html` mockup). The random-cat fetch is **shelved as of
2026-09-26**. Its future job is to re-skin the animated cat face with a fetched
photo, which needs research first (see "Shelved: random cat skins" in the feature
plan).

Original note: the optional cat wallpaper (opt-in, off by default; bundled fallback when
offline) and its pipeline (`ureq` fetch → `image` decode → ASCII conversion →
pastel color-shift) live in the engine plan's crate stack as a self-contained
feature. UI-side question: **compositing** a dim backdrop *behind* opaque panels
depends on the framework — trivial with ratatui's cell `Buffer`, needs an
explicit cell-overlay compositor in lipgloss's string-composition model. Weigh
this in D1.

### D5 — Widget mode — decided

**Decided on 2026-09-26.** PDFPundit shrinks to a small cat-head tile, so it can
live on a tiling-window-manager desktop as a terminal widget. This fits the goal:
the cat stays on screen all day, and there is still no cat-free way in. Mocked in
frames 7–9 of `pdfpundit-ansi-bbs.mockup.html`.

- **Size.** The tile is 32×16 cells. The cat is half the full cat's width and
  height (a quarter of its area), with one status line and a compact status bar.
- **Entry.** Chosen automatically by terminal size: below 112×38 the app draws
  the widget layout, and enlarging the tile brings the full UI back.
  `[ui] layout = "auto" | "full" | "widget"` pins one.
- **States.** The cat shows:
  - idle: the meme pose behind the plate, "feed me a pdf"
  - working: contented, with a progress bar and n/total
  - needs you: wide eyes on the viewer, ‼ blinking, "zoom me"
  - done: "burp." and the tally
  - a drop: the same reactions as the full UI (the chomp, or tracking in
    kitty), at widget scale
- **Decisions are never shown in the widget.** A parked job flags "needs you".
  The user zooms the tile (the WM's zoom or fullscreen key), and the resize event
  switches to the full UI, where the prompt is waiting. Where the terminal allows
  it, the app also sends one xterm resize request (`CSI 8;38;112 t`, if
  `[ui] request_resize` is on). Most tiling WMs ignore that, so "zoom me" is the
  real signal.
- **Input in the widget.** A dropped path (paste), Enter (repeat the resize
  request) and `q`.
- **Smaller than 32×16.** A one-line fallback: `=^..^= 3/7 ‼`.

## UI modules

Written with deep modules in mind: a small interface over a lot of behaviour, and
a seam only where two adapters really vary. The design is framework-agnostic,
because D1 is still open. Paths are under `src/ui/` (technical design §2).

| Module | Interface | What it hides | Deletion test |
| --- | --- | --- | --- |
| `cat.rs`, the cat renderer | `render(pose: &Pose, size: CellSize, theme: &Theme, glow: f32) -> CellGrid`. Deterministic, no I/O, transparent outside the cat, works at any size. | Model geometry, the sphere warp that turns the head, supersampling, OKLab shading, half-block packing, keeping lines at least a pixel thick on small cats, and a cache. | Both layouts would each have to carry the geometry. |
| `director.rs`, the cat timeline | `Director::on(&mut self, CatEvent)` for `DragAt(Option<CellPos>)` (kitty only; `None` = left), `Dropped { n }` and `Mood(Mood)`; `Director::frame(&self, t: Duration) -> CatFrame { pose, glow, hint, file_at }`. `Mood` is Idle, Working { progress }, NeedsYou, Done { ok, partial, failed } or Failed. | The two drop reactions (each at most 10 keyframes, as data): the chomp, and in kitty meme → tracking → chomp. Also the mood poses and aiming the gaze. `file_at` is in cat-model space, so each layout maps it to its own cells. | Each layout would reimplement the timing. |
| `view.rs`, the view model | `view(app: &AppState) -> ViewModel`, pure. It holds counts, the current file, progress, `needs_you: Option<(usize, String)>` and the done summary. | Deriving that from `QueueEntry`, `BatchState` and pending interactions (technical design §7). | Both layouts would derive the same state separately. |
| `layout/`, **the seam** | `trait Layout { fn min_size(&self) -> (u16, u16); fn draw(&self, buf, vm: &ViewModel, cat: &CatFrame); }`, plus the pure `choose(size: (u16, u16), pin: LayoutPin) -> LayoutKind`, re-run on every Resize event. | Two adapters: `full.rs` (112×38) and `widget.rs` (32×16). Two adapters make it a real seam. The one-line fallback below 32×16 lives in `widget.rs`; it is not a third adapter. | — |

Each interface is also where its tests go:
- `render`: snapshot tests at both sizes.
- `Director::frame`: a drop with no drag before it starts the chomp at "plop";
  a drop after `DragAt` events skips to "chomp"; both end on "burp"; a drag
  that leaves without a drop returns to idle; each reaction has at most 10
  keyframes; NeedsYou makes the cat look at the viewer.
- `view`: tested on fixture app states.
- `choose`: a table of sizes and pins.

The framework gets no seam of its own: there is only one framework adapter, so
layouts draw straight into the chosen framework's buffer.

## Non-goals here

- No engine coupling: choosing/changing any of the above must not require engine
  changes. If a candidate framework can't consume the `JobEvent` stream cleanly,
  that's a mark against it — not a reason to change the engine.

## Next steps

- [ ] Investigate D1 candidates (versions, licenses, widget/style coverage,
      maintenance) — ratatui + styling helpers vs. lipgloss-rs vs. full
      bubbletea-rs stack vs. charmed-bubbles.
- [ ] Pick the framework against the "library of styles" requirement.
- [x] Lock D2 layout and D3 aesthetic; then build the mockup to the chosen
      aesthetic: `pdfpundit-ansi-bbs.mockup.html` (ANSI BBS, DarkBerry, cat-face
      drop target). The earlier amber CRT mockup was rejected and removed.
      To review or change it, start with
      [`../mockups/pdfpundit-ansi-bbs/README.md`](../mockups/pdfpundit-ansi-bbs/README.md):
      it has the generator, PNG stills of every frame, and the open issues.
- [x] Mock widget mode (D5): frames 7–9 (widget states, widget drop reaction,
      tiled desktop).
- [x] Mock the chomp, the drop reaction for terminals without drag events:
      frames 2b and 8b.
