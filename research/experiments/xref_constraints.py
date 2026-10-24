#!/usr/bin/env python3
"""Seed 'SMT for offsets/xref' (agent D): restore a deleted object header (REPDF object_header class,
DMG for C5) byte-exactly from the surviving cross-reference data, once with z3 and once in closed form.

REPDF's object_header damage deletes 8 bytes, one 'N G obj' header with an EOL, and leaves the xref
table / xref stream unchanged, so every offset after the deletion is stale by the deleted length.
Unknowns: deletion point p, length L, and the producer's offset bias c (Microsoft Print to PDF points
every xref offset at the EOL byte before 'N G obj', so its headers sit at X_n + 1; Word Save As uses 0).
For each object n whose header is still found by a scan at H_n and whose xref offset is X_n:
    H_n = X_n + c            if X_n < p,  else  X_n + c - L.
Keyword pointers (startxref, /Prev -> 'xref') have bias 0: K = V or V - L, and /XRefStm (-> an object
header) has bias c. The deleted span must contain the header of an object that the xref lists but the
scan cannot find: p <= X_m + c < p + L. That object's header is re-synthesised as 'n g obj' padded with
the file's own EOL bytes, and inserted so it lands at X_m + c.

  z3 arm:     Int p, L, c; L and c must be unique ((L, c) != (L0, c0) is unsat) and p's feasible
              interval comes from z3 Optimize (min and max).
  brute force: enumerate c in -2..2 and L in 1..64 and keep the pairs with a feasible p (no solver).
Scoring against the original: is the restored file byte-identical to it?

    python3 xref_constraints.py /home/user/dfrc-korea/repdf results/xref_constraints
Needs z3-solver. Outputs <out>.csv and <out>.summary.json. Clean-room: no engine code consulted.
"""
import csv
import json
import re
import sys
import time
import zlib
from collections import Counter
from pathlib import Path

import z3

HDR = re.compile(rb"(?<![0-9])(\d+)[ \t\r\n\f\0]+(\d+)[ \t\r\n\f\0]+obj\b")
XREF_TABLE = re.compile(rb"(?<![A-Za-z])xref[ \t]*\r?\n")
SUBSEC = re.compile(rb"(\d+)[ \t]+(\d+)[ \t]*\r?\n")
ENTRY = re.compile(rb"(\d{10})[ \t](\d{5})[ \t]([nf])")


def xref_entries(data: bytes) -> dict[int, tuple[int, int]]:
    """Type-1 (in-file offset) entries {obj: (offset, gen)} from classic tables and xref streams."""
    ent: dict[int, tuple[int, int]] = {}
    for m in XREF_TABLE.finditer(data):
        i = m.end()
        while True:
            s = SUBSEC.match(data, i)
            if not s:
                break
            first, count = int(s.group(1)), int(s.group(2))
            i = s.end()
            for k in range(count):
                e = ENTRY.match(data, i)
                if not e:
                    break
                if e.group(3) == b"n":
                    ent[first + k] = (int(e.group(1)), int(e.group(2)))
                i = e.end()
                while i < len(data) and data[i] in b" \r\n":
                    i += 1
    for m in re.finditer(rb"/Type\s*/XRef\b", data):
        d0 = data.rfind(b"<<", 0, m.start())
        d1 = data.find(b"stream", m.start())
        dic = data[d0:d1]
        w = [int(x) for x in re.search(rb"/W\s*\[\s*([\d\s]+)\]", dic).group(1).split()]
        size = int(re.search(rb"/Size\s+(\d+)", dic).group(1))
        idx = re.search(rb"/Index\s*\[\s*([\d\s]+)\]", dic)
        idx = [int(x) for x in idx.group(1).split()] if idx else [0, size]
        length = int(re.search(rb"/Length\s+(\d+)", dic).group(1))
        s0 = d1 + len(b"stream")
        s0 += 2 if data[s0:s0 + 2] == b"\r\n" else 1
        raw = zlib.decompress(data[s0:s0 + length])
        if re.search(rb"/Predictor", dic):
            raise ValueError("xref stream predictor not implemented")
        rw = sum(w)
        rows = [raw[r * rw:(r + 1) * rw] for r in range(len(raw) // rw)]
        nums = [n for a, c in zip(idx[::2], idx[1::2]) for n in range(a, a + c)]
        for n, row in zip(nums, rows):
            f, o = 0, 0
            fields = []
            for width in w:
                fields.append(int.from_bytes(row[o:o + width], "big") if width else None)
                o += width
            t = fields[0] if w[0] else 1
            if t == 1:
                ent[n] = (fields[1], fields[2] or 0)
    return ent


def headers(data: bytes) -> dict[int, list[int]]:
    h: dict[int, list[int]] = {}
    for m in HDR.finditer(data):
        h.setdefault(int(m.group(1)), []).append(m.start())
    return h


def solve_z3(pairs, kw, objptr, missing) -> dict:
    """pairs: (X, H) object offsets; kw: (V, K) keyword pointers (bias 0); objptr: (V, K) object pointers
    (bias c); missing: xref offsets X_m of objects without a found header."""
    t0 = time.time()
    p, L, c = z3.Ints("p L c")
    cons = [L >= 1, L <= 64, p >= 0, c >= -2, c <= 2]
    cons += [z3.If(x < p, h == x + c, h == x + c - L) for x, h in pairs]
    cons += [z3.If(v < p, k == v, k == v - L) for v, k in kw]
    cons += [z3.If(v < p, k == v + c, k == v + c - L) for v, k in objptr]
    if missing:
        cons.append(z3.Or(*[z3.And(p <= x + c, x + c < p + L) for x in missing]))
    s = z3.Solver()
    s.add(*cons)
    if s.check() != z3.sat:
        return {"z3_sat": False, "z3_s": round(time.time() - t0, 3)}
    m = s.model()
    L0, c0 = m[L].as_long(), m[c].as_long()
    s.push()
    s.add(z3.Or(L != L0, c != c0))
    unique = s.check() == z3.unsat
    s.pop()
    bounds = []
    for goal in ("min", "max"):
        o = z3.Optimize()
        o.add(*cons, L == L0, c == c0)
        (o.minimize if goal == "min" else o.maximize)(p)
        o.check()
        bounds.append(o.model()[p].as_long())
    return {"z3_sat": True, "z3_L": L0, "z3_c": c0, "z3_unique": unique, "z3_p_lo": bounds[0],
            "z3_p_hi": bounds[1], "z3_s": round(time.time() - t0, 3)}


def brute_force(pairs, kw, objptr, missing) -> dict:
    """No solver: for each (c, L) the constraints bound p to an interval; keep pairs where it is non-empty."""
    feas = []
    for c in range(-2, 3):
        for L in range(1, 65):
            lo, hi, ok = 0, 10 ** 12, True
            for x, h, b in [(x, h, c) for x, h in pairs + objptr] + [(v, k, 0) for v, k in kw]:
                if h == x + b:
                    lo = max(lo, x + 1) if h != x + b - L else lo     # unshifted: p > x
                elif h == x + b - L:
                    hi = min(hi, x)                                   # shifted: p <= x
                else:
                    ok = False
                    break
            if ok and missing:
                ok = any(max(lo, x + c - L + 1) <= min(hi, x + c) for x in missing)
            if ok and lo <= hi:
                feas.append((L, c, lo, hi))
    return {"bf_feasible": len(feas), "bf_L": feas[0][0] if len(feas) == 1 else None,
            "bf_c": feas[0][1] if len(feas) == 1 else None}


def pointers(data: bytes, hs: dict[int, list[int]]):
    """(V, K) for startxref and /Prev (target 'xref' keyword) and /XRefStm (target an object header): K is
    the nearest such target at or before V in the damaged file (a deletion only moves targets earlier)."""
    kws = [m.start() for m in re.finditer(rb"(?<![A-Za-z])xref(?![A-Za-z])", data)]
    hpos = sorted(h for v in hs.values() for h in v)
    kw, objp = [], []
    for m in re.finditer(rb"(startxref\s+|/Prev\s+)(\d+)", data):
        v = int(m.group(2))
        k = max((x for x in kws if x <= v and x != m.start()), default=None)
        if k is not None:
            kw.append((v, k))
    for m in re.finditer(rb"/XRefStm\s+(\d+)", data):
        v = int(m.group(1))
        k = max((x for x in hpos if x <= v + 2), default=None)
        if k is not None:
            objp.append((v, k))
    return kw, objp


def restore(dam: bytes, missing: list[tuple[int, int, int]], L: int, c: int, plo: int, phi: int, eol: bytes):
    """Insert 'n g obj' plus `pad` EOL bytes (every split of CR/LF strings of that length before and after
    the header) so the header lands at X_m + c and the insertion point lies in [plo, phi]. Returns
    (candidate, eol_score); eol_score counts the file's own EOL sequence around the insertion, which
    ranks '\r\n1 0 obj' above '\r\r1 0 obj' in a CRLF file."""
    from itertools import product
    out = []
    for n, x, g in missing:
        x += c                             # where the header starts in the original layout
        hdr = f"{n} {g} obj".encode()
        pad = L - len(hdr)
        if pad < 0 or pad > 4:
            continue
        for k in range(pad + 1):           # k bytes before the header, pad-k after
            for fill in product((b"\r", b"\n"), repeat=pad):
                pre, post = b"".join(fill[:k]), b"".join(fill[k:])
                q = x - len(pre)
                if not plo <= q <= phi:
                    continue
                cand = dam[:q] + pre + hdr + post + dam[q:]
                score = cand[max(0, q - 4):q + L + 4].count(eol)
                out.append((cand, score))
    return out


def main() -> None:
    corpus, outp = Path(sys.argv[1]), Path(sys.argv[2])
    rows = []
    for d in sorted((corpus / "corrupted").rglob("*_object_header.pdf")):
        o = corpus / "original" / d.relative_to(corpus / "corrupted").parent / d.name.replace("_object_header.pdf", ".pdf")
        a, b = o.read_bytes(), d.read_bytes()
        t0 = time.time()
        ent = xref_entries(b)
        hs = headers(b)
        pairs = [(x, hs[n][0]) for n, (x, g) in ent.items() if len(hs.get(n, [])) == 1]
        missing = [(n, x, g) for n, (x, g) in ent.items() if n not in hs]
        kw, objp = pointers(b, hs)
        bf = brute_force(pairs, kw, objp, [x for _, x, _ in missing])
        t_bf = time.time() - t0
        z = solve_z3(pairs, kw, objp, [x for _, x, _ in missing])
        eol = b"\r\n" if a.count(b"endobj\r\n") > a.count(b"endobj\n") // 2 else b"\n"
        cands, exact, top_exact, top_unique = [], False, False, False
        if z.get("z3_unique"):
            raw = restore(b, missing, z["z3_L"], z["z3_c"], z["z3_p_lo"], z["z3_p_hi"], eol)
            ok = {}
            for cnd, sc in raw:
                hc = headers(cnd)
                if all(x + z["z3_c"] in hc.get(n, []) for n, (x, g) in ent.items()):
                    ok[cnd] = sc
            cands = list(ok)
            exact = a in ok
            if ok:
                best = max(ok.values())
                tops = [cnd for cnd, sc in ok.items() if sc == best]
                top_unique, top_exact = len(tops) == 1, tops[0] == a and len(tops) == 1
        rows.append({"file": str(d.relative_to(corpus)), "mode": d.parts[-3], "xref_type1_entries": len(ent),
                     "headers_paired": len(pairs), "missing_headers": len(missing),
                     "missing_objs": " ".join(str(n) for n, _, _ in missing[:5]),
                     "pointer_pairs": len(kw) + len(objp),
                     "true_deleted_len": len(a) - len(b), **bf, **z, "brute_force_s": round(t_bf, 3),
                     "z3_agrees": bf.get("bf_L") == z.get("z3_L") and bf.get("bf_c") == z.get("z3_c"),
                     "consistent_restorations": len(cands),
                     "distinct_restorations": len(set(cands)), "byte_exact_among_them": exact,
                     "eol_style_top_unique": top_unique, "eol_style_top_byte_exact": top_exact})
        print(rows[-1]["file"][-60:], rows[-1]["missing_objs"], bf, z, len(set(cands)), exact, flush=True)
    outp.parent.mkdir(parents=True, exist_ok=True)
    keys = sorted({k for r in rows for k in r}, key=lambda k: list(rows[0]).index(k) if k in rows[0] else 99)
    with open(f"{outp}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=keys)
        w.writeheader()
        w.writerows(rows)
    res = {"files": len(rows), "z3": z3.get_version_string(),
           "by_mode": {m: {"files": len(rs),
                           "one_missing_header": sum(r["missing_headers"] == 1 for r in rs),
                           "z3_L_c_unique": sum(bool(r.get("z3_unique")) for r in rs),
                           "z3_L_equals_true": sum(r.get("z3_L") == r["true_deleted_len"] for r in rs),
                           "z3_bias_c_counts": dict(Counter(r.get("z3_c") for r in rs)),
                           "brute_force_unique": sum(r["bf_feasible"] == 1 for r in rs),
                           "z3_agrees_with_brute_force": sum(r["z3_agrees"] for r in rs),
                           "unique_distinct_restoration": sum(r["distinct_restorations"] == 1 for r in rs),
                           "byte_exact_restoration_found": sum(r["byte_exact_among_them"] for r in rs),
                           "unique_and_byte_exact": sum(r["distinct_restorations"] == 1 and r["byte_exact_among_them"] for r in rs),
                           "eol_style_choice_byte_exact": sum(r["eol_style_top_byte_exact"] for r in rs),
                           "median_z3_s": sorted(r.get("z3_s", 0) for r in rs)[len(rs) // 2],
                           "median_brute_force_s": sorted(r["brute_force_s"] for r in rs)[len(rs) // 2]}
                       for m in sorted({r["mode"] for r in rows}) for rs in [[r for r in rows if r["mode"] == m]]}}
    Path(f"{outp}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
