"""`models.toml`: model name -> Hugging Face repo, revision and the sha256 of
its `inference.pdiparams`, and per tier the (det, rec) pair of each route
(D-067). `verify` runs before the first page; a mismatch fails the run."""

import hashlib
import tomllib
from dataclasses import dataclass
from pathlib import Path

#: The model routes a tier must name, in table order.
ROUTES = ("latin", "han", "arabic", "devanagari")

PARAMS = "inference.pdiparams"


class ModelError(Exception):
    pass


@dataclass(frozen=True)
class Model:
    name: str
    repo: str
    revision: str
    sha256: str
    size: int


@dataclass(frozen=True)
class Table:
    path: Path
    models: dict[str, Model]
    #: tier -> route -> (det, rec)
    tiers: dict[str, dict[str, tuple[str, str]]]


def load(path: Path) -> Table:
    path = Path(path)
    try:
        doc = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ModelError(f"{path}: {e}") from None
    try:
        models = {
            name: Model(name, m["repo"], m["revision"], m["pdiparams_sha256"].lower(), int(m["pdiparams_bytes"]))
            for name, m in doc.get("models", {}).items()
        }
        tiers = {
            tier: {route: (pair["det"], pair["rec"]) for route, pair in routes.items()}
            for tier, routes in doc.get("tiers", {}).items()
        }
    except (KeyError, TypeError, ValueError, AttributeError) as e:
        raise ModelError(f"{path}: malformed entry ({e})") from None
    for tier, routes in tiers.items():
        for route, names in routes.items():
            for name in names:
                if name not in models:
                    raise ModelError(f"{path}: tier {tier} route {route} names {name}, which has no [models] entry")
    return Table(path, models, tiers)


def pair(table: Table, tier: str, route: str) -> tuple[str, str]:
    """The (det, rec) model names of `route` in `tier`."""
    try:
        return table.tiers[tier][route]
    except KeyError:
        raise ModelError(f"{table.path}: tier {tier} has no {route} route") from None


def model_dir(cache: Path, name: str) -> Path:
    """Where paddlex keeps an official model under `PADDLE_PDX_CACHE_HOME`."""
    return Path(cache) / "official_models" / name


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def verify(table: Table, names: list[str], cache: Path) -> None:
    """Every named model's `inference.pdiparams` hashes to its pinned sha256."""
    for name in names:
        model = table.models[name]
        params = model_dir(cache, name) / PARAMS
        if not params.is_file():
            raise ModelError(f"{name}: {params} is missing")
        got = sha256(params)
        if got != model.sha256:
            raise ModelError(
                f"{name}: {PARAMS} sha256 {got} does not match {model.sha256} in {table.path.name}; "
                "delete the model directory to fetch the pinned revision again"
            )
