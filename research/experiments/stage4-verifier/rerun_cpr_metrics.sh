#!/usr/bin/env bash
# Stage 4 verifier (2026-10-05): rebuild and re-run the committed CPR Step 1 smoke test (OBS-0704) and the
# metric-suite probe (OBS-0706) after the restart deleted agent B's scratch venv, CPR checkout and model caches.
#   S=<scratch dir> bash rerun_cpr_metrics.sh cpr|metrics
# Install steps (all under $S; no apt, no sudo):
#   CPR       git clone https://github.com/BeenyHail/CPR $S/cpr && git -C $S/cpr checkout 9ecd6853a5eedad71631ec8e48c50ae4c23ff87c
#             (Step 1 only; no LLM is called, so no model download)
#   $S/venv   python3 -m venv $S/venv && $S/venv/bin/pip install pymupdf==1.28.2 fonttools==4.66.1 \
#               rapidfuzz==3.14.6 jiwer==4.0.0   (full freeze: results/venv_freeze.txt)
#   metrics   $S/venv-old with pymupdf==1.28.2 pypdfium2==5.13.0 pikepdf==10.14.0 scikit-image==0.26.0 lpips==0.1.4
#             torch==2.14.0+cpu torchvision (download.pytorch.org/whl/cpu) python-doctr==1.1.0 dinglehopper==0.11.0
#             rapidfuzz==3.14.6 jiwer==4.0.0; TORCH_HOME=$S/torch and DOCTR_CACHE_DIR=$S/doctr hold the AlexNet,
#             db_resnet50 and crnn_vgg16_bn weights (fetched on first use); Tesseract 5.5.3 from the conda-forge env
#             $S/cenv (see rerun_smoke.sh); qpdf 12.4.2 release zip in $S/dl/bin/qpdf12.
set -euo pipefail
: "${S:?set S to the scratch directory}"
REPO=$(cd "$(dirname "$0")/../../../.." && pwd)
R=/home/user/dfrc-korea/repdf   # REPDF @ e547d4d1b77ead7e8cccce8b02878a6d33aa427a
OUT="$REPO/research/experiments/stage4-verifier/results"
mkdir -p "$S/tmp" "$OUT"
cd "$REPO"
export PYTHONDONTWRITEBYTECODE=1 TMPDIR=$S/tmp
case "$1" in
  cpr)     PATH=$S/venv/bin:$PATH timeout 1200 nice -n 10 "$S/venv/bin/python" research/experiments/cpr_step1_smoke.py \
             "$R" "$S/cpr" "$OUT/cpr_step1_smoke" ;;
  metrics) TORCH_HOME=$S/torch DOCTR_CACHE_DIR=$S/doctr QPDF=$S/dl/bin/qpdf12/bin/qpdf TESSERACT=$S/cenv/bin/tesseract \
             timeout 1200 nice -n 10 "$S/venv-old/bin/python" research/experiments/metrics_probe.py "$R" "$OUT/metrics_probe" ;;
esac
