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
CODECACHE = CACHE / "code"                           # <repo-slug>@<commit12>/ shallow checkouts
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
    "observation": (ROOT / "observations" / "observations.jsonl", "OBS", "observation.schema.json"),
}

# Source types whose full text must be a real document of that work (title-checked, no ad-hoc .txt).
DOCUMENT_TYPES = {"paper", "preprint", "thesis", "standard", "book", "patent", "report"}


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


def fulltext_path(sha256: str) -> Path:
    return FULLTEXT / f"{sha256}.txt"


def fulltext_pages(sha256: str) -> list[str] | None:
    p = fulltext_path(sha256)
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8").split("\f")


def sha256_file(path: Path) -> str:
    import hashlib
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def repo_slug(url: str) -> str:
    """https://github.com/qpdf/qpdf(.git) -> github.com_qpdf_qpdf"""
    u = re.sub(r"^[a-z]+://", "", url.strip()).removesuffix(".git").strip("/")
    return re.sub(r"[^A-Za-z0-9._-]+", "_", u)


def code_checkout(url: str, commit: str) -> Path:
    return CODECACHE / f"{repo_slug(url)}@{commit[:12]}"


# Numbers that carry meaning in a claim's paraphrase (skips ids such as C9, CLM-0012, PDF-1.7).
_NUM = re.compile(r"(?<![A-Za-z\-\d.])\d+(?:[.,]\d+)*")


def _canon_number(n: str) -> str:
    if re.fullmatch(r"\d{1,3}(,\d{3})+", n):      # 1,000 / 12,345,678 -> thousands separators
        return n.replace(",", "")
    return n.replace(",", ".")                     # 90,67 -> 90.67 (decimal comma)


def numbers_in(text: str) -> set[str]:
    return {_canon_number(n) for n in _NUM.findall(unicodedata.normalize("NFKC", text))}
