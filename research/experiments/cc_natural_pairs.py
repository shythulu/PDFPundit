#!/usr/bin/env python3
"""Check whether Common Crawl's truncated PDF captures are byte prefixes of the complete files that
SafeDocs later refetched (CC-MAIN-2021-31-PDF-UNTRUNCATED). Such prefix-verified pairs are natural
pairs in the sense of charter §3(a): real truncation damage with a known original.

    python research/experiments/cc_natural_pairs.py [--n 30] \
        [--workdir /tmp/cc-safedocs] \
        [--out research/experiments/results/cc_natural_pairs.csv]

Input: the 1k provenance table (rows for the files in zipfiles/0000-0999/0000.zip). Rows with
cc_truncated != '' and fetched_status == REFETCHED_SUCCESS are sorted by url_id and the first --n taken
(deterministic). For each row it:
  1. range-fetches the WARC record [cc_warc_start, cc_warc_end] from data.commoncrawl.org (1.5 s apart),
     gunzips it and extracts the HTTP payload (the truncated capture) and the WARC-Truncated header;
  2. reads the same number of bytes (+1) of the refetched file from 0000.zip with HTTP range requests
     (zip central directory + one member, streamed);
  3. records capture length, whether the capture is a byte prefix of the refetched file, the first
     mismatching offset, and sha256 of both prefixes.
Only lengths, flags and hashes are written; no document bytes are stored.
"""
from __future__ import annotations

import argparse, csv, hashlib, io, json, time, urllib.request, zipfile, zlib
from pathlib import Path

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
META = "https://digitalcorpora.s3.amazonaws.com/corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/metadata/cc-provenance-20230324-1k.csv"
ZIP = "https://digitalcorpora.s3.amazonaws.com/corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/zipfiles/0000-0999/0000.zip"
CC = "https://data.commoncrawl.org/"


def get(url: str, rng: tuple[int, int] | None = None, tries: int = 4) -> bytes:
    for i in range(tries):
        hdr = {"User-Agent": UA}
        if rng:
            hdr["Range"] = f"bytes={rng[0]}-{rng[1]}"
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=hdr), timeout=120) as r:
                return r.read()
        except Exception as e:  # CC throttles with 403/503: back off
            if i == tries - 1:
                raise
            time.sleep(20 * (i + 1))
    raise RuntimeError("unreachable")


class HttpRangeFile(io.RawIOBase):
    """Minimal seekable read-only file over HTTP range requests, with a block cache."""
    BLOCK = 1 << 20

    def __init__(self, url: str):
        self.url, self.pos, self.cache = url, 0, {}
        req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": UA})
        with urllib.request.urlopen(req, timeout=60) as r:
            self.size = int(r.headers["Content-Length"])
        self.requests = 0

    def readable(self): return True
    def seekable(self): return True
    def tell(self): return self.pos

    def seek(self, off, whence=0):
        self.pos = off if whence == 0 else self.pos + off if whence == 1 else self.size + off
        return self.pos

    def _block(self, b):
        if b not in self.cache:
            lo = b * self.BLOCK
            self.cache[b] = get(self.url, (lo, min(self.size, lo + self.BLOCK) - 1))
            self.requests += 1
            if len(self.cache) > 64:
                self.cache.pop(next(iter(self.cache)))
        return self.cache[b]

    def read(self, n=-1):
        if n is None or n < 0:
            n = self.size - self.pos
        out = bytearray()
        while n > 0 and self.pos < self.size:
            b, off = divmod(self.pos, self.BLOCK)
            chunk = self._block(b)[off:off + n]
            out += chunk
            self.pos += len(chunk)
            n -= len(chunk)
        return bytes(out)

    def readinto(self, buf):
        data = self.read(len(buf))
        buf[:len(data)] = data
        return len(data)


def parse_warc(gz: bytes) -> dict:
    raw = zlib.decompressobj(31).decompress(gz)
    head, _, rest = raw.partition(b"\r\n\r\n")
    wh = dict(l.split(b":", 1) for l in head.split(b"\r\n")[1:] if b":" in l)
    wh = {k.strip().lower().decode(): v.strip().decode("latin-1") for k, v in wh.items()}
    block = rest[:int(wh.get("content-length", len(rest)))]
    http_head, _, payload = block.partition(b"\r\n\r\n")
    hh = {}
    for l in http_head.split(b"\r\n")[1:]:
        if b":" in l:
            k, v = l.split(b":", 1)
            hh[k.strip().lower().decode("latin-1")] = v.strip().decode("latin-1")
    return {"warc_truncated": wh.get("warc-truncated", ""), "payload": payload,
            "http_transfer_encoding": hh.get("transfer-encoding", ""), "http_content_encoding": hh.get("content-encoding", ""),
            "http_content_length": hh.get("content-length", "")}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=30)
    ap.add_argument("--workdir", type=Path, default=Path("/tmp/cc-safedocs"))
    ap.add_argument("--out", type=Path, default=Path("research/experiments/results/cc_natural_pairs.csv"))
    a = ap.parse_args()
    a.workdir.mkdir(parents=True, exist_ok=True)
    meta = a.workdir / "cc-provenance-20230324-1k.csv"
    if not meta.exists():
        meta.write_bytes(get(META))
    meta_sha = hashlib.sha256(meta.read_bytes()).hexdigest()
    with open(meta, encoding="utf-8-sig", newline="") as f:
        rows = [r for r in csv.DictReader(f) if r["cc_truncated"] and r["fetched_status"] == "REFETCHED_SUCCESS"]
    rows.sort(key=lambda r: int(r["url_id"]))
    picked = rows[:a.n]

    zf = zipfile.ZipFile(HttpRangeFile(ZIP))
    names = set(zf.namelist())
    out = []
    for r in picked:
        rec = {"url_id": r["url_id"], "file_name": r["file_name"], "cc_truncated": r["cc_truncated"],
               "fetched_length": r["fetched_length"]}
        try:
            w = parse_warc(get(CC + r["cc_warc_file_name"], (int(r["cc_warc_start"]), int(r["cc_warc_end"]))))
            time.sleep(1.5)
            cap = w["payload"]
            rec.update({k: w[k] for k in ("warc_truncated", "http_transfer_encoding", "http_content_encoding", "http_content_length")})
            rec["capture_len"] = len(cap)
            rec["capture_starts_pdf"] = cap[:5] == b"%PDF-"
            if r["file_name"] not in names:
                rec["status"] = "not-in-zip"
            else:
                zi = zf.getinfo(r["file_name"])
                rec["zip_file_size"] = zi.file_size
                with zf.open(zi) as fh:
                    ref = fh.read(len(cap) + 1)
                pre = ref[:len(cap)]
                rec["prefix_match"] = pre == cap
                rec["first_mismatch"] = next((i for i, (x, y) in enumerate(zip(pre, cap)) if x != y), None if len(pre) == len(cap) else min(len(pre), len(cap)))
                rec["capture_sha256"] = hashlib.sha256(cap).hexdigest()
                rec["refetch_prefix_sha256"] = hashlib.sha256(pre).hexdigest()
                rec["refetch_longer"] = len(ref) > len(cap)
                rec["status"] = "ok"
        except Exception as e:
            rec["status"] = f"error: {type(e).__name__}: {str(e)[:80]}"
        out.append(rec)
        print(json.dumps(rec))

    a.out.parent.mkdir(parents=True, exist_ok=True)
    cols = sorted({k for r in out for k in r})
    with open(a.out, "w", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=cols)
        wr.writeheader()
        wr.writerows(out)
    ok = [r for r in out if r.get("status") == "ok"]
    summary = {"input_meta_sha256": meta_sha, "eligible_rows": len(rows), "picked": len(picked), "checked": len(ok),
               "prefix_match": sum(1 for r in ok if r["prefix_match"]),
               "capture_len_values": sorted({r["capture_len"] for r in ok})[:10],
               "warc_truncated_values": sorted({r.get("warc_truncated", "") for r in ok}),
               "chunked": sum(1 for r in ok if "chunked" in r.get("http_transfer_encoding", "").lower()),
               "errors": [r["status"] for r in out if r.get("status") != "ok"]}
    a.out.with_suffix(".summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
