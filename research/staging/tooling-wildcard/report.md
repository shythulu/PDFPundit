# Stage 2 tooling report: cross-field wildcard (agent D, tooling-wildcard)

Relaunched on 2026-10-05 after the container restart; salvaged the 2026-09-27 experiments, then finished the brief. This is a staging report: nothing here is merged into the main registries.

**Records**
- `tooling/ledger.jsonl`: 27 TOOL entries (TOOL-450..476).
- `observations/observations.jsonl`: 9 OBS entries (OBS-0900..0908).
- `search-log.jsonl`: 3 SRCH entries (SRCH-0900..0902), all paid.
- No CLM, SRC, GAP, HYP or DMG records.
- Scripts and results are in `experiments/` and `experiments/results/`. Each input manifest there was recomputed with sha256 on 2026-10-05.
- `kb_validate.py --staging research/staging/tooling-wildcard --write --online` reports **0 errors**. Its 7 warnings all concern other agents' records.

## 1. Upsets, biggest first

Every item says which plan assumption it changes. "Plan" means the charter, `bleeding-edge-repair-10k.md` and the gap register. PR #16 is cross-checked in §8.

### 1. Encoder replay plus Adler-32 does not certify a unique correction

**Assumption changed:** charter grade 3, and GAP-154's premise that a wrong correction almost never passes replay.

**Evidence, OBS-0903:** one Save As ToUnicode stream has **3 single-byte candidates that pass inflate, Adler-32 and stock-zlib replay together**:
- the true byte;
- an Adler collision at byte 656;
- a one-byte rewrite of the Adler trailer.

The windowed search saw only the trailer candidate and would have reported it as "unique".

**Evidence, OBS-0904:**
- 87 of 820 damaged Save As streams (10.6%) are themselves canonical zlib output. Re-compressing their wrong plaintext reproduces the damaged body exactly, so only the trailer shows the damage.
- Rewriting the trailer then passes all three oracles. The rewrite needs 1 byte in 3 streams, 2 bytes in 1, 3 bytes in 54 and 4 bytes in 29.

**What follows:**
- Grade 3 needs three things:
  - exhaustive candidate enumeration that includes trailer edits;
  - an explicit damage prior that prefers body edits;
  - a content-grammar check, since both wrong candidates in OBS-0903 give malformed CMap lines.
- Replay plus a checksum alone is not enough.

### 2. "Identical under two dummy dictionaries" (charter grade 2) certifies garbage unless the restart point is confirmed

**Assumption changed:** charter grade 2 as written, and CLM-0337's use.

**Evidence, OBS-0900:**
- At the 807 true block boundaries of 300 multi-block streams, dictionary-invariant bytes were always right: precision 1.0, median recall 0.82.
- A blind bit-offset scan found **51,120 false starts at 746 of the 807 restarts**. These emit **8,521,181 bytes that are identical under both dictionaries and wrong**.
- Requiring the last block to end exactly at the Adler trailer rejects all 51,120 false starts and keeps 486 of the 807 true starts.

**What follows:** grade 2 must require a confirmed restart, either by trailer alignment or by a following block that decodes.

### 3. Pure Rust can host the replay oracle, through preflate-rs's predictor and a producer profile, not through a Rust zlib

**Assumptions changed:**
- GAP-154's 54% coverage figure;
- the implicit conflict between "pure Rust, no compiled C" (charter §5) and replay.

**Evidence:**
- **Stock zlib.** Once memLevel is swept, stock zlib 1.3 reproduces **2,074 of 2,074** Save As streams (OBS-0901). The default memLevel alone gives 1,124, and print gives 1 of 1,264.
- **General-purpose Rust encoders** don't reproduce it: zlib-rs 0.6.8 matched 0 of 400, and miniz_oxide 0.9.1 matched 5 of 400 (OBS-0902).
- **preflate-rs 0.7.6** (Microsoft, Apache-2.0, `unsafe` forbidden). Its token predictor, given zlib level-6 parameters and *zero* corrections, reproduces **395 of 400** of the same streams byte-for-byte (OBS-0908, TOOL-476).
  - It needs the parameters (memLevel 7 or 8) from a producer profile. With per-stream estimation it gets only 47 of 399.
- **Print to PDF is out of reach either way:** 0 of 400. Preflate fingerprints that encoder as zlib-like but not zlib: it allows matches to the stream start and very far matches.

**Caveats:**
- Forced parameters made preflate-rs panic 14 times.
- The probe mirrors a private struct, so a fork or an upstream API is needed.

### 4. A producer profile, not a generic PDF model, makes REPDF damage exactly solvable

**Assumption changed:** the plan's heuristic offset repair, and its treatment of "Print to PDF" as one opaque class.

**Evidence:**
- **memLevel 7 or 8 for Save As** (OBS-0901, OBS-0908).
- **Xref bias** (OBS-0905): Print to PDF points every xref offset one byte early (bias +1); Save As has bias 0.
- **xref and object_header restoration** (OBS-0905): a closed-form constraint model restores **100 of 100 REPDF object_header files byte-for-byte**. It needs no SMT solver; z3 agrees with plain enumeration on all 100. It also needs the file's EOL style to fill the gap.
- **Font identity survives** (OBS-0907).
- **One font instance per page** (OBS-0907).

**Transfer:** untrunc's reference-file repair of MP4 is the analogue from another format (TOOL-475; idea only, GPL).

### 5. Font identity is not lost in "Print to PDF" files

**Assumption changed:** PR #16's font note ("ignore the name and rely on widths plus decoding"), its design change 3, and the plan's need for name-free font identification (GAP-003) in most cases.

**Evidence, OBS-0907:**
- BaseFont is always `CIDFont+Fn`.
- But **889 of 889** embedded TrueType programs keep the real family in the `name` table: 40 families, such as Cambria and Rubik Medium.
- 879 also keep a usable `cmap`.

**What follows:** GAP-003 is needed only where no FontFile2 survives, as in REPDF C7 and C8.

### 6. Glyph-code decipherment works on long Latin slots, but it fabricates digits and short slots

**Assumption changed:** RQ3's "without a language prior inventing it" becomes measurable. Decipherment (GAP-205) is at best grade 4.

**Evidence, OBS-0906:** a character 4-gram model with simulated annealing, given no ToUnicode, no font program and no widths, gets:
- **76.6%** of 192,680 code occurrences overall;
- **0.86** on slots of 1,024–4,096 codes;
- **0.44–0.49** on slots under 256 codes;
- **6.5%** of digits.

Abstaining on low-margin codes keeps 86.8% of tokens at 86.3% accuracy.

**What follows:** digits need non-linguistic evidence, such as PR #16's widths.

### Seeds that did not overturn anything (Hold or Assess)
- **SMT (cvc5, OR-Tools):** not needed for the cases tested (item 4).
- **CDR:** PDF-CDR publishes no CDR or reconstruction code; Dangerzone rasterises and is AGPL.
- **Kaitai:** has no PDF specification, and its compiler is GPL.
- **LLM/VLM repair:** stays Assess until fabrication is measured, given item 6.

## 2. Ledger summary

| ID | Name | Verdict | Verified here |
|---|---|---|---|
| TOOL-450 | Stock zlib 1.x deflate as the replay reference (Zlib) | Adopt | pass |
| TOOL-451 | zlib-rs / libz-rs-sys 0.6.8 (Zlib) | Hold | pass |
| TOOL-452 | miniz_oxide 0.9.1 (MIT/Zlib/Apache-2.0) | Hold | pass |
| TOOL-453 | zlib-ng 2.2.5 via python-zlib-ng 1.0.0 | Assess | pass |
| TOOL-454 | ISA-L via python-isal 1.8.0 | Assess | pass |
| TOOL-455 | libdeflate via `deflate` 0.9.0 | Assess | pass |
| TOOL-456 | Two-dummy-dictionary resync (Adler's method) | Trial | pass |
| TOOL-457 | Replay-guided single-byte corrector (prototype) | Trial | pass |
| TOOL-458 | z3-solver 5.1.0.0 (MIT) | Assess | pass |
| TOOL-459 | cvc5 1.4.1 | Hold | untested |
| TOOL-460 | OR-Tools CP-SAT 9.15.6755 | Hold | untested |
| TOOL-461 | Closed-form xref deletion solver (prototype) | Trial | pass |
| TOOL-462 | Simulated-annealing glyph-code decipherer (prototype) | Trial | pass |
| TOOL-463 | hmmlearn 0.3.3 | Assess | untested |
| TOOL-464 | fontTools 4.66.1 (MIT) | Adopt | pass |
| TOOL-465 | fontations read-fonts 0.45 / skrifa 0.48 | Trial | untested |
| TOOL-466 | OpenType Sanitizer v9.3.0 | Assess | untested |
| TOOL-467 | llama.cpp b11430 / llama-cpp-python 0.3.36 | Assess | untested |
| TOOL-468 | Dangerzone v0.11.0 (**AGPL-3.0**) | Hold | untested |
| TOOL-469 | PDF-CDR (Dubin) | Hold | untested |
| TOOL-470 | MAPIE 1.5.0 (conformal risk control) | Trial | untested |
| TOOL-471 | DaeDaLus v1.0 + its PDF DDL specifications | Assess | untested |
| TOOL-472 | Kaitai Struct 0.11 (compiler **GPL-3.0**) | Hold | untested |
| TOOL-473 | grmtools lrpar 0.15.0 | Assess | untested |
| TOOL-474 | PolyFile 0.6.0 | Assess | untested |
| TOOL-475 | untrunc (**GPL-2.0**; idea transfer only) | Assess | untested |
| TOOL-476 | preflate-rs 0.7.6 predictor as a zero-correction replay oracle | Trial | pass |

**On the "verified here" column:**
- "pass" means the tool ran in this container on REPDF input. Exact commands and output lines are in each entry's `verification_evidence`.
- Two "pass" entries rest on 2026-09-27 results files and were not re-run: TOOL-456 (dict_resync) and the encoder-sweep part of TOOL-453..455.
- The tools TOOL-453..455 were re-run on 2026-10-05 on a smaller sample.

**Licences to flag:**
- AGPL: Dangerzone.
- GPL: the Kaitai compiler and untrunc.
- cvc5 has optional GPL builds.

## 3. Top recommendations per need

| Need | Recommendation | Evidence |
|---|---|---|
| Replay oracle, research side (GAP-154) | Stock zlib 1.x with a memLevel 1–9 × strategy sweep | TOOL-450, OBS-0901 |
| Replay oracle, product (pure Rust) | preflate-rs predictor with producer-profile parameters; fork or add an API, and isolate panics. Do not use zlib-rs or miniz_oxide | TOOL-476, OBS-0908 |
| Single-byte Flate correction (GAP-150/153) | PR #16's ladder (checkpointed search plus grammar localiser), with replay as one arm. Enumerate every candidate, trailer included, and require a grammar check | TOOL-457, OBS-0903/0904 |
| Multi-block resync (GAP-151/152) | Two-dummy-dictionary decode **only** at a restart confirmed by trailer alignment | TOOL-456, OBS-0900 |
| Offsets, xref, deleted headers (DMG-005) | Closed-form constraint enumeration plus a producer bias and EOL profile. z3 for research prototypes only | TOOL-461, TOOL-458, OBS-0905 |
| Font identity (GAP-003) | Read the FontFile2 `name`/`cmap` first: fontTools for research, read-fonts/skrifa in Rust. Width or outline fingerprints only for C7/C8 | TOOL-464/465, OBS-0907 |
| Glyph decipherment (GAP-205) | Simulated-annealing decipherer as a grade-4 inference with per-code margin abstention. Add widths and per-script LMs | TOOL-462, OBS-0906 |
| Selection with abstention (GAP-155) | Conformal risk control (MAPIE) on base-document-grouped calibration data, as an alternative to PR #16's Platt plus threshold | TOOL-470 |
| Grammar check for grade 3 | Port the relevant DaeDaLus PDF DDL rules (CMap, ContentStream) to Rust. Consider grmtools error recovery as a token-level repair generator | TOOL-471, TOOL-473 |
| Font-stream independent check | OTS (research side), next to PR #16's TrueType checksum localiser | TOOL-466 |
| LLM/VLM repair (GAP-201) | Only behind a fabrication harness (planted tokens, abstention). Nothing adopted | TOOL-467 |

## 4. API-account list (for the chair's single ask)

**No ledger entry of mine needs a keyed account.** One optional account:

| Service | Unlocks | Free quota (source, fetched) | Depends on it |
|---|---|---|---|
| Hugging Face (free user account plus access token) | Downloads of gated model weights, e.g. Llama 3.1 8B as used by CPR, after accepting the licence. Higher hub rate limits than anonymous use. Ungated GGUF models need no account | Per 5-minute window: anonymous (per IP) 500 API, 3,000 resolver and 100 page requests; free user 1,000 / 5,000 / 200. Table labelled "September '25"; the values carry an asterisk whose footnote was not read. https://huggingface.co/docs/hub/rate-limits, fetched 2026-10-05 | TOOL-467; GAP-201; GAP-205; agent B's CPR baseline |

OpenAlex and Semantic Scholar keys belong to agent A's list. Parallel Search is already provisioned.

**Host needs:**
- Rust 1.94 and cargo with crates.io access. This worked here.
- For research-side replay only: Python's stock zlib 1.3.
- The venv I used is recorded in `experiments/results/venv-requirements.txt`: z3-solver, fontTools, pypdf, zlib-ng, isal, deflate and numpy. The venv itself was deleted.

## 5. What I did NOT check

**LLM, decipherment and selection**
- No LLM or VLM was run, so fabrication is unmeasured (TOOL-467 is from registry and git metadata).
- The HMM/EM decipherment cross-check (hmmlearn) was not run.
- MAPIE was not trialled.

**Solvers, CDR and grammar frameworks**
- cvc5 and OR-Tools were not run.
- Dangerzone was not run (no container run).
- None of Kaitai, Hammer or DaeDaLus was built (DaeDaLus needs GHC).
- Parsley: no repository found.
- RepairFuzz and Gmutator (found by SRCH-0902): no code URL for RepairFuzz, and neither was read.
- ddmax was not checked.

**Fonts**
- read-fonts/skrifa and OTS were not run.
- Cross-page font-instance redundancy is an idea only. It does not help C7 or C8.

**preflate-rs**
- Why the 4 memLevel-7 streams miss.
- The root cause of the 14 panics.
- Whether preflate's misprediction bursts can localise damage in Print to PDF streams.
- Whether zero-correction replay keeps working on a damaged stream's undamaged prefix.

**Other**
- Whether zlib 1.3.2 output equals the tested 1.3.
- Multi-edit xref damage, and xref streams with predictors (the solver refuses both).
- Transfer from SQLite, JPEG, ZIP and Office repair tools beyond the Kaitai specification listing.
- Probabilistic parsing.
- E-APR itself.
- Reproducing PR #16's localisers. I cite them and did not re-run them.

## 6. Paid calls used

**3 of the 7-call cap:**
- SRCH-0900 (2026-09-27): CDR search;
- SRCH-0901 (2026-10-05): decipherment and width search;
- SRCH-0902 (2026-10-05): SMT, grammar and LLM file repair, and PDF-CDR.

All other lookups used free registry APIs, `git ls-remote` and raw.githubusercontent.com. The project-wide counter reads 25 of 80.

## 7. Decisions for the chair

1. **Reword charter grade 2:** "identical under two dummy dictionaries *from a restart confirmed by trailer alignment or a following decodable block*" (OBS-0900).
2. **Reword charter grade 3:**
   - require exhaustive candidate enumeration, trailer edits included;
   - require an explicit body-over-trailer prior;
   - require a content-grammar check where a grammar exists;
   - state that replay plus checksum is not uniqueness (OBS-0903/0904).
3. **Pure Rust:** keep the product decision.
   - Fund a Trial of preflate-rs's predictor as the replay oracle (TOOL-476): a fork or upstream API for zero-correction replay, panic isolation, and the memLevel-7 misses.
   - Allow stock C zlib in the research harness as the reference.
   - Drop "port zlib 1.x to Rust" from P4 unless the Trial fails.
4. **Make a producer profile a first-class model input**: replay parameters, xref bias, EOL style, per-page font instances, and the encoder fingerprint. Add more producers (GAP-002) before generalising.
5. **Amend PR #16's "ignore `CIDFont+Fn` names" advice**: read the FontFile2 `name` table first, and scope GAP-003 to C7/C8 (OBS-0907).
6. **Treat decipherment output as grade 4** with per-code abstention. Digits need width or other non-linguistic evidence (OBS-0906).
7. **Put SMT solvers, CDR toolchains and Kaitai on Hold for P4.** Keep z3 for research prototypes only.
8. **Reconcile with agent B (tooling-engines):**
   - B's zlib replay entry swept memLevel {8, 9} only (per its `deflate_replay.py` docstring) and reports 68% Save As coverage. Adding memLevel 7 gives 100% (OBS-0901; 175 of 400 sampled streams need it).
   - B's preflate-rs entry (round trip with corrections) and my TOOL-476 (zero-correction replay) cover the same crate in two uses. Merge them.
9. **LLM/VLM:** decide whether to fund a fabrication-measurement trial. It needs model downloads, and the Hugging Face token above if the model is gated.

## 8. PR #16 cross-check

| PR #16 statement | My result | Relation |
|---|---|---|
| Acceptance must require Adler-32, break ties with grammar, else mark Partial; "Another [false accept] came from damage to the Adler trailer itself" | OBS-0903: 3 candidates pass inflate, Adler and replay; wrong ones fail a CMap grammar. OBS-0904: 10.6% of damaged Save As bodies are canonical zlib, so trailer rewrites pass every oracle | **Corroborates and extends.** Replay does not close the trailer hole |
| Checkpointed 255-value search fixes 218/221 inflate-error streams; grammar localiser plus search fixes 324/343 | Replay localisation fixed 196/266 Save As uniquely; Adler-algebra localisation found 21/317 | **Corroborates** that PR #16's localisers are better. Mine are a replay arm only |
| Block-boundary resync is of little use because most streams are a single block | OBS-0900: where boundaries exist, dictionary invariance is exact only at confirmed restarts; a blind scan certifies 8.5M wrong bytes | **Extends** |
| "No published SAT/constraint-solver or LLM-guided repair of DEFLATE was found" (deflate note) | SRCH-0902 found no SMT- or LLM-based file-repair tool either | **Corroborates** |
| lopdf ±64-byte offset correction; qpdf header rebase; qpdf #341 off-by-one startxref | OBS-0905: exact closed-form restoration of 100/100 object_header files; Print to PDF xref bias +1 | **Extends**: exact rather than heuristic |
| "Print to PDF" names are only `CIDFont+F1..F22`; "ignore the name and rely on widths plus decoding" | OBS-0907: 889/889 embedded fonts keep their real family in the `name` table | **Corroborates** the BaseFont fact; **contradicts** the advice wherever a FontFile2 survives |
| Width vectors fingerprint fonts (widths.py) | Not used by my decipherer. Digits fail without them (OBS-0906) | **Complementary**: combine them |
| mzbench uses miniz_oxide (0.8.9) to decode | OBS-0902: miniz_oxide cannot *encode* for replay (5/400). Decoding is unaffected | **No conflict** |
| About 39% of damaged streams byte-exact (demonstrated) | Replay adds certification for Save As only (print 1/1,264; preflate 0/400) and cannot by itself raise uniqueness | **Bounds** what replay adds |

PR #16 is cited here as a source. These rows are not KB claims; agent E is ingesting the report.
