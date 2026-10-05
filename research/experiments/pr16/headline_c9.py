#!/usr/bin/env python3
"""pr16-ingest: recompute PR #16's C9 headline arithmetic (CLM-1030/1031) from our own re-runs.

usage: headline_c9.py <cases-dir> <results-dir> <out.json>

Reads <cases-dir>/*.meta (built by build_cases.py), <cases-dir>/*.flag (grammar_loc.py),
<results-dir>/mzbench_{err,grammar,small}.txt and <results-dir>/small_adler_audit.json.
PR #16 states 39% = (218 + 324 + ~20 small fonts) / 1,445.  mzbench prints only totals, so the
small-font term is taken from small_adler_audit.json: every Adler-only font stream <= 4 KiB that the
audit does not list as trailer-damaged, collision or one-trailer-byte case is byte-exact at its true offset,
and mzbench's small run reports no accept at a wrong position.
"""
import glob, json, os, re, sys
from collections import Counter

cases, res, out = sys.argv[1:4]
metas = {}
for f in glob.glob(os.path.join(cases, "*.meta")):
    metas[os.path.basename(f)[:-5]] = json.load(open(f))
flags = {os.path.basename(f)[:-5] for f in glob.glob(os.path.join(cases, "*.flag"))}

def mz(name):
    t = open(os.path.join(res, f"mzbench_{name}.txt")).read()
    m = re.search(r"cases=(\d+) fixed=(\d+) exact_match=(\d+) timeouts=(\d+) notfound=(\d+)", t)
    return dict(zip(["cases", "fixed", "exact", "timeouts", "notfound"], map(int, m.groups())))

audit = json.load(open(os.path.join(res, "small_adler_audit.json")))
bad = set()
for k in ("trailer_damaged", "true_offset_collision", "one_trailer_byte_fix"):
    v = audit.get(k)
    if isinstance(v, list):
        bad |= {x["id"] for x in v}

n = len(metas)
adler = {i: m for i, m in metas.items() if m["status"] == "adler"}
small = {i: m for i, m in adler.items() if m["clen"] <= 4096}
mid = {i: m for i, m in adler.items() if 4096 < m["clen"] <= 16384}
small_fonts = [i for i, m in small.items() if m["kind"] == "font"]
small_fonts_exact = [i for i in small_fonts if i not in bad]
flagged_small = [i for i in small if i in flags]
e, g, s = mz("err"), mz("grammar"), mz("small")
num_pr16_style = e["fixed"] + g["fixed"] + len(small_fonts_exact)
num_exact = e["exact"] + g["exact"] + len(small_fonts_exact)
r = {
    "damaged_flate_streams": n,
    "status": dict(Counter(m["status"] for m in metas.values())),
    "adler_small_le_4KiB_by_kind": dict(Counter(m["kind"] for m in small.values())),
    "adler_4_16KiB_by_kind": dict(Counter(m["kind"] for m in mid.values())),
    "adler_small_flagged_by_grammar": len(flagged_small),
    "mzbench_err": e, "mzbench_grammar": g, "mzbench_small": s,
    "small_fonts": len(small_fonts), "small_fonts_exact": len(small_fonts_exact),
    "pr16_style_numerator_err_fixed+grammar_fixed+small_fonts": num_pr16_style,
    "pr16_style_share": round(num_pr16_style / n, 4),
    "byte_exact_numerator_err_exact+grammar_exact+small_fonts_exact": num_exact,
    "byte_exact_share": round(num_exact / n, 4),
    "note": "err, grammar and small-font sets are disjoint (inflate-error vs Adler-only; grammar flags only content/CMap/text-other). "
            "Small non-font streams that the grammar localizer does not flag (e.g. 17 binary) are not counted, as in PR #16.",
}
json.dump(r, open(out, "w"), indent=1)
print(json.dumps(r, indent=1))
