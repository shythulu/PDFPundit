"""The REPDF-style OCR word recall (D-068) and its sensitivity columns.

Per page, the original's OCR text is the ground truth for each repaired
variant's OCR text of the same page. Both are normalised as T-25 does (NFC,
then each character lower-cased as the casefold, whitespace runs collapsed)
and tokenised on whitespace, or one token per character on `zh` pages. Then

    ocr_word_recall = sum_w min(c_orig(w), c_rep(w)) / sum_w c_orig(w)

a clipped bag, so repeated words cannot push it past 1.0. Beside it:
`ocr_word_recall_nopunct` (Unicode punctuation removed from every token
first) and `ocr_char_recall` (the same clipped bag over non-whitespace
characters), because these choices alone move the number by up to 30 points
on one pair of OCR outputs (eng-r3-fr1 §6). `text_visual_gap` is the page's
text-layer LCS-F1 minus `ocr_word_recall`.

Every value is an exact `Fraction`; nothing here depends on the engine.
"""

import unicodedata
from collections import Counter
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from fractions import Fraction

import manifest

#: A page's script from its manifest `lang` (D-069): routes the page to a
#: model pair and keys the per-script table.
SCRIPTS = {"en": "latin", "fr": "latin", "es": "latin", "zh": "han", "ar": "arabic", "hi": "devanagari"}

SCRIPT_ORDER = ("latin", "han", "arabic", "devanagari", "und")


def script_of(lang: str) -> str:
    """`und` (or any label T-26 does not know) stays `und`."""
    return SCRIPTS.get(lang, "und")


def route_of(script: str) -> str:
    """The model route of a script: an `und` page is read as Latin, whose
    PP-OCRv6 models also read Han."""
    return "latin" if script == "und" else script


def normalize(text: str) -> str:
    """NFC, then per-character lower case, then whitespace runs collapsed to
    one space and trimmed: T-25's `normalize` (TD §8)."""
    folded = "".join(c.lower() for c in unicodedata.normalize("NFC", text))
    return " ".join(folded.split())


def tokens(text: str, lang: str) -> list[str]:
    """Whitespace tokens of normalised `text`; one per character on `zh`."""
    if lang == "zh":
        return [c for c in text if not c.isspace()]
    return text.split()


def _nopunct(toks: Iterable[str]) -> list[str]:
    out = []
    for t in toks:
        kept = "".join(c for c in t if not unicodedata.category(c).startswith("P"))
        if kept:
            out.append(kept)
    return out


def clipped_recall(orig: Sequence[str], rep: Sequence[str]) -> Fraction | None:
    """`sum min(c_orig, c_rep) / sum c_orig`; `None` when the original is empty."""
    if not orig:
        return None
    have = Counter(rep)
    shared = sum(min(n, have[w]) for w, n in Counter(orig).items())
    return Fraction(shared, len(orig))


@dataclass(frozen=True)
class PageScore:
    words_orig: int
    words_rep: int
    word: Fraction | None
    word_nopunct: Fraction | None
    char: Fraction | None


def score_page(orig_lines: Sequence[str], rep_lines: Sequence[str], lang: str) -> PageScore:
    """One repaired page against its original. An original with no words has
    no ground truth: every recall is `None`."""
    orig = normalize("\n".join(orig_lines))
    rep = normalize("\n".join(rep_lines))
    ot, rt = tokens(orig, lang), tokens(rep, lang)
    oc = [c for c in orig if not c.isspace()]
    rc = [c for c in rep if not c.isspace()]
    word = clipped_recall(ot, rt)
    return PageScore(
        words_orig=len(ot),
        words_rep=len(rt),
        word=word,
        word_nopunct=clipped_recall(_nopunct(ot), _nopunct(rt)) if word is not None else None,
        char=clipped_recall(oc, rc) if word is not None else None,
    )


@dataclass(frozen=True)
class Result:
    """One page of one repaired file, scored against its original's page."""

    file: str
    cls: str
    producer: str
    base_doc: str
    page: int
    lang: str
    #: The repaired page had a raster (and so OCR text).
    open_ok: bool
    #: How many pages of this repaired file were OCRed.
    ocr_pages: int
    words_orig: int
    words_rep: int
    word: Fraction | None
    word_nopunct: Fraction | None
    char: Fraction | None
    lcs_f1: Fraction | None
    gap: Fraction | None

    @property
    def script(self) -> str:
        return script_of(self.lang)


def pair(rows: Sequence[manifest.Row], texts: Mapping[str, Sequence[str]]) -> list[Result]:
    """Scores every page of each repaired file's original, in manifest order.

    `texts` maps a row's `png` to its OCR lines. A repaired page that is
    missing, or has no raster, scores against empty text (0). Pages a repair
    has beyond its original's count have no ground truth and are not scored.
    """
    originals: dict[tuple[str, str], dict[int, manifest.Row]] = {}
    repairs: dict[str, list[manifest.Row]] = {}
    for r in rows:
        if r.role == "original":
            if r.page is not None:
                originals.setdefault(r.doc, {})[r.page] = r
            else:
                originals.setdefault(r.doc, {})
        else:
            repairs.setdefault(r.file, []).append(r)

    out = []
    for file, rep_rows in repairs.items():
        first = rep_rows[0]
        if first.doc not in originals:
            raise manifest.ManifestError(f"{file}: no original rows for {first.producer}/{first.base_doc}")
        orig_pages = originals[first.doc]
        by_page = {r.page: r for r in rep_rows if r.page is not None}
        ocr_pages = sum(1 for r in rep_rows if r.png is not None)
        for page in sorted(orig_pages):
            o = orig_pages[page]
            rep = by_page.get(page)
            rep_text = texts.get(rep.png, ()) if rep is not None and rep.png is not None else ()
            s = score_page(texts.get(o.png, ()) if o.png is not None else (), rep_text, o.lang)
            lcs_f1 = rep.lcs_f1 if rep is not None else None
            gap = lcs_f1 - s.word if lcs_f1 is not None and s.word is not None else None
            out.append(
                Result(
                    file=file,
                    cls=first.cls,
                    producer=first.producer,
                    base_doc=first.base_doc,
                    page=page,
                    lang=o.lang,
                    open_ok=rep is not None and rep.png is not None,
                    ocr_pages=ocr_pages,
                    words_orig=s.words_orig,
                    words_rep=s.words_rep,
                    word=s.word,
                    word_nopunct=s.word_nopunct,
                    char=s.char,
                    lcs_f1=lcs_f1,
                    gap=gap,
                )
            )
    return out


@dataclass(frozen=True)
class Means:
    #: Pages with ground truth (an original with at least one word).
    pages: int
    word: Fraction | None
    word_nopunct: Fraction | None
    char: Fraction | None
    gap: Fraction | None


def _mean(values: Iterable[Fraction | None]) -> Fraction | None:
    vs = [v for v in values if v is not None]
    return sum(vs, Fraction(0)) / len(vs) if vs else None


def _class_key(cls: str) -> tuple[int, str]:
    digits = cls[1:]
    return (int(digits), cls) if cls[:1] == "C" and digits.isdigit() else (1 << 30, cls)


def aggregate(results: Sequence[Result], by_script: bool) -> list[tuple[tuple[str, ...], Means]]:
    """Per-page means per class (and script), classes in C1..C10 order."""
    groups: dict[tuple[str, ...], list[Result]] = {}
    for r in results:
        key = (r.cls, r.script) if by_script else (r.cls,)
        groups.setdefault(key, []).append(r)

    def order(key: tuple[str, ...]):
        script = SCRIPT_ORDER.index(key[1]) if by_script else 0
        return (_class_key(key[0]), script)

    return [
        (
            key,
            Means(
                pages=sum(1 for r in rs if r.word is not None),
                word=_mean(r.word for r in rs),
                word_nopunct=_mean(r.word_nopunct for r in rs),
                char=_mean(r.char for r in rs),
                gap=_mean(r.gap for r in rs),
            ),
        )
        for key, rs in sorted(groups.items(), key=lambda kv: order(kv[0]))
    ]


def fixed(x: Fraction, places: int) -> str:
    """`x` to `places` decimals, rounded half to even, exactly."""
    q = round(x * 10**places)
    sign = "-" if q < 0 else ""
    digits = str(abs(q)).rjust(places + 1, "0")
    return f"{sign}{digits[:-places]}.{digits[-places:]}"
