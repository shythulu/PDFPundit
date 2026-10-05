#!/usr/bin/env python3
"""Which DEFLATE encoder (and parameters) reproduces the Flate streams of REPDF's originals
byte-for-byte? This bounds how far an encoder-replay oracle (GAP-154) reaches per producer.

    python3 encoder_sweep.py /home/user/dfrc-korea/repdf results/encoder_sweep [--per-file 6]

For every Flate stream (up to --per-file per original, in file order, decoded size <= 256 KiB)
it tries, in order, and records the first exact match:
  1. stock zlib (Python's zlib) compress at levels 0-9, default parameters;
  2. zlib compressobj at level 6 x memLevel 1-9 x strategy {default, filtered}, windowBits 15.
     Every REPDF Flate stream starts 78 9C: a 32 KiB window and FLEVEL 2, which stock zlib
     writes only for level 6 with strategy default or filtered, so no other zlib setting can
     reproduce these streams;
  3. zlib-ng (python zlib-ng), isa-l (python isal) and libdeflate (python deflate) at every level.
Streams are also classed by their dictionary (content, font file, image, ToUnicode, other).
Outputs <out>.csv (one row per stream) and <out>.summary.json.
"""
from __future__ import annotations

import csv
import json
import re
import sys
import zlib
from collections import Counter, defaultdict
from pathlib import Path

try:
    from zlib_ng import zlib_ng
except ImportError:  # pragma: no cover
    zlib_ng = None
try:
    from isal import isal_zlib
except ImportError:  # pragma: no cover
    isal_zlib = None
try:
    import deflate as libdeflate
except ImportError:  # pragma: no cover
    libdeflate = None

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def trim(b: bytes) -> bytes:
    return b[:-2] if b.endswith(b"\r\n") else b[:-1] if b.endswith((b"\n", b"\r")) else b


def kind_of(head: bytes) -> str:
    head = head[head.rfind(b"obj"):]
    if b"/Subtype/Image" in head or b"/Subtype /Image" in head:
        return "image"
    if b"/Length1" in head or b"/Subtype/CIDFontType0C" in head or b"/Subtype/Type1C" in head:
        return "fontfile"
    if b"/Type/XRef" in head or b"/Type /XRef" in head:
        return "xref"
    if b"/Type/ObjStm" in head or b"/Type /ObjStm" in head:
        return "objstm"
    if b"/Type/Metadata" in head:
        return "metadata"
    return "other"   # content streams, ToUnicode CMaps, forms, ... (refined below from data)


def refine(kind: str, data: bytes) -> str:
    if kind != "other":
        return kind
    if b"begincmap" in data[:400] or b"CIDInit" in data[:200]:
        return "tounicode"
    if re.search(rb"\b(BT|re|cm|Tf|Do)\b", data[:2000]):
        return "content"
    return "other"


def sweep(data: bytes, target: bytes) -> str:
    for lv in range(10):
        if zlib.compress(data, lv) == target:
            return f"zlib:level={lv}"
    for mem in range(1, 10):
        for strat in (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED):
            c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, strat)
            if c.compress(data) + c.flush() == target:
                return f"zlib:level=6,memLevel={mem},strategy={strat}"
    if zlib_ng is not None:
        for lv in range(10):
            if zlib_ng.compress(data, lv) == target:
                return f"zlib-ng:level={lv}"
    if isal_zlib is not None:
        for lv in range(4):
            if isal_zlib.compress(data, lv) == target:
                return f"isal:level={lv}"
    if libdeflate is not None:
        for lv in range(1, 13):
            if libdeflate.zlib_compress(data, lv) == target:
                return f"libdeflate:level={lv}"
    return ""


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    per_file = int(sys.argv[sys.argv.index("--per-file") + 1]) if "--per-file" in sys.argv else 6
    out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    for f in sorted((corpus / "original").rglob("*.pdf")):
        a = f.read_bytes()
        n = 0
        for m in STREAM_RE.finditer(a):
            s = trim(m.group(1))
            try:
                d = zlib.decompress(s)
            except zlib.error:
                continue
            if len(d) > 256 * 1024:
                continue
            kind = refine(kind_of(a[max(0, m.start() - 600):m.start()]), d)
            rows.append({"file": str(f.relative_to(corpus)), "mode": f.parent.parent.name, "kind": kind,
                         "comp_len": len(s), "decomp_len": len(d), "header": s[:2].hex(), "match": sweep(d, s)})
            n += 1
            if n >= per_file:
                break
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    summ: dict = defaultdict(lambda: defaultdict(Counter))
    for r in rows:
        enc = r["match"].split(",")[0] if r["match"] else "none"
        summ[r["mode"]][r["kind"]][enc] += 1
    res = {"streams": len(rows), "per_file": per_file, "header_bytes": dict(Counter(r["header"] for r in rows)),
           "versions": {"zlib": zlib.ZLIB_RUNTIME_VERSION,
                        "zlib_ng": getattr(zlib_ng, "ZLIBNG_VERSION", None) if zlib_ng else None,
                        "isal": __import__("isal").__version__ if isal_zlib else None,
                        "libdeflate": getattr(libdeflate, "__version__", None) if libdeflate else None},
           "by_mode_kind_encoder": {m: {k: dict(c) for k, c in v.items()} for m, v in summ.items()},
           "params_seen": dict(Counter(r["match"] for r in rows))}
    Path(f"{out}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
