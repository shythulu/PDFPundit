# 0006. One package with a private, synchronous engine facade

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-003 (owner answer (a)), D-006, D-105, D-129; GitHub issue #10

## Decision

- One package, `pdfpundit` (lib and bin). The only workspace member is `tools/build-templates`.
- The engine is reached through a synchronous facade: `analyze`, `plan` and `repair` on bytes.
- Questions go out through an `Interact` trait and progress through a `Progress` trait. The engine never touches the terminal.
- `trait Engine` sits behind `JobRunner<E: Engine>`, so the runner is tested with a `FakeEngine`.
- `analyze` returns an opaque `StateHandle`. `repair` reuses it, so it never re-carves or re-salvages.
- Engine work runs on std threads with channels. There is no async runtime.
- The contract types are frozen by snapshot tests. `RepairReport` carries a `schema_version`.

## Why

- The work is CPU-bound. An async runtime adds weight and no throughput.
- A sync facade with traits is testable without a terminal.
- One package keeps the private-engine rule of [0003](0003-cat-is-the-only-way-in.md) simple: `pub(crate)` is enough.

## Rejected

- A separate engine crate. It would need a public API, which [0003](0003-cat-is-the-only-way-in.md) forbids.
- tokio or crossbeam (TD:60-63).

## Known debt

- `src/pdf` imports `crate::engine` and `bench::metrics`, so a later crate split is not mechanical (audit A2).
- Export bypasses the `Engine` seam (A3). `RunRecord` is built in `ui/app.rs` (A5).

## Rules for agents

- New engine capabilities extend the facade or the report. They never reach into UI code.
- A change to a contract type updates its snapshot in the same commit. Bump `schema_version` when the shape changes.
