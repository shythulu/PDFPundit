# 0007. The carver loads; lopdf emits and gates

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-3 (D-031, D-023, D-030, D-108); GitHub issue #6; research TOOL-359, OBS-1103

## Decision

1. **The carver is the loader.** It is our own tolerant scanner: memchr landmarks, byte-order object assembly, a stream-extent ladder, mandatory object-stream expansion and a gap sweep.
2. **lopdf is the sole emitter.** It writes every output under the four writer rules in the plan.
3. **A strict lopdf reload is a verification gate**, never a loader.
4. Object-stream members take precedence by their container's offset.
5. Elements, objects and inflates are capped, so hostile input cannot exhaust memory.
6. miniz_oxide `=0.9.1` is the only inflater in the carver, probe and salvage. flate2 is transitive only.
7. A bare CR after `stream` is accepted with a note.

## Why

- lopdf opens 0 of the C1 files and 0 of the C10 files in the REPDF corpus (OBS-1103). It cannot be the loader.
- The research's verdict on lopdf is Adopt "as a permissive Rust library and emitter, not as a strong repair baseline" (TOOL-359).
- One emitter means one object model and one place where output rules live.
- One inflater means one answer to "does this stream decode". The zlib-rs vs miniz tripwire found 0 of 1,152 smoke streams disagree.

## Known limits

- lopdf stores reals as `f32`. Values above about 7 significant digits narrow. It is reported as `reals_narrowed` (D-075).

## Open question (owner or next review)

- hayro-syntax opens C1 100/100 and C10 Save As 50/50 (OBS-1103). Its text output was never measured. pdfrum also beats lopdf on some classes.
- Committee 005 sent "lopdf as sole loader and emitter" to a review board that never sat.
- This ADR answers the loader half: the carver loads, so the finding does not apply. The emitter half stays open until someone measures krilla, pdf-writer or oxidize-pdf against lopdf's writer.

## Revisit if

- A writer candidate beats lopdf on lossless reals and strict reload with no new C.
