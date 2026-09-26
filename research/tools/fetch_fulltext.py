#!/usr/bin/env python3
"""Fetch (or copy) a source's full text into the gitignored cache and record its hash.

    python research/tools/fetch_fulltext.py SRC-0001 <url-or-local-path> [--registry PATH]

Stores research/cache/pdf/<sha256>.pdf and research/cache/fulltext/<sha256>.txt (pages
separated by form feed, physical page order), then sets `fulltext_sha256`, `fulltext_from`
and `access` on the source record. HTML sources are stored as text with no page breaks.
Only cache open-access or locally provided files; never commit the cache.
"""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import urllib.request
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from kbcommon import FULLTEXT, PDFCACHE, REGISTRIES, load_jsonl, write_jsonl  # noqa: E402

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"


def get_bytes(src: str) -> bytes:
    p = Path(src)
    if p.exists():
        return p.read_bytes()
    req = urllib.request.Request(src, headers={"User-Agent": UA, "Accept": "application/pdf,*/*"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def pdf_to_text(pdf: Path) -> str:
    try:
        return subprocess.run(["pdftotext", "-enc", "UTF-8", str(pdf), "-"],
                              check=True, capture_output=True).stdout.decode("utf-8", "replace")
    except (FileNotFoundError, subprocess.CalledProcessError):
        import pymupdf  # fallback
        with pymupdf.open(pdf) as doc:
            return "\f".join(page.get_text() for page in doc)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src_id")
    ap.add_argument("location")
    ap.add_argument("--registry", type=Path, default=REGISTRIES["source"][0],
                    help="registry or staging JSONL holding the source record")
    a = ap.parse_args()

    data = get_bytes(a.location)
    sha = hashlib.sha256(data).hexdigest()
    FULLTEXT.mkdir(parents=True, exist_ok=True)
    PDFCACHE.mkdir(parents=True, exist_ok=True)
    if data[:5] == b"%PDF-":
        pdf = PDFCACHE / f"{sha}.pdf"
        pdf.write_bytes(data)
        text = pdf_to_text(pdf)
    else:
        text = data.decode("utf-8", "replace")
    (FULLTEXT / f"{sha}.txt").write_text(text, encoding="utf-8")

    recs = load_jsonl(a.registry)
    for r in recs:
        if r.get("id") == a.src_id:
            r["fulltext_sha256"] = sha
            r["fulltext_from"] = a.location
            r["fulltext_fetched_at"] = date.today().isoformat()
            break
    else:
        sys.exit(f"{a.src_id} not found in {a.registry}")
    write_jsonl(a.registry, recs)
    pages = text.count("\f") + 1
    print(f"{a.src_id}: sha256={sha} pages={pages} words={len(text.split())}")


if __name__ == "__main__":
    main()
