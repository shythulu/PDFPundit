# Repair-method (toolpath) selection, human escalation, and evaluation methodology for PDFPundit

Scope: evidence-based design for (1) picking the repair toolpath from diagnostic findings and deciding when to escalate to a human, and (2) evaluating repair quality rigorously. Grounded in adjacent literatures: automated program repair (APR), algorithm selection, rule-based expert systems, MCDM, selective prediction and calibration, document-parsing benchmarks, PDF validation, and experimental statistics. Current as of September 2026. Older foundational work is marked with its year.

PDFPundit context these notes respond to (from `pdfpundit-technical-design.md` and `review-the-plan-and-gentle-gosling.md`):
- §4.4/§17.4: selector is `TemplateAssemble iff passes ∩ {C6,C7,C8} ≠ ∅ or (C10 and reachable_fraction < 0.60); else Resave`. Post-emit verification: strict lopdf reload, hayro render all pages, re-diagnose.
- §5.2/§18.2: `conf = 0.5·hit + 0.5·margin`, `margin = (best − second)/max(best, ε)`, auto-accept iff `conf ≥ 0.35 && hit ≥ 0.5`, else `Interactive(FontPick)`. Weights are described as "tuned on corpus in M7", but no tuning procedure is given.
- §8: text recovery = `2·matched/(len_orig + len_repaired)` over a word-level Myers diff of hayro-extracted text. Image recovery = decoded-pixel-hash matches / originals. PR gate: `recovery ≥ baseline − 2%` on ~30 cached corpus files.
- Gentle-gosling plan: autonomous by default. `NeedsInteraction` only for genuinely ambiguous cases. The canonical case is text that can be read (a /ToUnicode map survives) when no embeddable font resolves.

---

## Q1. Is there an established methodology for mapping diagnostic findings to the most effective repair method, and which fits best when outputs can be cheaply verified?

### Takeaway
No single "repair selection matrix" standard exists. Four literatures bear on the problem:
- Algorithm selection (Rice 1976; SATzilla 2008) is the formal framework for going from instance features to the best method.
- APR and the portfolio literature show that when candidates are cheap to run and check, the best practice is to generate a small set of candidates and then validate and rank them.
- Rule-based expert systems (MYCIN, 1970s) show that the value lies in a transparent mapping from findings to remedies, not in hand-set numeric weights.
- MCDM methods (AHP, TOPSIS, weighted sums) fit poorly: their weights are subjective and some suffer rank reversal.

Because PDFPundit can re-parse, render and re-diagnose its own output, the best fit is a hybrid in three steps:
1. Rules decide which toolpaths are feasible.
2. All feasible toolpaths run.
3. A lexicographic verification score picks the winner, with calibrated deferral to a human when needed.

### Cited Findings
- **Rice (1976)** formalized the algorithm selection problem as a mapping from an instance's feature space to algorithm performance. The survey literature treats this as the founding framework, "guiding research for fifty years" — [ZeroFolio, arXiv 2604.19753 (2026)](https://arxiv.org/html/2604.19753v2); [Kotthoff survey, arXiv 1210.7959](https://arxiv.org/abs/1210.7959).
- **SATzilla (Xu, Hutter, Hoos, Leyton-Brown, JAIR 2008)** starts from the observation that "there is no single 'dominant' SAT solver; instead, different solvers perform best on different instances". Picking one winner per class ("winner-take-all") "has resulted in the neglect of many algorithms that are not competitive on average but that nevertheless offer very good performance on particular instances". SATzilla instead predicts per instance using empirical hardness models — [SATzilla, arXiv 1111.2249](https://arxiv.org/pdf/1111.2249).
- SATzilla's runtime procedure has three steps. First, run pre-solvers up to a fixed cutoff. Second, compute features, and if feature computation fails, run a pre-selected backup solver. Third, run the solver predicted to be best, and if it crashes, run the next-best — [SATzilla, arXiv 1111.2249](https://arxiv.org/pdf/1111.2249).
- The term "algorithm portfolio" (Huberman et al., 1997) originally meant "running several algorithms in parallel". Later work broadened it to any strategy that uses multiple black-box algorithms on one instance — [SATzilla, arXiv 1111.2249](https://arxiv.org/pdf/1111.2249).
- **ASlib (Bischl et al., 2016)** standardizes how algorithm selectors are evaluated. It uses nested cross-validation for hyperparameter tuning "to ensure unbiased performance results". The ICON challenge withheld its train/test splits "to avoid overly strong overfitting". Using several metrics (solved instances, PAR10, misclassification penalty) "revealed some strengths and weaknesses of algorithm selectors" — [ASlib, arXiv 1506.02465](https://ar5iv.labs.arxiv.org/html/1506.02465).
- Selectors are conventionally reported as the fraction of the gap between the single best solver (SBS) and the virtual best solver (VBS, a per-instance oracle) that they close — [ZeroFolio, arXiv 2604.19753 (2026)](https://arxiv.org/html/2604.19753v2).
- A 2026 floorplanning study addresses "the degenerate but practically important case in which all strategies are run and the policy must only rank their outputs". The failure mode it documents is "that the ranking cost itself can be mis-calibrated against the final evaluation metric". Source note: this comes from a secondary snippet on ResearchGate, and the primary paper was not read — [ResearchGate citing snippet, "Gap-Aware Candidate Reranking for Multi-Strategy VLSI Floorplanning", Jan 2026](https://www.researchgate.net/publication/232735414_Algorithm_Selection_for_Combinatorial_Search_Problems_A_Survey).
- **APR generate-and-validate with learned ranking (Prophet, Long & Rinard, POPL 2016).** Prophet generates a space of candidate patches, "uses the model to rank the candidate patches in order of likely correctness, and validates the ranked patches against a suite of test cases". It beat SPR's hand-coded ranking heuristics, which in several cases put an incorrect plausible patch first. A maximum-likelihood objective that considers all patches outperformed a hinge loss that considers only two — [Prophet paper](https://people.csail.mit.edu/fanl/papers/prophet-popl16.pdf).
- **Kali (Qi et al., ISSTA 2015)** used a much smaller, focused search space ("always less than 1700 patches", versus tens of thousands for earlier systems). It found as many correct and plausible patches as GenProg, RSRepair and AE — [Qi et al. 2015](https://people.csail.mit.edu/rinard/paper/issta15.pdf).
- **MYCIN (1970s rule-based diagnosis→therapy system)** used a knowledge base of ~600 production rules, cleanly separated from its inference engine. It ranked diagnoses, explained its reasoning, and recommended treatment. Its developers found "performance was minimally affected by perturbations in the uncertainty metrics associated with individual rules, suggesting that the power in the system was related more to its knowledge representation and reasoning scheme than to the details of its numerical uncertainty model". Later work showed that the certainty-factor model's implied assumptions are problematic (Heckerman & Shortliffe 1992) — [Wikipedia: Mycin](https://en.wikipedia.org/wiki/Mycin). Britannica gives "about 500 production rules", so the rule count differs between sources — [Britannica](https://www.britannica.com/technology/MYCIN).
- **MCDM caveats.**
  - Weighted-sum, TOPSIS, AHP, ELECTRE and PROMETHEE all aggregate criteria through weights. When those weights come from subjective decision-maker input, sensitivity analysis on the weights is "a key approach in assessing the robustness of the MCDA results" — [O'Shea et al., ESWA 2026 (weight stability intervals for WSM)](https://www.sciencedirect.com/science/article/pii/S0957417425020792).
  - AHP can reverse the ranking of two alternatives when an unrelated alternative is added or removed ("rank reversal", Belton & Gear 1983), which "has led to some criticism of the validity of the AHP" — [AHP rank reversals, Annals of OR 2023](https://link.springer.com/content/pdf/10.1007/s10479-023-05278-6.pdf?pdf=button).
  - TOPSIS has its own rank-reversal literature — [Computers & Industrial Engineering 2022](https://www.sciencedirect.com/science/article/abs/pii/S0360835222007641).
- **REPDF itself** picks a font by generating text through each candidate font's Unicode mapping, "followed by automatic or manual comparison to select the font that yields the most meaningful result". In other words, REPDF also generates candidates, scores them, and keeps a manual fallback — [REPDF paper, FSI: Digital Investigation 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).

### Inferences
- The fit between each paradigm and PDFPundit's situation (a handful of strategies, each cheap to run, with a self-check available):
  - **Predict-then-act (SATzilla-style).** Worth it only when running every candidate costs too much. SATzilla predicts because it cannot afford to run every SAT solver on every instance. PDFPundit's rebuilds run in seconds on typical files, so this does not apply except for very large files.
  - **Generate-and-validate.** The best fit: PDFPundit has both a cheap oracle (re-parse, render, re-diagnose) and a tiny candidate space. This matches Kali's lesson that a small, focused search space is enough.
  - **MCDM (AHP/TOPSIS).** Wrong tool. It suits decisions without an outcome oracle, where criteria weights encode stakeholder preferences. PDFPundit can measure outcomes against pristine originals on a corpus, so weights should be learned or validated, not elicited. Rank reversal is also undesirable in an engine where adding a third toolpath should not flip the Resave vs TemplateAssemble ranking.
  - **Rules (MYCIN lineage).** Correct for the feasibility layer: which toolpaths make sense for which findings (e.g., `/Encrypt` means Unrepairable, and C7/C8 need font work). MYCIN's own robustness finding says the numbers attached to rules matter less than the structure.
- The existing §17.4 selector is a pure rule-based predict-then-act selector. Its fixed 0.60 reachability cut-off for C10 is exactly the kind of hand-set constant the algorithm-selection literature replaces with either (a) running both candidates or (b) a threshold learned on held-out data.
- The floorplanning finding carries over directly. If PDFPundit ranks candidates by a proxy score (render success, dictionary hit-rate), it must check on the corpus that the proxy ranking agrees with true recovery against the pristine originals. Otherwise the ranker is "mis-calibrated against the final metric".

### Gaps
- No published decision logic was found for commercial PDF repair tools (iLovePDF, Wondershare Repairit, PDF24; all benchmarked as black boxes by REPDF) or for general data-recovery tools. Their selection logic is proprietary.
- No literature was found that applies algorithm selection or MCDM specifically to file-format repair. The mapping above is by analogy.
- Earlier document- and input-repair work cited by Kuchta et al. (Demsky & Rinard 2005 data-structure repair; Long et al. 2012 input rectification; Kuchta et al. 2014 document recovery) was not fetched. Its decision logic is unverified here.

### What PDFPundit should do
1. **Replace the binary selector with a three-layer toolpath engine in `pdf/diagnose.rs` → `pdf/repair.rs`.**
   - **Layer A — feasibility rules** (a transparent table keyed on Findings; MYCIN-style, explainable in the report):

     | Findings present | Candidate toolpaths generated | Notes |
     | --- | --- | --- |
     | `/Encrypt` | none | `Unrepairable("decrypt first")`, as today |
     | only C1–C3 | `Resave` | TemplateAssemble only as fallback if Resave fails the hard gates |
     | C4 and/or C5 (± C1–C3) | `Resave`, `TemplateAssemble` | both run. Resave usually wins on preservation |
     | C9 (any) | orthogonal: salvage ladder runs before either toolpath | `RepairedByteFlip` with an Adler-32 pass is a strong oracle. `Prefix` is always `Partial` |
     | C10 | `Resave`, `TemplateAssemble` | both run. The 0.60 constant is dropped from the decision and kept only as a prior for tie-breaks |
     | C7 with surviving `/ToUnicode` | `TemplateAssemble` (+ `Resave` as the structure-preserving comparison) | the "readable but not reproducible" family. See Q3 for escalation |
     | C6 / C8 | `TemplateAssemble` + font inference | font picks go through calibrated deferral (Q3) |

   - **Layer B — generate:** run every candidate from Layer A under a per-file time budget. As in SATzilla's pre-solver cutoff, if the budget is exceeded, fall back to the rule-preferred candidate.
   - **Layer C — validate and rank:** use the lexicographic verification tuple from Q2. Weights are needed only inside tiers, and are learned (Q2).
2. **Evaluate the selector the way ASlib evaluates selectors:**
   - Compute per-file true recovery (vs pristine) for always-Resave, always-TemplateAssemble, and the oracle VBS (the per-file max).
   - Report the fraction of the SBS→VBS gap the selector closes, per corruption class.
   - This is the "validation of the matrix against the REPDF corpus" the gentle-gosling plan asks for.
3. **Keep the old rule-based selector as the documented prior and tie-breaker.** Do not replace it with AHP or TOPSIS.

---

## Q2. Generate-and-validate vs predict-then-act; overfitting and "plausible-but-wrong" repairs

### Takeaway
APR's central lesson (2015 onward) is that validating against a weak oracle yields many plausible-but-incorrect outputs. The most common plausible-but-incorrect patch is one that deletes functionality. The PDF analogue is a repaired file that reloads, renders and re-diagnoses clean because content was dropped or wrong glyphs were emitted. Generate-and-validate is the right architecture for PDFPundit only if validation includes anti-deletion and content-fidelity checks, and only if some oracles are held out from selection.

### Cited Findings
- **Qi, Long, Achour, Rinard (ISSTA 2015):**
  - "Of the reported 414 GenProg patches, only 110 are plausible". GenProg produced a correct patch for only 2 of 105 defects, RSRepair for 2 of 24, and AE for 3 of 105.
  - "104 of the 110 plausible GenProg patches, 37 of the 44 plausible RSRepair patches, and 22 of the plausible 27 AE patches are equivalent to a single modification that deletes functionality."
  - GenProg's fitness function counts passed tests. With typically one negative test, "the difference between the fitness of the unpatched code and the fitness of a plausible patch that passes all test cases is only one."
  - Harness bugs let implausible patches through: GenProg "checks only that the higher order 8 bits of the exit code … are 0."
  - Source: [Qi et al. 2015](https://people.csail.mit.edu/rinard/paper/issta15.pdf).
- **Smith, Barr, Le Goues, Brun (FSE 2015)**, "Is the cure worse than the disease?", established "overfitting" in APR: patches pass the validation tests without being correct — [ACM DL](https://dl.acm.org/doi/10.1145/2786805.2786825). The problem is still considered "a major and long-standing challenge" — [Petke et al. 2024, ResearchGate](https://www.researchgate.net/publication/382158626_The_Patch_Overfitting_Problem_in_Automated_Program_Repair_Practical_Magnitude_and_a_Baseline_for_Realistic_Benchmarking). A 2025/2026 follow-up asks the same question for LLM-based repair — [arXiv 2511.16858](https://arxiv.org/abs/2511.16858v1).
- **2026 patch-overfitting-detection study:** "dynamic approaches excel at identifying correct patches (while over-predicting correctness for overfitting patches), whereas learning-based methods are better at identifying overfitting patches (at the cost of discarding many correct ones) … a combination … could offer the best of both worlds" — [arXiv 2603.11262](https://arxiv.org/html/2603.11262v1).
- **Prophet (2016):** "program value features are more important than modification features for distinguishing correct patches from plausible but incorrect patches" — [Prophet](https://people.csail.mit.edu/fanl/papers/prophet-popl16.pdf).
- **A single renderer is a weak oracle.**
  - Kuchta et al. (EMSE 2018) found that "13.5% PDF files are inconsistently rendered by at least one popular reader" (study of 2,313 files). Their automated approach on 230K Govdocs1 files with 11 readers found 30 unique bugs.
  - Inconsistencies come from bugs in the reader or in the file, and many files that one reader tolerates are rendered differently by another.
  - Source: [Kuchta et al. 2018](http://srg.doc.ic.ac.uk/files/papers/pdf-emse-18.pdf).
- **Whole-page image checks miss local damage.** Kuchta et al.: "a document in which only a few words are rendered with an incorrect font; automatically detecting those few incorrectly rendered words is hard … 'local' discrepancies are very hard to spot when looking at the entire pages". They suggest comparing image regions around text boxes instead — [Kuchta et al. 2018](http://srg.doc.ic.ac.uk/files/papers/pdf-emse-18.pdf).
- **An analogous accept-or-abstain design in OCR (2026).** A risk-controlled generative-OCR system "applies lightweight structural validity checks, and accepts a transcription only when cross-view agreement is sufficiently stable". Residual failures include "stable-but-wrong" outputs — [arXiv 2603.19790](http://arxiv.org/html/2603.19790v1).

### Inferences
- **The PDF analogues of "deletion patches":**
  - Resave or TemplateAssemble emits fewer pages than were discovered.
  - A content stream is replaced by an empty or truncated one (e.g., a C9 `Prefix` treated as complete).
  - A page renders blank.
  - A font is substituted so that ToUnicode-extracted text is correct but the visible glyphs are wrong, or the reverse.

  All of these can pass PDFPundit's current gates: strict reload, hayro render without error, and re-diagnose clean. Re-diagnose clean only shows that the detectors no longer fire. Like GenProg's fitness function, it is a weak proxy.
- **The strongest self-contained anti-deletion oracle is the input itself.** The carver and graph already know how many `/Page` objects, content streams, images and text-show operators exist. Any candidate must retain at least what was carved, or explain the loss as a Finding.
- **Use cross-candidate agreement as a quality signal**, analogous to the OCR paper's cross-view agreement. When Resave and TemplateAssemble both pass the gates but extract materially different text, at least one is wrong, and the engine should defer (Q3) rather than guess.
- **Beware overfitting at the corpus level, not just per file.** The design's C9 single-byte-flip brute force exploits REPDF's corruption generator ("the corpus's C9 flips exactly one byte", design §4.5). The C10 fixtures truncate at exactly 70%. These are the repair-tool analogue of patches overfitting a test suite: repairs overfit the corruption generator. A held-out corruption model is needed (Q6).

### Gaps
- No study was found that measures a "plausible-but-wrong" rate for PDF repair tools specifically. REPDF reports recovery rates, not the rate at which outputs look valid but are wrong. PDFPundit would be first to report it.

### What PDFPundit should do
1. **Verification tuple per candidate**, compared lexicographically in this order:
   - **V0 hard gates (all must pass):**
     - strict `lopdf` reload succeeds;
     - hayro renders every page without error;
     - re-diagnose shows the targeted classes cleared and no new classes;
     - `page_count(out) ≥ pages_discovered_by_carver`.
   - **V1 retention (anti-deletion)**, compared to references built from the input, not the output:
     - `text_ops_retained / text_ops_carved`;
     - `images_emitted / images_carved`;
     - content-stream bytes retained;
     - no page renders blank unless it was blank in the carve. Measure blankness as non-background pixel fraction below ε at low DPI.
   - **V2 text plausibility:**
     - U+FFFD fraction in the extracted text;
     - dictionary hit-rate of the extracted text (reuse the §18 dictionaries);
     - script consistency;
     - agreement with the surviving-/ToUnicode decode when one exists.
   - **V3 preservation:** outlines, annotations, `/Info` and XMP metadata, and AcroForm retained. This is Resave's advantage.
   - **V4 tie-break:** rule prior (Resave preferred), then smaller output.
2. **Learn any within-tier weights; do not hand-set them.**
   - On the tuning split, build candidate pairs `(Resave, TemplateAssemble)` for the same file.
   - Label a pair by which candidate had the higher true recovery against pristine.
   - Fit a pairwise logistic model on feature differences. This is the Prophet-style maximum-likelihood ranking objective.
   - Report weight-stability intervals to show the ranking is not knife-edge.
3. **Keep one oracle out of selection** so it can measure overfitting. Candidates are selected with hayro plus the self-checks above. The corpus harness then scores the chosen output with things the engine never saw: a second renderer, pristine-text comparison, and OCR (Q5).
4. **Report a new KPI, the plausible-but-wrong rate:** the fraction of files whose chosen output passes every V0 gate but whose true recovery is below the class target (e.g., < 0.9). Report it per class, next to recovery. This is the direct analogue of APR's plausible-vs-correct gap.

---

## Q3. Human-in-the-loop escalation: selective prediction, learning to defer, calibration, and setting thresholds from a target precision

### Takeaway
The design question has a textbook form: selective prediction with a reject option (Chow 1970; Geifman & El-Yaniv 2017). The procedure has three steps:
1. Turn the hand-made confidence into a calibrated probability of correctness on held-out data. Platt or logistic scaling suits calibration sets below roughly 1,000 examples.
2. Choose `auto_accept` thresholds from a target selective risk, with a finite-sample guarantee (SGR, or Learn-then-Test).
3. Report the risk–coverage trade-off.

Escalation should also be triggered by candidate disagreement and out-of-distribution inputs, not just low font-inference confidence. The canonical "readable but no embeddable font" case is a policy ambiguity rather than a prediction ambiguity, and should be handled as a batched prompt.

### Cited Findings
- **Reject option (Chow, 1970)**: optimal error–reject trade-off for a known likelihood ratio. El-Yaniv & Wiener (JMLR 2010) laid the learning-theoretic foundations of selective classification — [SCORC, arXiv 2606.08517 (2026)](https://arxiv.org/pdf/2606.08517).
- **Selection with guaranteed risk (Geifman & El-Yaniv, NeurIPS 2017):** "allows a user to set a desired risk level. At test time, the classifier rejects instances as needed, to grant the desired risk (with high probability)". Example: "2% error in top-5 ImageNet classification can be guaranteed with probability 99.9%, and almost 60% test coverage" — [arXiv 1705.08500](https://arxiv.org/abs/1705.08500).
- **Two formulations of the trade-off.** Bounded-improvement maximizes coverage subject to a guaranteed selective risk; bounded-abstention minimizes risk subject to a guaranteed coverage. The area under the risk–coverage curve (AuRC; Franc et al., JMLR 2023) is recommended as a summary metric — [Conservative Decisions with Risk Scores, arXiv 2509.25588](https://arxiv.org/html/2509.25588v1).
- **Learning to defer (Madras, Pitassi, Zemel, NeurIPS 2018)** "generalizes rejection learning by considering the effect of other agents in the decision-making process". Deferral should account for how accurate (or biased) the downstream human is — [NeurIPS 2018](https://proceedings.neurips.cc/paper_files/paper/2018/file/09d37c08f7b129e96277388757530c72-Paper.pdf). Consistent estimators for learning to defer: Mozannar & Sontag 2020 — [arXiv 2006.01862](https://arxiv.org/abs/2006.01862).
- **Learn then Test (Angelopoulos, Bates, Candès, Jordan, Lei, 2021):** calibrates "any underlying model" so predictions satisfy "explicit, finite-sample statistical guarantees" without refitting, by "reframing the risk-control problem as multiple hypothesis testing" — [arXiv 2110.01052](https://arxiv.org/abs/2110.01052). Conformal risk control (2022) and selective-CRC variants (2025–2026) extend this — [SCORC related-work survey, arXiv 2606.08517](https://arxiv.org/pdf/2606.08517).
- **An OCR system deployed with this pattern (2026):** "a single strictness knob m exposes a small family of operating points on a risk–coverage curve, enabling explicit operating-point control rather than ad hoc retuning". Risk is reported on the covered subset together with coverage — [arXiv 2603.19790](http://arxiv.org/html/2603.19790v1).
- **Calibration (Guo et al., ICML 2017):**
  - Reliability diagrams plot accuracy against confidence. ECE bins predictions into M equal-width intervals and averages |accuracy − confidence|.
  - Temperature scaling (a single parameter) was the most effective method. "Binning methods … tend to change class predictions". "Any calibration model with tens of thousands (or more) parameters will overfit to a small validation set".
  - Their summary: "calibration is best corrected by simple models."
  - Source: [arXiv 1706.04599](https://arxiv.org/pdf/1706.04599v2).
- **Platt vs isotonic (Niculescu-Mizil & Caruana, ICML 2005):** "When the calibration set is small (less than about 200-1000 cases), Platt Scaling outperforms Isotonic Regression … because Isotonic Regression is less constrained". "When there are 1000 or more points in the calibration set, Isotonic Regression always yields performance as good as, or better than, Platt Scaling". Isotonic "can correct any monotonic distortion" — [N-M & Caruana 2005](https://www.cs.cornell.edu/~alexn/papers/calibration.icml05.crc.rev3.pdf).
- **REPDF's font accuracy depends on creation method.** For C8 (fonts and /ToUnicode both lost), "Save As" files keep real font names, which enables database lookup. "Print to PDF" files use renamed fonts ("CIDFont+F1") and fall back to automatic font mapping, giving recovery similar to C6 — [REPDF 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).

### Inferences
- **`conf = 0.5·hit + 0.5·margin` is an uncalibrated score, not a probability.** A value of 0.35 has no stated relationship to error rate. Two flaws show up in the §18.2 definition:
  1. Scores can be negative because of the `P_UNMAPPED`, `P_RARE` and `P_MIXED` penalties. When `best.score ≤ 0`, `margin = (best − second)/max(best, ε)` divides by ε and explodes. Margin should be computed on a non-negative transform (e.g., a softmax over candidate scores, or `best.hit − second.hit`).
  2. The formula ignores sample size. A slot with 12 decoded characters and a slot with 5,000 can get the same conf. `log(n_chars)` belongs among the calibration features.
- **What "correct" means for calibration labels.** Define a per-font-slot label on the tuning split: 1 if the auto-picked font yields per-slot text recovery ≥ 0.95 against pristine (or the font id matches the ground-truth family), else 0. Also label per-document "acceptable repair" (recovery ≥ class target) for document-level deferral.
- **Sample size needed for a risk guarantee.** Rule-of-three arithmetic (not from a fetched source): certifying ≤ 2% wrong auto-picks at 95% confidence with zero observed errors needs about 150 auto-accepted labeled slots; with a few errors it needs several hundred. REPDF's C6–C8 subset (probably ~300 files; unverified) gives multiple font slots per file across six languages, which is likely enough for a slot-level logistic calibration. It is probably not enough for per-language isotonic curves. So use a pooled logistic model with a language feature.
- **Two kinds of ambiguity.** Epistemic ambiguity ("which font is this?") is what calibrated deferral handles. Policy ambiguity (readable text but no embeddable font: should the engine substitute a generic font, keep text only, or wait for the user to supply a font?) is a user preference. It does not become clearer with more data. It suits a per-batch default plus the §5.5 "Apply best to all" prompt, not a per-file stop.
- **Learning to defer adds a point:** escalation only helps if the human beats the engine on the escalated cases. Humans picking fonts from a 200-character preview are probably good on Latin-script prose and weaker on unfamiliar scripts. This can be measured.

### Gaps
- The number of labeled font slots in the REPDF corpus is unknown until the corpus is processed. Whether isotonic calibration is viable is therefore undetermined.
- The fixed-sequence-testing mechanics of LTT and the exact SGR binary-search algorithm were not read in full (abstracts only). The procedure below follows their stated framing plus standard binomial bounds.
- No empirical data exists on human accuracy at PDFPundit's FontPick task.

### What PDFPundit should do
1. **Instrument inference.** For every font slot, log `(hit, margin_fixed, raw_best, raw_second, n_chars, n_runs, lang, script, tounicode_dict_used, creation_method_hint, rank_of_true_font)` to the corpus CSV.
2. **Calibrate.**
   - Fit `p_correct = σ(w·x + b)` (Platt/logistic) on the tuning split. Start with `x = [conf, hit, margin_fixed, log n_chars, has_tounicode_dict, lang one-hot]`.
   - Switch to isotonic only if at least 1,000 labeled slots are available. Store `w` and `b` in `fontdb` config, versioned with the dictionaries.
   - Report a reliability diagram (10 bins), ECE, and Brier score on the calibration split and on the test split.
3. **Set `auto_accept` from a target risk.**
   - Config becomes `repair.font_max_selective_risk = 0.02` and `repair.risk_confidence = 0.95`, replacing the magic 0.35.
   - On the calibration split, sweep τ from strict to loose. For each τ compute accepted count `n` and errors `k`, and a one-sided Clopper-Pearson (or Wilson) upper bound U(τ).
   - Walk down from the strictest τ and keep going while U(τ) ≤ target (SGR/LTT style). Ship the loosest τ that passed.
   - Keep `hit ≥ 0.5` as a hard out-of-distribution floor.
   - Publish the risk–coverage curve and AuRC for each release.
4. **Document-level escalation reasons.** `NeedsInteraction` carries a machine-readable reason, and the job is parked as `WaitingOnUser` without blocking the batch:
   - `FontPick` — some slot has `p_correct < τ`.
   - `FontUnreproducible` — /ToUnicode decodes but no bundled or system font covers the codes. This is the canonical case, a policy prompt. Batch it per font family across the queue, with a configurable default (`fonts.unreproducible = ask | substitute_generic | text_only`). Default `ask`, per the user's intent.
   - `CandidatesDisagree` — two candidates pass V0 but their extracted-text LCS-F1 is below δ. Learn δ from the tuning-split ROC for separating good from bad chosen outputs.
   - `OutOfDistribution` — a script or language without dictionaries, feature values outside the calibration range, or a file larger than the per-file budget.
   - `NoCandidatePassed` — emit the best `Partial` output with a report. This is an informational flag, not a blocking prompt.
5. **Budget deferrals.** Track coverage (share of files finished with no human input) as a first-class metric alongside recovery. Set a product target (e.g., at least 90% of REPDF-like files fully autonomous) and trade τ against it using the risk–coverage curve.

---

## Q4. What metrics do document-recovery, OCR and PDF-extraction benchmarks use, and is the planned Dice-style word metric sound?

### Takeaway
The field uses edit-distance families (CER/WER, normalized edit distance), structure metrics (TEDS for tables, CDM for formulas), reading-order edit distance, and increasingly unit-test "facts" (olmOCR-Bench) and faithfulness/hallucination dimensions (ParseBench 2026). PDFPundit's `2·matched/(len_orig+len_repaired)` over a Myers diff is mathematically the LCS-based F1 (ROUGE-L-F1-equivalent). It is order-aware and duplicate-safe, but it:
- conflates missing text with fabricated text;
- penalizes page or reading-order shifts as content loss;
- is undefined for unsegmented scripts like Chinese;
- shares an extractor bias with the thing being measured;
- is not the metric REPDF reports.

REPDF reports word recall against OCR of the rendered pages, so PDFPundit's numbers cannot be compared to REPDF's baselines as planned.

### Cited Findings
- **REPDF's metric.** "The text recovery rate is calculated as the percentage of words in the repaired document that exactly match those in the original document, based on the total number of words in the original": `Text Recovery Rate = # correctly recovered words / # words in original × 100`. Crucially, "the OCR results of the original PDF files were used as ground truth and compared with those of the recovered PDF files". Images get two metrics: image recovery (average 94.75%) and rendering rate (92%) — [REPDF 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).
- **REPDF's corpus design.** 1,000 files across ten corruption scenarios, multiple languages, and two creation methods ("Save As" and "Print to PDF"). "Each document comprises six pages, with identical content presented in a different language on each page. Two of the six pages include simple images" — [REPDF 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).
- **CER/WER** are Levenshtein-based: "the minimum number of character substitutions, deletions, or insertions required to transform one text sequence into another", divided by reference length — [CER/WER explainer, 2025](https://munirakholdorova.github.io/portfolio/2025/10/21/Evaluate-OCR-Output-Quality-with-Character-Error-Rate-(CER)-and-Word-Error-Rate-(WER)-blog.html); [MATLAB evaluateOCR](https://www.mathworks.com/help/vision/ref/evaluateocr.html).
- **OmniDocBench (CVPR 2025):**
  - Pure text: normalized edit distance, "averaging these metrics at the sample level".
  - Tables: TEDS plus normalized edit distance, after converting to HTML.
  - Formulas: CDM, normalized edit distance, BLEU.
  - Reading order: normalized edit distance over text components only.
  - An "Adjacency Search Match" merges and splits paragraphs so that paragraph segmentation does not distort scores.
  - Headers, footers, page numbers and some captions are "ignored" because of inconsistent output conventions.
  - BLEU and edit distance "fail to accurately and fairly assess parsing effectiveness when dealing with markup languages … that allow diverse syntactic expressions".
  - Sources: [CVPR paper](https://openaccess.thecvf.com/content/CVPR2025/papers/Ouyang_OmniDocBench_Benchmarking_Diverse_PDF_Document_Parsing_with_Comprehensive_Annotations_CVPR_2025_paper.pdf); [arXiv 2412.07626](https://arxiv.org/html/2412.07626v1); [GitHub](https://github.com/opendatalab/OmniDocBench).
- **olmOCR-Bench (AllenAI, 2025):** 1,400 PDFs tested with "simple, unambiguous, and machine-checkable" facts, "similar to a unit test". Test classes:
  - text presence (fuzzy matching, optional first-N or last-N character window, case-sensitive by default);
  - text absence (e.g., headers and footers must not appear);
  - natural reading order (relative order of blocks, with ordering between independent articles allowed to vary);
  - table accuracy (cell-neighbour relations);
  - math formula accuracy (KaTeX render match).
  - Sources: [olmOCR-Bench README](https://github.com/allenai/olmocr/blob/main/olmocr/bench/README.md); [arXiv 2502.18443](https://arxiv.org/abs/2502.18443).
- **ParseBench (2026)** includes a Faithfulness dimension that "captures omissions, hallucinations, and reading-order mistakes" — [arXiv 2604.08538](https://arxiv.org/html/2604.08538v2).
- **REPDF per-class results** (text recovery %, Table 2 and Table 5):
  - C1–C3: 100%. C4: near 100%, with a ≤ 2% drop where `/Pages` and `/Page` objects were adjacent. C5: near 100%. C6: above 90%. C7: near 100% (/ToUnicode intact).
  - C8: depends on creation method; "Print to PDF" behaves like C6.
  - C9: 58.77% (Save As) and 62.70% (Print to PDF) in the tool comparison; 49.94–71.69% by language in Table 2.
  - C10: 99.58% (Save As) vs 30.49% (Print to PDF). By language, Print to PDF ranged 0.05%–79.68%.
  - Competing tools reached at most 56.86% on C6–C8, and under 5% for Print-to-PDF files that lost both fonts and Unicode maps.
  - Source: [REPDF 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).

### Inferences
- **Dice over a Myers/LCS alignment equals ROUGE-L F1** (derivation, not from a fetched source). With `P = LCS/len_rep` and `R = LCS/len_orig`, `F1 = 2PR/(P+R) = 2·LCS/(len_orig+len_rep)`. So the planned metric:
  - is order-sensitive, because LCS respects order;
  - is duplicate-safe, because each token is matched at most once;
  - treats omission and hallucination symmetrically. For forensic use this is a weakness: fabricated words (a wrong font mapping that yields dictionary-valid words) are worse than missing words and deserve their own number.
- **Reading-order and page-order errors look like content loss.** If C4 page-tree reconstruction permutes two pages, whole-document LCS drops sharply even though every word was recovered. Reporting an order-insensitive multiset F1 (bag-of-words intersection) next to LCS-F1 separates the two failure modes: the gap between them is the ordering error. Per-page alignment (after checking page count) localizes it.
- **Word-level tokenization breaks for Chinese** (no spaces). The design already notes this for scoring (bigram model). The evaluation metric needs character-level CER or char-F1 for zh, and ideally for all languages as a secondary metric: word-level scoring zeros a whole word for a single wrong glyph.
- **Shared-extractor bias.** Extracting both pristine and repaired text with hayro-interpret means an extractor quirk cancels out. It also means only the text layer is measured. A TemplateAssemble output whose rebuilt /ToUnicode maps codes to the right Unicode, while the substituted glyphs are visually wrong, scores 100%. REPDF's OCR-of-render metric measures the visual layer instead. Both layers matter forensically, and disagreement between them is itself a red flag.
- **Not comparable to REPDF.** REPDF computes recall (denominator = original words only) on OCR text. PDFPundit plans Dice on extracted text. Recall is always ≥ Dice when the repaired text has extra words. Per-class comparisons against the "C6/C8 ≈ 90%" baselines are therefore apples-to-oranges until PDFPundit also computes REPDF's metric.
- **Exact decoded-pixel hashes are brittle for images.** A TemplateAssemble path that re-encodes (e.g., DCT → Flate, colour-space normalization) produces a visually identical but hash-different image. A hash match should count as "exact"; otherwise fall back to SSIM or pHash similarity.

### Gaps
- ~~REPDF names the OCR engine only as an "OCR API".~~ Resolved 2026-10-04: REPDF §5.3 names "the OCR engine of the Google Cloud Document AI API", applied to the original and repaired PDF files. The processor version and request options are not stated. Whether its word matching is order-aware (a bag of words or an alignment) is still not stated, and the dataset repository (`github.com/dfrc-korea/REPDF`) holds only a README and the `corrupted/` and `original/` files, no evaluation code. See [outlined_text_and_ocr.md §4](outlined_text_and_ocr.md).
- No benchmark was found that evaluates PDF repair (as opposed to parsing) with a text-plus-visual metric pair. The pairing proposed here is a synthesis.

### What PDFPundit should do
1. **Report a metric family per file**, all after the same NFC, casefold and whitespace normalization, which should be published:
   - `lcs_f1` — the current metric, renamed for clarity.
   - `word_recall = LCS/len_orig`.
   - `word_precision = LCS/len_rep`. Its complement is the **hallucination rate** `1 − precision`, reported separately.
   - `bag_f1` — multiset intersection, order-free. `order_penalty = bag_f1 − lcs_f1`.
   - `char_f1` or `CER` — mandatory for zh, secondary for all other languages.
   - `page_count_match` and per-page `lcs_f1`, averaged.
2. **REPDF-comparable metric.** In the corpus harness only (external tool, no FFI in the product), OCR pristine and repaired files with Google Cloud Document AI, the engine REPDF used (pinned processor version, PDFs sent directly, native parsing off), with local PaddleOCR as the CI proxy, and compute `ocr_word_recall = matched/len_orig`. See [outlined_text_and_ocr.md §4](outlined_text_and_ocr.md). Report it next to REPDF's Table 2 and Table 5 numbers per class × creation method × language. This is the only defensible comparison to the paper's baselines.
3. **Visual-vs-text consistency check:** flag a file when `text_layer_lcs_f1 − ocr_lcs_f1 > γ`. Learn γ on the tuning split. This catches correct-ToUnicode-but-wrong-glyph outputs, the "local discrepancy" Kuchta et al. say whole-page comparison misses.
4. **olmOCR-style unit facts per fixture and per corpus document.** Generate them automatically from the pristine file:
   - k sampled sentences must be present;
   - U+FFFD and mojibake patterns must be absent;
   - sentence A precedes B within a page;
   - page N contains image hash H.

   Report the pass rate per test class. These are interpretable and robust to normalization quirks.
5. **Images:** report `image_exact` (decoded-pixel hash), `image_similar` (SSIM ≥ 0.98, or pHash Hamming distance ≤ threshold, calibrated on known re-encodes), and REPDF's `rendering_rate` (the image actually appears on the rendered page) as separate numbers.
6. **Aggregate at file level first, then macro-average per stratum** (class × creation method × language page). Also report micro (pooled words). OmniDocBench also averages per sample.

---

## Q5. How should the repaired file itself be validated (structural checkers, multi-renderer open-and-render, render-diff, differential testing)?

### Takeaway
Validation has three layers, each with a different blind spot:
- **Syntax/structure:** qpdf `--check`. It checks syntax only and does not validate content, and its exit codes are not fully reliable.
- **Specification conformance:** the Arlington PDF Model via TestGrammar or veraPDF's Arlington checker.
- **Behaviour:** open and render across several independent renderers, with perceptual render-diff against the pristine file.

Pixel-exact diffs are too strict. CW-SSIM and pHash worked best in the one large PDF-reader study. Whole-page similarity misses localized font errors.

### Cited Findings
- **qpdf `--check`** "Check[s] the structure of the PDF file as well as a number of other aspects … Note that qpdf does not perform any validation of the actual PDF page content or semantic correctness … It merely checks that the PDF file is syntactically valid". Exit codes: 0 = no errors or warnings; 2 = errors; 3 = warnings (unless `--warning-exit-0`) — [qpdf(1) man page](https://manpages.debian.org/testing/qpdf/qpdf.1.en.html).
- **qpdf exit codes have been inconsistent in practice.** A long-running issue reported exit code 2 with only a WARNING printed (linearization hint table), still present in 8.4.2 per a 2019 comment — [qpdf issue #50](https://github.com/qpdf/qpdf/issues/50).
- **qpdf has a separate "damaged" parsing mode** with its own limits (`--parser-max-container-size-damaged`, default 5,000, "when the PDF document's xref table is damaged"). A clean `--check` on PDFPundit output should therefore show no damaged-mode recovery — [qpdf(1)](https://manpages.debian.org/testing/qpdf/qpdf.1.en.html).
- **Arlington PDF Model:** a "vendor- and implementation-independent specification-derived, machine-readable model of PDF". Implementations include the TestGrammar C++ PoC, veraPDF's Arlington checker, and PDFix's checker, available as a web service, a Docker image and a CLI — [pdf-association/arlington-pdf-model](https://github.com/pdf-association/arlington-pdf-model). It was developed for DARPA SafeDocs and covers "all ISO-standardized and commonly encountered PDF objects", including data-integrity relationships. veraPDF's Arlington checker ships as GUI, CLI, Docker and REST — [PDF Association](https://pdfa.org/verapdfs-arlington-pdf-model-checker-released).
- **TestGrammar semantics:**
  - Exit codes: 0 = processed; −1/255 = fatal error ("a corrupted PDF where the trailer cannot be located"); 134 = assertion; 139 = segfault.
  - Every run should end with "END".
  - Warnings include "X preamble junk bytes detected before '%PDF-x.y' header", "X postamble junk bytes … after '%%EOF'", and "XRefStream is present in file with header %PDF-x.y and Document Catalog Version of PDF a.b".
  - It "does not track context"; inheritance is checked only for Required keys.
  - Source: [TestGrammar README](https://github.com/pdf-association/arlington-pdf-model/blob/master/TestGrammar/README.md).
- **Kuchta et al. (EMSE 2018), cross-reader differential testing:**
  - Setup: 11 readers, over 230K Govdocs1 PDFs. Pipeline: load in every reader, record crashes and error or warning messages, cluster them, then compare renders.
  - "A pixel-by-pixel comparison … is too strict, leading to many spurious alarms". They used CW-SSIM, with an empirically chosen threshold, against a base reader.
  - In an ROC comparison of AE, MAE, MSE, RMSE, NCC, PSNR, pHash and CW-SSIM, "CW-SSIM and PHASH are the best performing".
  - Reaching an 80% true-positive rate required accepting a 50% false-positive rate.
  - 73% of false positives came from near-blank pages with little structure.
  - Source: [Kuchta et al. 2018](http://srg.doc.ic.ac.uk/files/papers/pdf-emse-18.pdf).
- **Earlier document-recovery work** "uses image similarity to assess the quality of repaired input (Long et al. 2012)" — [Kuchta et al. 2018](https://link.springer.com/article/10.1007/s10664-018-9600-2).
- **SSIM (Wang et al., 2004)** measures structural-information degradation and is computed with a sliding window. MS-SSIM (Wang, Simoncelli, Bovik 2003) adds multi-scale weighting because "the right scale depends on viewing conditions" — [MS-SSIM paper, ResearchGate](https://www.researchgate.net/publication/4071876_Multiscale_structural_similarity_for_image_quality_assessment).

### Inferences
- PDFPundit's in-process gates (strict lopdf reload plus a hayro render) test only two parsers, and neither has the ecosystem coverage of Acrobat, pdf.js, PDFium, MuPDF or Poppler. Given the 13.5% cross-reader inconsistency base rate on ordinary government PDFs, some outputs that hayro and lopdf tolerate will misrender elsewhere. The harness must test other renderers.
- **The no-FFI rule applies to the shipped binary, not the CI harness.** External validators (qpdf, veraPDF-Arlington in Docker, `mutool draw`, `pdftoppm`, pdf.js via Node, a PDFium CLI) can run as subprocesses in `src/bin/corpus.rs` or in CI scripts. (License obligations of each tool when used only as an external CLI were not researched; check them before adopting.)
- **Font substitution legitimately changes pixels.** For C6–C8, a render-diff against pristine will show low SSIM even when the text is right, because the "sister" font differs from the original. Class-aware thresholds are needed:
  - C1–C5, C9 and C10 (Resave path, original fonts): expect near-identical renders.
  - C6–C8: rely on OCR-text agreement (Q4) rather than pixel similarity.
- **Near-blank pages defeat SSIM** (Kuchta's main false-positive cause). A blank-page detector should short-circuit: blank in pristine and blank in output is a pass; non-blank in pristine and blank in output is a hard failure.

### Gaps
- No published study was found comparing veraPDF-Arlington and TestGrammar findings on repaired files specifically, or measuring how often repair tools' outputs fail Arlington checks.
- The exact CW-SSIM threshold Kuchta et al. chose was not in the fetched excerpts.
- Performance of pure-Rust SSIM/pHash crates was not researched.

### What PDFPundit should do
1. **In-engine (shipped, pure Rust):**
   - keep strict lopdf reload, the hayro render of all pages, and re-diagnose;
   - add the V0/V1 gates from Q2: page count, blank-page, retention;
   - add a lightweight self-lint for the Arlington warnings PDFPundit's own emitter could trigger: header version vs catalog `/Version` vs xref-stream presence, junk before the header or after `%%EOF`.
2. **Corpus/CI oracle battery (external tools, subprocesses only):**
   - **Structure:** run `qpdf --check`. Pass means the output contains no "WARNING"/"ERROR" lines and no damaged-file recovery messages. Parse stdout instead of trusting the exit code (per issue #50).
   - **Spec conformance:** run the veraPDF Arlington checker (Docker) on all outputs nightly and on a sample per PR. Track error counts per class and fail on any new error type.
   - **Differential rendering:** render every output page at 100 DPI with at least 3 independent engines (hayro, MuPDF `mutool draw`, Poppler `pdftoppm`; optionally PDFium and pdf.js). Pass means every engine opens and renders without error, and pairwise pHash distance or CW-SSIM between engines exceeds a threshold picked on the ROC of labeled known-good and known-bad outputs. A disagreement is a flag for human triage, not an automatic failure, because some disagreements are reader bugs.
   - **Render-diff vs pristine (same engine, same DPI):**
     - per page, compute SSIM plus pHash;
     - for Resave-path classes also compute SSIM on text-box crops, to catch local font errors;
     - use class-aware thresholds, learned on the tuning split;
     - for C6–C8 use OCR-text agreement instead of pixels.
3. **Log every oracle's verdict per file in `corpus_results.csv`.** Compute the plausible-but-wrong rate: all in-engine gates pass, but some external oracle fails or true recovery is below target.

---

## Q6. Experimental design: held-out splits so tuning does not overfit REPDF, stratification, confidence intervals, and reporting against REPDF's per-class baselines

### Takeaway
Tuning a few weights and thresholds on the same 1,000 files that measure the result produces optimistically biased estimates. Cawley & Talbot (2010) show the bias can match the size of real differences between methods. PDFPundit needs:
- **Group-aware splits:** the same source document appears under many corruption classes.
- **Nested tuning** inside a sealed test split.
- **A held-out corruption model**, to catch overfitting to REPDF's specific corruptors.
- **Cluster-bootstrap CIs** for mean recovery and Wilson/Jeffreys intervals for proportions.
- **Paired tests** for version-to-version comparisons.
- **Non-inferiority reporting** against REPDF's point estimates, computed with REPDF's own metric.

### Cited Findings
- **Cawley & Talbot (JMLR 2010):** "a non-negligible variance introduces the potential for over-fitting in model selection". "The effects of this form of over-fitting are often of comparable magnitude to differences in performance between learning algorithms". "Some common performance evaluation practices are susceptible to a form of selection bias". The findings "apply to any model selection practice involving the optimisation of a model selection criterion evaluated over a finite sample of data" — [JMLR v11](https://jmlr.org/papers/v11/cawley10a.html).
- **ASlib (2016)** tuned hyperparameters "using random search with 250 iterations and a nested cross validation (with three internal folds) to ensure unbiased performance results". It notes that non-nested CV "may result in overconfident performance estimates" — [ASlib](https://ar5iv.labs.arxiv.org/html/1506.02465).
- **Dror et al. (ACL 2018),** "The Hitchhiker's Guide to Testing Statistical Significance in NLP", covers choosing significance tests by task, setup and measure — [ACL Anthology P18-1128](https://aclanthology.org/P18-1128). Riezler & Maxwell (2005) found approximate randomization gives more conservative p-values than bootstrap tests — [Semantic Scholar listing of Dror et al. references](https://www.semanticscholar.org/paper/The-Hitchhiker%E2%80%99s-Guide-to-Testing-Statistical-in-Dror-Baumer/d10df96b3fb0ab5c6b1d0cc22c7400d0acccc3cc).
- **Brown, Cai & DasGupta (Statistical Science 2001):** the Wald interval's coverage is "chaotic", and textbook safety rules "cannot be trusted". They "recommend the Wilson interval or the equal-tailed Jeffreys prior interval for small n [n < 40] and the interval suggested in Agresti and Coull for larger n". The Clopper-Pearson interval is "very conservative" — [Project Euclid](https://projecteuclid.org/journals/statistical-science/volume-16/issue-2/Interval-Estimation-for-a-Binomial-Proportion/10.1214/ss/1009213286.full); [tech report](https://www.stat.purdue.edu/~dasgupta/publications/tr99-19.pdf).
- **Cluster bootstrap** is the standard tool for clustered or hierarchical data: resample whole clusters, and report the number of clusters and the cluster-to-naive SE ratio — [ClusterBootstrap R package](https://cran.r-universe.dev/ClusterBootstrap/doc/manual.html); [MetricGate docs](https://metricgate.com/docs/cluster-bootstrap-hierarchical/).
- **REPDF corpus structure and reporting:** ten corruption scenarios × two creation methods × multiple languages. Languages are pages within a file (six pages, same content, one language per page). Results are reported per corruption type × creation method × language, with a separate comparison table against iLovePDF, Wondershare Repairit and PDF24 — [REPDF 2026](https://dfrws.org/wp-content/uploads/2026/03/REPDF-Repairing-corrupted-PDF-files-through-f_2026_Forensic-Science-Interna.pdf).
- **A real-world corpus exists:** Govdocs1 (Garfinkel et al. 2009), 230K+ PDFs mined from US government websites. Kuchta et al. used it for PDF-reader differential testing — [Kuchta et al. 2018](https://link.springer.com/article/10.1007/s10664-018-9600-2).

### Inferences
- **Leakage risk.** With ten corruption classes and two creation methods over 1,000 files, each underlying source document probably appears in many corrupted variants (1,000/20 = 50 would fit, but this is unverified). A random file-level split puts the same text, fonts and page layout in both tuning and test. That inflates scores for font inference, dictionary weights and thresholds, which depend on the text. Splits must group by source document (GroupKFold), stratified by class × creation method.
- **Language is page-level in REPDF.** Per-language metrics must be computed per page. They cannot be split at file level, and language-specific calibration therefore cannot hold out whole files.
- **Corruptor overfitting.** C9 (exactly one byte flipped) and C10 (fixed truncation fraction in the fixtures) are generator-specific. A second corruptor family, never used during development, gives a held-out corruption model split:
  - multi-byte bursts;
  - two-byte flips;
  - truncation at 30–95%;
  - xref offsets shifted rather than removed;
  - C4 variants that damage `/Kids` partially.
- **Recovery is a bounded continuous per-file value, not a binomial count,** so Wilson intervals do not apply to mean recovery. Use the cluster bootstrap (resampling source documents). Wilson or Jeffreys intervals do apply to proportions such as "% of files fully recovered", "auto-accept error rate", "coverage" and "plausible-but-wrong rate".
- **The CI gate needs no significance test between runs.** The engine is deterministic, so on fixed files there is no run-to-run sampling noise. The planned PR gate (`≥ baseline − 2%` on ~30 files) is a regression detector and should be per-file (no file drops by more than ε), not aggregate-versus-paper. Claims about generalization belong only to the sealed test split, with CIs.
- **REPDF publishes aggregates only** (no per-file results), so a paired test against REPDF is impossible. The defensible claim is non-inferiority: the lower bound of PDFPundit's 95% CI (REPDF metric, same stratum) ≥ REPDF's point estimate − margin.

### Gaps
- The exact count of source documents in the REPDF corpus, and how files map to them, was not verified. Check `github.com/dfrc-korea/REPDF` (`original/` vs `corrupted/`).
- Whether REPDF's per-class numbers carry their own uncertainty (they are single runs on 100 files per class, if the 1,000 files are evenly split) is not reported in the excerpts read.
- No source was found on a standard "naturally corrupted PDF" benchmark with ground truth. Real-world damaged files usually lack pristine originals, which limits evaluation to oracle-only metrics (Q5) for that set.

### What PDFPundit should do
1. **Splits** (fixed seed, committed as `corpus/splits.json`):
   - Group by source document. Stratify by class × creation method.
   - Partition: 60% dev (tuning), 20% calibration (for Q3's Platt fit and τ selection only), 20% sealed test.
   - Read the sealed test once per release. Commit results with the release tag.
   - Inside dev, use 5-fold grouped CV for weight search (`W_WORD`, `W_PREFIX`, `P_*`, verification-rank weights, δ, γ), following ASlib's nested scheme.
2. **Held-out corruption split:** implement a `corruptors_v2` set (multi-byte bursts, variable truncation, partial `/Kids` damage, xref offset shifts). Run it only at release time. Report the gap between REPDF-corruptor and v2-corruptor recovery as an overfitting indicator.
3. **Real-world sanity set:**
   - A few hundred Govdocs1 PDFs, corrupted with both corruptor families, which gives ground truth.
   - Separately, any genuinely damaged files the user collects, scored with Q5 oracles only.
4. **Statistics in `src/bin/corpus.rs`:**
   - Mean recovery per stratum with a 95% cluster-bootstrap percentile CI (10,000 resamples of source documents).
   - Proportions with Wilson intervals (n < 40: Wilson or Jeffreys; n ≥ 40: Wilson or Agresti-Coull).
   - Version-vs-version comparisons with a paired bootstrap or approximate randomization over the same files. Report effect size and CI, not only p.
   - With many strata, control multiplicity (e.g., Holm) when claiming "improved in class X".
5. **Report table per release**, rows = C1…C10 × {Save As, Print to PDF}, columns:
   - `ocr_word_recall` (REPDF metric) [95% CI], next to the REPDF baseline and a non-inferiority verdict at margin 2 points;
   - `lcs_f1`, `word_precision` (hallucination), `bag_f1`, `char_f1` (zh);
   - image exact / similar / rendering rate;
   - coverage (autonomous %), selective risk of auto-accepted outputs [Wilson CI];
   - plausible-but-wrong rate;
   - SBS→VBS gap closed by the selector;
   - per-language breakdown (page-level).
6. **PR gate:** per-file non-regression on the 30 cached files (no file's `lcs_f1` or `ocr_word_recall` drops by more than 0.01; no new hard-gate failures), plus the unchanged fixture suite. Aggregate-vs-paper comparisons run nightly (`corpus-full`) and never gate PRs on noisy small samples.
