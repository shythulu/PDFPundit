"""The command line: engine stubs, the install hint, the model hash gate, the
D-063 header and engine independence of the scores."""

import csv
import hashlib
import subprocess
import sys
import types
from pathlib import Path

import pytest

import models
import ocr_recall
from conftest import FixedEngine, original, repaired, write_manifest
from engines import paddle

HERE = Path(__file__).resolve().parent.parent


def manifest_with_one_pair(tmp_path: Path) -> Path:
    return write_manifest(
        tmp_path,
        [
            original("Doc", 0),
            original("Doc", 1, "zh"),
            repaired("Doc", "C2", 0, lcs_f1="1/1"),
            repaired("Doc", "C2", 1, "zh", lcs_f1="1/2"),
            repaired("Doc", "C4", None),
        ],
    )


TEXTS = {
    "Doc(print)/p0.png": ["Hello, world.", "Second line"],
    "Doc(print)/p1.png": ["中文测试"],
    "Doc(print)_c2/p0.png": ["hello world", "second line"],
    "Doc(print)_c2/p1.png": ["中文"],
}


def run(tmp_path, engine, *extra) -> tuple[int, Path]:
    out = tmp_path / "out" / "ocr_results.csv"
    argv = ["--manifest", str(manifest_with_one_pair(tmp_path)), "--out", str(out), *extra]
    return ocr_recall.main(argv, engine=engine), out


def read_rows(out: Path) -> list[dict]:
    lines = out.read_text(encoding="utf-8").splitlines()
    return list(csv.DictReader(lines[1:]))


@pytest.mark.parametrize(
    ("engine", "decision"),
    [("documentai", "blocked on D-014"), ("tesseract", "blocked on D-071")],
)
def test_stub_engines_exit_with_their_blocking_decision(tmp_path, engine, decision):
    # Through the real entry point, isolated, as CI runs it.
    proc = subprocess.run(
        [
            sys.executable,
            "-I",
            str(HERE / "ocr_recall.py"),
            "--manifest",
            str(tmp_path / "missing.csv"),
            "--out",
            str(tmp_path / "out.csv"),
            "--engine",
            engine,
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    assert proc.returncode != 0
    assert decision in proc.stderr
    assert len(proc.stderr.strip().splitlines()) == 1
    assert not (tmp_path / "out.csv").exists()


def test_dry_run_without_paddle_prints_a_one_line_install_hint(tmp_path, monkeypatch, capsys):
    monkeypatch.setitem(sys.modules, "paddleocr", None)
    code, out = run(tmp_path, None, "--dry-run")
    err = capsys.readouterr().err.strip()
    assert code != 0
    assert len(err.splitlines()) == 1
    assert "tools/ocr-recall/requirements.lock" in err
    assert "--require-hashes" in err
    assert not out.exists()


def test_dry_run_reads_the_manifest_and_runs_no_ocr(tmp_path, capsys):
    engine = FixedEngine(TEXTS)
    code, out = run(tmp_path, engine, "--dry-run")
    assert code == 0
    assert engine.recogniser.calls == []
    assert not out.exists()
    assert "4 PNGs" in capsys.readouterr().out


def test_first_header_line_is_the_d063_label(tmp_path, capsys):
    code, out = run(tmp_path, FixedEngine(TEXTS, "fixed", "2.5"))
    assert code == 0
    label = (
        "regression baseline; 44-file REPDF smoke subset; REPDF-style OCR word recall, "
        "engine fixed 2.5; not REPDF's metric; not comparable to the paper; "
        "not a published recovery rate"
    )
    assert out.read_text(encoding="utf-8").splitlines()[0] == label
    stdout = capsys.readouterr().out.splitlines()
    assert stdout[0] == label
    # Every aggregate table is headed by the label too.
    assert stdout.count(label) == 3


def test_results_csv_scores_every_original_page(tmp_path):
    code, out = run(tmp_path, FixedEngine(TEXTS))
    assert code == 0
    rows = read_rows(out)
    got = [
        (r["class"], r["page"], r["script"], r["ocr_pages"], r["ocr_word_recall"], r["ocr_word_recall_nopunct"], r["ocr_char_recall"], r["text_visual_gap"])
        for r in rows
    ]
    assert got == [
        ("C2", "0", "latin", "2", "0.500000", "1.000000", "0.909091", "0.500000"),
        ("C2", "1", "han", "2", "0.500000", "0.500000", "0.500000", "0.000000"),
        ("C4", "0", "latin", "0", "0.000000", "0.000000", "0.000000", ""),
        ("C4", "1", "han", "0", "0.000000", "0.000000", "0.000000", ""),
    ]
    assert {(r["engine"], r["engine_version"], r["tier"]) for r in rows} == {("fixed", "1.0", "tiny")}


def test_ocr_runs_once_per_png_in_manifest_order(tmp_path):
    engine = FixedEngine(TEXTS)
    run(tmp_path, engine)
    assert engine.recogniser.calls == list(TEXTS)


def test_the_log_prints_each_pages_ocr_digest(tmp_path, capsys):
    run(tmp_path, FixedEngine(TEXTS))
    digest = hashlib.sha256("\n".join(TEXTS["Doc(print)/p0.png"]).encode()).hexdigest()
    assert any("Doc(print)/p0.png" in l and digest in l for l in capsys.readouterr().out.splitlines())


def test_identical_line_lists_from_any_engine_score_identically(tmp_path):
    _, a = run(tmp_path / "a", FixedEngine(TEXTS, "paddleocr", "3.7.0"))
    _, b = run(tmp_path / "b", FixedEngine(TEXTS, "tesseract", "5.5.3"))
    strip = lambda rows: [{k: v for k, v in r.items() if k not in ("engine", "engine_version")} for r in rows]
    assert strip(read_rows(a)) == strip(read_rows(b))


def test_a_pdf_in_the_manifest_fails_the_run(tmp_path, capsys):
    row = original("Doc", 0)
    row["png"] = "Doc(print)/p0.pdf"
    path = write_manifest(tmp_path, [row])
    engine = FixedEngine(TEXTS)
    code = ocr_recall.main(["--manifest", str(path), "--out", str(tmp_path / "o.csv")], engine=engine)
    assert code != 0
    assert ".pdf" in capsys.readouterr().err
    assert engine.recogniser.calls == []


# ── the model hash gate ──────────────────────────────────────────────────


def fake_paddleocr(constructed: list) -> types.ModuleType:
    mod = types.ModuleType("paddleocr")

    class PaddleOCR:
        def __init__(self, **kwargs):
            constructed.append(kwargs)

        def predict(self, png):
            raise AssertionError("OCR ran")

    mod.PaddleOCR = PaddleOCR
    return mod


def fill_cache(cache: Path, names: list[str], content: bytes = b"not the model") -> None:
    for name in names:
        d = cache / "official_models" / name
        d.mkdir(parents=True)
        (d / "inference.pdiparams").write_bytes(content)


def test_a_models_toml_hash_mismatch_aborts_before_any_ocr(tmp_path, monkeypatch, capsys):
    constructed: list = []
    monkeypatch.setitem(sys.modules, "paddleocr", fake_paddleocr(constructed))
    cache = tmp_path / "cache"
    table = models.load(paddle.MODELS_TOML)
    fill_cache(cache, sorted({n for r in models.ROUTES for n in models.pair(table, "tiny", r)}))
    monkeypatch.setenv("PADDLE_PDX_CACHE_HOME", str(cache))
    code, out = run(tmp_path, None)
    err = capsys.readouterr().err
    assert code != 0
    assert "sha256" in err and "models.toml" in err
    assert constructed == []
    assert not out.exists()


def test_matching_hashes_pass_and_name_every_model(tmp_path):
    cache = tmp_path / "cache"
    fill_cache(cache, ["det_a", "rec_a"], b"weights")
    digest = hashlib.sha256(b"weights").hexdigest()
    toml = tmp_path / "models.toml"
    toml.write_text(
        "".join(
            f'[models.{n}]\nrepo = "o/{n}"\nrevision = "r"\npdiparams_sha256 = "{digest}"\npdiparams_bytes = 7\n'
            for n in ("det_a", "rec_a")
        )
        + '[tiers.tiny.latin]\ndet = "det_a"\nrec = "rec_a"\n',
        encoding="utf-8",
    )
    table = models.load(toml)
    assert models.pair(table, "tiny", "latin") == ("det_a", "rec_a")
    models.verify(table, ["det_a", "rec_a"], cache)
    (cache / "official_models" / "rec_a" / "inference.pdiparams").write_bytes(b"other")
    with pytest.raises(models.ModelError, match="rec_a"):
        models.verify(table, ["det_a", "rec_a"], cache)


def test_models_toml_routes_every_script_in_both_tiers():
    table = models.load(paddle.MODELS_TOML)
    for tier in ("tiny", "medium"):
        for route in models.ROUTES:
            det, rec = models.pair(table, tier, route)
            assert det in table.models and rec in table.models
    assert models.pair(table, "tiny", "latin") == ("PP-OCRv6_tiny_det", "PP-OCRv6_tiny_rec")
    assert models.pair(table, "tiny", "arabic") == ("PP-OCRv5_mobile_det", "arabic_PP-OCRv5_mobile_rec")
    assert models.pair(table, "tiny", "devanagari")[1] == "devanagari_PP-OCRv5_mobile_rec"
    assert models.pair(table, "medium", "han") == ("PP-OCRv6_medium_det", "PP-OCRv6_medium_rec")
