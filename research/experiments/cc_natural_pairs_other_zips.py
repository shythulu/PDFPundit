#!/usr/bin/env python3
"""Small-scale extension of research/experiments/cc_natural_pairs.py (OBS-0201) to ZIPs other than
zipfiles/0000-0999/0000.zip, and stratified by truncation type (length-cap vs disconnect).

OBS-0201 checked only 0000.zip, using the pre-cut "-1k" provenance CSV that Common Crawl / SafeDocs
publish specifically for that one zip. There is no "-1k" CSV for any other zip, so this script instead
range-fetches a bounded prefix of the FULL provenance table (cc-provenance-20230303.csv.gz, 1.29 GB
compressed) directly from digitalcorpora's S3 bucket, decompresses only that prefix, and filters rows
by the zip-number embedded in file_name (e.g. "0001NNN.pdf" -> zipfiles/0000-0999/0001.zip). Empirically
(2026-09-27) the first ~2 MB of compressed metadata already contains >1000 rows each for zips 0000-0011,
so no more than a few MB of metadata need to be fetched for a small sample.

For each sampled row it repeats OBS-0201's check: range-fetch the WARC capture, range-fetch the same
number of bytes (+1) from the refetched file inside the *correct* zip for that row, and record whether
the capture is an exact byte prefix. Only lengths, flags and hashes are written; no document bytes are
stored or committed.

    python3 cc_natural_pairs_other_zips.py \
        [--meta-bytes 2000000] [--zips 0001,0002] [--n-length-per-zip 15] \
        [--workdir <scratch>] [--out research/experiments/results/cc_natural_pairs_other_zips.csv]

Budget: this run fetched 2,000,000 bytes of compressed metadata (5,221,560 bytes decompressed) plus
per-row WARC/zip range reads; total transfer for the run is logged in the summary JSON's
`approx_bytes_transferred` field and stayed under the 200 MB test-run cap.
"""
from __future__ import annotations

import argparse, csv, hashlib, io, json, re, time, urllib.request, zipfile, zlib
from pathlib import Path

UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"
FULL_META = "https://digitalcorpora.s3.amazonaws.com/corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/metadata/cc-provenance-20230303.csv.gz"
ZIP_BASE = "https://digitalcorpora.s3.amazonaws.com/corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/zipfiles/0000-0999/{z}.zip"
CC = "https://data.commoncrawl.org/"

_bytes_transferred = 0


def get(url: str, rng: tuple[int, int] | None = None, tries: int = 4) -> bytes:
    global _bytes_transferred
    for i in range(tries):
        hdr = {"User-Agent": UA}
        if rng:
            hdr["Range"] = f"bytes={rng[0]}-{rng[1]}"
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=hdr), timeout=120) as r:
                data = r.read()
                _bytes_transferred += len(data)
                return data
        except Exception as e:
            if i == tries - 1:
                raise
            time.sleep(20 * (i + 1))
    raise RuntimeError("unreachable")


class HttpRangeFile(io.RawIOBase):
    """Minimal seekable read-only file over HTTP range requests, with a block cache. Same as
    research/experiments/cc_natural_pairs.py, duplicated here so this script stands alone."""
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


def fetch_meta_prefix(nbytes: int) -> tuple[list[dict], str]:
    """Range-fetch the first `nbytes` of the FULL provenance csv.gz and decompress. Returns
    (rows, sha256_of_compressed_prefix). The last, possibly-partial CSV line is dropped."""
    comp = get(FULL_META, (0, nbytes - 1))
    comp_sha = hashlib.sha256(comp).hexdigest()
    d = zlib.decompressobj(16 + zlib.MAX_WBITS)
    dec = d.decompress(comp)
    text = dec.decode("utf-8", errors="ignore")
    lines = text.split("\n")
    complete = lines[:-1]  # drop last partial line
    rows = list(csv.DictReader(complete))
    return rows, comp_sha


def zip_of(file_name: str) -> str | None:
    m = re.match(r"^(\d{4})\d{3}\.pdf$", file_name)
    return m.group(1) if m else None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--meta-bytes", type=int, default=2_000_000, help="bytes of compressed metadata to range-fetch")
    ap.add_argument("--zips", default="0001,0002", help="comma-separated 4-digit zip numbers, all != 0000")
    ap.add_argument("--n-length-per-zip", type=int, default=13, help="how many length-truncated pairs per zip")
    ap.add_argument("--n-disconnect-per-zip", type=int, default=2, help="how many disconnect-truncated pairs per zip (capped by availability)")
    ap.add_argument("--workdir", type=Path, default=Path("/tmp/cc-safedocs-otherzips"))
    ap.add_argument("--out", type=Path, default=Path("research/experiments/results/cc_natural_pairs_other_zips.csv"))
    a = ap.parse_args()
    a.workdir.mkdir(parents=True, exist_ok=True)
    target_zips = a.zips.split(",")
    assert "0000" not in target_zips, "task requires zips other than 0000 (the one OBS-0201 used)"

    rows, comp_sha = fetch_meta_prefix(a.meta_bytes)
    (a.workdir / "meta_prefix_sha256.txt").write_text(comp_sha + "\n")

    by_zip_type: dict[tuple[str, str], list[dict]] = {}
    for r in rows:
        z = zip_of(r.get("file_name", ""))
        if z in target_zips and r.get("cc_truncated") and r.get("fetched_status") == "REFETCHED_SUCCESS":
            by_zip_type.setdefault((z, r["cc_truncated"]), []).append(r)

    picked: list[dict] = []
    coverage = {}
    for z in target_zips:
        length_rows = sorted(by_zip_type.get((z, "length"), []), key=lambda r: r["file_name"])[:a.n_length_per_zip]
        disc_rows = sorted(by_zip_type.get((z, "disconnect"), []), key=lambda r: r["file_name"])[:a.n_disconnect_per_zip]
        coverage[z] = {"length_available": len(by_zip_type.get((z, "length"), [])),
                        "length_picked": len(length_rows),
                        "disconnect_available": len(by_zip_type.get((z, "disconnect"), [])),
                        "disconnect_picked": len(disc_rows)}
        picked.extend(length_rows)
        picked.extend(disc_rows)

    zf_cache: dict[str, zipfile.ZipFile] = {}
    def zf_for(z: str) -> zipfile.ZipFile:
        if z not in zf_cache:
            zf_cache[z] = zipfile.ZipFile(HttpRangeFile(ZIP_BASE.format(z=z)))
        return zf_cache[z]

    out = []
    for r in picked:
        z = zip_of(r["file_name"])
        rec = {"zip": z, "url_id": r["url_id"], "file_name": r["file_name"], "cc_truncated": r["cc_truncated"],
               "fetched_length": r["fetched_length"]}
        try:
            w = parse_warc(get(CC + r["cc_warc_file_name"], (int(r["cc_warc_start"]), int(r["cc_warc_end"]))))
            time.sleep(1.5)
            cap = w["payload"]
            rec.update({k: w[k] for k in ("warc_truncated", "http_transfer_encoding", "http_content_encoding", "http_content_length")})
            rec["capture_len"] = len(cap)
            rec["capture_starts_pdf"] = cap[:5] == b"%PDF-"
            zf = zf_for(z)
            names = set(zf.namelist())
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
    summary = {
        "meta_prefix_bytes_compressed": a.meta_bytes,
        "meta_prefix_sha256": comp_sha,
        "target_zips": target_zips,
        "coverage_by_zip": coverage,
        "picked": len(picked), "checked": len(ok),
        "prefix_match": sum(1 for r in ok if r["prefix_match"]),
        "prefix_mismatch_rows": [r["file_name"] for r in ok if not r["prefix_match"]],
        "by_truncation_type": {t: sum(1 for r in ok if r["cc_truncated"] == t) for t in ("length", "disconnect")},
        "errors": [r["status"] for r in out if r.get("status") != "ok"],
        "approx_bytes_transferred": _bytes_transferred,
    }
    a.out.with_suffix(".summary.json").write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
