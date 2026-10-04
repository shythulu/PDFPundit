# Which fonts get outlined? For every page, the fonts the Save As original shows text with
# (glyph counts per /BaseFont) against whether the matching Print to PDF page draws outlines.
import glob, collections, re
from pypdf import PdfReader
from pypdf.generic import ContentStream, ByteStringObject, TextStringObject

def fonts_used(r, pg):
    fonts = pg["/Resources"].get_object().get("/Font", {})
    info = {}
    for k in fonts:
        f = fonts[k].get_object()
        info[k] = (re.sub(r"^[A-Z]{6}\+", "", str(f.get("/BaseFont")).lstrip("/")), f.get("/Subtype") == "/Type0")
    used = collections.Counter(); cur = None
    for o, op in ContentStream(pg.get_contents(), r).operations:
        if op == b"Tf": cur = info.get(o[0])
        elif cur and op in (b"Tj", b"TJ", b"'", b'"'):
            items = o[0] if op == b"TJ" else [o[-1]]
            for x in items:
                if isinstance(x, (ByteStringObject, TextStringObject)):
                    b = x.original_bytes if hasattr(x, "original_bytes") else bytes(x, "latin-1")
                    used[cur[0]] += len(b) // 2 if cur[1] else len(b)
    return used

def contours(r, pg):
    curves = ms = n = 0
    for _, op in ContentStream(pg.get_contents(), r).operations:
        if op in (b"c", b"v", b"y"): curves += 1
        elif op == b"m": ms += 1
        elif op in (b"f", b"f*", b"F", b"B", b"B*", b"b", b"b*", b"n", b"S", b"s"):
            if op in (b"f", b"f*", b"F") and curves >= 3: n += ms
            curves = ms = 0
    return n

pages = []   # (doc, page, fonts used in Save As, outline contours in Print)
for sv in sorted(glob.glob("repdf-repo/original/saveas/*/*.pdf")):
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    rs, rp = PdfReader(sv, strict=False), PdfReader(pr, strict=False)
    for i, (ps, pp) in enumerate(zip(rs.pages, rp.pages)):
        pages.append((sv.split("/")[-1], i, fonts_used(rs, ps), contours(rp, pp)))

by_font = collections.defaultdict(lambda: [0, 0, 0])   # pages using, of which outlined, glyphs
for _, _, used, c in pages:
    for f, g in used.items():
        by_font[f][0] += 1; by_font[f][1] += c > 0; by_font[f][2] += g
print(f"{'font':40s} {'pages':>5s} {'outlined':>8s} {'glyphs':>7s}")
for f, (n, o, g) in sorted(by_font.items(), key=lambda kv: -kv[1][1] / kv[1][0]):
    print(f"{f:40s} {n:5d} {o:8d} {g:7d}")
hits = [f for f, (n, o, g) in by_font.items() if n >= 3 and o == n]   # ignore one-off fonts
out_pages = [p for p in pages if p[3] > 0]
unexplained = [p[:2] for p in out_pages if not any(f in p[2] for f in hits)]
docs_out = {p[0] for p in out_pages}
print(f"\nfonts outlined on every page that uses them: {hits}")
print(f"outlined pages: {len(out_pages)} in {len(docs_out)} documents; outlined pages not using those fonts: {unexplained}")
print(f"pages using those fonts without outlines: {[p[:2] for p in pages if any(f in p[2] for f in hits) and p[3] == 0]}")
