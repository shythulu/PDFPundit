"""Reads `target/corpus/ocr_inputs/manifest.csv`, written by the corpus `ocr`
mode (T-38a): one row per rendered page, or per page or file hayro could not
render (`open_ok = 0`, no PNG). Every row is checked; a bad row fails the run
before any OCR."""

import csv
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path, PurePosixPath

COLUMNS = ("file", "class", "producer", "base_doc", "role", "page", "lang", "png", "open_ok", "lcs_f1")

ROLES = ("original", "repaired")


class ManifestError(Exception):
    pass


@dataclass(frozen=True)
class Row:
    file: str
    cls: str
    producer: str
    base_doc: str
    role: str
    #: 0-based; `None` on the one row of a file hayro could not open.
    page: int | None
    lang: str
    #: Relative to the manifest, `/`-separated; `None` when `open_ok = 0`.
    png: str | None
    png_path: Path | None
    open_ok: bool
    #: The page's text-layer LCS-F1 (T-26), when the manifest has one.
    lcs_f1: Fraction | None

    @property
    def doc(self) -> tuple[str, str]:
        """The original this row belongs to."""
        return (self.producer, self.base_doc)


def read(path: Path) -> list[Row]:
    path = Path(path)
    try:
        with path.open(newline="", encoding="utf-8") as f:
            lines = list(csv.reader(f))
    except OSError as e:
        raise ManifestError(f"{path}: {e.strerror or e}") from None
    if not lines or tuple(lines[0]) != COLUMNS:
        raise ManifestError(f"{path}: the columns are not T-38a's ({','.join(COLUMNS)})")
    rows = []
    seen = set()
    for n, cells in enumerate(lines[1:], start=2):
        where = f"{path}:{n}"
        if len(cells) != len(COLUMNS):
            raise ManifestError(f"{where}: {len(cells)} fields, expected {len(COLUMNS)}")
        row = _row(dict(zip(COLUMNS, cells)), path.parent, where)
        key = (row.file, row.role, row.page)
        if key in seen:
            raise ManifestError(f"{where}: {row.file} page {row.page} appears twice")
        seen.add(key)
        rows.append(row)
    return rows


def _row(c: dict[str, str], root: Path, where: str) -> Row:
    role = c["role"]
    if role not in ROLES:
        raise ManifestError(f"{where}: role {role!r} is not original or repaired")
    if (role == "original") != (c["class"] == ""):
        raise ManifestError(f"{where}: an original has no class and a repair has one")
    if c["open_ok"] not in ("0", "1"):
        raise ManifestError(f"{where}: open_ok {c['open_ok']!r} is not 0 or 1")
    open_ok = c["open_ok"] == "1"
    png = c["png"] or None
    if open_ok != (png is not None):
        raise ManifestError(f"{where}: open_ok = {c['open_ok']} disagrees with png {c['png']!r}")
    png_path = None
    if png is not None:
        png_path = _png(png, root, where)
    page = None
    if c["page"]:
        if not c["page"].isdigit():
            raise ManifestError(f"{where}: page {c['page']!r} is not a number")
        page = int(c["page"])
    elif open_ok:
        raise ManifestError(f"{where}: a rendered row has no page")
    return Row(
        file=c["file"],
        cls=c["class"],
        producer=c["producer"],
        base_doc=c["base_doc"],
        role=role,
        page=page,
        lang=c["lang"],
        png=png,
        png_path=png_path,
        open_ok=open_ok,
        lcs_f1=_ratio(c["lcs_f1"], where),
    )


def _png(png: str, root: Path, where: str) -> Path:
    """Only PNGs: a PDF handed to PaddleOCR would be rasterised by PDFium
    (paddlex pulls pypdfium2), not hayro."""
    lower = png.lower()
    if lower.endswith(".pdf"):
        raise ManifestError(f"{where}: png {png!r} is a .pdf; only hayro's PNG rasters are OCR input")
    if not lower.endswith(".png"):
        raise ManifestError(f"{where}: png {png!r} is not a .png")
    rel = PurePosixPath(png)
    if rel.is_absolute() or ".." in rel.parts:
        raise ManifestError(f"{where}: png {png!r} is not inside the manifest's directory")
    path = root.joinpath(*rel.parts)
    if not path.is_file():
        raise ManifestError(f"{where}: png {png!r} does not exist")
    return path


def _ratio(s: str, where: str) -> Fraction | None:
    if not s:
        return None
    num, sep, den = s.partition("/")
    if not sep or not num.isdigit() or not den.isdigit() or int(den) == 0:
        raise ManifestError(f"{where}: lcs_f1 {s!r} is not num/den")
    return Fraction(int(num), int(den))
