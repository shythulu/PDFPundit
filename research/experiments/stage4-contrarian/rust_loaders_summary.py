"""Summarize experiments/results/rust_loaders.tsv (output of the rust_loaders probe).

Per damage class x producer x library: files opened, files with the original's page count,
and the median text-char ratio vs. the same library on the matching original (capped at 1.0
per file before the median; files that fail to open count as 0).
With --text-dir DIR (the probe's TEXT_DIR), also a bag-of-characters recall and precision
(NFKC, whitespace removed) of each damaged file's text against the same library's text of the
matching original; a file that does not open scores recall 0.
Usage: python3 rust_loaders_summary.py results/rust_loaders.tsv results/rust_loaders.summary.json [--text-dir DIR]
"""
import csv, json, os, re, statistics, sys, unicodedata
from collections import Counter
from collections import defaultdict

SUFFIXES = ["_font_mapping_loss", "_remove_unicode_fonts", "_remove_fonts", "_object_header", "_page_tree",
            "_stream_zlib", "_partial_cut", "_trailer", "_header", "_xref"]
CLASS = {"_header": "C1", "_xref": "C2", "_trailer": "C3", "_page_tree": "C4", "_object_header": "C5",
         "_font_mapping_loss": "C6", "_remove_fonts": "C7", "_remove_unicode_fonts": "C8", "_stream_zlib": "C9",
         "_partial_cut": "C10"}

def key(path):
    name = path.rsplit("/", 1)[1][:-4]
    prod = "saveas" if "/saveas/" in path else "print"
    for s in SUFFIXES:
        if name.endswith(s):
            return name[: -len(s)], prod, CLASS[s]
    return name, prod, "orig"

TEXT_DIR = sys.argv[sys.argv.index("--text-dir") + 1] if "--text-dir" in sys.argv else None

def bag(lib, path):
    f = os.path.join(TEXT_DIR, lib, path.rsplit("/", 1)[1] + ".txt")
    if not os.path.exists(f):
        return None
    t = unicodedata.normalize("NFKC", open(f, encoding="utf-8", errors="replace").read())
    return Counter(c for c in t if not c.isspace())

def recall_precision(o, d):
    if not o:
        return None, None
    inter = sum((o & d).values()) if d is not None else 0
    rec = inter / sum(o.values())
    prec = (inter / sum(d.values())) if d and sum(d.values()) else None
    return rec, prec

rows = list(csv.DictReader(open(sys.argv[1]), delimiter="\t"))
orig = {}
for r in rows:
    base, prod, cls = key(r["path"])
    if cls == "orig":
        orig[(base, r["lib"])] = r
agg = defaultdict(lambda: {"n": 0, "opened": 0, "panic": 0, "pages_match": 0, "ratios": [], "diag_files": 0, "rec": [], "prec": []})
for r in rows:
    base, prod, cls = key(r["path"])
    a = agg[(cls, prod, r["lib"])]
    a["n"] += 1
    o = orig.get((base, r["lib"]))
    a["panic"] += r["status"] == "panic"
    if TEXT_DIR and r["lib"] != "hayro-syntax-0.8.0" and o is not None and o["status"] == "ok":
        ob = bag(r["lib"], o["path"])
        db = bag(r["lib"], r["path"]) if r["status"] == "ok" else None
        rec, prec = recall_precision(ob, db)
        if rec is not None:
            a["rec"].append(rec)
        if prec is not None:
            a["prec"].append(prec)
    if r["status"] != "ok":
        a["ratios"].append(0.0)
        continue
    a["opened"] += 1
    if int(r["diagnostics"]) > 0:
        a["diag_files"] += 1
    if o and o["status"] == "ok" and int(r["pages"]) == int(o["pages"]):
        a["pages_match"] += 1
    oc = int(o["text_chars"]) if o and o["status"] == "ok" else -1
    if oc > 0 and int(r["text_chars"]) >= 0:
        a["ratios"].append(min(1.0, int(r["text_chars"]) / oc))
out = {}
for (cls, prod, lib), a in sorted(agg.items()):
    out[f"{cls}|{prod}|{lib}"] = {"n": a["n"], "opened": a["opened"], "panic": a["panic"],
                                   "pages_match": a["pages_match"], "files_with_repair_diagnostics": a["diag_files"],
                                   "median_text_ratio": round(statistics.median(a["ratios"]), 3) if a["ratios"] else None,
                                   "mean_text_ratio": round(statistics.mean(a["ratios"]), 3) if a["ratios"] else None,
                                   "mean_char_recall": round(statistics.mean(a["rec"]), 3) if a["rec"] else None,
                                   "mean_char_precision": round(statistics.mean(a["prec"]), 3) if a["prec"] else None}
json.dump(out, open(sys.argv[2], "w"), indent=1)
libs = sorted({k.split("|")[2] for k in out})
print("class prod " + " ".join(f"{l}(open/pages/mean)" for l in libs))
for cls in ["orig", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"]:
    for prod in ["saveas", "print"]:
        cells = []
        for l in libs:
            v = out.get(f"{cls}|{prod}|{l}")
            cells.append(f"{v['opened']}/{v['pages_match']}/R{v['mean_char_recall']}/P{v['mean_char_precision']}" + (f" p{v['panic']}" if v["panic"] else ""))
        print(cls, prod, " | ".join(cells))
