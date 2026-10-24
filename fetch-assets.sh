#!/bin/sh
# Re-downloads the pinned font, Adobe Glyph List and word-list assets and
# verifies their SHA-256 hashes. Dev-time only; nothing here runs in a build or
# in CI tests.
# After it, `cargo run -p build-templates` regenerates assets/gmaps and
# assets/fontindex.json (`-- --check` only compares).
#
# Usage: fetch-assets.sh [--fonts] [DEST_ROOT]
#   DEST_ROOT defaults to the directory holding this script; --fonts fetches
#   only the google/fonts extras and their licences (what the CI assets job
#   needs to regenerate every .gmap).
#
# Sources:
#   notofonts/notofonts.github.io @ 28b15b4b43b7bed62b5cf6e6b0b5ff5846270535
#   notofonts/latin-greek-cyrillic (OFL.txt, byte-identical across Noto repos)
#   adobe-type-tools/agl-aglfn (frozen upstream; pinned by hash)
#   Leipzig Corpora Collection news corpora, 1M sentences (CC BY 4.0): the
#   archives (about 290 MB each) are pinned by hash, their *-words.txt is cut
#   to assets/dicts/{en,fr,es}.txt by tools/cut-wordlist.py (python3), and the
#   cut is checked against its own pinned hash (G-09, D-011).
#   google/fonts @ 2eb0b48d5f760f62e286216f0859a8c540dbc1bd (SIL OFL 1.1): the
#   corpus's other open-licensed faces (G-10, D-010 (b)). Each program goes to
#   font-sources/ (git-ignored: only its .gmap and index entry are committed,
#   tools/build-templates EXTRAS); each family's OFL.txt to
#   assets/licenses/ofl/<family>.txt.
# tests/data/NotoSans-Regular-subset.ttf is generated once with pyftsubset and
# committed; it has no download source, so it is only verified, never fetched.
set -eu

ONLY_FONTS=0
if [ "${1:-}" = "--fonts" ]; then ONLY_FONTS=1; shift; fi
ROOT=${1:-$(cd "$(dirname "$0")" && pwd)}
GFONTS=https://raw.githubusercontent.com/google/fonts/2eb0b48d5f760f62e286216f0859a8c540dbc1bd
NOTO=https://raw.githubusercontent.com/notofonts/notofonts.github.io/28b15b4b43b7bed62b5cf6e6b0b5ff5846270535/fonts
OFL=https://raw.githubusercontent.com/notofonts/latin-greek-cyrillic/main/OFL.txt
AGL=https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/master
LEIPZIG=https://downloads.wortschatz-leipzig.de/corpora
TOOLS=$(cd "$(dirname "$0")" && pwd)/tools

H_SANS=f3961a9cde016d41a4879aecda1474d3a36d6bf54fa0e4643de029cc2248b0e8
H_SERIF=a15cfbbc1539d707115111d672d590a3d70d4f74b4c0a315956da20ae19a14e1
H_OFL=cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a
H_GLYPHLIST=a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789
H_ZAPF=f6394e3cb8a447e84a1dad75d4baaf2aa7f45dc104faf369f4720e1a774ef2dc
H_AGLLIC=58147d341e7a34aa2196862395a34d2fd95716c41d5ed26efb59ab0e12f92089
H_SUBSET=6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383
H_ENG_NEWS=7cad9136013d27b6230841558d19c5ab39b18c502dce8ccd3a821fdf74b4081b
H_FRA_NEWS=907eed297ab7b5fbe0ea87105084899158f660890e87f121b2a24eb9853a6467
H_SPA_NEWS=4666b54caad54cd46cdfea80e7cb727663b220c2ead871b4ccf6285df0599090
H_DICT_EN=f01e59f5364ee5d3d0af81fbe284e306bd831a150ad8ea5f9fa8508f925485d5
H_DICT_FR=fb6e75fc81e415ec9566cd4af0ab69f9ddeefa73404ea4d880e795a4b97950bc
H_DICT_ES=4bbc67a4df753bfa29e3b0c60246ff320c2b9f1cc1d207678fe83d211ed1ffda

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
# wordlist CORPUS ARCHIVE_HASH REL_PATH HASH: download a Leipzig archive, check
# it, cut its word list, check the cut.
wordlist() {
  dest="$ROOT/$3"
  mkdir -p "$(dirname "$dest")"
  work=$(mktemp -d)
  if ! curl -fsSL --retry 3 -o "$work/$1.tar.gz" "$LEIPZIG/$1.tar.gz"; then
    echo "FAIL download $1 ($LEIPZIG/$1.tar.gz)" >&2; rm -rf "$work"; fail=1; return
  fi
  got=$(sha256 "$work/$1.tar.gz")
  if [ "$got" != "$2" ]; then
    echo "FAIL hash $1.tar.gz: expected $2, got $got" >&2; rm -rf "$work"; fail=1; return
  fi
  if ! tar -xzf "$work/$1.tar.gz" -C "$work" "$1/$1-words.txt" ||
     ! python3 -I "$TOOLS/cut-wordlist.py" "$work/$1/$1-words.txt" "$work/cut.txt" >/dev/null; then
    echo "FAIL cut $3 from $1" >&2; rm -rf "$work"; fail=1; return
  fi
  got=$(sha256 "$work/cut.txt")
  if [ "$got" != "$4" ]; then
    echo "FAIL hash $3: expected $4, got $got (another Python's Unicode tables?)" >&2
    rm -rf "$work"; fail=1; return
  fi
  mv "$work/cut.txt" "$dest"
  rm -rf "$work"
  echo "ok   $3"
}
# gfont PATH HASH LICENCE_HASH: a google/fonts program into font-sources/ and
# its family's OFL.txt into assets/licenses/ofl/. Brackets in a variable
# font's name are percent-encoded for the URL.
gfont() {
  url=$(printf '%s' "$1" | sed 's/\[/%5B/g; s/\]/%5D/g')
  dir=$(printf '%s' "$1" | cut -d/ -f2)
  fetch "$GFONTS/$url" "font-sources/$(basename "$1")" "$2"
  fetch "$GFONTS/ofl/$dir/OFL.txt" "assets/licenses/ofl/$dir.txt" "$3"
}
# verify REL_PATH HASH
verify() {
  if [ ! -f "$ROOT/$1" ]; then echo "MISSING $1 (generated once, committed; not downloadable)" >&2; fail=1; return; fi
  got=$(sha256 "$ROOT/$1")
  if [ "$got" != "$2" ]; then echo "FAIL hash $1: expected $2, got $got" >&2; fail=1; return; fi
  echo "ok   $1"
}

gfont 'ofl/abrilfatface/AbrilFatface-Regular.ttf' 5971d4a3758a922a9fedc7f6fb825a96341a2e718c45a4b2c9a6b417c8c4dbe9 aba8997e16b1e3888c6e855ba883c70d96bd4375bff8cb9c7ce0f097200f74b8
gfont 'ofl/archivoblack/ArchivoBlack-Regular.ttf' dd9a89a019b4849f66ab75455fe7bdf931311042cbb0f0f97acc061539703180 3173acd82f8c6159b5b1037b539fcbd4edff68e65c2ea8b9412b5a5ca97b08ff
gfont 'ofl/batang/Batang-Regular.ttf' 0929031e799b2feadda22208c58f503515e6f8fa2eaba75acd2e6847d73fc54b cc115f501827fffddcbf1ef86e5248ee55e40f0c3fb9725173b248ccc2d5df51
gfont 'ofl/belgrano/Belgrano-Regular.ttf' 5bf095dfbc56718bea7d74c0b30c36413714aaf8833d2cb012b604b64fd383d9 284a5a26e6db9a04259a5690ad57d52c56a4c515ca2ece1943d950f99e709dcd
gfont 'ofl/bigshoulders/BigShoulders[opsz,wght].ttf' 4b4b24aa6f799aa73cdcd5b6fa840cbcbbb38b81fa9fa82c25126a4530c1ba44 fbc746aabf0eb1847dfd92e2efc4596d79fa897d60b8e64062a22f585508fb3f
gfont 'ofl/biryani/Biryani-Regular.ttf' 0b846b4f8600e7943a3a86a2f7ce04c20daa7d2cdad74c951edcc8e07c367116 505bb8f3c30f2006b4e02d250fda31ed94b651a35f1124a201ca1c405ce989af
gfont 'ofl/bricolagegrotesque/BricolageGrotesque[opsz,wdth,wght].ttf' 413e7357809ddd12fd80a96a8a396de0e401638d4acd3cb3e37532f0472ac682 4b5a7d8f37f5602621c8a8d7358a6a2e71317e6c231c661e15aef0275d3e07ba
gfont 'ofl/cormorant/Cormorant[wght].ttf' 8f12cb21f05b61649192eaff13eeeb1b5619bc524feeae672fb916974259a076 60700d351cac4650c51f3f9db318d2a420f8b45052dba2715eb5fec41f0f6956
gfont 'ofl/harmattan/Harmattan-Regular.ttf' 5fbaafc51ad21663729b168afaff5f28d3e3087f1fc736ada32d7cd99674c0e0 8625c5d464ebfe457d071266178fc8b699ccdb7bd58051d53046b3e06806f4e1
gfont 'ofl/hind/Hind-Regular.ttf' 01de158022f53077b52303e46de3b0ab5fb245222a7ffe25a2a57fdd9e969162 f62ef357d3a1c3d27edd35a6e1ba350e8a8d13499797964eeadefbf0b3b15d1f
gfont 'ofl/marhey/Marhey[wght].ttf' 19c30686157ec965797fae8c8feb7bb142b082217227562d1a809c067df447e9 096744f008d418398e0b72ce8d4c46f195c036600a7a48df256ac95e026a79dc
gfont 'ofl/merriweather/Merriweather[opsz,wdth,wght].ttf' d0ed0e359e396af7ad05e73dffd11a3a4c326ea0d0283c56bd9361cb2cc86a96 715bd349bfa6116f99e351e1548321dcfa25b91df65502d22ca8d624557697d2
gfont 'ofl/mukta/Mukta-Regular.ttf' 2958e4af564507df2a856164df6f9978dacb03f999a4f34a0c269dc8a4de9688 2ee5e8e47cd7d08f60bb9555f72b25912c9e81b13f5bc9a0551ddf943da6ca98
gfont 'ofl/nobile/Nobile-Regular.ttf' fc2eab24ea3dbe7f5d80324fa5c5d3ea5098175755c3218e3d54cec4355987f2 9465823369fbe1ae0b5a3065021d53ef8c56e113e664229455dc80b237fa6a07
gfont 'ofl/notosanskr/NotoSansKR[wght].ttf' 194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252 1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9
gfont 'ofl/opensans/OpenSans[wdth,wght].ttf' 36643644f318a812aab2d2ed3bb98f8cf0872527f835fe9398d95fe6b9adb878 fbbbcfef55318de350562559b671360de6d597112ecc5c73881b05092db89602
gfont 'ofl/oswald/Oswald[wght].ttf' 5b38c246e255a12f5712d640d56bcced0472466fc68983d2d0410ec0457c2817 0fd731a904b729a4e02eaf5e8ebd06783edd9abe400e8882760160230675b652
gfont 'ofl/playfairdisplay/PlayfairDisplay[wght].ttf' c40f2293766a503bc70cce9e512ef844a4ccb7cbcde792fe2ea31d191917d8d6 566be814f8e96e93dfa16101331557eb6b5467e9e03f627c0910fe93ca12300e
gfont 'ofl/poppins/Poppins-Regular.ttf' 7e65201e9b79159e2300267cc885e16c8dcef2424cdfa09a29bfb0980a94a7ba 6be04893d770899a015649c7aa3b582f871b272f8747a92b78b17c3e5c8b2573
gfont 'ofl/prompt/Prompt-Regular.ttf' dbd497803cec3caffbc6b7f599ca6fed8beea0ed7e0ad1e098130c5ebbc4fd42 1d08c63944e639bbfe8a1b81e3c6a63836806c126b3573b9cda0db83fd27ffe9
gfont 'ofl/rubik/Rubik[wght].ttf' 1b3a7437ba2af80e465e773ed60c5036d1ba6ace492d89046dbcf18fb31e4e88 472cbe7c25441df63e9c7864b43eb3c0f4b3df950c66a76224e6cfe1eae843fb
gfont 'ofl/tajawal/Tajawal-Regular.ttf' 6882892da3e03527d5db2bbab3b48bde6ef2e878a43f522d1a4eebda90010a19 9b584984f9db0ee30347391a76eff9c0a6b03dc450c3c6afe3757a2cb3a4db87
gfont 'ofl/teko/Teko[wght].ttf' d1321889f262bbbff632e7976349853399cd097b6f382d4b19790c915c13c1ae 4a193c54e911c1e8194d4db657423f2527f54a1239f24cb67ecb09128dc9f065
gfont 'ofl/zcoolxiaowei/ZCOOLXiaoWei-Regular.ttf' a42b620140f493db42f741351dfbf343c0936d58588ee8004b8b2a218d997ff1 a094514ca57cf8f9c5e8d8d1adab5d8cd3a377297ff016f9df2c05b3ecd77f0a
if [ "$ONLY_FONTS" -eq 1 ]; then
  [ "$fail" -eq 0 ] && echo "google/fonts extras verified" && exit 0
  echo "some google/fonts extras failed" >&2; exit 1
fi

fetch "$NOTO/NotoSans/unhinted/ttf/NotoSans-Regular.ttf"   assets/fonts/NotoSans-Regular.ttf  $H_SANS
fetch "$NOTO/NotoSerif/unhinted/ttf/NotoSerif-Regular.ttf" assets/fonts/NotoSerif-Regular.ttf $H_SERIF
fetch "$OFL"                  assets/licenses/OFL-Noto.txt $H_OFL
fetch "$OFL"                  tests/data/OFL.txt           $H_OFL
fetch "$AGL/glyphlist.txt"    assets/agl/glyphlist.txt     $H_GLYPHLIST
fetch "$AGL/zapfdingbats.txt" assets/agl/zapfdingbats.txt  $H_ZAPF
fetch "$AGL/LICENSE.md"       assets/agl/LICENSE.md        $H_AGLLIC
verify tests/data/NotoSans-Regular-subset.ttf $H_SUBSET
wordlist eng_news_2025_1M $H_ENG_NEWS assets/dicts/en.txt $H_DICT_EN
wordlist fra_news_2024_1M $H_FRA_NEWS assets/dicts/fr.txt $H_DICT_FR
wordlist spa_news_2024_1M $H_SPA_NEWS assets/dicts/es.txt $H_DICT_ES

[ "$fail" -eq 0 ] && echo "all assets verified" || { echo "some assets failed" >&2; exit 1; }
