#!/usr/bin/env python3
"""Stage 4 verifier (2026-10-05), TOOL-465: does fontations (read-fonts 0.45.0 / skrifa 0.48.0) read the 'name'
table family of the embedded TrueType programs in REPDF Print to PDF originals, and does it match OBS-0907
(fontTools, research/experiments/results/print_font_names.csv)?

    $S/venv/bin/python fontations_names.py /home/user/dfrc-korea/repdf <fontations_probe binary> <workdir> <out-prefix>

Extraction mirrors print_font_names.py: every page font of every original under original/print/, Type0 ->
DescendantFonts[0] -> FontDescriptor -> FontFile2 (decoded with pypdf). Each program is written to <workdir>,
read by the Rust probe (fontations_probe/, built with `cargo build --release -j2`), and the family is compared
with OBS-0907's family for the same (file, page, font tag, object) row and with fontTools on the same bytes.
Outputs <out-prefix>.tsv and <out-prefix>.summary.json.
"""
import csv
import io
import json
import subprocess
import sys
from collections import Counter
from pathlib import Path

import fontTools
import pypdf
from fontTools.ttLib import TTFont


def ft_family(data: bytes) -> str:
    tt = TTFont(io.BytesIO(data), lazy=True)
    n = tt["name"]
    rec = n.getName(1, 3, 1, 0x409) or n.getName(1, 1, 0, 0) or n.getDebugName(1)
    return str(rec) if rec is not None else ""


def main() -> None:
    root, probe, work, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4])
    repo = Path(__file__).resolve().parents[4]
    ref = {(r["file"], r["page"], r["tag"], r["obj"]): r["family"]
           for r in csv.DictReader(open(repo / "research/experiments/results/print_font_names.csv"))}
    work.mkdir(parents=True, exist_ok=True)
    rows = []
    for pdf in sorted((root / "original/print").rglob("*.pdf")):
        rel = str(pdf.relative_to(root))
        rd = pypdf.PdfReader(str(pdf))
        for pi, page in enumerate(rd.pages):
            fonts = (page.get("/Resources") or {}).get_object().get("/Font")
            if not fonts:
                continue
            for tag, ref_obj in fonts.get_object().items():
                font = ref_obj.get_object()
                if font.get("/Subtype") != "/Type0" or "/DescendantFonts" not in font:
                    continue
                fd = font["/DescendantFonts"][0].get_object().get("/FontDescriptor")
                fd = fd.get_object() if fd else None
                ff2 = fd.get("/FontFile2") if fd else None
                if ff2 is None:
                    continue
                data = ff2.get_object().get_data()
                obj = getattr(ref_obj, "idnum", "")
                p = work / f"f{len(rows):04d}.ttf"
                p.write_bytes(data)
                rows.append({"file": rel, "page": str(pi), "tag": str(tag), "obj": str(obj), "path": str(p),
                             "bytes": len(data), "fonttools": ft_family(data),
                             "obs0907": ref.get((rel, str(pi), str(tag), str(obj)), "<no row>")})
    res = {}
    for i in range(0, len(rows), 200):
        cp = subprocess.run([probe, *[r["path"] for r in rows[i:i + 200]]], capture_output=True, text=True, check=True)
        for line in cp.stdout.splitlines():
            f = line.split("\t")
            res[f[0]] = f
    for r in rows:
        f = res.get(r["path"], [r["path"], "missing", "", "", ""])
        r.update({"status": f[1], "read_fonts": f[2], "skrifa": f[3], "num_glyphs": f[4]})
    cols = ["file", "page", "tag", "obj", "bytes", "status", "read_fonts", "skrifa", "fonttools", "obs0907", "num_glyphs"]
    with open(out.with_suffix(".tsv"), "w", newline="") as fo:
        w = csv.DictWriter(fo, cols, delimiter="\t", extrasaction="ignore"); w.writeheader(); w.writerows(rows)
    s = {
        "date": "2026-10-05", "crates": {"read-fonts": "0.45.0", "skrifa": "0.48.0"},
        "python": {"fonttools": fontTools.version, "pypdf": pypdf.__version__},
        "files": len({r["file"] for r in rows}), "fontfile2_programs": len(rows),
        "parsed_ok": sum(r["status"] == "ok" for r in rows),
        "read_fonts_family_nonempty": sum(bool(r["read_fonts"]) for r in rows),
        "read_fonts_eq_skrifa": sum(r["read_fonts"] == r["skrifa"] for r in rows),
        "read_fonts_eq_fonttools_same_bytes": sum(r["read_fonts"] == r["fonttools"] for r in rows),
        "rows_matched_to_obs0907": sum(r["obs0907"] != "<no row>" for r in rows),
        "read_fonts_eq_obs0907": sum(r["read_fonts"] == r["obs0907"] for r in rows),
        "distinct_families": len({r["read_fonts"] for r in rows}),
        # rows OBS-0907's CSV has no line for: the same font object drawn again on a later page (OBS-0907 lists
        # each (file, object) once); they are still compared with fontTools and skrifa above
        "rows_not_in_obs0907": sum(r["obs0907"] == "<no row>" for r in rows),
        "mismatches": [r for r in rows if (r["obs0907"] != "<no row>" and r["read_fonts"] != r["obs0907"])
                       or r["read_fonts"] != r["skrifa"] or r["read_fonts"] != r["fonttools"]][:20],
        "top_families": dict(Counter(r["read_fonts"] for r in rows).most_common(8)),
    }
    out.with_suffix(".summary.json").write_text(json.dumps(s, indent=1, ensure_ascii=False))
    print(json.dumps({k: v for k, v in s.items() if k != "mismatches"}, ensure_ascii=False))


if __name__ == "__main__":
    main()
