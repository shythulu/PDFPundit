#!/usr/bin/env python3
"""Compare the zlib-rs outputs written by rust_replay (second argument) with zlib-ng 2.2.5 (python-zlib-ng)
at the same level 6 / memLevel / strategy, to see which C library zlib-rs is byte-compatible with.

    python3 rust_replay_compare_ng.py <sample-dir> <zlib-rs-output-dir>
"""
import sys
from pathlib import Path

import zlib_ng.zlib_ng as zn

d, o = Path(sys.argv[1]), Path(sys.argv[2])
rows = [ln.split("\t") for ln in (d / "index.tsv").read_text().splitlines() if ln]
same = 0
for name, mem, strat, *_ in rows:
    raw = (d / f"{name}.raw").read_bytes()
    c = zn.compressobj(6, zn.DEFLATED, 15, int(mem), int(strat))
    same += (c.compress(raw) + c.flush()) == (o / f"{name}.zrs").read_bytes()
print(f"zlib-ng {zn.ZLIBNG_VERSION} level 6 == zlib-rs output: {same}/{len(rows)}")
