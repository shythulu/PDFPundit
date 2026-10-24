"""Google Cloud Document AI, the paper's engine: a stub until the user decides
D-014 (an account and about $10 per run, with the CI job's no-retention
conditions). Even this leg yields a REPDF-style number; its value is
calibrating the local engine against the paper's engine on the originals."""

from engines import Apparatus, EngineBlocked

NAME = "documentai"

BLOCKED = (
    "engine documentai is blocked on D-014 "
    "(a Document AI account and its cost are the user's decision)"
)


def check_installed() -> None:
    raise EngineBlocked(BLOCKED)


def prepare(tier: str, routes: set[str], threads: int) -> Apparatus:
    raise EngineBlocked(BLOCKED)
