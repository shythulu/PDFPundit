#!/usr/bin/env python3
"""OBS-0601: compare pdfplumber and pymupdf4llm against the pdftotext baseline on a cached
academic paper (SRC-0123, CPR), checking whether sections, the reference list and the
limitations text extract intact from a two-column layout.

    python3 -m venv /tmp/ppc-venv && . /tmp/ppc-venv/bin/activate
    pip install pdfplumber pymupdf4llm
    python3 research/experiments/paper_parsing_comparison.py \
        research/cache/pdf/73f1223104c84ac21b184107ce767150543d1343deec2ae8b197087509be5ca9.pdf \
        research/experiments/results/paper_parsing_comparison

Writes <outdir>/pymupdf4llm.md and <outdir>/pdfplumber.txt, and prints a short report: whether
a "References" heading/line and the paper's own "limitations" sentence survive intact, and
whether any line shows an out-of-order two-column merge (a "known-continuation" phrase from one
part of the paper directly adjacent to unrelated text from another part).
"""
from __future__ import annotations

import sys
import time
from pathlib import Path

MARKER_SENTENCE = "resulting CER of 0.134 reflects unrecovered segments"
GARBLE_MARKER = "reflects unrecovered segments references and reconstructing"


def run_pymupdf4llm(pdf: Path, out: Path) -> tuple[float, str]:
    import pymupdf4llm
    t0 = time.time()
    md = pymupdf4llm.to_markdown(str(pdf))
    out.write_text(md)
    return time.time() - t0, md


def run_pdfplumber(pdf: Path, out: Path) -> tuple[float, str]:
    import pdfplumber
    t0 = time.time()
    pages = []
    with pdfplumber.open(str(pdf)) as doc:
        for p in doc.pages:
            pages.append(p.extract_text() or "")
    text = "\n\x0c\n".join(pages)
    out.write_text(text)
    return time.time() - t0, text


def main() -> None:
    pdf = Path(sys.argv[1])
    outdir = Path(sys.argv[2])
    outdir.mkdir(parents=True, exist_ok=True)

    t_md, md = run_pymupdf4llm(pdf, outdir / "pymupdf4llm.md")
    t_txt, txt = run_pdfplumber(pdf, outdir / "pdfplumber.txt")

    def check(label: str, text: str) -> None:
        has_ref_heading = "### **References**" in text or "\nReferences" in text
        has_limitations = "CPR has limitations" in text
        clean = MARKER_SENTENCE in text and GARBLE_MARKER not in text
        print(f"{label}: elapsed={t_md if label.startswith('pymu') else t_txt:.1f}s "
              f"references_found={has_ref_heading} limitations_sentence_intact={has_limitations} "
              f"two_column_merge_clean={clean}")

    check("pymupdf4llm", md)
    check("pdfplumber", txt)


if __name__ == "__main__":
    main()
