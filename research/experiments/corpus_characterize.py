#!/usr/bin/env python3
"""Characterize how each REPDF corruption class (C1-C10) actually changes the bytes.

    python research/experiments/corpus_characterize.py /home/user/dfrc-korea/repdf \
        research/experiments/results/corpus_characterize

Diffs every corrupted file against its original and writes:
  <out>.csv          one row per corrupted file
  <out>.summary.json per class x creation method: min/median/max of each measure

Measures: size delta; common prefix/suffix length; length of the differing middle
(the damaged span); for equal-size files the number of differing bytes, how many distinct
original stream bodies (`stream`..`endstream`) contain a changed byte, and the most common
replacement byte (0x20 = space-filled blanking). Non-truncation files that lost more than half
their bytes are listed as generator outliers.
"""

from __future__ import annotations

import csv
import json
import re
import statistics
import sys
from collections import Counter
from pathlib import Path

SUFFIX_TO_CLASS = {
    "_header": "C1", "_xref": "C2", "_trailer": "C3", "_page_tree": "C4",
    "_object_header": "C5", "_font_mapping_loss": "C6", "_remove_fonts": "C7",
    "_remove_unicode_fonts": "C8", "_stream_zlib": "C9", "_partial_cut": "C10",
}
STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def classify(name: str) -> tuple[str, str] | None:
    stem = name[:-4]
    for suf in sorted(SUFFIX_TO_CLASS, key=len, reverse=True):   # longest first (_remove_unicode_fonts)
        if stem.endswith(suf):
            return stem[: -len(suf)], SUFFIX_TO_CLASS[suf]
    return None


def common_prefix(a: bytes, b: bytes) -> int:
    n = min(len(a), len(b))
    i = 0
    while i < n and a[i] == b[i]:
        i += 1
    return i


def common_suffix(a: bytes, b: bytes, limit: int) -> int:
    n = min(len(a), len(b)) - limit
    i = 0
    while i < n and a[-1 - i] == b[-1 - i]:
        i += 1
    return i


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    rows = []
    for f in sorted((corpus / "corrupted").rglob("*.pdf")):
        mode, kind = f.parent.parent.name, f.parent.name
        c = classify(f.name)
        if c is None:
            continue
        base, cls = c
        orig = corpus / "original" / mode / kind / f"{base}.pdf"
        a, b = orig.read_bytes(), f.read_bytes()
        pre = common_prefix(a, b)
        suf = common_suffix(a, b, pre)
        row = {"file": str(f.relative_to(corpus)), "class": cls, "mode": mode, "kind": kind,
               "orig_size": len(a), "size_delta": len(b) - len(a), "prefix": pre, "suffix": suf,
               "orig_span": len(a) - pre - suf, "corrupt_span": len(b) - pre - suf,
               "diff_bytes": "", "streams_hit": "", "top_repl_byte": "", "top_repl_share": ""}
        if len(a) == len(b):
            diffs = [i for i in range(pre, len(a) - suf) if a[i] != b[i]]
            spans = [(m.start(1), m.end(1)) for m in STREAM_RE.finditer(a)]
            hit = {j for i in diffs for j, (s, e) in enumerate(spans) if s <= i < e}
            row["diff_bytes"], row["streams_hit"] = len(diffs), len(hit)
            if diffs:
                top, n = Counter(b[i] for i in diffs).most_common(1)[0]
                row["top_repl_byte"], row["top_repl_share"] = f"0x{top:02x}", round(n / len(diffs), 4)
        rows.append(row)

    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)

    summary: dict[str, dict] = {}
    for cls in [f"C{i}" for i in range(1, 11)]:
        for mode in ("saveas", "print"):
            sel = [r for r in rows if r["class"] == cls and r["mode"] == mode]
            if not sel:
                continue
            stats = {}
            for k in ("size_delta", "orig_span", "corrupt_span", "diff_bytes", "streams_hit"):
                vals = [r[k] for r in sel if r[k] != ""]
                if vals:
                    stats[k] = {"min": min(vals), "median": statistics.median(vals), "max": max(vals)}
            stats["kept_fraction"] = {
                "min": round(min((r["orig_size"] + r["size_delta"]) / r["orig_size"] for r in sel), 4),
                "max": round(max((r["orig_size"] + r["size_delta"]) / r["orig_size"] for r in sel), 4)}
            repl = Counter(r["top_repl_byte"] for r in sel if r["top_repl_byte"])
            if repl:
                stats["top_repl_byte"] = dict(repl.most_common(3))
                stats["top_repl_share_min"] = min(r["top_repl_share"] for r in sel if r["top_repl_byte"])
            # generator outliers: non-truncation classes that lost more than half the file
            outliers = [r["file"] for r in sel if cls != "C10" and r["size_delta"] < -0.5 * r["orig_size"]]
            summary[f"{cls}/{mode}"] = {"files": len(sel), **stats, "outliers": outliers}
    Path(f"{out}.summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: v for k, v in summary.items() if k.startswith(("C1/", "C9/", "C10/"))}, indent=1))
    print(f"{len(rows)} files -> {out}.csv, {out}.summary.json")


if __name__ == "__main__":
    main()
