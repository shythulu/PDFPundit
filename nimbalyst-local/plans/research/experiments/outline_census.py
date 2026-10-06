# Census of how text is drawn in the REPDF originals: text operators vs vector paths vs Type3 vs images.
import sys, glob, collections
from pypdf import PdfReader
from pypdf.generic import ContentStream, IndirectObject

TEXT_SHOW = {b"Tj", b"TJ", b"'", b'"'}
SEG = {b"m", b"l", b"c", b"v", b"y", b"re"}
FILL = {b"f", b"F", b"f*", b"B", b"B*", b"b", b"b*"}

def walk(res, content, reader, acc, depth=0):
    try:
        ops = ContentStream(content, reader).operations
    except Exception as e:
        acc["parse_err"] += 1
        return
    for operands, op in ops:
        if op in TEXT_SHOW: acc["text_ops"] += 1
        elif op in SEG: acc["seg_ops"] += 1
        elif op == b"c" or op in (b"v", b"y"): pass
        elif op in FILL: acc["fills"] += 1
        elif op == b"Do" and res is not None and depth < 6:
            xo = res.get("/XObject")
            if xo is None: continue
            xo = xo.get_object()
            name = operands[0]
            if name not in xo: continue
            x = xo[name].get_object()
            st = x.get("/Subtype")
            if st == "/Image":
                acc["images"] += 1
            elif st == "/Form":
                acc["forms"] += 1
                walk(x.get("/Resources", res).get_object() if x.get("/Resources") else res, x, reader, acc, depth + 1)
    # count curve segments separately
    acc["curves"] += sum(1 for _, op in ops if op in (b"c", b"v", b"y"))

def fonts_of(res, out, seen):
    if res is None: return
    res = res.get_object()
    f = res.get("/Font")
    if f:
        for k, v in f.get_object().items():
            v = v.get_object()
            out[(str(v.get("/Subtype")), str(v.get("/BaseFont")))] += 1
    xo = res.get("/XObject")
    if xo:
        for k, v in xo.get_object().items():
            if isinstance(v, IndirectObject):
                if v.idnum in seen: continue
                seen.add(v.idnum)
            v = v.get_object()
            if v.get("/Subtype") == "/Form" and v.get("/Resources"):
                fonts_of(v["/Resources"], out, seen)

rows = []
for path in sorted(glob.glob(sys.argv[1] + "/original/*/*/*.pdf")):
    r = PdfReader(path, strict=False)
    acc = collections.Counter(); fonts = collections.Counter()
    for p in r.pages:
        res = p.get("/Resources")
        res = res.get_object() if res else None
        c = p.get_contents()
        if c is not None: walk(res, c, r, acc)
        fonts_of(res, fonts, set())
    t3 = sum(n for (st, bf), n in fonts.items() if st == "/Type3")
    rows.append((path.split("original/")[1], len(r.pages), acc["text_ops"], acc["seg_ops"], acc["curves"], acc["fills"], acc["images"], acc["forms"], t3, acc["parse_err"]))

print("file pages text_ops seg_ops curves fills images forms type3 parse_err")
for row in rows: print(*row)
