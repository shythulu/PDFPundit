# Glyphs drawn via text operators, per page position, Save As vs Print to PDF; plus outlined glyph contours in Print.
# Each REPDF document has six pages, one language each (order checked below from the Save As text).
import glob, os, collections
from pypdf import PdfReader
from pypdf.generic import ContentStream, ByteStringObject, TextStringObject, ArrayObject

def page_stats(r, pg):
    fonts = pg["/Resources"].get_object().get("/Font", {})
    two = {k: fonts[k].get_object().get("/Subtype") == "/Type0" for k in fonts}
    ops = ContentStream(pg.get_contents(), r).operations
    glyphs = 0; cur2 = True; curves = 0; runs = 0; contours = 0; ms = 0
    def n(s, two_b):
        b = s.original_bytes if hasattr(s, "original_bytes") else bytes(s, "latin-1")
        return len(b) // 2 if two_b else len(b)
    for o, op in ops:
        if op == b"Tf": cur2 = two.get(o[0], True)
        elif op in (b"Tj", b"'", b'"'): glyphs += n(o[-1], cur2)
        elif op == b"TJ": glyphs += sum(n(x, cur2) for x in o[0] if isinstance(x, (ByteStringObject, TextStringObject)))
        elif op in (b"c", b"v", b"y"): curves += 1
        elif op == b"m": ms += 1
        elif op in (b"f", b"f*", b"F", b"B", b"B*", b"b", b"b*", b"n", b"S", b"s"):
            if op in (b"f", b"f*", b"F") and curves >= 3: runs += 1; contours += ms
            curves = 0; ms = 0
    return glyphs, contours

def lang(text):
    c = collections.Counter()
    for ch in text:
        o = ord(ch)
        if 0x0600 <= o <= 0x06FF or 0xFB50 <= o <= 0xFEFF: c["ar"] += 1
        elif 0x0900 <= o <= 0x097F: c["hi"] += 1
        elif 0x4E00 <= o <= 0x9FFF: c["zh"] += 1
        elif ch.isalpha() and o < 0x250: c["latin"] += 1
    return c.most_common(1)[0][0] if c else "?"

agg = collections.defaultdict(lambda: [0, 0, 0])
for sv in sorted(glob.glob("repdf-repo/original/saveas/*/*.pdf")):
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    rs, rp = PdfReader(sv, strict=False), PdfReader(pr, strict=False)
    for i, (ps, pp) in enumerate(zip(rs.pages, rp.pages)):
        L = lang(ps.extract_text() or "")
        key = L if L != "latin" else f"latin(p{i+1})"
        gs, _ = page_stats(rs, ps); gp, cont = page_stats(rp, pp)
        agg[key][0] += gs; agg[key][1] += gp; agg[key][2] += cont
print(f"{'page language':14s} {'SaveAs glyphs':>14s} {'Print glyphs':>13s} {'as text':>8s} {'outline contours':>17s}")
T = [0, 0, 0]
for k in sorted(agg):
    a, b, c = agg[k]; T = [T[0]+a, T[1]+b, T[2]+c]
    print(f"{k:14s} {a:14d} {b:13d} {b/a:8.1%} {c:17d}")
print(f"{'ALL':14s} {T[0]:14d} {T[1]:13d} {T[1]/T[0]:8.1%} {T[2]:17d}")
