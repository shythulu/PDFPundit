#!/usr/bin/env python3
"""infgen 3.6 and rapidgzip 0.16.0 on the damaged C9 streams (OBS-0707). A small probe, not a benchmark.

    TMPDIR=<scratch> python3 deflate_tools_probe.py /home/user/dfrc-korea/repdf <out-prefix> --infgen <path>

Same 63 damaged zlib streams as deflate_replay.py / gzrecover_probe.py (seed 20260927). For each stream:
  - infgen (madler/infgen 1e36c3da, zlib licence; `cc -O2 infgen.c -lz`) run as `infgen -s -q -i` on the
    damaged and on the original stream. From its per-block "! stats inout n:m (symbols) out reach" lines
    we get the block boundaries (bit offsets), the block that holds the changed byte, whether infgen
    parsed to the end of the stream, what it warned about, and whether every block other than the
    damaged one has the same statistics as in the original (damage confined to one block's symbols).
    infgen does not compare the zlib trailer with the data (its own header comment says so), so it
    is a structure oracle only.
  - rapidgzip (`rapidgzip.open(BytesIO(stream), parallelization=1)`): does it raise on the damage,
    how many bytes does it return, how many of them are wrong (compared with the original plaintext).
Outputs: <out-prefix>.csv and <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import io
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from deflate_replay import first_diff, flate_streams, inflate  # noqa: E402
from engine_smoke import sha  # noqa: E402
from gzrecover_probe import picks_c9, wrong  # noqa: E402

INOUT = re.compile(r"^! stats inout (\d+):(\d+) \((\d+)\) (\d+) -?(\d+)$")


def infgen(exe: str, raw: bytes) -> dict:
    cp = subprocess.run([exe, "-s", "-q", "-i"], input=raw, capture_output=True, timeout=120)
    lines = cp.stdout.decode("latin-1").splitlines()
    blocks = [tuple(int(x) for x in m.groups()[:4]) for m in map(INOUT.match, lines) if m]
    starts, pos = [], 2 * 8                       # bit offset in the stream (after the 2-byte zlib header)
    for nbytes, nbits, _, _ in blocks:
        starts.append(pos)
        pos += nbytes * 8 + nbits
    return {"rc": cp.returncode, "blocks": blocks, "starts": starts, "end_bit": pos,
            "reached_trailer": any(ln.strip() == "adler" for ln in lines),
            "warning": cp.stderr.decode("utf-8", "replace").strip().splitlines()[0][:160] if cp.stderr.strip() else ""}


def rapid(raw: bytes) -> tuple[str, bytes]:
    import rapidgzip
    try:
        with rapidgzip.open(io.BytesIO(raw), parallelization=1) as f:
            return "ok", f.read()
    except Exception as e:  # noqa: BLE001  (record whatever it raises)
        return f"{type(e).__name__}: {str(e).split('] ')[-1][:120]}", b""


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    exe = sys.argv[sys.argv.index("--infgen") + 1]
    rows = []
    for pk in picks_c9(root):
        orig, dmg = flate_streams(pk["original"]), flate_streams(pk["corrupted"])
        for og, raw in sorted(dmg.items()):
            o = orig.get(og)
            if o is None or o == raw:
                continue
            flip = first_diff(o, raw)
            _, plain, _ = inflate(o)
            zo, _, _ = inflate(raw)
            gd, go = infgen(exe, raw), infgen(exe, o)
            fb = max((i for i, s in enumerate(gd["starts"]) if s <= flip * 8), default=None)
            in_trailer = flip * 8 >= gd["end_bit"]
            others_same = (len(gd["blocks"]) == len(go["blocks"]) and
                           all(a == b for i, (a, b) in enumerate(zip(gd["blocks"], go["blocks"])) if i != fb))
            ro, rdata = rapid(raw)
            rows.append({"doc": pk["doc"], "obj": og[0], "raw_len": len(raw), "flip_offset": flip,
                         "zlib_outcome": zo, "infgen_rc": gd["rc"], "infgen_warning": gd["warning"],
                         "infgen_reached_trailer": gd["reached_trailer"], "blocks_damaged": len(gd["blocks"]),
                         "blocks_original": len(go["blocks"]), "flip_block": "trailer" if in_trailer else fb,
                         "other_blocks_identical": others_same, "rapidgzip_outcome": ro,
                         "rapidgzip_out_len": len(rdata), "plain_len": len(plain),
                         "rapidgzip_wrong_bytes": wrong(rdata, plain) + abs(len(rdata) - len(plain)) * (ro == "ok")})
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out.parent / f"{out.name}.csv", "w", newline="") as f:
        w = csv.DictWriter(f, list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    adler = [r for r in rows if "data check" in r["zlib_outcome"]]
    hard = [r for r in rows if "data check" not in r["zlib_outcome"]]
    grp = lambda rs: {  # noqa: E731
        "n": len(rs),
        "infgen_parsed_to_trailer": sum(r["infgen_reached_trailer"] for r in rs),
        "infgen_warned": sum(bool(r["infgen_warning"]) for r in rs),
        "block_count_unchanged": sum(r["blocks_damaged"] == r["blocks_original"] for r in rs),
        "damage_confined_to_one_block": sum(r["other_blocks_identical"] for r in rs),
        "rapidgzip_outcomes": dict(Counter(r["rapidgzip_outcome"].split(":")[0] for r in rs)),
        "rapidgzip_ok_with_wrong_bytes": sum(r["rapidgzip_outcome"] == "ok" and r["rapidgzip_wrong_bytes"] > 0 for r in rs)}
    summ = {"kind": "small probe, not a benchmark",
            "tools": {"infgen": "3.6 (madler/infgen 1e36c3da)", "rapidgzip": __import__("rapidgzip").__version__},
            "n": len(rows), "adler_only": grp(adler), "hard_errors": grp(hard),
            "hard_error_warnings": [r["infgen_warning"] for r in hard],
            "blocks_per_stream": dict(Counter(r["blocks_damaged"] for r in rows)),
            "inputs": [{"path": str(pk["corrupted"].relative_to(root)), "sha256": sha(pk["corrupted"])} for pk in picks_c9(root)]}
    (out.parent / f"{out.name}.summary.json").write_text(json.dumps(summ, indent=1) + "\n")
    print(json.dumps({k: v for k, v in summ.items() if k != "inputs"}, indent=1))


if __name__ == "__main__":
    main()
