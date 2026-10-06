# How much outlined text does OCR recover? REPDF originals, English and Chinese pages.
# Ground truth = Save As text layer (never outlined). Compared per page:
#   print_text : Print to PDF text layer (pypdf)          -> what a text-layer metric sees
#   print_ocr  : OCR of the rendered Print to PDF page    -> what REPDF's OCR metric sees
#   saveas_ocr : OCR of the rendered Save As page         -> OCR engine error alone
# OCR = PaddleOCR PP-OCRv4 (det + ch rec) ONNX models via RapidOCR; render = PDFium at 200 dpi.
import glob, os, re, sys, collections, json
import pypdfium2 as pdfium
from pypdf import PdfReader
from rapidocr_onnxruntime import RapidOCR

ocr = RapidOCR()
DPI = 200

def lang(text):
    c = collections.Counter()
    for ch in text:
        o = ord(ch)
        if 0x0600 <= o <= 0x06FF or 0xFB50 <= o <= 0xFEFF: c["ar"] += 1
        elif 0x0900 <= o <= 0x097F: c["hi"] += 1
        elif 0x4E00 <= o <= 0x9FFF: c["zh"] += 1
        elif ch.isalpha() and o < 0x250: c["latin"] += 1
    return c.most_common(1)[0][0] if c else "?"

def toks(text, lg):
    if lg == "zh":
        return collections.Counter(ch for ch in text if 0x4E00 <= ord(ch) <= 0x9FFF)
    return collections.Counter(re.findall(r"[a-z0-9]+", text.lower()))

def chars(text):
    return collections.Counter(ch for ch in text.lower() if ch.isalnum())

def recall(gt, got):
    tot = sum(gt.values())
    return sum(min(v, got[k]) for k, v in gt.items()) / tot if tot else float("nan"), tot

def ocr_page(path, i):
    pdf = pdfium.PdfDocument(path)
    img = pdf[i].render(scale=DPI / 72).to_numpy()
    res, _ = ocr(img)
    return "\n".join(r[1] for r in (res or []))

docs = sorted(glob.glob("repdf-repo/original/saveas/*/*.pdf"))
if len(sys.argv) > 1: docs = docs[: int(sys.argv[1])]
os.makedirs("ocr", exist_ok=True)
out = open("ocr/ocr_outlined.jsonl", "w")
for sv in docs:
    pr = sv.replace("/saveas/", "/print/").replace("(saveas)", "(print)")
    rs, rp = PdfReader(sv, strict=False), PdfReader(pr, strict=False)
    for i, (ps, pp) in enumerate(zip(rs.pages, rp.pages)):
        gt_text = ps.extract_text() or ""
        lg = lang(gt_text)
        if lg not in ("latin", "zh") or i > 1: continue   # page 0 = en, page 1 = zh
        lg = "en" if lg == "latin" else lg
        pt = pp.extract_text() or ""
        po, so = ocr_page(pr, i), ocr_page(sv, i)
        gt, gc = toks(gt_text, lg), chars(gt_text)
        row = {"doc": sv.split("/")[-1], "page": i, "lang": lg}
        for name, t in (("print_text", pt), ("print_ocr", po), ("saveas_ocr", so)):
            row[name], row["n"] = recall(gt, toks(t, lg))
            row[name + "_chr"], _ = recall(gc, chars(t))
        out.write(json.dumps(row, ensure_ascii=False) + "\n"); out.flush()
        print(row, flush=True)
