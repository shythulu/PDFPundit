#!/usr/bin/env python3
"""Text-layer coverage check on the seeded REPDF sample (OBS-0708). A feasibility check, not a benchmark.

    TESSERACT=<tesseract> python3 textlayer_coverage.py /home/user/dfrc-korea/repdf <out-prefix>

Why: metrics_probe.py (OBS-0706) scored OCR against PyMuPDF's page-1 text layer. On one text+img
original both OCR engines "read" twice as many characters as the reference holds. This script
checks, for page 1 of every sampled original (engine_smoke.py picks, seed 20260927), how many
non-whitespace characters three sources yield:
  - PyMuPDF text layer (page.get_text()),
  - PDFium text layer (pypdfium2 get_textpage().get_text_range()),
  - Tesseract OCR of a 200 dpi PDFium render (eng, --psm 3, OMP_THREAD_LIMIT=1),
and flags pages where OCR finds >= 1.25x the characters of the larger text layer
("text layer incomplete": some visible text is not in the text layer).
It also reports the share of the OCR's words (lower-cased, >= 4 letters) found in the text layer.
Outputs: <out-prefix>.csv, <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import pymupdf
import pypdfium2 as pdfium

sys.path.insert(0, str(Path(__file__).parent))
from engine_smoke import sample, sha  # noqa: E402

TESS = os.environ.get("TESSERACT", "tesseract")


def nonws(s: str) -> int:
    return len(re.sub(r"\s+", "", s))


def words(s: str) -> set[str]:
    return {w.lower() for w in re.findall(r"[A-Za-z]{4,}", s)}


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    originals = {}
    for pk in sample(root):
        originals.setdefault(pk["doc"], pk["original"])
    rows = []
    env = dict(os.environ, OMP_THREAD_LIMIT="1")
    with tempfile.TemporaryDirectory() as td:
        for doc, p in sorted(originals.items()):
            mu = pymupdf.open(p)[0].get_text()
            pd = pdfium.PdfDocument(p)
            pg = pd[0]
            tp = pg.get_textpage()
            fi = tp.get_text_range()
            img = pg.render(scale=200 / 72).to_pil().convert("L")
            png = Path(td) / "p.png"
            img.save(png)
            cp = subprocess.run([TESS, str(png), "-", "-l", "eng", "--psm", "3"], capture_output=True,
                                text=True, env=env, timeout=120)
            ocr = cp.stdout
            layer = max(nonws(mu), nonws(fi))
            ow = words(ocr)
            lw = words(mu) | words(fi)
            rows.append({"doc": doc, "kind": "text+img" if "text+img" in doc else "text",
                         "pymupdf_chars": nonws(mu), "pdfium_chars": nonws(fi), "ocr_chars": nonws(ocr),
                         "ocr_over_layer": round(nonws(ocr) / max(1, layer), 3),
                         "ocr_words_in_layer": round(len(ow & lw) / max(1, len(ow)), 3),
                         "flag_layer_incomplete": nonws(ocr) >= 1.25 * layer})
    with open(f"{out}.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    flagged = [r["doc"] for r in rows if r["flag_layer_incomplete"]]
    summary = {"kind": "feasibility check, not a benchmark", "seed": 20260927,
               "tools": {"pymupdf": pymupdf.VersionBind, "pypdfium2": str(pdfium.PYPDFIUM_INFO) if hasattr(pdfium, "PYPDFIUM_INFO") else "",
                         "pdfium": str(pdfium.PDFIUM_INFO) if hasattr(pdfium, "PDFIUM_INFO") else "",
                         "tesseract": subprocess.run([TESS, "--version"], capture_output=True, text=True).stdout.splitlines()[0]},
               "n_pages": len(rows), "by_kind": {k: sum(1 for r in rows if r["kind"] == k) for k in ("text", "text+img")},
               "flagged_layer_incomplete": flagged,
               "flagged_by_kind": {k: sum(1 for r in rows if r["flag_layer_incomplete"] and r["kind"] == k)
                                   for k in ("text", "text+img")},
               "inputs": [{"path": str(p.relative_to(root)), "sha256": sha(p)} for _, p in sorted(originals.items())]}
    Path(f"{out}.summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: v for k, v in summary.items() if k != "inputs"}, indent=1))


if __name__ == "__main__":
    main()
