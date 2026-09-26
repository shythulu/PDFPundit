#!/usr/bin/env python3
"""Validate the research knowledge base. Run before every commit.

    python research/tools/kb_validate.py                 # check main registries
    python research/tools/kb_validate.py --write         # also store computed quote_check values
    python research/tools/kb_validate.py --online --write  # also verify DOIs against Crossref
    python research/tools/kb_validate.py --staging research/staging/<agent>   # check an agent's staging area
                                                           # (overlaid on the main registries)

Checks: JSON Schema per record type; unique ids; every cross-reference resolves; screening
reason codes and topics come from schemas/vocab.json; dedupe keys are unique; every quote is
found in the cached full text (quote_check is computed here and must never be hand-set);
gaps above `candidate` cite >=1 verified quote; Parallel Search spend is under the cap.
Exit status 1 on any error.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.error
import urllib.request
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import (CROSSREF, REGISTRIES, ROOT, SCHEMAS, dedupe_key, doi_slug,  # noqa: E402
                      fulltext_pages, load_jsonl, normalize, write_jsonl)

PAID_CAP, PAID_WARN = 80, 60
FUZZY_MIN = 95
UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"

errors: list[str] = []
warnings: list[str] = []


def err(msg: str) -> None:
    errors.append(msg)


def warn(msg: str) -> None:
    warnings.append(msg)


def load(roots: list[Path]) -> dict[str, list[tuple[dict, Path]]]:
    """Records per type from each root (main research/ first, then staging overlays)."""
    out: dict[str, list[tuple[dict, Path]]] = {t: [] for t in REGISTRIES}
    for root in roots:
        for rtype, (path, _, _) in REGISTRIES.items():
            p = root / path.relative_to(ROOT)
            out[rtype] += [(r, p) for r in load_jsonl(p)]
    return out


def check_schemas(recs) -> None:
    try:
        import jsonschema
    except ImportError:
        warn("jsonschema not installed; schema checks skipped (pip install jsonschema)")
        return
    for rtype, (_, _, sfile) in REGISTRIES.items():
        schema = json.loads((SCHEMAS / sfile).read_text())
        v = jsonschema.Draft202012Validator(schema)
        for r, p in recs[rtype]:
            for e in v.iter_errors(r):
                loc = "/".join(str(x) for x in e.absolute_path) or "(root)"
                err(f"{p.name}: {r.get('id', r.get('src', '?'))}: {loc}: {e.message}")


def check_ids_and_refs(recs) -> dict[str, set[str]]:
    ids: dict[str, set[str]] = {}
    for rtype, (_, prefix, _) in REGISTRIES.items():
        seen: set[str] = set()
        for r, p in recs[rtype]:
            rid = r.get("id")
            if prefix is None:
                continue
            if rid in seen:
                err(f"{p.name}: duplicate id {rid}")
            seen.add(rid)
        ids[rtype] = seen

    def ref(kind: str, value, where: str) -> None:
        if value and value not in ids[kind]:
            err(f"{where}: unknown {kind} reference {value}")

    for r, p in recs["source"]:
        ref("source", r.get("duplicate_of"), r["id"])
        sid = (r.get("provenance") or {}).get("search_id")
        ref("search", sid, r["id"])
        if r.get("notes_path") and not (ROOT.parent / r["notes_path"]).exists():
            err(f"{r['id']}: notes_path {r['notes_path']} does not exist")
    for r, p in recs["screening"]:
        ref("source", r.get("src"), f"screening {r.get('src')}")
    for r, p in recs["claim"]:
        ref("source", r.get("src"), r.get("id", "?"))
    for r, p in recs["gap"]:
        for c in r.get("evidence", []):
            ref("claim", c, r["id"])
        for g in r.get("related_gaps", []):
            ref("gap", g, r["id"])
        for h in r.get("hypotheses", []):
            ref("hypothesis", h, r["id"])
    for r, p in recs["hypothesis"]:
        for g in r.get("gaps", []):
            ref("gap", g, r["id"])
        for t in r.get("tools", []):
            ref("tool", t, r["id"])
    for r, p in recs["damage"]:
        for e in r.get("evidence", []):
            ref("claim" if e.startswith("CLM") else "source", e, r["id"])
    return ids


def check_vocab_and_dedupe(recs) -> None:
    vocab = json.loads((SCHEMAS / "vocab.json").read_text())
    topics, reasons = set(vocab["topics"]), set(vocab["screening_reasons"])
    keys: dict[str, str] = {}
    for r, p in recs["source"]:
        for t in r.get("topics", []):
            if t not in topics:
                warn(f"{r['id']}: topic '{t}' not in vocab.json (add it there if it is real)")
        k = dedupe_key(r)
        if r.get("dedupe_key") and r["dedupe_key"] != k:
            warn(f"{r['id']}: dedupe_key {r['dedupe_key']} != computed {k}")
        if r.get("duplicate_of"):
            continue
        if k in keys:
            err(f"{r['id']}: same work as {keys[k]} ({k}); set duplicate_of or merge")
        keys[k] = r["id"]
        for a in r.get("aliases", []):
            keys.setdefault(a, r["id"])
    for r, p in recs["screening"]:
        if r.get("reason_code") not in reasons:
            err(f"screening {r.get('src')}: reason_code {r.get('reason_code')} not in vocab.json")


def quote_status(claim: dict, src: dict | None) -> str:
    quote = claim.get("quote")
    if not quote:
        return "no-quote"
    sha = (src or {}).get("fulltext_sha256")
    pages = fulltext_pages(sha) if sha else None
    if pages is None:
        return "no-fulltext"
    nq = normalize(quote)
    whole = normalize("\n".join(pages))
    page = claim.get("page")
    page_text = normalize(pages[page - 1]) if page and 0 < page <= len(pages) else None
    if page_text is not None and nq in page_text:
        return "exact"
    if nq in whole:
        return "wrong-page" if page else "exact"
    try:
        from rapidfuzz import fuzz
    except ImportError:
        return "not-found"
    target = page_text if page_text is not None else whole
    if fuzz.partial_ratio(nq, target) >= FUZZY_MIN:
        return "fuzzy"
    if page_text is not None and fuzz.partial_ratio(nq, whole) >= FUZZY_MIN:
        return "wrong-page"
    return "not-found"


def check_quotes(recs, write: bool) -> set[str]:
    srcs = {r["id"]: r for r, _ in recs["source"]}
    verified: set[str] = set()
    for r, p in recs["claim"]:
        status = quote_status(r, srcs.get(r.get("src")))
        stored = r.get("quote_check")
        if status in ("exact", "fuzzy"):
            verified.add(r["id"])
        if write:
            r["quote_check"] = status
        elif stored != status and status != "no-fulltext" and not (stored is None and status == "no-quote"):
            err(f"{r['id']}: quote_check stored '{stored}' but computed '{status}' (run with --write)")
        if status == "not-found":
            err(f"{r['id']}: quote not found in {r.get('src')} full text — fix or remove the quote")
        elif status == "wrong-page":
            warn(f"{r['id']}: quote found, but not on page {r.get('page')} (physical page index)")
        elif status == "no-fulltext" and r.get("quote"):
            warn(f"{r['id']}: cannot verify quote — {r.get('src')} has no cached full text "
                 f"(fetch_fulltext.py) ")
    return verified


def check_gaps(recs, verified: set[str]) -> None:
    for r, p in recs["gap"]:
        if r.get("status") != "candidate" and not (set(r.get("evidence", [])) & verified):
            err(f"{r['id']}: status '{r.get('status')}' requires >=1 evidence claim with a verified quote")
        if r.get("still_open") in ("yes", "no") and not r.get("still_open_checked_at"):
            warn(f"{r['id']}: still_open set without still_open_checked_at")


def check_budget(recs) -> None:
    paid = sum(r.get("calls", 1) for r, _ in recs["search"] if r.get("paid"))
    if paid > PAID_CAP:
        err(f"Parallel Search spend {paid} exceeds cap {PAID_CAP}")
    elif paid >= PAID_WARN:
        warn(f"Parallel Search spend {paid}/{PAID_CAP} — past the warning line")
    print(f"paid search calls: {paid}/{PAID_CAP}")


def crossref_title(doi: str) -> str | None:
    CROSSREF.mkdir(parents=True, exist_ok=True)
    cache = CROSSREF / f"{doi_slug(doi)}.json"
    if cache.exists():
        data = json.loads(cache.read_text())
    else:
        req = urllib.request.Request(f"https://api.crossref.org/works/{doi}", headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = json.load(resp)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                data = {"status": "not-found"}
            else:
                raise
        cache.write_text(json.dumps(data))
    if data.get("status") == "not-found":
        return None
    titles = data.get("message", {}).get("title") or [""]
    return titles[0]


def check_dois(recs, write: bool) -> None:
    from rapidfuzz import fuzz
    for r, p in recs["source"]:
        doi = r.get("doi")
        if not doi:
            continue
        try:
            title = crossref_title(doi)
        except Exception as e:  # network trouble: leave unchecked
            warn(f"{r['id']}: Crossref lookup failed ({e})")
            continue
        if title is None:
            status = "not-found"
        else:
            status = "match" if fuzz.token_sort_ratio(normalize(title), normalize(r["title"])) >= 85 else "mismatch"
        if status != "match":
            err(f"{r['id']}: DOI {doi} {status} (Crossref title: {title!r})")
        if write:
            r["doi_check"], r["doi_checked_at"] = status, date.today().isoformat()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write", action="store_true", help="store computed quote_check / doi_check values")
    ap.add_argument("--online", action="store_true", help="verify DOIs against Crossref (cached)")
    ap.add_argument("--staging", type=Path, help="agent staging dir to overlay and (with --write) update")
    a = ap.parse_args()

    roots = [ROOT] + ([a.staging.resolve()] if a.staging else [])
    recs = load(roots)
    check_schemas(recs)
    check_ids_and_refs(recs)
    check_vocab_and_dedupe(recs)
    verified = check_quotes(recs, a.write)
    check_gaps(recs, verified)
    if a.online:
        check_dois(recs, a.write)
    check_budget(recs)

    if a.write:
        writable = roots[-1]
        for rtype, (path, _, _) in REGISTRIES.items():
            target = writable / path.relative_to(ROOT)
            own = [r for r, p in recs[rtype] if p == target]
            if own:
                write_jsonl(target, own)

    counts = ", ".join(f"{t}={len(v)}" for t, v in recs.items() if v)
    print(f"records: {counts}")
    for w in warnings:
        print(f"WARN  {w}")
    for e in errors:
        print(f"ERROR {e}")
    print(f"{len(errors)} error(s), {len(warnings)} warning(s)")
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
