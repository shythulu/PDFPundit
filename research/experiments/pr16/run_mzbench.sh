#!/usr/bin/env bash
# pr16-ingest: re-run PR #16's E4 search benchmark (mzbench) in the configurations the PR #16 notes describe.
# usage: run_mzbench.sh <mzbench-binary> <cases-dir> <log-dir> [configs...]
# configs: err | small | grammar | mid   (default: err small grammar)
#   err     : inflate-error streams, window [k-4096, k+8), 60 s budget           (note: "E4: inflate-error streams")
#   small   : Adler-only streams <= 4 KiB, full range, 30 s budget                (note: "E4: Adler-only streams <= 4 KiB")
#   grammar : Adler-only streams with a grammar_loc.py flag, [flag-256, flag+8),
#             any size up to 400 KB, 20 s budget  (run grammar_loc.py first)    (note: "E4 + E5")
#   mid     : Adler-only streams 4-16 KiB, full range, 20 s budget, every 10th case only
#             (PR #16 sampled 239 of them with 4 processes; we sample to stay inside a 20-minute run)
# Each config runs under `timeout 1200` and `nice -n 10`.  stderr lists accepts at a wrong position.
set -u
BIN=$1; CASES=$2; LOG=$3; shift 3
CONFIGS=${*:-err small grammar}
mkdir -p "$LOG"
for c in $CONFIGS; do
  case $c in
    err)     args="err 4096 1000000000 60" ;;
    small)   args="adler 0 4096 30" ;;
    grammar) args="adler 256 400000 20" ;;
    mid)     args="adler 0 16384 20 4096 10 0" ;;
  esac
  echo "== $c: $BIN $CASES $args  start $(date -u +%FT%TZ)" > "$LOG/$c.log"
  ( time timeout 1200 nice -n 10 "$BIN" "$CASES" $args ) >> "$LOG/$c.log" 2>&1
  echo "exit $? end $(date -u +%FT%TZ)" >> "$LOG/$c.log"
done
