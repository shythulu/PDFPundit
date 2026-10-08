# Test data

Committed binaries the fixtures (`src/pdf/fixtures.rs`) compile in with
`include_bytes!`. Neither file is generated in CI: both are committed once and
checked by hash in the fixture tests.

## `NotoSans-Regular-subset.ttf` (D-028)

A subset of Noto Sans Regular 2.015, unhinted, under the SIL Open Font License
1.1 (`OFL.txt` beside it; Noto declares no Reserved Font Name, so a subset keeps
the name).

- 9,672 bytes, sha256 `6c448bfe6b5ac43a463d04a8eeb5597ec4f2d33b736ab487bcaa2e9244a94383`
- tables `OS/2 cmap glyf head hhea hmtx loca maxp name post`; 109 glyphs, 104
  cmap entries (format 4), `post` 2.0 with real glyph names, composite glyphs
  for é è ç ñ ü

Source: `fonts/NotoSans/unhinted/ttf/NotoSans-Regular.ttf` from
`notofonts/notofonts.github.io` at commit
`28b15b4b43b7bed62b5cf6e6b0b5ff5846270535` (431,364 bytes, sha256
`f3961a9cde016d41a4879aecda1474d3a36d6bf54fa0e4643de029cc2248b0e8`).

Recipe, run once with fontTools 4.66.1:

```sh
pyftsubset NotoSans-Regular.ttf \
  --unicodes="U+0020-007E,U+00E9,U+00E8,U+00F1,U+00FC,U+00E7,U+0153,U+00AB,U+00BB,U+2019" \
  --layout-features='' --glyph-names --notdef-outline --no-hinting \
  --drop-tables+=GDEF,GPOS,GSUB \
  --output-file=NotoSans-Regular-subset.ttf
```

The output's bytes depend on the fontTools version, so the file is committed and
never regenerated; a different fontTools gives a different hash.

## `OFL.txt`

The Noto Project's OFL 1.1 text (4,396 bytes, sha256
`cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a`), byte-identical
to `OFL.txt` in `notofonts/latin-greek-cyrillic`.

## `tiny.jpg`

Our own 8×8 baseline JFIF JPEG, three components (650 bytes, sha256
`b138c5b6855e4eb2c36341f8e9ff7d10f00d153d1583bf9327b91f38791cb1ea`): the goldens'
DCT image. No JPEG encoder is a dependency, so it is committed as is; a test
asserts hayro decodes it.

## `ui-self/` (T-37, D-048)

Self-goldens: frames of screens the mockup has no frame for yet (the browse
picker, full layout and widget), drawn once by this crate and committed in the
`ui/` goldens' JSON format. They hold the look still until the user supplies a
mockup frame. To accept a deliberate change, run the test with
`PDFPUNDIT_BLESS_UI=1` and review the diff. The file names in them are the
mockup's sample names, not corpus files.
