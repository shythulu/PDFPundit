# 0022. UI: a framework-free canvas, tested cell for cell against the mockup

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-004, D-045, D-048, D-055, D-064, D-117, D-135, D-118, D-149

## Decision

1. ratatui and crossterm are used only behind `ui/term.rs`.
2. Layouts draw into our own `Canvas`, a port of the mockup's `generate.py`.
3. The mockup is the source of truth. Layouts must match goldens dumped from `generate.py` cell for cell. Masks are temporary.
4. Layout math uses `f64` throughout.
5. Three presentations: full (112×38), widget (32×16), and a one-line face below 32×16 that accepts no input.
6. A screen with no designed frame answers "not yet". It is not improvised.
7. Untrusted text, such as file names and PDF text, is drawn only through `Canvas::text`. Control, bidi and format characters become U+FFFD. Width uses `unicode-width`.
8. Under `NO_COLOR`, colour roles become attributes and the cat is drawn in shade blocks.

## Why

- Goldens make UI changes reviewable as data, and testable without a terminal.
- A framework-free canvas means the framework can change without redrawing every layout.
- A PDF can carry bidi overrides and control codes in its names and text. Drawing them raw lets a file spoof the screen.

## Rules for agents

- A UI change starts in the mockup, then regenerates the goldens, then changes the code.
- When the evidence schema, the UX design and the owner's later answers conflict, the owner's answers win, then the schema.
