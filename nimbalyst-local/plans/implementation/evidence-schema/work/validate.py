"""Validate the worked example against the schema and check every hash link and rule.

Usage: validate.py <example dir>. Prints one line per check and ALL OK or FAILURES.
"""
import hashlib
import json
import os
import subprocess
import sys

from jsonschema import Draft202012Validator

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import templates as T  # noqa: E402

EX = sys.argv[1]
schema = json.load(open(os.path.join(EX, "pdfpundit-custody.schema.json")))
Draft202012Validator.check_schema(schema)
results = []


def sub(name):
    return {"$schema": schema["$schema"], "$defs": schema["$defs"], "$ref": f"#/$defs/{name}"}


def check(sch, doc, label):
    errs = sorted(Draft202012Validator(sch).iter_errors(doc), key=str)
    print(f"{label}: {'valid' if not errs else 'INVALID'}")
    for e in errs:
        print("  ", list(e.absolute_path), e.message[:200])
    return not errs


def report(n, label, ok, extra=""):
    print(f"{n:>2}. {label}: {ok}{(' ' + str(extra)) if extra else ''}")
    results.append(ok)


sha = lambda b: hashlib.sha256(b).hexdigest()
compact = lambda v: json.dumps(v, ensure_ascii=False, separators=(",", ":")).encode()
ok_schema = True
rec_name = [n for n in os.listdir(EX) if n.endswith(".custody.json")][0]
rec_bytes = open(os.path.join(EX, rec_name), "rb").read()
rec = json.loads(rec_bytes)
R, run = rec["reproducible"], rec["run"]
ok_schema &= check(schema, rec, "custody record")

img_dir = run["images_dir"]["name"]
idx_bytes = open(os.path.join(EX, img_dir, "images.json"), "rb").read()
idx = json.loads(idx_bytes)
ok_schema &= check(sub("images_index"), idx, "images.json")

log_lines = open(os.path.join(EX, "custody.log"), "rb").read().split(b"\n")[:-1]
for line in log_lines:
    ok_schema &= check(sub("log_entry"), json.loads(line), f"log entry {json.loads(line)['seq']}")

fx_path = os.path.join(EX, "fixtures", "invoice_scan_2024.md.custody.json")
fx = json.load(open(fx_path))
ok_schema &= check(schema, fx, "fixture: export after a custody-off repair")
sx = json.load(open(os.path.join(EX, "fixtures", "salvage-outcomes.custody.json")))
ok_schema &= check(schema, sx, "fixture: unverified and cut-short streams")
results.append(ok_schema)
ALL = [rec, fx, sx]

# 1. reproducible_sha256 == sha256(compact(reproducible)), re-serialised independently
report(1, "reproducible_sha256 recomputed from the pretty file, all three records",
       all(sha(compact(r["reproducible"])) == r["reproducible_sha256"] for r in ALL))
# 2. chain: 41 -> 42 -> 43
report(2, "log chain 41 -> 42 -> 43", all(
    json.loads(log_lines[i + 1])["prev"] == sha(log_lines[i]) for i in range(len(log_lines) - 1)))
# 3. record's log position == entry 42's prev
report(3, "record names the log entry before its own",
       run["log"]["prev_entry_sha256"] == json.loads(log_lines[1])["prev"])
# 4. log entry 42 hashes the three record files, located by role through run.record_files
role_name = {k: run["record_files"][k] for k in ("json", "text", "checksums")}
files = {role_name[f["role"]]: f["sha256"] for f in json.loads(log_lines[1])["files"]}
report(4, "log entry 42 file hashes match files on disk",
       all(sha(open(os.path.join(EX, n), "rb").read()) == d for n, d in files.items()))
# 5. products == placed for every image-folder file whose bytes exist here
prod = {(p["role"], p["file"]): p["sha256"] for p in R["products"]}
report(5, "images.json product hash matches file", prod[("images_index", "images.json")] == sha(idx_bytes))
# 6. image records identical in custody record and images.json
report(6, "image records identical in both files", idx["records"] == R["images"]["records"])
# 7. every page of the .txt prints the SHA-256 of the .json it was rendered from
txt = open(os.path.join(EX, run["record_files"]["text"]), encoding="utf-8").read()
heads = [l for l in txt.split("\n") if l.startswith("JSON SHA-256")]
report(7, "every .txt page header carries the .json SHA-256",
       len(heads) == txt.count("\f") + 1 and all(l.split()[-1] == sha(rec_bytes) for l in heads))
# 8. log lines hold no case details, input names or paths
log_text = b"\n".join(log_lines).decode()
leaks = [x for x in (run["case"]["case_reference"], run["case"]["examiner"], run["case"]["notes"],
                     run["case"]["submission_reference"], run["input_file"]["name"],
                     run["record_files"]["json"], "/") if x and x in log_text]
report(8, "log holds no case details, names or paths", not leaks, leaks or "")
# 9. Part B is printable ASCII, and jq gives the same compact bytes as Python
ok9 = True
for name, r in (("record", rec), ("fixtures/invoice_scan_2024.md.custody.json", fx),
                ("fixtures/salvage-outcomes.custody.json", sx)):
    b = compact(r["reproducible"])
    jq = subprocess.run(["jq", "-cj", ".reproducible", os.path.join(EX, rec_name if name == "record" else name)],
                        capture_output=True, check=True).stdout
    ok9 &= all(0x20 <= x <= 0x7E for x in b) and jq == b and sha(jq) == r["reproducible_sha256"]
report(9, "Part B printable ASCII; jq and Python give the same bytes and hash, all three", ok9)


# 10. font decisions and interactions (m8): many slots may share one interaction; every
#     interaction is used by at least one slot; a slot's basis is its interaction's source.
def c10(r):
    R_ = r["reproducible"]
    inter = {q["seq"]: q for q in R_["interactions"]}
    used = set()
    ok = True
    for d in R_["font_decisions"]:
        asked = d["basis"] in ("user", "batched", "policy")
        ok &= (d["interaction_seq"] is not None) == asked
        if asked:
            q = inter.get(d["interaction_seq"])
            ok &= q is not None and q["source"] == d["basis"]
            used.add(d["interaction_seq"])
    ok &= used == set(inter)
    for q in R_["interactions"]:  # carried_from points back, only for batched
        cf = q["carried_from"]
        ok &= (cf is not None) == (q["source"] == "batched")
        ok &= cf is None or cf["type"] != "this_record" or cf["seq"] < q["seq"]
        ok &= (q["policy_setting"] is not None) == (q["source"] == "policy")
    return ok


report(10, "every slot names one interaction, every interaction covers >= 1 slot", all(c10(r) for r in ALL))


# 11. every scored font slot's decision follows fontdb/score.rs resolve() from its evidence
def ge(a, b):
    return a["num"] * b["den"] >= b["num"] * a["den"]


def decide(ev, unrepro):
    refusal = (ev["top_candidate"] is None or not ge(ev["confidence"], ev["threshold"])
               or not ge(ev["hit"], ev["min_hit"]) or ev["inference"]["n_chars"] < ev["sample_floor"])
    decodes = ev["tounicode"] == "present" or not refusal
    reproducible = ev["top_candidate"] is not None and (
        not decodes or ge(ev["best_coverage"], ev["min_coverage"]))
    if not reproducible:
        return ("text_only", "unreproducible") if unrepro == "text_only" else ("unreproducible",)
    return ("auto_accept",) if not refusal else ("ask",)


fd = R["font_decisions"]
ok11 = all(d["evidence"] is None or d["evidence"]["decision"] in
           decide(d["evidence"], R["settings"]["unreproducible"]) for d in fd)
ok11 &= all((d["basis"] == "auto_accept") == (d["evidence"] is not None
                                              and d["evidence"]["decision"] == "auto_accept") for d in fd)
report(11, "font decisions follow the resolve() rule from their evidence", ok11)

# 12. times ordered: batch opened <= started <= answers <= ended <= record entry; and
#     shown_utc is set only for answers the user saw (m1)
src = {q["seq"]: q["source"] for q in R["interactions"]}
t = [json.loads(log_lines[0])["time_utc"], run["clock"]["started_utc"]]
for a in run["answer_times"]:
    t += [x for x in (a["shown_utc"], a["answered_utc"]) if x]
t += [run["clock"]["ended_utc"], json.loads(log_lines[1])["time_utc"]]
ok12 = t == sorted(t)
ok12 &= all((a["shown_utc"] is not None) == (src[a["seq"]] == "user") for a in run["answer_times"])
report(12, "times ordered; shown_utc only on answers the user saw", ok12)

# 13. C9 search records (M3, m10): counters add up; each pass's count is 255 per window byte;
#     Exact needs the last pass exhausted and one distinct output; error-window arithmetic per
#     salvage.rs; an image from a searched stream carries the same object and the matching status
ERROR_WINDOW, DEEP, SLACK = 4096, 16384, 8
GRADE_KEY = {"exact": "exact", "accepted": "accepted", "ambiguous": "ambiguous"}
STATUS_OF = {"repaired": "repaired", "checksum_mismatch": "unverified", "prefix": "partial"}


def c13(r):
    R_ = r["reproducible"]
    ss = R_["stream_salvage"]["summary"]
    streams = R_["stream_salvage"]["streams"]
    ok = ss["streams_damaged"] == (ss["repaired"] + ss["checksum_mismatch"] + ss["prefix"]
                                   + ss["unrecoverable"] + ss["unsearched"])
    ok &= ss["repaired"] == ss["exact"] + ss["accepted"] + ss["ambiguous"]
    ok &= ss["streams_damaged"] == len(streams)
    for k in ("repaired", "checksum_mismatch", "prefix", "unrecoverable", "unsearched"):
        ok &= ss[k] == sum(s["search"]["outcome"] == k for s in streams)
    for k in GRADE_KEY:
        ok &= ss[k] == sum(s["search"]["grade"] == k for s in streams)
    for st in streams:
        sr = st["search"]
        ok &= (sr["grade"] is not None) == (sr["outcome"] in ("repaired", "unsearched"))
        ok &= (sr["outcome"] == "unsearched") == (sr["grade"] == "unsearched")
        ok &= (sr["prefix"] is not None) == (sr["outcome"] == "prefix")
        ok &= sr["outcome"] != "unsearched" or not sr["passes"]
        for ps in sr["passes"]:
            ok &= ps["candidates_in_window"] == 255 * sum(w["end"] - w["start"] for w in ps["window"])
            ok &= ps["candidates_tried"] <= ps["candidates_in_window"]
            if ps["localizer"] == "error_window":
                k = ps["damage"]["at"]
                lo, hi = max(0, k - ERROR_WINDOW), k + SLACK
                want = [{"start": lo, "end": hi}]
                if ps["deep"]:
                    want.append({"start": max(0, k - DEEP), "end": lo})
                ok &= ps["window"] == want
            if ps["localizer"] == "adler_only":
                ok &= ps["window"] == [{"start": 2, "end": ps["damage"]["at"]}]
        if sr["outcome"] == "repaired":
            last = sr["passes"][-1] if sr["passes"] else None
            ok &= sr["distinct_outputs"] >= 1 and sr["passing_edits"] >= 1 and bool(sr["edits"])
            if sr["grade"] == "exact":
                ok &= sr["distinct_outputs"] == 1
                ok &= last is not None and last["candidates_tried"] == last["candidates_in_window"]
                ok &= last["check"] != "none_raw_deflate"
        else:
            ok &= not sr["edits"]
    img_by_obj = {im["source"]["object"]: im for im in R_["images"]["records"]}
    for st in streams:
        im = img_by_obj.get(st["object"])
        if im is not None:
            ok &= im["integrity"]["salvage"] == st["search"]
            want = STATUS_OF.get(st["search"]["outcome"])
            ok &= want is None or im["integrity"]["status"] == want
    return ok


report(13, "C9 counters add up; pass, grade and window rules; image status follows the stream",
       all(c13(r) for r in ALL))

# 14. a file holding tool-altered or unverified samples carries its status in its name
MARKED = ("repaired", "partial", "truncated", "unverified")
report(14, "repaired, partial, truncated and unverified image files are marked in their names", all(
    (im["integrity"]["status"] in MARKED)
    == (im["file"] is not None and f".{im['integrity']['status']}." in im["file"])
    for r in ALL for im in r["reproducible"]["images"]["records"]))
# 15. manifest lists only files PDFPundit wrote, never the input
man = open(os.path.join(EX, run["record_files"]["checksums"]), encoding="utf-8").read()
report(15, "checksum manifest does not list the input",
       run["input_file"]["name"] not in [l.split("  ", 1)[1] for l in man.splitlines()])
# 16. fixture: custody-off repair, then custody-on export; answers never read as given here
rs = fx["run"]["answers_replayed_from"]
ok16 = rs["type"] == "run_without_record" and rs["seqs"] == [q["seq"] for q in fx["reproducible"]["interactions"]]
ok16 &= fx["run"]["answer_times"] == []
b7 = open(os.path.join(EX, "fixtures", "export-after-custody-off.b7.txt")).read()
ok16 &= "no custody record; not given in this run" in b7.replace("\n", " ")
report(16, "fixture names the earlier run and says the answers were not given here", ok16)

# 17. log kinds: log_created with a nonce; log_continued with a reason; batch_cancelled with
#     counts only; batch_unclosed
Z = "0" * 64
le = sub("log_entry")
TOOLID = {"version": "0.1.0", "build_commit": None, "build_origin": "local"}
samples = {
    "log_created": {"seq": 1, "kind": "log_created", "time_utc": "2026-10-09T08:00:00Z", "prev": Z,
                    "tool": TOOLID, "nonce": "c" * 32},
    "log_continued": {"seq": 1, "kind": "log_continued", "time_utc": "2026-10-09T08:00:00Z",
                      "prev": Z, "reason": "head_mismatch", "previous_log_id": "a" * 64,
                      "previous_seq": 43, "previous_head_sha256": "b" * 64,
                      "known_from": "history", "tool": TOOLID},
    "batch_cancelled": {"seq": 44, "kind": "batch_cancelled", "time_utc": "2026-10-09T08:01:00Z",
                        "prev": Z, "prompt_shown": True, "files": 3, "received": 1},
    "verify": {"seq": 45, "kind": "verify", "time_utc": "2026-10-09T08:02:00Z", "prev": Z,
               "record_seq": 42, "record_json_sha256": "d" * 64, "verify_n": 1,
               "text_sha256": "e" * 64, "checks": {}, "head_after_record": "f" * 64},
    "batch_unclosed": {"seq": 2, "kind": "batch_unclosed", "time_utc": "2026-10-09T08:00:00Z",
                       "prev": Z, "batch_seq": 41, "files": 3, "records": 2, "failed": 0},
}
ok17 = all(check(le, v, f"sample {k}") for k, v in samples.items())
bad = [dict(samples["batch_cancelled"], files=[{"sha256": Z, "size_bytes": 1}]),
       {k: v for k, v in samples["log_created"].items() if k != "nonce"},
       dict(samples["batch_cancelled"], case_reference="2026-0412")]
ok17 &= all(list(Draft202012Validator(le).iter_errors(b)) for b in bad)
report(17, "log kinds accept their shapes and refuse hashes, a missing nonce or a case detail", ok17)

# 18. fixed text (m2, n6): standing, statements and this-run sentences are exactly the
#     templates, and the this-run list follows from Part B alone
ok18 = True
for r in ALL:
    R_ = r["reproducible"]
    ok18 &= R_["limitations"]["standing"] == T.STANDING
    ok18 &= R_["statements"] == T.statements(R_["tool"]["build_commit"])
    ok18 &= R_["limitations"]["this_run"] == T.this_run(R_)
report(18, "standing, statements and this-run text equal the templates computed from Part B", ok18)

# 19. settings_sha256 (n3) is the SHA-256 of Part B settings in compact form
report(19, "images.json settings_sha256 = SHA-256 of compact Part B settings",
       idx["settings_sha256"] == sha(compact(R["settings"])))


# 20. finding payloads (M5): detail matches kind; signed_fields agrees with the signed finding
def c20(r):
    R_ = r["reproducible"]
    ok = True
    for f in R_["findings_before"] + R_["findings_after"]:
        if f["kind"] in ("corruption", "encrypted"):
            ok &= f["detail"] is None
        else:
            ok &= f["detail"] is not None and f["detail"]["type"] == f["kind"]
        ok &= (f["class"] is not None) == (f["kind"] == "corruption")
    sig = sum(f["detail"]["fields"] for f in R_["findings_before"] if f["kind"] == "signed")
    return ok and sig == R_["input"]["signed_fields"]


report(20, "finding detail matches kind; signed_fields agrees", all(c20(r) for r in ALL))

# 21. earlier records (m3, m15): records in one log list every earlier record for the same input;
#     a reused images folder shows reused placements
refs = []
for r, name in ((rec, rec_name), (fx, "fixtures/invoice_scan_2024.md.custody.json")):
    refs.append((r["run"]["log"]["log_id"], r["run"]["log"]["seq"],
                 r["reproducible"]["input"]["sha256"], r))
ok21 = True
for log_id, seq, inp, r in refs:
    want = {(l, s) for l, s, i, _ in refs if l in r["run"]["earlier_records_logs"]["searched"]
            and i == inp and (l, s) != (log_id, seq) and (l != log_id or s < seq)}
    got = {(e["log_id"], e["log_seq"]) for e in r["run"]["earlier_records"]}
    ok21 &= want <= got
    ok21 &= r["run"]["earlier_records_logs"]["searched"][0] == log_id
    reused = r["run"]["images_dir"]["outcome"] == "reused_identical"
    imgs = [p for p in r["run"]["placed"] if p["name"].startswith(r["run"]["images_dir"]["name"] + "/")]
    ok21 &= all((p["placement"] == "reused_identical") == reused for p in imgs)
report(21, "earlier_records lists every earlier record of the same input in the logs searched", ok21)

# 22. no overclaiming engine text reaches Part B (M2, m5, m6)
BANNED = ["no other repair fits", "lossless", "losslessly", "the original is digitally signed",
          "forensically sound", "court-ready"]
partb = b"".join(compact(r["reproducible"]) for r in ALL).decode()
found = [w for w in BANNED if w in partb]
report(22, "Part B quotes none of the overclaiming or banned phrases", not found, found or "")

# 23. decision Q16: the destination is output_dir and never an input's own folder
ok23 = all(r["run"]["destination"]["path"] and (
    r["run"]["input_file"]["path"] is None
    or os.path.dirname(r["run"]["input_file"]["path"]) != r["run"]["destination"]["path"].rstrip("/"))
    for r in ALL)
report(23, "custody destination is set and is not the input's folder", ok23)

# 24. decision Q7: the UTC offset is filled, with its source, and the .txt prints it
c = run["clock"]
ok24 = all(r["run"]["clock"]["host_utc_offset_minutes"] is not None
           and r["run"]["clock"]["host_utc_offset_from"] is not None for r in ALL)
sign = "-" if c["host_utc_offset_minutes"] < 0 else "+"
m = abs(c["host_utc_offset_minutes"])
ok24 &= f"UTC{sign}{m // 60:02d}:{m % 60:02d} at Started" in txt
report(24, "host UTC offset filled with its source, and printed", ok24)

ok = all(results)
print("ALL OK" if ok else "FAILURES")
sys.exit(0 if ok else 1)
