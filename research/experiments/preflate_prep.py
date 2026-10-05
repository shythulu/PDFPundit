#!/usr/bin/env python3
"""Sample REPDF original Flate streams per producer for preflate_probe/ (does Microsoft's pure-Rust preflate-rs
predict the producer's DEFLATE token stream from the plaintext alone, i.e. could it serve as a pure-Rust
encoder-replay oracle where zlib-rs and miniz_oxide fail (OBS-0902)?).

Writes <out>/<name>.z (the exact zlib stream, header and Adler trailer included, trailing PDF bytes cut) and
<out>/index.tsv: name, producer, zlib_replay (1 if stock zlib level 6 reproduces it under some memLevel 1-9 and
default or filtered strategy, else 0), file, offset. Summarise the probe's TSV with --summarize.

    python3 preflate_prep.py /home/user/dfrc-korea/repdf <outdir> [--max 400]
    python3 preflate_prep.py --summarize <probe.tsv> <replay.tsv> <summary.json>

probe.tsv is the output of `preflate_probe <dir>`, replay.tsv that of `preflate_probe <dir> --replay`.
"""
import json
import re
import statistics
import sys
import zlib
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def replayable(raw: bytes, z: bytes) -> int:
    for mem in (8, 7, 9, 6, 5, 4, 3, 2, 1):
        for strat in (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED):
            c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, strat)
            if c.compress(raw) + c.flush() == z:
                return 1
    return 0


def prep(corpus: Path, out: Path, cap: int) -> None:
    out.mkdir(parents=True, exist_ok=True)
    rows = []
    for producer in ("saveas", "print"):
        n = 0
        for f in sorted((corpus / "original" / producer).rglob("*.pdf")):
            for m in STREAM_RE.finditer(f.read_bytes()):
                body = m.group(1)
                d = zlib.decompressobj()
                try:
                    raw = d.decompress(body)
                except zlib.error:
                    continue
                if not d.eof:
                    continue
                z = body[: len(body) - len(d.unused_data)]
                name = f"{len(rows):04d}"
                (out / f"{name}.z").write_bytes(z)
                rows.append(f"{name}\t{producer}\t{replayable(raw, z)}\t{f.relative_to(corpus)}\t{m.start(1)}")
                n += 1
                if n >= cap:
                    break
            if n >= cap:
                break
    (out / "index.tsv").write_text("\n".join(rows) + "\n")
    print(f"written {len(rows)}")


def summarize(tsv: Path, replay_tsv: Path, out: Path) -> None:
    lines = [l.split("\t") for l in tsv.read_text().splitlines() if l and not l.startswith("#")]
    hdr, rows = lines[0], [dict(zip(lines[0], l)) for l in lines[1:]]
    res = {}
    for prod in ("saveas", "print"):
        for rep in ("1", "0"):
            g = [r for r in rows if r["producer"] == prod and r["zlib_replay"] == rep]
            if not g:
                continue
            ok = [r for r in g if r["status"] == "ok"]
            cab = [int(r["cabac_bytes"]) for r in ok]
            comp = sum(int(r["comp_bytes"]) for r in ok)
            res[f"{prod}_zlib_replay{rep}"] = {
                "streams": len(g),
                "preflate_ok_and_recreated_exact": sum(r["recreated_exact"] == "1" for r in ok),
                "errors": sorted({r["status"] for r in g if r["status"] != "ok"}),
                "cabac_bytes_min_median_max": [min(cab), statistics.median(cab), max(cab)] if cab else None,
                "cabac_bytes_total": sum(cab),
                "corrections_total_bytes": sum(int(r["corr_bytes"]) for r in ok),
                "compressed_bytes_total": comp,
                "streams_cabac_le_4_bytes": sum(c <= 4 for c in cab),
                "zlib_compatible_true": sum(r["zlib_compatible"] == "true" for r in ok),
                "params_top": _top([r["params"] for r in ok]),
            }
    rl = replay_tsv.read_text().splitlines()
    rrows = [dict(zip(rl[0].split("\t"), l.split("\t"))) for l in rl[1:] if l and not l.startswith("#")]
    for prod in ("saveas", "print"):
        g = [r for r in rrows if r["producer"] == prod]
        mems = {}
        for r in g:
            mems[r["zlib6_exact_mem"]] = mems.get(r["zlib6_exact_mem"], 0) + 1
        res[f"{prod}_zero_correction_replay"] = {
            "streams": len(g),
            "stock_zlib_replayable": sum(r["zlib_replay"] == "1" for r in g),
            "zlib6_params_exact": sum(r["zlib6_exact_mem"] not in ("0", "err") for r in g),
            "zlib6_exact_by_memlevel": mems,
            "own_estimated_params_exact": sum(r["own_params_exact"] == "1" for r in g),
            "analysis_errors": sum(r["zlib6_exact_mem"] == "err" for r in g),
        }
    res["preflate_panics_caught"] = [l for l in rl if l.startswith("# preflate-rs panics")]
    out.write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps(res, indent=1))


def _top(xs, k=4):
    c = {}
    for x in xs:
        c[x] = c.get(x, 0) + 1
    return sorted(c.items(), key=lambda t: -t[1])[:k]


if __name__ == "__main__":
    if sys.argv[1] == "--summarize":
        summarize(Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]))
    else:
        cap = int(sys.argv[sys.argv.index("--max") + 1]) if "--max" in sys.argv else 400
        prep(Path(sys.argv[1]), Path(sys.argv[2]), cap)
