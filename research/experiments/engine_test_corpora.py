#!/usr/bin/env python3
"""Harvest the malformed/odd PDFs that ship in mature PDF engines' test suites (OBS-0500).

    python3 research/experiments/engine_test_corpora.py \
        [--cache research/cache/code] [--out research/experiments/results/engine_test_corpora] \
        [--timeout 10] [--workers 4]

For each engine checkout (pinned commits below, fetched with research/tools/fetch_code.py) the
script walks the engine's *test* directories and lists every PDF or PDF-like file:
  - kind=pdf        a file with a .pdf extension;
  - kind=fuzz       a qpdf OSS-Fuzz reproducer (*.fuzz, header often destroyed) or a fuzz seed
                    with "%PDF-" in its first 1024 bytes;
  - kind=template   a PDFium hand-written .in template, expanded to a PDF with PDFium's own
                    testing/tools/fixup_pdf_template.py (run from the checkout, never copied);
  - kind=magic      any other file with "%PDF-" in its first 1024 bytes.
Each file gets repo, path, size, sha256, licence and a label from `qpdf --check` (exit code,
warning count and the first error/warning line, 10 s timeout per file). pdf.js *.link files
(test PDFs hosted elsewhere) are listed separately, joined to test/test_manifest.json; they are
NOT downloaded. No PDF is copied anywhere; outputs are CSV + JSON only.

qpdf --check exit codes: 0 = no problems, 3 = warnings (qpdf recovered/damaged), 2 = errors;
a negative return code is a crash (signal). "Damaged" = warnings | error | timeout | crash.
Messages are grouped into coarse classes (MSG_CLASSES, our own grouping) so the summary can say
what kind of damage the engines test; qpdf_reconstructed marks files where qpdf had to rebuild
the cross-reference table by scanning.
"""

from __future__ import annotations

import argparse
import collections
import csv
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "research" / "tools"))
from kbcommon import code_checkout  # noqa: E402

# Pinned checkouts (fetch each with research/tools/fetch_code.py <SRC> <url> <commit> --license ...).
# test_roots: directories (relative to the checkout) that hold each engine's test files.
ENGINES = [
    dict(engine="qpdf", src="SRC-0500", url="https://github.com/qpdf/qpdf",
         commit="4eba95899886e851cc41d76886483b347612f2a8", licence="Apache-2.0",
         test_roots=["qpdf/qtest", "examples/qtest", "compare-for-test/qtest", "libtests/qtest", "fuzz"]),
    dict(engine="mupdf", src="SRC-0501", url="https://github.com/ArtifexSoftware/mupdf",
         commit="d587982f971e2404e72b0539b2d9c60ad2c9b03f", licence="AGPL-3.0-or-later",
         test_roots=[]),  # MuPDF keeps no test-file corpus in its public repository
    dict(engine="pdf.js", src="SRC-0502", url="https://github.com/mozilla/pdf.js",
         commit="d52fdf411a6e4d338180687456e0df019e28475e", licence="Apache-2.0",
         test_roots=["test"]),
    dict(engine="pdfium", src="SRC-0503", url="https://pdfium.googlesource.com/pdfium",
         commit="8ca5b735df4263f43c830479b78213854a392c22", licence="BSD-3-Clause",
         test_roots=["testing"]),
    dict(engine="poppler", src="SRC-0504", url="https://gitlab.freedesktop.org/poppler/poppler.git",
         commit="c8dc23d564f233875386b7aa1d59fc909ea58799", licence="GPL-2.0-or-later",
         test_roots=["test", "qt5/tests", "qt6/tests", "glib/tests", "cpp/tests"]),
    dict(engine="poppler", src="SRC-0506", url="https://gitlab.freedesktop.org/poppler/test.git",
         commit="48b6219b84fc0a708040cb279d51095cc4e1c603", licence="NOASSERTION (repo COPYING is GPL-2.0; PDFs of mixed origin)",
         test_roots=["."]),
    dict(engine="ghostscript", src="SRC-0505", url="https://github.com/ArtifexSoftware/ghostpdl",
         commit="a3cc9bd74ac017c169da704c8792dcb2365094f3", licence="AGPL-3.0-or-later",
         test_roots=["toolbin/tests", "examples"]),  # no regression corpus in the public repo
]

FIELDS = ["engine", "src", "licence", "path", "kind", "size", "sha256", "header_offset",
          "has_eof_marker", "qpdf_exit", "qpdf_label", "qpdf_warnings", "qpdf_reconstructed",
          "qpdf_first_class", "qpdf_msg_classes", "qpdf_first_msg", "qpdf_seconds", "expanded_sha256"]
LINK_FIELDS = ["engine", "src", "path", "url", "host", "manifest_ids", "manifest_md5",
               "manifest_types", "password"]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


SOURCE_EXT = {".c", ".cc", ".cpp", ".h", ".js", ".mjs", ".py", ".pl", ".json", ".md", ".html",
              ".txt", ".test", ".cmake", ".sh", ".gn", ".xml"}


def kind_of(p: Path, head: bytes) -> str | None:
    suf = p.suffix.lower()
    if suf in SOURCE_EXT:
        return None  # test scripts/expected outputs that merely mention "%PDF-"
    if suf == ".pdf":
        return "pdf"
    if suf == ".in" and "testing" in p.parts:
        return "template"
    if suf == ".link":
        return None  # handled separately
    if suf == ".fuzz":
        return "fuzz"  # qpdf OSS-Fuzz reproducers: PDF-like even when the header is destroyed
    if b"%PDF-" in head:
        return "fuzz" if (suf == ".fuzz" or "fuzz" in p.parts) else "magic"
    return None


GENERIC = ("file is damaged", "attempting to reconstruct cross-reference table",
           "operation succeeded with warnings", "errors detected")
# Coarse message classes, checked in order (first match wins). Our own grouping, for the summary.
MSG_CLASSES = [
    ("encryption", r"password|encrypt|/id in trailer"),
    ("header", r"pdf header"),
    ("linearization", r"lineariz|hint table|\(/e\) mismatch|first page|shared object|shared identifier"
                      r"|outlines table|/l mismatch|/o mismatch|/h |page offset|hint stream|nshared"),
    ("filter-decode", r"decoding stream data|zlib|lzw|flate|decoder|predictor|dct|jbig2|jpx|runlength"
                      r"|output may still be valid"
                      r"|ascii85|asciihex|unsupported filter|filter"),
    ("stream-length", r"endstream|stream length|/length|stream keyword"),
    ("xref-trailer", r"xref|startxref|cross-reference|reported number of objects|trailer|/root|/size"
                     r"|object stream|objstm|offset 0"),
    ("page-tree", r"kid |pages tree|/pages|mediabox|page object|/resources|resources is|annots|catalog"
                  r"|/type key should be /page|no pages|/count"),
    ("object-syntax", r"expected endobj|unknown token|expected n n obj|obj$|dictionary|unexpected|treating as"
                      r"|stray #|parse|object has offset|loop detected|not a stream|null|string|name|array"
                      r"|eof|expected|negative number|deeply nested|id 0"),
]


def msg_class(msg: str) -> str:
    m = msg.lower()
    for name, rx in MSG_CLASSES:
        if re.search(rx, m):
            return name
    return "other"


def qpdf_label(path: Path, timeout: float) -> dict:
    t0 = time.monotonic()
    try:
        cp = subprocess.run(["qpdf", "--check", str(path)], capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return dict(qpdf_exit="timeout", qpdf_label="timeout", qpdf_warnings="", qpdf_reconstructed="",
                    qpdf_first_msg="", qpdf_first_class="", qpdf_msg_classes="",
                    qpdf_seconds=round(time.monotonic() - t0, 2))
    out = (cp.stderr + b"\n" + cp.stdout).decode("utf-8", "replace").replace(str(path), "<file>")
    lines = [ln.strip() for ln in out.splitlines() if ln.strip()]
    msgs = [ln for ln in lines if ln.startswith("WARNING:") or ln.startswith("qpdf:")
            or "invalid password" in ln]
    specific = [m for m in msgs if not any(g in m.lower() for g in GENERIC)]
    first = specific[0] if specific else (msgs[0] if msgs else "")
    if cp.returncode < 0:
        label = f"crash-signal-{-cp.returncode}"
    else:
        label = {0: "ok", 3: "warnings", 2: "error"}.get(cp.returncode, f"exit-{cp.returncode}")
    if "invalid password" in out:
        label = "password"
    classes = sorted({msg_class(norm_msg(m)) for m in specific})
    return dict(qpdf_exit=cp.returncode, qpdf_label=label,
                qpdf_warnings=sum(ln.startswith("WARNING:") for ln in lines),
                qpdf_reconstructed=int("attempting to reconstruct cross-reference table" in out.lower()),
                qpdf_first_msg=first[:300], qpdf_first_class=msg_class(norm_msg(first)) if first else "",
                qpdf_msg_classes=";".join(classes), qpdf_seconds=round(time.monotonic() - t0, 2))


def expand_template(checkout: Path, src: Path, tmp: Path) -> Path | None:
    out_dir = tmp / sha256(str(src).encode())[:16]
    out_dir.mkdir(parents=True, exist_ok=True)
    tool = checkout / "testing" / "tools" / "fixup_pdf_template.py"
    try:
        subprocess.run([sys.executable, str(tool), "--output-dir", str(out_dir), str(src)],
                       check=True, capture_output=True, timeout=30)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        return None
    out = out_dir / (src.stem + ".pdf")
    return out if out.exists() else None


def scan_engine(spec: dict, cache: Path) -> tuple[list[dict], list[dict], int]:
    checkout = code_checkout(spec["url"], spec["commit"])
    if cache != REPO / "research" / "cache" / "code":
        checkout = cache / checkout.name
    if not checkout.is_dir():
        sys.exit(f"missing checkout {checkout}: run research/tools/fetch_code.py {spec['src']} "
                 f"{spec['url']} {spec['commit']} --license ...")
    files, links = [], []
    for root in spec["test_roots"]:
        base = checkout / root
        if not base.is_dir():
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d != ".git")
            for fn in sorted(filenames):
                p = Path(dirpath) / fn
                if p.is_symlink() or not p.is_file():
                    continue
                rel = p.relative_to(checkout).as_posix()
                if p.suffix.lower() == ".link":
                    links.append(dict(engine=spec["engine"], src=spec["src"], path=rel,
                                      url=p.read_text(errors="replace").strip()))
                    continue
                with open(p, "rb") as f:
                    head = f.read(1024)
                kind = kind_of(p, head)
                if kind:
                    files.append(dict(engine=spec["engine"], src=spec["src"], licence=spec["licence"],
                                      path=rel, kind=kind, _abs=p))
    # every PDF anywhere in the checkout (to report non-test PDFs that were excluded)
    all_pdfs = sum(1 for p in checkout.rglob("*") if p.suffix.lower() == ".pdf" and ".git" not in p.parts)
    return files, links, all_pdfs


def enrich(rec: dict, timeout: float, tmp: Path, checkout: Path) -> dict:
    p: Path = rec.pop("_abs")
    data = p.read_bytes()
    rec.update(size=len(data), sha256=sha256(data), expanded_sha256="")
    target = p
    if rec["kind"] == "template":
        exp = expand_template(checkout, p, tmp)
        if exp is None:
            rec.update(header_offset="", has_eof_marker="", qpdf_exit="", qpdf_label="expand-failed",
                       qpdf_warnings="", qpdf_reconstructed="", qpdf_first_msg="", qpdf_first_class="",
                       qpdf_msg_classes="", qpdf_seconds="")
            return rec
        target = exp
        data = exp.read_bytes()
        rec["expanded_sha256"] = sha256(data)
    rec["header_offset"] = data[:1024].find(b"%PDF-")
    rec["has_eof_marker"] = int(b"%%EOF" in data[-1024:])
    rec.update(qpdf_label(target, timeout))
    return rec


def norm_msg(m: str) -> str:
    m = re.sub(r"^(WARNING|qpdf): <file>(?: \([^)]*\)|, [^:]*)?: ?", "", m)
    m = re.sub(r"^<file>(?: \([^)]*\))?: ?", "", m)
    return re.sub(r"\d+", "N", m)[:120]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--cache", type=Path, default=REPO / "research" / "cache" / "code")
    ap.add_argument("--out", type=Path, default=REPO / "research" / "experiments" / "results" / "engine_test_corpora")
    ap.add_argument("--timeout", type=float, default=10.0)
    ap.add_argument("--workers", type=int, default=4)
    a = ap.parse_args()

    qv = subprocess.run(["qpdf", "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    rows, links, non_test = [], [], {}
    with tempfile.TemporaryDirectory(prefix="engine_corpora_") as td, ThreadPoolExecutor(a.workers) as ex:
        futs = []
        for spec in ENGINES:
            files, lk, all_pdfs = scan_engine(spec, a.cache)
            checkout = code_checkout(spec["url"], spec["commit"])
            non_test[spec["src"]] = all_pdfs
            links += lk
            futs += [ex.submit(enrich, f, a.timeout, Path(td), checkout) for f in files]
        rows = [f.result() for f in futs]
    rows.sort(key=lambda r: (r["engine"], r["src"], r["path"]))

    # pdf.js: join .link files to the test manifest
    pdfjs = next(s for s in ENGINES if s["engine"] == "pdf.js")
    manifest = json.loads((code_checkout(pdfjs["url"], pdfjs["commit"]) / "test" / "test_manifest.json").read_text())
    by_file = collections.defaultdict(list)
    for m in manifest:
        by_file[m.get("file", "")].append(m)
    for lk in links:
        ms = by_file.get("pdfs/" + Path(lk["path"]).name.removesuffix(".link"), [])
        lk.update(host=re.sub(r"^[a-z]+://([^/]+).*$", r"\1", lk["url"]) if "://" in lk["url"] else "",
                  manifest_ids=";".join(m["id"] for m in ms),
                  manifest_md5=";".join(sorted({m.get("md5", "") for m in ms})),
                  manifest_types=";".join(sorted({m.get("type", "") for m in ms})),
                  password=int(any("password" in m for m in ms)))

    a.out.parent.mkdir(parents=True, exist_ok=True)
    with open(a.out.with_suffix(".csv"), "w", newline="") as f:
        w = csv.DictWriter(f, FIELDS)
        w.writeheader()
        w.writerows(rows)
    with open(a.out.parent / (a.out.name + "_links.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, LINK_FIELDS)
        w.writeheader()
        w.writerows(sorted(links, key=lambda r: r["path"]))

    damaged_labels = {"warnings", "error", "timeout"} | {r["qpdf_label"] for r in rows
                                                          if r["qpdf_label"].startswith("crash")}
    summary = {"tools": {"qpdf": qv, "python": sys.version.split()[0]},
               "timeout_s": a.timeout, "engines": {}, "totals": {}}
    for spec in ENGINES:
        rs = [r for r in rows if r["src"] == spec["src"]]
        labels = collections.Counter(r["qpdf_label"] for r in rs)
        msgs = collections.Counter(norm_msg(r["qpdf_first_msg"]) for r in rs
                                   if r["qpdf_label"] in damaged_labels and r["qpdf_first_msg"])
        summary["engines"][spec["src"]] = {
            "engine": spec["engine"], "url": spec["url"], "commit": spec["commit"], "licence": spec["licence"],
            "test_roots": spec["test_roots"], "files": len(rs),
            "unique_sha256": len({r["sha256"] for r in rs}),
            "bytes": sum(r["size"] for r in rs),
            "by_kind": dict(collections.Counter(r["kind"] for r in rs)),
            "qpdf_labels": dict(labels),
            "qpdf_flags_damaged": sum(labels[k] for k in damaged_labels),
            "qpdf_reconstructed_xref": sum(1 for r in rs if r["qpdf_reconstructed"] == 1),
            "first_message_class": dict(collections.Counter(r["qpdf_first_class"] for r in rs
                                                            if r["qpdf_label"] in damaged_labels)),
            "files_with_message_class": dict(collections.Counter(
                c for r in rs if r["qpdf_msg_classes"] for c in r["qpdf_msg_classes"].split(";"))),
            "damaged_excluding_linearization_only": sum(
                1 for r in rs if r["qpdf_label"] in damaged_labels
                and r["qpdf_msg_classes"] != "linearization"),
            "no_header_in_first_1024": sum(1 for r in rs if r["header_offset"] == -1),
            "no_eof_in_last_1024": sum(1 for r in rs if r["has_eof_marker"] == 0),
            "pdfs_anywhere_in_checkout": non_test[spec["src"]],
            "top_first_messages": msgs.most_common(12),
            "link_files": sum(1 for lk in links if lk["src"] == spec["src"]),
        }
    all_labels = collections.Counter(r["qpdf_label"] for r in rows)
    summary["totals"] = {
        "files": len(rows), "unique_sha256": len({r["sha256"] for r in rows}),
        "qpdf_labels": dict(all_labels), "qpdf_flags_damaged": sum(all_labels[k] for k in damaged_labels),
        "damaged_unique_sha256": len({r["sha256"] for r in rows if r["qpdf_label"] in damaged_labels}),
        "damaged_excluding_linearization_only": sum(
            1 for r in rows if r["qpdf_label"] in damaged_labels and r["qpdf_msg_classes"] != "linearization"),
        "qpdf_reconstructed_xref": sum(1 for r in rows if r["qpdf_reconstructed"] == 1),
        "first_message_class": dict(collections.Counter(r["qpdf_first_class"] for r in rows
                                                        if r["qpdf_label"] in damaged_labels)),
        "link_files": len(links), "link_hosts": len({lk["host"] for lk in links}),
        "link_files_password": sum(lk["password"] for lk in links),
    }
    (a.out.parent / (a.out.name + ".summary.json")).write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary["totals"], indent=1))
    for s, e in summary["engines"].items():
        print(s, e["engine"], "files", e["files"], "damaged", e["qpdf_flags_damaged"], e["qpdf_labels"])


if __name__ == "__main__":
    main()
