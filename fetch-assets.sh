#!/bin/sh
# Re-downloads the pinned font and Adobe Glyph List assets and verifies their
# SHA-256 hashes. Dev-time only; nothing here runs in a build or in CI tests.
# After it, `cargo run -p build-templates` regenerates assets/gmaps and
# assets/fontindex.json (`-- --check` only compares).
#
# Usage: fetch-assets.sh [DEST_ROOT]   (default: the directory holding this script)
#
# Sources:
#   notofonts/notofonts.github.io @ 28b15b4b43b7bed62b5cf6e6b0b5ff5846270535
#   notofonts/latin-greek-cyrillic (OFL.txt, byte-identical across Noto repos)
#   adobe-type-tools/agl-aglfn (frozen upstream; pinned by hash)
# tests/data/NotoSans-Regular-subset.ttf is generated once with pyftsubset and
# committed; it has no download source, so it is only verified, never fetched.
set -eu

ROOT=${1:-$(cd "$(dirname "$0")" && pwd)}
NOTO=https://raw.githubusercontent.com/notofonts/notofonts.github.io/28b15b4b43b7bed62b5cf6e6b0b5ff5846270535/fonts
OFL=https://raw.githubusercontent.com/notofonts/latin-greek-cyrillic/main/OFL.txt
AGL=https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master

H_SANS=f3961a9cde016d41a4879aecda1474d3a36d6bf54fa0e4643de029cc2248b0e8
H_SERIF=a15cfbbc1539d707115111d672d590a3d70d4f74b4c0a315956da20ae19a14e1
H_OFL=cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a
H_GLYPHLIST=a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789
H_ZAPF=f6394e3cb8a447e84a1dad75d4baaf2aa7f45dc104faf369f4720e1a774ef2dc
H_AGLLIC=58147d341e7a34aa2196862395a34d2fd95716c41d5ed26efb59ab0e12f92089
H_SUBSET=6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

fail=0
# fetch URL REL_PATH HASH
fetch() {
  dest="$ROOT/$2"
  mkdir -p "$(dirname "$dest")"
  tmp="$dest.part"
  if ! curl -fsSL --retry 3 -o "$tmp" "$1"; then
    echo "FAIL download $2 ($1)" >&2; rm -f "$tmp"; fail=1; return
  fi
  got=$(sha256 "$tmp")
  if [ "$got" != "$3" ]; then
    echo "FAIL hash $2: expected $3, got $got" >&2; rm -f "$tmp"; fail=1; return
  fi
  mv "$tmp" "$dest"
  echo "ok   $2"
}
# verify REL_PATH HASH
verify() {
  if [ ! -f "$ROOT/$1" ]; then echo "MISSING $1 (generated once, committed; not downloadable)" >&2; fail=1; return; fi
  got=$(sha256 "$ROOT/$1")
  if [ "$got" != "$2" ]; then echo "FAIL hash $1: expected $2, got $got" >&2; fail=1; return; fi
  echo "ok   $1"
}

fetch "$NOTO/NotoSans/unhinted/ttf/NotoSans-Regular.ttf"   assets/fonts/NotoSans-Regular.ttf  $H_SANS
fetch "$NOTO/NotoSerif/unhinted/ttf/NotoSerif-Regular.ttf" assets/fonts/NotoSerif-Regular.ttf $H_SERIF
fetch "$OFL"                  assets/licenses/OFL-Noto.txt $H_OFL
fetch "$OFL"                  tests/data/OFL.txt           $H_OFL
fetch "$AGL/glyphlist.txt"    assets/agl/glyphlist.txt     $H_GLYPHLIST
fetch "$AGL/zapfdingbats.txt" assets/agl/zapfdingbats.txt  $H_ZAPF
fetch "$AGL/LICENSE.md"       assets/agl/LICENSE.md        $H_AGLLIC
verify tests/data/NotoSans-Regular-subset.ttf $H_SUBSET

[ "$fail" -eq 0 ] && echo "all assets verified" || { echo "some assets failed" >&2; exit 1; }
