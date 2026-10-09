#!/usr/bin/env python3
"""GAP-205 prototype: recover glyph-code -> character maps of REPDF 'Print to PDF' originals as a
substitution cipher, using only what survives when the font programs are blanked (C7/C8, OBS-0003):
the content-stream code sequences and the /W widths. Ground truth comes from each embedded TrueType
font's own cmap, inverted to GID -> character. (The originals also carry a ToUnicode CMap, which this
scenario assumes lost; print_font_names.py finds the two agree on 35,673 of 35,792 shared codes, the rest
being presentation-form and hyphen variants.)

  1. Code sequences: every hex-string glyph code (Identity-H, 2 bytes) shown by Tj/TJ, per
     (page, font tag); one show operator = one run, runs separated by a known boundary.
  2. Slots: runs are grouped by font object (the indirect reference survives C7/C8); fonts whose
     /W widths agree on every shared code (>= 3 shared codes, no conflict) are merged, because Print
     to PDF re-tags fonts per page (F1 on page 1 need not be F1 on page 2).
  3. Language model: interpolated character 4-gram over a 90-symbol alphabet, trained only on the
     cached open-access paper texts in research/cache/fulltext (no REPDF document text).
  4. Solver: simulated annealing over injective code -> symbol maps (frequency-rank start, swap or
     reassign moves, max(--iters, 150 x codes) steps, 3 restarts, best kept). No widths are used in the solve (pure language prior).
  5. Scoring: token accuracy (share of in-alphabet code occurrences decoded right; codes whose true
     character is outside the alphabet, e.g. CJK, are counted separately), by slot length; digit tokens
     separately (a language prior cannot tell '2024' from '2042'); and an abstention view: each code's
     margin (log-probability lost if it is reassigned to its best alternative symbol) against a
     threshold, giving accuracy on kept tokens versus the share abstained.

    python3 decipher_codes.py /home/user/dfrc-korea/repdf research/cache/fulltext <out-prefix> [--max-files N]
        [--iters 6000] [--shard k/n]      # shard k of n: writes <out>.shard<k>.csv after each file
    python3 decipher_codes.py ... <out-prefix> --merge n   # combine n shard CSVs into <out>.csv + summary
Needs pypdf and fontTools. Outputs <out>.csv (one row per slot) and <out>.summary.json.
"""
from __future__ import annotations

import csv
import io
import json
import math
import random
import re
import statistics
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont
from pypdf import PdfReader
from pypdf.generic import IndirectObject

ALPHA = (" abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
         ".,;:'\"-()!?/&%$@#*+=[]’“”–—")
IDX = {c: i for i, c in enumerate(ALPHA)}
V = len(ALPHA)
SP = IDX[" "]
DIGITS = set("0123456789")
TOK = re.compile(rb"<([0-9A-Fa-f\s]*)>|/([^\s/<>\[\]()]+)|\[|\]|\((?:\\.|[^\\)])*\)|([A-Za-z'\"*]+)|([-+.0-9]+)")


def train_lm(textdir: Path) -> np.ndarray:
    """Dense log-probability table L[a,b,c,d] = log P(d | a b c), linear interpolation of 1-4-grams."""
    seq: list[int] = []
    for f in sorted(textdir.glob("*.txt")):
        t = f.read_text(errors="ignore")
        t = re.sub(r"\s+", " ", t)
        seq.extend(IDX.get(ch, SP) for ch in t)
    s = np.array(seq, dtype=np.int64)
    c1 = np.bincount(s, minlength=V).astype(np.float64) + 0.5
    c2 = np.bincount(s[:-1] * V + s[1:], minlength=V ** 2).reshape(V, V).astype(np.float64)
    c3 = np.bincount((s[:-2] * V + s[1:-1]) * V + s[2:], minlength=V ** 3).reshape(V, V, V).astype(np.float64)
    c4 = np.bincount(((s[:-3] * V + s[1:-2]) * V + s[2:-1]) * V + s[3:], minlength=V ** 4).reshape(V, V, V, V)
    p1 = c1 / c1.sum()
    with np.errstate(invalid="ignore", divide="ignore"):
        p2 = np.nan_to_num(c2 / c2.sum(1, keepdims=True))
        p3 = np.nan_to_num(c3 / c3.sum(2, keepdims=True))
        n3 = c4.sum(3, keepdims=True)
        p4 = np.where(n3 > 0, c4 / np.maximum(n3, 1), 0.0).astype(np.float32)
    lm = (0.55 * p4 + (0.25 * p3)[None] + (0.14 * p2)[None, None] + (0.06 * p1)[None, None, None]).astype(np.float32)
    np.log(lm, out=lm)
    return lm, len(s)


def widths_of(desc) -> dict[int, float]:
    w = {}
    arr = desc.get("/W")
    if arr is None:
        return w
    arr = [x.get_object() if isinstance(x, IndirectObject) else x for x in arr.get_object()]
    i = 0
    while i < len(arr):
        a = int(arr[i])
        if isinstance(arr[i + 1], list) or hasattr(arr[i + 1], "__iter__"):
            for k, x in enumerate(arr[i + 1]):
                w[a + k] = float(x)
            i += 2
        else:
            b, x = int(arr[i + 1]), float(arr[i + 2])
            for g in range(a, b + 1):
                w[g] = x
            i += 3
    return w


def truth_of(desc) -> dict[int, str]:
    fd = desc.get("/FontDescriptor")
    ff = fd.get_object().get("/FontFile2") if fd else None
    if ff is None:
        return {}
    t = TTFont(io.BytesIO(ff.get_object().get_data()), lazy=True)
    cm = t.getBestCmap() or {}
    out = {}
    for u, name in cm.items():
        try:
            out.setdefault(t.getGlyphID(name), chr(u))
        except Exception:
            pass
    return out


def runs_of(page) -> list[tuple[str, list[int]]]:
    cs = page.get_contents()
    data = cs.get_data() if cs is not None else b""   # a ContentStream is a dict subclass: can be falsy
    runs, font, cur, in_arr, last_name = [], None, [], False, None
    for m in TOK.finditer(data):
        hexs, name, op, num = m.group(1), m.group(2), m.group(3), m.group(4)
        tok = m.group(0)
        if name is not None:
            last_name = "/" + name.decode("latin-1")
        elif hexs is not None:
            h = re.sub(rb"\s", b"", hexs)
            cur.extend(int(h[i:i + 4], 16) for i in range(0, len(h) - 3, 4))
        elif op is not None:
            o = op.decode()
            if o == "Tf":
                font = last_name
            elif o in ("Tj", "TJ", "'", '"'):
                if cur and font:
                    runs.append((font, cur))
                cur = []
    return runs


def load_doc(path: Path):
    r = PdfReader(str(path))
    fonts, slots_runs = {}, defaultdict(list)
    for pi, page in enumerate(r.pages):
        res = page.get("/Resources")
        fdict = res.get_object().get("/Font") if res else None
        if fdict is None:
            continue
        fdict = fdict.get_object()
        for tag, ref in fdict.items():
            key = ref.idnum if isinstance(ref, IndirectObject) else (pi, tag)
            if key not in fonts:
                f = ref.get_object()
                dfs = f.get("/DescendantFonts")
                if dfs is None:
                    continue
                desc = dfs.get_object()[0].get_object()
                fonts[key] = {"w": widths_of(desc), "truth": truth_of(desc)}
        for tag, codes in runs_of(page):
            ref = fdict.get(tag)
            key = ref.idnum if isinstance(ref, IndirectObject) else (pi, tag)
            if key in fonts:
                slots_runs[key].append(codes)
    # merge fonts whose /W agree on >= 3 shared codes with no conflict (union-find)
    keys = [k for k in fonts if slots_runs.get(k)]
    parent = {k: k for k in keys}

    def find(k):
        while parent[k] != k:
            parent[k] = parent[parent[k]]
            k = parent[k]
        return k
    for i, a in enumerate(keys):
        for b in keys[i + 1:]:
            wa, wb = fonts[a]["w"], fonts[b]["w"]
            shared = set(wa) & set(wb)
            if len(shared) >= 3 and all(wa[g] == wb[g] for g in shared):
                parent[find(a)] = find(b)
    groups = defaultdict(list)
    for k in keys:
        groups[find(k)].append(k)
    slots = []
    for root, ks in groups.items():
        runs = [c for k in ks for c in slots_runs[k]]
        truth = {}
        conflict = 0
        for k in ks:
            for g, ch in fonts[k]["truth"].items():
                if g in truth and truth[g] != ch:
                    conflict += 1
                truth.setdefault(g, ch)
        slots.append({"fonts": len(ks), "runs": runs, "truth": truth, "truth_conflicts": conflict})
    return slots


def solve(runs: list[list[int]], lm: np.ndarray, rng: random.Random, iters: int, restarts: int = 3):
    codes = sorted({c for r in runs for c in r})
    K = len(codes)
    ci = {c: i for i, c in enumerate(codes)}
    seq = [-1]
    for r in runs:
        seq.extend(ci[c] for c in r)
        seq.append(-1)
    c = np.array(seq, dtype=np.int64)
    n = len(c)
    occ = [np.nonzero(c == k)[0] for k in range(K)]
    freq = np.array([len(o) for o in occ], dtype=np.float64)
    lm_flat = lm.reshape(-1)
    uni = np.argsort(-lm[SP, SP, SP])        # symbols by unigram-ish plausibility after spaces

    def plain(m):
        p = np.where(c >= 0, m[np.maximum(c, 0)], SP)
        return p

    def score_at(p, ends):
        ends = ends[ends >= 3]
        return float(lm_flat[((p[ends - 3] * V + p[ends - 2]) * V + p[ends - 1]) * V + p[ends]].sum())

    def total(m):
        p = plain(m)
        return score_at(p, np.arange(n)), p

    def affected(ks):
        e = np.concatenate([occ[k] + d for k in ks for d in range(4)])
        return np.unique(e[(e >= 0) & (e < n)])

    best_m, best_s = None, -math.inf
    order = np.argsort(-freq)
    for rs in range(restarts):
        m = np.zeros(K, dtype=np.int64)
        start = list(uni)
        if rs > 0:
            rng.shuffle(start[:12])
        for rank, k in enumerate(order):
            m[k] = start[rank % V] if rank < V else start[rng.randrange(V)]
        owner = {}
        for k in range(K):
            owner.setdefault(int(m[k]), k)
        s, p = total(m)
        T0 = 4.0
        for it in range(iters):
            T = T0 * (1 - it / iters) + 0.02
            a = int(rng.choices(range(K), weights=freq)[0]) if rng.random() < 0.7 else rng.randrange(K)
            x = rng.randrange(V)
            if x == m[a]:
                continue
            b = owner.get(x)
            ks = [a] if b is None or b == a else [a, b]
            ends = affected(ks)
            old = score_at(p, ends)
            olda = m[a]
            m[a] = x
            if len(ks) == 2:
                m[b] = olda
            p_new = p.copy()
            for k in ks:
                p_new[occ[k]] = m[k]
            new = score_at(p_new, ends)
            if new >= old or rng.random() < math.exp((new - old) / T):
                p = p_new
                s += new - old
                if owner.get(int(olda)) == a:
                    del owner[int(olda)]
                owner[x] = a
                if len(ks) == 2:
                    owner[int(olda)] = b
            else:
                m[a] = olda
                if len(ks) == 2:
                    m[b] = x
        if s > best_s:
            best_s, best_m = s, m.copy()
    # margins: log-probability lost by moving each code to its best alternative symbol (swap if taken)
    m = best_m
    p = plain(m)
    owner = {int(m[k]): k for k in range(K)}
    margins = np.zeros(K)
    for a in range(K):
        best_alt = -math.inf
        for x in range(V):
            if x == m[a]:
                continue
            b = owner.get(x)
            ks = [a] if b is None else [a, b]
            ends = affected(ks)
            old = score_at(p, ends)
            p2 = p.copy()
            p2[occ[a]] = x
            if b is not None:
                p2[occ[b]] = m[a]
            best_alt = max(best_alt, score_at(p2, ends) - old)
        margins[a] = -best_alt
    return codes, m, freq, margins, n


def main() -> None:
    corpus, textdir, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    max_files = int(sys.argv[sys.argv.index("--max-files") + 1]) if "--max-files" in sys.argv else 999
    iters = int(sys.argv[sys.argv.index("--iters") + 1]) if "--iters" in sys.argv else 6000
    t0 = time.time()
    lm, lm_chars = train_lm(textdir)
    files = [f for f in sorted((corpus / "original" / "print").rglob("*.pdf")) if "multi" not in f.name.lower()]
    files = files[:max_files]
    if "--merge" in sys.argv:                 # combine shard CSVs written by --shard runs
        n_sh = int(sys.argv[sys.argv.index("--merge") + 1])
        rows = []
        for k in range(n_sh):
            with open(f"{out}.shard{k}.csv", newline="") as fh:
                for r in csv.DictReader(fh):
                    rows.append({kk: (int(v) if v.lstrip("-").isdigit() else (float(v) if kk == "token_acc" and v else v))
                                 for kk, v in r.items()})
        write_outputs(out, rows, files, lm_chars, iters, time.time() - t0)
        return
    shard = sys.argv[sys.argv.index("--shard") + 1] if "--shard" in sys.argv else None
    if shard:
        k, n_sh = map(int, shard.split("/"))
        files = files[k::n_sh]
    rng = random.Random(20261005)
    rows = []
    for f in files:
        for si, slot in enumerate(load_doc(f)):
            truth = slot["truth"]
            ntok = sum(len(r) for r in slot["runs"])
            if ntok < 8:
                continue
            k_codes = len({c for r in slot["runs"] for c in r})
            codes, m, freq, margins, n = solve(slot["runs"], lm, rng, max(iters, 150 * k_codes))
            dec = {g: ALPHA[int(m[i])] for i, g in enumerate(codes)}
            tok_scored = tok_ok = dig = dig_ok = 0
            out_alpha = 0
            keep = {th: [0, 0] for th in (0.0, 2.0, 5.0, 10.0)}
            for i, g in enumerate(codes):
                t = truth.get(g)
                if t is None:
                    continue
                k = int(freq[i])
                if t not in IDX:          # truth outside the 90-symbol alphabet (CJK, symbols): unscorable
                    out_alpha += k
                    continue
                ok = dec[g] == t
                tok_scored += k
                tok_ok += k * ok
                if t in DIGITS:
                    dig += k
                    dig_ok += k * ok
                for th in keep:
                    if margins[i] > th:
                        keep[th][0] += k
                        keep[th][1] += k * ok
            rows.append({"file": str(f.relative_to(corpus)), "slot": si, "fonts_merged": slot["fonts"],
                         "truth_conflicts": slot["truth_conflicts"], "codes": len(codes), "tokens": ntok,
                         "tokens_scored": tok_scored, "tokens_correct": tok_ok,
                         "token_acc": round(tok_ok / tok_scored, 4) if tok_scored else "",
                         "truth_outside_alphabet": out_alpha, "digit_tokens": dig, "digit_correct": dig_ok,
                         **{f"kept_m{th:g}": keep[th][0] for th in keep},
                         **{f"kept_correct_m{th:g}": keep[th][1] for th in keep}})
            print(rows[-1]["file"][-50:], si, len(codes), ntok, rows[-1]["token_acc"], f"{time.time() - t0:.0f}s", flush=True)
        if shard:                             # rewrite the shard CSV after every file (survives a timeout)
            out.parent.mkdir(parents=True, exist_ok=True)
            with open(f"{out}.shard{shard.split('/')[0]}.csv", "w", newline="") as fh:
                w = csv.DictWriter(fh, fieldnames=list(rows[0]))
                w.writeheader()
                w.writerows(rows)
    if shard:
        return
    write_outputs(out, rows, files, lm_chars, iters, time.time() - t0)


def write_outputs(out, rows, files, lm_chars, iters, wall_s) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)

    def agg(rs):
        sc = sum(r["tokens_scored"] for r in rs)
        ok = sum(r["tokens_correct"] for r in rs)
        d = sum(r["digit_tokens"] for r in rs)
        dk = sum(r["digit_correct"] for r in rs)
        oa = sum(r["truth_outside_alphabet"] for r in rs)
        res = {"slots": len(rs), "tokens_scored": sc, "tokens_truth_outside_alphabet": oa,
               "token_acc": round(ok / sc, 4) if sc else None,
               "digit_tokens": d, "digit_acc": round(dk / d, 4) if d else None,
               "median_slot_acc": statistics.median([r["token_acc"] for r in rs if r["token_acc"] != ""]) if rs else None}
        for th in ("0", "2", "5", "10"):
            kept = sum(r[f"kept_m{th}"] for r in rs)
            kc = sum(r[f"kept_correct_m{th}"] for r in rs)
            res[f"margin>{th}"] = {"coverage": round(kept / sc, 4) if sc else None,
                                   "acc_on_kept": round(kc / kept, 4) if kept else None}
        return res
    bins = [(8, 64), (64, 256), (256, 1024), (1024, 4096), (4096, 10 ** 9)]
    res = {"files": len({r["file"] for r in rows}), "files_selected": len(files), "lm_training_chars": lm_chars,
           "alphabet": V, "iters_min": iters, "iters_per_code": 150, "restarts": 3,
           "wall_s_merge_or_run": round(wall_s, 1), "all": agg(rows),
           "by_slot_tokens": {f"{a}-{b}": agg([r for r in rows if a <= r["tokens"] < b]) for a, b in bins},
           "latin_slots_only": agg([r for r in rows if r["tokens_scored"] >= 0.9 * (r["tokens_scored"] + r["truth_outside_alphabet"]) and r["tokens_scored"]]),
           "slots_merged_from_several_fonts": sum(r["fonts_merged"] > 1 for r in rows),
           "slots_with_truth_conflicts": sum(r["truth_conflicts"] > 0 for r in rows),
           "versions": {"python": sys.version.split()[0], "numpy": np.__version__,
                        "pypdf": __import__("pypdf").__version__, "fonttools": __import__("fontTools").version}}
    Path(f"{out}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
