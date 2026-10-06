# Embedding permission (OS/2 fsType) and outline table of every embedded font program in the REPDF originals,
# per Save As /BaseFont, against the pages that show text with the font and how many of those pages draw
# outlines in the Print to PDF file. Answers "is outlining a licence effect?". Needs pypdf and fontTools.
import glob, io, re, sys, collections
from pypdf import PdfReader
from pypdf.generic import ContentStream
from fontTools.ttLib import TTFont

root = sys.argv[1] if len(sys.argv) > 1 else "repdf-repo"
FSTYPE = {0: "installable", 1: "bit0 (reserved)", 2: "restricted", 4: "preview&print", 8: "editable"}
strip = lambda n: re.sub(r"^[A-Z]{6}\+", "", str(n).lstrip("/"))

def program(fontdict):
    """(fsType, outline table, FontFile key) of the embedded program behind a font dict, or None."""
    f = fontdict
    if f.get("/Subtype") == "/Type0": f = f["/DescendantFonts"][0].get_object()
    fd = f.get("/FontDescriptor")
    if fd is None: return None
    fd = fd.get_object()
    key = next((k for k in ("/FontFile2", "/FontFile3", "/FontFile") if k in fd), None)
    if key is None: return None
    t = TTFont(io.BytesIO(fd[key].get_object().get_data()), lazy=True)
    return (t["OS/2"].fsType if "OS/2" in t else None, "glyf" if "glyf" in t else ("CFF" if "CFF " in t else "?"), key)

def used(r, pg):
    """stripped /BaseFont names that show at least one glyph on the page."""
    fonts = pg["/Resources"].get_object().get("/Font", {})
    names = {k: strip(fonts[k].get_object().get("/BaseFont")) for k in fonts}
    out = set(); cur = None
    for o, op in ContentStream(pg.get_contents(), r).operations:
        if op == b"Tf": cur = names.get(o[0])
        elif cur and op in (b"Tj", b"TJ", b"'", b'"'): out.add(cur)
    return out

def outlined(r, pg):
    curves = ms = n = 0
    for _, op in ContentStream(pg.get_contents(), r).operations:
        if op in (b"c", b"v", b"y"): curves += 1
        elif op == b"m": ms += 1
        elif op in (b"f", b"f*", b"F", b"B", b"B*", b"b", b"b*", b"n", b"S", b"s"):
            if op in (b"f", b"f*", b"F") and curves >= 3: n += ms
            curves = ms = 0
    return n > 0

info = {}                                        # Save As font name -> (fsType, table, FontFile key)
use = collections.defaultdict(lambda: [0, 0])    # name -> [Print pages whose Save As twin shows it, of which outlined]
print_programs = collections.Counter()           # Print to PDF: (FontFile key, table, fsType) -> font dicts
for sv in sorted(glob.glob(root + "/original/saveas/*/*.pdf")):
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    rs, rp = PdfReader(sv, strict=False), PdfReader(pr, strict=False)
    for ps, pp in zip(rs.pages, rp.pages):
        out = outlined(rp, pp)
        fonts = ps["/Resources"].get_object().get("/Font", {})
        for name in used(rs, ps):
            use[name][0] += 1; use[name][1] += out
        for k in fonts:
            f = fonts[k].get_object(); name = strip(f.get("/BaseFont"))
            if name not in info: info[name] = program(f) or (None, "not embedded", None)
        for k in pp["/Resources"].get_object().get("/Font", {}).values():
            p = program(k.get_object())
            if p: print_programs[p] += 1

print(f"{'Save As font':40s} {'fsType':>6s} {'meaning':15s} {'table':5s} {'pages':>5s} {'outlined':>8s}")
for name in sorted(info, key=lambda n: (-(use[n][1] / max(use[n][0], 1)), -use[n][0], n)):
    fst, tbl, key = info[name]
    print(f"{name:40s} {str(fst):>6s} {FSTYPE.get(fst, '?'):15s} {tbl:5s} {use[name][0]:5d} {use[name][1]:8d}")
print("\nPrint to PDF embedded programs, (FontFile key, outline table, fsType) -> font dicts:")
for k in sorted(print_programs, key=str): print("  ", k, print_programs[k])
print("\nSave As fonts by fsType:", dict(collections.Counter(FSTYPE.get(v[0], str(v[0])) for v in info.values())))
print("fonts shown on >= 3 pages and outlined on every one of them:",
      [n for n in info if use[n][0] >= 3 and use[n][1] == use[n][0]])
print("fonts with fsType 4 (preview & print):", [n for n in info if info[n][0] == 4])
