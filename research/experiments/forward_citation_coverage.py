#!/usr/bin/env python3
"""OBS-0600: compare keyless forward-citation coverage (Crossref vs OpenCitations COCI)
across 10 already-merged sources spanning PDFPundit's fields.

    python3 research/experiments/forward_citation_coverage.py \
        research/experiments/results/forward_citation_coverage.json

For each DOI: fetches Crossref's `is-referenced-by-count` (works/<doi>) and OpenCitations
COCI's citer list (index/coci/api/v1/citations/<doi>), both keyless, both through
research/tools/polite_get.py (shared rate limiter). Prints a summary table and writes the
raw counts as JSON.
"""
from __future__ import annotations

import json
import sys
import time
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "research" / "tools"))
from polite_get import polite_get  # noqa: E402

# (SRC id, DOI, field/venue tag)
SOURCES = [
    ("SRC-0119", "10.1109/das.2018.64", "fonts/OCR (IEEE DAS 2018)"),
    ("SRC-0123", "10.1016/j.fsidi.2026.302054", "forensics (FSI:DI 2026)"),
    ("SRC-0001", "10.1016/j.fsidi.2026.302061", "forensics (FSI:DI 2026)"),
    ("SRC-0125", "10.1016/j.diin.2013.06.003", "compression (Digital Investigation)"),
    ("SRC-0303", "10.1145/3588195.3592992", "compression (ACM, rapidgzip)"),
    ("SRC-0411", "10.56553/popets-2023-0069", "security/fonts (PoPETs)"),
    ("SRC-0413", "10.1145/2733373.2806219", "fonts/ML (ACM MM, DeepFont)"),
    ("SRC-0426", "10.1109/jcdl.2017.7991564", "PDF/IR (IEEE JCDL)"),
    ("SRC-0112", "10.1109/tifs.2024.3445733", "forensics (IEEE TIFS)"),
    ("SRC-0407", "10.1109/icdar.2015.7333716", "OCR (IEEE ICDAR)"),
]


def crossref_count(doi: str) -> int | None:
    url = f"https://api.crossref.org/works/{urllib.parse.quote(doi)}"
    try:
        body = polite_get(url, accept="application/json")
    except Exception as e:  # noqa: BLE001
        print(f"  crossref error for {doi}: {e}", file=sys.stderr)
        return None
    data = json.loads(body)
    return data["message"].get("is-referenced-by-count")


def opencitations_count(doi: str) -> int | None:
    url = f"https://opencitations.net/index/api/v2/citations/doi:{urllib.parse.quote(doi)}"
    try:
        body = polite_get(url, accept="application/json")
    except Exception as e:  # noqa: BLE001
        print(f"  opencitations error for {doi}: {e}", file=sys.stderr)
        return None
    data = json.loads(body)
    return len(data) if isinstance(data, list) else None


def main() -> None:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    rows = []
    for src_id, doi, field in SOURCES:
        print(f"{src_id} {doi} ({field})")
        cr = crossref_count(doi)
        time.sleep(0.2)
        oc = opencitations_count(doi)
        print(f"  crossref is-referenced-by-count={cr}  opencitations citers={oc}")
        rows.append({"id": src_id, "doi": doi, "field": field,
                      "crossref_is_referenced_by_count": cr,
                      "opencitations_citers": oc})

    n = len(rows)
    cr_zero = sum(1 for r in rows if r["crossref_is_referenced_by_count"] == 0)
    oc_zero = sum(1 for r in rows if r["opencitations_citers"] == 0)
    cr_nonzero = sum(1 for r in rows if (r["crossref_is_referenced_by_count"] or 0) > 0)
    oc_nonzero = sum(1 for r in rows if (r["opencitations_citers"] or 0) > 0)
    summary = {
        "n_sources": n,
        "crossref_zero": cr_zero,
        "crossref_nonzero": cr_nonzero,
        "opencitations_zero": oc_zero,
        "opencitations_nonzero": oc_nonzero,
        "rows": rows,
    }
    print(json.dumps(summary, indent=2))
    if out_path:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(summary, indent=2))
        print(f"wrote {out_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
