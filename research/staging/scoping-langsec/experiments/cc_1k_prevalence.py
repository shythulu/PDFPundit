#!/usr/bin/env python3
"""Real-world damage/prevalence counts on the 1k sample (0000.zip) of the SafeDocs
CC-MAIN-2021-31-PDF-UNTRUNCATED corpus, from its published per-file metadata tables.

    python research/staging/scoping-langsec/experiments/cc_1k_prevalence.py \
        [--workdir /tmp/cc-safedocs] [--out research/staging/scoping-langsec/experiments/results/cc_1k_prevalence.json]

Downloads (once) the three *-1k.csv tables from the Digital Corpora S3 bucket, prints their sha256,
and counts: Common Crawl truncation flags and refetch status (provenance table); Poppler pdfinfo exit
codes and stderr message classes; Apache Tika parse status, exceptions, incremental-update counts,
signatures, encryption, damaged fonts. Rows are URL-based (1,045 rows for 1,000 unique files).
Only aggregate counts are written; no document content.
"""
from __future__ import annotations

import argparse, collections, csv, hashlib, json, re, urllib.request
from pathlib import Path

BASE = "https://digitalcorpora.s3.amazonaws.com/corpora/files/CC-MAIN-2021-31-PDF-UNTRUNCATED/metadata/"
TABLES = ["cc-provenance-20230324-1k.csv", "pdfinfo-20230324-1k.csv", "tika-20230714-1k.csv"]
UA = "PDFPundit-research/0.1 (https://github.com/shythulu/PDFPundit)"


def fetch(name: str, workdir: Path) -> Path:
    p = workdir / name
    if not p.exists():
        req = urllib.request.Request(BASE + name, headers={"User-Agent": UA})
        with urllib.request.urlopen(req, timeout=120) as r:
            p.write_bytes(r.read())
    return p


def rows(p: Path) -> list[dict]:
    with open(p, encoding="utf-8-sig", newline="") as f:
        return list(csv.DictReader(f))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--workdir", type=Path, default=Path("/tmp/cc-safedocs"))
    ap.add_argument("--out", type=Path, default=Path("research/staging/scoping-langsec/experiments/results/cc_1k_prevalence.json"))
    a = ap.parse_args()
    a.workdir.mkdir(parents=True, exist_ok=True)
    paths = {n: fetch(n, a.workdir) for n in TABLES}
    shas = {n: hashlib.sha256(p.read_bytes()).hexdigest() for n, p in paths.items()}
    prov, info, tika = (rows(paths[n]) for n in TABLES)

    res: dict = {"inputs": {n: {"url": BASE + n, "sha256": s} for n, s in shas.items()}}
    res["provenance"] = {
        "rows": len(prov), "unique_files": len({r["file_name"] for r in prov}),
        "cc_truncated": dict(collections.Counter(r["cc_truncated"] or "(not truncated)" for r in prov)),
        "fetched_status": dict(collections.Counter(r["fetched_status"] for r in prov)),
    }
    classes = collections.Counter()
    for r in info:
        for line in {x.strip() for x in re.split(r"\|x\||\n", r["stderr"]) if x.strip()}:
            classes[re.sub(r"<[0-9a-f]+>", "<X>", re.sub(r"\(\d+\)", "(N)", line))[:100]] += 1
    res["pdfinfo"] = {
        "rows": len(info),
        "exit_value": dict(collections.Counter(r["exit_value"] for r in info)),
        "timeout": dict(collections.Counter(r["timeout"] for r in info)),
        "rows_with_stderr": sum(1 for r in info if r["stderr"].strip()),
        "stderr_message_classes": dict(classes.most_common()),
    }
    iu = collections.Counter(r["pdf_incremental_updates"] for r in tika)
    res["tika"] = {
        "rows": len(tika),
        "parse_status": dict(collections.Counter(r["parse_status"] for r in tika)),
        "container_exception_first_line": dict(collections.Counter(
            r["container_exception"].splitlines()[0][:120] for r in tika if r["container_exception"])),
        "mime": dict(collections.Counter(r["mime"] for r in tika)),
        "encrypted": dict(collections.Counter(r["encrypted"] for r in tika)),
        "has_signature": dict(collections.Counter(r["has_signature"] for r in tika)),
        "pdf_contains_damaged_font": dict(collections.Counter(r["pdf_contains_damaged_font"] for r in tika)),
        "pdf_incremental_updates": dict(sorted(iu.items(), key=lambda kv: (len(kv[0]), kv[0]))),
        "rows_with_ge1_incremental_update": sum(v for k, v in iu.items() if k.isdigit() and int(k) >= 1),
        "rows_with_incremental_update_value": sum(v for k, v in iu.items() if k.isdigit()),
    }
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(json.dumps(res, indent=1, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in res.items() if k != "inputs"}, indent=1)[:4000])
    print("sha256:", shas)


if __name__ == "__main__":
    main()
