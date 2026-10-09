"""Per-script Tesseract (committee 005 C3 on PR #19): a stub until the PR #19
board rules on D-071. When it does, this module implements the interface in
`engines/__init__.py` and nothing else in the harness changes."""

from engines import Apparatus, EngineBlocked

NAME = "tesseract"

BLOCKED = (
    "engine tesseract is blocked on D-071 "
    "(which OCR apparatus feeds the REPDF-style number is the PR #19 board's ruling)"
)


def check_installed() -> None:
    raise EngineBlocked(BLOCKED)


def prepare(tier: str, routes: set[str], threads: int) -> Apparatus:
    raise EngineBlocked(BLOCKED)
