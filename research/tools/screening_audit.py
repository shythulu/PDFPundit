#!/usr/bin/env python3
"""Screening audit: blind re-screen sample, claim spot-check sample, agreement and recall.

    python research/tools/screening_audit.py sample  --out DIR [--frac 0.2] [--claims 10] [--seed N]
    python research/tools/screening_audit.py score   --audit DIR
    python research/tools/screening_audit.py recall  --gold GOLD.json [--exclude-refs-of SRC-0001]

`sample` reads the main registries (run it after the merge) and writes to DIR:
  - rescreen-sample.jsonl: a seeded random share of screening records, stratified by screener.
    Each row carries only bibliographic fields (id, title, authors, year, venue, doi, arxiv, url),
    never the original decision, reason or summary, so the auditor screens blind.
  - claim-sample.jsonl: seeded random verified non-critique claims, stratified by agent, with
    the quote, page or locator, and where the cached source text lives.
  - key.jsonl: the original decisions, for `score` only. The auditor must not read it.
`score` compares DIR/rescreen.jsonl (the auditor's decisions, screening schema) with the key and
prints percent agreement and Cohen's kappa (3-way and include-vs-not), then tabulates
DIR/claim-verdicts.jsonl. `recall` matches a gold list (title, doi, year) against the registry
by DOI, then by title, and reports how many were found and how many were included.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import REGISTRIES, code_checkout, fulltext_path, load_jsonl, normalize, write_jsonl  # noqa: E402

BIB = ("id", "type", "title", "authors", "year", "venue", "doi", "arxiv", "url")
VERDICTS = ("faithful", "overstated", "understated", "misread", "unsupported")


def registries() -> dict[str, list[dict]]:
    return {t: load_jsonl(p) for t, (p, _, _) in REGISTRIES.items()}


def stratified(rows: list[dict], key, k_of, rng: random.Random) -> list[dict]:
    groups: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        groups[key(r)].append(r)
    out = []
    for g in sorted(groups):
        items = sorted(groups[g], key=lambda r: json.dumps(r, sort_keys=True))
        out += rng.sample(items, min(len(items), k_of(len(items))))
    return out


def cmd_sample(a) -> None:
    recs = registries()
    rng = random.Random(a.seed)
    src = {r["id"]: r for r in recs["source"]}
    screen = [s for s in recs["screening"] if s["src"] in src]
    picked = stratified(screen, lambda s: s["by"], lambda n: max(1, round(n * a.frac)), rng)
    a.out.mkdir(parents=True, exist_ok=True)
    write_jsonl(a.out / "rescreen-sample.jsonl",
                [{k: src[s["src"]].get(k) for k in BIB} for s in picked])
    write_jsonl(a.out / "key.jsonl", picked)

    claims = [c for c in recs["claim"]
              if c.get("kind") != "critique" and c.get("quote_check") in ("exact", "fuzzy")]
    per_agent = max(1, math.ceil(a.claims / max(1, len({c["provenance"]["agent"] for c in claims}))))
    cs = stratified(claims, lambda c: c["provenance"]["agent"], lambda n: min(n, per_agent), rng)
    rng.shuffle(cs)
    rows = []
    for c in cs[: a.claims]:
        s = src.get(c["src"], {})
        cached = fulltext_path(s["fulltext_sha256"]) if s.get("fulltext_sha256") else None
        if c.get("locator") and s.get("commit") and s.get("url"):
            cached = code_checkout(s["url"], s["commit"]) / c["locator"]["path"]
        rows.append({"claim": c["id"], "src": c["src"], "src_title": s.get("title"),
                     "src_url": s.get("url") or (f"https://doi.org/{s['doi']}" if s.get("doi") else None),
                     "kind": c.get("kind"), "text": c.get("text"), "quote": c.get("quote"),
                     "page": c.get("page"), "locator": c.get("locator"),
                     "cached_text": str(cached) if cached and cached.exists() else None,
                     "commit": s.get("commit")})
    write_jsonl(a.out / "claim-sample.jsonl", rows)
    print(f"rescreen sample: {len(picked)} of {len(screen)} screening records "
          f"({dict(Counter(s['by'] for s in picked))})")
    print(f"claim sample: {len(rows)} of {len(claims)} verified non-critique claims")


def kappa(pairs: list[tuple[str, str]]) -> tuple[float, float]:
    n = len(pairs)
    if not n:
        return float("nan"), float("nan")
    po = sum(x == y for x, y in pairs) / n
    ca, cb = Counter(x for x, _ in pairs), Counter(y for _, y in pairs)
    pe = sum(ca[c] * cb[c] for c in set(ca) | set(cb)) / (n * n)
    return po, (po - pe) / (1 - pe) if pe < 1 else float("nan")


def cmd_score(a) -> None:
    key = {r["src"]: r for r in load_jsonl(a.audit / "key.jsonl")}
    mine = {r["src"]: r for r in load_jsonl(a.audit / "rescreen.jsonl")}
    missing = sorted(set(key) - set(mine))
    both = [s for s in key if s in mine]
    three = [(key[s]["decision"], mine[s]["decision"]) for s in both]
    binary = [(x == "include", y == "include") for x, y in three]
    po3, k3 = kappa(three)
    po2, k2 = kappa([(str(x), str(y)) for x, y in binary])
    print(f"re-screened {len(both)} of {len(key)} sampled records" + (f"; missing {missing}" if missing else ""))
    print(f"3-way (include/maybe/exclude): agreement {po3:.0%}, Cohen's kappa {k3:.2f}")
    print(f"include vs not:                agreement {po2:.0%}, Cohen's kappa {k2:.2f}")
    print("confusion (original -> auditor):", dict(Counter(f"{x}->{y}" for x, y in three)))
    for s in both:
        if key[s]["decision"] != mine[s]["decision"]:
            print(f"  {s}: {key[s]['by']} {key[s]['decision']} ({key[s]['reason_code']}) vs auditor "
                  f"{mine[s]['decision']} ({mine[s]['reason_code']}): {mine[s].get('reason', '')[:120]}")
    vpath = a.audit / "claim-verdicts.jsonl"
    if vpath.exists():
        v = load_jsonl(vpath)
        bad = [r for r in v if r.get("verdict") not in VERDICTS]
        if bad:
            sys.exit(f"claim verdicts must be one of {VERDICTS}: {[r.get('claim') for r in bad]}")
        print(f"claim spot-check: {dict(Counter(r['verdict'] for r in v))} over {len(v)} claims")
        for r in v:
            if r["verdict"] != "faithful":
                print(f"  {r['claim']}: {r['verdict']}: {r.get('note', '')[:160]}")


def crossref_ref_dois(doi: str) -> set[str]:
    from polite_get import polite_get
    data = json.loads(polite_get(f"https://api.crossref.org/works/{doi}", "application/json"))
    return {r["DOI"].lower() for r in data["message"].get("reference", []) if r.get("DOI")}


def cmd_recall(a) -> None:
    from rapidfuzz import fuzz
    recs = registries()
    gold = json.loads(Path(a.gold).read_text())
    by_doi = {r["doi"].lower(): r for r in recs["source"] if r.get("doi")}
    screened = {s["src"]: s["decision"] for s in recs["screening"]}
    excluded: set[str] = set()
    if a.exclude_refs_of:
        host = next(r for r in recs["source"] if r["id"] == a.exclude_refs_of)
        excluded = crossref_ref_dois(host["doi"])

    def find(g: dict) -> dict | None:
        if g.get("doi") and g["doi"].lower() in by_doi:
            return by_doi[g["doi"].lower()]
        gt = normalize(g["title"])
        best = max(recs["source"], key=lambda r: fuzz.ratio(gt, normalize(r.get("title", ""))))
        return best if fuzz.ratio(gt, normalize(best.get("title", ""))) >= 90 else None

    rows = []
    for g in gold:
        r = find(g)
        dec = screened.get(r["id"]) if r else None
        is_ref = (g.get("doi") or "").lower() in excluded
        rows.append((g, r, dec, is_ref))
        print(f"{'FOUND' if r else 'miss '} {dec or '-':8} {'[ref]' if is_ref else '     '} "
              f"{g.get('community', ''):20} {g['title'][:70]}" + (f"  -> {r['id']}" if r else ""))

    def summary(sel):
        n = len(sel)
        found = sum(1 for _, r, _, _ in sel if r)
        inc = sum(1 for _, r, d, _ in sel if r and d in ("include", "maybe"))
        return f"{found}/{n} found ({found / n:.0%}), {inc}/{n} included or maybe" if n else "n/a"

    print(f"recall, all gold works:        {summary(rows)}")
    if a.exclude_refs_of:
        print(f"recall, excluding {a.exclude_refs_of} refs: {summary([x for x in rows if not x[3]])}")
    by_comm = defaultdict(list)
    for x in rows:
        by_comm[x[0].get("community", "?")].append(x)
    for c, sel in sorted(by_comm.items()):
        print(f"  {c:20} {summary(sel)}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("sample")
    s.add_argument("--out", type=Path, required=True)
    s.add_argument("--frac", type=float, default=0.2)
    s.add_argument("--claims", type=int, default=10)
    s.add_argument("--seed", type=int, default=20260927)
    s.set_defaults(fn=cmd_sample)
    c = sub.add_parser("score")
    c.add_argument("--audit", type=Path, required=True)
    c.set_defaults(fn=cmd_score)
    r = sub.add_parser("recall")
    r.add_argument("--gold", required=True)
    r.add_argument("--exclude-refs-of")
    r.set_defaults(fn=cmd_recall)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
