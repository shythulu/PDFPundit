"""Fixture for review finding 8: input-derived strings in Part B.

A PDF resource name with bytes 0x7F and 0xC3 0x28 (not UTF-8) is written two ways:
  raw:     decoded lossily and stored as is (what the first draft allowed)
  escaped: PDF name syntax, every byte outside 0x21..0x7E, '#' and the delimiters as #xx (the rule now)
For each, the script builds a Part B, hashes it with Python's compact writer, and asks jq
for the same compact bytes. Usage: ascii_fixture.py <custody record.json> <scratch dir>
"""
import hashlib
import json
import os
import subprocess
import sys

rec = json.load(open(sys.argv[1]))
scratch = sys.argv[2]
os.makedirs(scratch, exist_ok=True)
NAME = b"F\x7f\xc3(1"


DELIMS = set(b"#()<>[]{}/%")


def pdf_name_escape(b):
    """ISO 32000 name syntax: regular characters as is, anything else as #xx."""
    return "".join(chr(x) if 0x21 <= x <= 0x7E and x not in DELIMS else f"#{x:02X}" for x in b)


def run(label, slot):
    repro = json.loads(json.dumps(rec["reproducible"]))
    repro["interactions"][0]["slot"] = slot
    repro["font_decisions"][1]["slot"] = slot
    py = json.dumps(repro, ensure_ascii=False, separators=(",", ":")).encode()
    path = os.path.join(scratch, f"{label}.json")
    with open(path, "w", encoding="utf-8") as f:
        json.dump({"reproducible": repro}, f, ensure_ascii=False, indent=2)
    jq = subprocess.run(["jq", "-cj", ".reproducible", path], capture_output=True, check=True).stdout
    same = py == jq
    printable = all(0x20 <= x <= 0x7E for x in py)
    print(f"{label:<8} slot={slot!r:<16} printable={printable!s:<5} jq==python={same!s:<5} "
          f"py={hashlib.sha256(py).hexdigest()[:16]} jq={hashlib.sha256(jq).hexdigest()[:16]}")
    return same, printable


raw_same, _ = run("raw", NAME.decode("utf-8", errors="replace").replace("�", "Ã") + "\x7f")
esc_same, esc_printable = run("escaped", pdf_name_escape(NAME))
assert not raw_same, "expected the raw form to disagree"
assert esc_same and esc_printable
print("fixture OK: raw form disagrees, escaped form agrees")
