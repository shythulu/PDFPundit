# Pending design amendments — autonomous repair flow

Queued changes to fold into `pdfpundit-technical-design.md` (+ the engine-flow
diagram and feature plan) once out of plan mode, combining the user's two flow
corrections with the deep-research results (running).

## 1. Autonomous-by-default data flow (user correction, 2026-07-17)

- **Default mode = auto-repair and output.** After analysis, the engine chooses
  the repair toolpath itself from the findings and proceeds straight through
  repair → emit → verify → `<name>.repaired.pdf` with **no user gate**.
- The old "step 6: user picks passes + font policy" is **not** a mandatory stop.
  Pass selection is automatic (driven by diagnosis); the config/context menu
  remains only as an optional override.
- **Human intervention is exception-only**: triggered when a decision is
  genuinely ambiguous — canonical case: a font's text can be *read* but not
  *reproduced* (glyphs decodable via surviving /ToUnicode, but no embeddable
  equivalent font resolves automatically). Then the job raises
  `NeedsInteraction`.
- **Interventions do not block the batch.** A job needing input is parked in a
  `WaitingOnUser` state and *flagged* in the queue; the runner immediately
  starts the next queued file. The parked job resumes when the user answers
  (or is skipped). This replaces the current blocking rendezvous semantics:
  the `sync_channel(1)` reply mechanism stays, but the runner must support
  **parked jobs + continue-with-next** instead of holding the single job slot.
  - Engine changes: `JobRunner` gains a `parked: Vec<ParkedJob>` set; queue
    entry status gains `WaitingOnUser`; batch progress counts parked jobs
    separately ("3/7 done · 1 needs input").
- Diagram: step 6 becomes a side branch off the repair path (exception loop),
  not an inline stage; add "auto toolpath selection" between diagnose and
  repair.

## 2. Repair-method selection matrix (research in flight)

Deep-research question: is there an established weighted matrix / decision
methodology to map analysis outcomes (C1–C10 findings, recovery ratios,
salvage outcomes, font-inference confidence) to the most effective repair
method with high precision — covering document-repair/forensics literature,
MCDM (weighted scoring, AHP, TOPSIS), rule-based diagnosis→remedy mapping,
automated-program-repair strategy selection, and confidence-threshold
escalation (auto vs. human-in-the-loop).

On completion: distill into a **toolpath-selection design** for
`pdf/diagnose.rs`/`pdf/repair.rs` — the decision inputs, weights/thresholds
(incl. `auto_accept_confidence`-style knobs), the Resave/TemplateAssemble
selector generalized into the matrix, and validation of the matrix against
the REPDF corpus in the benchmark harness.
