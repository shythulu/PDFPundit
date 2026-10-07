#!/usr/bin/env python3
"""How do installed engine CLIs cope with header loss alone vs header loss + a missing xref? (OBS-0501)

    python3 research/experiments/engine_header_loss.py /home/user/dfrc-korea/repdf \
        research/experiments/results/engine_header_loss

Motivation (engine source study, CLM-0540): Ghostscript's pdfi repair gives up when no "%PDF"
appears anywhere in the file, whereas qpdf and MuPDF continue without a header. REPDF's C1
("_header") files overwrite the first 12 bytes, which removes the only "%PDF" in the file, but
leave the xref intact, so repair may never be needed. This script therefore also builds a
*compound* variant per document (our DMG-001-style combination, generated in a temp dir only):
the C2 ("_xref", classic xref removed) file with its first 12 bytes replaced by the C1 file's
first 12 bytes. Header + xref loss forces every engine into its repair path without a header.

Variants per document: original, C1, C2, C1+C2 (compound). Tools: qpdf --check (exit code),
mutool draw -F txt, gs -sDEVICE=txtwrite (default CLI, which picks PostScript or PDF by sniffing the
header), gs with the PDF interpreter forced ("runpdf"), pdftotext (characters of extracted text). For each
tool the extracted text of a damaged variant is compared with that tool's text from the original
(ratio of non-whitespace characters; "recovered" = ratio >= 0.95; gs writes text to a file because
it reports errors on stdout). Nothing is written except the
CSV/JSON outputs; the compound files live in a temporary directory that is deleted.
"""

from __future__ import annotations

import csv
import json
import statistics
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

TIMEOUT = 30


def versions() -> dict:
    def first(cmd):
        cp = subprocess.run(cmd, capture_output=True, text=True)
        return (cp.stdout or cp.stderr).strip().splitlines()[0]
    return {"qpdf": first(["qpdf", "--version"]), "mutool": first(["mutool", "-v"]),
            "gs": first(["gs", "--version"]), "pdftotext": first(["pdftotext", "-v"]),
            "python": sys.version.split()[0]}


def run(cmd: list[str]) -> tuple[int | str, bytes]:
    try:
        cp = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT)
        return cp.returncode, cp.stdout
    except subprocess.TimeoutExpired:
        return "timeout", b""


def nonws(b: bytes) -> int:
    return sum(1 for c in b.decode("utf-8", "replace") if not c.isspace())


def tools(path: Path) -> dict:
    """Run every tool on `path` (a simple-named copy inside the temp dir)."""
    p = str(path)
    out = {"has_%PDF": int(b"%PDF" in path.read_bytes())}
    rc, _ = run(["qpdf", "--check", p])
    out["qpdf"] = (rc, None)
    rc, txt = run(["mutool", "draw", "-q", "-F", "txt", "-o", "-", p])
    out["mutool"] = (rc, nonws(txt))
    rc, txt = run(["pdftotext", "-q", p, "-"])
    out["pdftotext"] = (rc, nonws(txt))
    # gs: txtwrite into a file (gs reports errors on stdout). "gs" = default CLI, which sniffs the
    # file type from its header; "gs_forced" = the PDF interpreter invoked explicitly (runpdf).
    for name, extra in (("gs", ["-dSAFER", p]),
                        ("gs_forced", [f"--permit-file-read={path.parent}/", "-c", f"({p}) (r) file runpdf"])):
        txt_path = path.with_suffix(f".{name}.txt")
        rc, _ = run(["gs", "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=txtwrite", "-o", str(txt_path), *extra])
        out[name] = (rc, nonws(txt_path.read_bytes()) if txt_path.exists() else 0)
    return out


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    docs = []
    for c1 in sorted((root / "corrupted").rglob("*)_header.pdf")):
        stem = c1.name[: -len("_header.pdf")]
        rel = c1.parent.relative_to(root / "corrupted")
        orig = root / "original" / rel / f"{stem}.pdf"
        c2 = c1.with_name(f"{stem}_xref.pdf")
        if orig.exists() and c2.exists():
            docs.append((f"{rel.as_posix()}/{stem}", orig, c1, c2))

    rows = []
    with tempfile.TemporaryDirectory(prefix="hdrloss_") as td, ThreadPoolExecutor(4) as ex:
        jobs = []
        for i, (doc, orig, c1, c2) in enumerate(docs):
            b2 = bytearray(c2.read_bytes())
            b2[:12] = c1.read_bytes()[:12]           # compound: C2 file with C1's damaged header
            variants = {"original": orig.read_bytes(), "C1": c1.read_bytes(), "C2": c2.read_bytes(),
                        "C1+C2": bytes(b2)}
            for j, (variant, data) in enumerate(variants.items()):
                p = Path(td) / f"{i:03d}_{j}.pdf"        # simple names (REPDF names contain parentheses)
                p.write_bytes(data)
                jobs.append((doc, variant, ex.submit(tools, p)))
        results = {(d, v): f.result() for d, v, f in jobs}

    tool_names = ["qpdf", "mutool", "gs", "gs_forced", "pdftotext"]
    for (doc, variant), res in sorted(results.items()):
        base = results[(doc, "original")]
        row = {"doc": doc, "variant": variant, "has_%PDF": res["has_%PDF"]}
        for t in tool_names:
            rc, chars = res[t]
            row[f"{t}_exit"] = rc
            if t != "qpdf":
                row[f"{t}_chars"] = chars
                row[f"{t}_ratio"] = round(chars / base[t][1], 4) if base[t][1] else ""
        rows.append(row)

    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out.with_suffix(".csv"), "w", newline="") as f:
        w = csv.DictWriter(f, list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)

    summary = {"tools": versions(), "timeout_s": TIMEOUT, "documents": len(docs), "by_variant": {}}
    for variant in ("original", "C1", "C2", "C1+C2"):
        rs = [r for r in rows if r["variant"] == variant]
        s = {"n": len(rs), "files_with_%PDF": sum(r["has_%PDF"] == 1 for r in rs),
             "qpdf_exit_counts": {str(k): sum(1 for r in rs if str(r["qpdf_exit"]) == str(k))
                                  for k in sorted({str(r["qpdf_exit"]) for r in rs})}}
        for t in tool_names[1:]:
            ratios = [r[f"{t}_ratio"] for r in rs if r[f"{t}_ratio"] != ""]
            s[t] = {"recovered_ge_0.95": sum(1 for x in ratios if x >= 0.95),
                    "zero_text": sum(1 for x in ratios if x == 0),
                    "median_ratio": statistics.median(ratios) if ratios else None,
                    "nonzero_exit": sum(1 for r in rs if r[f"{t}_exit"] != 0)}
        summary["by_variant"][variant] = s
    (out.parent / (out.name + ".summary.json")).write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
