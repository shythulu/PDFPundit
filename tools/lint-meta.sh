#!/usr/bin/env bash
# Lint meta-tests (plan §3.1, D-076, eng-r3-q9): prove the determinism bans fire.
#
# Copies the crate to a temp dir, injects one forbidden construct under src/pdf/
# at a time, and asserts `cargo clippy --all-targets -- -D warnings` fails on the
# expected lint:
#   std::time::Instant::now()   -> clippy::disallowed_types
#   2.0_f64.cbrt()              -> clippy::disallowed_methods
#   for _ in HashMap::new()     -> clippy::iter_over_hash_type
# An untouched copy must pass first, so a failure is the injection's and nothing
# else's. Runs as a CI step, not inside `cargo test`, with its own target dir so it
# never waits on the outer build's lock.
#
# Usage: tools/lint-meta.sh   (from anywhere; needs bash, rsync, cargo + clippy)
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

crate="$work/crate"
export CARGO_TARGET_DIR="$work/target"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS

rsync -a --exclude /target --exclude /.git "$repo/" "$crate/"
cp "$crate/src/pdf/mod.rs" "$work/pdf-mod.rs.orig"

clippy() {
    (cd "$crate" && cargo clippy --locked --all-targets --color never -- -D warnings) >"$work/out.txt" 2>&1
}

echo "lint-meta: baseline (untouched copy must pass)"
if ! clippy; then
    cat "$work/out.txt"
    echo "lint-meta: FAIL: the untouched copy does not pass clippy" >&2
    exit 1
fi

failures=0
check() {
    local lint="$1" body="$2"
    cp "$work/pdf-mod.rs.orig" "$crate/src/pdf/mod.rs"
    printf '\n#[allow(dead_code)]\nfn lint_meta_probe() {\n    %s\n}\n' "$body" >>"$crate/src/pdf/mod.rs"
    if clippy; then
        echo "lint-meta: FAIL: clippy passed with \`$body\` under src/pdf/" >&2
        failures=$((failures + 1))
    # The note reads `clippy::disallowed-types`, the lint name `disallowed_types`.
    elif grep -Eq "clippy::${lint//_/[-_]}" "$work/out.txt"; then
        echo "lint-meta: ok: \`$body\` -> clippy::$lint"
    else
        cat "$work/out.txt"
        echo "lint-meta: FAIL: clippy failed, but not on clippy::$lint" >&2
        failures=$((failures + 1))
    fi
}

check disallowed_types 'let _ = std::time::Instant::now();'
check disallowed_methods 'let _ = 2.0_f64.cbrt();'
check iter_over_hash_type 'for _ in std::collections::HashMap::<u8, u8>::new() {}'

if [ "$failures" -ne 0 ]; then
    echo "lint-meta: $failures check(s) failed" >&2
    exit 1
fi
echo "lint-meta: all checks passed"
