#!/usr/bin/env python3
"""CPR (SRC-0123) Step-1 smoke run on the seeded REPDF sample (OBS-0704). Not a benchmark.

    TMPDIR=<scratch> python3 cpr_step1_smoke.py /home/user/dfrc-korea/repdf <cpr-checkout> <out-prefix>

Runs CPR's interactive main.py (github.com/BeenyHail/CPR, commit 9ecd6853) on each corrupted file of
the engine_smoke.py sample (seed 20260927), answering option "1", the file path, and "no" to the
optional Step 2 (FontDB + local LLM), so no model is needed. Reads Step 1's text output and scores it
against the text PyMuPDF extracts from the undamaged original:
  - cer_rapidfuzz: Levenshtein distance / reference length (rapidfuzz), whitespace-normalised;
  - cer_jiwer: jiwer.cer on the same strings (cross-check of the two libraries);
  - word_recall: multiset share of reference words present in the output (order-free);
  - word_recall_glyphnames: word_recall after replacing common glyph names that CPR's Step 1 spells
    out when a font has no usable mapping ("space" -> " ", "comma" -> ",", ...). A crude heuristic:
    it would also rewrite those strings inside real words.
Null baseline: the same scores for PyMuPDF text of the corrupted file itself.
CPR is written for Windows: it joins paths with "\\" string literals, so on Linux it cannot find its own
CSV tables. The script copies CPR's Code/ directory to a temp dir and rewrites every double-quoted
string literal that starts with a backslash pair to start with "/" (28 path literals in 5 files; the
PDF-escape byte literals, which start with b'...', are untouched). CPR also shells out to fontTools'
`ttx`, so the interpreter's bin directory is put on PATH.
CPR is CC BY-NC 4.0: run here for non-commercial research only; nothing from it is copied into notes.
Outputs: <out-prefix>.csv and <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from collections import Counter
from pathlib import Path

import jiwer
import pymupdf
from rapidfuzz.distance import Levenshtein

sys.path.insert(0, str(Path(__file__).parent))
from engine_smoke import sample, sha  # noqa: E402

TIMEOUT = 180
GLYPH_NAMES = {"space": " ", "comma": ",", "period": ".", "hyphen": "-", "colon": ":", "semicolon": ";",
               "parenleft": "(", "parenright": ")", "quoteright": "\u2019", "quoteleft": "\u2018",
               "quotedbl": '"', "slash": "/", "percent": "%", "ampersand": "&"}


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def mupdf_text(p: Path) -> str:
    try:
        with pymupdf.open(p) as d:
            return norm(" ".join(pg.get_text() for pg in d))
    except Exception:  # noqa: BLE001
        return ""


def scores(hyp: str, ref: str) -> dict:
    if not ref:
        return {"cer_rapidfuzz": None, "cer_jiwer": None, "word_recall": None}
    rw, hw = Counter(ref.split()), Counter(hyp.split())
    return {"cer_rapidfuzz": round(Levenshtein.distance(hyp, ref) / len(ref), 4),
            "cer_jiwer": round(jiwer.cer(ref, hyp), 4) if hyp else 1.0,
            "word_recall": round(sum((rw & hw).values()) / sum(rw.values()), 4)}


def run_cpr(code: Path, pdf: Path) -> tuple[str, float, str]:
    t = time.time()
    try:
        # CPR shells out to fontTools' `ttx` CLI, so the interpreter's bin directory must be on PATH.
        env = dict(os.environ, PATH=f"{Path(sys.executable).parent}:{os.environ.get('PATH', '')}")
        cp = subprocess.run([sys.executable, "main.py"], cwd=code, input=f"1\n{pdf}\nno\n".encode(),
                            capture_output=True, timeout=TIMEOUT, env=env)
        log = (cp.stdout + cp.stderr).decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return "timeout", time.time() - t, ""
    # With the path patch, Step 1 writes Code/Export/<input name>/PDF_0_Step1_Content.txt.
    hits = [Path(dp) / f for dp, _, fs in os.walk(code) for f in fs
            if f == "PDF_0_Step1_Content.txt" and Path(dp).name == pdf.name]
    text = norm(hits[0].read_text("utf-8", "replace")) if hits else ""
    status = "text" if text else ("no-text" if "No text mapped" in log or "Step 1" in log else "error: " + log.strip().splitlines()[-1][:160] if log.strip() else "error")
    return status, time.time() - t, text


def main() -> None:
    root, cpr, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    rows = []
    with tempfile.TemporaryDirectory(prefix="cpr_") as td:
        code = Path(td) / "cpr" / "Code"
        shutil.copytree(cpr / "Code", code, ignore=shutil.ignore_patterns("__pycache__"))
        patched = 0
        for py in code.glob("*.py"):
            src = py.read_text("utf-8")
            new, n = re.subn(r'(?<![bBrR])"\\\\', '"/', src)
            patched += n
            py.write_text(new, "utf-8")
        for i, pk in enumerate(sample(root)):
            src = Path(td) / f"in{i:02d}.pdf"
            shutil.copyfile(pk["corrupted"], src)
            status, secs, text = run_cpr(code, src)
            ref = mupdf_text(pk["original"])
            null = mupdf_text(pk["corrupted"])
            row = {"class": pk["class"], "doc": pk["doc"], "cpr_status": status, "cpr_seconds": round(secs, 1),
                   "cpr_chars": len(text), "ref_chars": len(ref), "null_chars": len(null)}
            row.update({f"cpr_{k}": v for k, v in scores(text, ref).items()})
            gtext = text
            for name, ch in GLYPH_NAMES.items():
                gtext = gtext.replace(name, ch)
            row["cpr_word_recall_glyphnames"] = scores(norm(gtext), ref)["word_recall"]
            row.update({f"null_{k}": v for k, v in scores(null, ref).items()})
            rows.append(row)
            print(json.dumps(row), flush=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out.with_suffix(".csv"), "w", newline="") as f:
        w = csv.DictWriter(f, list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    by = {}
    for cls in dict.fromkeys(r["class"] for r in rows):
        rs = [r for r in rows if r["class"] == cls]
        by[cls] = {"cpr_text": sum(r["cpr_status"] == "text" for r in rs),
                   "cpr_word_recall": [r["cpr_word_recall"] for r in rs],
                   "cpr_word_recall_glyphnames": [r["cpr_word_recall_glyphnames"] for r in rs],
                   "null_word_recall": [r["null_word_recall"] for r in rs],
                   "cpr_cer": [r["cpr_cer_rapidfuzz"] for r in rs], "null_cer": [r["null_cer_rapidfuzz"] for r in rs]}
    git = subprocess.run(["git", "-C", str(cpr), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    summ = {"kind": "smoke test, not a benchmark", "cpr_commit": git, "path_literals_patched": patched, "step2_llm": "not run (answered no)",
            "tools": {"pymupdf": pymupdf.__version__, "jiwer": jiwer.__version__ if hasattr(jiwer, "__version__") else "4.0.0",
                      "python": sys.version.split()[0]},
            "cer_libraries_agree": all(r["cpr_cer_rapidfuzz"] == r["cpr_cer_jiwer"] for r in rows if r["cpr_cer_jiwer"] is not None and r["cpr_chars"]),
            "by_class": by,
            "inputs": [{"path": str(pk["corrupted"].relative_to(root)), "sha256": sha(pk["corrupted"])} for pk in sample(root)]}
    (out.parent / f"{out.name}.summary.json").write_text(json.dumps(summ, indent=1) + "\n")


if __name__ == "__main__":
    main()
