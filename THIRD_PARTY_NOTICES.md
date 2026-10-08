# Third-party notices

PDFPundit's binary carries data from third parties beyond the licences its crates
declare in their manifests. `cargo deny check licenses` (see `deny.toml`) covers the
crates; this file covers the data compiled into them, which no manifest declares.
The entry for the Noto fonts is added with the code that bundles them.

## Foxit standard-14 substitute fonts (via hayro-interpret 0.8.0)

hayro-interpret compiles in fourteen Type 1 fonts (`Foxit*.pfb`) that substitute
for the PDF standard-14 fonts. They were extracted from PDFium; the original code
is copyright 2014 Foxit Software Inc. Licence (BSD-3-Clause), from
`hayro-interpret-0.8.0/assets/LICENSE_FOXIT`:

```
Copyright 2014 PDFium Authors. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google Inc. nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Adobe predefined CMaps (via hayro-cmap 0.1.0)

hayro-cmap compiles in Adobe's predefined CMaps (`assets/cmaps.brotli`). Licence
(BSD-3-Clause), from `hayro-cmap-0.1.0/assets/LICENSE.txt`:

```
Copyright 1990-2023 Adobe. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.

Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

Neither the name of Adobe nor the names of its contributors may be
used to endorse or promote products derived from this software without
specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## ICC profiles (via hayro-interpret 0.8.0)

- `CGATS001Compat-v2-micro.icc`, the CMYK profile from saucecontrol's
  Compact-ICC-Profiles, is dedicated to the public domain under CC0-1.0
  (`hayro-interpret-0.8.0/assets/CGATS_LICENSE.txt`,
  https://creativecommons.org/publicdomain/zero/1.0/).
- `LAB.icc` was generated with LCMS2; hayro ships no licence text for it.

## Adobe Glyph List and ITC Zapf Dingbats Glyph List (`assets/agl/`)

The glyph-name decoder compiles in a table generated from the Adobe Glyph List 2.0
(`glyphlist.txt`) and the ITC Zapf Dingbats Glyph List 2.0 (`zapfdingbats.txt`),
from https://github.com/adobe-type-tools/agl-aglfn. Both files are vendored
unmodified and their headers are copied into the generated
`src/pdf/fontdb/agl_table.rs`. Copyright 2002-2019 Adobe (http://www.adobe.com/).
Licence (BSD-3-Clause), from `assets/agl/LICENSE.md`:

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.

Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

Neither the name of Adobe nor the names of its contributors may be
used to endorse or promote products derived from this software without
specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## pdf.js glyph names (`assets/agl/pdfjs-extra.txt`)

The glyph-name decoder's heuristic layer compiles in 127 glyph names, mostly
TeX/math names such as `braceleftBig` and `arrowhookleft`, with their Unicode
values, taken from `src/core/glyphlist.js` of pdf.js
(https://github.com/mozilla/pdf.js). Copyright 2012 Mozilla Foundation. Licensed
under the Apache License, Version 2.0; the full text, from the pdf.js repository's
`LICENSE`, is in `assets/licenses/pdfjs-Apache-2.0.txt`. pdf.js ships no `NOTICE`
file. The names and values are unchanged.

## DarkBerry palette (`assets/theme/darkberry-palette.json`)

The four DarkBerry themes (Blackwater, Mire, Fen, Wisp) read their colours from
the DarkBerry palette, v0.3.0, published at https://darkberry.slacklab.ca/. The
file is a byte-identical copy of the one vendored with the mockup
(`nimbalyst-local/mockups/pdfpundit-ansi-bbs/darkberry-palette.json`); a test
pins it to the hash the UI goldens were dumped with. The palette ships with no
licence text; its terms are to be confirmed with its author before the first
public binary (D-046).
