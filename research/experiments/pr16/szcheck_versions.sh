#!/usr/bin/env bash
# pr16-ingest: check PR #16's E4 decoder-state sizes and that mzbench's checkpoint/resume (DecompressorOxide: Clone)
# also builds against miniz_oxide 0.9 (the version the design pins).  Not part of PR #16.
# usage: szcheck_versions.sh <experiments-dir> <scratch-dir>
#   0.8.9 : szcheck and mzbench as committed (Cargo.lock pins 0.8.9), built --locked
#   0.9.1 : copies of both crates with miniz_oxide = "=0.9.1", built --offline from the local cargo cache
set -eu
E=$1; S=$2
for c in szcheck mzbench; do (cd "$E/$c" && CARGO_TARGET_DIR="$S/target" nice -n 10 cargo build --release -j2 --locked -q); done
echo "miniz_oxide $(grep -A1 'name = "miniz_oxide"' "$E/szcheck/Cargo.lock" | sed -n 's/version = "\(.*\)"/\1/p'): $("$S/target/release/szcheck")"
rm -rf "$S/v09"; mkdir -p "$S/v09"; cp -r "$E/szcheck" "$E/mzbench" "$S/v09/"
for c in szcheck mzbench; do
  sed -i 's/miniz_oxide = "0.8"/miniz_oxide = "=0.9.1"/' "$S/v09/$c/Cargo.toml"; rm -f "$S/v09/$c/Cargo.lock"
  (cd "$S/v09/$c" && CARGO_TARGET_DIR="$S/target09" nice -n 10 cargo build --release -j2 --offline -q)
done
echo "miniz_oxide $(grep -A1 'name = "miniz_oxide"' "$S/v09/szcheck/Cargo.lock" | sed -n 's/version = "\(.*\)"/\1/p'): $("$S/target09/release/szcheck")"
echo "mzbench (uses DecompressorOxide::clone) builds against 0.9.1: $(test -x "$S/target09/release/mzbench" && echo yes || echo no)"
echo "rustc: $(rustc --version)"
