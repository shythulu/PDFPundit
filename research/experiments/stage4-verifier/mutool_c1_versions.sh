#!/usr/bin/env bash
# Stage 4 verifier (2026-10-05): is the mutool text regression on two C1 (header-damaged) REPDF files a MuPDF
# version change or a build difference? Runs `mutool draw -q -F txt` from three builds on the two files whose
# engine_smoke rows changed between the recorded run (mutool 1.28.0) and the pinned run (1.28.5), plus
# PyMuPDF 1.28.2 (bundles MuPDF 1.28.2) on the same files.
#   S=<scratch> bash mutool_c1_versions.sh > results/mutool_c1_versions.txt
# Builds: $S/cenv/bin/mutool (conda-forge mupdf 1.28.0), $S/pins/mupdf-1.28.0 and $S/pins/mupdf-1.28.5
# (both: build_pins.sh, same recipe: release tarball from mupdf.com, bundled thirdparty, make -j2 tools).
set -u
: "${S:?set S to the scratch directory}"
R=/home/user/dfrc-korea/repdf   # REPDF @ e547d4d1b77ead7e8cccce8b02878a6d33aa427a
FILES=("corrupted/saveas/text/Lumina_Interior_Lighting_Designbook(saveas)_header.pdf"
       "corrupted/print/text/global_cloud_datacenter_cooling_failure_report(print)_header.pdf")
T=$(mktemp -d -p "$S/tmp")
for f in "${FILES[@]}"; do
  echo "== $f  sha256 $(sha256sum "$R/$f" | cut -c1-64)"
  for m in "$S/cenv/bin/mutool" "$S/pins/mupdf-1.28.0/bin/mutool" "$S/pins/mupdf-1.28.5/bin/mutool"; do
    v=$("$m" -v 2>&1 | head -1)
    nice -n 10 "$m" draw -q -F txt -o "$T/x.txt" "$R/$f" 2>"$T/x.err"; rc=$?
    echo "$v [$m]: rc=$rc text_bytes=$(wc -c < "$T/x.txt") errors: $(grep -v 'repeated' "$T/x.err" | sort -u | head -4 | tr '\n' '|')"
  done
  "$S/venv/bin/python" - "$R/$f" <<'PY'
import sys, pymupdf
d = pymupdf.open(sys.argv[1])
t = "".join(p.get_text() for p in d)
print(f"PyMuPDF {pymupdf.__version__} (MuPDF {pymupdf.mupdf_version}): pages={len(d)} chars={len(t)} "
      f"warnings: {pymupdf.TOOLS.mupdf_warnings()[:200]!r}")
PY
done
rm -rf "$T"
