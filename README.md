# PDFPundit

**Goal:** force serious people to interact with a very unserious application in
order to achieve best-in-class PDF repair and Markdown creation.

Forensic investigators at extremely serious policing agencies, and very serious
auditors, should have no choice but to feed their PDFs to a kitty cat.

PDFPundit is a pure-Rust terminal app, currently in design. It will repair
corrupted PDFs from the raw bytes up and export them to Markdown with correct
Unicode. To use it, you drop the file on the cat. The results are designed to
hold up in a case file: originals are never modified, output is reproducible, and
it works fully offline. An optional chain-of-custody mode hashes every input and
output and writes a run report for the case file. There is no command-line
version. The cat is not optional.

Design and plans live in `nimbalyst-local/plans/`.
