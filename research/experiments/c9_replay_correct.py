#!/usr/bin/env python3
"""Can REPDF's C9 damage (exactly one changed byte per damaged Flate stream, OBS-0300) be *corrected*,
not just salvaged, and is the correction unique? Tests three cheap ideas from the wildcard scout:

  A. Replay localisation (GAP-150): re-compress the damaged stream's decoded output with stock zlib
     (level 6, memLevel 1-9, strategy default/filtered) and compare the two DEFLATE token sequences
     (literals, matches, block type; not the Huffman tables, which depend on the whole block).
     Everything before the damage agrees, so the damage lies at or before the first divergence b0.
  B. Adler-32 algebra: when the damaged stream still inflates to the end (Adler-only failure), the
     stored and recomputed Adler-32 differ by (dA, dB). If one literal token changed by delta, and
     the literal is copied c times by later matches to output positions k_i, then dA = c*delta and
     dB = delta*sum(n - k_i) (mod 65521). Every literal token is tested in O(n); the ones that fit
     give candidate compressed-byte positions. This needs no encoder model, so it also applies to
     non-replayable (print) streams.
  C. Candidate search and verification: every single-byte substitution at the candidate positions
     ([b0-256, b0+8], the Adler candidates +-1 byte, and the 4-byte Adler trailer) is inflated; a
     candidate "passes Adler" when zlib inflates it to the end with a valid Adler-32, and "passes
     replay" (GAP-154) when stock zlib re-compression of its output reproduces it byte-for-byte.
For scoring only, the original stream gives the true position and value. An "oracle window"
(true position +-32 bytes) measures how ambiguous Adler-32 alone is if localisation were perfect,
and short streams (<= --full-max compressed bytes) are searched exhaustively (every position).

    python3 c9_replay_correct.py /home/user/dfrc-korea/repdf results/c9_replay_correct \
        [--full-max 1536] [--max-comp 32768] [--every 1] [--workers 2]

Outputs <out>.csv (one row per damaged stream) and <out>.summary.json.
The DEFLATE token parser below is written from RFC 1951 (SRC-0312); no third-party code.
"""
from __future__ import annotations

import csv
import json
import re
import statistics
import sys
import time
import zlib
from collections import Counter
from multiprocessing import Pool
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)

LEN_BASE = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
            131, 163, 195, 227, 258]
LEN_EXTRA = [0] * 8 + [1] * 4 + [2] * 4 + [3] * 4 + [4] * 4 + [5] * 4 + [0]
DIST_BASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
             2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577]
DIST_EXTRA = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13]
CL_ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]


class DeflateError(Exception):
    pass


class Huff:
    """Canonical Huffman code as per-length counts plus symbols sorted by (length, value)."""

    def __init__(self, lengths: list[int]):
        self.count = [0] * 16
        for ln in lengths:
            self.count[ln] += 1
        self.count[0] = 0
        offs = [0] * 16
        for i in range(1, 16):
            offs[i] = offs[i - 1] + self.count[i - 1] * (i > 1)
        # symbols ordered by code length, then symbol value
        self.symbol = [s for ln in range(1, 16) for s, l2 in enumerate(lengths) if l2 == ln]


class Bits:
    def __init__(self, data: bytes, bytepos: int):
        self.data, self.pos = data, bytepos * 8

    def bit(self) -> int:
        p = self.pos
        try:
            b = (self.data[p >> 3] >> (p & 7)) & 1
        except IndexError:
            raise DeflateError("out of input") from None
        self.pos = p + 1
        return b

    def bits(self, n: int) -> int:
        v = 0
        for i in range(n):
            v |= self.bit() << i
        return v

    def decode(self, h: Huff) -> int:
        code = first = index = 0
        for ln in range(1, 16):
            code |= self.bit()
            c = h.count[ln]
            if code - c < first:
                return h.symbol[index + (code - first)]
            index += c
            first = (first + c) << 1
            code <<= 1
        raise DeflateError("invalid Huffman code")


FIXED_LIT = Huff([8] * 144 + [9] * 112 + [7] * 24 + [8] * 8)
FIXED_DIST = Huff([5] * 30)


def tokens(stream: bytes) -> tuple[list[tuple[int, tuple]], bytes, str]:
    """Parse a zlib stream into (bit position, token) pairs and the decoded bytes.
    Tokens: ('B', final, type[, lengths]) block header, ('L', byte), ('M', length, distance).
    Stops at the first error; returns what was parsed and the error text ('' when clean)."""
    toks: list[tuple[int, tuple]] = []
    out = bytearray()
    br = Bits(stream, 2)
    try:
        while True:
            start = br.pos
            final, btype = br.bit(), br.bits(2)
            if btype == 0:
                br.pos = (br.pos + 7) & ~7
                ln, nln = br.bits(16), br.bits(16)
                if ln != (~nln & 0xFFFF):
                    raise DeflateError("stored length mismatch")
                toks.append((start, ("B", final, 0, ln)))
                for _ in range(ln):
                    out.append(br.bits(8))
            elif btype in (1, 2):
                if btype == 1:
                    lit, dist = FIXED_LIT, FIXED_DIST
                    toks.append((start, ("B", final, 1)))
                else:
                    hlit, hdist, hclen = br.bits(5) + 257, br.bits(5) + 1, br.bits(4) + 4
                    cl = [0] * 19
                    for i in range(hclen):
                        cl[CL_ORDER[i]] = br.bits(3)
                    clh = Huff(cl)
                    lens: list[int] = []
                    while len(lens) < hlit + hdist:
                        sym = br.decode(clh)
                        if sym < 16:
                            lens.append(sym)
                        elif sym == 16:
                            if not lens:
                                raise DeflateError("repeat with no previous length")
                            lens += [lens[-1]] * (3 + br.bits(2))
                        elif sym == 17:
                            lens += [0] * (3 + br.bits(3))
                        else:
                            lens += [0] * (11 + br.bits(7))
                    if len(lens) > hlit + hdist:
                        raise DeflateError("too many code lengths")
                    lit, dist = Huff(lens[:hlit]), Huff(lens[hlit:])
                    toks.append((start, ("B", final, 2, tuple(lens))))
                while True:
                    p = br.pos
                    sym = br.decode(lit)
                    if sym < 256:
                        out.append(sym)
                        toks.append((p, ("L", sym)))
                    elif sym == 256:
                        toks.append((p, ("E",)))
                        break
                    else:
                        sym -= 257
                        if sym >= 29:
                            raise DeflateError("invalid length symbol")
                        length = LEN_BASE[sym] + br.bits(LEN_EXTRA[sym])
                        ds = br.decode(dist)
                        if ds >= 30:
                            raise DeflateError("invalid distance symbol")
                        d = DIST_BASE[ds] + br.bits(DIST_EXTRA[ds])
                        if d > len(out):
                            raise DeflateError("distance too far back")
                        for _ in range(length):
                            out.append(out[-d])
                        toks.append((p, ("M", length, d)))
            else:
                raise DeflateError("invalid block type")
            if final:
                break
    except DeflateError as e:
        return toks, bytes(out), str(e)
    return toks, bytes(out), ""


def trim(b: bytes) -> bytes:
    return b[:-2] if b.endswith(b"\r\n") else b[:-1] if b.endswith((b"\n", b"\r")) else b


REPLAY_SETTINGS = [(m, s) for m in (8, 1, 2, 3, 4, 5, 6, 7, 9) for s in (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED)]


def zcompress(data: bytes, mem: int, strat: int) -> bytes:
    c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, strat)
    return c.compress(data) + c.flush()


def replays(data: bytes, cand: bytes) -> str:
    for mem, strat in REPLAY_SETTINGS:
        if zcompress(data, mem, strat) == cand:
            return f"{mem}/{strat}"
    return ""


def _key(t: tuple) -> tuple:
    """Token identity for comparison: a dynamic block's Huffman code lengths depend on the whole
    block's symbol counts, so a changed byte anywhere in the block alters them; compare only the
    block's final flag and type, plus every literal, match and end-of-block token."""
    return t[:3] if t[0] == "B" else t


def localise(damaged: bytes) -> tuple[int, str, str]:
    """Byte offset (in the zlib stream) of the first token where the damaged stream departs from the
    best-agreeing stock-zlib replay of its own decoded output; also the replay setting and parse error."""
    toks, out, err = tokens(damaged)
    key = [_key(t) for _, t in toks]
    best_j, best_set = -1, ""
    for mem, strat in REPLAY_SETTINGS:
        rt, _, _ = tokens(zcompress(out, mem, strat))
        rkey = [_key(t) for _, t in rt]
        j = 0
        n = min(len(key), len(rkey))
        while j < n and key[j] == rkey[j]:
            j += 1
        if j > best_j:
            best_j, best_set = j, f"{mem}/{strat}"
    b0 = toks[best_j][0] // 8 if best_j < len(toks) else len(damaged) - 4
    return b0, best_set, err


def search(damaged: bytes, positions: list[int]) -> list[tuple[int, int, str]]:
    """All single-byte substitutions at the given positions that inflate with a valid Adler-32.
    Returns (position, value, replay setting or '')."""
    hits = []
    d = zlib.decompressobj()
    prefix_out: list[bytes] = []
    fed = 0
    for p in sorted(set(positions)):
        try:
            prefix_out.append(d.decompress(damaged[fed:p]))
        except zlib.error:
            break            # the damaged prefix is already undecodable: later positions cannot fix it
        fed = p
        rest = damaged[p + 1:]
        orig = damaged[p]
        for v in range(256):
            if v == orig:
                continue
            c = d.copy()
            try:
                tail = c.decompress(bytes([v]) + rest)
                tail += c.flush()
            except zlib.error:
                continue
            if not c.eof:
                continue
            cand = damaged[:p] + bytes([v]) + rest
            hits.append((p, v, replays(b"".join(prefix_out) + tail, cand)))
    return hits


MOD = 65521


def adler_candidates(damaged: bytes) -> tuple[list[int], int]:
    """Compressed-byte positions of literal tokens whose change by one delta would explain the
    Adler-32 mismatch (idea B). Returns (positions, number of fitting literal tokens)."""
    toks, out, err = tokens(damaged)
    if err or len(damaged) < 6:
        return [], 0
    n = len(out)
    stored = int.from_bytes(damaged[-4:], "big")
    comp = zlib.adler32(out)
    da = ((stored & 0xFFFF) - (comp & 0xFFFF)) % MOD
    db = ((stored >> 16) - (comp >> 16)) % MOD
    if da == 0 and db == 0:
        return [], 0
    # source literal of every output byte, following match copies back to the literal they copy
    src = [0] * n
    lit_bit: dict[int, int] = {}
    o = 0
    for bitpos, t in toks:
        if t[0] == "L":
            src[o] = o
            lit_bit[o] = bitpos
            o += 1
        elif t[0] == "M":
            ln, dist = t[1], t[2]
            for _ in range(ln):
                src[o] = src[o - dist]
                o += 1
        elif t[0] == "B" and t[2] == 0:
            for _ in range(t[3]):          # stored block: bytes are literal but have no token
                src[o] = -1
                o += 1
    cnt: dict[int, int] = {}
    wsum: dict[int, int] = {}
    for i, s in enumerate(src):
        if s >= 0:
            cnt[s] = cnt.get(s, 0) + 1
            wsum[s] = (wsum.get(s, 0) + (n - i)) % MOD
    pos: set[int] = set()
    fits = 0
    for k, c in cnt.items():
        for delta in range(-255, 256):
            if delta == 0 or (c * delta) % MOD != da:
                continue
            if (delta * wsum[k]) % MOD == db and 0 <= out[k] + delta <= 255:
                fits += 1
                b = lit_bit[k] // 8
                pos.update(q for q in (b - 1, b, b + 1, b + 2) if 2 <= q < len(damaged))
    return sorted(pos), fits


def run_one(job: dict) -> dict:
    a, b = Path(job["orig"]).read_bytes(), Path(job["dam"]).read_bytes()
    s, e = job["span"]
    ob, db = trim(a[s:e]), trim(b[s:e])
    diffs = [i for i in range(len(ob)) if ob[i] != db[i]]
    pstar, vstar = diffs[0], ob[diffs[0]]
    n = len(db)
    t0 = time.time()
    b0, setting, err = localise(db)
    t_loc = time.time() - t0
    t0 = time.time()
    apos, afits = adler_candidates(db)
    t_adl = time.time() - t0
    rset = set(range(max(2, b0 - 256), min(n, b0 + 9)))
    trailer = set(range(max(2, n - 4), n))
    method = rset | set(apos) | trailer
    oset = set(range(max(0, pstar - 32), min(n, pstar + 33)))
    full = n <= job["full_max"]
    t0 = time.time()
    hits = search(db, list(range(n)) if full else sorted(method | oset))
    t_search = time.time() - t0

    def arm(ps):
        hs = [x for x in hits if x[0] in ps]
        rep = [x for x in hs if x[2]]
        ok = lambda xs: any(x[0] == pstar and x[1] == vstar for x in xs)
        return {"adler": len(hs), "adler_correct": ok(hs), "replay": len(rep), "replay_correct": ok(rep)}

    m_arm, o_arm = arm(method), arm(oset)
    f_arm = arm(set(range(n))) if full else None
    orig_replay = replays(zlib.decompress(ob), ob)
    return {
        "file": job["rel"], "mode": job["mode"], "stream_start": s, "comp_len": n, "true_pos": pstar,
        "orig_replayable": orig_replay or "no", "parse_error": err, "b0": b0, "loc_offset": pstar - b0,
        "loc_setting": setting, "true_in_replay_window": pstar in rset,
        "adler_fits": afits, "adler_positions": len(apos), "true_in_adler_positions": pstar in apos,
        "true_in_trailer": pstar in trailer, "true_in_method_set": pstar in method,
        "method_positions": len(method),
        "method_adler": m_arm["adler"], "method_adler_correct": m_arm["adler_correct"],
        "method_replay": m_arm["replay"], "method_replay_correct": m_arm["replay_correct"],
        "method_unique_adler_correct": m_arm["adler"] == 1 and m_arm["adler_correct"],
        "method_unique_replay_correct": m_arm["replay"] == 1 and m_arm["replay_correct"],
        "owin_adler": o_arm["adler"], "owin_adler_correct": o_arm["adler_correct"],
        "owin_replay": o_arm["replay"], "owin_replay_correct": o_arm["replay_correct"],
        "full_searched": full,
        "full_adler": f_arm["adler"] if full else "", "full_adler_correct": f_arm["adler_correct"] if full else "",
        "full_replay": f_arm["replay"] if full else "",
        "t_localise_s": round(t_loc, 2), "t_adler_s": round(t_adl, 2), "t_search_s": round(t_search, 2),
    }


def jobs_for(corpus: Path, max_comp: int, every: int, full_max: int) -> list[dict]:
    jobs = []
    for f in sorted((corpus / "corrupted").rglob("*_stream_zlib.pdf")):
        mode, kind = f.parent.parent.name, f.parent.name
        orig = corpus / "original" / mode / kind / (f.name[: -len("_stream_zlib.pdf")] + ".pdf")
        a, b = orig.read_bytes(), f.read_bytes()
        if len(a) != len(b):
            continue
        for m in STREAM_RE.finditer(a):
            s, e = m.start(1), m.end(1)
            ob, db = trim(a[s:e]), trim(b[s:e])
            if ob == db or len(ob) != len(db) or len(ob) > max_comp:
                continue        # unchanged, or the change hit the EOL before 'endstream' (not Flate data)
            try:
                zlib.decompress(ob)
            except zlib.error:
                continue
            jobs.append({"orig": str(orig), "dam": str(f), "rel": str(f.relative_to(corpus)), "mode": mode,
                         "span": (s, e), "full_max": full_max})
    return jobs[::every]


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])

    def opt(name, default):
        return int(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default

    full_max, max_comp = opt("--full-max", 1536), opt("--max-comp", 32768)
    every, workers = opt("--every", 1), opt("--workers", 2)
    jobs = jobs_for(corpus, max_comp, every, full_max)
    t0 = time.time()
    with Pool(workers) as pool:
        rows = pool.map(run_one, jobs, chunksize=1)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)

    def summarise(rs):
        rep = [r for r in rs if r["orig_replayable"] != "no"]
        full = [r for r in rs if r["full_searched"]]
        clean = [r for r in rs if not r["parse_error"]]
        offs = [r["loc_offset"] for r in rep]
        return {
            "streams": len(rs),
            "orig_replayable": len(rep),
            "damaged_parses_to_end": len(clean),
            "true_in_method_set": sum(r["true_in_method_set"] for r in rs),
            "true_in_replay_window_among_replayable": sum(r["true_in_replay_window"] for r in rep),
            "true_in_adler_positions": sum(r["true_in_adler_positions"] for r in rs),
            "true_in_adler_positions_among_parses_to_end": sum(r["true_in_adler_positions"] for r in clean),
            "true_in_trailer": sum(r["true_in_trailer"] for r in rs),
            "method_unique_adler_correct": sum(r["method_unique_adler_correct"] for r in rs),
            "method_unique_replay_correct": sum(r["method_unique_replay_correct"] for r in rs),
            "method_unique_replay_correct_among_replayable": sum(r["method_unique_replay_correct"] for r in rep),
            "method_adler_counts": dict(sorted(Counter(min(r["method_adler"], 5) for r in rs).items())),
            "method_replay_counts": dict(sorted(Counter(min(r["method_replay"], 5) for r in rs).items())),
            "method_wrong_replay_candidates": sum(r["method_replay"] - int(r["method_replay_correct"]) for r in rs),
            "loc_offset_replayable_min_median_max": [min(offs), statistics.median(offs), max(offs)] if offs else None,
            "oracle_window_adler_counts": dict(sorted(Counter(min(r["owin_adler"], 5) for r in rs).items())),
            "oracle_window_true_value_passes_adler": sum(r["owin_adler_correct"] for r in rs),
            "oracle_window_adler_unique_and_correct": sum(r["owin_adler"] == 1 and r["owin_adler_correct"] for r in rs),
            "full_search_streams": len(full),
            "full_search_adler_counts": dict(sorted(Counter(min(r["full_adler"], 5) for r in full).items())),
            "full_search_adler_unique_and_correct": sum(r["full_adler"] == 1 and r["full_adler_correct"] for r in full),
            "full_search_replay_counts": dict(sorted(Counter(min(r["full_replay"], 5) for r in full).items())),
            "median_method_positions": statistics.median(r["method_positions"] for r in rs),
            "median_search_s": statistics.median(r["t_search_s"] for r in rs),
            "max_search_s": max(r["t_search_s"] for r in rs),
        }

    res = {"params": {"full_max": full_max, "max_comp": max_comp, "every": every},
           "zlib": zlib.ZLIB_RUNTIME_VERSION, "python": sys.version.split()[0],
           "wall_s": round(time.time() - t0, 1),
           "all": summarise(rows),
           "by_mode": {m: summarise([r for r in rows if r["mode"] == m]) for m in sorted({r["mode"] for r in rows})}}
    Path(f"{out}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
