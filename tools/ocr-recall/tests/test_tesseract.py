"""The Tesseract engine (G-11, D-014): per-script tessdata_best models pinned
by hash in `tessdata.toml`, routed by the page's language label, read by a
`tesseract` binary on PATH. A fake binary stands in for Tesseract, so no
test installs it or downloads a model."""

import csv
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

import models
import ocr_recall
from conftest import original, repaired, write_manifest
from engines import EngineMissing, tesseract

HERE = Path(__file__).resolve().parent.parent

LABEL_TAIL = "not REPDF's metric; not comparable to the paper; not a published recovery rate"

FAKE = '''#!{python}
import json, os, sys
args = sys.argv[1:]
if args == ["--version"]:
    print("tesseract 5.5.3")
    print(" leptonica-1.85.0")
    sys.exit(0)
with open(os.environ["FAKE_TESSERACT_LOG"], "a", encoding="utf-8") as f:
    f.write(json.dumps({{"args": args, "omp": os.environ.get("OMP_THREAD_LIMIT")}}) + "\\n")
png = args[0]
if os.path.basename(os.path.dirname(png)).endswith("_fail"):
    print("Error in pixReadStream: Unknown format", file=sys.stderr)
    sys.exit(1)
texts = json.load(open(os.environ["FAKE_TESSERACT_TEXTS"], encoding="utf-8"))
key = os.path.basename(os.path.dirname(png)) + "/" + os.path.basename(png)
sys.stdout.write("\\n\\n".join(texts.get(key, [])) + "\\n\\f")
'''


@pytest.fixture
def fake_tesseract(tmp_path, monkeypatch):
    """Puts a fake `tesseract` first on PATH; returns a function that sets the
    text each PNG reads as, and the log of every OCR call."""
    bindir = tmp_path / "bin"
    bindir.mkdir()
    exe = bindir / "tesseract"
    exe.write_text(FAKE.format(python=sys.executable), encoding="utf-8")
    exe.chmod(0o755)
    log = tmp_path / "tesseract.log"
    texts = tmp_path / "texts.json"
    texts.write_text("{}", encoding="utf-8")
    monkeypatch.delenv("TESSERACT", raising=False)
    monkeypatch.setenv("PATH", f"{bindir}{os.pathsep}{os.environ.get('PATH', '')}")
    monkeypatch.setenv("FAKE_TESSERACT_LOG", str(log))
    monkeypatch.setenv("FAKE_TESSERACT_TEXTS", str(texts))

    class Fake:
        def set_texts(self, mapping: dict[str, list[str]]) -> None:
            texts.write_text(json.dumps(mapping), encoding="utf-8")

        def calls(self) -> list[dict]:
            if not log.exists():
                return []
            return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]

    return Fake()


def no_download(url: str, dest: Path) -> None:
    raise AssertionError(f"downloaded {url}")


def pinned_table(tmp_path: Path, content: bytes = b"weights") -> Path:
    """A `tessdata.toml` whose every model hashes to `sha256(content)`."""
    digest = hashlib.sha256(content).hexdigest()
    toml = tmp_path / "tessdata.toml"
    toml.write_text(
        '[source]\nrepo = "tesseract-ocr/tessdata_best"\ncommit = "' + "c" * 40 + '"\n\n'
        '[routes]\nlatin = ["eng", "fra", "spa"]\nhan = ["chi_sim"]\narabic = ["ara"]\ndevanagari = ["hin"]\n\n'
        + "".join(
            f'[models.{n}]\nsha256 = "{digest}"\nbytes = {len(content)}\n\n'
            for n in ("eng", "fra", "spa", "chi_sim", "ara", "hin")
        ),
        encoding="utf-8",
    )
    return toml


def fill_cache(cache: Path, names, content: bytes = b"weights") -> None:
    cache.mkdir(parents=True, exist_ok=True)
    for name in names:
        (cache / f"{name}.traineddata").write_bytes(content)


ALL = ("eng", "fra", "spa", "chi_sim", "ara", "hin")


def six_language_manifest(tmp_path: Path) -> Path:
    return write_manifest(
        tmp_path / "in",
        [
            original("Doc", 0, "en"),
            original("Doc", 1, "fr"),
            original("Doc", 2, "zh"),
            original("Doc", 3, "ar"),
            original("Doc", 4, "hi"),
            original("Doc", 5, "es"),
            repaired("Doc", "C2", 0, "en"),
            repaired("Doc", "C2", 2, "zh"),
        ],
    )


def run(tmp_path, manifest_path, *extra) -> tuple[int, Path]:
    out = tmp_path / "out" / "ocr_results.csv"
    argv = ["--manifest", str(manifest_path), "--out", str(out), "--engine", "tesseract", *extra]
    return ocr_recall.main(argv), out


# ── the pinned models ─────────────────────────────────────────────────────


def test_tessdata_toml_pins_one_tessdata_best_commit_and_every_model():
    table = tesseract.load(tesseract.TESSDATA_TOML)
    assert table.repo == "tesseract-ocr/tessdata_best"
    assert len(table.commit) == 40 and all(c in "0123456789abcdef" for c in table.commit)
    assert sorted(table.models) == sorted(ALL)
    for m in table.models.values():
        assert len(m.sha256) == 64 and all(c in "0123456789abcdef" for c in m.sha256)
        assert m.size > 0
    assert tesseract.url(table, "eng") == (
        f"https://raw.githubusercontent.com/tesseract-ocr/tessdata_best/{table.commit}/eng.traineddata"
    )


def test_every_script_routes_to_its_own_models():
    table = tesseract.load(tesseract.TESSDATA_TOML)
    assert {r: table.routes[r] for r in models.ROUTES} == {
        "latin": ("eng", "fra", "spa"),
        "han": ("chi_sim",),
        "arabic": ("ara",),
        "devanagari": ("hin",),
    }


def test_a_route_naming_an_unpinned_model_is_rejected(tmp_path):
    toml = pinned_table(tmp_path)
    toml.write_text(toml.read_text(encoding="utf-8").replace('han = ["chi_sim"]', 'han = ["chi_tra"]'), encoding="utf-8")
    with pytest.raises(models.ModelError, match="chi_tra"):
        tesseract.load(toml)


# ── install check and dry run ─────────────────────────────────────────────


def test_dry_run_without_tesseract_prints_a_one_line_install_hint(tmp_path, monkeypatch, capsys):
    monkeypatch.delenv("TESSERACT", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty"))
    code, out = run(tmp_path, six_language_manifest(tmp_path), "--dry-run")
    err = capsys.readouterr().err.strip()
    assert code != 0
    assert len(err.splitlines()) == 1
    assert "tesseract" in err and "apt-get install" in err
    assert not out.exists()


def test_dry_run_reads_the_manifest_and_fetches_and_ocrs_nothing(tmp_path, monkeypatch, fake_tesseract, capsys):
    monkeypatch.setattr(tesseract, "_download", no_download)
    code, out = run(tmp_path, six_language_manifest(tmp_path), "--dry-run")
    stdout = capsys.readouterr().out
    assert code == 0
    assert "engine tesseract" in stdout
    assert "8 PNGs" in stdout
    assert "routes arabic, devanagari, han, latin" in stdout
    assert fake_tesseract.calls() == []
    assert not out.exists()


def test_dry_run_through_the_real_entry_point(tmp_path, fake_tesseract):
    # As CI runs it: isolated, the engine chosen by flag.
    proc = subprocess.run(
        [
            sys.executable,
            "-I",
            str(HERE / "ocr_recall.py"),
            "--manifest",
            str(six_language_manifest(tmp_path)),
            "--out",
            str(tmp_path / "out.csv"),
            "--engine",
            "tesseract",
            "--dry-run",
        ],
        capture_output=True,
        text=True,
        check=False,
        env=os.environ.copy(),
    )
    assert proc.returncode == 0, proc.stderr
    assert "dry run: engine tesseract" in proc.stdout
    assert fake_tesseract.calls() == []
    assert not (tmp_path / "out.csv").exists()


# ── the hash gate ─────────────────────────────────────────────────────────


def test_a_hash_mismatch_aborts_before_any_ocr(tmp_path, monkeypatch, fake_tesseract, capsys):
    cache = tmp_path / "cache"
    monkeypatch.setenv("TESSDATA_BEST_CACHE", str(cache))
    fetched: list[str] = []

    def download(url: str, dest: Path) -> None:
        fetched.append(url)
        dest.write_bytes(b"not the model")

    monkeypatch.setattr(tesseract, "_download", download)
    code, out = run(tmp_path, six_language_manifest(tmp_path))
    err = capsys.readouterr().err
    assert code != 0
    assert "sha256" in err and "tessdata.toml" in err
    table = tesseract.load(tesseract.TESSDATA_TOML)
    assert sorted(fetched) == sorted(tesseract.url(table, n) for n in ALL)
    assert fake_tesseract.calls() == []
    assert not out.exists()


def test_models_already_in_the_cache_are_not_fetched_again(tmp_path, monkeypatch, fake_tesseract):
    cache = tmp_path / "cache"
    fill_cache(cache, ALL)
    monkeypatch.setenv("TESSDATA_BEST_CACHE", str(cache))
    monkeypatch.setattr(tesseract, "TESSDATA_TOML", pinned_table(tmp_path))
    monkeypatch.setattr(tesseract, "_download", no_download)
    code, _ = run(tmp_path, six_language_manifest(tmp_path))
    assert code == 0


def test_only_the_routes_a_run_needs_are_fetched(tmp_path, monkeypatch, fake_tesseract):
    cache = tmp_path / "cache"
    monkeypatch.setenv("TESSDATA_BEST_CACHE", str(cache))
    monkeypatch.setattr(tesseract, "TESSDATA_TOML", pinned_table(tmp_path))
    fetched: list[str] = []

    def download(url: str, dest: Path) -> None:
        fetched.append(url.rsplit("/", 1)[1])
        dest.write_bytes(b"weights")

    monkeypatch.setattr(tesseract, "_download", download)
    path = write_manifest(tmp_path / "in", [original("Doc", 0, "ar"), repaired("Doc", "C2", 0, "ar")])
    code, _ = run(tmp_path, path)
    assert code == 0
    assert fetched == ["ara.traineddata"]


# ── a run ─────────────────────────────────────────────────────────────────


@pytest.fixture
def ready(tmp_path, monkeypatch, fake_tesseract):
    """The fake binary on PATH, a pinned table and a filled cache."""
    cache = tmp_path / "cache"
    fill_cache(cache, ALL)
    monkeypatch.setenv("TESSDATA_BEST_CACHE", str(cache))
    monkeypatch.setattr(tesseract, "TESSDATA_TOML", pinned_table(tmp_path))
    monkeypatch.setattr(tesseract, "_download", no_download)
    return fake_tesseract, cache


def test_each_page_is_read_with_the_models_of_its_language_label(tmp_path, ready):
    fake, cache = ready
    code, _ = run(tmp_path, six_language_manifest(tmp_path))
    assert code == 0
    langs = {Path(c["args"][0]).parent.name + "/" + Path(c["args"][0]).name: c["args"] for c in fake.calls()}

    def lang_of(key: str) -> str:
        args = langs[key]
        return args[args.index("-l") + 1]

    assert lang_of("Doc(print)/p0.png") == "eng+fra+spa"
    assert lang_of("Doc(print)/p1.png") == "eng+fra+spa"
    assert lang_of("Doc(print)/p5.png") == "eng+fra+spa"
    assert lang_of("Doc(print)/p2.png") == "chi_sim"
    assert lang_of("Doc(print)/p3.png") == "ara"
    assert lang_of("Doc(print)/p4.png") == "hin"
    assert lang_of("Doc(print)_c2/p2.png") == "chi_sim"
    for args in langs.values():
        assert args[1] == "stdout"
        assert args[args.index("--tessdata-dir") + 1] == str(cache)
        assert args[args.index("--oem") + 1] == "1"
        assert args[args.index("--psm") + 1] == "3"
        assert args[args.index("--dpi") + 1] == "200"


def test_ocr_runs_once_per_png_in_manifest_order(tmp_path, ready):
    fake, _ = ready
    run(tmp_path, six_language_manifest(tmp_path))
    order = [Path(c["args"][0]).parent.name + "/" + Path(c["args"][0]).name for c in fake.calls()]
    assert order == [
        "Doc(print)/p0.png",
        "Doc(print)/p1.png",
        "Doc(print)/p2.png",
        "Doc(print)/p3.png",
        "Doc(print)/p4.png",
        "Doc(print)/p5.png",
        "Doc(print)_c2/p0.png",
        "Doc(print)_c2/p2.png",
    ]


def test_the_thread_count_reaches_tesseract(tmp_path, ready):
    fake, _ = ready
    run(tmp_path, six_language_manifest(tmp_path))
    assert {c["omp"] for c in fake.calls()} == {str(os.cpu_count() or 1)}


def test_the_d063_label_heads_the_csv_and_every_table(tmp_path, ready, capsys):
    code, out = run(tmp_path, six_language_manifest(tmp_path))
    assert code == 0
    label = (
        "regression baseline; 44-file REPDF smoke subset; REPDF-style OCR word recall, "
        f"engine tesseract 5.5.3; {LABEL_TAIL}"
    )
    assert out.read_text(encoding="utf-8").splitlines()[0] == label
    stdout = capsys.readouterr().out.splitlines()
    assert stdout[0] == label
    assert stdout.count(label) == 3
    # The apparatus line names the pinned commit, the routing and the settings.
    assert stdout[1].startswith("apparatus: engine tesseract 5.5.3; tessdata_best@" + "c" * 40)
    assert "latin: eng+fra+spa" in stdout[1] and "han: chi_sim" in stdout[1]
    assert "arabic: ara" in stdout[1] and "devanagari: hin" in stdout[1]
    assert "oem 1" in stdout[1] and "psm 3" in stdout[1] and "dpi 200" in stdout[1]


def test_the_csv_names_the_engine_and_each_routes_models(tmp_path, ready):
    fake, _ = ready
    fake.set_texts(
        {
            "Doc(print)/p0.png": ["Hello world", "", "second line"],
            "Doc(print)/p2.png": ["中文测试"],
            "Doc(print)_c2/p0.png": ["hello world"],
            "Doc(print)_c2/p2.png": ["中文"],
        }
    )
    code, out = run(tmp_path, six_language_manifest(tmp_path))
    assert code == 0
    rows = list(csv.DictReader(out.read_text(encoding="utf-8").splitlines()[1:]))
    assert {(r["engine"], r["engine_version"]) for r in rows} == {("tesseract", "5.5.3")}
    got = [(r["page"], r["script"], r["models"], r["ocr_word_recall"]) for r in rows]
    # Pages the variant lacks score 0; those whose original OCR found no words are empty.
    assert got == [
        ("0", "latin", "eng+fra+spa", "0.500000"),
        ("1", "latin", "eng+fra+spa", ""),
        ("2", "han", "chi_sim", "0.500000"),
        ("3", "arabic", "ara", ""),
        ("4", "devanagari", "hin", ""),
        ("5", "latin", "eng+fra+spa", ""),
    ]


def test_ocr_returns_text_lines_without_blanks_or_the_page_break(tmp_path, ready):
    fake, _ = ready
    fake.set_texts({"Doc(print)/p0.png": ["first line", "   ", "second line"]})
    apparatus = tesseract.prepare("tiny", {"latin"}, 2)
    png = tmp_path / "in" / "Doc(print)" / "p0.png"
    png.parent.mkdir(parents=True)
    png.write_bytes(b"")
    assert apparatus.recognisers["latin"].ocr(png) == ["first line", "second line"]
    assert apparatus.name == "tesseract" and apparatus.version == "5.5.3"


def test_a_tesseract_failure_names_the_page_and_the_error(tmp_path, ready):
    apparatus = tesseract.prepare("tiny", {"latin"}, 1)
    png = tmp_path / "Doc(print)_fail" / "p0.png"
    png.parent.mkdir(parents=True)
    png.write_bytes(b"")
    with pytest.raises(tesseract.TesseractError, match=r"p0\.png.*Unknown format"):
        apparatus.recognisers["latin"].ocr(png)


def test_prepare_without_the_binary_raises_the_install_hint(tmp_path, monkeypatch):
    monkeypatch.delenv("TESSERACT", raising=False)
    monkeypatch.setenv("PATH", str(tmp_path / "empty"))
    with pytest.raises(EngineMissing, match="apt-get install"):
        tesseract.prepare("tiny", {"latin"}, 1)
