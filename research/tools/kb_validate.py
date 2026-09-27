#!/usr/bin/env python3
"""Validate the research knowledge base. Run before every commit.

    python research/tools/kb_validate.py                   # check main registries
    python research/tools/kb_validate.py --write           # also store computed quote_check values
    python research/tools/kb_validate.py --online --write  # also verify DOIs against Crossref
    python research/tools/kb_validate.py --staging research/staging/<agent>   # check an agent's staging
                                                           # area (overlaid on the main registries)

What is actually verified (and what is not):
- JSON Schema per record type; unique ids; every cross-reference resolves.
- Screening reason codes and topics come from schemas/vocab.json; dedupe keys are unique.
- Cached full text is unedited (its sha256 must equal the source's `text_sha256`).
- Every quote is found in that cached text (`quote_check` is computed here, never hand-set):
  exact | fuzzy (>=95) | wrong-page | not-found | no-fulltext | short (<8 words; code <20 chars)
  | unpaged (multi-page source, no page) | partial (a number in the paraphrase is not in the quote).
  Only exact/fuzzy count as verified.
- Code citations: the quote must appear in the cited lines (+-5) of the file at the pinned commit.
- Observations: the reproducing script must exist in the repo; local inputs must match their hash.
- NOT verified: that a quote *supports* the paraphrase beyond its numbers (entailment), and that
  a source is what its registry says beyond the title check done at fetch time.
Rules on top: gaps above `candidate`, damage classes at `accepted`, and `spec-ready` hypotheses
need verified, non-critique evidence; spec-ready also needs still-open gaps checked within 90 days.
Exit status 1 on any error.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.error
import urllib.request
from datetime import date, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import (CROSSREF, REGISTRIES, REPO, ROOT, SCHEMAS, code_checkout,  # noqa: E402
                      dedupe_key, doi_slug, fulltext_pages, fulltext_path, load_jsonl,
                      normalize, numbers_in, sha256_file, write_jsonl)

PAID_CAP, PAID_WARN = 80, 60
FUZZY_MIN = 95
MIN_QUOTE_WORDS, MIN_CODE_CHARS = 8, 20
STILL_OPEN_MAX_AGE = timedelta(days=90)
VERIFIED = {"exact", "fuzzy"}
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


def check_ids_and_refs(recs) -> None:
    ids: dict[str, set[str]] = {}
    for rtype, (_, prefix, _) in REGISTRIES.items():
        seen: set[str] = set()
        for r, p in recs[rtype]:
            if prefix is None:
                continue
            rid = r.get("id")
            if rid in seen:
                err(f"{p.name}: duplicate id {rid}")
            seen.add(rid)
        ids[rtype] = seen
    kind_of = {"CLM": "claim", "OBS": "observation", "SRC": "source", "GAP": "gap",
               "HYP": "hypothesis", "TOOL": "tool", "DMG": "damage", "SRCH": "search"}

    def ref(value, where: str) -> None:
        if value and value not in ids[kind_of[value.split("-")[0]]]:
            err(f"{where}: unknown reference {value}")

    for r, _ in recs["source"]:
        ref(r.get("duplicate_of"), r["id"])
        ref((r.get("provenance") or {}).get("search_id"), r["id"])
        if r.get("notes_path") and not (REPO / r["notes_path"]).exists():
            err(f"{r['id']}: notes_path {r['notes_path']} does not exist")
    for r, _ in recs["screening"]:
        ref(r.get("src"), f"screening {r.get('src')}")
    for r, _ in recs["claim"]:
        ref(r.get("src"), r.get("id", "?"))
        ref((r.get("provenance") or {}).get("search_id"), r.get("id", "?"))
    for r, _ in recs["gap"]:
        for x in r.get("evidence", []) + r.get("related_gaps", []) + r.get("hypotheses", []):
            ref(x, r["id"])
    for r, _ in recs["hypothesis"]:
        for x in r.get("gaps", []) + r.get("tools", []):
            ref(x, r["id"])
    for r, _ in recs["damage"]:
        for x in r.get("evidence", []):
            ref(x, r["id"])


def check_vocab_and_dedupe(recs) -> None:
    vocab = json.loads((SCHEMAS / "vocab.json").read_text())
    topics, reasons = set(vocab["topics"]), set(vocab["screening_reasons"])
    keys: dict[str, str] = {}
    for r, _ in recs["source"]:
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
        for al in r.get("aliases", []):
            keys.setdefault(al, r["id"])
    for r, _ in recs["screening"]:
        if r.get("reason_code") not in reasons:
            err(f"screening {r.get('src')}: reason_code {r.get('reason_code')} not in vocab.json")


def check_cache_integrity(recs) -> None:
    for r, _ in recs["source"]:
        sha, tsha = r.get("fulltext_sha256"), r.get("text_sha256")
        if not sha:
            continue
        p = fulltext_path(sha)
        if not p.exists():
            continue  # cache is per-machine; quote checks will report no-fulltext
        if not tsha:
            err(f"{r['id']}: has cached full text but no text_sha256 — re-run fetch_fulltext.py")
        elif hashlib.sha256(p.read_bytes()).hexdigest() != tsha:
            err(f"{r['id']}: cached text {p.name} was modified after fetch (text_sha256 mismatch)")


def _match(nq: str, target: str) -> str | None:
    if nq in target:
        return "exact"
    try:
        from rapidfuzz import fuzz
    except ImportError:
        return None
    return "fuzzy" if fuzz.partial_ratio(nq, target) >= FUZZY_MIN else None


def quote_status(claim: dict, src: dict | None) -> str:
    quote = claim.get("quote")
    if not quote:
        return "no-quote"
    src = src or {}
    nq = normalize(quote)

    if claim.get("locator"):  # code citation
        if len(nq) < MIN_CODE_CHARS:
            return "short"
        if not (src.get("url") and src.get("commit")):
            return "no-fulltext"
        f = code_checkout(src["url"], src["commit"]) / claim["locator"]["path"]
        if not f.exists():
            return "no-fulltext"
        lines = f.read_text(encoding="utf-8", errors="replace").splitlines()
        lo = max(0, claim["locator"]["line_start"] - 6)
        hi = claim["locator"]["line_end"] + 5
        status = _match(nq, normalize("\n".join(lines[lo:hi])))
        if status:
            return status
        return "wrong-page" if _match(nq, normalize("\n".join(lines))) else "not-found"

    if len(nq.split()) < MIN_QUOTE_WORDS:
        return "short"
    sha = src.get("fulltext_sha256")
    pages = fulltext_pages(sha) if sha else None
    if pages is None:
        return "no-fulltext"
    real_pages = [p for p in pages if p.strip()]
    page = claim.get("page")
    if page is None and len(real_pages) > 1:
        return "unpaged"
    whole = normalize("\n".join(pages))
    if page is not None:
        if not 0 < page <= len(pages):
            return "not-found"
        status = _match(nq, normalize(pages[page - 1]))
        if status is None:
            return "wrong-page" if _match(nq, whole) else "not-found"
    else:
        status = _match(nq, whole)
        if status is None:
            return "not-found"
    missing = numbers_in(claim.get("text", "")) - numbers_in(quote)
    return "partial" if missing else status


def check_quotes(recs, write: bool) -> set[str]:
    srcs = {r["id"]: r for r, _ in recs["source"]}
    verified: set[str] = set()
    for r, _ in recs["claim"]:
        status = quote_status(r, srcs.get(r.get("src")))
        stored = r.get("quote_check")
        if status in VERIFIED and r.get("kind") != "critique":
            verified.add(r["id"])
        if write:
            r["quote_check"] = status
        elif stored != status and status != "no-fulltext" and not (stored is None and status == "no-quote"):
            err(f"{r['id']}: quote_check stored '{stored}' but computed '{status}' (run with --write)")
        if status == "not-found":
            err(f"{r['id']}: quote not found in {r.get('src')} — fix or remove the quote")
        elif status == "partial":
            err(f"{r['id']}: numbers {sorted(numbers_in(r.get('text', '')) - numbers_in(r['quote']))} "
                f"in the paraphrase are not in the quote — quote the numbers or drop them from `text`")
        elif status in ("short", "unpaged"):
            err(f"{r['id']}: quote is {status} (>= {MIN_QUOTE_WORDS} words and a page are required)")
        elif status == "wrong-page":
            warn(f"{r['id']}: quote found, but not at the cited page/lines")
        elif status == "no-fulltext" and r.get("quote"):
            warn(f"{r['id']}: cannot verify quote — {r.get('src')} has no cached full text/checkout here")
    return verified


def check_observations(recs) -> set[str]:
    verified: set[str] = set()
    for r, _ in recs["observation"]:
        script = REPO / r.get("script", "")
        if not script.is_file():
            err(f"{r['id']}: script {r.get('script')} does not exist in the repo")
            continue
        ok = True
        for inp in r.get("inputs", []):
            p = Path(inp["path"]) if Path(inp["path"]).is_absolute() else REPO / inp["path"]
            if inp.get("sha256") and p.is_file() and sha256_file(p) != inp["sha256"]:
                err(f"{r['id']}: input {inp['path']} no longer matches its recorded sha256")
                ok = False
        if r.get("result_path") and not (REPO / r["result_path"]).exists():
            err(f"{r['id']}: result_path {r['result_path']} does not exist")
            ok = False
        if ok:
            verified.add(r["id"])
    return verified


def check_rules(recs, verified: set[str]) -> None:
    today = date.today()
    gaps = {r["id"]: r for r, _ in recs["gap"]}
    for g in gaps.values():
        if g.get("status") != "candidate" and not (set(g.get("evidence", [])) & verified):
            err(f"{g['id']}: status '{g.get('status')}' requires >=1 verified non-critique evidence (CLM/OBS)")
        if g.get("still_open") in ("yes", "no") and not g.get("still_open_checked_at"):
            warn(f"{g['id']}: still_open set without still_open_checked_at")
    for r, _ in recs["damage"]:
        if r.get("status") in ("accepted", "implemented") and not (set(r.get("evidence", [])) & verified):
            err(f"{r['id']}: status '{r['status']}' requires >=1 verified non-critique CLM or OBS")
    for h, _ in recs["hypothesis"]:
        if h.get("status") != "spec-ready":
            continue
        if not all(h["readiness"].values()):
            err(f"{h['id']}: spec-ready but readiness flags are not all true")
        ev = set()
        for gid in h.get("gaps", []):
            g = gaps.get(gid, {})
            ev |= set(g.get("evidence", []))
            checked = g.get("still_open_checked_at")
            if g.get("still_open") != "yes" or not checked or \
                    today - date.fromisoformat(checked) > STILL_OPEN_MAX_AGE:
                err(f"{h['id']}: gap {gid} must be still_open=yes, checked within {STILL_OPEN_MAX_AGE.days} days")
        if not ev & verified:
            err(f"{h['id']}: spec-ready requires its gaps to cite >=1 verified CLM/OBS")


def check_budget(recs) -> None:
    if not REGISTRIES["search"][0].exists():
        warn("search-log.jsonl is missing — every search must be logged (tools/log_search.py)")
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
            if e.code != 404:
                raise
            data = {"status": "not-found"}
        cache.write_text(json.dumps(data))
    if data.get("status") == "not-found":
        return None
    return (data.get("message", {}).get("title") or [""])[0]


def check_dois(recs, write: bool) -> None:
    from rapidfuzz import fuzz
    for r, _ in recs["source"]:
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
    check_cache_integrity(recs)
    verified = check_quotes(recs, a.write) | check_observations(recs)
    check_rules(recs, verified)
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

    print("records: " + ", ".join(f"{t}={len(v)}" for t, v in recs.items() if v))
    for w in warnings:
        print(f"WARN  {w}")
    for e in errors:
        print(f"ERROR {e}")
    print(f"{len(errors)} error(s), {len(warnings)} warning(s)")
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
