# 0008. Duplicate objects: the last well-formed copy wins

- **Status:** accepted
- **Date:** 2026-10-23
- **Decided in:** A-1 (D-032); research HYP-301, WP-4.2 flag

## Decision

- When the carve finds several copies of one object, keyed by `(num, gen)`, the last copy in byte order that is well formed wins.
- A truncation guard stops a copy cut short by end of file from replacing an earlier good copy.
- Losing copies are kept as shadows and reported. They are never discarded silently.

## Why

- qpdf, PDFium, lopdf, hayro and ISO 32000 §7.5.6 agree on "last copy wins". It matches how incremental updates work.
- The well-formed gate comes from pdf.js. Without it, a damaged final copy beats a good earlier one.
- The truncation guard is our own addition, aimed at REPDF's C10 damage.
- Keeping shadows preserves evidence for a later revision-history analysis ([0020](0020-tamper-evidence.md)).

## Rejected

- Plain last-copy-wins (lopdf, hayro). A truncated tail copy would win.
- Highest-generation wins. Noted in D-032, not adopted. No measurement compared it.

## Revisit if

- The engine matrix (research WP-1.4) shows another rule recovers more on real-world damage. HYP-301 is still untested outside REPDF.
