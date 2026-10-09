#!/usr/bin/env python3
"""Metric and OCR tooling probe on the seeded REPDF sample (OBS-0706). A feasibility check, not a benchmark.

    TMPDIR=<scratch> TORCH_HOME=<scratch>/torch DOCTR_CACHE_DIR=<scratch>/doctr QPDF=<qpdf> TESSERACT=<tesseract> \
        python3 metrics_probe.py /home/user/dfrc-korea/repdf <out-prefix>

Sample: engine_smoke.py's seeded picks (seed 20260927).
A. Visual metrics, page 1 at 72 dpi, rendered by two engines (PyMuPDF/MuPDF and pypdfium2/PDFium):
   - renderer floor: SSIM (scikit-image, grayscale) and share of pixels differing by > 32 levels between
     the two renderers on the same undamaged original;
   - damage signal: the same scores between original and corrupted file, per renderer, for the picks
     of the content classes C6-C9 that both renderers open;
   - LPIPS (lpips 0.1.4, AlexNet, CPU, 2 threads) on the same pairs, with wall time per pair.
B. OCR on page 1 of the first 4 sampled originals rendered at 200 dpi by PDFium: Tesseract (CLI, eng,
   tessdata_fast model from conda-forge) and docTR (ocr_predictor(pretrained=True): fast_base +
   crnn_vgg16_bn), Tesseract run with OMP_THREAD_LIMIT=1, each scored against PyMuPDF's page-1 text layer: CER (rapidfuzz Levenshtein / reference
   length), grapheme-aware CER (dinglehopper), WER (jiwer); seconds per page.
C. Structural diff: `qpdf --json=2` object maps of the original and of the qpdf-rewritten corrupted
   file for the C2 and C7 picks, compared with DeepDiff (stream data excluded); counts and time.
Outputs: <out-prefix>.visual.csv, <out-prefix>.ocr.csv, <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import jiwer
import lpips
import numpy as np
import pymupdf
import pypdfium2 as pdfium
import torch
from deepdiff import DeepDiff
from dinglehopper.character_error_rate import character_error_rate
from PIL import Image
from rapidfuzz.distance import Levenshtein
from skimage.metrics import structural_similarity

sys.path.insert(0, str(Path(__file__).parent))
from engine_smoke import sample, sha  # noqa: E402

torch.set_num_threads(2)
QPDF, TESS = os.environ.get("QPDF", "qpdf"), os.environ.get("TESSERACT", "tesseract")


def render(engine: str, path: Path, dpi: int, gray: bool) -> np.ndarray | None:
    try:
        if engine == "mupdf":
            with pymupdf.open(path) as d:
                if d.page_count < 1:
                    return None
                pix = d[0].get_pixmap(dpi=dpi, colorspace=pymupdf.csGRAY if gray else pymupdf.csRGB, alpha=False)
                a = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width, pix.n)
        else:
            d = pdfium.PdfDocument(str(path))
            if len(d) < 1:
                return None
            img = d[0].render(scale=dpi / 72, grayscale=gray).to_pil()
            a = np.asarray(img.convert("L" if gray else "RGB"))
            a = a[..., None] if a.ndim == 2 else a
            d.close()
        return a
    except Exception:  # noqa: BLE001
        return None


def crop(a: np.ndarray, b: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
    return a[:h, :w], b[:h, :w]


def vis(a: np.ndarray, b: np.ndarray) -> dict:
    a, b = crop(a[..., 0], b[..., 0])
    return {"ssim": round(float(structural_similarity(a, b, data_range=255)), 4),
            "pix_diff": round(float((np.abs(a.astype(int) - b.astype(int)) > 32).mean()), 5)}


LP = lpips.LPIPS(net="alex", verbose=False)


def lp(a: np.ndarray, b: np.ndarray) -> tuple[float, float]:
    a, b = crop(a, b)
    t = lambda x: torch.from_numpy(x.astype(np.float32) / 127.5 - 1).permute(2, 0, 1)[None]  # noqa: E731
    t0 = time.time()
    with torch.no_grad():
        v = float(LP(t(a), t(b)))
    return round(v, 4), round(time.time() - t0, 2)


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip()


def ocr_scores(hyp: str, ref: str) -> dict:
    return {"cer": round(Levenshtein.distance(hyp, ref) / max(1, len(ref)), 4),
            "cer_dinglehopper": round(character_error_rate(ref, hyp), 4),
            "wer": round(jiwer.wer(ref, hyp), 4) if hyp else 1.0}


def qjson(p: Path) -> dict:
    cp = subprocess.run([QPDF, "--json=2", "--json-stream-data=none", str(p)], capture_output=True, timeout=120)
    return json.loads(cp.stdout)["qpdf"][1]


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    picks = sample(root)
    originals = {}
    for pk in picks:
        originals.setdefault(pk["doc"], pk["original"])
    vrows = []
    for doc, p in sorted(originals.items()):
        m, f = render("mupdf", p, 72, True), render("pdfium", p, 72, True)
        mc, fc = render("mupdf", p, 72, False), render("pdfium", p, 72, False)
        lv, lt = lp(mc, fc)
        vrows.append({"pair": "renderer-floor", "doc": doc, "class": "original", "renderer": "mupdf-vs-pdfium",
                      **vis(m, f), "lpips": lv, "lpips_s": lt})
    for pk in picks:
        if pk["class"] not in ("C6", "C7", "C8", "C9"):
            continue
        for eng in ("mupdf", "pdfium"):
            a, b = render(eng, pk["original"], 72, True), render(eng, pk["corrupted"], 72, True)
            if a is None or b is None:
                vrows.append({"pair": "damage", "doc": pk["doc"], "class": pk["class"], "renderer": eng, "ssim": None})
                continue
            ac, bc = render(eng, pk["original"], 72, False), render(eng, pk["corrupted"], 72, False)
            lv, lt = lp(ac, bc)
            vrows.append({"pair": "damage", "doc": pk["doc"], "class": pk["class"], "renderer": eng,
                          **vis(a, b), "lpips": lv, "lpips_s": lt})
    orows = []
    from doctr.io import DocumentFile  # noqa: F401  (import check)
    from doctr.models import ocr_predictor
    t0 = time.time()
    doctr_model = ocr_predictor(pretrained=True)
    doctr_load_s = round(time.time() - t0, 1)
    with tempfile.TemporaryDirectory(prefix="ocr_") as td:
        for doc, p in sorted(originals.items())[:4]:
            with pymupdf.open(p) as d:
                ref = norm(d[0].get_text())
            img = render("pdfium", p, 200, False)
            png = Path(td) / "p.png"
            Image.fromarray(img).save(png)
            t = time.time()
            # OMP_THREAD_LIMIT=1: without it the OpenMP build hung past 300 s next to torch on this 4-CPU box.
            cp = subprocess.run([TESS, str(png), "stdout", "-l", "eng", "--psm", "3"], capture_output=True, timeout=300,
                                env=dict(os.environ, OMP_THREAD_LIMIT="1"))
            tess_s, tess = round(time.time() - t, 1), norm(cp.stdout.decode("utf-8", "replace"))
            t = time.time()
            res = doctr_model([img])
            doctr_s, dtxt = round(time.time() - t, 1), norm(res.render())
            for eng, txt, secs in (("tesseract", tess, tess_s), ("doctr", dtxt, doctr_s)):
                orows.append({"doc": doc, "engine": eng, "seconds": secs, "ref_chars": len(ref), "hyp_chars": len(txt),
                              **ocr_scores(txt, ref)})
        srows = []
        for pk in picks:
            if pk["class"] not in ("C2", "C7"):
                continue
            q = Path(td) / "q.pdf"
            subprocess.run([QPDF, str(pk["corrupted"]), str(q)], capture_output=True, timeout=120)
            t = time.time()
            dd = DeepDiff(qjson(pk["original"]), qjson(q), ignore_order=True, view="tree")
            srows.append({"doc": pk["doc"], "class": pk["class"], "seconds": round(time.time() - t, 2),
                          **{k: len(v) for k, v in dd.items()}})
    out.parent.mkdir(parents=True, exist_ok=True)
    for name, rows in (("visual", vrows), ("ocr", orows)):
        keys = list(dict.fromkeys(k for r in rows for k in r))
        with open(out.parent / f"{out.name}.{name}.csv", "w", newline="") as f:
            w = csv.DictWriter(f, keys)
            w.writeheader()
            w.writerows(rows)
    fl = [r for r in vrows if r["pair"] == "renderer-floor"]
    dm = [r for r in vrows if r["pair"] == "damage" and r.get("ssim") is not None]
    tess_v = subprocess.run([TESS, "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    import doctr
    summ = {"kind": "feasibility probe, not a benchmark",
            "tools": {"scikit-image": __import__("skimage").__version__, "lpips": "0.1.4 (alex)", "torch": torch.__version__,
                      "pymupdf": pymupdf.__version__, "pypdfium2": str(pdfium.PYPDFIUM_INFO), "pdfium": str(pdfium.PDFIUM_INFO),
                      "tesseract": tess_v, "doctr": doctr.__version__, "dinglehopper": "0.11.0",
                      "jiwer": "4.0.0", "deepdiff": __import__("deepdiff").__version__,
                      "qpdf": subprocess.run([QPDF, "--version"], capture_output=True, text=True).stdout.splitlines()[0]},
            "renderer_floor": {"n": len(fl), "ssim_min": min(r["ssim"] for r in fl), "ssim_median": sorted(r["ssim"] for r in fl)[len(fl) // 2],
                               "pix_diff_max": max(r["pix_diff"] for r in fl), "lpips_max": max(r["lpips"] for r in fl)},
            "damage": [{k: r[k] for k in ("class", "renderer", "ssim", "pix_diff", "lpips")} for r in dm],
            "lpips_seconds_per_pair_72dpi": sorted(r["lpips_s"] for r in vrows if r.get("lpips_s"))[len(vrows) // 2],
            "doctr_model_load_s": doctr_load_s, "ocr": orows, "structural_diff": srows,
            "inputs": [{"path": str(p.relative_to(root)), "sha256": sha(p)} for p in sorted(originals.values())]}
    (out.parent / f"{out.name}.summary.json").write_text(json.dumps(summ, indent=1, default=str) + "\n")
    print(json.dumps({k: v for k, v in summ.items() if k != "inputs"}, indent=1, default=str)[:5000])


if __name__ == "__main__":
    main()
