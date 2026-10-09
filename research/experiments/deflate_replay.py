#!/usr/bin/env python3
"""DEFLATE oracle probe on REPDF (OBS-0703): encoder replay, inflate error offset, preflate-rs corrections.

    TMPDIR=<scratch> python3 deflate_replay.py /home/user/dfrc-korea/repdf <out-prefix> --pfprobe <path>

Uses the seeded smoke sample of engine_smoke.py (seed 20260927), whose two C9 picks are both "print"
files, plus two "saveas" C9 files drawn with random.Random(20260927) so that the replay locator is
tried on streams stock zlib can reproduce. A small probe, not a benchmark.
1. Every single-filter /FlateDecode stream in the sampled originals (raw bytes read with pikepdf):
   - zlib header FLEVEL (RFC 1950: 0 fastest .. 3 maximum) and window size (CINFO);
   - encoder replay: does compressobj(level, DEFLATED, wbits, memLevel, strategy) reproduce the exact
     bytes? Grid: level 0..9 x memLevel {8, 9} x strategy {default, filtered}, with stock zlib (the
     Python runtime) and with zlib-ng (python-zlib-ng binding);
   - preflate-rs (pfprobe): parse ok?, corrections size, estimated parameters, exact round trip.
2. Every stream of the sampled C9 (_stream_zlib) files whose raw bytes differ from the original's
   stream with the same object number:
   - the changed byte offsets (compressed domain);
   - stock-zlib inflate outcome, and the input offset at which inflate first raises (fed 1 byte at a time);
   - the first wrong plaintext byte versus the original plaintext;
   - replay locator: recompress the damaged plaintext with the configuration that reproduced the
     original stream; the first offset where that differs from the damaged stream;
   - preflate-rs on the damaged stream: ok?, corrections size versus the original's.
Outputs: <out-prefix>.streams.csv, <out-prefix>.damaged.csv and <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import json
import random
import subprocess
import sys
import tempfile
import zlib
from collections import Counter
from pathlib import Path

import pikepdf
from zlib_ng import zlib_ng

sys.path.insert(0, str(Path(__file__).parent))
from engine_smoke import sample, sha  # noqa: E402

LIBS = {"zlib": zlib, "zlib-ng": zlib_ng}
GRID = [(lv, ml, st) for lv in range(10) for ml in (8, 9) for st in (0, 1)]  # st: 0 default, 1 filtered


def flate_streams(path: Path) -> dict[tuple[int, int], bytes]:
    out = {}
    with pikepdf.open(path) as pdf:
        for obj in pdf.objects:
            if not isinstance(obj, pikepdf.Stream):
                continue
            f = obj.get("/Filter")
            if isinstance(f, pikepdf.Array):
                f = f[0] if len(f) == 1 else None
            if f == pikepdf.Name.FlateDecode and obj.get("/DecodeParms") is None:
                try:
                    out[obj.objgen] = bytes(obj.read_raw_bytes())
                except Exception:  # noqa: BLE001
                    pass
    return out


def replay(raw: bytes, plain: bytes) -> list[str]:
    wbits = ((raw[0] >> 4) + 8) if raw else 15
    hits = []
    for lib, mod in LIBS.items():
        for lv, ml, st in GRID:
            try:
                c = mod.compressobj(lv, zlib.DEFLATED, wbits, ml, st)
                if c.compress(plain) + c.flush() == raw:
                    hits.append(f"{lib}:{lv}:{ml}:{st}")
            except Exception:  # noqa: BLE001
                pass
    return hits


def inflate(raw: bytes) -> tuple[str, bytes, int | None]:
    """(outcome, plaintext produced, input offset where inflate first raised)."""
    d = zlib.decompressobj()
    out = []
    for i in range(len(raw)):
        try:
            out.append(d.decompress(raw[i:i + 1]))
        except zlib.error as e:
            return str(e), b"".join(out), i
    try:
        out.append(d.flush())
    except zlib.error as e:
        return str(e), b"".join(out), len(raw)
    return ("ok" if d.eof else "truncated"), b"".join(out), None


def first_diff(a: bytes, b: bytes) -> int | None:
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return i
    return None if len(a) == len(b) else min(len(a), len(b))


def pfprobe(exe: str, blobs: dict[str, bytes], td: Path) -> dict[str, dict]:
    paths = []
    for k, v in blobs.items():
        p = td / f"{k}.zz"
        p.write_bytes(v)
        paths.append(str(p))
    res = {}
    for i in range(0, len(paths), 200):
        cp = subprocess.run([exe, *paths[i:i + 200]], capture_output=True, timeout=600)
        for ln in cp.stdout.decode("utf-8", "replace").splitlines():
            p, ok, csize, plen, corr, rt, rest = (ln.split("\t") + [""] * 7)[:7]
            res[Path(p).stem] = {"pf_ok": ok == "ok", "pf_corrections": int(corr or 0),
                                 "pf_roundtrip": rt == "true", "pf_info": rest[:300]}
    return res


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    exe = sys.argv[sys.argv.index("--pfprobe") + 1]
    picks = sample(root)
    suf = "_stream_zlib"
    saveas_c9 = sorted((root / "corrupted" / "saveas").rglob(f"*){suf}.pdf"))
    for f in random.Random(20260927).sample(saveas_c9, 2):
        stem, rel = f.name[: -len(f"{suf}.pdf")], f.parent.relative_to(root / "corrupted")
        picks.append({"class": "C9", "corrupted": f, "original": root / "original" / rel / f"{stem}.pdf",
                      "doc": f"{rel.as_posix()}/{stem}", "extra": True})
    originals = {}
    for pk in picks:
        originals.setdefault(pk["doc"], pk["original"])
    srows, drows, blobs = [], [], {}
    orig_streams = {}
    for doc, path in sorted(originals.items()):
        prod = "print" if "(print)" in path.name else "saveas"
        streams = flate_streams(path)
        orig_streams[doc] = streams
        for (num, gen), raw in sorted(streams.items()):
            outcome, plain, _ = inflate(raw)
            key = f"o_{len(srows)}"
            blobs[key] = raw
            hits = replay(raw, plain) if outcome == "ok" else []
            srows.append({"key": key, "doc": doc, "producer": prod, "obj": num, "raw_len": len(raw),
                          "plain_len": len(plain), "inflate": outcome, "cmf": raw[0], "flg": raw[1],
                          "flevel": raw[1] >> 6, "replay_hits": " ".join(hits),
                          "zlib_match": any(h.startswith("zlib:") for h in hits),
                          "zlibng_match": any(h.startswith("zlib-ng:") for h in hits)})
    for pk in picks:
        if pk["class"] != "C9":
            continue
        dmg = flate_streams(pk["corrupted"])
        base = orig_streams[pk["doc"]]
        for og, raw in sorted(dmg.items()):
            o = base.get(og)
            if o is None or o == raw:
                continue
            key = f"d_{len(drows)}"
            blobs[key] = raw
            orow = next(r for r in srows if r["doc"] == pk["doc"] and r["obj"] == og[0])
            flips = [i for i, (a, b) in enumerate(zip(o, raw)) if a != b]
            outcome, plain_d, err_at = inflate(raw)
            _, plain_o, _ = inflate(o)
            loc = None
            hit = (orow["replay_hits"].split() or [None])[0]
            if hit:
                lib, lv, ml, st = hit.split(":")
                c = LIBS[lib].compressobj(int(lv), zlib.DEFLATED, ((raw[0] >> 4) + 8), int(ml), int(st))
                loc = first_diff(c.compress(plain_d) + c.flush(), raw)
            drows.append({"key": key, "orig_key": orow["key"], "doc": pk["doc"], "producer": orow["producer"],
                          "obj": og[0], "raw_len": len(raw), "len_equal": len(raw) == len(o),
                          "changed_offsets": " ".join(map(str, flips[:10])), "n_changed": len(flips),
                          "inflate": outcome, "inflate_error_at": err_at,
                          "first_wrong_plain_byte": first_diff(plain_d, plain_o),
                          "plain_len_damaged": len(plain_d), "plain_len_original": len(plain_o),
                          "replay_config": hit, "replay_first_mismatch": loc,
                          "replay_minus_flip": (loc - flips[0]) if loc is not None and flips else None})
    with tempfile.TemporaryDirectory(prefix="pf_") as td:
        pf = pfprobe(exe, blobs, Path(td))
    for r in srows + drows:
        r.update(pf.get(r["key"], {"pf_ok": None}))
    for r in drows:
        r["pf_corrections_original"] = pf.get(r["orig_key"], {}).get("pf_corrections")
    out.parent.mkdir(parents=True, exist_ok=True)
    for name, rows in (("streams", srows), ("damaged", drows)):
        with open(out.parent / f"{out.name}.{name}.csv", "w", newline="") as f:
            w = csv.DictWriter(f, list(rows[0]))
            w.writeheader()
            w.writerows(rows)
    summ = {"kind": "small probe on the seeded smoke sample, not a benchmark",
            "tools": {"zlib": zlib.ZLIB_RUNTIME_VERSION, "zlib-ng": zlib_ng.ZLIBNG_VERSION,
                      "pikepdf": pikepdf.__version__, "preflate-rs": "0.7.6 (pfprobe)"},
            "streams_by_producer": {}}
    for prod in ("print", "saveas"):
        rs = [r for r in srows if r["producer"] == prod]
        summ["streams_by_producer"][prod] = {
            "n": len(rs), "flevel": dict(Counter(r["flevel"] for r in rs)),
            "zlib_exact_replay": sum(r["zlib_match"] for r in rs),
            "zlibng_exact_replay": sum(r["zlibng_match"] for r in rs),
            "top_replay_configs": Counter(h for r in rs for h in r["replay_hits"].split()).most_common(6),
            "preflate_ok": sum(bool(r["pf_ok"]) for r in rs),
            "preflate_roundtrip": sum(bool(r.get("pf_roundtrip")) for r in rs),
            "preflate_corrections_median": sorted(r.get("pf_corrections") or 0 for r in rs)[len(rs) // 2] if rs else None}
    summ["damaged"] = {
        "n": len(drows), "inflate_outcome": dict(Counter(r["inflate"] for r in drows)),
        "n_changed_bytes": dict(Counter(r["n_changed"] for r in drows)),
        "with_replay_config": sum(bool(r["replay_config"]) for r in drows),
        "replay_mismatch_minus_flip": [r["replay_minus_flip"] for r in drows if r["replay_minus_flip"] is not None],
        "rows": [{k: r[k] for k in ("doc", "producer", "obj", "raw_len", "changed_offsets", "inflate", "inflate_error_at",
                                    "first_wrong_plain_byte", "replay_config", "replay_first_mismatch", "pf_ok",
                                    "pf_corrections", "pf_corrections_original")} for r in drows]}
    summ["inputs"] = [{"path": str(p.relative_to(root)), "sha256": sha(p)} for p in sorted(originals.values())] + \
        [{"path": str(pk["corrupted"].relative_to(root)), "sha256": sha(pk["corrupted"])} for pk in picks if pk["class"] == "C9"]
    (out.parent / f"{out.name}.summary.json").write_text(json.dumps(summ, indent=1, default=str) + "\n")
    print(json.dumps({k: v for k, v in summ.items() if k != "inputs"}, indent=1, default=str)[:6000])


if __name__ == "__main__":
    main()
