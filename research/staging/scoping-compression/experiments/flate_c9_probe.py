#!/usr/bin/env python3
"""Probe how REPDF's C9 ("stream zlib") damage looks to a zlib decoder, and how many DEFLATE
blocks PDF Flate streams have (which bounds what block-level resynchronization can recover).

    python3 research/staging/scoping-compression/experiments/flate_c9_probe.py \
        /home/user/dfrc-korea/repdf \
        research/staging/scoping-compression/experiments/results/flate_c9_probe

Inputs: the REPDF corpus (SRC-0002, commit e547d4d), originals + `*_stream_zlib.pdf` files.
Outputs: <out>.csv (one row per damaged Flate stream), <out>.blocks.csv (one row per Flate
stream of every original that has a C9 counterpart), <out>.summary.json.

Per damaged stream (a stream body of the original that contains >= 1 changed byte; sizes are
unchanged in C9, so offsets align):
  - changed bytes in the body and offset of the first one (compressed domain);
  - what a plain zlib inflate does on the damaged body: 'data-error' (decoder rejects the
    bitstream: invalid code / distance too far / ...), 'check-only' (decodes to the end, only the
    Adler-32 check fails), 'ok' (decodes and passes), 'incomplete' (runs out of input);
  - detection latency: compressed bytes consumed past the first changed byte when the error fired;
  - emitted = decoded bytes before the error; verbatim = common prefix with the original's
    decoded bytes; silent_wrong = emitted - verbatim (bytes a naive "decode until error"
    salvage outputs that are NOT the original's);
  - blocks_total / blocks_after: DEFLATE blocks in the original stream, and blocks that start
    after the first changed byte (the most that block-level resync, as in Brown 2011 or pugz,
    could reach).
Block boundaries come from zlib's own inflate(Z_BLOCK) via ctypes (the method zlib's zran.c uses).
"""

from __future__ import annotations

import csv
import ctypes
import ctypes.util
import json
import re
import statistics
import sys
import zlib
from collections import Counter
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)
Z_OK, Z_STREAM_END, Z_BUF_ERROR, Z_DATA_ERROR, Z_NEED_DICT = 0, 1, -5, -3, 2
Z_NO_FLUSH, Z_BLOCK = 0, 5


class ZStream(ctypes.Structure):
    _fields_ = [("next_in", ctypes.c_void_p), ("avail_in", ctypes.c_uint), ("total_in", ctypes.c_ulong),
                ("next_out", ctypes.c_void_p), ("avail_out", ctypes.c_uint), ("total_out", ctypes.c_ulong),
                ("msg", ctypes.c_char_p), ("state", ctypes.c_void_p),
                ("zalloc", ctypes.c_void_p), ("zfree", ctypes.c_void_p), ("opaque", ctypes.c_void_p),
                ("data_type", ctypes.c_int), ("adler", ctypes.c_ulong), ("reserved", ctypes.c_ulong)]


LIBZ = ctypes.CDLL(ctypes.util.find_library("z"))
LIBZ.zlibVersion.restype = ctypes.c_char_p
LIBZ.inflateInit2_.argtypes = [ctypes.POINTER(ZStream), ctypes.c_int, ctypes.c_char_p, ctypes.c_int]
LIBZ.inflate.argtypes = [ctypes.POINTER(ZStream), ctypes.c_int]
LIBZ.inflateEnd.argtypes = [ctypes.POINTER(ZStream)]
ZVER = LIBZ.zlibVersion()


def inflate(data: bytes, block_mode: bool) -> dict:
    """Inflate a zlib (FlateDecode) stream. Returns output, return code, zlib message, bytes
    consumed, and (block_mode) the compressed byte offsets at which each DEFLATE block ended."""
    s = ZStream()
    if LIBZ.inflateInit2_(ctypes.byref(s), 15, ZVER, ctypes.sizeof(ZStream)) != Z_OK:
        raise RuntimeError("inflateInit2 failed")
    inbuf = ctypes.create_string_buffer(data, len(data))
    s.next_in, s.avail_in = ctypes.cast(inbuf, ctypes.c_void_p), len(data)
    out, chunk = bytearray(), 1 << 16
    obuf = ctypes.create_string_buffer(chunk)
    block_ends: list[int] = []
    ret = Z_OK
    while True:
        s.next_out, s.avail_out = ctypes.cast(obuf, ctypes.c_void_p), chunk
        before_in = s.total_in
        ret = LIBZ.inflate(ctypes.byref(s), Z_BLOCK if block_mode else Z_NO_FLUSH)
        out += obuf.raw[: chunk - s.avail_out]
        if block_mode and ret in (Z_OK, Z_STREAM_END) and (s.data_type & 128) and s.total_out > 0:
            if not block_ends or block_ends[-1] != s.total_in:
                block_ends.append(s.total_in)
        if ret == Z_STREAM_END or ret < 0 or ret == Z_NEED_DICT:
            break
        if s.avail_in == 0 and s.avail_out != 0:
            ret = Z_BUF_ERROR            # input exhausted before end of stream
            break
        if s.total_in == before_in and s.avail_out == chunk and ret == Z_OK and not block_mode:
            break
    msg = s.msg.decode() if s.msg else None
    res = {"out": bytes(out), "ret": ret, "msg": msg, "consumed": s.total_in, "block_ends": block_ends}
    LIBZ.inflateEnd(ctypes.byref(s))
    return res


def body_spans(pdf: bytes) -> list[tuple[int, int]]:
    return [(m.start(1), m.end(1)) for m in STREAM_RE.finditer(pdf)]


def trim_eol(body: bytes) -> bytes:
    return body[:-2] if body.endswith(b"\r\n") else body[:-1] if body.endswith((b"\n", b"\r")) else body


def outcome(res: dict) -> str:
    if res["ret"] == Z_STREAM_END:
        return "ok"
    if res["ret"] == Z_DATA_ERROR:
        return "check-only" if res["msg"] == "incorrect data check" else "data-error"
    return "incomplete"


def common_prefix(a: bytes, b: bytes) -> int:
    n = min(len(a), len(b))
    i = 0
    while i < n and a[i] == b[i]:
        i += 1
    return i


def stats(vals):
    vals = [v for v in vals if v is not None]
    if not vals:
        return None
    return {"n": len(vals), "min": min(vals), "median": statistics.median(vals), "max": max(vals)}


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    rows, block_rows = [], []
    for f in sorted((corpus / "corrupted").rglob("*_stream_zlib.pdf")):
        mode, kind = f.parent.parent.name, f.parent.name
        base = f.name[: -len("_stream_zlib.pdf")]
        orig = corpus / "original" / mode / kind / f"{base}.pdf"
        a, b = orig.read_bytes(), f.read_bytes()
        if len(a) != len(b):
            continue
        for idx, (s, e) in enumerate(body_spans(a)):
            ob = trim_eol(a[s:e])
            ores = inflate(ob, block_mode=True)
            if ores["ret"] != Z_STREAM_END:
                continue                                  # not a (valid) Flate stream in the original
            nblocks = len(ores["block_ends"])
            block_rows.append({"file": str(f.relative_to(corpus)), "mode": mode, "stream": idx,
                               "comp_len": len(ob), "decomp_len": len(ores["out"]), "blocks": nblocks})
            cb = trim_eol(b[s:e])
            diffs = [i for i in range(len(ob)) if ob[i] != cb[i]]
            if not diffs:
                continue
            first = diffs[0]
            cres = inflate(cb, block_mode=False)
            oc = outcome(cres)
            verbatim = common_prefix(ores["out"], cres["out"])
            starts = [0] + ores["block_ends"][:-1]           # block i starts where block i-1 ended
            rows.append({
                "file": str(f.relative_to(corpus)), "mode": mode, "kind": kind, "stream": idx,
                "comp_len": len(ob), "decomp_len": len(ores["out"]), "changed_bytes": len(diffs),
                "first_change": first, "first_change_frac": round(first / len(ob), 4),
                "outcome": oc, "zlib_msg": cres["msg"] or "",
                "detect_latency": (cres["consumed"] - first) if oc in ("data-error", "check-only") else "",
                "emitted": len(cres["out"]), "verbatim": verbatim,
                "silent_wrong": max(0, len(cres["out"]) - verbatim),
                "verbatim_frac": round(verbatim / len(ores["out"]), 4) if ores["out"] else "",
                "blocks_total": nblocks, "blocks_after": sum(1 for st in starts if st > first),
            })

    for path, rs in ((f"{out}.csv", rows), (f"{out}.blocks.csv", block_rows)):
        with open(path, "w", newline="") as fh:
            w = csv.DictWriter(fh, fieldnames=list(rs[0]))
            w.writeheader()
            w.writerows(rs)

    nb = Counter(r["blocks"] for r in block_rows)
    summary = {
        "zlib_version": ZVER.decode(),
        "flate_streams_in_originals": len(block_rows),
        "blocks_per_stream": {"single_block": nb[1], "two_to_five": sum(v for k, v in nb.items() if 2 <= k <= 5),
                              "more_than_five": sum(v for k, v in nb.items() if k > 5),
                              "stats": stats([r["blocks"] for r in block_rows])},
        "damaged_streams": len(rows),
        "damaged_files": len({r["file"] for r in rows}),
        "changed_bytes_per_damaged_stream": dict(sorted(Counter(r["changed_bytes"] for r in rows).items())),
        "outcome": dict(Counter(r["outcome"] for r in rows)),
        "zlib_msg": dict(Counter(r["zlib_msg"] for r in rows)),
        "detect_latency_bytes": stats([r["detect_latency"] for r in rows if r["detect_latency"] != ""]),
        "silent_wrong_bytes": stats([r["silent_wrong"] for r in rows]),
        "streams_with_silent_wrong_output": sum(1 for r in rows if r["silent_wrong"] > 0),
        "verbatim_frac": stats([r["verbatim_frac"] for r in rows if r["verbatim_frac"] != ""]),
        "damaged_in_single_block_stream": sum(1 for r in rows if r["blocks_total"] == 1),
        "damaged_with_zero_blocks_after": sum(1 for r in rows if r["blocks_after"] == 0),
        "blocks_after": stats([r["blocks_after"] for r in rows]),
    }
    Path(f"{out}.summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
