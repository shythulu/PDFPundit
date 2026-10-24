#!/usr/bin/env python3
"""WP-3.14 challenge: are producer profiles needed as a model input for encoder replay, or does per-file self-calibration
from the file's own intact Flate streams suffice?

A. Originals: for every Flate stream in REPDF original/{saveas,print}, the set of memLevels (1..9) at which stock zlib
   level 6, default strategy, windowBits 15 reproduces the stored zlib stream byte-for-byte.
B. C9 files (corrupted/*/*/*_stream_zlib.pdf): calibrate from the streams that still inflate with a valid Adler-32
   (intersection of their memLevel sets, no producer label used), then check each damaged stream's original counterpart
   (same byte offset in the original file; C9 changes bytes in place) against that calibration.

    python3 selfcal.py <repdf_root> <out_prefix> [--workers 2]
writes <out_prefix>.originals.csv, <out_prefix>.c9.csv and <out_prefix>.summary.json
"""
import csv
import json
import re
import sys
import zlib
from collections import Counter
from multiprocessing import Pool
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def memset(raw: bytes, z: bytes) -> frozenset:
    hits = set()
    for mem in range(1, 10):
        c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, zlib.Z_DEFAULT_STRATEGY)
        if c.compress(raw) + c.flush() == z:
            hits.add(mem)
    return frozenset(hits)


def streams(data: bytes):
    """Yield (offset, status, raw, z) for every stream whose body starts like zlib."""
    for m in STREAM_RE.finditer(data):
        body = m.group(1)
        d = zlib.decompressobj()
        try:
            raw = d.decompress(body)
            ok = d.eof
        except zlib.error:
            yield m.start(1), "damaged", None, None
            continue
        if not ok:
            yield m.start(1), "damaged", None, None
            continue
        yield m.start(1), "intact", raw, body[: len(body) - len(d.unused_data)]


def fmt(s) -> str:
    return ",".join(map(str, sorted(s))) or "-"


def do_original(f: str):
    out = []
    p = Path(f)
    for off, st, raw, z in streams(p.read_bytes()):
        if st == "intact":
            out.append((f, off, len(raw), fmt(memset(raw, z))))
    return out


def do_c9(args):
    root, f = args
    cf = Path(f)
    rel = cf.relative_to(Path(root) / "corrupted")
    producer = rel.parts[0]
    orig = Path(root) / "original" / rel.parent / cf.name.replace("_stream_zlib.pdf", ".pdf")
    intact, damaged = [], []
    for off, st, raw, z in streams(cf.read_bytes()):
        if st == "intact":
            intact.append(memset(raw, z))
        else:
            damaged.append(off)
    cal = frozenset(range(1, 10))
    for s in intact:
        cal &= s
    omap = {off: (raw, z) for off, st, raw, z in streams(orig.read_bytes()) if st == "intact"}
    rows = []
    for off in damaged:
        if off not in omap:
            rows.append((str(rel), producer, len(intact), fmt(cal), off, "no_counterpart", "-", ""))
            continue
        truth = memset(*omap[off])
        hit = bool(cal & truth) if cal else False
        rows.append((str(rel), producer, len(intact), fmt(cal), off, "ok", fmt(truth), int(hit)))
    return rows


def main(root: Path, prefix: Path, workers: int) -> None:
    prefix.parent.mkdir(parents=True, exist_ok=True)
    origs = sorted(str(p) for p in (root / "original").rglob("*.pdf"))
    c9 = sorted(str(p) for p in (root / "corrupted").rglob("*_stream_zlib.pdf"))
    with Pool(workers) as pool:
        orows = [r for rs in pool.map(do_original, origs, chunksize=4) for r in rs]
        crows = [r for rs in pool.map(do_c9, [(str(root), f) for f in c9], chunksize=2) for r in rs]
    with open(f"{prefix}.originals.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["file", "offset", "plain_len", "memset"])
        w.writerows([(str(Path(f).relative_to(root)), o, n, m) for f, o, n, m in orows])
    with open(f"{prefix}.c9.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["file", "producer", "intact_streams", "calibrated_memset", "damaged_offset", "counterpart",
                    "truth_memset", "calibration_replays_truth"])
        w.writerows(crows)
    summ = {}
    for prod in ("saveas", "print"):
        o = [r for r in orows if f"/original/{prod}/" in r[0]]
        sets = [frozenset(int(x) for x in r[3].split(",")) if r[3] != "-" else frozenset() for r in o]
        common = frozenset(range(1, 10))
        for s in sets:
            common &= s
        summ[f"originals_{prod}"] = {
            "intact_flate_streams": len(o),
            "replayable_any_memlevel": sum(bool(s) for s in sets),
            "memlevels_common_to_all_replayable": fmt(common) if any(sets) else "-",
            "streams_by_memlevel": {m: sum(m in s for s in sets) for m in range(1, 10)},
            "files": len({r[0] for r in o}),
        }
        c = [r for r in crows if r[1] == prod]
        summ[f"c9_{prod}"] = {
            "files": len({r[0] for r in c}),
            "damaged_streams": len(c),
            "with_counterpart": sum(r[5] == "ok" for r in c),
            "calibrated_memset_per_file": dict(Counter(r[3] for r in {(r[0], r[3]): r for r in c}.values())),
            "intact_streams_per_file_min_median_max": _mmm([r[2] for r in {r[0]: r for r in c}.values()]),
            "counterpart_replayable": sum(r[6] not in ("-",) for r in c if r[5] == "ok"),
            "calibration_replays_counterpart": sum(r[7] == 1 for r in c),
        }
    Path(f"{prefix}.summary.json").write_text(json.dumps(summ, indent=1) + "\n")
    print(json.dumps(summ, indent=1))


def _mmm(xs):
    xs = sorted(xs)
    return [xs[0], xs[len(xs) // 2], xs[-1]] if xs else None


if __name__ == "__main__":
    w = int(sys.argv[sys.argv.index("--workers") + 1]) if "--workers" in sys.argv else 2
    main(Path(sys.argv[1]), Path(sys.argv[2]), w)
