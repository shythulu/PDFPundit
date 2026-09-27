#!/usr/bin/env bash
# Hands-on producer-diversity (GAP-002, task 5) and damage-generator (DMG-009, GAP-104, task 6)
# checks for the tooling-damage agent (W3-C). Reproduces, in one place, every CLI/library
# invocation whose output is cited as verification_evidence in
# research/staging/tooling-damage/tooling/ledger.jsonl.
#
# Requires (installed in-container 2026-09-27): libreoffice-writer, ghostscript, texlive-latex-base,
# zzuf (all via apt), a Python venv with reportlab/pycairo/pikepdf/peepdf-3 (pip), the preinstalled
# Chromium at /opt/pw-browsers/chromium-1194, and a `radamsa` binary built from
# https://gitlab.com/akihe/radamsa (MIT) under $OUTDIR/radamsa-src.
#
# Usage: producer_and_generator_checks.sh <scratch-outdir> <repdf-original-pdf> <venv-python> <radamsa-bin>
set -euo pipefail
OUT="${1:?scratch outdir}"
ORIG="${2:?path to a REPDF original PDF}"
PY="${3:?path to venv python3}"
RADAMSA="${4:?path to radamsa binary}"
mkdir -p "$OUT"
cd "$OUT"

echo "### LibreOffice headless (txt -> pdf)"
printf 'PDFPundit producer-diversity test.\nThis is a small sample document used to test headless PDF producers.\n' > lo_input.txt
export HOME="$OUT"
rm -rf "$OUT/lo_profile"
soffice --headless -env:UserInstallation="file://$OUT/lo_profile" --convert-to pdf lo_input.txt --outdir "$OUT"
ls -la lo_input.pdf

echo "### Ghostscript ps2pdf"
cat > sample.ps <<'PS'
%!PS
/Helvetica findfont 24 scalefont setfont
72 700 moveto
(PDFPundit producer-diversity test - Ghostscript ps2pdf) show
showpage
PS
ps2pdf sample.ps sample_gs.pdf
ls -la sample_gs.pdf

echo "### Chromium headless print-to-pdf"
cat > sample.html <<'HTML'
<html><body><h1>PDFPundit producer-diversity test</h1><p>Chromium headless print-to-pdf sample.</p></body></html>
HTML
/opt/pw-browsers/chromium-1194/chrome-linux/chrome --headless --disable-gpu --no-sandbox \
  --print-to-pdf="$OUT/sample_chromium.pdf" "file://$OUT/sample.html" || true
ls -la sample_chromium.pdf

echo "### LaTeX (pdflatex)"
cat > sample.tex <<'TEX'
\documentclass{article}
\begin{document}
PDFPundit producer-diversity test -- LaTeX pdflatex sample.
\end{document}
TEX
pdflatex -interaction=nonstopmode -output-directory="$OUT" sample.tex >/dev/null
ls -la sample.pdf

echo "### reportlab"
"$PY" - <<'EOF'
from reportlab.pdfgen import canvas
c = canvas.Canvas("sample_reportlab.pdf")
c.drawString(72, 700, "PDFPundit producer-diversity test - reportlab")
c.save()
EOF
ls -la sample_reportlab.pdf

echo "### pycairo (PDF surface)"
"$PY" - <<'EOF'
import cairo
surface = cairo.PDFSurface("sample_cairo.pdf", 200, 200)
ctx = cairo.Context(surface)
ctx.select_font_face("Sans")
ctx.set_font_size(14)
ctx.move_to(10, 100)
ctx.show_text("PDFPundit producer-diversity test - cairo")
ctx.show_page()
surface.finish()
EOF
ls -la sample_cairo.pdf

echo "### pikepdf sanity check on the outputs above"
"$PY" - <<'EOF'
import pikepdf
for f in ["sample_reportlab.pdf", "sample_cairo.pdf", "lo_input.pdf", "sample_gs.pdf", "sample.pdf", "sample_chromium.pdf"]:
    pdf = pikepdf.open(f)
    print(f, "pages:", len(pdf.pages))
EOF

echo "### zzuf mutation of a REPDF original"
cp "$ORIG" ./sample_orig.pdf
zzuf -s 0 -r 0.001 < sample_orig.pdf > mutated_zzuf.pdf
python3 -c "
a=open('sample_orig.pdf','rb').read(); b=open('mutated_zzuf.pdf','rb').read()
print('same size:', len(a)==len(b), 'bytes differing:', sum(1 for x,y in zip(a,b) if x!=y))
"

echo "### radamsa mutation of the same file"
"$RADAMSA" -s 1 sample_orig.pdf > mutated_radamsa.pdf
python3 -c "
a=open('sample_orig.pdf','rb').read(); b=open('mutated_radamsa.pdf','rb').read()
print('orig len:', len(a), 'mutated len:', len(b), 'bytes differing (min len):', sum(1 for x,y in zip(a,b) if x!=y))
"

echo "### peepdf-3 structural scan"
"$(dirname "$PY")/peepdf" -l sample_orig.pdf | head -20

echo "### dfxml (dfxml_python) round-trip on sample_orig.pdf"
"$PY" - <<'EOF'
from dfxml import objects as Objects
import hashlib
data = open("sample_orig.pdf", "rb").read()
fi = Objects.FileObject()
fi.filename = "sample_orig.pdf"
fi.filesize = len(data)
fi.sha256 = hashlib.sha256(data).hexdigest()
vol = Objects.VolumeObject(); vol.append(fi)
doc = Objects.DFXMLObject(version="1.2.0"); doc.append(vol)
xml = doc.to_dfxml()
assert fi.sha256 in xml
print("DFXML sha256 hashdigest present in serialized output, len=", len(xml))
EOF

echo "ALL CHECKS COMPLETED"
