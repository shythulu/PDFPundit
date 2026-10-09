#!/usr/bin/env python3
"""Cuts a Leipzig Corpora Collection word list to the bundled format (G-09, D-011).

Run with an isolated interpreter:

    python3 -I tools/cut-wordlist.py WORDS_TXT OUT_TXT   # write the 50k cut
    python3 -I tools/cut-wordlist.py --self-test         # on a synthetic list

WORDS_TXT is a corpus's `<corpus>-words.txt`: `id<TAB>word<TAB>count` lines,
case-sensitive, punctuation and numbers included. OUT_TXT gets the format
`FrequencyList::from_bytes` reads (`src/pdf/fontdb/dict.rs`): one `word count`
line per word, LF endings, count descending.

The cut, in order:
  1. each word is NFC-normalised, lower-cased, and NFC-normalised again;
  2. a word is kept only if it is a run of letters, optionally joined by single
     internal apostrophes (' or U+2019) or hyphens, with at most one trailing
     apostrophe: no digits, no punctuation-only tokens, no spaces;
  3. counts of words that are now equal are summed;
  4. the words are sorted by (count descending, word ascending in code point
     order) and the first LIMIT kept.

The output depends on the Python's Unicode tables (step 1 and 2), so
fetch-assets.sh checks it against a pinned SHA-256 rather than trusting a
re-run on another interpreter.
"""

import re
import sys
import unicodedata

LIMIT = 50_000
WORD = re.compile(r"[^\W\d_]+(?:['\u2019-][^\W\d_]+)*['\u2019]?")


def cut(lines, limit=LIMIT):
    """The `word count` lines (without newlines) of the cut of `lines`."""
    counts = {}
    for line in lines:
        parts = line.rstrip("\r\n").split("\t")
        if len(parts) != 3 or not parts[2].isdigit():
            continue
        word = unicodedata.normalize("NFC", parts[1])
        word = unicodedata.normalize("NFC", word.lower())
        if not WORD.fullmatch(word):
            continue
        counts[word] = counts.get(word, 0) + int(parts[2])
    ranked = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))
    return [f"{w} {n}" for w, n in ranked[:limit]]


def self_test():
    sample = [
        "1\t!\t900",
        "2\tThe\t500",
        "3\tthe\t400",
        "4\t2024\t300",
        "5\tdon't\t200",
        "6\tdon\u2019t\t150",
        "7\t\u00e9t\u00e9\t100",
        "8\te\u0301te\u0301\t50",  # decomposed: merges with "été"
        "9\tl'\t100",
        "10\twell-known\t90",
        "11\t-x\t80",
        "12\ta b\t70",
        "13\tB2B\t60",
        "14\tzebra\t100",
        "15\tbad line",
        "16\t\u00c9cole\t5",
    ]
    got = cut(sample)
    want = [
        "the 900",
        "don't 200",
        "don\u2019t 150",
        "\u00e9t\u00e9 150",
        "l' 100",
        "zebra 100",
        "well-known 90",
        "\u00e9cole 5",
    ]
    assert got == want, got
    assert cut(sample, limit=2) == want[:2]
    print("cut-wordlist self-test ok")


def main(argv):
    if argv == ["--self-test"]:
        self_test()
        return 0
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[0], encoding="utf-8", newline="\n") as f:
        lines = cut(f)
    with open(argv[1], "w", encoding="utf-8", newline="\n") as f:
        f.write("".join(line + "\n" for line in lines))
    print(f"{argv[1]}: {len(lines)} words, last {lines[-1] if lines else '-'}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
