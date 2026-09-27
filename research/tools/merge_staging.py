#!/usr/bin/env python3
"""Merge agent staging areas into the main registries (run by the chair, one agent at a time).

    python research/tools/merge_staging.py research/staging/<agent> [--dry-run]

Staging dirs mirror research/ (sources/registry.jsonl, sources/claims.jsonl, gaps/gaps.jsonl, ...).
Agents write only inside their own id blocks (see agents/brief-template.md), so ids never
collide; this script still refuses to merge on a collision. Sources whose dedupe key (or an
alias) already exists are folded into the existing record (aliases/topics unioned) and every
reference in the staging records is rewritten to the surviving id. Notes files move to
research/sources/notes/. A MERGED marker is left in the staging dir.
"""

from __future__ import annotations

import argparse
import shutil
import sys
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import REGISTRIES, ROOT, dedupe_key, load_jsonl, write_jsonl  # noqa: E402

FULLTEXT_FIELDS = ("fulltext_sha256", "text_sha256", "extractor", "extractor_version",
                   "fulltext_from", "fulltext_fetched_at", "title_check", "access")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("staging", type=Path)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    stg = a.staging.resolve()
    if (stg / "MERGED").exists():
        sys.exit(f"{stg} already merged")

    main_recs = {t: load_jsonl(p) for t, (p, _, _) in REGISTRIES.items()}
    stg_recs = {t: load_jsonl(stg / p.relative_to(ROOT)) for t, (p, _, _) in REGISTRIES.items()}

    # 1. fold duplicate sources
    by_key: dict[str, dict] = {}
    for r in main_recs["source"]:
        by_key[dedupe_key(r)] = r
        for al in r.get("aliases", []):
            by_key.setdefault(al, r)
    remap: dict[str, str] = {}
    fresh = []
    for r in stg_recs["source"]:
        keys = [dedupe_key(r)] + r.get("aliases", [])
        hit = next((by_key[k] for k in keys if k in by_key), None)
        if hit:
            remap[r["id"]] = hit["id"]
            hit["aliases"] = sorted(set(hit.get("aliases", [])) | {k for k in keys if k != dedupe_key(hit)})
            hit["topics"] = sorted(set(hit.get("topics", [])) | set(r.get("topics", [])))
            if r.get("fulltext_sha256") and not hit.get("fulltext_sha256"):
                for f in FULLTEXT_FIELDS:        # the cached-text fields travel together or not at all
                    if f in r:
                        hit[f] = r[f]
            for f in ("notes_path", "summary", "evidence_grade", "relevance"):
                if r.get(f) and not hit.get(f):
                    hit[f] = r[f]
        else:
            fresh.append(r)
            for k in keys:
                by_key[k] = r
    stg_recs["source"] = fresh

    def fix(v: str | None) -> str | None:
        return remap.get(v, v) if v else v

    for r in stg_recs["claim"] + stg_recs["screening"]:
        r["src"] = fix(r.get("src"))
    for r in stg_recs["damage"]:
        r["evidence"] = [fix(e) for e in r.get("evidence", [])]

    # 2. collision check + append
    report = []
    for t, (path, prefix, _) in REGISTRIES.items():
        if not stg_recs[t]:
            continue
        if prefix:
            existing = {r["id"] for r in main_recs[t]}
            clash = [r["id"] for r in stg_recs[t] if r["id"] in existing]
            if clash:
                sys.exit(f"id collision in {t}: {clash[:5]} — agent wrote outside its id block")
        main_recs[t] += stg_recs[t]
        report.append(f"{t}: +{len(stg_recs[t])}")
    report.append(f"sources folded into existing: {len(remap)} {remap if remap else ''}")
    print("\n".join(report))
    if a.dry_run:
        return

    # 3. move notes
    notes_src = stg / "sources" / "notes"
    if notes_src.exists():
        dest = ROOT / "sources" / "notes"
        dest.mkdir(parents=True, exist_ok=True)
        for f in notes_src.glob("*.md"):
            shutil.move(str(f), dest / f.name)
        for r in main_recs["source"]:
            np_ = r.get("notes_path") or ""
            if "staging/" in np_:
                r["notes_path"] = f"research/sources/notes/{Path(np_).name}"

    for t, (path, _, _) in REGISTRIES.items():
        write_jsonl(path, main_recs[t])
    (stg / "MERGED").write_text(f"merged {date.today().isoformat()}\n" + "\n".join(report) + "\n")


if __name__ == "__main__":
    main()
