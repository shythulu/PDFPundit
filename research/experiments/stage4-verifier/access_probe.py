#!/usr/bin/env python3
"""Stage 4 verifier: one request per keyless discovery API and per access-gate host (2026-10-05).

    python3 access_probe.py <out-prefix> [label ...]   # labels: run only those checks (the CORE retry)

For each endpoint it makes exactly one request (no retry), using the per-host lock files that
research/tools/polite_get.py uses, so the spacing holds across agents. It records the HTTP status,
the rate-limit and retry headers, the body size and sha256, and any proxy/TLS error (a CONNECT
refused by the egress proxy shows up as an error with status None). Afterwards it reads
$HTTPS_PROXY/__agentproxy/status for relay failures.

Model hosts: the model paths are read at run time from the ledger entries TOOL-386 (Ollama
registry) and the brief's Hugging Face example; the output records them as <TOOL-386 model> and
<HF model> so no model identifier is written into this repository.
"""
from __future__ import annotations

import fcntl
import hashlib
import json
import os
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
LOCKDIR = Path(os.environ.get("TMPDIR", tempfile.gettempdir())) / "pdfpundit-polite"
MIN_GAP = {"api.crossref.org": 2.0}
KEEP = ("x-ratelimit-limit", "x-ratelimit-remaining", "x-ratelimit-reset", "x-ratelimit-interval",
        "x-rate-limit-limit", "x-rate-limit-remaining", "x-rate-limit-reset", "x-rate-limit-interval",
        "retry-after", "x-api-key", "ratelimit-limit", "ratelimit-remaining", "ratelimit-reset",
        "x-amzn-requestid", "content-type", "location", "server", "x-ratelimit-requests-remaining",
        "x-ratelimit-limit-requests", "x-error-code")


def ledger_url(tid: str) -> str:
    for ln in open(REPO / "research/tooling/ledger.jsonl"):
        r = json.loads(ln)
        if r["id"] == tid:
            return r["url"]
    raise KeyError(tid)


def one(url: str, method: str = "GET", accept: str = "*/*", max_body: int = 50_000_000) -> dict:
    host = urllib.parse.urlparse(url).hostname
    LOCKDIR.mkdir(parents=True, exist_ok=True)
    stamp = LOCKDIR / f"{host}.last"
    with open(LOCKDIR / f"{host}.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        last = float(stamp.read_text()) if stamp.exists() else 0.0
        wait = MIN_GAP.get(host, 1.0) - (time.time() - last)
        if wait > 0:
            time.sleep(wait)
        t0 = time.time()
        rec = {"method": method, "status": None, "error": None}
        try:
            rq = urllib.request.Request(url, method=method, headers={"User-Agent": UA, "Accept": accept})
            with urllib.request.urlopen(rq, timeout=60) as r:
                body = r.read(max_body) if method == "GET" else b""
                rec.update(status=r.status, final_url=r.geturl(), headers=dict(r.headers))
        except urllib.error.HTTPError as e:
            body = e.read() if method == "GET" else b""
            rec.update(status=e.code, headers=dict(e.headers or {}))
        except Exception as e:  # noqa: BLE001
            body = b""
            rec["error"] = f"{type(e).__name__}: {e}"[:300]
        stamp.write_text(str(time.time()))
    rec["elapsed_s"] = round(time.time() - t0, 3)
    hdr = {k.lower(): v for k, v in (rec.pop("headers", None) or {}).items()}
    rec["headers"] = {k: hdr[k] for k in KEEP if k in hdr}
    rec["bytes"] = len(body)
    rec["sha256"] = hashlib.sha256(body).hexdigest() if body else None
    rec["body_head"] = body[:160].decode("utf-8", "replace") if body else None
    rec["is_pdf"] = body[:5] == b"%PDF-"
    return rec


def main() -> None:
    out = Path(sys.argv[1])
    doi = "10.1109/das.2018.64"   # the ledger's own probe DOI (TOOL-300/301/310)
    ollama = urllib.parse.urlparse(ledger_url("TOOL-386")).path.split("/library/")[1]   # name:tag
    oname, otag = ollama.split(":")
    hf = os.environ.get("HF_MODEL_PATH", "")
    checks = [
        # (label, url, method, accept, redact)
        ("crossref", "https://api.crossref.org/works?query.bibliographic=pdf+repair&rows=1", "GET", "application/json", None),
        ("core", "https://api.core.ac.uk/v3/search/works?q=pdf+repair&limit=1", "GET", "application/json", None),
        ("unpaywall", f"https://api.unpaywall.org/v2/{doi}?email=research@pdfpundit.invalid", "GET", "application/json", None),
        ("opencitations", f"https://opencitations.net/index/api/v2/citations/doi:{doi}", "GET", "application/json", None),
        ("openalex", f"https://api.openalex.org/works/doi:{doi}", "GET", "application/json", None),
        ("semantic-scholar", f"https://api.semanticscholar.org/graph/v1/paper/DOI:{doi}?fields=title", "GET", "application/json", None),
        ("repdf.site", "https://repdf.site/", "GET", "*/*", None),
        ("www.ohchr.org", "https://www.ohchr.org/", "HEAD", "*/*", None),
        ("web.archive.org", "https://web.archive.org/", "HEAD", "*/*", None),
        ("napierone-bucket (path-style; the bucket name has a dot)",
         "https://s3.eu-north-1.amazonaws.com/napierone.com/?list-type=2&max-keys=1", "GET", "*/*", None),
        ("registry.ollama.ai (manifest only)",
         f"https://registry.ollama.ai/v2/library/{oname}/manifests/{otag}", "GET",
         "application/vnd.docker.distribution.manifest.v2+json", (ollama, "<TOOL-386 model>")),
        ("SRC-0132 direct", "https://nti.khai.edu/ojs/index.php/reks/article/download/reks.2023.1.14/2018", "GET", "*/*", None),
        ("SRC-0132 via doi.org", "https://doi.org/10.32620/reks.2023.1.14", "GET", "*/*", None),
    ]
    if hf:
        checks.append(("huggingface (model metadata, unauthenticated)", f"https://huggingface.co/api/models/{hf}",
                       "GET", "application/json", (hf, "<HF model>")))
    only = sys.argv[2:]
    if only:
        checks = [c for c in checks if c[0] in only]
    res = []
    for label, url, method, accept, redact in checks:
        r = one(url, method, accept)
        shown = url.replace(*redact).replace(redact[0].split(":")[0], redact[1]) if redact else url
        r = {"label": label, "url": shown, **r}
        if redact:
            for k in ("final_url", "body_head"):
                if r.get(k):
                    r[k] = r[k].replace(*redact).replace(redact[0].split(":")[0], redact[1])
        res.append(r)
        print(json.dumps({k: r.get(k) for k in ("label", "status", "error", "bytes", "headers", "elapsed_s")}), flush=True)
    try:
        st = json.loads(urllib.request.urlopen(os.environ["HTTPS_PROXY"].rstrip("/") + "/__agentproxy/status",
                                               timeout=10).read())
        relay = st.get("recentRelayFailures")
    except Exception as e:  # noqa: BLE001
        relay = f"status endpoint error: {e}"
    out.with_suffix(".json").write_text(json.dumps({"date": time.strftime("%Y-%m-%d"), "results": res,
                                                    "proxy_recent_relay_failures": relay}, indent=1) + "\n")


if __name__ == "__main__":
    main()
