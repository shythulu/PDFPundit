# Implementation plan (2026-10-07/08 agent panel)

Produced by an autonomous planning session: a lead planner and two adversarial
reviewers worked the wayfinder map (issue #1) with the grilling method while the
user was away. Nothing here overrides a recorded user decision; questions the
panel could not settle are in `decision-log.md` with a recommended answer.

| File | What it is |
| --- | --- |
| `plan.md` | The plan: fixed decisions, crate shape, the engine facade, build order, every ticket in full. |
| `tickets.json` | The same tickets, machine-readable, in dependency order. |
| `decision-log.md` | Every question the session could not determine: user decisions, reversible defaults, research still pending. |
| `map-resolutions.md` | Proposed resolution for the map and each ticket #2–#15, and what to post to each issue. |
| `grill-log.md` | Every grilling question and how it was disposed. |

The fact-finding reports the plan cites (FR-xx, eng-*, goal-*, lead-* ids) live
outside the repository in the session's working folder, because they contain
machine-specific notes; their conclusions are restated in the plan and log.
