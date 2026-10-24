"""Per-script Tesseract (committee 005 C3 on PR #19), the OCR-scoring
apparatus since the user's D-014 answer of 2026-10-08.

Models come from `tessdata.toml`: each tessdata_best `.traineddata` is
fetched at the pinned commit into `TESSDATA_BEST_CACHE` (default
`target/corpus/tessdata-best`), and every one a run needs is hashed before
the first page is read. A page is read with the models of its script's
route, so an Arabic page gets `ara` and a Latin page `eng+fra+spa`. The
binary is `tesseract` on PATH, or `TESSERACT`; it runs once per page with
LSTM only (`--oem 1`), automatic page segmentation (`--psm 3`) and the
rasters' 200 dpi. The distro's own language packs are never read.
"""

import os
import shutil
import subprocess
import tomllib
import urllib.request
from dataclasses import dataclass
from pathlib import Path

import models
from engines import Apparatus, EngineMissing

NAME = "tesseract"

HERE = Path(__file__).resolve().parent.parent
TESSDATA_TOML = HERE / "tessdata.toml"
DEFAULT_CACHE = HERE.parent.parent / "target" / "corpus" / "tessdata-best"

INSTALL_HINT = (
    "tesseract is not installed or not on PATH; on Ubuntu run "
    "`sudo apt-get install tesseract-ocr` (models come from tools/ocr-recall/tessdata.toml, "
    "not the distro's language packs), or set TESSERACT to the binary"
)

#: T-38a renders every page at 200 dpi; PNGs may carry no resolution tag.
DPI = 200
OEM = 1
PSM = 3


class TesseractError(Exception):
    """Tesseract failed on a page; the message names the page and the error."""


@dataclass(frozen=True)
class Model:
    name: str
    sha256: str
    size: int


@dataclass(frozen=True)
class Table:
    path: Path
    repo: str
    commit: str
    #: route -> the models Tesseract reads it with, in `-l` order
    routes: dict[str, tuple[str, ...]]
    models: dict[str, Model]


def load(path: Path) -> Table:
    path = Path(path)
    try:
        doc = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise models.ModelError(f"{path}: {e}") from None
    try:
        source = doc["source"]
        table = Table(
            path=path,
            repo=str(source["repo"]),
            commit=str(source["commit"]),
            routes={route: tuple(str(n) for n in names) for route, names in doc["routes"].items()},
            models={
                name: Model(name, str(m["sha256"]).lower(), int(m["bytes"]))
                for name, m in doc.get("models", {}).items()
            },
        )
    except (KeyError, TypeError, ValueError, AttributeError) as e:
        raise models.ModelError(f"{path}: malformed entry ({e})") from None
    for route, names in table.routes.items():
        if not names:
            raise models.ModelError(f"{path}: route {route} names no model")
        for name in names:
            if name not in table.models:
                raise models.ModelError(f"{path}: route {route} names {name}, which has no [models] entry")
    return table


def url(table: Table, name: str) -> str:
    return f"https://raw.githubusercontent.com/{table.repo}/{table.commit}/{name}.traineddata"


def cache_home() -> Path:
    return Path(os.environ.get("TESSDATA_BEST_CACHE") or DEFAULT_CACHE)


def _binary() -> str:
    found = shutil.which(os.environ.get("TESSERACT") or "tesseract")
    if found is None:
        raise EngineMissing(INSTALL_HINT)
    return found


def check_installed() -> None:
    _binary()


def _version(binary: str) -> str:
    """The version from `tesseract --version`'s first line ("tesseract 5.5.3");
    older releases print it on stderr."""
    proc = subprocess.run([binary, "--version"], capture_output=True, text=True, check=False)
    for line in (proc.stdout + proc.stderr).splitlines():
        words = line.split()
        if len(words) >= 2 and words[0] == "tesseract":
            return words[1]
    return "unknown"


def _download(source: str, dest: Path) -> None:
    """Writes `source` to `dest` through a temporary name, so an interrupted
    fetch never leaves a partial file under the model's name."""
    partial = dest.with_name(dest.name + ".partial")
    with urllib.request.urlopen(source) as response, partial.open("wb") as f:
        shutil.copyfileobj(response, f)
    partial.replace(dest)


def model_path(cache: Path, name: str) -> Path:
    return Path(cache) / f"{name}.traineddata"


def _fetch(table: Table, name: str, cache: Path) -> None:
    """Downloads `name` at the pinned commit unless it is there. A file already
    present is never replaced here; `verify` judges it."""
    target = model_path(cache, name)
    if target.is_file():
        return
    cache.mkdir(parents=True, exist_ok=True)
    _download(url(table, name), target)


def verify(table: Table, names: list[str], cache: Path) -> None:
    """Every named model hashes to its pinned sha256."""
    for name in names:
        path = model_path(cache, name)
        if not path.is_file():
            raise models.ModelError(f"{name}: {path} is missing")
        got = models.sha256(path)
        want = table.models[name].sha256
        if got != want:
            raise models.ModelError(
                f"{name}: {path.name} sha256 {got} does not match {want} in {table.path.name}; "
                "delete the file to fetch the pinned commit again"
            )


@dataclass(frozen=True)
class _Recogniser:
    binary: str
    options: tuple[str, ...]
    env: dict[str, str]
    models: str

    def ocr(self, png: Path) -> list[str]:
        proc = subprocess.run(
            [self.binary, str(png), "stdout", *self.options],
            capture_output=True,
            env=self.env,
            check=False,
        )
        if proc.returncode != 0:
            err = proc.stderr.decode("utf-8", "replace").strip().splitlines()
            raise TesseractError(f"tesseract exited {proc.returncode} on {png}: {err[-1] if err else 'no message'}")
        # Tesseract ends each page with a form feed, which splitlines treats
        # as a line break; blank lines carry no words.
        return [line for line in proc.stdout.decode("utf-8").splitlines() if line.strip()]


def prepare(tier: str, routes: set[str], threads: int) -> Apparatus:
    binary = _binary()
    table = load(TESSDATA_TOML)
    chosen = {}
    for route in models.ROUTES:
        if route in routes:
            if route not in table.routes:
                raise models.ModelError(f"{table.path}: no {route} route")
            chosen[route] = table.routes[route]
    names = sorted({name for names in chosen.values() for name in names})
    cache = cache_home()
    for name in names:
        _fetch(table, name, cache)
    # Every hash is checked before the first page is read.
    verify(table, names, cache)

    env = {**os.environ, "OMP_THREAD_LIMIT": str(threads)}
    settings = f"oem {OEM}, psm {PSM}, dpi {DPI}"
    recognisers = {}
    for route, langs in chosen.items():
        joined = "+".join(langs)
        options = ("--tessdata-dir", str(cache), "-l", joined, "--oem", str(OEM), "--psm", str(PSM), "--dpi", str(DPI))
        recognisers[route] = _Recogniser(binary, options, env, joined)

    routing = "; ".join(f"{route}: {'+'.join(langs)}" for route, langs in chosen.items())
    return Apparatus(
        name=NAME,
        version=_version(binary),
        detail=(
            f"tessdata_best@{table.commit}; {routing}; {settings}; OMP_THREAD_LIMIT {threads}; "
            f"tier {tier} has no effect (one model set)"
        ),
        recognisers=recognisers,
    )
