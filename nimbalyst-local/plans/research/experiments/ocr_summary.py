# Summarise ocr_outlined.jsonl: pooled recall by page language, split by whether the
# Print to PDF page draws any glyphs as outlines (filled paths with >= 3 curve segments).
import json, glob, collections, sys
from pypdf import PdfReader
from pypdf.generic import ContentStream

def contours(path, i):
    r = PdfReader(path, strict=False)
    ops = ContentStream(r.pages[i].get_contents(), r).operations
    curves = ms = n = 0
    for _, op in ops:
        if op in (b"c", b"v", b"y"): curves += 1
        elif op == b"m": ms += 1
        elif op in (b"f", b"f*", b"F", b"B", b"B*", b"b", b"b*", b"n", b"S", b"s"):
            if op in (b"f", b"f*", b"F") and curves >= 3: n += ms
            curves = ms = 0
    return n

def chars_total(path, i):
    t = PdfReader(path, strict=False).pages[i].extract_text() or ""
    return sum(1 for ch in t.lower() if ch.isalnum())

src = sys.argv[1] if len(sys.argv) > 1 else "ocr/ocr_outlined.jsonl"
rows = [json.loads(l) for l in open(src)]
paths = {p.split("/")[-1]: p for p in glob.glob("repdf-repo/original/saveas/*/*.pdf")}
M = ("print_text", "print_ocr", "saveas_ocr")
agg = collections.defaultdict(lambda: collections.Counter())
for r in rows:
    sv = paths[r["doc"]]
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    k = (r["lang"], "outlined" if contours(pr, r["page"]) else "no outlines")
    nc = chars_total(sv, r["page"])
    a = agg[k]; a["pages"] += 1; a["n"] += r["n"]; a["nc"] += nc
    for m in M:
        a[m] += r[m] * r["n"]; a[m + "_chr"] += r[m + "_chr"] * nc

unit = {"en": "word", "zh": "char"}
print("lang  pages  print_to_pdf_pages   tokens   text-layer  OCR(print)  OCR(saveas)   | alnum-char recall: text-layer  OCR(print)  OCR(saveas)")
for k in sorted(agg):
    a = agg[k]
    w = "  ".join(f"{a[m]/a['n']:10.1%}" for m in M)
    c = "  ".join(f"{a[m+'_chr']/a['nc']:10.1%}" for m in M)
    print(f"{k[0]:4s} {a['pages']:5d}  {k[1]:18s} {a['n']:7d} {unit[k[0]]:>4s} {w}   | {c}")
