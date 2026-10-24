#!/usr/bin/env python3
"""WP-3.5 / WP-3.2 challenge: how good is the Tesseract substitute for Google Document AI on REPDF's non-Latin pages, and
does a 2026 CPU OCR (RapidOCR 3.9.2 with its bundled PP-OCRv6 small multilingual models) do better?

Ground truth: the text layer of REPDF *Save As* originals (OBS-0708 found Save As text layers complete on page 1).
Each page is labelled by its dominant script (Han, Arabic, Devanagari, else Latin). Pages are rendered at 200 dpi
(PyMuPDF, grey). Metric: bag-of-characters recall and precision against the NFKC-normalised, whitespace-free text layer
(order-free, so Arabic visual/logical order and line breaks do not matter).

Engines:
  tess_eng       Tesseract (tesserocr wheel) -l eng, the configuration TOOL-382 was verified with
  tess_script    Tesseract with the page's script language (chi_sim / ara / hin / eng+fra+spa), an oracle hint
  tess_all       Tesseract -l eng+fra+spa+ara+hin+chi_sim (no hint)
  rapidocr       RapidOCR default pipeline (PP-OCRv6 det/rec small, bundled in the wheel; no language hint)
  rapid_script   RapidOCR with the page's script recogniser (PP-OCRv5 mobile arabic / devanagari / latin .onnx
                 fetched from modelscope.cn into --pp-models, SHA256 as in rapidocr's default_models.yaml; Han pages
                 keep the default PP-OCRv6), an oracle hint

One document per run (each run stays under 10 minutes), then merge:
    OMP_THREAD_LIMIT=1 <venv>/bin/python ocr_script_recall.py <repdf_root> <tessdata_dir> <out_prefix> --doc-index K --pp-models <dir>
    <venv>/bin/python ocr_script_recall.py --summarize <out_prefix> <prefix_K.csv>...
Documents are the sorted original/saveas/text/*.pdf; K indexes that list.
"""
import csv
import json
import sys
import time
import unicodedata
from collections import Counter
from pathlib import Path

import numpy as np
import pymupdf
import tesserocr
from PIL import Image
from rapidocr import RapidOCR

SCRIPT_LANG = {"HAN": "chi_sim", "ARABIC": "ara", "DEVANAGARI": "hin", "LATIN": "eng+fra+spa"}


def norm_chars(text: str) -> Counter:
    t = unicodedata.normalize("NFKC", text)
    return Counter(ch for ch in t if not ch.isspace())


def script_of(c: Counter) -> str:
    s = Counter()
    for ch, n in c.items():
        if ch.isalpha():
            name = unicodedata.name(ch, "")
            for k in ("CJK", "ARABIC", "DEVANAGARI", "LATIN"):
                if name.startswith(k):
                    s["HAN" if k == "CJK" else k] += n
    return s.most_common(1)[0][0] if s else "NONE"


def score(truth: Counter, got: Counter):
    inter = sum((truth & got).values())
    return inter / max(1, sum(truth.values())), inter / max(1, sum(got.values()))


def main(root: Path, tessdata: Path, prefix: Path, k: int, pp: Path) -> None:
    from rapidocr.utils.typings import LangRec, ModelType, OCRVersion
    docs = sorted((root / "original" / "saveas" / "text").glob("*.pdf"))[k:k + 1]
    apis = {
        "tess_eng": tesserocr.PyTessBaseAPI(path=str(tessdata), lang="eng"),
        "tess_all": tesserocr.PyTessBaseAPI(path=str(tessdata), lang="eng+fra+spa+ara+hin+chi_sim"),
    }
    script_apis = {k: tesserocr.PyTessBaseAPI(path=str(tessdata), lang=v) for k, v in SCRIPT_LANG.items()}
    rapid = RapidOCR(params={"Global.log_level": "error", "EngineConfig.onnxruntime.intra_op_num_threads": 2})
    rapid_script = {"HAN": rapid}
    for scr, lang in (("ARABIC", LangRec.ARABIC), ("DEVANAGARI", LangRec.DEVANAGARI), ("LATIN", LangRec.LATIN)):
        mp = pp / f"{lang.value}_PP-OCRv5_rec_mobile.onnx"
        rapid_script[scr] = RapidOCR(params={"Global.log_level": "error", "EngineConfig.onnxruntime.intra_op_num_threads": 2, "Rec.lang_type": lang,
                                             "Rec.ocr_version": OCRVersion.PPOCRV5, "Rec.model_type": ModelType.MOBILE,
                                             "Rec.model_path": str(mp)})
    rows = []
    for f in docs:
        doc = pymupdf.open(f)
        for pno, page in enumerate(doc):
            truth = norm_chars(page.get_text())
            if sum(truth.values()) < 50:
                continue
            scr = script_of(truth)
            pix = page.get_pixmap(dpi=200, colorspace=pymupdf.csGRAY)
            img = Image.frombytes("L", (pix.width, pix.height), pix.samples)
            outs = {}
            for name, api in list(apis.items()) + [("tess_script", script_apis.get(scr, apis["tess_eng"]))]:
                t = time.perf_counter()
                api.SetImage(img)
                outs[name] = (api.GetUTF8Text(), time.perf_counter() - t)
            t = time.perf_counter()
            res = rapid(np.array(img.convert("RGB")))
            txt = "".join(res.txts) if res.txts else ""
            outs["rapidocr"] = (txt, time.perf_counter() - t)
            t = time.perf_counter()
            res = rapid_script.get(scr, rapid)(np.array(img.convert("RGB")))
            txt = "".join(res.txts) if res.txts else ""
            outs["rapid_script"] = (txt, time.perf_counter() - t)
            for eng, (txt, secs) in outs.items():
                r, p = score(truth, norm_chars(txt))
                rows.append({"doc": f.name, "page": pno + 1, "script": scr, "layer_chars": sum(truth.values()),
                             "engine": eng, "char_recall": round(r, 4), "char_precision": round(p, 4),
                             "seconds": round(secs, 2)})
            print(f.name[:40], pno + 1, scr, {e: next(x["char_recall"] for x in rows[-5:] if x["engine"] == e)
                                              for e in outs}, flush=True)
    with open(f"{prefix}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)


def summarize(prefix: Path, parts) -> None:
    rows = []
    for p in parts:
        with open(p) as fh:
            for r in csv.DictReader(fh):
                for k in ("char_recall", "char_precision", "seconds"):
                    r[k] = float(r[k])
                rows.append(r)
    with open(f"{prefix}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    summ = {"documents": sorted({r["doc"] for r in rows})}
    for scr in sorted({r["script"] for r in rows}):  # noqa: B007
        for eng in ("tess_eng", "tess_script", "tess_all", "rapidocr", "rapid_script"):
            g = [r for r in rows if r["script"] == scr and r["engine"] == eng]
            summ.setdefault(scr, {"pages": len(g)})[eng] = {
                "recall_mean": round(sum(r["char_recall"] for r in g) / len(g), 4),
                "recall_min": min(r["char_recall"] for r in g),
                "precision_mean": round(sum(r["char_precision"] for r in g) / len(g), 4),
                "seconds_mean": round(sum(r["seconds"] for r in g) / len(g), 2),
            }
    import rapidocr
    summ["versions"] = {"rapidocr": __import__("importlib.metadata").metadata.version("rapidocr"), "onnxruntime": __import__("importlib.metadata").metadata.version("onnxruntime"), "tesseract": tesserocr.tesseract_version().splitlines()[0], "tesserocr": tesserocr.__version__,
                        "pymupdf": pymupdf.__version__}
    Path(f"{prefix}.summary.json").write_text(json.dumps(summ, indent=1) + "\n")
    print(json.dumps(summ, indent=1))


if __name__ == "__main__":
    if sys.argv[1] == "--summarize":
        summarize(Path(sys.argv[2]), sys.argv[3:])
    else:
        main(Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[sys.argv.index("--doc-index") + 1]),
             Path(sys.argv[sys.argv.index("--pp-models") + 1]))
