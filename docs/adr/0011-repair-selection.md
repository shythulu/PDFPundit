# 0011. Repair selection: generate every candidate, pick by a verification tuple

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-2 (D-008, D-009, D-059, D-074); GitHub issues #2 and #12; research WP-3.13, CLM-1063

## Decision

1. **Feasibility rules** decide which toolpaths can apply. There are two: `Resave` and `TemplateAssemble`.
2. **Every feasible candidate is built.** None is skipped on a prediction.
3. **A lexicographic verification tuple** picks the winner:

| Tier | Checks | Kind |
|---|---|---|
| V0 | Strict lopdf reload, hayro render of every page, re-diagnosis clean for the targeted classes | Pass or fail |
| V1 | Retention: blank pages, glyphs, text ops, images, path fills, content bytes | Integer ratios vs the input |
| V2 | Plausibility: U+FFFD count, unmapped glyphs, script consistency, dictionary hits | Integer ratios |
| V3 | Preserved features: catalog and trailer entries, annotations | Counts |

4. A fixed prior breaks exact ties between toolpaths. The font track's prior is `TemplateAssemble`.
5. A file with no corruption gets no candidates. It is reported as "nothing to repair".
6. No raster-derived number reaches the tuple. Blank pages come from hayro's paint counts.
7. Font auto-accept stays at `conf ≥ 0.35 && hit ≥ 0.5 && n_chars ≥ 20` until corpus calibration tunes it. Below 20 characters the slot always asks.

## Why

- Rules that predict a winner fail on files the rules did not foresee. Building all candidates and checking them is cheap at this scale.
- A lexicographic tuple is explainable. Each output can say which tier decided it.
- Rendering passes and "opens in a viewer" are necessary, not sufficient. Render comparators give 50% false positives at 80% true positives (CLM-1063). So V0 is a gate, never a correctness label.

## Rejected

- MCDM or weighted scores. Weights hide why a candidate won.
- A learned selector in v1. It needs labelled data that does not exist yet.
- The technical design's 0.60 reachable-fraction switch.

## Rules for agents

- The toolpath prior breaks ties between whole outputs. It never resolves byte-level ambiguity inside a stream. That stays `Ambiguous` ([0009](0009-c9-salvage-certainty.md)).
- When no candidate passes V0, the result is Partial or Unrepairable with a reason, never the best failing candidate.

## Revisit if

- Research shows a calibrated abstention rule (Platt-calibrated, length-aware) beats the fixed thresholds. Research WP-3.13 holds the design.
