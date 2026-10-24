"""The manifest reader: T-38a's columns, PNGs only, and consistent rows."""

from fractions import Fraction

import pytest

import manifest
from conftest import COLUMNS, original, repaired, write_manifest


def test_reads_t38a_rows(tmp_path):
    path = write_manifest(tmp_path, [original("Doc", 0, "ar"), repaired("Doc", "C3", 0, "ar", "3/4")])
    rows = manifest.read(path)
    assert [r.role for r in rows] == ["original", "repaired"]
    assert rows[1].cls == "C3" and rows[1].page == 0 and rows[1].lang == "ar"
    assert rows[1].lcs_f1 == Fraction(3, 4)
    assert rows[1].png == "Doc(print)_c3/p0.png"
    assert rows[1].png_path == tmp_path / "Doc(print)_c3" / "p0.png"


def test_a_png_that_ends_in_pdf_is_rejected(tmp_path):
    row = original("Doc", 0)
    row["png"] = "Doc(print)/p0.pdf"
    path = write_manifest(tmp_path, [row])
    with pytest.raises(manifest.ManifestError, match=r"\.pdf"):
        manifest.read(path)


def test_only_pngs_are_accepted(tmp_path):
    row = original("Doc", 0)
    row["png"] = "Doc(print)/p0.jpg"
    with pytest.raises(manifest.ManifestError):
        manifest.read(write_manifest(tmp_path, [row]))


def test_wrong_columns_are_rejected(tmp_path):
    path = tmp_path / "manifest.csv"
    path.write_text(",".join(COLUMNS[:-1]) + "\n", encoding="utf-8")
    with pytest.raises(manifest.ManifestError, match="columns"):
        manifest.read(path)


def test_open_ok_must_agree_with_png(tmp_path):
    row = original("Doc", 0)
    row["open_ok"] = "0"
    with pytest.raises(manifest.ManifestError, match="open_ok"):
        manifest.read(write_manifest(tmp_path, [row]))


def test_a_png_outside_the_manifest_directory_is_rejected(tmp_path):
    row = original("Doc", 0)
    row["png"] = "../elsewhere/p0.png"
    path = tmp_path / "m" / "manifest.csv"
    path.parent.mkdir()
    with pytest.raises(manifest.ManifestError):
        manifest.read(write_manifest(path.parent, [row]))


def test_duplicate_pages_are_rejected(tmp_path):
    with pytest.raises(manifest.ManifestError, match="twice"):
        manifest.read(write_manifest(tmp_path, [original("Doc", 0), original("Doc", 0)]))
