#!/bin/sh
# Regenerate the schema, the example, the CASE view and the design document.
# Usage: build.sh <example dir> <output document>
set -eu
W=$(cd "$(dirname "$0")" && pwd)
EX=$1
DOC=$2
PY="$W/.venv/bin/python"
rm -rf "$EX"
mkdir -p "$EX"
"$PY" "$W/schema.py" "$EX/pdfpundit-custody.schema.json"
"$PY" "$W/example.py" "$EX"
"$PY" "$W/case_from_custody.py" "$EX/invoice_scan_2024.repaired.pdf.custody.json" \
  "$EX/invoice_scan_2024.repaired.pdf.case.jsonld"
"$PY" "$W/condense.py" "$EX/invoice_scan_2024.repaired.pdf.custody.json" > "$EX/display.json"
"$PY" "$W/splice.py" "$W/doc_template.md" "$EX" "$DOC"
