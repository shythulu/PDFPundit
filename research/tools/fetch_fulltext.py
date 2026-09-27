#!/usr/bin/env python3
"""Fetch (or copy) a source's full text into the gitignored cache and record its hashes.

    python research/tools/fetch_fulltext.py SRC-0001 <url-or-local-path> [--registry PATH]
        [--local-ok] [--no-title-check REASON]

Stores research/cache/pdf/<sha256>.pdf and research/cache/fulltext/<sha256>.txt (pages
separated by form feed, physical page order), then records on the source record:
`fulltext_sha256` (hash of the fetched bytes), `text_sha256` (hash of the extracted text, so any
later edit of the cache is detected), `extractor` + `extractor_version`, `fulltext_from`,
`title_check`.

Guards against self-made evidence:
- a paper/preprint/thesis/standard/book must be a PDF, unless --local-ok (then access=local);
- its registry title must appear (fuzzy >= 85) in the first pages of the extracted text,
  unless --no-title-check "<reason>" (recorded as title_check: skipped:<reason>).
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
from kbcommon import (DOCUMENT_TYPES, FULLTEXT, PDFCACHE, REGISTRIES, load_jsonl,  # noqa: E402
                      normalize, write_jsonl)

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
TITLE_MIN = 85


def get_bytes(src: str) -> tuple[bytes, bool]:
    p = Path(src)
    if p.exists():
        return p.read_bytes(), True
    req = urllib.request.Request(src, headers={"User-Agent": UA, "Accept": "application/pdf,*/*"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read(), False


def pdf_to_text(pdf: Path) -> tuple[str, str, str]:
    """Return (text, extractor, version)."""
    try:
        out = subprocess.run(["pdftotext", "-enc", "UTF-8", str(pdf), "-"],
                             check=True, capture_output=True).stdout.decode("utf-8", "replace")
        ver = subprocess.run(["pdftotext", "-v"], capture_output=True, text=True).stderr.splitlines()[0]
        return out, "pdftotext", ver.strip()
    except (FileNotFoundError, subprocess.CalledProcessError, IndexError):
        import pymupdf
        with pymupdf.open(pdf) as doc:
            return "\f".join(page.get_text() for page in doc), "pymupdf", pymupdf.__version__


def title_found(title: str, text: str) -> bool:
    from rapidfuzz import fuzz
    head = normalize("\n".join(text.split("\f")[:2]))[:20000]
    return fuzz.partial_ratio(normalize(title), head) >= TITLE_MIN


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src_id")
    ap.add_argument("location")
    ap.add_argument("--registry", type=Path, default=REGISTRIES["source"][0],
                    help="registry or staging JSONL holding the source record")
    ap.add_argument("--local-ok", action="store_true",
                    help="allow a non-PDF local file for a document-type source (sets access=local)")
    ap.add_argument("--no-title-check", metavar="REASON",
                    help="skip the title check, recording why (e.g. 'scanned PDF, no text layer')")
    a = ap.parse_args()

    recs = load_jsonl(a.registry)
    rec = next((r for r in recs if r.get("id") == a.src_id), None)
    if rec is None:
        sys.exit(f"{a.src_id} not found in {a.registry}")

    data, is_local = get_bytes(a.location)
    is_pdf = data[:5] == b"%PDF-"
    is_document = rec.get("type") in DOCUMENT_TYPES
    if is_document and not is_pdf and not a.local_ok:
        sys.exit(f"REFUSED: {a.src_id} is a {rec.get('type')} but {a.location} is not a PDF. "
                 "Fetch the actual document, or pass --local-ok if a text/HTML copy is genuinely the source.")

    sha = hashlib.sha256(data).hexdigest()
    FULLTEXT.mkdir(parents=True, exist_ok=True)
    PDFCACHE.mkdir(parents=True, exist_ok=True)
    if is_pdf:
        pdf = PDFCACHE / f"{sha}.pdf"
        pdf.write_bytes(data)
        text, extractor, version = pdf_to_text(pdf)
    else:
        text, extractor, version = data.decode("utf-8", "replace"), "raw", "n/a"

    if not is_document:
        title_check = "n/a"
    elif a.no_title_check:
        title_check = f"skipped:{a.no_title_check}"
    elif title_found(rec.get("title", ""), text):
        title_check = "pass"
    else:
        sys.exit(f"REFUSED: the registry title of {a.src_id} does not appear in the first pages of "
                 f"{a.location}. Wrong file, or pass --no-title-check '<reason>'.")

    txt_path = FULLTEXT / f"{sha}.txt"
    txt_path.write_text(text, encoding="utf-8")
    rec.update({
        "fulltext_sha256": sha,
        "text_sha256": hashlib.sha256(txt_path.read_bytes()).hexdigest(),
        "extractor": extractor,
        "extractor_version": version,
        "fulltext_from": a.location,
        "fulltext_fetched_at": date.today().isoformat(),
        "title_check": title_check,
    })
    if is_local and not is_pdf and a.local_ok and is_document:
        rec["access"] = "local"
    write_jsonl(a.registry, recs)
    print(f"{a.src_id}: sha256={sha} pages={text.count(chr(12)) + 1} words={len(text.split())} "
          f"extractor={extractor} title_check={title_check}")


if __name__ == "__main__":
    main()
