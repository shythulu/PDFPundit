"""Shared helpers for the research knowledge base (paths, JSONL I/O, text normalization)."""

from __future__ import annotations

import json
import re
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]          # research/
REPO = ROOT.parent
CACHE = ROOT / "cache"
FULLTEXT = CACHE / "fulltext"                        # <sha256>.txt, pages separated by \f
PDFCACHE = CACHE / "pdf"                             # <sha256>.pdf
CROSSREF = CACHE / "crossref"                        # <doi-slug>.json
SCHEMAS = ROOT / "schemas"

# Record type -> (registry file, id prefix, schema file)
REGISTRIES = {
    "source":     (ROOT / "sources" / "registry.jsonl",   "SRC",  "source.schema.json"),
    "screening":  (ROOT / "sources" / "screening.jsonl",  None,   "screening.schema.json"),
    "claim":      (ROOT / "sources" / "claims.jsonl",     "CLM",  "claim.schema.json"),
    "gap":        (ROOT / "gaps" / "gaps.jsonl",          "GAP",  "gap.schema.json"),
    "hypothesis": (ROOT / "hypotheses" / "hypotheses.jsonl", "HYP", "hypothesis.schema.json"),
    "damage":     (ROOT / "damage" / "classes.jsonl",     "DMG",  "damage.schema.json"),
    "tool":       (ROOT / "tooling" / "ledger.jsonl",     "TOOL", "tool.schema.json"),
    "search":     (ROOT / "search-log.jsonl",             "SRCH", "search.schema.json"),
}


def load_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    out = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError as e:
            raise SystemExit(f"{path}:{n}: invalid JSON: {e}") from e
    return out


def write_jsonl(path: Path, records: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = "".join(json.dumps(r, ensure_ascii=False, sort_keys=False) + "\n" for r in records)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(text, encoding="utf-8")
    tmp.replace(path)


_QUOTES = str.maketrans({"‘": "'", "’": "'", "‚": "'", "‛": "'",
                         "“": '"', "”": '"', "„": '"', "«": '"', "»": '"',
                         "‐": "-", "‑": "-", "‒": "-", "–": "-", "—": "-",
                         "−": "-", " ": " "})


def normalize(text: str) -> str:
    """Normalize for quote matching: NFKC, drop soft hyphens, de-hyphenate line breaks,
    unify quotes/dashes, casefold, collapse whitespace."""
    t = unicodedata.normalize("NFKC", text).replace("­", "")
    t = t.translate(_QUOTES)
    t = re.sub(r"(\w)-\s*\n\s*(\w)", r"\1\2", t)      # "hier-\narchical" -> "hierarchical"
    t = t.casefold()
    return re.sub(r"\s+", " ", t).strip()


def doi_slug(doi: str) -> str:
    return re.sub(r"[^a-z0-9]+", "_", doi.lower()).strip("_")


def title_key(title: str, year) -> str:
    import hashlib
    t = re.sub(r"[^a-z0-9]+", " ", unicodedata.normalize("NFKC", title).casefold()).strip()
    return "th:" + hashlib.sha1(f"{t}|{year or ''}".encode()).hexdigest()[:16]


def dedupe_key(rec: dict) -> str:
    if rec.get("doi"):
        return "doi:" + rec["doi"].lower().strip()
    if rec.get("arxiv"):
        return "arxiv:" + re.sub(r"v\d+$", "", rec["arxiv"].lower().strip())
    return title_key(rec.get("title", ""), rec.get("year"))


def fulltext_pages(sha256: str) -> list[str] | None:
    p = FULLTEXT / f"{sha256}.txt"
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8").split("\f")
