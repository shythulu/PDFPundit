# 0016. Generated assets are committed

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** D-021 (owner answer (c)), D-011, D-143 (owner answer 2026-10-23); GitHub issue #9

## Decision

- `assets/` holds the bundled fonts, their OFL texts, the Adobe Glyph List and three word lists (en, fr, es; Leipzig news corpora, 50,000 words each).
- `tools/build-templates` generates `assets/gmaps/*.gmap` and `assets/fontindex.json`. Both are committed.
- CI regenerates them and fails on any drift.
- Template PDFs are built in memory at run time, through the same writer as every output.
- Hash tests pin every committed asset.

## Word-list licence

- Leipzig labels its downloads CC BY 4.0, but its website says CC BY-NC 4.0. The owner chose the stricter reading: **treat the word lists as CC BY-NC 4.0.**
- The source code stays MIT. The word lists are compiled into the binary, so **every release archive carries non-commercial data.** The app as shipped is for non-commercial use.
- `THIRD_PARTY_NOTICES.md` and the release notes must say CC BY-NC 4.0, with Leipzig attribution. On PR #21 they still say CC BY 4.0.
- Revisit if commercial use is ever wanted. Then replace the lists with permissively licensed word frequencies, or get written permission from Leipzig.

## Why

- A fresh checkout builds with no network and no generator step.
- Committed output shows asset changes in review, as a diff.

## Rejected

- Git-ignored generated assets (the original map line). A fresh build would need the generator and its inputs.
- Committed template PDFs. They would be a second writer path to keep in sync.

## Rules for agents

- Edit the generator, run it, and commit the generated files with it. Never hand-edit a `.gmap` or `fontindex.json`.
- A new asset needs its licence text under `assets/licenses/` and a line in the third-party notices.
