#!/usr/bin/env bash
# Stage 4 verifier: rebuild the engines after the 2026-10-05 restart and re-run the committed smoke
# tests (research/experiments/engine_smoke.py, oracle_smoke.py) at their recorded seed (20260927).
#
#   (no automated install mode: run the install steps below by hand first; every path is under $S, no apt, no sudo)
#   S=<scratch dir> bash rerun_smoke.sh engine recorded    # versions Stage 2 ran (OBS-0700/0701)
#   S=<scratch dir> bash rerun_smoke.sh engine pinned      # the 5k plan rev 1 WP-3.1 pins
#   S=<scratch dir> bash rerun_smoke.sh oracle recorded    # OBS-0702
#
# Install steps (what "install" does; every path is under $S):
#   qpdf 12.4.2      github.com/qpdf/qpdf/releases/download/v12.4.2/qpdf-12.4.2-bin-linux-x86_64.zip -> $S/dl/bin/qpdf12
#   pdfcpu 0.15.0 / 0.16.1  github.com/pdfcpu/pdfcpu/releases/download/v<ver>/pdfcpu_<ver>_Linux_x86_64.tar.xz
#   PDFBox 3.0.8     repo1.maven.org/maven2/org/apache/pdfbox/pdfbox-app/3.0.8/pdfbox-app-3.0.8.jar
#   mutool 1.28.0, gs 10.08.0, Poppler 26.09.0, Tesseract 5.5.3
#                    micromamba (micro.mamba.pm/api/micromamba/linux-64/latest) create -p $S/cenv -c conda-forge
#                    mupdf=1.28.0 ghostscript=10.08.0 poppler=26.09.0 tesseract=5.5.3
#   pdf.js           npm install pdfjs-dist@6.3.289 (recorded) / @6.4.299 (pinned) in $S/npm-<ver>
#   Python libs      $S/venv-old: pikepdf==10.14.0 pymupdf==1.28.2 pypdfium2==5.13.0 pillow (recorded)
#                    $S/venv:     pikepdf==10.16.0 pymupdf==1.28.2 pypdfium2==5.14.0 (pinned) + metric libs
#   Rust probe       research/experiments/rsprobe copied to $S/rust/rsprobe071 (Cargo.lock as committed:
#                    lopdf 0.45.0, pdf 0.10.0, hayro 0.7.1) and $S/rust/rsprobe080 (hayro =0.8.0,
#                    hayro-syntax =0.8.0); `cargo build --release -j2` each
#   veraPDF 1.30.2   software.verapdf.org/releases/arlington/1.30/verapdf-arlington-1.30.2-installer.zip,
#                    `java -jar verapdf-izpack-installer-1.30.2.jar auto.xml` (CLI pack only) -> $S/dl/vera/arl
set -euo pipefail
: "${S:?set S to the scratch directory}"
REPO=$(cd "$(dirname "$0")/../../../.." && pwd)
R=/home/user/dfrc-korea/repdf
OUT="$REPO/research/experiments/stage4-verifier/results"
QFILE="$REPO/research/cache/code/github.com_qpdf_qpdf@4eba95899886/qpdf/qtest/qpdf/xref-compressed-in-compressed.pdf"
mkdir -p "$S/tmp" "$OUT"
case "${2:-recorded}" in
  recorded) PY=$S/venv-old/bin/python; PDFCPU=$S/dl/bin/pdfcpu15/pdfcpu_0.15.0_Linux_x86_64/pdfcpu
            PJ=$S/npm-6.3.289; RS=$S/dl/bin/rs071/rsprobe; MUT=$S/cenv/bin/mutool; PT=$S/cenv/bin/pdftotext ;;
  pinned)   PY=$S/venv/bin/python; PDFCPU=$S/dl/bin/pdfcpu16/pdfcpu_0.16.1_Linux_x86_64/pdfcpu
            PJ=$S/npm-6.4.299; RS=$S/dl/bin/rs080/rsprobe
            MUT=${MUTOOL_PINNED:-$S/cenv/bin/mutool}; PT=${PDFTOTEXT_PINNED:-$S/cenv/bin/pdftotext}
            # pdfcpu 0.16.1 refuses the 0.15.0 config in ~/.config/pdfcpu ("run: pdfcpu config reset", exit 1 on
            # every command; first pinned run, results/engine_smoke_pinned_run1_sharedconfig.*), so it gets its own.
            mkdir -p "$S/xdg/pdfcpu16"; export XDG_CONFIG_HOME=$S/xdg/pdfcpu16 ;;
esac
export QPDF=$S/dl/bin/qpdf12/bin/qpdf QPDF_APT=/usr/bin/qpdf MUTOOL=$MUT GS=$S/cenv/bin/gs PDFTOTEXT=$PT \
       PDFCPU=$PDFCPU PDFBOX_JAR=$S/dl/pdfbox-app-3.0.8.jar NODE=node \
       PDFJS_MJS=$PJ/node_modules/pdfjs-dist/legacy/build/pdf.mjs RSPROBE=$RS \
       ARLINGTON=$S/dl/vera/arl/arlington-pdf-model-checker TMPDIR=$S/tmp PYTHONDONTWRITEBYTECODE=1
case "$1" in
  engine) nice -n 10 "$PY" "$REPO/research/experiments/engine_smoke.py" "$R" "$OUT/engine_smoke_$2" --recheck "$QFILE" ;;
  oracle) nice -n 10 "$PY" "$REPO/research/experiments/oracle_smoke.py" "$R" "$OUT/oracle_smoke_$2" ;;
  install) echo "not automated: run the install steps in the comment block by hand (as the verifier did on 2026-10-05)" ;;
esac
