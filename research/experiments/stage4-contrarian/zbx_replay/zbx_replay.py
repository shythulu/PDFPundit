#!/usr/bin/env python3
"""Can a pure-Rust, BSD-3 zlib port (zlib-bitexact-rs 0.131.1, parameterized by patch_zbx.py) replace preflate-rs as
WP-3.3's encoder-replay oracle? Samples REPDF original Flate streams with the same rule as
research/experiments/preflate_prep.py (first N streams per producer, files sorted), so stream names match
results/preflate_replay.tsv (OBS-0908).

    python3 zbx_replay.py prep <repdf_root> <sample_dir> [--max 400]
        writes <name>.raw (inflated plaintext), <name>.z (exact zlib stream) and index.tsv:
        name, producer, stock_zlib_exact_mems (level 6, default strategy, memLevel 1..9), file, offset
    python3 zbx_replay.py compare <sample_dir> <rust.tsv> <summary.json> [--preflate <preflate_replay.tsv>]
    python3 zbx_replay.py time-stock <sample_dir>
        wall time of stock C zlib (Python's zlib module) for the same 9 memLevel encodes per stream, per producer
"""
import json
import re
import sys
import zlib
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def stock_mems(raw: bytes, z: bytes) -> list:
    hits = []
    for mem in range(1, 10):
        c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, zlib.Z_DEFAULT_STRATEGY)
        if c.compress(raw) + c.flush() == z:
            hits.append(mem)
    return hits


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
                (out / f"{name}.raw").write_bytes(raw)
                mems = ",".join(map(str, stock_mems(raw, z))) or "-"
                rows.append(f"{name}\t{producer}\t{mems}\t{f.relative_to(corpus)}\t{m.start(1)}")
                n += 1
                if n >= cap:
                    break
            if n >= cap:
                break
    (out / "index.tsv").write_text("\n".join(rows) + "\n")
    print(f"written {len(rows)}")


def compare(sample: Path, rust_tsv: Path, out: Path, preflate_tsv) -> None:
    idx = {}
    for l in (sample / "index.tsv").read_text().splitlines():
        if l:
            name, prod, mems, f, off = l.split("\t")
            idx[name] = (prod, mems)
    rl = rust_tsv.read_text().splitlines()
    rust = {r[0]: (r[1], int(r[2])) for r in (x.split("\t") for x in rl[1:] if x)}
    pf = {}
    if preflate_tsv:
        pl = preflate_tsv.read_text().splitlines()
        hdr = pl[0].split("\t")
        for x in pl[1:]:
            if x and not x.startswith("#"):
                d = dict(zip(hdr, x.split("\t")))
                pf[d["name"]] = d["zlib6_exact_mem"]
    res = {}
    for prod in ("saveas", "print"):
        names = [n for n, (p, _) in idx.items() if p == prod]
        stock_ok = [n for n in names if idx[n][1] != "-"]
        rust_ok = [n for n in names if rust[n][0] != "-"]
        agree = sum(idx[n][1] == rust[n][0] for n in names)
        r = {
            "streams": len(names),
            "stock_zlib_exact_any_mem": len(stock_ok),
            "rust_exact_any_mem": len(rust_ok),
            "rust_mem_set_equals_stock_mem_set": agree,
            "rust_exact_but_stock_not": sorted(set(rust_ok) - set(stock_ok)),
            "stock_exact_but_rust_not": sorted(set(stock_ok) - set(rust_ok)),
            "rust_micros_total_9_mems": sum(rust[n][1] for n in names),
            "plaintext_bytes_total": sum((sample / f"{n}.raw").stat().st_size for n in names),
        }
        if pf:
            pf_ok = [n for n in names if pf.get(n) not in (None, "0", "err")]
            r["preflate_rs_zero_correction_exact"] = len(pf_ok)
            r["preflate_failed_rust_exact"] = sorted(n for n in rust_ok if n not in pf_ok)
        res[prod] = r
    out.write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps(res, indent=1))


def time_stock(sample: Path) -> None:
    import time
    rows = [l.split("\t") for l in (sample / "index.tsv").read_text().splitlines() if l]
    for prod in ("saveas", "print"):
        raws = [(sample / f"{r[0]}.raw").read_bytes() for r in rows if r[1] == prod]
        t = time.perf_counter()
        for raw in raws:
            for mem in range(1, 10):
                c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, zlib.Z_DEFAULT_STRATEGY)
                c.compress(raw) + c.flush()
        print(f"{prod}\tstock_zlib_{zlib.ZLIB_RUNTIME_VERSION}_seconds_9_mems\t{time.perf_counter() - t:.2f}")


if __name__ == "__main__":
    a = sys.argv
    if a[1] == "time-stock":
        time_stock(Path(a[2]))
    elif a[1] == "prep":
        cap = int(a[a.index("--max") + 1]) if "--max" in a else 400
        prep(Path(a[2]), Path(a[3]), cap)
    else:
        pft = Path(a[a.index("--preflate") + 1]) if "--preflate" in a else None
        compare(Path(a[2]), Path(a[3]), Path(a[4]), pft)
