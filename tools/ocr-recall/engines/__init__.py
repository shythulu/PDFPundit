"""OCR engines behind one interface (D-071: the apparatus is research-pending,
so the harness is engine-agnostic and the board's ruling is a flag).

An engine module provides:

- `NAME`: the engine's name as every header and CSV row prints it;
- `check_installed()`: returns, or raises `EngineMissing` (one-line install
  hint) or `EngineBlocked` (the decision the engine waits on);
- `prepare(tier, routes, threads) -> Apparatus`: everything a run needs
  before its first page (models fetched and verified), one `Recogniser` per
  route in `routes` (`latin`, `han`, `arabic`, `devanagari`).

A `Recogniser` has `models` (a label for the CSV) and `ocr(png) -> list[str]`,
the text lines of one page. The scorer never sees which engine produced them.
"""

import importlib
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import Protocol

NAMES = ("paddle", "tesseract", "documentai")


class EngineMissing(Exception):
    """The engine's packages are not installed; the message is the hint."""


class EngineBlocked(Exception):
    """The engine waits on a decision; the message names it."""


class Recogniser(Protocol):
    models: str

    def ocr(self, png: Path) -> list[str]: ...


@dataclass(frozen=True)
class Apparatus:
    """A prepared engine: its name and version (the D-063 label's), the
    header line naming tier, models and package versions, and the
    recogniser for each route."""

    name: str
    version: str
    detail: str
    recognisers: Mapping[str, Recogniser]


def load(name: str) -> ModuleType:
    if name not in NAMES:
        raise ValueError(f"unknown engine {name!r}")
    return importlib.import_module(f"engines.{name}")
