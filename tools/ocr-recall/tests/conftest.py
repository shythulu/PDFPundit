"""Shared helpers: a hand-written manifest and an engine that reads fixed line
lists, so no test downloads a model or runs OCR."""

import csv
from dataclasses import dataclass, field
from pathlib import Path

from engines import Apparatus

COLUMNS = ["file", "class", "producer", "base_doc", "role", "page", "lang", "png", "open_ok", "lcs_f1"]


def write_manifest(dir: Path, rows: list[dict]) -> Path:
    """Writes `manifest.csv` under `dir` and an empty file for every PNG named."""
    dir.mkdir(parents=True, exist_ok=True)
    path = dir / "manifest.csv"
    with path.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(COLUMNS)
        for r in rows:
            w.writerow([r.get(c, "") for c in COLUMNS])
            if r.get("png"):
                png = dir / r["png"]
                png.parent.mkdir(parents=True, exist_ok=True)
                png.write_bytes(b"")
    return path


def original(base: str, page: int, lang: str = "en") -> dict:
    return {
        "file": f"original/print/text/{base}(print).pdf",
        "producer": "print",
        "base_doc": base,
        "role": "original",
        "page": str(page),
        "lang": lang,
        "png": f"{base}(print)/p{page}.png",
        "open_ok": "1",
        "lcs_f1": "1/1",
    }


def repaired(base: str, cls: str, page: int | None, lang: str = "en", lcs_f1: str = "1/1") -> dict:
    stem = f"{base}(print)_{cls.lower()}"
    row = {
        "file": f"corrupted/print/text/{stem}.pdf",
        "class": cls,
        "producer": "print",
        "base_doc": base,
        "role": "repaired",
        "page": "" if page is None else str(page),
        "lang": lang if page is not None else "und",
        "png": "" if page is None else f"{stem}/p{page}.png",
        "open_ok": "0" if page is None else "1",
        "lcs_f1": "" if page is None else lcs_f1,
    }
    return row


@dataclass
class FixedRecogniser:
    texts: dict[str, list[str]]
    models: str
    calls: list[str] = field(default_factory=list)

    def ocr(self, png: Path) -> list[str]:
        key = f"{png.parent.name}/{png.name}"
        self.calls.append(key)
        return list(self.texts.get(key, []))


class FixedEngine:
    """An engine module stand-in: OCR text comes from a dict keyed by the
    manifest's `png` value."""

    def __init__(self, texts: dict[str, list[str]], name: str = "fixed", version: str = "1.0"):
        self.NAME = name
        self.version = version
        self.recogniser = FixedRecogniser(texts, models="fixed-det+fixed-rec")

    def check_installed(self) -> None:
        pass

    def prepare(self, tier: str, routes: set[str], threads: int) -> Apparatus:
        return Apparatus(
            name=self.NAME,
            version=self.version,
            detail=f"tier {tier}; fixed line lists",
            recognisers={r: self.recogniser for r in routes},
        )
