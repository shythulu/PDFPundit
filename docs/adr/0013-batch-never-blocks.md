# 0013. The batch never blocks, and every answer records its source

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-005, D-103, D-153, D-020, D-126, D-131, D-141, D-147; plan amendment "gentle-gosling" §1

## Decision

1. **Autonomous by default.** A job that needs an answer parks in its queue row. Other jobs keep running.
2. **No count ever stops the batch.** At the 1 GiB memory cap, the runner evicts parked analysis state, cheapest first. On reply it replays the analysis and re-reads the input with a SHA-256 check. Evicted jobs re-run one at a time in a single resume slot.
3. **Every answer is `Answer { reply, source }`.** The source is `User`, `UseBest`, `Batched` or `Policy`.
4. `a` answers every waiting question at once. Answers carry over within a font family in the same file.

## Why

- A batch that stops on the first question wastes the user's time. It also makes results depend on when the user looked.
- The report must never claim the user saw a question the app answered for them. A forensic reader needs to know who decided each font.

## Rejected

- The technical design's blocking, one-job rendezvous (TD §3).
- The rev-5 cap: stop starting jobs at 8 parked jobs or 1 GiB.
- A per-file "ticks the repair tasks" checklist before repair (DA data flow, step 4).

## Rules for agents

- New interaction kinds add a source value only if none of the four fits.
- A test that answers a question must assert the recorded source.
