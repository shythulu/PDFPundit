#!/usr/bin/env python3
"""Rate-limited GET shared by every agent in this container (one IP, many agents).

    python research/tools/polite_get.py '<url>' [-o out_file] [--accept application/json]

Serializes requests per host through a lock file under $TMPDIR/pdfpundit-polite/, keeping a
minimum gap between requests to the same host (Crossref 2 s, DBLP 3 s, others 1 s). On HTTP 429
or 503 it waits 60 s and retries, at most 3 attempts, then exits non-zero. Body goes to stdout
(or -o). Use it for every free-API call (Crossref, DBLP, OpenCitations, arXiv, crates.io, PyPI).
"""

from __future__ import annotations

import argparse
import fcntl
import os
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
MIN_GAP = {"api.crossref.org": 2.0, "dblp.org": 3.0, "export.arxiv.org": 3.0}
LOCKDIR = Path(os.environ.get("TMPDIR", tempfile.gettempdir())) / "pdfpundit-polite"


def polite_get(url: str, accept: str = "*/*", attempts: int = 3) -> bytes:
    host = urllib.parse.urlparse(url).hostname or "unknown"
    LOCKDIR.mkdir(parents=True, exist_ok=True)
    stamp = LOCKDIR / f"{host}.last"
    for attempt in range(attempts):
        with open(LOCKDIR / f"{host}.lock", "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)          # one request per host at a time, container-wide
            last = float(stamp.read_text()) if stamp.exists() else 0.0
            wait = MIN_GAP.get(host, 1.0) - (time.time() - last)
            if wait > 0:
                time.sleep(wait)
            try:
                req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": accept})
                with urllib.request.urlopen(req, timeout=60) as r:
                    body = r.read()
                stamp.write_text(str(time.time()))
                return body
            except urllib.error.HTTPError as e:
                stamp.write_text(str(time.time()))
                if e.code not in (429, 503) or attempt == attempts - 1:
                    raise
        print(f"polite_get: HTTP {e.code} from {host}, backing off 60 s (attempt {attempt + 1})", file=sys.stderr)
        time.sleep(60)
    raise RuntimeError("unreachable")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("url")
    ap.add_argument("-o", "--out", type=Path)
    ap.add_argument("--accept", default="*/*")
    a = ap.parse_args()
    try:
        body = polite_get(a.url, a.accept)
    except urllib.error.HTTPError as e:
        sys.exit(f"HTTP {e.code} for {a.url}")
    if a.out:
        a.out.write_bytes(body)
    else:
        sys.stdout.buffer.write(body)


if __name__ == "__main__":
    main()
