#!/usr/bin/env python3
"""Smoke test (NOT a benchmark) of current PDF engine releases on a small seeded REPDF sample (OBS-0700, OBS-0701).

    python3 engine_smoke.py /home/user/dfrc-korea/repdf <out-prefix> [--recheck <qpdf-regression-file>]

Set TMPDIR to keep the per-run temp files (copies and outputs, deleted at the end) off /tmp.

Sample: random.seed(20260927); for each REPDF damage class C1..C10 (in that order) pick 2 corrupted
files with random.sample() from the sorted list of that class, plus each pick's original: 20 corrupted
files and their (at most 20) originals. Every engine runs once per file in a temp dir under a simple
file name, with a per-run timeout. For every run we record the exit code, whether the engine produced
output (a PDF, a page count, text or a rendered page), the page count and non-whitespace text
characters where the engine reports them, and, for damaged files, the ratio of text characters to the
same engine's text from the original. This says only whether each engine runs here and produces
*something*; it says nothing about the correctness of the output.

Engine locations come from environment variables (defaults: the names on PATH):
  QPDF, QPDF_APT (for --recheck), MUTOOL, GS, PDFTOTEXT, PDFCPU, PDFBOX_JAR, NODE, PDFJS_MJS,
  RSPROBE (the Rust probe in rsprobe/, built with `cargo build --release`).
The Python libraries (pikepdf, pymupdf, pypdfium2) are imported by this interpreter in a child process
(`engine_smoke.py --probe <lib> ...`), so a crash in native code only fails that run.

--recheck FILE: run `qpdf --check FILE` with QPDF_APT and QPDF and record exit codes and the last
output lines (the OBS-0500 segfault re-check).

Outputs: <out-prefix>.csv (one row per file x engine) and <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import hashlib
import json
import os
import random
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

SEED = 20260927
TIMEOUT = 60
CLASSES = [("C1", "_header"), ("C2", "_xref"), ("C3", "_trailer"), ("C4", "_page_tree"),
           ("C5", "_object_header"), ("C6", "_font_mapping_loss"), ("C7", "_remove_fonts"),
           ("C8", "_remove_unicode_fonts"), ("C9", "_stream_zlib"), ("C10", "_partial_cut")]
ENV = {k: os.environ.get(k, d) for k, d in [
    ("QPDF", "qpdf"), ("QPDF_APT", "/usr/bin/qpdf"), ("MUTOOL", "mutool"), ("GS", "gs"),
    ("PDFTOTEXT", "pdftotext"), ("PDFCPU", "pdfcpu"), ("PDFBOX_JAR", "pdfbox-app.jar"), ("NODE", "node"),
    ("PDFJS_MJS", "pdf.mjs"), ("RSPROBE", "rsprobe")]}
PDFJS_PROBE = str(Path(__file__).with_name("engine_smoke_pdfjs.mjs"))


def nonws(s: str | bytes) -> int:
    if isinstance(s, bytes):
        s = s.decode("utf-8", "replace")
    return sum(1 for c in s if not c.isspace())


# ---------------------------------------------------------------- in-process probes (child process)
def probe(lib: str, src: str, out: str) -> None:
    r = {"ok": False, "pages": None, "chars": None, "error": None}
    try:
        if lib == "pikepdf":            # pikepdf.open() = libqpdf with its default recovery on
            import pikepdf
            with pikepdf.open(src) as pdf:
                r["pages"] = len(pdf.pages)
                pdf.save(out)
            r["ok"] = Path(out).stat().st_size > 0
        elif lib == "pymupdf":
            import pymupdf
            doc = pymupdf.open(src)
            r["pages"] = doc.page_count
            r["chars"] = sum(nonws(p.get_text()) for p in doc)
            r["ok"] = True
        elif lib == "pypdfium2":
            import pypdfium2 as pdfium
            doc = pdfium.PdfDocument(src)
            r["pages"] = len(doc)
            r["chars"] = sum(nonws(doc[i].get_textpage().get_text_range()) for i in range(len(doc)))
            r["ok"] = True
    except Exception as e:  # noqa: BLE001 - we record every failure
        r["error"] = f"{type(e).__name__}: {e}"[:300]
    print(json.dumps(r))


# ---------------------------------------------------------------- one engine run
def run(cmd: list[str], **kw) -> tuple[object, str, str]:
    try:
        cp = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT, **kw)
        return cp.returncode, cp.stdout.decode("utf-8", "replace"), cp.stderr.decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return "timeout", "", ""


def last_line(*texts: str) -> str | None:
    lines = [ln.strip() for t in texts for ln in t.splitlines() if ln.strip()
             and not ln.startswith("Picked up JAVA_TOOL_OPTIONS")]
    return lines[-1][:300] if lines else None


def nonwhite_fraction(png: Path) -> float | None:
    try:
        from PIL import Image
        hist = Image.open(png).convert("L").histogram()
        return round(sum(hist[:250]) / sum(hist), 5)
    except Exception:  # noqa: BLE001
        return None


def engine(name: str, p: Path) -> dict:
    """Run engine `name` on `p` (inside the temp dir); outputs go next to it."""
    o = p.with_name(f"{p.stem}.{name}")
    r: dict = {"rc": None, "ok": False, "pages": None, "chars": None, "note": None}
    if name == "qpdf_check":
        rc, so, se = run([ENV["QPDF"], "--check", str(p)])
        r.update(rc=rc, ok=rc in (0, 3), note=last_line(so, se))
    elif name == "qpdf_rewrite":
        rc, so, se = run([ENV["QPDF"], str(p), f"{o}.pdf"])
        ok = Path(f"{o}.pdf").exists() and Path(f"{o}.pdf").stat().st_size > 0
        pages = None
        if ok:
            rc2, so2, _ = run([ENV["QPDF"], "--show-npages", f"{o}.pdf"])
            pages = int(so2.strip()) if rc2 in (0, 3) and so2.strip().isdigit() else None
        r.update(rc=rc, ok=ok, pages=pages, note=last_line(so, se))
    elif name in ("pikepdf", "pymupdf", "pypdfium2"):
        rc, so, se = run([sys.executable, __file__, "--probe", name, str(p), f"{o}.pdf"])
        try:
            j = json.loads(so.strip().splitlines()[-1])
            r.update(rc=rc, ok=j["ok"], pages=j["pages"], chars=j["chars"], note=j["error"])
        except (IndexError, json.JSONDecodeError):
            r.update(rc=rc, note=last_line(se))
    elif name == "mutool_clean":            # mutool clean -gg: repair on load, garbage-collect, write
        rc, so, se = run([ENV["MUTOOL"], "clean", "-gg", str(p), f"{o}.pdf"])
        ok = Path(f"{o}.pdf").exists() and Path(f"{o}.pdf").stat().st_size > 0
        pages = None
        if ok:
            _, so2, _ = run([ENV["MUTOOL"], "pages", f"{o}.pdf"])
            pages = so2.count("<page pagenum=")
        r.update(rc=rc, ok=ok, pages=pages, note=last_line(se))
    elif name == "mutool_text":
        rc, so, se = run([ENV["MUTOOL"], "draw", "-q", "-F", "txt", "-o", "-", str(p)])
        n = nonws(so)
        r.update(rc=rc, ok=n > 0, chars=n, note=last_line(se))
    elif name == "pdfjs":
        rc, so, se = run([ENV["NODE"], PDFJS_PROBE, str(p)], env={**os.environ, "PDFJS_MJS": ENV["PDFJS_MJS"]})
        try:
            j = json.loads(so.strip().splitlines()[-1])
            r.update(rc=rc, ok=j["ok"] and (j["pages"] or 0) > 0, pages=j["pages"], chars=j["chars"], note=j["error"])
        except (IndexError, json.JSONDecodeError):
            r.update(rc=rc, note=last_line(se))
    elif name == "pdfcpu_validate":       # relaxed is pdfcpu's default mode
        rc, so, se = run([ENV["PDFCPU"], "validate", "-m", "relaxed", str(p)])
        r.update(rc=rc, ok=rc == 0, note=last_line(so, se))
    elif name == "pdfcpu_optimize":       # read (with pdfcpu's xref repair), write a new file
        rc, so, se = run([ENV["PDFCPU"], "optimize", str(p), f"{o}.pdf"])
        ok = rc == 0 and Path(f"{o}.pdf").exists() and Path(f"{o}.pdf").stat().st_size > 0
        pages = None
        if ok:
            _, so2, _ = run([ENV["PDFCPU"], "info", f"{o}.pdf"])
            for ln in so2.splitlines():
                if ln.strip().lower().startswith("page count:"):
                    pages = int(ln.split(":")[1])
        r.update(rc=rc, ok=ok, pages=pages, note=last_line(so, se))
    elif name == "pdfbox_text":           # -alwaysNext: continue to the next page after an IOException
        rc, so, se = run(["java", "-jar", ENV["PDFBOX_JAR"], "export:text", "-alwaysNext",
                          f"-i={p}", f"-o={o}.txt"])
        n = nonws(Path(f"{o}.txt").read_bytes()) if Path(f"{o}.txt").exists() else 0
        r.update(rc=rc, ok=n > 0, chars=n, note=last_line(se))
    elif name == "pdftotext":
        rc, so, se = run([ENV["PDFTOTEXT"], "-q", str(p), "-"])
        n = nonws(so)
        r.update(rc=rc, ok=n > 0, chars=n, note=last_line(se))
    elif name == "gs_text":               # PDF interpreter forced (GAP-256, OBS-0501), txtwrite device
        rc, so, se = run([ENV["GS"], "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=txtwrite", "-o", f"{o}.txt",
                          f"--permit-file-read={p.parent}/", "-c", f"({p}) (r) file runpdf"])
        n = nonws(Path(f"{o}.txt").read_bytes()) if Path(f"{o}.txt").exists() else 0
        r.update(rc=rc, ok=n > 0, chars=n, note=last_line(so, se))
    elif name in ("lopdf", "pdfrs", "hayro"):
        tgt = {"lopdf": [f"{o}.pdf"], "pdfrs": [], "hayro": [f"{o}.png"]}[name]
        rc, so, se = run([ENV["RSPROBE"], name, str(p), *tgt])
        out = last_line(so) or ""
        pages = None
        for tok in out.split():
            if tok.startswith("pages="):
                pages = int(tok.split("=")[1])
        r.update(rc=rc, ok=rc == 0 and out.endswith(tuple("0123456789")) and " ok " in f" {out} ",
                 pages=pages, note=(out or last_line(se)))
        if name == "hayro" and Path(f"{o}.png").exists():
            r["chars"] = None
            r["note"] = f"{out}; page1 nonwhite={nonwhite_fraction(Path(f'{o}.png'))}"
    if r["pages"] is not None and r["pages"] < 1:    # opened, but no page survived: no output
        r["ok"] = False
    return r


ENGINES = ["qpdf_check", "qpdf_rewrite", "pikepdf", "mutool_clean", "mutool_text", "pymupdf", "pdfjs",
           "pypdfium2", "pdfcpu_validate", "pdfcpu_optimize", "pdfbox_text", "pdftotext", "gs_text",
           "lopdf", "pdfrs", "hayro"]
PRODUCES = {  # what "produced output" means per engine
    "qpdf_check": "exit 0 or 3 (checker ran to completion; an oracle, not a repair)",
    "qpdf_rewrite": "non-empty output PDF with >=1 page", "pikepdf": "open + save succeeded, >=1 page",
    "mutool_clean": "non-empty output PDF with >=1 page", "mutool_text": ">0 text chars",
    "pymupdf": "document opened with >=1 page", "pdfjs": "document opened with >=1 page",
    "pypdfium2": "document opened with >=1 page", "pdfcpu_validate": "exit 0 (valid in relaxed mode; an oracle)",
    "pdfcpu_optimize": "exit 0 and non-empty output PDF with >=1 page", "pdfbox_text": ">0 text chars",
    "pdftotext": ">0 text chars", "gs_text": ">0 text chars", "lopdf": "load + save succeeded, >=1 page",
    "pdfrs": "document opened with >=1 page", "hayro": "document loaded, >=1 page, every page rendered"}


def versions() -> dict:
    def first(cmd, **kw):
        rc, so, se = run(cmd, **kw)
        return last_line(so.splitlines()[0] if so.strip() else "", se.splitlines()[0] if se.strip() and not so.strip() else "")
    v = {"qpdf": first([ENV["QPDF"], "--version"]), "mutool": first([ENV["MUTOOL"], "-v"]),
         "gs": first([ENV["GS"], "--version"]), "pdftotext": first([ENV["PDFTOTEXT"], "-v"]),
         "pdfcpu": first([ENV["PDFCPU"], "version"]),
         "pdfbox": first(["java", "-jar", ENV["PDFBOX_JAR"], "version"]),
         "node": first([ENV["NODE"], "--version"]), "python": sys.version.split()[0]}
    for mod, expr in (("pikepdf", "m.__version__ + ' (libqpdf ' + m.__libqpdf_version__ + ')'"),
                      ("pymupdf", "m.VersionBind + ' (MuPDF ' + m.VersionFitz + ')'"),
                      ("pypdfium2", "str(m.version.PYPDFIUM_INFO) + ' (PDFium ' + str(m.version.PDFIUM_INFO) + ')'")):
        rc, so, _ = run([sys.executable, "-c", f"import {mod} as m; print({expr})"])
        v[mod] = so.strip() or None
    pj = Path(ENV["PDFJS_MJS"]).resolve()
    for parent in pj.parents:
        if (parent / "package.json").exists():
            v["pdfjs-dist"] = json.loads((parent / "package.json").read_text()).get("version")
            break
    v["rsprobe"] = "lopdf 0.45.0, pdf 0.10.0, hayro 0.7.1 (hayro-syntax 0.7.2); see rsprobe/Cargo.lock"
    return v


def sample(root: Path) -> list[dict]:
    random.seed(SEED)
    picks = []
    for cls, suf in CLASSES:
        files = sorted((root / "corrupted").rglob(f"*){suf}.pdf"))
        for f in random.sample(files, 2):
            stem = f.name[: -len(f"{suf}.pdf")]
            rel = f.parent.relative_to(root / "corrupted")
            picks.append({"class": cls, "corrupted": f, "original": root / "original" / rel / f"{stem}.pdf",
                          "doc": f"{rel.as_posix()}/{stem}"})
    return picks


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main() -> None:
    if sys.argv[1] == "--probe":
        probe(*sys.argv[2:5])
        return
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    recheck = sys.argv[sys.argv.index("--recheck") + 1] if "--recheck" in sys.argv else None
    picks = sample(root)
    files: dict[str, tuple[str, Path]] = {}      # key -> (class or 'original', path)
    for pk in picks:
        files[f"{pk['class']}:{pk['doc']}"] = (pk["class"], pk["corrupted"])
        files[f"original:{pk['doc']}"] = ("original", pk["original"])
    rows = []
    with tempfile.TemporaryDirectory(prefix="engsmoke_") as td, ThreadPoolExecutor(2) as ex:
        jobs = []
        for i, (key, (cls, path)) in enumerate(sorted(files.items())):
            p = Path(td) / f"f{i:02d}.pdf"
            p.write_bytes(path.read_bytes())
            for e in ENGINES:
                jobs.append((key, cls, path, e, ex.submit(engine, e, p)))
        res = {(k, e): (cls, path, f.result()) for k, cls, path, e, f in jobs}
    for (key, e), (cls, path, r) in sorted(res.items()):
        doc = key.split(":", 1)[1]
        base = res[(f"original:{doc}", e)][2]
        ratio = (round(r["chars"] / base["chars"], 4) if r["chars"] is not None and base["chars"] else None)
        rows.append({"file": str(path.relative_to(root)), "class": cls, "engine": e, "rc": r["rc"],
                     "produced_output": int(bool(r["ok"])), "pages": r["pages"], "chars": r["chars"],
                     "text_ratio_vs_original": ratio if cls != "original" else None, "note": r["note"]})
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out.with_suffix(".csv"), "w", newline="") as f:
        w = csv.DictWriter(f, list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
    classes = ["original"] + [c for c, _ in CLASSES]
    table = {e: {c: f"{sum(r['produced_output'] for r in rows if r['engine'] == e and r['class'] == c)}"
                    f"/{sum(1 for r in rows if r['engine'] == e and r['class'] == c)}" for c in classes}
             for e in ENGINES}
    text95 = {e: {c: sum(1 for r in rows if r["engine"] == e and r["class"] == c
                         and r["text_ratio_vs_original"] is not None and r["text_ratio_vs_original"] >= 0.95)
                  for c in classes[1:]} for e in ENGINES
              if any(r["chars"] is not None for r in rows if r["engine"] == e)}
    summary = {
        "kind": "smoke test, not a benchmark",
        "seed": SEED, "timeout_s": TIMEOUT, "tools": versions(), "produced_output_means": PRODUCES,
        "inputs": [{"class": pk["class"], "corrupted": str(pk["corrupted"].relative_to(root)),
                    "corrupted_sha256": sha(pk["corrupted"]), "original": str(pk["original"].relative_to(root)),
                    "original_sha256": sha(pk["original"])} for pk in picks],
        "n_files": len(files), "produced_output_by_engine_and_class": table,
        "text_ratio_ge_0.95_by_engine_and_class": text95,
        "timeouts": sum(1 for r in rows if r["rc"] == "timeout"),
        "crashes_negative_rc": [(r["file"], r["engine"], r["rc"]) for r in rows
                                if isinstance(r["rc"], int) and r["rc"] < 0],
    }
    if recheck:
        rp = Path(recheck)
        summary["obs0500_recheck"] = {"file": recheck, "sha256": sha(rp)}
        for label, exe in (("apt", ENV["QPDF_APT"]), ("latest", ENV["QPDF"])):
            rc, so, se = run([exe, "--check", recheck])
            vv = run([exe, "--version"])[1].splitlines()[0]
            summary["obs0500_recheck"][label] = {"version": vv, "rc": rc,
                                                 "last_lines": [ln for ln in (so + se).splitlines() if ln.strip()][-2:]}
    (out.parent / (out.name + ".summary.json")).write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps({k: summary[k] for k in ("tools", "produced_output_by_engine_and_class", "timeouts",
                                              "crashes_negative_rc")} | ({"obs0500_recheck": summary["obs0500_recheck"]}
                                                                         if recheck else {}), indent=1))


if __name__ == "__main__":
    main()
