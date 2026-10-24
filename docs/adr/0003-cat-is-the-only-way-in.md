# 0003. The cat is the only way in: no command-line interface

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-4 (D-002, D-043, D-047, D-064, D-080, D-081); desktop-app plan DA:45, 63-64, 123

## Decision

The shipped binary has one entry point: the terminal UI.

- argv is ignored. There are no subcommands and no flags.
- A non-terminal stdin or stdout exits with code 2.
- No input is accepted before the first frame. Pre-frame bytes are discarded and the terminal input buffer is flushed (`tcflush` / `FlushConsoleInputBuffer`).
- The one-line face (below 32×16) accepts no input.
- The crate has `publish = false`. The engine is `pub(crate)` with no cfg, feature or flag that exposes it. A git dependant sees `run()` and nothing else.
- The corpus harness is an `#[ignore]` test, not a binary. It reads only files in the pinned 1,100-entry blob manifest ([0018](0018-repdf-corpus-regression-only.md)).

## Why

- The owner's product goal: "the cat is the only way in". Headless batch use is deferred to a later product (Pundit+).
- A `--cfg` gate was tried and rejected. An environment `RUSTFLAGS` drops it silently, and a dependant can set it.

## Known limits

- A pty can still script pastes into the UI. This ADR does not claim to stop automation.
- The source is public. Reaching the engine means editing the source, which is a fork. Forks are not prevented.

## Rules for agents

- Do not add a CLI, a `src/bin/*`, a public engine API or an env var that drives the engine.
- Developer switches exist only under `cfg(debug_assertions)` (D-051). No env var changes engine output in a release build.
