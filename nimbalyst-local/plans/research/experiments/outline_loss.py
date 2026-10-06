# Per document: non-whitespace characters extractable from the text layer, Print to PDF vs Save As,
# plus a count of filled paths that look like glyph runs (>=3 curve segments) in the Print to PDF file.
import glob, os, re, sys
from pypdf import PdfReader
from pypdf.generic import ContentStream

def chars(path):
    r = PdfReader(path, strict=False)
    return sum(len(re.sub(r"\s", "", p.extract_text() or "")) for p in r.pages)

def glyph_paths(path):
    r = PdfReader(path, strict=False)
    runs = subpaths = 0
    for p in r.pages:
        ops = ContentStream(p.get_contents(), r).operations
        curves = ms = 0
        for _, op in ops:
            if op in (b"c", b"v", b"y"): curves += 1
            elif op == b"m": ms += 1
            elif op in (b"f", b"f*", b"F", b"B", b"B*", b"b", b"b*", b"n", b"S", b"s"):
                if op not in (b"n", b"S", b"s") and curves >= 3:
                    runs += 1; subpaths += ms
                curves = ms = 0
    return runs, subpaths

root = sys.argv[1]
tot_s = tot_p = 0
print("doc saveas_chars print_chars ratio glyph_runs contours")
for sv in sorted(glob.glob(root + "/original/saveas/*/*.pdf")):
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    if not os.path.exists(pr): continue
    s, p = chars(sv), chars(pr)
    runs, sub = glyph_paths(pr)
    tot_s += s; tot_p += p
    print(os.path.basename(sv).replace("(saveas).pdf", ""), s, p, f"{p/s:.3f}" if s else "nan", runs, sub)
print("TOTAL", tot_s, tot_p, f"{tot_p/tot_s:.3f}")
