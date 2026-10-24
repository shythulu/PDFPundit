# PDFPundit

PDFPundit is a terminal app that diagnoses and repairs damaged PDF files for forensic use. It never alters the input, and it reports how sure it is of every repair.

## Input and the UI

**Cat**:
The app's face and its only entry point. Files are fed to it by drop, paste or the browse picker.
_Avoid_: CLI, front end

**Drop gate**:
The admission check on a fed file: `.pdf` extension, readable, within size limits, not a UNC path.

**Full layout / widget / one-line face**:
The three presentations, at 112×38, at 32×16, and below 32×16. The one-line face accepts no input.

## Damage

**Corruption class**:
One of REPDF's ten damage kinds, C1 to C10: header, xref, trailer, page tree, object tag, font map, font stream, font resources, zlib stream, truncation.
_Avoid_: error type, corruption type

**Finding**:
One diagnosed fact about a file, with its kind, location, evidence and repairability. Info findings describe the file and never trigger a repair.

**Near-miss**:
A keyword or number one byte away from valid, taken as evidence of C9 damage.

## Loading

**Carve**:
Our tolerant byte-level reading of a damaged file into objects, streams and dead ranges. The carver is the loader.
_Avoid_: parse (that means lopdf's strict reading)

**Landmark**:
A byte pattern the carver anchors on, such as `N G obj`, `endstream` or `%%EOF`.

**Shadow**:
A losing copy of a duplicated object. It is kept and reported, never dropped.

## Stream salvage

**Salvage**:
The search that repairs a damaged Flate stream by trying byte replacements.

**Salvage grade**:
How sure a salvage is. `Exact`: the window was exhausted and one candidate passed. `Accepted`: checks passed but the search was not exhausted. `Ambiguous`: more than one candidate passed.

**Localizer**:
A rule that narrows where in a stream the damaged byte can be, such as a failing TrueType table checksum.

**Work unit (W)**:
The deterministic budget unit. One inflated input byte is 1 W, and each inflate call adds 244 W.
_Avoid_: timeout, time budget

## Repair

**Toolpath**:
A way of building an output file. There are two: Resave and TemplateAssemble.

**Resave**:
Re-emit the whole file through lopdf with a rebuilt xref, trailer and page tree.

**TemplateAssemble**:
Rebuild a page with a substitute font from the bundled font database.

**Candidate**:
One output built by one toolpath. Every feasible candidate is built and then verified.

**Pass**:
The repair step for one corruption class.

**Verification tuple (V0 to V3)**:
The ordered checks that pick a winning candidate. V0 gates, V1 retention, V2 plausibility, V3 preserved features.

**Retention**:
How much of the input's content (glyphs, text operators, images, path fills) survives in a candidate.

## Fonts

**Font database**:
The bundled fonts, their glyph maps (`.gmap`) and the font index.

**TextOnly**:
Keep the damaged font as found and recover text through a rebuilt `/ToUnicode`, with no substitute font.

**Auto-accept**:
A font inference confirmed without asking the user, because its confidence and hit rate clear the thresholds.

**Font pick**:
A question to the user about which font a damaged slot held.

## Interaction

**Interaction**:
A question the engine asks through the `Interact` trait. Font pick and unreproducible-font are the two kinds.

**Answer source**:
Who answered: `User`, `UseBest`, `Batched` or `Policy`.

**UseBest**:
The engine's best-guess answer, also used when no human is asked.

**Parked job**:
A job waiting on an answer. It holds its row in the queue, and the batch carries on.

**Evict-and-replay**:
Freeing a parked job's analysis state under memory pressure, then rebuilding it when the answer arrives.

## Records

**Repair report**:
The pure, reproducible account of a job: findings, candidates, winner and answers.
_Avoid_: log

**Run record**:
The facts of one run that are not reproducible: paths, times, placement tier.

**Placement**:
How an output reaches its name without replacing anything. Tiers: atomic, linked, claimed-then-renamed.

**Custody mode**:
An opt-in mode that writes an evidence record per job and a hash-chained custody log.

## Evaluation

**REPDF corpus**:
The paper's 1,100 damaged and original files, pinned at one commit and kept outside the repo. It is a regression set, not a held-out test.

**Smoke subset**:
The 44 corpus files CI scores on every change.

**Golden**:
A stored score or UI frame that a test compares against.

**Regression gate**:
The text-layer LCS-F1 check per corruption class. It is labelled "not REPDF's metric".
_Avoid_: REPDF score

**REPDF-style OCR recall**:
Exact-word recall over OCR of the rendered output, reported per release. Never mixed with the regression gate.

**Clean-room**:
The rule that coding agents never read copyleft source. Every rule is grounded on permissive sources.
