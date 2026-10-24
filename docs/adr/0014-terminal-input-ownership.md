# 0014. Terminal input: the app owns stdin on kitty

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-5 (D-033, D-057, D-109, D-119)

## Decision

- On kitty, the app owns stdin after a handshake of at most 300 ms (`t=q` query plus DA1).
- It parses OSC 72 file drops itself.
- Unparsed terminal replies never reach the key handler.
- Windows and every other terminal use crossterm's event reader.

## Why

- crossterm 0.29 turns an OSC 72 drop into keystrokes. A dropped path would arrive as typed text.
- Windows console input is not a byte stream, so a raw reader cannot work there.

## Known limits

- An OSC 72 sequence split by more than 50 ms is replayed as keys.
- OSC 72 drops in Ghostty, WezTerm, foot and iTerm2 need a recheck before release.
- crossterm key types leak into four files outside `ui/term.rs` (audit A4).

## Rules for agents

- Terminal-specific parsing stays behind `ui/term.rs`. Do not read stdin anywhere else.
