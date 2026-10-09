#!/usr/bin/env bash
# Stage 4 verifier (2026-10-05): re-run the committed tooling-wildcard and tooling-engines experiments behind
# TOOL-450/456/457/461/462/464/476 and OBS-0703, writing only to research/experiments/stage4-verifier/results/.
#   S=<scratch dir> bash rerun_wildcard.sh c9|resync|xref|fonts|preflate|decipher|decipher-frozen|deflate
# $S/venv: CPython 3.11.15 (zlib 1.3) with pikepdf 10.16.0, pypdf 6.19.0, fonttools 4.66.1, numpy, z3-solver 5.1.0.0,
# zlib-ng 1.0.0 (pip, no apt). Rust probes are built from research/experiments/{pfprobe,preflate_probe} with
# `CARGO_TARGET_DIR=$S/rust/target_pf cargo build --release --locked -j2` and copied to $S/dl/bin/pf/.
set -euo pipefail
: "${S:?set S to the scratch directory}"
REPO=$(cd "$(dirname "$0")/../../../.." && pwd)
R=/home/user/dfrc-korea/repdf
OUT="$REPO/research/experiments/stage4-verifier/results"
PY=$S/venv/bin/python
export PYTHONDONTWRITEBYTECODE=1 TMPDIR=$S/tmp
mkdir -p "$OUT" "$S/tmp"
cd "$REPO/research/experiments"
case "$1" in
  c9)       timeout 1500 nice -n 10 python3 c9_replay_correct.py "$R" "$OUT/c9_replay_correct" --every 3 --workers 2 ;;
  resync)   timeout 1500 nice -n 10 python3 dict_resync.py "$R" "$OUT/dict_resync" --max-streams 300 ;;
  xref)     timeout 900 nice -n 10 "$PY" xref_constraints.py "$R" "$OUT/xref_constraints" ;;
  fonts)    timeout 600 nice -n 10 "$PY" print_font_names.py "$R" "$OUT/print_font_names" ;;
  preflate) rm -rf "$S/pf_sample"
            nice -n 10 python3 preflate_prep.py "$R" "$S/pf_sample" --max 400
            nice -n 10 "$S/dl/bin/pf/preflate_probe" "$S/pf_sample" > "$OUT/preflate_probe.tsv"
            nice -n 10 "$S/dl/bin/pf/preflate_probe" "$S/pf_sample" --replay > "$OUT/preflate_replay.tsv"
            python3 preflate_prep.py --summarize "$OUT/preflate_probe.tsv" "$OUT/preflate_replay.tsv" "$OUT/preflate_probe.summary.json" ;;
  decipher) for k in 0 1; do timeout 1150 nice -n 10 "$PY" decipher_codes.py "$R" "$REPO/research/cache/fulltext" "$OUT/decipher_codes" --shard $k/2 & done; wait
            "$PY" decipher_codes.py "$R" "$REPO/research/cache/fulltext" "$OUT/decipher_codes" --merge 2 ;;
  decipher-frozen)   # the LM corpus as the stored run saw it: research/cache/fulltext minus the 4 texts the contrarian
            # cached at 2026-10-05 18:46 (after the stored run at 17:17); the language model is trained on every *.txt there
            F=$S/fulltext_frozen; rm -rf "$F"; mkdir -p "$F"
            for t in "$REPO"/research/cache/fulltext/*.txt; do
              case "$(basename "$t")" in 32c3b9689cb6*|170e215db5f0*|a43d0f089add*|2677aec2ba45*) ;; *) ln -s "$t" "$F/" ;; esac
            done
            for k in 0 1; do timeout 1150 nice -n 10 "$PY" decipher_codes.py "$R" "$F" "$OUT/decipher_codes_lm29" --shard $k/2 & done; wait
            "$PY" decipher_codes.py "$R" "$F" "$OUT/decipher_codes_lm29" --merge 2 ;;
  deflate)  timeout 1200 nice -n 10 "$PY" deflate_replay.py "$R" "$OUT/deflate_replay" --pfprobe "$S/dl/bin/pf/pfprobe" ;;
esac
