---
planStatus:
  planId: plan-pdfpundit-ui-design
  title: PDFPundit — TUI / UI Design (decisions parked)
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
  updated: "2026-07-17T06:09:55.000Z"
  progress: 0
---
# PDFPundit — TUI / UI Design (decisions parked)

## Context

The terminal-UI framework, layout, and aesthetic are being **decided
separately** from the engine so the core application design
([pdfpundit-technical-design.md](pdfpundit-technical-design.md)) can stay focused
on the forensic PDF work. Everything here is **open** — candidates and
requirements to evaluate, not commitments. The engine is deliberately
UI-agnostic: it runs analyze/repair/export jobs on background threads and emits
a stream of typed events (`JobEvent`: progress, findings, needs-interaction,
done, failed); *any* UI layer consumes those events. So the choices below can be
made — and changed — without touching the engine.

> This plan absorbs the UI/aesthetic ideas that were floated during engine
> design (cat-first layout, pastel palette, "furensic" copy, Bubble Tea). They
> are recorded as **options**, not decisions.

## Guiding requirement

**A ready-made library of styles/components to pick from.** The priority is a
framework where common pieces (lists, viewports, progress bars, spinners, file
pickers, styled/bordered boxes, key-hint bars) come prebuilt and are easy to
theme — minimize hand-building widgets. Secondary: pure-Rust, single
self-contained binary, cross-platform (macOS/Windows/Linux), mouse support.

## Open decisions

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

### D2 — Layout (leading candidate: "cat-first")

The layout idea developed during engine design (recorded as the front-runner,
not final):

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

### D3 — Aesthetic / theme

Options floated (undecided):

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

### D4 — ASCII-art backdrop (optional cosmetic)

The optional cat wallpaper (opt-in, off by default; bundled fallback when
offline) and its pipeline (`ureq` fetch → `image` decode → ASCII conversion →
pastel color-shift) live in the engine plan's crate stack as a self-contained
feature. UI-side question: **compositing** a dim backdrop *behind* opaque panels
depends on the framework — trivial with ratatui's cell `Buffer`, needs an
explicit cell-overlay compositor in lipgloss's string-composition model. Weigh
this in D1.

## Non-goals here

- No engine coupling: choosing/changing any of the above must not require engine
  changes. If a candidate framework can't consume the `JobEvent` stream cleanly,
  that's a mark against it — not a reason to change the engine.

## Next steps

- [ ] Investigate D1 candidates (versions, licenses, widget/style coverage,
      maintenance) — ratatui + styling helpers vs. lipgloss-rs vs. full
      bubbletea-rs stack vs. charmed-bubbles.
- [ ] Pick the framework against the "library of styles" requirement.
- [ ] Lock D2 layout and D3 aesthetic; then build the mockup
      (`/mockup`, `pdfpundit-tui.mockup.html`) to the chosen aesthetic.
