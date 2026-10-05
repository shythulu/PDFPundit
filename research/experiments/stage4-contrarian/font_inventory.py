#!/usr/bin/env python3
"""What do REPDF's font classes remove, and do grouped (per-base-document) splits leak font mappings?

A. Inventory. For every original and every C6 (_font_mapping_loss), C7 (_remove_fonts) and C8 (_remove_unicode_fonts)
   file, over every indirect object (C6-C8 leave the xref intact; unreferenced objects are counted too): font
   dictionaries, fonts with /ToUnicode, font descriptors that reference an embedded program (/FontFile, /FontFile2,
   /FontFile3), streams that look like font programs (/Length1, or /Subtype Type1C, CIDFontType0C, OpenType) and
   streams whose first 4 KiB contain 'begincmap' (CMaps, referenced or orphaned).
B. Leakage. For each Save As / Print original, parse every ToUnicode CMap (bfchar and bfrange) into (font name without
   the subset tag, code) -> Unicode. Print to PDF names its fonts CIDFont+F<n> (a per-document index, OBS-0907), so
   its result keys on anonymous names and measures code reuse, not font identity. Leave one base document out: what share of its (font, code) entries also occur in
   the other 49 documents' maps of the same producer, and what share of those agree. A selector or decipherer fitted on
   the other documents would see these mappings, so a split by base document does not hide them.

    python3 font_inventory.py <repdf_root> <out_prefix>
writes <out_prefix>.files.csv and <out_prefix>.summary.json
"""
import csv
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

import pypdf
from pypdf.generic import ArrayObject, DictionaryObject, IndirectObject, NameObject

CLASSES = {"_font_mapping_loss": "C6", "_remove_fonts": "C7", "_remove_unicode_fonts": "C8"}


def all_objects(reader):
    nums = set()
    for gen in reader.xref.values():
        nums.update(gen.keys())
    nums.update(getattr(reader, "xref_objStm", {}).keys())
    for n in sorted(nums):
        try:
            o = reader.get_object(n)
        except Exception:
            continue
        if o is not None:
            yield n, o


def has_program_ref(o, depth=0) -> bool:
    """True if this dictionary, or a direct (inline) dictionary/array inside it, references an embedded program.
    Print to PDF writes the FontDescriptor inline in the descendant font, so top-level dicts alone miss it."""
    if depth > 4:
        return False
    if isinstance(o, DictionaryObject):
        if any(k in o for k in ("/FontFile", "/FontFile2", "/FontFile3")):
            return True
        return any(has_program_ref(v, depth + 1) for v in o.values() if not isinstance(v, IndirectObject))
    if isinstance(o, ArrayObject):
        return any(has_program_ref(v, depth + 1) for v in o if not isinstance(v, IndirectObject))
    return False


def hexcode(s):
    return int(s, 16), len(s)


def parse_cmap(data: bytes):
    txt = data.decode("latin-1", "replace")
    out = {}
    for blk in re.findall(r"beginbfchar(.*?)endbfchar", txt, re.S):
        for a, b in re.findall(r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]*)>", blk):
            out[(a.upper().zfill(len(a)))] = b.upper()
    for blk in re.findall(r"beginbfrange(.*?)endbfrange", txt, re.S):
        for m in re.finditer(r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>\s*(<[0-9A-Fa-f]*>|\[[^\]]*\])", blk):
            lo, w = hexcode(m.group(1))
            hi, _ = hexcode(m.group(2))
            dst = m.group(3)
            if hi - lo > 65535:
                continue
            if dst.startswith("["):
                items = re.findall(r"<([0-9A-Fa-f]*)>", dst)
                for i, it in enumerate(items[: hi - lo + 1]):
                    out[format(lo + i, f"0{w}X")] = it.upper()
            else:
                base = dst[1:-1]
                if not base:
                    continue
                bv = int(base, 16)
                for i in range(hi - lo + 1):
                    out[format(lo + i, f"0{w}X")] = format(bv + i, f"0{len(base)}X")
    return out


def strip_tag(name: str) -> str:
    return re.sub(r"^[A-Z]{6}\+", "", name.lstrip("/"))


def inspect(path: Path, want_maps: bool):
    r = pypdf.PdfReader(str(path), strict=False)
    fonts = tounicode = programs = cmap_streams = program_streams = 0
    maps = defaultdict(dict)
    for _, o in all_objects(r):
        if not isinstance(o, DictionaryObject):
            continue
        if "/Length1" in o or o.get("/Subtype") in ("/Type1C", "/CIDFontType0C", "/OpenType"):
            program_streams += 1
        elif hasattr(o, "get_data"):
            try:
                if b"begincmap" in o.get_data()[:4096]:
                    cmap_streams += 1
            except Exception:
                pass
        t = o.get("/Type")
        if t == "/Font":
            fonts += 1
            if "/ToUnicode" in o:
                tounicode += 1
                if want_maps:
                    try:
                        data = o["/ToUnicode"].get_object().get_data()
                        maps[strip_tag(str(o.get("/BaseFont", "?")))].update(parse_cmap(data))
                    except Exception:
                        pass
        if t != "/Font" and any(k in o for k in ("/FontFile", "/FontFile2", "/FontFile3")):
            programs += 1  # an indirect FontDescriptor (Save As)
        elif t == "/Font" and has_program_ref(o):
            programs += 1  # a font whose FontDescriptor is inline (Print to PDF descendant fonts)
    return fonts, tounicode, programs, cmap_streams, program_streams, maps


def main():
    root, out = Path(sys.argv[1]), sys.argv[2]
    rows = []
    maps = {}  # (producer, base) -> {font: {code: uni}}
    for prod in ("saveas", "print"):
        for orig in sorted((root / "original" / prod).glob("*/*.pdf")):
            base = orig.stem
            f, tu, pr, cs, ps, m = inspect(orig, True)
            maps[(prod, base)] = m
            rows.append({"producer": prod, "base": base, "class": "orig", "fonts": f, "tounicode": tu, "programs": pr,
                         "cmap_streams": cs, "program_streams": ps})
            for suf, cls in CLASSES.items():
                cands = list((root / "corrupted" / prod).glob(f"*/{base}{suf}.pdf"))
                if not cands:
                    continue
                f, tu, pr, cs, ps, _ = inspect(cands[0], False)
                rows.append({"producer": prod, "base": base, "class": cls, "fonts": f, "tounicode": tu, "programs": pr,
                             "cmap_streams": cs, "program_streams": ps})
    with open(out + ".files.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    summ = {"pypdf": pypdf.__version__, "inventory": {}, "leakage": {}}
    for prod in ("saveas", "print"):
        for cls in ("orig", "C6", "C7", "C8"):
            sel = [r for r in rows if r["producer"] == prod and r["class"] == cls]
            if not sel:
                continue
            summ["inventory"][f"{prod}|{cls}"] = {
                "files": len(sel),
                "fonts_total": sum(r["fonts"] for r in sel),
                "tounicode_total": sum(r["tounicode"] for r in sel),
                "programs_total": sum(r["programs"] for r in sel),
                "cmap_streams_total": sum(r["cmap_streams"] for r in sel),
                "program_streams_total": sum(r["program_streams"] for r in sel),
                "files_with_any_tounicode": sum(1 for r in sel if r["tounicode"] > 0),
                "files_with_any_program": sum(1 for r in sel if r["programs"] > 0),
            }
        bases = sorted(b for (p, b) in maps if p == prod)
        entries = covered = agree = 0
        docs_with_cover = 0
        per_doc = []
        for b in bases:
            mine = maps[(prod, b)]
            others = defaultdict(dict)
            for b2 in bases:
                if b2 == b:
                    continue
                for font, m in maps[(prod, b2)].items():
                    for c, u in m.items():
                        others[font].setdefault(c, u)
            e = cov = ag = 0
            for font, m in mine.items():
                for c, u in m.items():
                    e += 1
                    if c in others.get(font, {}):
                        cov += 1
                        ag += others[font][c] == u
            entries += e
            covered += cov
            agree += ag
            if e:
                per_doc.append(cov / e)
                docs_with_cover += cov > 0
        per_doc.sort()
        summ["leakage"][prod] = {
            "documents": len(bases),
            "documents_with_tounicode_entries": len(per_doc),
            "tounicode_entries": entries,
            "covered_by_other_documents_same_font_name": covered,
            "covered_share": round(covered / entries, 4) if entries else None,
            "agree_share_of_covered": round(agree / covered, 4) if covered else None,
            "per_document_covered_share_min_median_max": [round(per_doc[0], 4), round(per_doc[len(per_doc) // 2], 4),
                                                          round(per_doc[-1], 4)] if per_doc else None,
            "documents_with_any_covered_entry": docs_with_cover,
        }
    Path(out + ".summary.json").write_text(json.dumps(summ, indent=1))
    print(json.dumps(summ, indent=1))


if __name__ == "__main__":
    main()
