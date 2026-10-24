"""Local PaddleOCR (DA M7), the engine built first (D-071).

Models are named explicitly from `models.toml` (D-067): passing names turns
off PaddleOCR's `lang`/`ocr_version` defaults, which in 3.7.0 pick the
PP-OCRv6 medium pair. Each model is fetched from its Hugging Face repo at the
pinned revision into `PADDLE_PDX_CACHE_HOME` (default
`target/corpus/paddle-models`), every `inference.pdiparams` is hashed before
any pipeline is built, and orientation, unwarping and textline models are
off. One pipeline per (det, rec) pair; pages run in one process.
"""

import importlib
import importlib.metadata
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import models
from engines import Apparatus, EngineMissing

NAME = "paddleocr"

HERE = Path(__file__).resolve().parent.parent
MODELS_TOML = HERE / "models.toml"
DEFAULT_CACHE = HERE.parent.parent / "target" / "corpus" / "paddle-models"

INSTALL_HINT = (
    "paddleocr is not installed; on Python 3.12 or 3.13 run "
    "`uv pip install --require-hashes -r tools/ocr-recall/requirements.lock`"
)

PACKAGES = ("paddleocr", "paddlex", "paddlepaddle")


def cache_home() -> Path:
    return Path(os.environ.get("PADDLE_PDX_CACHE_HOME") or DEFAULT_CACHE)


def _environment() -> None:
    """paddlex reads these when it is imported: models live in the run's cache
    and no model-source probe goes to the network."""
    os.environ["PADDLE_PDX_CACHE_HOME"] = str(cache_home())
    os.environ["PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK"] = "True"


def check_installed() -> None:
    _environment()
    try:
        importlib.import_module("paddleocr")
    except ImportError:
        raise EngineMissing(INSTALL_HINT) from None


def _version(package: str) -> str:
    try:
        return importlib.metadata.version(package)
    except importlib.metadata.PackageNotFoundError:
        return "unknown"


def _fetch(model: models.Model, cache: Path) -> None:
    """Downloads `model` at its pinned revision unless its weights are there.
    A file already present is never replaced here; `models.verify` judges it."""
    target = models.model_dir(cache, model.name)
    if (target / models.PARAMS).is_file():
        return
    try:
        from huggingface_hub import snapshot_download
    except ImportError:
        raise EngineMissing(INSTALL_HINT) from None
    snapshot_download(repo_id=model.repo, revision=model.revision, local_dir=str(target))


@dataclass
class _Recogniser:
    pipeline: Any
    models: str

    def ocr(self, png: Path) -> list[str]:
        result = self.pipeline.predict(str(png))
        if not result:
            return []
        return [str(t) for t in result[0]["rec_texts"]]


def prepare(tier: str, routes: set[str], threads: int) -> Apparatus:
    _environment()
    table = models.load(MODELS_TOML)
    pairs = {route: models.pair(table, tier, route) for route in models.ROUTES if route in routes}
    names = sorted({name for p in pairs.values() for name in p})
    cache = cache_home()
    for name in names:
        _fetch(table.models[name], cache)
    # Every hash is checked before the first pipeline exists, so a mismatch
    # stops the run before any page is read.
    models.verify(table, names, cache)

    from paddleocr import PaddleOCR

    pipelines: dict[tuple[str, str], _Recogniser] = {}
    recognisers = {}
    for route, (det, rec) in pairs.items():
        if (det, rec) not in pipelines:
            pipeline = PaddleOCR(
                text_detection_model_name=det,
                text_detection_model_dir=str(models.model_dir(cache, det)),
                text_recognition_model_name=rec,
                text_recognition_model_dir=str(models.model_dir(cache, rec)),
                use_doc_orientation_classify=False,
                use_doc_unwarping=False,
                use_textline_orientation=False,
                device="cpu",
                cpu_threads=threads,
            )
            pipelines[(det, rec)] = _Recogniser(pipeline, f"{det}+{rec}")
        recognisers[route] = pipelines[(det, rec)]

    routing = "; ".join(
        f"{route}: {det}@{table.models[det].revision} + {rec}@{table.models[rec].revision}"
        for route, (det, rec) in pairs.items()
    )
    versions = ", ".join(f"{p} {_version(p)}" for p in PACKAGES)
    return Apparatus(
        name=NAME,
        version=_version("paddleocr"),
        detail=f"tier {tier}; {routing}; {versions}; cpu_threads {threads}",
        recognisers=recognisers,
    )
