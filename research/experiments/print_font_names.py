#!/usr/bin/env python3
"""GAP-003 check: do REPDF 'Print to PDF' originals really lose their font names? Their BaseFont is a
stripped 'CIDFont+F<n>' (PR #16 found only CIDFont+F1..F22), but the embedded TrueType program
(FontFile2) carries its own 'name' table. If that table survives subsetting, name-free identification
(width vectors, outline hashes) is needed only when the font program itself is lost (C7/C8), not
whenever BaseFont is anonymous.

For every page font of every original under original/print/: BaseFont, whether a FontFile2 is
embedded, and the name-table family (nameID 1), full name (4) and PostScript name (6); also whether
the font has a ToUnicode CMap and a usable cmap table, and whether the cmap-derived GID -> character map
(the ground truth decipher_codes.py scores against) agrees with the ToUnicode CMap on every shared code.

    python3 print_font_names.py /home/user/dfrc-korea/repdf results/print_font_names
Needs pypdf and fontTools. Outputs <out>.csv (one row per distinct font object) and <out>.summary.json.
"""
import csv
import io
import json
import sys
from collections import Counter
from pathlib import Path

import fontTools
import pypdf
from fontTools.ttLib import TTFont

HEX = r"<([0-9A-Fa-f]+)>"


def tounicode_map(stream) -> dict[int, str]:
    """bfchar and bfrange entries of a ToUnicode CMap (single-target ranges and array ranges)."""
    import re
    data = stream.get_object().get_data().decode("latin-1")
    out = {}
    for block in re.findall(r"beginbfchar(.*?)endbfchar", data, re.S):
        for src, dst in re.findall(HEX + r"\s*" + HEX, block):
            out[int(src, 16)] = bytes.fromhex(dst).decode("utf-16-be", "replace")
    for block in re.findall(r"beginbfrange(.*?)endbfrange", data, re.S):
        for lo, hi, rest in re.findall(HEX + r"\s*" + HEX + r"\s*(\[[^\]]*\]|<[0-9A-Fa-f]+>)", block):
            lo, hi = int(lo, 16), int(hi, 16)
            if rest.startswith("["):
                for i, d in enumerate(re.findall(HEX, rest)):
                    out[lo + i] = bytes.fromhex(d).decode("utf-16-be", "replace")
            else:
                base = int(rest[1:-1], 16)
                for i in range(hi - lo + 1):
                    out[lo + i] = chr(base + i)
    return out


def cmap_truth(tt) -> dict[int, str]:
    """Same derivation as decipher_codes.truth_of: invert the best cmap, first code point per glyph."""
    out = {}
    for u, name in (tt.getBestCmap() or {}).items():
        try:
            out.setdefault(tt.getGlyphID(name), chr(u))
        except Exception:  # noqa: BLE001
            pass
    return out


def name_of(tt, nid):
    if "name" not in tt:
        return ""
    rec = tt["name"].getName(nid, 3, 1, 0x409) or tt["name"].getName(nid, 1, 0, 0) or tt["name"].getDebugName(nid)
    return str(rec) if rec else ""


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    rows = []
    files = sorted((corpus / "original" / "print").rglob("*.pdf"))
    for f in files:
        r = pypdf.PdfReader(str(f))
        seen = set()
        for pno, page in enumerate(r.pages):
            res = page.get("/Resources")
            fonts = res.get_object().get("/Font") if res else None
            if not fonts:
                continue
            for tag, ref in fonts.get_object().items():
                key = (ref.idnum, ref.generation) if hasattr(ref, "idnum") else (tag, pno)
                if key in seen:
                    continue
                seen.add(key)
                font = ref.get_object()
                base = str(font.get("/BaseFont", ""))
                desc_font = font
                if font.get("/Subtype") == "/Type0" and "/DescendantFonts" in font:
                    desc_font = font["/DescendantFonts"][0].get_object()
                fd = desc_font.get("/FontDescriptor")
                fd = fd.get_object() if fd else {}
                ff2 = fd.get("/FontFile2") if fd else None
                row = {"file": str(f.relative_to(corpus)), "page": pno, "tag": tag, "obj": key[0],
                       "basefont": base, "subtype": str(font.get("/Subtype", "")),
                       "has_tounicode": "/ToUnicode" in font, "fontfile2": ff2 is not None,
                       "name_table": False, "family": "", "full": "", "psname": "", "cmap_table": False,
                       "glyphs": "", "tu_codes": "", "shared_codes": "", "agree_codes": ""}
                if ff2 is not None:
                    try:
                        tt = TTFont(io.BytesIO(ff2.get_object().get_data()), lazy=True)
                        row["name_table"] = "name" in tt
                        row["family"], row["full"], row["psname"] = (name_of(tt, 1), name_of(tt, 4), name_of(tt, 6))
                        row["cmap_table"] = "cmap" in tt and tt.getBestCmap() is not None
                        row["glyphs"] = tt["maxp"].numGlyphs
                        if "/ToUnicode" in font:
                            tu, ct = tounicode_map(font["/ToUnicode"]), cmap_truth(tt)
                            shared = [g for g in tu if g in ct]
                            row["tu_codes"], row["shared_codes"] = len(tu), len(shared)
                            row["agree_codes"] = sum(tu[g] == ct[g] for g in shared)
                    except Exception as e:  # noqa: BLE001
                        row["family"] = f"ERROR {type(e).__name__}"
                rows.append(row)
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    ff = [x for x in rows if x["fontfile2"]]
    summary = {
        "files": len(files),
        "font_objects": len(rows),
        "basefont_prefix_CIDFont+F": sum(x["basefont"].startswith("/CIDFont+F") for x in rows),
        "distinct_basefonts": len({x["basefont"] for x in rows}),
        "with_tounicode": sum(x["has_tounicode"] for x in rows),
        "with_fontfile2": len(ff),
        "fontfile2_with_name_table": sum(x["name_table"] for x in ff),
        "fontfile2_with_family_name": sum(bool(x["family"]) and not x["family"].startswith("ERROR") for x in ff),
        "fontfile2_with_cmap_table": sum(x["cmap_table"] for x in ff),
        "parse_errors": sum(x["family"].startswith("ERROR") for x in ff),
        "tounicode_codes": sum(x["tu_codes"] or 0 for x in ff),
        "tounicode_codes_also_in_cmap_truth": sum(x["shared_codes"] or 0 for x in ff),
        "cmap_truth_agrees_with_tounicode": sum(x["agree_codes"] or 0 for x in ff),
        "fonts_with_any_disagreement": sum(1 for x in ff if x["shared_codes"] != "" and x["agree_codes"] != x["shared_codes"]),
        "distinct_families": dict(Counter(x["family"] for x in ff).most_common()),
        "font_objects_per_file_min_median_max": None,
        "versions": {"python": sys.version.split()[0], "pypdf": pypdf.__version__, "fonttools": fontTools.version},
    }
    per = sorted(Counter(x["file"] for x in rows).values())
    if per:
        summary["font_objects_per_file_min_median_max"] = [per[0], per[len(per) // 2], per[-1]]
    Path(f"{out}.summary.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False))
    print(json.dumps(summary, indent=1, ensure_ascii=False))


if __name__ == "__main__":
    main()
