# 0004. Pure-Rust build with no network code

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** owner ruling 2026-10-23 on the zlib exception; D-029, D-050, D-053, D-082, D-138; research charter §5

## Decision

1. **No compiled or vendored C, C++ or assembly** in the build, with one exception: stock zlib for replay (below). CI's purity gate allows only listed `-sys` crates and rejects `links` and `cc` build steps.
2. **FFI is limited to OS APIs** for terminal I/O and app directories, through pure-Rust binding crates.
3. **No HTTP or TLS crate** in `Cargo.lock`. CI's network gate enforces it. There is no `offline` setting because there is nothing to switch off.
4. **Tools written in C stay outside the binary.** Tesseract, qpdf, MuPDF, Ghostscript and Poppler run only in research or the eval harness, as external commands.

## The one exception: stock zlib for replay

Owner ruling, 2026-10-23. Stock zlib (C) may be linked for one job: re-compressing a C9 candidate to check it ([0010](0010-replay-oracle-stock-zlib.md)).

- Link it statically through `libz-sys` with its `static` feature, so the zlib version is pinned in `Cargo.lock`. Never use the system zlib.
- Never enable `zlib-ng` or a zlib-ng compat mode. zlib-ng's output differs from stock zlib's, which breaks replay.
- Use it to compress only. miniz_oxide stays the only inflater ([0007](0007-carver-loads-lopdf-emits.md)).
- Call it from one module. No other code may use it.
- The purity gate allow-lists `libz-sys` by name. Every other `-sys`, `cc` or `links` crate is still refused.
- zlib's licence is permissive (Zlib). Add `Zlib` to the `deny.toml` licence allow-list if it is missing.

Why the exception: replay needs byte-identical zlib output. Of the pure-Rust options, zlib-rs reproduces 0 of 400 streams and miniz_oxide 5 of 400. The two that come close both need a fork (OBS-0902, OBS-1100).

Cost: every release target now needs a C compiler. The GitHub runners for all three targets have one.

## Why

- One clean binary cross-builds for macOS, Linux and Windows with no C toolchain.
- The tool may run air-gapped on evidence. No network code means no question about what leaves the machine.
- Accepted cost: the tolerant carver is the robustness ceiling. qpdf, MuPDF or PDFium cannot be linked in to help.

## Rejected

- Linking qpdf, MuPDF, PDFium or bundled SQLite (C).
- First-use OCR model downloads with a kill switch (D-050 (b)). Replaced by bundled models or a model pack from a file.
- Tesseract or onnxruntime bindings for product OCR (C/C++).

## Rules for agents

- A new dependency must pass `cargo deny` and the purity and network gates before it lands.
- The gates do not check `path` packages yet (D-082). Treat that as a gap, not a loophole.
