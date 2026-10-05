#!/usr/bin/env python3
"""gzrecover (gzrt 0.90, GPL-2.0-or-later, run as a binary only) on the damaged C9 streams (OBS-0705).

    TMPDIR=<scratch> python3 gzrecover_probe.py /home/user/dfrc-korea/repdf <out-prefix> --gzrecover <path>

Same damaged streams as deflate_replay.py (the seeded smoke sample's two C9 "print" files plus two
seeded "saveas" C9 files). Each damaged zlib stream is re-wrapped as a gzip member (10-byte header,
the raw DEFLATE data without zlib header and Adler-32, and a CRC-32/ISIZE trailer computed from the
original plaintext: the same integrity information the PDF stream already carries as Adler-32) and passed to
`gzrecover -p`. The output is compared with the original stream's plaintext:
  - out_len vs plain_len;
  - correct_prefix: bytes before the first wrong byte;
  - wrong_bytes_emitted: positions in the common length that differ (bytes gzrecover emitted that are
    wrong: the GAP-151 hazard), and the same count for plain zlib inflate for comparison.
A small probe, not a benchmark.
"""

from __future__ import annotations

import csv
import json
import random
import struct
import subprocess
import sys
import zlib
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from deflate_replay import first_diff, flate_streams, inflate  # noqa: E402
from engine_smoke import sample, sha  # noqa: E402


def picks_c9(root: Path) -> list[dict]:
    ps = [pk for pk in sample(root) if pk["class"] == "C9"]
    suf = "_stream_zlib"
    files = sorted((root / "corrupted" / "saveas").rglob(f"*){suf}.pdf"))
    for f in random.Random(20260927).sample(files, 2):
        stem, rel = f.name[: -len(f"{suf}.pdf")], f.parent.relative_to(root / "corrupted")
        ps.append({"class": "C9", "corrupted": f, "original": root / "original" / rel / f"{stem}.pdf",
                   "doc": f"{rel.as_posix()}/{stem}"})
    return ps


def wrong(a: bytes, b: bytes) -> int:
    return sum(x != y for x, y in zip(a, b))


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    exe = sys.argv[sys.argv.index("--gzrecover") + 1]
    rows = []
    for pk in picks_c9(root):
        orig, dmg = flate_streams(pk["original"]), flate_streams(pk["corrupted"])
        for og, raw in sorted(dmg.items()):
            o = orig.get(og)
            if o is None or o == raw:
                continue
            _, plain, _ = inflate(o)
            zo, zplain, _ = inflate(raw)
            gz = (b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\xff" + raw[2:-4]
                  + struct.pack("<II", zlib.crc32(plain), len(plain) & 0xFFFFFFFF))
            cp = subprocess.run([exe, "-p"], input=gz, capture_output=True, timeout=120)
            g = cp.stdout
            rows.append({"doc": pk["doc"], "obj": og[0], "plain_len": len(plain),
                         "zlib_outcome": zo, "zlib_out_len": len(zplain), "zlib_wrong_bytes": wrong(zplain, plain),
                         "gzrecover_rc": cp.returncode, "gz_out_len": len(g),
                         "gz_correct_prefix": first_diff(g, plain) if first_diff(g, plain) is not None else len(g),
                         "gz_wrong_bytes_emitted": wrong(g, plain),
                         "gz_recovered_beyond_zlib": len(g) - len(zplain)})
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out.parent / f"{out.name}.csv", "w", newline="") as f:
        w = csv.DictWriter(f, list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    adler = [r for r in rows if "data check" in r["zlib_outcome"]]
    hard = [r for r in rows if "data check" not in r["zlib_outcome"]]
    summ = {"kind": "small probe, not a benchmark", "gzrecover": "gzrt 0.90 (arenn/gzrt 97b3f9c8)", "n": len(rows),
            "adler_only": {"n": len(adler), "gz_emits_wrong_bytes": sum(r["gz_wrong_bytes_emitted"] > 0 for r in adler),
                           "zlib_emits_wrong_bytes": sum(r["zlib_wrong_bytes"] > 0 for r in adler)},
            "hard_errors": {"n": len(hard), "outcomes": dict(Counter(r["zlib_outcome"] for r in hard)),
                            "rows": [{k: r[k] for k in ("obj", "plain_len", "zlib_out_len", "gz_out_len",
                                                         "gz_correct_prefix", "gz_wrong_bytes_emitted")} for r in hard]},
            "inputs": [{"path": str(pk["corrupted"].relative_to(root)), "sha256": sha(pk["corrupted"])} for pk in picks_c9(root)]}
    (out.parent / f"{out.name}.summary.json").write_text(json.dumps(summ, indent=1) + "\n")
    print(json.dumps({k: v for k, v in summ.items() if k != "inputs"}, indent=1))


if __name__ == "__main__":
    main()
