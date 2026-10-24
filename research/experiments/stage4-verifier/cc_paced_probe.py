#!/usr/bin/env python3
"""Stage 4 verifier: paced re-test of Common Crawl ranged reads (TOOL-403/404, WP-3.7) and a
re-check of the digitalcorpora unsigned-S3 technique (TOOL-402).

    python3 cc_paced_probe.py <out-prefix> [--max-cc 30] [--gap 2.0]

Part A (digitalcorpora, no credentials; TOOL-402 / OBS-0800):
  1. ListObjectsV2 on the CC-MAIN-2021-31-PDF-UNTRUNCATED metadata prefix;
  2. range-read the first 2,000,000 bytes of cc-provenance-20230303.csv.gz and count the complete CSV
     rows that decompress (OBS-0800 recorded 12,946);
  3. read the central directory of zipfiles/0000-0999/0001.zip by HTTP ranges and count entries
     (OBS-0800: 1,000).
  From the rows of step 2 it takes the first REFETCHED_SUCCESS row with cc_truncated set, as a real
  WARC record locator for Part B.
Part B (data.commoncrawl.org; at most --max-cc requests, --gap seconds apart):
  small ranged GETs (1 KiB) cycling over three targets: crawl-data/CC-MAIN-2021-31/warc.paths.gz,
  crawl-data/CC-MAIN-2021-31/cc-index.paths.gz, and the first 1 KiB of the WARC record found in
  Part A. On HTTP 403 it backs off 30 s, then 60 s, then 120 s (each further 403 doubles, cap 120 s);
  after 6 consecutive 403s it stops. Every request counts toward the cap, and the status sequence is
  recorded with timestamps.
Outputs: <out-prefix>.json (every request: target, status, bytes, elapsed, time).
"""
from __future__ import annotations

import argparse
import csv
import io
import json
import struct
import time
import urllib.error
import urllib.request
import zlib
from datetime import datetime, timezone

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
DC = "https://digitalcorpora.s3.amazonaws.com/"
PFX = "corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/"
META = DC + PFX + "metadata/cc-provenance-20230303.csv.gz"
ZIP1 = DC + PFX + "zipfiles/0000-0999/0001.zip"
CC = "https://data.commoncrawl.org/"


def req(url, rng=None, method="GET", timeout=60):
    h = {"User-Agent": UA}
    if rng:
        h["Range"] = f"bytes={rng[0]}-{rng[1]}"
    t0 = time.time()
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=h, method=method), timeout=timeout) as r:
            body = r.read()
            return r.status, body, dict(r.headers), round(time.time() - t0, 3), None
    except urllib.error.HTTPError as e:
        body = e.read() if hasattr(e, "read") else b""
        return e.code, body, dict(e.headers or {}), round(time.time() - t0, 3), None
    except Exception as e:  # noqa: BLE001 - record proxy/TLS/network errors
        return None, b"", {}, round(time.time() - t0, 3), f"{type(e).__name__}: {e}"[:300]


def now():
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def part_a(log):
    out = {}
    st, body, _, el, er = req(DC + f"?list-type=2&prefix={PFX}metadata/&max-keys=20")
    out["list_objects"] = {"status": st, "elapsed_s": el, "error": er,
                           "keys": body.decode("utf-8", "replace").count("<Key>")}
    st, body, _, el, er = req(META, (0, 1_999_999))
    rows = []
    if st == 206:
        d = zlib.decompressobj(16 + zlib.MAX_WBITS)
        text = d.decompress(body).decode("utf-8", "replace")
        complete = text[: text.rfind("\n") + 1]
        rows = list(csv.DictReader(io.StringIO(complete)))
    out["metadata_range"] = {"status": st, "elapsed_s": el, "error": er, "bytes": len(body), "rows": len(rows)}
    # zip central directory: read the tail, find EOCD (or ZIP64 EOCD), then the directory
    st, head, hdrs, el1, er = req(ZIP1, method="HEAD")
    size = int(hdrs.get("Content-Length", 0)) if st == 200 else 0
    n_entries, el2 = None, None
    if size:
        st2, tail, _, el2, er2 = req(ZIP1, (size - 65_536, size - 1))
        i = tail.rfind(b"PK\x05\x06")
        if i >= 0:
            n_entries = struct.unpack("<H", tail[i + 10:i + 12])[0]
            if n_entries == 0xFFFF:
                j = tail.rfind(b"PK\x06\x06")
                n_entries = struct.unpack("<Q", tail[j + 32:j + 40])[0] if j >= 0 else None
    out["zip_central_directory"] = {"head_status": st, "size": size, "entries": n_entries,
                                    "elapsed_s": round((el1 or 0) + (el2 or 0), 3)}
    loc = next((r for r in rows if r.get("cc_truncated") and r.get("fetched_status") == "REFETCHED_SUCCESS"), None)
    out["warc_locator"] = ({k: loc[k] for k in ("url_id", "cc_truncated", "cc_warc_file_name", "cc_warc_start",
                                                "cc_warc_end")} if loc else None)
    log.append({"part": "A", **out})
    return out, loc


def part_b(loc, max_cc, gap):
    targets = [("warc.paths.gz", CC + "crawl-data/CC-MAIN-2021-31/warc.paths.gz", (0, 1023)),
               ("cc-index.paths.gz", CC + "crawl-data/CC-MAIN-2021-31/cc-index.paths.gz", (0, 1023))]
    if loc:
        s = int(loc["cc_warc_start"])
        targets.append(("warc-record", CC + loc["cc_warc_file_name"], (s, s + 1023)))
    seq, consec403, backoff = [], 0, 30
    for i in range(max_cc):
        name, url, rng = targets[i % len(targets)]
        st, body, hdrs, el, er = req(url, rng)
        seq.append({"n": i + 1, "time": now(), "target": name, "range": f"{rng[0]}-{rng[1]}", "status": st,
                    "bytes": len(body), "elapsed_s": el, "error": er,
                    "retry_after": hdrs.get("Retry-After"), "server": hdrs.get("Server")})
        print(json.dumps(seq[-1]), flush=True)
        if st == 403:
            consec403 += 1
            if consec403 >= 6:
                break
            time.sleep(backoff)
            backoff = min(backoff * 2, 120)
        else:
            consec403, backoff = 0, 30
            time.sleep(gap)
    return seq


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--max-cc", type=int, default=30)
    ap.add_argument("--gap", type=float, default=2.0)
    a = ap.parse_args()
    log = []
    t0 = time.time()
    a_out, loc = part_a(log)
    print(json.dumps(a_out), flush=True)
    seq = part_b(loc, a.max_cc, a.gap)
    codes = [s["status"] for s in seq]
    summary = {"started": now(), "part_a": a_out, "part_b_requests": seq,
               "part_b_status_sequence": codes,
               "part_b_counts": {str(c): codes.count(c) for c in sorted(set(codes), key=str)},
               "wall_s": round(time.time() - t0, 1)}
    with open(a.out + ".json", "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps({k: summary[k] for k in ("part_b_status_sequence", "part_b_counts", "wall_s")}))


if __name__ == "__main__":
    main()
