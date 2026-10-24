#!/usr/bin/env python3
"""Stage 4 verifier (2026-10-05): re-run the `verification_evidence` command of each Adopt/Trial ledger entry whose
`verified_in_container` is `pass`, on real REPDF input, after the 2026-10-05 container restart.

    S=<scratch> <env below> $S/venv/bin/python tool_checks.py <out-prefix>

Writes <out-prefix>.json (one record per check: tool, label, cmd, rc, key line, expected, ok) and <out-prefix>.txt.
Engine binaries come from colon-separated env lists so the versions Stage 2 ran and the WP-3.1 pins run side by side:
QPDF, MUTOOLS, PDFCPUS, PDFTOTEXTS, RSPROBES, PDFJS_MJSS, PYS (Python interpreters: pikepdf/pypdfium2 versions),
GS, PS2PDF, PDFBOX_JAR, ARLINGTON, VERAPDF, PFPROBE, INFGEN, TESSERACT, CHROME, SOFFICE, PDFLATEX, RADAMSA, ZZUF, PEEPDF.
Expected values are the ones the ledger records (research/tooling/ledger.jsonl, verification_evidence).
"""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

R = Path("/home/user/dfrc-korea/repdf")   # REPDF @ e547d4d1b77ead7e8cccce8b02878a6d33aa427a
C2 = R / "corrupted/print/text/Next_Decade_Space_Exploration(print)_xref.pdf"
ORIG = R / "original/print/text/Next_Decade_Space_Exploration(print).pdf"
SAVEAS = R / "original/saveas/text/Next_Decade_Space_Exploration(saveas).pdf"
REPO = Path(__file__).resolve().parents[4]
CPR_PDF = REPO / "research/cache/pdf/73f1223104c84ac21b184107ce767150543d1343deec2ae8b197087509be5ca9.pdf"
ENV = os.environ
W = Path(tempfile.mkdtemp(prefix="toolchk_"))
OUT: list[dict] = []


def env_list(k: str) -> list[str]:
    return [x for x in ENV.get(k, "").split(":") if x]


def run(cmd, stdin=None, stdout_file=None, env=None, timeout=300, cwd=None):
    t = time.time()
    try:
        cp = subprocess.run([str(c) for c in cmd], stdin=stdin, stdout=stdout_file or subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=timeout, env={**ENV, **(env or {})}, cwd=cwd)
        so = "" if stdout_file else cp.stdout.decode("utf-8", "replace")
        return cp.returncode, so, cp.stderr.decode("utf-8", "replace"), round(time.time() - t, 2)
    except FileNotFoundError as e:
        return "missing", "", str(e), 0.0
    except subprocess.TimeoutExpired:
        return "timeout", "", "", float(timeout)


def pyrun(py: str, code: str, *args, timeout=300):
    return run([py, "-c", code, *args], timeout=timeout)


def pages(p: Path) -> int | str:
    import pikepdf
    try:
        with pikepdf.open(p) as d:
            return len(d.pages)
    except Exception as e:  # noqa: BLE001
        return f"err {type(e).__name__}"


def rec(tool, label, cmd, rc, key, expect, ok, secs=None):
    r = {"tool": tool, "label": label, "cmd": " ".join(str(c) for c in cmd) if isinstance(cmd, list) else cmd,
         "rc": rc, "key": key.strip()[:300], "expected": expect, "ok": bool(ok), "secs": secs}
    OUT.append(r)
    print(f"{tool} {'PASS' if ok else 'FAIL'} {label}: {r['key'][:160]}", flush=True)


def last(s: str) -> str:
    # skip the JVM's "Picked up JAVA_TOOL_OPTIONS" banner (container proxy settings), which is not tool output
    ls = [x for x in s.strip().splitlines() if x.strip() and not x.startswith("Picked up JAVA_TOOL_OPTIONS")]
    return ls[-1] if ls else ""


def size(p: Path) -> int:
    return p.stat().st_size if p.exists() else -1


def ver(cmd) -> str:
    rc, so, se, _ = run(cmd, timeout=60)
    return (so or se).strip().splitlines()[0] if (so or se).strip() else f"rc={rc}"


def main() -> None:
    out = Path(sys.argv[1])
    # ---- TOOL-350 qpdf
    q = ENV.get("QPDF", "qpdf")
    v = ver([q, "--version"])
    rc, so, se, s = run([q, C2, W / "q.pdf"])
    rec("TOOL-350", f"{v}: rewrite C2", [q, "<C2>", "q.pdf"], rc, last(se + so), "exit 3, 'succeeded with warnings'",
        rc == 3 and "succeeded with warnings" in se + so, s)
    rc, so, se, s = run([q, "--check", W / "q.pdf"])
    rec("TOOL-350", f"{v}: --check output", [q, "--check", "q.pdf"], rc, last(so + se), "exit 0", rc == 0, s)
    rc, so, se, s = run([q, "--suppress-recovery", C2, W / "qs.pdf"])
    rec("TOOL-350", f"{v}: --suppress-recovery C2", [q, "--suppress-recovery", "<C2>", "qs.pdf"], rc, last(se + so),
        "exit 2, 'xref not found'", rc == 2 and "xref not found" in se + so, s)
    # ---- TOOL-351 mutool
    for m in env_list("MUTOOLS"):
        v = ver([m, "-v"])
        rc, so, se, s = run([m, "clean", "-gggg", C2, W / "m4.pdf"])
        pg = pages(W / "m4.pdf")
        rec("TOOL-351", f"{v}: clean -gggg C2", [m, "clean", "-gggg", "<C2>", "m4.pdf"], rc,
            f"pages={pg}; {last(se)}", "exit 0, 'repairing PDF document', 6 pages",
            rc == 0 and pg == 6 and "repairing" in se, s)
        rc, so, se, s = run([m, "clean", "-gggg", "-s", C2, W / "m4s.pdf"])
        pg = pages(W / "m4s.pdf")
        rec("TOOL-351", f"{v}: clean -gggg -s C2", [m, "clean", "-gggg", "-s", "<C2>", "m4s.pdf"], rc,
            f"pages={pg}", "6 pages", rc == 0 and pg == 6, s)
        (W / "m.txt").unlink(missing_ok=True)
        rc, so, se, s = run([m, "draw", "-q", "-F", "txt", "-o", W / "m.txt", C2])
        n = size(W / "m.txt")
        rec("TOOL-351", f"{v}: draw -q -F txt C2", [m, "draw", "-q", "-F", "txt", "-o", "m.txt", "<C2>"], rc,
            f"{n} bytes", "13,856 bytes", rc == 0 and n == 13856, s)
    # ---- Python libraries, per interpreter (TOOL-352 PyMuPDF, TOOL-354 pypdfium2, TOOL-426 pikepdf)
    for py in env_list("PYS"):
        code = ("import sys,pymupdf;d=pymupdf.open(sys.argv[1]);"
                "print(pymupdf.__version__, len(d), sum(len(p.get_text()) for p in d))")
        rc, so, se, s = pyrun(py, code, C2)
        f = so.split()
        rec("TOOL-352", f"pymupdf {f[0] if f else '?'} ({py}): open C2", "pymupdf.open(<C2>)", rc,
            f"pages={f[1] if len(f) > 1 else '?'} chars={f[2] if len(f) > 2 else '?'}", "6 pages, 9,221 chars",
            rc == 0 and f[1:3] == ["6", "9221"], s)
        code = ("import sys,pypdfium2 as p;d=p.PdfDocument(sys.argv[1]);n=len(d);d.save(sys.argv[2]);"
                "e=p.PdfDocument(sys.argv[2]);print(p.version.PYPDFIUM_INFO, p.version.PDFIUM_INFO, n, len(e), sep='|')")
        rc, so, se, s = pyrun(py, code, C2, W / "pf.pdf")
        f = so.strip().split("|")
        rec("TOOL-354", f"pypdfium2 {f[0] if f else '?'} ({f[1] if len(f) > 1 else '?'}): open + save C2",
            "pypdfium2.PdfDocument(<C2>); save('pf.pdf')", rc,
            f"pages={f[2] if len(f) > 2 else '?'} saved_pages={f[3] if len(f) > 3 else '?'}", "6 pages; 6-page output",
            rc == 0 and f[2:4] == ["6", "6"], s)
    # ---- TOOL-353 pdf.js
    for mjs in env_list("PDFJS_MJSS"):
        rc, so, se, s = run(["node", REPO / "research/experiments/engine_smoke_pdfjs.mjs", C2], env={"PDFJS_MJS": mjs})
        vj = json.loads((Path(mjs).parents[2] / "package.json").read_text())["version"]
        ok = False
        try:
            j = json.loads(last(so)); ok = j.get("ok") and j.get("pages") == 6 and j.get("chars") == 7604
        except Exception:  # noqa: BLE001
            pass
        rec("TOOL-353", f"pdfjs-dist {vj}: engine_smoke_pdfjs.mjs C2", "node engine_smoke_pdfjs.mjs <C2>", rc, last(so),
            '{"ok":true,"pages":6,"chars":7604}', ok, s)
    # ---- TOOL-355 pdfcpu
    for i, pc in enumerate(env_list("PDFCPUS")):
        xe = {"XDG_CONFIG_HOME": str(W / f"xdg_pdfcpu{i}")}   # own config per version: 0.16.1 refuses 0.15.0's
        rc0, so0, _, _ = run([pc, "version"], env=xe, timeout=60)
        v = next((ln.split(":", 1)[1].strip() for ln in so0.splitlines() if ln.startswith("version:")), f"rc={rc0}")
        rc, so, se, s = run([pc, "validate", "-m", "relaxed", C2], env=xe)
        rec("TOOL-355", f"pdfcpu {v}: validate -m relaxed C2", [pc, "validate", "-m", "relaxed", "<C2>"], rc,
            last(so + se), "'validation ok'", rc == 0 and "validation ok" in so + se, s)
    # ---- TOOL-356 PDFBox
    jar = ENV.get("PDFBOX_JAR")
    if jar:
        rc, so, se, s = run(["java", "-jar", jar, "export:text", f"-i={C2}", f"-o={W / 'pb.txt'}"])
        n = size(W / "pb.txt")
        rec("TOOL-356", "pdfbox 3.0.8: export:text C2", "java -jar pdfbox-app-3.0.8.jar export:text -i=<C2> -o=pb.txt",
            rc, f"{n} bytes", "exit 0, 12,363 bytes", rc == 0 and n == 12363, s)
        rc, so, se, s = run(["java", "-jar", jar, "decode", C2, W / "pbd.pdf"])
        pg = pages(W / "pbd.pdf")
        rec("TOOL-356", "pdfbox 3.0.8: decode C2", "java -jar pdfbox-app-3.0.8.jar decode <C2> pbd.pdf", rc,
            f"pages={pg}", "exit 0, 6 pages", rc == 0 and pg == 6, s)
    # ---- TOOL-357 Poppler
    for pt in env_list("PDFTOTEXTS"):
        v = ver([pt, "-v"])
        rc, so, se, s = run([pt, C2, W / "pt.txt"])
        n = size(W / "pt.txt")
        rec("TOOL-357", f"{v}: pdftotext C2", [pt, "<C2>", "pt.txt"], rc, f"{n} bytes", "exit 0, 13,155 bytes",
            rc == 0 and n == 13155, s)
    # ---- TOOL-358 Ghostscript
    gs = ENV.get("GS")
    if gs:
        v = ver([gs, "--version"])
        rc, so, se, s = run([gs, "-q", "-dNOPAUSE", "-dBATCH", "-o", W / "gw.pdf", "-sDEVICE=pdfwrite", C2])
        pg = pages(W / "gw.pdf")
        rec("TOOL-358", f"gs {v}: pdfwrite C2", "gs -q -dNOPAUSE -dBATCH -o gw.pdf -sDEVICE=pdfwrite <C2>", rc,
            f"pages={pg}", "exit 0, 6 pages", rc == 0 and pg == 6, s)
        rc, so, se, s = run([gs, "-q", "-dNOPAUSE", "-dBATCH", "-o", W / "gt.txt", "-sDEVICE=txtwrite", C2])
        n = size(W / "gt.txt")
        rec("TOOL-358", f"gs {v}: txtwrite C2", "gs -q -dNOPAUSE -dBATCH -o gt.txt -sDEVICE=txtwrite <C2>", rc,
            f"{n} bytes", "16,415 bytes", rc == 0 and n == 16415, s)
        rc, so, se, s = run([gs, "-q", "-dNOPAUSE", "-dBATCH", "-dPDFSTOPONERROR", "-o", "/dev/null",
                             "-sDEVICE=nullpage", C2])
        k = next((x for x in (so + se).splitlines() if "Error:" in x), last(so + se))
        rec("TOOL-358", f"gs {v}: -dPDFSTOPONERROR C2", "gs -dPDFSTOPONERROR -o /dev/null -sDEVICE=nullpage <C2>", rc,
            k, "exit 1, 'Error: /syntaxerror in --runpdf--'", rc == 1 and "/syntaxerror" in so + se, s)
    # ---- TOOL-359 lopdf, TOOL-361 hayro (rsprobe)
    for rs in env_list("RSPROBES"):
        rc, so, se, s = run([rs, "lopdf", C2, W / "l.pdf"])
        rec("TOOL-359", f"lopdf 0.45.0 ({rs}): C2", f"{rs} lopdf <C2> l.pdf", rc, last(so), "'lopdf ok pages=6 objects=165'",
            "lopdf ok pages=6 objects=165" in so, s)
        rc, so, se, s = run([rs, "hayro", C2, W / "h.png"])
        rec("TOOL-361", f"hayro ({rs}): C2", f"{rs} hayro <C2> h.png", rc, last(so) + f"; png {size(W / 'h.png')} bytes",
            "'hayro ok pages=6'", "hayro ok pages=6" in so, s)
    # ---- TOOL-362 veraPDF / Arlington
    if ENV.get("ARLINGTON"):
        rc, so, se, s = run([ENV["ARLINGTON"], "--version"])
        vl = [x.strip() for x in (so + se).splitlines() if "1.30" in x and not x.startswith("Picked up")]
        rec("TOOL-362", "arlington-pdf-model-checker --version", "arlington-pdf-model-checker --version", rc,
            vl[0] if vl else last(so + se), "1.30.2", "1.30.2" in so + se, s)
    if ENV.get("VERAPDF"):
        rc, so, se, s = run([ENV["VERAPDF"], "--format", "text", ORIG])
        rec("TOOL-362", "verapdf (greenfield) on the print original", "verapdf --format text <ORIG>", rc, last(so),
            "'FAIL ... 1b'", "FAIL" in so and "1b" in so, s)
    # ---- TOOL-364 zlib runtime
    for py in env_list("PYS") + ["/usr/bin/python3"]:
        rc, so, se, s = pyrun(py, "import zlib; print(zlib.ZLIB_RUNTIME_VERSION)")
        rec("TOOL-364", f"zlib runtime ({py})", "python -c 'import zlib; print(zlib.ZLIB_RUNTIME_VERSION)'", rc,
            so.strip(), "1.3", so.strip() == "1.3", s)
    # ---- TOOL-366 preflate-rs via pfprobe: first 3 FlateDecode streams of the Save As original
    if ENV.get("PFPROBE"):
        import pikepdf
        zs = []
        with pikepdf.open(SAVEAS) as d:
            for o in d.objects:
                if isinstance(o, pikepdf.Stream) and o.get("/Filter") == pikepdf.Name("/FlateDecode"):
                    p = W / f"s{len(zs)}.zz"; p.write_bytes(o.read_raw_bytes()); zs.append(p)
                    if len(zs) == 3:
                        break
        rc, so, se, s = run([ENV["PFPROBE"], *zs])
        rows = [x.split("\t") for x in so.strip().splitlines()]
        ok = len(rows) == 3 and all(r[1] == "ok" and r[5] == "true" for r in rows)
        rec("TOOL-366", "pfprobe (preflate-rs 0.7.6) on 3 Flate streams of the Save As original",
            "pfprobe s0.zz s1.zz s2.zz", rc,
            "; ".join(f"{r[1]} corrections={r[4]} roundtrip={r[5]}" for r in rows),
            "ok + exact round trip for all 3 (Stage 2: corrections 178, 96, 290)", ok, s)
    # ---- TOOL-368 infgen
    if ENV.get("INFGEN"):
        rc, so, se, s = run([ENV["INFGEN"], "-h"])
        rec("TOOL-368", "infgen -h", "infgen -h", rc, (so + se).strip().splitlines()[0] if (so + se).strip() else "",
            "'infgen 3.6'", "infgen 3.6" in so + se, s)
    # ---- TOOL-372 rapidfuzz, TOOL-373 jiwer, TOOL-375 scikit-image (the rv3 pair)
    import jiwer
    import numpy as np
    import pymupdf
    import pypdfium2 as pdfium
    import rapidfuzz
    import skimage
    from rapidfuzz.distance import Levenshtein
    from skimage.metrics import structural_similarity
    hyp = pymupdf.open(C2)[0].get_text(); ref = pymupdf.open(ORIG)[0].get_text()
    # the ledger's 1,889 characters is the whitespace-normalised length (" ".join(text.split())); raw is 1,933
    hn, rn = " ".join(hyp.split()), " ".join(ref.split())
    cer = Levenshtein.distance(hn, rn) / max(1, len(rn))
    rec("TOOL-372", f"rapidfuzz {rapidfuzz.__version__}: CER page 1 C2 vs original (PyMuPDF text, whitespace-normalised)",
        "Levenshtein.distance(hyp, ref) / len(ref)", 0, f"CER {cer} ({len(rn)} chars normalised, {len(ref)} raw)",
        "CER 0.0 (1,889 chars)", cer == 0.0 and len(rn) == 1889)
    wer = jiwer.wer(ref, hyp)
    rec("TOOL-373", f"jiwer {jiwer.__version__ if hasattr(jiwer, '__version__') else '4.0.0'}: WER same pair",
        "jiwer.wer(ref, hyp)", 0, f"WER {wer}", "WER 0.0", wer == 0.0)
    a = np.frombuffer(pymupdf.open(ORIG)[0].get_pixmap(dpi=72, colorspace=pymupdf.csGRAY, alpha=False).samples,
                      dtype=np.uint8)
    pm = pymupdf.open(ORIG)[0].get_pixmap(dpi=72, colorspace=pymupdf.csGRAY, alpha=False)
    a = a.reshape(pm.height, pm.width)
    b = np.asarray(pdfium.PdfDocument(str(ORIG))[0].render(scale=1, grayscale=True).to_pil().convert("L"))
    h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
    ss = round(float(structural_similarity(a[:h, :w], b[:h, :w], data_range=255)), 4)
    rec("TOOL-375", f"scikit-image {skimage.__version__}: SSIM MuPDF vs PDFium, page 1 of the original, 72 dpi gray",
        "structural_similarity(mupdf, pdfium, data_range=255)", 0,
        f"SSIM {ss} (pypdfium2 {pdfium.version.PYPDFIUM_INFO})", "SSIM 0.9398", abs(ss - 0.9398) < 0.0005)
    # ---- TOOL-382 Tesseract
    if ENV.get("TESSERACT"):
        t = ENV["TESSERACT"]
        v = ver([t, "--version"])
        img = W / "p1.png"
        pdfium.PdfDocument(str(ORIG))[0].render(scale=200 / 72).to_pil().save(img)
        rc, so, se, s = run([t, img, "-", "-l", "eng", "--psm", "3"], env={"OMP_THREAD_LIMIT": "1"})
        c = Levenshtein.distance(" ".join(so.split()), " ".join(ref.split())) / max(1, len(" ".join(ref.split())))
        rec("TOOL-382", f"{v}: OCR page 1 of the print original at 200 dpi (PDFium)",
            "OMP_THREAD_LIMIT=1 tesseract <200 dpi PDFium render> - -l eng --psm 3", rc,
            f"{len(so)} chars, CER vs text layer {c:.4f}", "tesseract 5.5.3 runs", rc == 0 and "5.5.3" in v and len(so) > 100, s)
    # ---- producers (OBS-0801): TOOL-413..418, then TOOL-426 opens their outputs
    prod = {}
    (W / "lo_input.txt").write_text("PDFPundit verifier sample text.\n" * 40)
    if ENV.get("SOFFICE"):
        rc, so, se, s = run([ENV["SOFFICE"], "--headless", f"-env:UserInstallation=file://{W}/lo_profile",
                             "--convert-to", "pdf", W / "lo_input.txt", "--outdir", W], timeout=180)
        prod["lo_input.pdf"] = W / "lo_input.pdf"
        wr = Path("/usr/lib/libreoffice/program/libswlo.so").exists()   # Writer module (apt libreoffice-writer)
        rec("TOOL-413", f"soffice --headless --convert-to pdf ({ver([ENV['SOFFICE'], '--version'])})",
            "soffice --headless -env:UserInstallation=file://.../lo_profile --convert-to pdf lo_input.txt --outdir ...",
            rc, f"{size(W / 'lo_input.pdf')} bytes, pages={pages(W / 'lo_input.pdf')}; Writer module present: {wr}; "
            f"{last(se + so)}", "1-page PDF",
            pages(W / "lo_input.pdf") == 1, s)
    (W / "sample.ps").write_text("%!PS\n/Helvetica findfont 24 scalefont setfont\n72 720 moveto (PDFPundit verifier) show\nshowpage\n")
    ps = ENV.get("PS2PDF", "ps2pdf")
    rc, so, se, s = run([ps, W / "sample.ps", W / "sample_gs.pdf"])
    prod["sample_gs.pdf"] = W / "sample_gs.pdf"
    rec("TOOL-414", f"ps2pdf ({ps}, gs {ver(['gs', '--version'])})", "ps2pdf sample.ps sample_gs.pdf", rc,
        f"{size(W / 'sample_gs.pdf')} bytes, pages={pages(W / 'sample_gs.pdf')}", "1-page PDF",
        pages(W / "sample_gs.pdf") == 1, s)
    (W / "sample.html").write_text("<html><body><h1>PDFPundit verifier</h1><p>sample</p></body></html>")
    if ENV.get("CHROME"):
        # Chromium's singleton socket path must fit in 108 bytes; the scratch TMPDIR is too long (SIGTRAP, rc -5),
        # so it gets a relative TMPDIR inside the work directory and its own profile there.
        rc, so, se, s = run([ENV["CHROME"], "--headless", "--disable-gpu", "--no-sandbox", f"--user-data-dir={W / 'chrome_profile'}",
                             f"--print-to-pdf={W / 'sample_chromium.pdf'}", f"file://{W / 'sample.html'}"], timeout=120,
                            env={"TMPDIR": "."}, cwd=W)
        prod["sample_chromium.pdf"] = W / "sample_chromium.pdf"
        rec("TOOL-415", f"chromium --headless --print-to-pdf ({ver([ENV['CHROME'], '--version'])})",
            "chrome --headless --disable-gpu --no-sandbox --print-to-pdf=sample_chromium.pdf file://sample.html", rc,
            f"{size(W / 'sample_chromium.pdf')} bytes, pages={pages(W / 'sample_chromium.pdf')}", "1-page PDF",
            pages(W / "sample_chromium.pdf") == 1, s)
    (W / "sample.tex").write_text("\\documentclass{article}\\begin{document}PDFPundit verifier\\end{document}\n")
    pl = ENV.get("PDFLATEX") or shutil.which("pdflatex")
    if pl:
        rc, so, se, s = run([pl, "-interaction=nonstopmode", f"-output-directory={W}", W / "sample.tex"], timeout=300)
        prod["sample.pdf"] = W / "sample.pdf"
        rec("TOOL-416", f"pdflatex ({ver([pl, '--version'])})", "pdflatex -interaction=nonstopmode sample.tex", rc,
            f"{size(W / 'sample.pdf')} bytes, pages={pages(W / 'sample.pdf')}", "1-page PDF", pages(W / "sample.pdf") == 1, s)
    else:
        rec("TOOL-416", "pdflatex", "which pdflatex", "missing", "pdflatex not installed after the restart (apt only)",
            "1-page PDF", False)
    for py in env_list("PYS")[-1:]:   # the pinned venv carries reportlab, pycairo and pymupdf4llm
        code = ("import sys,reportlab;from reportlab.pdfgen import canvas;c=canvas.Canvas(sys.argv[1]);"
                "c.drawString(72,720,'PDFPundit verifier');c.save();print(reportlab.Version)")
        rc, so, se, s = pyrun(py, code, W / "sample_reportlab.pdf")
        prod["sample_reportlab.pdf"] = W / "sample_reportlab.pdf"
        rec("TOOL-417", f"reportlab {so.strip()}", "canvas.Canvas(...).drawString(...); c.save()", rc,
            f"{size(W / 'sample_reportlab.pdf')} bytes, pages={pages(W / 'sample_reportlab.pdf')}", "1-page PDF",
            pages(W / "sample_reportlab.pdf") == 1, s)
        code = ("import sys,cairo;s=cairo.PDFSurface(sys.argv[1],200,200);c=cairo.Context(s);c.move_to(10,100);"
                "c.show_text('PDFPundit verifier');s.finish();print(cairo.version)")
        rc, so, se, s = pyrun(py, code, W / "sample_cairo.pdf")
        prod["sample_cairo.pdf"] = W / "sample_cairo.pdf"
        rec("TOOL-418", f"pycairo {so.strip()}", "cairo.PDFSurface(...); ctx.show_text(...); surface.finish()", rc,
            f"{size(W / 'sample_cairo.pdf')} bytes, pages={pages(W / 'sample_cairo.pdf')}", "1-page PDF",
            pages(W / "sample_cairo.pdf") == 1, s)
    for py in env_list("PYS"):
        code = ("import sys,pikepdf;r=[]\nfor f in sys.argv[1:]:\n try:\n  r.append(str(len(pikepdf.open(f).pages)))\n"
                " except Exception as e: r.append(type(e).__name__)\nprint(pikepdf.__version__, pikepdf.__libqpdf_version__, ','.join(r))")
        made = {k: v for k, v in prod.items() if v.exists()}   # a producer that failed above is reported there
        rc, so, se, s = pyrun(py, code, *made.values())
        f = so.split()
        rec("TOOL-426", f"pikepdf {f[0] if f else '?'} (libqpdf {f[1] if len(f) > 1 else '?'}): open producer outputs",
            "pikepdf.open(f) for " + ", ".join(made), rc,
            f"pages per file: {f[2] if len(f) > 2 else se[-200:]} ({len(made)} of {len(prod)} producers made a file)",
            "1 page each producer output", rc == 0 and len(f) > 2 and set(f[2].split(",")) == {"1"}, s)
    # ---- fuzzers / analysers on the print original (TOOL-421, 422, 423)
    orig = ORIG.read_bytes()
    def diffbytes(p: Path) -> str:
        m = p.read_bytes()
        return f"orig {len(orig)} mutated {len(m)} differing {sum(1 for x, y in zip(orig, m) if x != y)}"
    if ENV.get("RADAMSA"):
        with open(W / "mutated_radamsa.pdf", "wb") as fo:
            rc, so, se, s = run([ENV["RADAMSA"], "-s", "1", ORIG], stdout_file=fo)
        rec("TOOL-421", f"radamsa ({ver([ENV['RADAMSA'], '--version'])})", "radamsa -s 1 <ORIG> > mutated_radamsa.pdf", rc,
            diffbytes(W / "mutated_radamsa.pdf"), "mutated output differs", rc == 0 and W.joinpath("mutated_radamsa.pdf").read_bytes() != orig, s)
    zz = ENV.get("ZZUF") or shutil.which("zzuf")
    if zz:
        with open(ORIG, "rb") as fi, open(W / "mutated_zzuf.pdf", "wb") as fo:
            rc, so, se, s = run([zz, "-s", "0", "-r", "0.001"], stdin=fi, stdout_file=fo)
        m = (W / "mutated_zzuf.pdf").read_bytes()
        rec("TOOL-422", f"zzuf ({ver([zz, '--version'])})", "zzuf -s 0 -r 0.001 < <ORIG> > mutated_zzuf.pdf", rc,
            diffbytes(W / "mutated_zzuf.pdf"), "same size, some bytes differ", rc == 0 and len(m) == len(orig) and m != orig, s)
    if ENV.get("PEEPDF"):
        rc, so, se, s = run([ENV["PEEPDF"], "-l", ORIG], timeout=300)
        plain = re.sub(r"\x1b\[[0-9;]*m", "", so)   # peepdf colours its labels with ANSI escapes
        k = " | ".join(x.strip() for x in plain.splitlines()
                       if x.strip().startswith(("SHA256:", "PDF Format Version:", "Objects:", "Streams:", "Errors:")))[:250]
        rec("TOOL-423", "peepdf-3 5.4.1: peepdf -l <ORIG>", "peepdf -l <ORIG>", rc, k, "full structural report",
            rc == 0 and "Objects" in so, s)
    # ---- TOOL-323 pymupdf4llm on the cached CPR paper (SRC-0123)
    for py in env_list("PYS")[-1:]:
        code = ("import sys,time,pymupdf4llm;t=time.time();md=pymupdf4llm.to_markdown(sys.argv[1]);"
                "h=[l for l in md.splitlines() if l.startswith('#')];"
                "print(pymupdf4llm.__version__, round(time.time()-t,1), len(md), len(h), sum('References' in l for l in h), sep='|')")
        rc, so, se, s = pyrun(py, code, CPR_PDF, timeout=600)
        f = last(so).split("|")
        rec("TOOL-323", f"pymupdf4llm {f[0] if f else '?'}: CPR paper to Markdown", "pymupdf4llm.to_markdown(<CPR pdf>)", rc,
            f"{f[1] if len(f) > 1 else '?'} s, {f[2] if len(f) > 2 else '?'} chars, headings {f[3] if len(f) > 3 else '?'}, "
            f"'References' headings {f[4] if len(f) > 4 else '?'}", "Markdown with leveled headings and a References heading",
            rc == 0 and len(f) > 4 and int(f[3]) >= 5 and int(f[4]) >= 1, s)
    out.with_suffix(".json").write_text(json.dumps({"date": "2026-10-05", "C2": str(C2), "ORIG": str(ORIG),
                                                     "SAVEAS": str(SAVEAS), "checks": OUT}, indent=1))
    with open(out.with_suffix(".txt"), "w") as f:
        for r in OUT:
            f.write(f"{r['tool']}\t{'PASS' if r['ok'] else 'FAIL'}\t{r['label']}\trc={r['rc']}\t{r['key']}\n")
    shutil.rmtree(W, ignore_errors=True)


if __name__ == "__main__":
    main()
