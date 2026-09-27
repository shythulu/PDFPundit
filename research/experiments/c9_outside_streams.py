#!/usr/bin/env python3
"""Where do REPDF's C9 ("stream zlib") byte changes land, and how many fall in each stream?

    python research/experiments/c9_outside_streams.py /home/user/dfrc-korea/repdf \
        research/experiments/results/c9_outside_streams

Diffs each of the 100 C9 files against its original (sizes are equal, so offsets line up) and
places every changed byte in the original's layout: inside a stream body (`stream` EOL ..
`endstream`), inside an object but outside its stream body (dictionary or array syntax), or
between objects (xref table, trailer, whitespace). For changes inside object syntax it records
the object's value type (dictionary, array, other) and the nearest preceding name token
inside that object (for example /Type) as a rough location. Writes
<out>.csv (one row per changed byte outside stream bodies) and <out>.summary.json.
Independent of flate_c9_probe.py, which reaches the same per-stream count by another route.
"""

from __future__ import annotations

import csv
import json
import re
import sys
from collections import Counter
from pathlib import Path

STREAM = re.compile(rb"(?<!end)stream\r?\n")
OBJ = re.compile(rb"(\d+)\s+(\d+)\s+obj\b")
NAME = re.compile(rb"/[A-Za-z0-9_.+-]+")


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    rows, per_stream, where, keys, objtype = [], Counter(), Counter(), Counter(), Counter()
    files = sorted(root.glob("corrupted/**/*_stream_zlib.pdf"))
    for f in files:
        orig = Path(str(f).replace("/corrupted/", "/original/").replace("_stream_zlib.pdf", ".pdf"))
        a, b = orig.read_bytes(), f.read_bytes()
        assert len(a) == len(b), f
        bodies = [(m.end(), a.find(b"endstream", m.end())) for m in STREAM.finditer(a)]
        objs = [(m.start(), a.find(b"endobj", m.end())) for m in OBJ.finditer(a)]
        hits = Counter()
        for d in (i for i in range(len(a)) if a[i] != b[i]):
            s = next((k for k, (s0, e0) in enumerate(bodies) if s0 <= d < e0), None)
            if s is not None:
                hits[s] += 1
                continue
            enc = next((s0 for s0, e0 in objs if s0 <= d < e0), None)
            kind = "object syntax" if enc is not None else "between objects"
            key, vtype = "", ""
            if enc is not None:
                body = a[OBJ.match(a, enc).end():d].lstrip()
                vtype = "dict" if body.startswith(b"<<") else "array" if body.startswith(b"[") else "other"
                names = NAME.findall(body)
                key = names[-1].decode() if names else ""
                objtype[vtype] += 1
            where[kind] += 1
            if key:
                keys[key] += 1
            rows.append({"file": str(f.relative_to(root)), "offset": d, "where": kind, "object_value": vtype,
                         "nearest_name": key, "orig_byte": a[d], "new_byte": b[d]})
        per_stream.update(hits.values())
        where["stream body"] += sum(hits.values())

    out.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    summary = {
        "files": len(files),
        "changed_bytes_by_location": dict(where),
        "changed_bytes_per_damaged_stream": {str(k): v for k, v in sorted(per_stream.items())},
        "files_with_changes_outside_streams": len({r["file"] for r in rows}),
        "object_syntax_by_value_type": dict(objtype),
        "nearest_name_in_enclosing_object_top10": dict(keys.most_common(10)),
    }
    Path(f"{out}.summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
