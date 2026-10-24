"""The scorer (D-068): clipped-bag word recall over NFC + casefolded whitespace
tokens (zh per character), its two sensitivity columns, and the pairing of
repaired pages with their original's OCR."""

from fractions import Fraction

import manifest
import recall
from conftest import original, repaired, write_manifest


def words(orig: list[str], rep: list[str], lang: str = "en") -> Fraction | None:
    return recall.score_page(orig, rep, lang).word


def test_perfect_recall():
    assert words(["The quick brown fox"], ["The quick", "brown fox"]) == 1


def test_zero_recall():
    assert words(["The quick brown fox"], ["lorem ipsum"]) == 0
    assert words(["The quick brown fox"], []) == 0


def test_partial_recall():
    assert words(["a b c d"], ["a b"]) == Fraction(1, 2)
    assert words(["a b c d"], ["d x y z q"]) == Fraction(1, 4)


def test_clipped_bag():
    assert words(["a a"], ["a a a"]) == 1
    assert words(["a a a"], ["a"]) == Fraction(1, 3)


def test_casefold_and_nfc_are_equivalent():
    precomposed = "Café STRASSE"
    decomposed = "café strasse"
    assert words([precomposed], [decomposed]) == 1
    assert recall.score_page([precomposed], [decomposed], "en").char == 1


def test_zh_is_scored_per_character():
    s = recall.score_page(["中文测试"], ["中文"], "zh")
    assert s.word == Fraction(1, 2)
    assert s.words_orig == 4
    # The same text read as whitespace tokens would share no token.
    assert recall.score_page(["中文测试"], ["中文"], "en").word == 0


def test_the_three_columns_differ_on_a_punctuated_pair():
    s = recall.score_page(["Hello, world."], ["hello world"], "en")
    assert s.word == 0
    assert s.word_nopunct == 1
    assert s.char == Fraction(10, 12)
    assert len({s.word, s.word_nopunct, s.char}) == 3


def test_an_empty_original_has_no_ground_truth():
    s = recall.score_page([], ["anything"], "en")
    assert s.words_orig == 0
    assert s.word is None and s.word_nopunct is None and s.char is None


def test_identical_line_lists_score_identically_for_any_engine():
    lines = (["Bonjour le monde", "été"], ["bonjour le", "ete"])
    a = recall.score_page(*lines, "fr")
    b = recall.score_page(*lines, "fr")
    assert a == b


def pages(tmp_path, rows):
    return manifest.read(write_manifest(tmp_path, rows))


def test_unopenable_repair_scores_zero_on_every_original_page(tmp_path):
    rows = pages(
        tmp_path,
        [original("Doc", 0), original("Doc", 1), repaired("Doc", "C4", None)],
    )
    texts = {"Doc(print)/p0.png": ["alpha beta"], "Doc(print)/p1.png": ["gamma"]}
    out = recall.pair(rows, texts)
    assert [(r.page, r.word, r.ocr_pages, r.open_ok) for r in out] == [
        (0, Fraction(0), 0, False),
        (1, Fraction(0), 0, False),
    ]
    # No lcs_f1 in the manifest for an unopened file: no gap either.
    assert all(r.gap is None for r in out)


def test_missing_pages_score_zero(tmp_path):
    rows = pages(
        tmp_path,
        [original("Doc", 0), original("Doc", 1), repaired("Doc", "C10", 0)],
    )
    texts = {
        "Doc(print)/p0.png": ["alpha beta"],
        "Doc(print)/p1.png": ["gamma"],
        "Doc(print)_c10/p0.png": ["alpha beta"],
    }
    out = recall.pair(rows, texts)
    assert [(r.page, r.word, r.ocr_pages) for r in out] == [(0, 1, 1), (1, 0, 1)]


def test_text_visual_gap_is_lcs_f1_minus_word_recall(tmp_path):
    rows = pages(
        tmp_path,
        [original("Doc", 0), repaired("Doc", "C2", 0, lcs_f1="9/10")],
    )
    texts = {"Doc(print)/p0.png": ["a b c d"], "Doc(print)_c2/p0.png": ["a b"]}
    (r,) = recall.pair(rows, texts)
    assert r.word == Fraction(1, 2)
    assert r.gap == Fraction(9, 10) - Fraction(1, 2)


def test_pages_route_by_lang_never_by_index(tmp_path):
    rows = pages(
        tmp_path,
        [original("Doc", 0, "hi"), original("Doc", 1, "zh"), repaired("Doc", "C1", 0, "hi")],
    )
    assert [recall.script_of(r.lang) for r in rows] == ["devanagari", "han", "devanagari"]
    assert recall.route_of("und") == "latin"


def test_aggregates_are_means_per_class_and_script():
    a = recall.Result("f1", "C1", "print", "Doc", 0, "en", True, 1, 2, 2, Fraction(1), Fraction(1), Fraction(1), Fraction(1), Fraction(0))
    b = recall.Result("f2", "C1", "print", "Doc", 1, "zh", True, 1, 2, 1, Fraction(1, 2), Fraction(1, 2), Fraction(1, 2), None, None)
    by_script = recall.aggregate([a, b], by_script=True)
    assert [(k, m.pages, m.word) for k, m in by_script] == [
        (("C1", "latin"), 1, Fraction(1)),
        (("C1", "han"), 1, Fraction(1, 2)),
    ]
    ((key, m),) = recall.aggregate([a, b], by_script=False)
    assert key == ("C1",) and m.word == Fraction(3, 4) and m.gap == Fraction(0)


def test_fixed_point_formatting_is_exact():
    assert recall.fixed(Fraction(1, 3), 3) == "0.333"
    assert recall.fixed(Fraction(2, 3), 3) == "0.667"
    assert recall.fixed(Fraction(-1, 8), 2) == "-0.12"
    assert recall.fixed(Fraction(1), 3) == "1.000"
    assert recall.fixed(Fraction(-1, 10**9), 3) == "0.000"
