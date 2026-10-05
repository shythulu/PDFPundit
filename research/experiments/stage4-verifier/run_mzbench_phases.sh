#!/usr/bin/env bash
# Stage 4 verifier (2026-10-05): PR #16's mid-size unlocalized search (CLM-1019; pr16-ingest OBS-1010 reproduced
# 28/47 on the stride-10 sample, phase 0, against PR #16's 158/239 = 66%) re-run on phase 0 (control, same host load)
# and on further stride-10 phases, so more of the ~470 mid-size cases are covered.
#   S=<scratch> bash run_mzbench_phases.sh <phase> [<phase> ...]
# mzbench: research/experiments/pr16/mzbench, unchanged (Cargo.lock: miniz_oxide 0.8.9), built with
#   CARGO_TARGET_DIR=$S/rust/target_mz cargo build --release -j2 --locked
# cases: python3 research/experiments/pr16/build_cases.py <repdf> $S/pr16/c9out/partA.json $S/pr16/cases
#   (partA.json from research/experiments/pr16/c9exp.py Part A; 1,445 cases, as pr16-ingest rebuilt them)
# Configuration as run_mzbench.sh "mid": adler, full range, max 16 KiB, 20 s budget, min 4 KiB, stride 10.
set -u
: "${S:?set S to the scratch directory}"
OUT=$(cd "$(dirname "$0")" && pwd)/results
for ph in "$@"; do
  log="$OUT/mzbench_mid_phase$ph.txt"
  echo "== mid phase $ph: mzbench <cases> adler 0 16384 20 4096 10 $ph  start $(date -u +%FT%TZ)  load $(cut -d' ' -f1-3 /proc/loadavg)" > "$log"
  ( time timeout 1200 nice -n 10 "$S/rust/target_mz/release/mzbench" "$S/pr16/cases" adler 0 16384 20 4096 10 "$ph" ) >> "$log" 2>&1
  echo "exit $? end $(date -u +%FT%TZ)  load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$log"
done
