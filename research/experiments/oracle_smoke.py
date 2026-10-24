#!/usr/bin/env python3
"""Smoke test (NOT a benchmark) of conformance oracles on damaged and repaired PDFs (OBS-0702).

    TMPDIR=<scratch> python3 oracle_smoke.py /home/user/dfrc-korea/repdf <out-prefix>

Same seeded REPDF sample as engine_smoke.py (seed 20260927, 2 corrupted files per class C1..C10 plus
their originals). Each corrupted file is also repaired twice, by `qpdf in out` and by
`mutool clean -gg in out`, in a temp dir. Four variants per pick: original, corrupted, qpdf-repaired,
mutool-repaired. Oracles run on every variant:
  - qpdf --check                       exit 0 = no problems, 3 = warnings only, 2 = errors
  - pdfcpu validate -m strict / relaxed exit 0 = valid
  - veraPDF Arlington checker          (arlington-pdf-model-checker --format json, flavour detected
                                         from the file): compliant / non-compliant (failed checks) /
                                         not parsed (taskException)
The question is only which oracle can *judge* which variant, and whether the oracles agree;
conformance says nothing about whether the repaired content is right (GAP-106).

Environment: QPDF, MUTOOL, PDFCPU, ARLINGTON (path to arlington-pdf-model-checker).
Outputs: <out-prefix>.csv and <out-prefix>.summary.json.
"""

from __future__ import annotations

import csv
import json
import os
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from engine_smoke import sample, sha  # noqa: E402  (same seeded sample)

TIMEOUT = 120
ENV = {k: os.environ.get(k, d) for k, d in [("QPDF", "qpdf"), ("MUTOOL", "mutool"), ("PDFCPU", "pdfcpu"),
                                            ("ARLINGTON", "arlington-pdf-model-checker")]}


def run(cmd: list[str]) -> tuple[object, str]:
    try:
        cp = subprocess.run(cmd, capture_output=True, timeout=TIMEOUT)
        return cp.returncode, (cp.stdout + cp.stderr).decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return "timeout", ""


def arlington(paths: list[Path]) -> dict[str, dict]:
    """One JVM for the whole batch; returns {path: {status, failed_checks, failed_rules, profile, note}}."""
    try:
        cp = subprocess.run([ENV["ARLINGTON"], "--format", "json", *map(str, paths)], capture_output=True,
                            timeout=TIMEOUT * 10)
        report = json.loads(cp.stdout.decode("utf-8", "replace"))
    except (subprocess.TimeoutExpired, json.JSONDecodeError) as e:
        return {str(p): {"status": "checker-error", "note": str(e)[:200]} for p in paths}
    out = {}
    for job in report["report"]["jobs"]:
        name = job["itemDetails"]["name"]
        if "taskException" in job:
            out[name] = {"status": "not-parsed", "note": job["taskException"].get("exceptionMessage", "")[:200]}
            continue
        res = (job.get("arlingtonResult") or [{}])[0]
        det = res.get("details", {})
        out[name] = {"status": "compliant" if res.get("compliant") else "non-compliant",
                     "failed_checks": det.get("failedChecks"), "failed_rules": det.get("failedRules"),
                     "profile": res.get("profileName"), "note": None}
    return out


def versions() -> dict:
    first = lambda cmd: next((ln for ln in run(cmd)[1].splitlines()  # noqa: E731
                              if ln.strip() and not ln.startswith("Picked up")), None)
    return {"qpdf": first([ENV["QPDF"], "--version"]), "mutool": first([ENV["MUTOOL"], "-v"]),
            "pdfcpu": first([ENV["PDFCPU"], "version"]), "arlington": first([ENV["ARLINGTON"], "--version"])}


def main() -> None:
    root, out = Path(sys.argv[1]), Path(sys.argv[2])
    picks = sample(root)
    rows = []
    with tempfile.TemporaryDirectory(prefix="oracles_") as td:
        variants: list[tuple[str, str, Path]] = []      # (pick label, variant, path)
        seen_orig = {}
        for i, pk in enumerate(picks):
            label = f"{pk['class']}:{pk['doc']}"
            c = Path(td) / f"c{i:02d}.pdf"
            c.write_bytes(pk["corrupted"].read_bytes())
            if pk["doc"] not in seen_orig:
                o = Path(td) / f"o{i:02d}.pdf"
                o.write_bytes(pk["original"].read_bytes())
                seen_orig[pk["doc"]] = o
                variants.append((f"original:{pk['doc']}", "original", o))
            variants.append((label, "corrupted", c))
            q, m = Path(td) / f"c{i:02d}.qpdf.pdf", Path(td) / f"c{i:02d}.mutool.pdf"
            run([ENV["QPDF"], str(c), str(q)])
            run([ENV["MUTOOL"], "clean", "-gg", str(c), str(m)])
            for v, p in (("qpdf-repaired", q), ("mutool-repaired", m)):
                if p.exists() and p.stat().st_size > 0:
                    variants.append((label, v, p))
                else:
                    rows.append({"pick": label, "variant": v, "exists": 0})
        arl = arlington([p for _, _, p in variants])
        for label, v, p in variants:
            rc_q, txt_q = run([ENV["QPDF"], "--check", str(p)])
            rc_s, txt_s = run([ENV["PDFCPU"], "validate", "-m", "strict", str(p)])
            rc_r, _ = run([ENV["PDFCPU"], "validate", "-m", "relaxed", str(p)])
            a = arl.get(str(p), {"status": "missing-from-report"})
            rows.append({"pick": label, "variant": v, "exists": 1, "qpdf_check_rc": rc_q,
                         "pdfcpu_strict_rc": rc_s, "pdfcpu_relaxed_rc": rc_r,
                         "arlington": a["status"], "arlington_failed_checks": a.get("failed_checks"),
                         "arlington_profile": a.get("profile"), "arlington_note": a.get("note"),
                         "pdfcpu_strict_note": next((ln for ln in reversed(txt_s.splitlines()) if ln.strip()), "")[:200]})
    out.parent.mkdir(parents=True, exist_ok=True)
    keys = sorted({k for r in rows for k in r}, key=lambda k: list(rows[-1]).index(k) if k in rows[-1] else 99)
    with open(out.with_suffix(".csv"), "w", newline="") as f:
        w = csv.DictWriter(f, keys)
        w.writeheader()
        w.writerows(rows)
    summary = {"kind": "smoke test, not a benchmark", "tools": versions(), "by_variant": {}}
    for v in ("original", "corrupted", "qpdf-repaired", "mutool-repaired"):
        rs = [r for r in rows if r["variant"] == v]
        ex = [r for r in rs if r["exists"]]
        summary["by_variant"][v] = {
            "n": len(rs), "output_missing": len(rs) - len(ex),
            "qpdf_check_rc": dict(Counter(str(r["qpdf_check_rc"]) for r in ex)),
            "pdfcpu_strict_valid": sum(r["pdfcpu_strict_rc"] == 0 for r in ex),
            "pdfcpu_relaxed_valid": sum(r["pdfcpu_relaxed_rc"] == 0 for r in ex),
            "arlington": dict(Counter(r["arlington"] for r in ex))}
    rep = [r for r in rows if r["variant"].endswith("repaired") and r["exists"]]
    summary["repaired_disagreements"] = {
        "qpdf_clean_but_arlington_noncompliant": sum(r["qpdf_check_rc"] == 0 and r["arlington"] == "non-compliant" for r in rep),
        "arlington_compliant_but_pdfcpu_strict_invalid": sum(r["arlington"] == "compliant" and r["pdfcpu_strict_rc"] != 0 for r in rep),
        "pdfcpu_strict_valid_but_arlington_noncompliant": sum(r["pdfcpu_strict_rc"] == 0 and r["arlington"] == "non-compliant" for r in rep)}
    summary["inputs"] = [{"class": pk["class"], "corrupted": str(pk["corrupted"].relative_to(root)),
                          "corrupted_sha256": sha(pk["corrupted"])} for pk in picks]
    (out.parent / (out.name + ".summary.json")).write_text(json.dumps(summary, indent=1) + "\n")
    print(json.dumps({k: summary[k] for k in ("tools", "by_variant", "repaired_disagreements")}, indent=1))


if __name__ == "__main__":
    main()
