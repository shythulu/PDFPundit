# 0012. Determinism: work budgets, pure reports

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-6 (D-041, D-059, D-066, D-072, D-073, D-076, D-086, D-112, D-122, D-128, D-130, D-155); research charter §4

## Decision

1. **Budgets count work, never time.** One work unit W is one input byte inflated. Each inflate call adds 244 W. Settings are `work`, `deep_work` and `deep_pool`.
2. **The wall clock only cancels and displays.** `std::time` is banned crate-wide except in `ui/`, `jobs.rs`, `panic_guard.rs` and `tests/guard.rs`.
3. **The repair report is a pure function** of input bytes, app version, settings and interactions. Run facts (paths, times, placement tier) live in `RunRecord`.
4. **Parallel salvage allocates first and is admission-throttled.** The result does not depend on thread count or free memory. The output digest is identical at 1, 2, 4 and 8 threads.
5. **Ordered output only.** Use `BTreeMap` or a sorted `Vec` wherever order reaches output. `iter_over_hash_type` is denied. No rayon.
6. **No platform floating-point drift.** libm-lowered `f32`/`f64` methods are banned in `clippy.toml`. Use the `libm` crate.
7. The toolchain is pinned in `rust-toolchain.toml`. `.gitattributes` sets `eol=lf`.
8. Any learned component added later must be local, versioned and deterministic. Its model checksum goes in the report.

## Why

- The same input, version and settings must give a byte-identical file. A forensic tool has to reproduce its own result for someone else.
- Time budgets give different results on a slow machine. Pixel counts differ across CPUs because of SIMD dispatch.

## Known gaps

- `sin_cos` is not banned yet.
- The guarantee does not cover dependencies. hayro and kurbo call libm (audit A6).

## Rules for agents

- A new budget is in work units. A new output field that depends on the run goes in `RunRecord`, not the report.
