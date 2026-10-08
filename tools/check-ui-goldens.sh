#!/usr/bin/env bash
# UI goldens check (T-00, D-055): the dump from generate.py is deterministic and
# the committed tests/data/ui is exactly what it produces.
#
# Runs dump_goldens.py twice into temp dirs, compares the two by SHA-256, then
# compares the committed set against them: same file list, same bytes. A
# difference is a bug report (a platform-specific diff included), not a tolerance.
#
# Usage: tools/check-ui-goldens.sh   (from anywhere; needs bash, python3, shasum)
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
dump="$repo/nimbalyst-local/mockups/pdfpundit-ansi-bbs/dump_goldens.py"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

python3 -I "$dump" "$work/a"
python3 -I "$dump" "$work/b"
(cd "$work/a" && shasum -a 256 -- *.json) > "$work/a.sha256"

(cd "$work/b" && shasum -a 256 -c --quiet "$work/a.sha256") || {
  echo "two runs of dump_goldens.py differ" >&2
  exit 1
}
if ! diff <(ls "$work/a") <(ls "$repo/tests/data/ui"); then
  echo "tests/data/ui does not hold exactly the dumped set; re-run dump_goldens.py" >&2
  exit 1
fi
(cd "$repo/tests/data/ui" && shasum -a 256 -c --quiet "$work/a.sha256") || {
  echo "tests/data/ui differs from the dump; re-run dump_goldens.py" >&2
  exit 1
}
echo "ui goldens: deterministic and committed ($(wc -l < "$work/a.sha256" | tr -d " ") files)"
