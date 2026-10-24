# 0021. Custody mode is a future feature, not v1

- **Status:** accepted (deferred out of v1 by the owner, 2026-10-23)
- **Date:** 2026-10-23
- **Decided in:** owner ruling 2026-10-23; D-016; design on `impl/H-01:nimbalyst-local/plans/implementation/evidence-schema.md` rev 5 (owner answers Q27 to Q47; the schema's own Q1 to Q12 map onto them)

## Decision

- v1 ships without custody mode. Tickets T-33, H-02 to H-21 and H-32 to H-35 move to the backlog.
- The evidence-schema design on `impl/H-01` is kept as the starting point. It is not reviewed and not binding.
- The custody prompt and evidence screens (mockup frames 11, 13, 13b, 14) are not built in v1.

## Why v1 can drop it safely

- The pieces custody mode would rest on already ship:
  - the pure repair report and the separate run record ([0012](0012-determinism.md));
  - recorded answer sources ([0013](0013-batch-never-blocks.md));
  - no-clobber placement ([0002](0002-outputs-placed-never-replaced.md)).
- So custody mode can be added later without changing the engine.

## Design notes for whoever picks it up

1. Off by default.
2. Per job, three files in `output_dir`: `.custody.json`, `.custody.txt`, `.custody.sha256`.
3. Part B, reproducible: a pure function of the job, with its own hash. Part A, run: times, host, case details.
4. SHA-256 always. MD5 and SHA-1 only as labelled legacy hashes.
5. Each font slot records why its font was chosen, with the answer source.
6. The custody log is JSON Lines with a SHA-256 chain. Call it "tamper-evident", never "tamper-proof".
7. Custody code never reads a clock. Part A gets its times from the caller.
8. CASE/UCO JSON-LD output off by default. No per-case log.

## Rules for agents

- Do not build custody features in v1.
- Do not break the report / run-record split. Custody mode depends on it.
