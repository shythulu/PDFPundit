"""Fill doc_template.md with the generated example files and the fixed text in templates.py.

Usage: splice.py <template> <example dir> <out>
"""
import hashlib
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import templates as T  # noqa: E402

tpl, ex, out = sys.argv[1:4]
r = lambda n: open(os.path.join(ex, n), encoding="utf-8").read().rstrip("\n")
case = json.load(open(os.path.join(ex, "invoice_scan_2024.repaired.pdf.case.jsonld")))
g = case["@graph"]
img = [n for n in g if n["@type"] == "uco-observable:File"
       and any(f["@type"] == "uco-observable:RasterPictureFacet" for f in n["uco-core:hasFacet"])][0]
act = [n for n in g if n.get("uco-core:name") == "Extract images"][0]
txt = r("invoice_scan_2024.repaired.pdf.custody.txt")


def cell(t):
    return t.replace("|", "\\|")


statements = ["| Key | Text |", "|---|---|"]
for k, v in T.STATEMENTS.items():
    if k == "validation":
        statements.append(f"| `validation`, with a build commit | {cell(T.VALIDATION_WITH_COMMIT)} |")
        statements.append(f"| `validation`, no build commit | {cell(T.VALIDATION_NO_COMMIT)} |")
    else:
        statements.append(f"| `{k}` | {cell(v)} |")

standing = "\n".join(f"- {s}" for s in T.STANDING)

this_run = ["| Key | Condition, from Part B | Template |", "|---|---|---|"]
this_run += [f"| `{k}` | {cond} | {cell(t)} |" for k, cond, t in T.THIS_RUN]

E, F = T.ENGINE, T.ENGINE_FIX
engine = ["| Where | Today | Revision 4 asks for |", "|---|---|---|",
          f"| `repair.rs` 690, C9 action, Exact | `{E['c9_exact_action_today']}` | `{F['c9_exact_action']}` |",
          f"| `diagnose.rs` 1726, C9 finding, Exact | `{E['c9_exact_summary_today']}` | `{F['c9_exact_summary']}` |",
          f"| `repair.rs`, C9 action, Exact with `trailer_edit` | same as Exact | `{F['c9_trailer_action']}` |",
          f"| `diagnose.rs`, C9 finding, Exact with `trailer_edit` | same as Exact | `{F['c9_trailer_summary']}` |",
          f"| `report.rs` 25 `SIGNED_NOTE` | `{E['signed_note_today']}` | `{F['signed_note']}` |",
          f"| `diagnose.rs` 134 `SIGNED_SUMMARY` | `{E['signed_summary_today']}` | `{F['signed_summary']}` |"]

s = open(tpl, encoding="utf-8").read()
for k, v in {
    "{{DISPLAY_JSON}}": r("display.json"),
    "{{TXT}}": txt.replace("\f", "~" * 80 + "\n"),
    "{{TXT_PAGES}}": str(txt.count("\f") + 1),
    "{{LOG}}": r("custody.log"),
    "{{MANIFEST}}": r("invoice_scan_2024.repaired.pdf.custody.sha256"),
    "{{SUMS}}": r("f7c39aa7.images/SHA256SUMS"),
    "{{CASE}}": json.dumps([img, act], indent=2, ensure_ascii=False),
    "{{FIXTURE_B7}}": r("fixtures/export-after-custody-off.b7.txt"),
    "{{HEAD42}}": hashlib.sha256(r("custody.log").split("\n")[1].encode()).hexdigest(),
    "{{HEAD43}}": hashlib.sha256(r("custody.log").split("\n")[2].encode()).hexdigest(),
    "{{STATEMENTS_TABLE}}": "\n".join(statements),
    "{{STANDING_LIST}}": standing,
    "{{THIS_RUN_TABLE}}": "\n".join(this_run),
    "{{ENGINE_FIX_TABLE}}": "\n".join(engine),
}.items():
    assert k in s, k
    s = s.replace(k, v)
assert "{{" not in s, s[s.index("{{"):s.index("{{") + 40]
open(out, "w", encoding="utf-8").write(s)
