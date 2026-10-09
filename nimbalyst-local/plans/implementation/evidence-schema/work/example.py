"""Worked example: one repaired file with two extracted images, custody mode on.

Writes into the directory given as argv[1]:
  invoice_scan_2024.repaired.pdf.custody.json
  invoice_scan_2024.repaired.pdf.custody.txt
  invoice_scan_2024.repaired.pdf.custody.sha256
  custody.log (three lines: entries 41 to 43)
  f7c39aa7.images/images.json, f7c39aa7.images/SHA256SUMS
  fixtures/ (the custody-off-then-export record, its B.7 lines, and the salvage-outcome record)
Hashes of the PDF and image files are stand-ins (no such files exist); every hash
that covers a file this script writes is computed from that file's real bytes.
Every finding summary, action and fixed sentence comes from templates.py, which quotes the
engine's own format strings at HEAD (revision 4, finding M2).
"""
import copy
import datetime
import hashlib
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import templates as T  # noqa: E402

OUT = sys.argv[1]


def h(label, algo="sha256"):
    return hashlib.new(algo, label.encode()).hexdigest()


def sha(b):
    return hashlib.sha256(b).hexdigest()


def compact(v):
    return json.dumps(v, ensure_ascii=False, separators=(",", ":"))


def pretty(v):
    return json.dumps(v, ensure_ascii=False, indent=2) + "\n"


ratio = lambda n, d: {"num": n, "den": d}
span = lambda a, b: {"start": a, "end": b}

INPUT_NAME = "invoice_scan_2024.pdf"
INPUT_SHA = h("input bytes")
HASH8 = INPUT_SHA[:8]
IMG_DIR = f"{HASH8}.images"
OUTPUT_NAME = "invoice_scan_2024.repaired.pdf"
PRIMARY = OUTPUT_NAME
# The input sits in an exhibit folder, mode 0444 on a writable volume (finding m4: a
# read-only mount would not update the access time the example shows changing). Outputs go
# to a separate work folder set by [general] output_dir.
INPUT_DIR = "/Volumes/EXHIBIT-JD3/exhibits"
CASE_DIR = "/Cases/2026-0412/work"

# Image 1: DCT, verbatim. raw bytes == file bytes, so raw_sha256 == file_sha256.
IMG1_SHA = h("jpeg bytes")
IMG1_FILE = f"{IMG1_SHA[:16]}.jpg"
# Image 2: Flate raster, repaired Exact, written as a PNG with the same sample values.
IMG2_SHA = h("png bytes")
IMG2_FILE = f"{IMG2_SHA[:16]}.repaired.png"
PDF_SHA = h("repaired pdf bytes")

# The C9 search on object 14, as salvage.rs ran it: one pass, zlib, an inflate error at
# offset 1,294 of the stream data (no filter before /FlateDecode), so the error window is
# [1294 - 4096, 1294 + 8) clamped to [0, 1302): 1,302 x 255 candidates. Exhausted under
# `work`, so no deep draw.
EDITS_14 = [[1207, 0x3a, 0x7a]]
SEARCH_14 = {
    "outcome": "repaired",
    "grade": "exact",
    "adler_rerun": False,
    "trailer_edit": False,
    "predictor_stopped": False,
    "prefix": None,
    "edit_domain": {"kind": "raw_stream_data", "filters_before": []},
    "passes": [{
        "mode": "zlib",
        "damage": {"kind": "inflate_error", "at": 1294},
        "localizer": "error_window",
        "check": "adler32",
        "deep": False,
        "window": [span(0, 1302)],
        "candidates_in_window": 332010,
        "candidates_tried": 332010,
    }],
    "passing_edits": 1,
    "distinct_outputs": 1,
    "edits": EDITS_14,
}


def description(w, h_, bpc, comps, cs):
    return {"width": w, "height": h_, "actual_width": w, "actual_height": h_,
            "bits_per_component": bpc, "components": comps, "colour_space": cs, "icc": None,
            "palette_entries": None, "decode": None, "image_mask": False, "interpolate": False,
            "intent": None, "needs_pdf_context": False}


def image(id_, file_, file_sha, file_len, fmt, obj, out_obj, raw_span, raw_sha, filters, parms,
          codec, desc, placements, integrity):
    return {
        "id": id_, "file": file_, "file_sha256": file_sha, "file_len": file_len, "format": fmt,
        "view_file": None, "view_sha256": None,
        "source": {"kind": "xobject", "copy": "winner", "object": obj, "output_object": out_obj,
                   "parent": None, "inline_origin": None,
                   "raw": {"domain": "file", "span": raw_span, "container_object": None,
                           "container_file_span": None},
                   "raw_len": raw_span["end"] - raw_span["start"], "raw_sha256": raw_sha,
                   "length_source": "declared"},
        "encoding": {"filters": filters, "decode_parms": parms, "codec": codec,
                     "jbig2_globals": None},
        "description": desc,
        "placements": placements,
        "integrity": integrity,
    }


def placement(page, order, bbox, matrix):
    return {"page_in": page, "page_out": page, "paint_order": order, "bbox_mpt": bbox,
            "matrix_micro": matrix, "form_chain": []}


def integrity(status, rows=None, salvage=None, repaired=None, samples=None):
    return {"status": status, "rows_written": rows, "rows_declared": rows, "salvage": salvage,
            "repaired_stream_sha256": repaired, "samples_sha256": samples, "warnings": []}


image1 = image("img-0001", IMG1_FILE, IMG1_SHA, 211876, "jpg", "9 0", "5 0",
               span(20311, 232187), IMG1_SHA, ["DCTDecode"], [None], "jpeg",
               description(1240, 1754, 8, 3, "/DeviceRGB"),
               [placement(1, 0, [36000, 51000, 559000, 791000],
                          [523000000, 0, 0, 740000000, 36000000, 51000000])],
               integrity("intact"))
image2 = image("img-0002", IMG2_FILE, IMG2_SHA, 9871, "png", "14 0", "8 0",
               span(233010, 251412), h("damaged flate stream bytes"), ["FlateDecode"],
               ["<</BitsPerComponent 8 /Colors 1 /Columns 600 /Predictor 15>>"], None,
               description(600, 200, 8, 1, "/DeviceGray"),
               [placement(2, 3, [400000, 60000, 550000, 110000],
                          [150000000, 0, 0, 50000000, 400000000, 60000000]),
                placement(3, 1, [400000, 60000, 550000, 110000],
                          [150000000, 0, 0, 50000000, 400000000, 60000000])],
               integrity("repaired", 200, SEARCH_14, h("repaired flate stream bytes"),
                         h("decoded gray samples")))
images = [image1, image2]

settings = {
    "salvage_work": 700000000,
    "salvage_deep_work": 10000000000,
    "salvage_deep_pool": 80000000000,
    "max_search_stream": 4194304,
    "passes": None,
    "font_policy": "bundled",
    "font_source_effective": "bundled",
    "prompt_unresolved": True,
    "derived_image_views": False,
    "unreproducible": "ask",
    "default_page_size": "a4",
    "extract_unplaceable_images": True,
    "extract_type3_glyph_images": False,
    "auto_accept_confidence": ratio(35, 100),
    "max_font_candidates": 5,
}
TOOL = {
    "name": "PDFPundit",
    "version": "0.1.0",
    "build_commit": h("commit", "sha1"),
    "build_origin": "release",
    "platform": {"os": "macos", "arch": "aarch64"},
    "report_schema_version": 2,
    "font_db_sha256": h("font db"),
}

# images.json. settings_sha256 is the SHA-256 of Part B's `settings` in compact form (n3).
images_index = {
    "format": "pdfpundit-images",
    "images_schema_version": 1,
    "input_sha256": INPUT_SHA,
    "tool": {"name": "PDFPundit", "version": "0.1.0"},
    "settings_sha256": sha(compact(settings).encode()),
    "type3_glyph_images": 0,
    "records": images,
}
images_index_bytes = pretty(images_index).encode()
sums_lines = sorted([(IMG1_FILE, IMG1_SHA), (IMG2_FILE, IMG2_SHA),
                     ("images.json", sha(images_index_bytes))])
sums_bytes = "".join(f"{d}  {n}\n" for n, d in sums_lines).encode()

products = [
    {"role": "repaired_pdf", "file": None, "sha256": PDF_SHA, "size_bytes": 489302},
    {"role": "image", "file": IMG1_FILE, "sha256": IMG1_SHA, "size_bytes": 211876},
    {"role": "image", "file": IMG2_FILE, "sha256": IMG2_SHA, "size_bytes": 9871},
    {"role": "images_index", "file": "images.json", "sha256": sha(images_index_bytes),
     "size_bytes": len(images_index_bytes)},
    {"role": "images_checksums", "file": "SHA256SUMS", "sha256": sha(sums_bytes),
     "size_bytes": len(sums_bytes)},
]


def evidence(top, conf, hit, margin, inf, tounicode, cov, cov_font, decision):
    return {"top_candidate": top, "confidence": ratio(*conf), "hit": ratio(*hit),
            "margin": ratio(*margin), "threshold": ratio(35, 100), "min_hit": ratio(1, 2),
            "inference": {"lang": inf[0], "script": inf[1], "n_chars": inf[2], "n_runs": inf[3]},
            "sample_floor": 20, "tounicode": tounicode, "best_coverage": ratio(*cov),
            "best_coverage_font": cov_font, "min_coverage": ratio(95, 100), "decision": decision}


# Three damaged Type0 Identity-H fonts named as Microsoft Print to PDF names them. For each:
# the Type0 font a page selects, its CIDFont, the descriptor, and the /FontFile2 it names,
# which is not a stream. C7 findings and actions sit on the descriptor (diagnose.rs 1604,
# fonts.rs 203); font_decisions name the Type0 font.
FONTS = [  # slot, page, Type0, descriptor, FontFile2, descriptor span
    ("F1", 1, "21 0", "23 0", (24, 0), span(252100, 252412)),
    ("F3", 2, "27 0", "29 0", (30, 0), span(255300, 255611)),
    ("F5", 3, "33 0", "35 0", (36, 0), span(258700, 259013)),
]


def finding(fid, kind, cls, sev, loc, summary, detail=None):
    return {"id": fid, "kind": kind, "class": cls, "severity": sev, "location": loc,
            "summary": summary, "detail": detail}


obj_loc = lambda o, s: {"type": "object", "object": o, "span": s}
findings_before = [
    finding("C2-001", "corruption", "C2", "error", {"type": "file"}, T.ENGINE["c2_summary"]),
    # Exact is a Warning in diagnose.rs 1727; the summary uses the corrected text (M2).
    finding("C9-001", "corruption", "C9", "warning", obj_loc("14 0", span(232950, 251431)),
            T.ENGINE_FIX["c9_exact_summary"].format(n=14, g=0)),
] + [
    finding(f"C7-00{i}", "corruption", "C7", "error", obj_loc(desc, sp),
            T.ENGINE["c7_summary"].format(name=f"CIDFont+{slot}", key="FontFile2",
                                          n=ff[0], g=ff[1]))
    for i, (slot, _, _, desc, ff, sp) in enumerate(FONTS, 1)
] + [
    # A page-1 font left out on purpose: Info, not damage (D-084). Its payload is kept (M5).
    finding("NOEMBED-001", "font_not_embedded", None, "info", obj_loc("41 0", span(262040, 262318)),
            T.ENGINE["noembed_summary"].format(name="ArialMT"),
            {"type": "font_not_embedded", "font": "40 0", "base_font": "ArialMT"}),
]
# Re-diagnosis of the output: object numbers and spans are the output's (4.3).
findings_after = [
    finding("NOEMBED-001", "font_not_embedded", None, "info", obj_loc("13 0", span(30411, 30690)),
            T.ENGINE["noembed_summary"].format(name="ArialMT"),
            {"type": "font_not_embedded", "font": "12 0", "base_font": "ArialMT"}),
]

verification = lambda gates, glyphs_unmapped: {
    "baseline": "extracted",
    "gates": {"lopdf_reload": True, "hayro_render_all_pages": True, "rediagnose_clean": gates,
              "page_count_ok": True, "root_and_pages_present": True},
    "retention": {"text_ops": ratio(1412, 1412), "path_fills": ratio(88, 88),
                  "images": ratio(2, 2), "content_bytes": ratio(61230, 61230),
                  "blank_pages": 0, "glyph_count": ratio(5120, 5120)},
    "plausibility": {"unmapped_glyph": ratio(glyphs_unmapped, 5120), "fffd": ratio(0, 5120),
                     "script_consistency": ratio(5120, 5120),
                     "dictionary_hit": ratio(902, 940)}}

reproducible = {
    "job": "repair",
    "tool": TOOL,
    "settings": settings,
    "input": {"sha256": INPUT_SHA, "size_bytes": 482113, "pdf_version": "1.6",
              "pages_discovered": 3, "signed_fields": 0, "encrypted": False},
    "interactions": [
        {"seq": 1, "kind": "font_pick", "page": 2, "slot": "F3",
         "candidates": ["dejavu-sans", "liberation-sans", "noto-sans"],
         "reply": {"type": "pick", "font_id": "dejavu-sans"}, "source": "user",
         "carried_from": None, "policy_setting": None},
        {"seq": 2, "kind": "font_pick", "page": 3, "slot": "F5",
         "candidates": ["dejavu-sans", "liberation-sans", "noto-sans"],
         "reply": {"type": "pick", "font_id": "dejavu-sans"}, "source": "batched",
         "carried_from": {"type": "this_record", "seq": 1}, "policy_setting": None},
    ],
    "font_decisions": [
        {"page": 1, "slot": "F1", "object": "21 0", "basis": "auto_accept",
         "font_id": "liberation-serif", "interaction_seq": None,
         "evidence": evidence("liberation-serif", (77, 100), (94, 100), (60, 100),
                              ("en", "Latn", 431, 37), "missing", (100, 100),
                              "liberation-serif", "auto_accept"),
         "reason": None},
        {"page": 2, "slot": "F3", "object": "27 0", "basis": "user",
         "font_id": "dejavu-sans", "interaction_seq": 1,
         "evidence": evidence("dejavu-sans", (25, 100), (48, 100), (2, 100),
                              ("en", "Latn", 212, 19), "missing", (97, 100),
                              "dejavu-sans", "ask"),
         "reason": None},
        {"page": 3, "slot": "F5", "object": "33 0", "basis": "batched",
         "font_id": "dejavu-sans", "interaction_seq": 2,
         "evidence": evidence("dejavu-sans", (26, 100), (51, 100), (1, 100),
                              ("en", "Latn", 188, 16), "missing", (97, 100),
                              "dejavu-sans", "ask"),
         "reason": None},
    ],
    # Fonts are substituted only on the TemplateAssemble toolpath (fonts.rs 118-130), so that
    # candidate is the chosen one.
    "outcome": {"status": "repaired", "reasons": [], "chosen_toolpath": "template_assemble"},
    "findings_before": findings_before,
    "findings_after": findings_after,
    "passes": [
        {"class": "C2", "outcome": "fixed", "note": None,
         # RepairAction.object is FILE, (0, 0), for a file-wide finding: null here (m7).
         "actions": [{"object": None, "what": T.ENGINE["reemitted"].format(fid="C2-001"),
                      "grade": None}]},
        {"class": "C7", "outcome": "fixed", "note": None,
         "actions": [{"object": desc, "what": T.ENGINE["font_substituted"].format(id=fid),
                      "grade": None}
                     for (_, _, _, desc, _, _), fid in zip(
                         FONTS, ["liberation-serif", "dejavu-sans", "dejavu-sans"])]},
        {"class": "C9", "outcome": "fixed", "note": None,
         "actions": [{"object": "14 0",
                      "what": T.ENGINE_FIX["c9_exact_action"].format(edits=T.edit_list(EDITS_14)),
                      "grade": "exact"}]},
    ],
    "candidates": [
        {"toolpath": "resave", "chosen": False, "verification": verification(True, 1840)},
        {"toolpath": "template_assemble", "chosen": True, "verification": verification(True, 0)},
    ],
    "stream_salvage": {
        "summary": {"streams_damaged": 1, "repaired": 1, "exact": 1, "accepted": 0,
                    "ambiguous": 0, "checksum_mismatch": 0, "prefix": 0, "unrecoverable": 0,
                    "unsearched": 0},
        "streams": [{"object": "14 0", "search": SEARCH_14}],
        "filter_rewrites": [],
    },
    "shadows": [],
    "reals_narrowed": 0,
    "text_warnings": [],
    "products": products,
    "images": {"dir": IMG_DIR, "type3_glyph_images": 0, "records": images},
    "limitations": {"standing": T.STANDING, "this_run": []},
    "statements": T.statements(TOOL["build_commit"]),
}
reproducible["limitations"]["this_run"] = T.this_run(reproducible)
STATEMENTS = reproducible["statements"]

repro_bytes = compact(reproducible).encode()
REPRO_SHA = sha(repro_bytes)

# ---- custody log: entry 41 (batch opened) is written before the job starts
PREV_40 = h("log entry 40")
LOG_ID = h("log entry 1: log_created")  # stand-in: entries 1 to 40 are not shown
LOG_PATH = "/Users/examiner/Library/Application Support/PDFPundit/custody.log"
TOOL_ID = {"version": "0.1.0", "build_commit": TOOL["build_commit"], "build_origin": "release"}
entry41 = {"seq": 41, "kind": "batch_opened", "time_utc": "2026-10-08T14:03:09Z",
           "prev": PREV_40, "files": 1, "prompt_shown": True, "tool": TOOL_ID}
line41 = compact(entry41)
PREV_42 = sha(line41.encode())

legacy = lambda label: {"sha256": h(label), "sha1": h(label, "sha1"), "md5": h(label, "md5")}

json_name = f"{PRIMARY}.custody.json"
txt_name = f"{PRIMARY}.custody.txt"
sums_name = f"{PRIMARY}.custody.sha256"


def ftime(iso, nsec):
    dt = datetime.datetime.strptime(iso, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)
    return {"sec": int(dt.timestamp()), "nsec": nsec}


STAT_BEFORE = {
    "size_bytes": 482113,
    "modified": ftime("2026-10-01T09:12:44Z", 0),
    "changed": ftime("2026-10-02T16:40:05Z", 120000000),
    "accessed": ftime("2026-10-07T18:22:31Z", 4000000),
    "created": ftime("2026-10-01T09:12:44Z", 0),
    "read_only": True,
    "unix_mode": 0o100444,
    "source": "unix_stat",
}
# The job's own read, after stat_before, moved the access time. The custody-mode gate means
# no earlier read by PDFPundit is folded into stat_before (M1).
STAT_AFTER = dict(STAT_BEFORE, accessed=ftime("2026-10-08T14:03:11Z", 402000000))


def placed_img(role, fname, digest, size, dest=CASE_DIR, placement_="create_new"):
    return {
        "role": role, "name": f"{IMG_DIR}/{fname}", "path": f"{dest}/{IMG_DIR}/{fname}",
        "size_bytes": size,
        "hashes": {"sha256": digest, "sha1": h(fname, "sha1"), "md5": h(fname, "md5")},
        "placement": placement_, "matches_product": True, "note": None,
    }


run = {
    "job_status": "completed",
    "batch": {"log_seq": 41, "position": 1, "count": 1},
    "case": {"prompt_shown": True, "case_reference": "2026-0412", "examiner": "J. Doe",
             "submission_reference": "Lab request 26-0412-07", "notes": "Authority: warrant 26-1187."},
    "clock": {"started_utc": "2026-10-08T14:03:11Z", "ended_utc": "2026-10-08T14:03:19Z",
              # Decision Q7: the offset at started_utc, from /etc/localtime through the app's
              # TZif reader (clock.rs). Toronto is on EDT, UTC-4, on 2026-10-08.
              "host_utc_offset_minutes": -240,
              "host_utc_offset_from": "etc_localtime_tzif",
              "host_zone_hint": {"name": "America/Toronto", "from": "etc_localtime_link"},
              "source": "host_clock_unverified"},
    # Finding m1: shown_utc is null for an answer nobody saw.
    "answer_times": [
        {"seq": 1, "shown_utc": "2026-10-08T14:03:13Z", "answered_utc": "2026-10-08T14:03:16Z"},
        {"seq": 2, "shown_utc": None, "answered_utc": "2026-10-08T14:03:16Z"},
    ],
    "host": {"name": "EXAM-WS-07", "os": "macos", "tool_binary_sha256": h("binary")},
    "custody_settings": {"hashes": ["sha256", "sha1", "md5"], "ask_case_details": True,
                         "require_case_details": False, "record_host_name": True,
                         "case_jsonld": False},
    "input_file": {
        "name": INPUT_NAME, "path": f"{INPUT_DIR}/{INPUT_NAME}", "path_kind": "file",
        "first_read": "job_start",
        "size_bytes": 482113,
        "hashes_before": legacy("input bytes"),
        "hashes_after": legacy("input bytes"),
        "unchanged": "yes", "not_rechecked_reason": None,
        "stat_before": STAT_BEFORE,
        "stat_after": STAT_AFTER,
        "stat_changed": [k for k in STAT_BEFORE if STAT_BEFORE[k] != STAT_AFTER[k]],
    },
    "destination": {"path": CASE_DIR},
    "placed": [
        {"role": "repaired_pdf", "name": OUTPUT_NAME, "path": f"{CASE_DIR}/{OUTPUT_NAME}",
         "size_bytes": 489302, "hashes": legacy("repaired pdf bytes"),
         "placement": "atomic", "matches_product": True, "note": None},
        placed_img("image", IMG1_FILE, IMG1_SHA, 211876),
        placed_img("image", IMG2_FILE, IMG2_SHA, 9871),
        placed_img("images_index", "images.json", sha(images_index_bytes), len(images_index_bytes)),
        placed_img("images_checksums", "SHA256SUMS", sha(sums_bytes), len(sums_bytes)),
    ],
    "images_dir": {"path": f"{CASE_DIR}/{IMG_DIR}", "name": IMG_DIR, "outcome": "created"},
    "processing": {"analysis_state": "reused", "rebuilt_reason": None,
                   "config_path": "/Users/examiner/Library/Application Support/PDFPundit/config.toml"},
    "record_files": {"json": json_name, "text": txt_name, "checksums": sums_name,
                     "case_jsonld": None},
    "earlier_records": [],
    "earlier_records_logs": {"searched": [LOG_ID], "missing": None},
    "answers_replayed_from": {"type": "none"},
    "log": {"path": LOG_PATH, "log_id": LOG_ID, "seq": 42, "prev_entry_sha256": PREV_42},
}
for p, b in ((run["placed"][3], images_index_bytes), (run["placed"][4], sums_bytes)):
    p["hashes"] = {"sha256": sha(b), "sha1": hashlib.sha1(b).hexdigest(),
                   "md5": hashlib.md5(b).hexdigest()}
# The image files' legacy hashes are stand-ins, as their bytes are.
for p, d in ((run["placed"][1], IMG1_SHA), (run["placed"][2], IMG2_SHA)):
    p["hashes"]["sha256"] = d

record = {
    "format": "pdfpundit-custody",
    "custody_schema_version": 1,
    "reproducible_sha256": REPRO_SHA,
    "reproducible": reproducible,
    "run": run,
}
record_bytes = pretty(record).encode()

# ---------------------------------------------------------------- text report

W = 80
PAGE_LINES = 60
HEAD_LINES = 4
RECORD_SHA = sha(record_bytes)


def kv(k, v, w=16):
    """Label in a w-column field; value wrapped at '/' (paths) or hard (hashes)."""
    room = W - w
    v = str(v)
    parts = []
    while len(v) > room:
        cut = v.rfind("/", 0, room) + 1 or room
        parts.append(v[:cut])
        v = v[cut:]
    parts.append(v)
    return "\n".join(f"{(k if i == 0 else ''):<{w}}{part}" for i, part in enumerate(parts))


def wrap(text, width=W, indent=""):
    words, out, line = text.split(), [], indent
    for wd in words:
        if len(line) + len(wd) + (0 if line.strip() == "" else 1) > width:
            out.append(line.rstrip())
            line = indent + wd
        else:
            line = (line + " " + wd) if line.strip() else indent + wd
    if line.strip():
        out.append(line.rstrip())
    return out


def kvw(k, text, w=16):
    """Label in a w-column field; value word-wrapped."""
    lines = wrap(text, width=W, indent=" " * w)
    first = f"{k:<{w}}" + lines[0][w:]
    return "\n".join([first] + lines[1:])


thousands = lambda n: f"{n:,}"
rule = lambda ch="-": ch * W
opt = lambda v: "not provided" if v is None or v == "" else v
utc = lambda s_: s_.replace("T", " ")
ratio_txt = lambda r: f"{r['num']}/{r['den']}"


def mins(off):
    s_ = "+" if off >= 0 else "-"
    return f"UTC{s_}{abs(off)//60:02d}:{abs(off)%60:02d}"


def ftime_txt(t):
    if t is None:
        return "not recorded"
    d = datetime.datetime.fromtimestamp(t["sec"], datetime.timezone.utc)
    return d.strftime("%Y-%m-%d %H:%M:%S") + f".{t['nsec']:09d}Z"


def ge(a, b):
    return a["num"] * b["den"] >= b["num"] * a["den"]


def refusals(ev):
    """Every reason resolve() refused the top candidate (n5)."""
    out = []
    if ev["top_candidate"] is None:
        return ["no font in the database is a candidate"]
    if not ge(ev["confidence"], ev["threshold"]):
        out.append(f"confidence {ratio_txt(ev['confidence'])} < {ratio_txt(ev['threshold'])}")
    if not ge(ev["hit"], ev["min_hit"]):
        out.append(f"hit {ratio_txt(ev['hit'])} < {ratio_txt(ev['min_hit'])}")
    if ev["inference"]["n_chars"] < ev["sample_floor"]:
        out.append(f"{ev['inference']['n_chars']} chars < {ev['sample_floor']}")
    return out


def who(q):
    cf = q["carried_from"]
    if q["source"] == "user":
        return "user, on screen"
    if q["source"] == "batched":
        src = f"#{cf['seq']}" if cf["type"] == "this_record" else "another file's answer"
        return f"carried from {src}, not shown"
    return f"setting {q['policy_setting']}, not shown"


def reply_txt(r):
    return {"pick": lambda: f"font {r['font_id']}", "use_best": lambda: "use best guess",
            "skip": lambda: "skip", "substitute": lambda: f"substitute {r.get('choice')}",
            "text_only": lambda: "text only"}[r["type"]]()


def question_lines(rep_, run_):
    """B.7 'Questions asked'. Replayed answers never read as given here (review 2, finding 3)."""
    out = ["Questions asked"]
    src = run_["answers_replayed_from"]
    replayed = set(src.get("seqs", []))
    if src["type"] == "custody_record":
        out += wrap(f"Answers marked * were given in an earlier run, recorded in {src['record']} "
                    f"(custody log entry {src['log_seq']}), and replayed here. They were not "
                    "given in this run.")
    elif src["type"] == "run_without_record":
        when = (f"started {utc(src['started_utc'])}" if src["started_utc"]
                else "whose start time was not kept")
        out += wrap(f"Answers marked * were given in an earlier run, {when}, that has no custody "
                    "record; not given in this run. They were replayed here.")
    times = {t["seq"]: t for t in run_["answer_times"]}
    out.append(f"{'#':<4}{'Page':<6}{'Slot':<6}{'Answer':<22}Answered by")
    for q in rep_["interactions"]:
        w_ = who(q)
        mark = "*" if q["seq"] in replayed else " "
        if q["seq"] in replayed:
            w_ += ", earlier run"
        t = times.get(q["seq"])
        if t is not None:
            w_ += f", {t['answered_utc'][11:19]}"
        out.append(f"{str(q['seq']) + mark:<4}{q['page']:<6}{q['slot']:<6}"
                   f"{reply_txt(q['reply']):<22}{w_}")
    if times:
        out.append("Times are UTC, when the answer was given in this run.")
    return out


def slot_basis(fd, inter):
    ev = fd["evidence"]
    if fd["basis"] == "named_match":
        return "matched by font name"
    if fd["basis"] == "unsupported":
        return f"not substituted: {fd['reason']}"
    tu = f"/ToUnicode {ev['tounicode']}"
    cov = f"coverage {ratio_txt(ev['best_coverage'])} (needs {ratio_txt(ev['min_coverage'])})"
    if fd["basis"] == "auto_accept":
        return (f"chosen without asking: confidence {ratio_txt(ev['confidence'])} >= "
                f"{ratio_txt(ev['threshold'])}, hit {ratio_txt(ev['hit'])} >= "
                f"{ratio_txt(ev['min_hit'])}, {ev['inference']['n_chars']} chars >= "
                f"{ev['sample_floor']}; {cov}; {tu}")
    refused = " and ".join(refusals(ev))
    decodes = ev["tounicode"] == "present"
    cov_txt = cov if decodes else f"{tu}, so coverage not required"
    q = inter[fd["interaction_seq"]]
    if q["source"] == "user":
        return f"answer #{q['seq']}; asked because {refused}; {cov_txt}"
    # A carried or policy answer: nobody was asked (finding m1).
    origin = who(q).replace(", not shown", "")
    return (f"answer #{q['seq']}, not asked: {origin}; the score alone would have asked, "
            f"{refused}; {cov_txt}")


LOCALIZER_TXT = {
    "error_window": "error window: 4,096 bytes before the inflate error to 8 after",
    "adler_only": "whole body: from byte 2 to the Adler-32 trailer",
    "ttf_tables": "font tables: bytes before the first failing sfnt table, less verified tables",
    "widened": "widened: the font-table positions again, then the rest of the whole body",
}
DEEP_TXT = {"error_window": "; deep draw added 16,384 bytes before the error",
            "adler_only": "; deep draw, same window", "ttf_tables": "; deep draw, same window",
            "widened": "; deep draw"}
CHECK_TXT = {"adler32": "Adler-32",
             "adler32_after_ttf_table_checksum": "font table checksum, then Adler-32",
             "none_raw_deflate": "nothing (raw deflate has no Adler-32)"}
DAMAGE_TXT = {"inflate_error": "inflate error", "adler_mismatch": "Adler-32 mismatch",
              "header_rejected": "zlib header rejected"}
OUTCOME_TXT = {"repaired": "repaired", "checksum_mismatch": "written decoded, unverified",
               "prefix": "cut short", "unrecoverable": "written as found",
               "unsearched": "written as found, not searched"}


def search_lines(sr, raw_start):
    out = []
    dom = sr["edit_domain"]
    in_raw = dom["kind"] == "raw_stream_data"
    where = "stream data" if in_raw else "Flate input"
    for i, ps in enumerate(sr["passes"], 1):
        mode = "zlib" if ps["mode"] == "zlib" else "raw deflate"
        out.append(kvw(f"  Pass {i}", f"{mode}; {DAMAGE_TXT[ps['damage']['kind']]} at offset "
                                      f"{thousands(ps['damage']['at'])} of the {where}"))
        out.append(kvw("  Window", LOCALIZER_TXT[ps["localizer"]]
                       + (DEEP_TXT[ps["localizer"]] if ps["deep"] else "")))
        win = "; ".join(f"bytes {thousands(w['start'])} to {thousands(w['end'])}"
                        for w in ps["window"])
        out.append(kvw("  Searched", f"{win} of the {where}; single-byte replacements tried "
                                     f"{thousands(ps['candidates_tried'])} of "
                                     f"{thousands(ps['candidates_in_window'])}"))
        out.append(kv("  Check", CHECK_TXT[ps["check"]]))
    if sr["passes"]:
        out.append(kvw("  Passed check", f"{sr['passing_edits']} replacement"
                                         f"{'' if sr['passing_edits'] == 1 else 's'}, "
                                         f"{sr['distinct_outputs']} distinct output"
                                         f"{'' if sr['distinct_outputs'] == 1 else 's'}"))
    for off_, frm, to in sr["edits"]:
        loc_txt = (f"offset {thousands(off_)} in the stream data (file offset "
                   f"{thousands(raw_start + off_)})" if in_raw and raw_start is not None
                   else f"offset {thousands(off_)} in the Flate input, after "
                        f"{' '.join(dom['filters_before'])}")
        out.append(kvw("  Edit", f"{loc_txt}, 0x{frm:02x} -> 0x{to:02x}"))
    if sr["trailer_edit"]:
        out.append(kvw("", "the stored Adler-32 was rewritten to match the decoded data; no "
                           "checksum covers that data"))
    if sr["prefix"]:
        out.append(kvw("  Kept", f"what decodes before input byte "
                                 f"{thousands(sr['prefix']['in_used'])} of "
                                 f"{thousands(sr['prefix']['in_total'])}; the rest is lost"))
    return out


def render_txt(record, json_name, record_sha):
    R_, run = record["reproducible"], record["run"]
    STATEMENTS = R_["statements"]
    images = R_["images"]["records"]
    body = []
    add, ext = body.append, body.extend

    ext(wrap(STATEMENTS["role"]))
    add("")
    ext(wrap("Part A, run context, is true of this run only. Part B, reproducible result, "
             "is the same on every re-run with the same input, build, settings and answers. "
             "Abbreviations are listed in part C. JSON SHA-256 in the page header is the "
             "SHA-256 of the record file this text was rendered from. That file is written "
             "first, so the hash ties every printed page to one machine record."))
    add("")
    add("A. RUN CONTEXT")
    add(rule())
    c = run["case"]
    add(kv("Case reference", opt(c["case_reference"])))
    add(kv("Examiner", opt(c["examiner"])))
    n_batch = run["batch"]["count"]
    add(kv("Submission ref", opt(c["submission_reference"])))
    add(kvw("", ("applies to all " + f"{n_batch} files" if n_batch > 1 else "applies to the 1 file")
                + " of this batch; the item itself is identified by its input SHA-256"))
    add(kv("Authority/notes", opt(c["notes"])))
    add(kv("", "Typed at the custody prompt; not verified by PDFPundit."))
    add(kv("Batch", f"custody log entry {run['batch']['log_seq']}, "
                    f"file {run['batch']['position']} of {run['batch']['count']}"))
    o = run["input_file"]
    # Finding M1: what "Started" marks depends on how the input arrived.
    started_txt = {"job_start": "before the job first read the input's file state",
                   "drop": "after the input was received whole at the drop"}[o["first_read"]]
    add(kvw("Started (UTC)", utc(run["clock"]["started_utc"]) + ", " + started_txt))
    add(kvw("Ended (UTC)", utc(run["clock"]["ended_utc"]) + ", before the record was written"))
    off = run["clock"]["host_utc_offset_minutes"]
    off_from = {"etc_localtime_tzif": "from the /etc/localtime zone file",
                "windows_local_file_time": "from Windows local time"}
    add(kvw("Host UTC offset", "not recorded" if off is None else
            f"{mins(off)} at Started, {off_from[run['clock']['host_utc_offset_from']]}"))
    zh = run["clock"]["host_zone_hint"]
    if zh:
        src = {"etc_localtime_link": "/etc/localtime link",
               "windows_time_zone_info": "Windows time zone setting"}[zh["from"]]
        add(kv("Host zone name", f"{zh['name']} (from {src}; unverified)"))
    os_txt = {"macos": "macOS", "windows": "Windows", "linux": "Linux"}[run["host"]["os"]]
    add(kv("Host", f"{opt(run['host']['name'])} ({os_txt})"))
    add(kv("Binary SHA-256", run["host"]["tool_binary_sha256"]))
    add("")
    add("A.1 Input file (as received by PDFPundit)")
    ext(wrap("PDFPundit cannot tell whether this file is the original evidence or a working copy."))
    add(kv("Name", o["name"]))
    add(kv("Path", opt(o["path"])))
    received = {
        "file": "by path; first opened by the job, after the file state below was read",
        "dropped": "sent whole by the terminal at the drop, before the custody prompt; "
                   "the file state below was read later, at job start",
        "memory": "as bytes with no path, at the drop; no file state exists"}[o["path_kind"]]
    add(kvw("Received", received))
    add(kv("Size", f"{thousands(o['size_bytes'])} bytes"))
    add(kv("SHA-256 before", o["hashes_before"]["sha256"]))
    add(kv("SHA-256 after", o["hashes_after"]["sha256"]))
    add(kv("MD5 legacy", o["hashes_before"]["md5"]))
    add(kv("SHA-1 legacy", o["hashes_before"]["sha1"]))
    add(kv("Content", "UNCHANGED. SHA-256 before equals SHA-256 after."))
    add(kv("Opened", "read-only; PDFPundit never writes to its input"))
    add("")
    sb, sa = o["stat_before"], o["stat_after"]
    add(f"{'File state':<16}{'before':<32}after")
    for label, key in (("  Modified", "modified"), ("  Changed", "changed"),
                       ("  Accessed", "accessed"), ("  Created", "created")):
        b_, a_ = ftime_txt(sb[key]), ftime_txt(sa[key])
        add(f"{label:<16}{b_:<32}{'same' if sb[key] == sa[key] else a_}")
    add(f"{'  Read-only':<16}{('yes' if sb['read_only'] else 'no'):<32}"
        f"{'same' if sb['read_only'] == sa['read_only'] else ('yes' if sa['read_only'] else 'no')}")
    add(f"{'  Mode':<16}{oct(sb['unix_mode'] & 0o7777)[2:]:<32}"
        f"{'same' if sb['unix_mode'] == sa['unix_mode'] else oct(sa['unix_mode'] & 0o7777)[2:]}")
    diff = o["stat_changed"]
    if diff:
        add(kvw("Differences", f"{', '.join(diff)}. Reading a file can update its access time. "
                               "The content hash above is unaffected."))
    else:
        add(kv("Differences", "none"))
    add("")
    add("A.2 Files written")
    dst = run["destination"]
    add(kv("Destination", dst["path"]))
    add(kvw("", "set by output_dir, which custody mode requires; never an input's own folder"))
    add("")
    for i, p in enumerate(run["placed"], 1):
        if i > 1:
            add("")
        add(kv(f"File {i}", p["name"]))
        add(kv("  Kind", p["role"].replace("_", " ")))
        add(kv("  Size", f"{thousands(p['size_bytes'])} bytes"))
        add(kv("  SHA-256", p["hashes"]["sha256"]))
        add(kv("  MD5 legacy", p["hashes"]["md5"]))
        add(kv("  SHA-1 legacy", p["hashes"]["sha1"]))
        add(kv("  Check", "re-read after writing; matches part B"))
    add(kv("Folder", f"{run['images_dir']['path']} "
                     f"({run['images_dir']['outcome'].replace('_', ' ')})"))
    add("")
    add("B. REPRODUCIBLE RESULT")
    add(rule())
    add(kv("Part B SHA-256", record["reproducible_sha256"]))
    ext(wrap("Part B SHA-256 is the hash of the 'reproducible' member of the JSON record in "
             "compact form. A re-run that gives the same hash gave the same result."))
    add("")
    add("B.1 Tool")
    t = R_["tool"]
    add(kv("Tool", f"PDFPundit {t['version']}"))
    add(kv("Build commit", t["build_commit"] or "not recorded"))
    add(kv("Build origin", f"{t['build_origin']} build, as reported by the program"))
    os_t = {"macos": "macOS", "windows": "Windows", "linux": "Linux"}[t["platform"]["os"]]
    add(kv("Platform", f"{os_t}, {t['platform']['arch']}; part of the build for re-runs"))
    add(kv("Font DB SHA-256", t["font_db_sha256"]))
    add(kv("Report schema", f"version {t['report_schema_version']}; custody schema version 1"))
    add("")
    add("B.2 Settings")
    s = R_["settings"]
    add(kv("Passes", "all (C1 to C10)" if s["passes"] is None else ", ".join(s["passes"])))
    add(kv("Salvage work", f"{thousands(s['salvage_work'])} (deep {thousands(s['salvage_deep_work'])}, "
                           f"pool {thousands(s['salvage_deep_pool'])})"))
    add(kv("Search limit", f"{thousands(s['max_search_stream'])} bytes"))
    add(kv("Font source", s["font_policy"] + (
        "" if s["font_source_effective"] == s["font_policy"]
        else f"; in effect: {s['font_source_effective']}")))
    add(kv("Font questions", "asked on screen" if s["prompt_unresolved"]
           else "answered by settings: use best"))
    add(kv("Image views", "derived PNG views written" if s["derived_image_views"] else "none"))
    add(kv("No-match fonts", s["unreproducible"]))
    add(kv("Page size", s["default_page_size"]))
    add(kv("Unplaced images", "extracted" if s["extract_unplaceable_images"] else "not extracted"))
    add(kv("Type3 glyphs", "extracted" if s["extract_type3_glyph_images"] else "counted only"))
    add(kv("Font auto-pick", f"confidence {s['auto_accept_confidence']['num']}/"
                               f"{s['auto_accept_confidence']['den']} or higher"))
    add(kv("Font candidates", f"at most {s['max_font_candidates']}"))
    add("")
    add("B.3 Input and outcome")
    add(kv("Input SHA-256", R_["input"]["sha256"]))
    add(kv("Input size", f"{thousands(R_['input']['size_bytes'])} bytes, PDF "
                         f"{R_['input']['pdf_version']}, {R_['input']['pages_discovered']} pages found"))
    add(kv("Outcome", R_["outcome"]["status"].upper()))
    chosen = [cd for cd in R_["candidates"] if cd["chosen"]][0]
    add(kvw("Method", f"{chosen['toolpath'].replace('_', ' ')}, chosen over "
                      + ", ".join(cd["toolpath"].replace("_", " ")
                                  for cd in R_["candidates"] if not cd["chosen"])))
    prod = {p_["role"]: p_ for p_ in R_["products"]}
    add(kv("Output SHA-256", prod["repaired_pdf"]["sha256"]))
    add(kv("Output checks", "reloads; every page renders; re-diagnosis clean;"))
    add(kv("", "page count matches; catalog and page tree present"))
    ret = chosen["verification"]["retention"]
    add(kvw("Kept", "compared with what was readable in the damaged input, not with the "
                    "undamaged document (baseline: extracted)"))
    add(kv("", f"text operators {thousands(ret['text_ops']['num'])}/{thousands(ret['text_ops']['den'])}; "
               f"images {ret['images']['num']}/{ret['images']['den']}; glyphs "
               f"{thousands(ret['glyph_count']['num'])}/{thousands(ret['glyph_count']['den'])}"))
    add("")
    add("B.4 Findings")
    ext(wrap("Objects are named as in the input. A finding still present after the repair is "
             "named as in the output."))
    add(f"{'ID':<13}{'Class':<7}{'Severity':<10}{'Where':<11}Before -> after")
    after = {(f["kind"], f["summary"]): f for f in R_["findings_after"]}
    for f in R_["findings_before"]:
        loc = f["location"]
        where = "file" if loc["type"] == "file" else f"obj {loc['object']}"
        still = after.get((f["kind"], f["summary"]))
        fate = "found -> gone" if still is None else (
            f"found -> found, obj {still['location']['object']}")
        add(f"{f['id']:<13}{(f['class'] or '-'):<7}{f['severity']:<10}{where:<11}{fate}")
        ext(wrap(f["summary"], indent=" " * 13))
    add("")
    add("B.5 Repair actions")
    add(f"{'Pass':<6}{'Object':<9}{'Grade':<8}Action")
    for p in R_["passes"]:
        for a in p["actions"]:
            first, *rest = wrap(a["what"], width=W - 23)
            add(f"{p['class']:<6}{(a['object'] or 'file'):<9}{(a['grade'] or '-'):<8}{first}")
            for r in rest:
                add(" " * 23 + r)
    add("")
    add("B.6 Compressed-stream salvage")
    ss = R_["stream_salvage"]["summary"]
    add(kvw("Damaged streams", f"{ss['streams_damaged']}: repaired {ss['repaired']} "
                               f"(Exact {ss['exact']}, Accepted {ss['accepted']}, "
                               f"Ambiguous {ss['ambiguous']}); unverified "
                               f"{ss['checksum_mismatch']}; cut short {ss['prefix']}; "
                               f"unrecoverable {ss['unrecoverable']}; not searched "
                               f"{ss['unsearched']}"))
    img_by_obj = {im["source"]["object"]: im for im in images}
    for st in R_["stream_salvage"]["streams"]:
        sr = st["search"]
        grade = f", graded {sr['grade'].capitalize()}" if sr["grade"] else ", no grade"
        add(kv(f"obj {st['object']}", OUTCOME_TXT[sr["outcome"]] + grade))
        im = img_by_obj.get(st["object"])
        raw_start = (im["source"]["raw"]["span"]["start"]
                     if im and im["source"]["raw"]["domain"] == "file" else None)
        ext(search_lines(sr, raw_start))
    add("")
    add("B.7 Font slots and answers")
    ext(question_lines(R_, run))
    add("")
    add("Every font slot")
    add(f"{'Page':<6}{'Slot':<6}{'Object':<8}{'Font':<19}Basis")
    inter = {q["seq"]: q for q in R_["interactions"]}
    for fd in R_["font_decisions"]:
        lines = wrap(slot_basis(fd, inter), width=W - 39)
        add(f"{fd['page']:<6}{fd['slot']:<6}{fd['object']:<8}{fd['font_id'] or '-':<19}{lines[0]}")
        for l in lines[1:]:
            add(" " * 39 + l)
    add("")
    add("B.8 Extracted images")
    for im in images:
        d, src, ig = im["description"], im["source"], im["integrity"]
        if im is not images[0]:
            add("")
        add(kv(f"{im['id']}", f"{R_['images']['dir']}/{im['file']}"))
        add(kv("  Source", f"obj {src['object']} (obj {src['output_object']} in repaired PDF), "
                             f"{src['kind']}"))
        add(kv("  Pages", ", ".join(str(p["page_out"]) for p in im["placements"])))
        add(kv("  Filters", " ".join(im["encoding"]["filters"])))
        add(kv("  Image", f"{d['width']} x {d['height']}, {d['bits_per_component']} bit, "
                            f"{d['colour_space']}"))
        if src["raw"]["domain"] == "file":
            add(kv("  In input file", f"offset {thousands(src['raw']['span']['start'])}, "
                                     f"{thousands(src['raw_len'])} bytes"))
        else:
            add(kvw("  In input file", f"bytes {thousands(src['raw']['span']['start'])} on of the "
                                       f"decoded content stream obj {src['raw']['container_object']}; "
                                       "not a span of the file"))
        add(kv("  Raw SHA-256", src["raw_sha256"]))
        add(kv("  File SHA-256", im["file_sha256"]))
        sv = ig["salvage"]
        status = ig["status"] if sv is None or not sv["grade"] else f"{ig['status']}, graded {sv['grade']}"
        add(kv("  Status", status))
        if sv is not None:
            add(kv("  Search", f"see B.6, obj {src['object']}"))
        if src["raw_sha256"] == im["file_sha256"]:
            add(kv("  File holds", "the stream bytes as found, unchanged"))
        elif ig["status"] == "intact":
            add(kvw("  File holds", "decoded samples, re-encoded as PNG with the same sample values"))
        else:
            add(kvw("  File holds", f"decoded samples after PDFPundit's salvage, as PNG; not an "
                                    f"as-found extraction. The name says '{ig['status']}'"))
    add("")
    add("B.9 Limitations")
    add("Standing limits of this version:")

    def bullet(text):
        lines = wrap(text, indent="  ")
        return ["- " + lines[0][2:]] + lines[1:]

    for l in R_["limitations"]["standing"]:
        ext(bullet(l))
    add("Limits of this run:")
    for l in R_["limitations"]["this_run"]:
        ext(bullet(l))
    add("")
    add("B.10 Statements")
    for k in ["reproducibility", "validation", "tool_identity", "input_file", "case_details",
              "times", "log", "legacy_hashes"]:
        ext(wrap(STATEMENTS[k]))
        add("")
    add("C. ABBREVIATIONS")
    add(rule())
    for a, d in [("C1..C10", "the ten damage classes PDFPundit repairs; listed in its manual"),
                 ("Exact", "every single-byte replacement in the searched window was tried, and "
                           "every one that passed the check gave the same output. Damage of other "
                           "kinds is not searched. The grade does not show the repaired bytes equal "
                           "the original bytes"),
                 ("Accepted", "the work limit ran out before every single-byte replacement in the "
                              "window was tried; every one that passed the check up to then gave "
                              "the same output"),
                 ("Ambiguous", "replacements that passed the check gave two or more different "
                               "outputs, or the stream is raw deflate with no Adler-32; a fixed "
                               "order chose one"),
                 ("unverified", "decoded bytes no checksum confirms; written decoded, with no grade"),
                 ("obj N G", "PDF object number N, generation G"),
                 ("SHA-256", "integrity hash; MD5 and SHA-1 are legacy hashes"),
                 ("resave", "one of two ways PDFPundit builds an output: the damaged file "
                            "re-written; see its manual"),
                 ("template assemble", "the other way: the same, with damaged font programs "
                                       "substituted from the bundled fonts"),
                 ("UTC", "Coordinated Universal Time")]:
        if len(a) >= 12:
            add(a)
            ext(wrap(d, indent=" " * 12))
        else:
            add(kvw(a, d, 12))
    add("")
    add("D. CUSTODY LOG AND SIGN-OFF")
    add(rule())
    add(kv("Custody log", run["log"]["path"]))
    add(kv("Log identity", run["log"]["log_id"]))
    add(kv("This record", f"entry {run['log']['seq']}"))
    add("Log entry before this record")
    add(kv("  Entry", f"{run['log']['seq'] - 1}, batch opened"))
    add(kv("  SHA-256", run["log"]["prev_entry_sha256"]))
    ext(wrap("This page cannot show the log head after this record. The log entry for this "
             "record holds this page's hash, so it is written after this page. When the batch "
             "closes, PDFPundit shows the log head on screen, and Verify prints it later. Copy it "
             "into your notes. It pins the log, this record included, at that point."))
    add("")
    add("Reviewed by (name): ______________________________________________")
    add("")
    add("Signature: ___________________________   Date: ____________________")

    def paginate(lines):
        room = PAGE_LINES - HEAD_LINES
        blocks, cur = [], []
        for l in lines + [""]:
            cur.append(l)
            if l == "":
                blocks.append(cur)
                cur = []
        pages, page = [], []
        for b in blocks:
            if len(page) + len(b) > room and page:
                pages.append(page)
                page = []
            while len(b) > room:
                pages.append(b[:room])
                b = b[room:]
            page.extend(b)
        if page:
            pages.append(page)
        pages = [pg[:-1] if pg and pg[-1] == "" else pg for pg in pages]
        out = []
        for n, pg in enumerate(pages, 1):
            left = "PDFPUNDIT TOOL RUN RECORD"
            right = f"Page {n} of {len(pages)}"
            head = left + " " * (W - len(left) - len(right)) + right
            out.append(("\f" if n > 1 else "") + head)
            out.append(f"Record {json_name}")
            out.append(kv("JSON SHA-256", record_sha))
            out.append(rule("="))
            out.extend(pg)
        return "\n".join(out) + "\n"

    body = [x for l in body for x in l.split("\n")]
    txt = paginate(body)
    over = [l for l in txt.split("\n") if len(l.lstrip("\f")) > W]
    assert not over, over
    return txt


txt = render_txt(record, json_name, RECORD_SHA)
txt_bytes = txt.encode()


# -------------------------------------------------------- checksums manifest
def gnu_line(digest, name):
    """GNU untagged format; a name with backslash, LF or CR gets a leading backslash."""
    if any(c in name for c in "\\\n\r"):
        esc = name.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r")
        return f"\\{digest}  {esc}\n"
    return f"{digest}  {name}\n"


assert gnu_line("ab", "a\\b\nc") == "\\ab  a\\\\b\\nc\n"
manifest = [
    (OUTPUT_NAME, PDF_SHA),
    (f"{IMG_DIR}/{IMG1_FILE}", IMG1_SHA),
    (f"{IMG_DIR}/{IMG2_FILE}", IMG2_SHA),
    (f"{IMG_DIR}/images.json", sha(images_index_bytes)),
    (f"{IMG_DIR}/SHA256SUMS", sha(sums_bytes)),
    (json_name, sha(record_bytes)),
    (txt_name, sha(txt_bytes)),
]
manifest_bytes = "".join(gnu_line(d, n) for n, d in sorted(manifest, key=lambda x: x[0].encode())).encode()

# Log entries hold hashes, counts and outcomes only: no case details, names or paths.
entry42 = {
    "seq": 42, "kind": "record", "time_utc": "2026-10-08T14:03:19Z", "prev": PREV_42,
    "batch_seq": 41, "input_sha256": INPUT_SHA, "outcome": "repaired", "input_unchanged": "yes",
    "files": [
        {"role": "json", "sha256": sha(record_bytes)},
        {"role": "text", "sha256": sha(txt_bytes)},
        {"role": "checksums", "sha256": sha(manifest_bytes)},
    ],
}
line42 = compact(entry42)
entry43 = {"seq": 43, "kind": "batch_closed", "time_utc": "2026-10-08T14:03:20Z",
           "prev": sha(line42.encode()), "batch_seq": 41, "records": 1, "failed": 0}
line43 = compact(entry43)
log_bytes = (line41 + "\n" + line42 + "\n" + line43 + "\n").encode()

os.makedirs(os.path.join(OUT, IMG_DIR), exist_ok=True)
for name, b in [(json_name, record_bytes), (txt_name, txt_bytes), (sums_name, manifest_bytes),
                ("custody.log", log_bytes), (f"{IMG_DIR}/images.json", images_index_bytes),
                (f"{IMG_DIR}/SHA256SUMS", sums_bytes)]:
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(b)

print("reproducible_sha256", REPRO_SHA)
print("record json sha256", RECORD_SHA)
print("head after record (entry 42)", sha(line42.encode()))
print("head after batch (entry 43)", sha(line43.encode()))
print("record bytes", len(record_bytes), "txt lines", txt.count("\n"), "pages", txt.count("\f") + 1)

# ------------------------------------------------ fixture: review 2 finding 3, review 3 m3
# The examiner repaired with custody off and answered the font questions, then switched
# custody on and exported Markdown. The export replays the earlier answers. Part B keeps
# source "user"; Part A says the answers were given in an earlier run with no record.
# Finding m3: this is a different case on another workstation, with its own custody log and
# its own work folder, so its empty earlier_records and its created images folder are true.
os.makedirs(os.path.join(OUT, "fixtures"), exist_ok=True)
fx = copy.deepcopy(record)
fr, fa = fx["reproducible"], fx["run"]
MD_NAME = "invoice_scan_2024.md"
MD_SHA = h("markdown bytes")
FX_DIR = "/Cases/2026-0398/work"
FX_LOG_ID = h("fixture log, line 1: log_created")
fr["job"] = "export"
fr["products"] = [{"role": "markdown", "file": None, "sha256": MD_SHA, "size_bytes": 18233}] + [
    p for p in fr["products"] if p["role"] != "repaired_pdf"]
fx["reproducible_sha256"] = sha(compact(fr).encode())
fa["batch"] = {"log_seq": 7, "position": 1, "count": 1}
fa["case"] = dict(fa["case"], case_reference="2026-0398", submission_reference=None,
                  notes=None)
fa["clock"] = dict(fa["clock"], started_utc="2026-10-08T15:10:02Z", ended_utc="2026-10-08T15:10:09Z")
fa["answer_times"] = []
fa["answers_replayed_from"] = {"type": "run_without_record", "input_sha256": INPUT_SHA,
                               "started_utc": "2026-10-08T13:41:52Z", "history_run": True,
                               "seqs": [1, 2]}
fa["destination"] = dict(fa["destination"], path=FX_DIR)
fa["placed"] = [{"role": "markdown", "name": MD_NAME, "path": f"{FX_DIR}/{MD_NAME}",
                 "size_bytes": 18233, "hashes": legacy("markdown bytes"), "placement": "atomic",
                 "matches_product": True, "note": None}] + [
    dict(p, path=p["path"].replace(CASE_DIR, FX_DIR))
    for p in fa["placed"] if p["role"] != "repaired_pdf"]
fa["images_dir"] = {"path": f"{FX_DIR}/{IMG_DIR}", "name": IMG_DIR, "outcome": "created"}
fa["record_files"] = {"json": f"{MD_NAME}.custody.json", "text": f"{MD_NAME}.custody.txt",
                      "checksums": f"{MD_NAME}.custody.sha256", "case_jsonld": None}
fa["earlier_records"] = []
fa["earlier_records_logs"] = {"searched": [FX_LOG_ID], "missing": None}
fa["log"] = {"path": "/Users/examiner2/Library/Application Support/PDFPundit/custody.log",
             "log_id": FX_LOG_ID, "seq": 8, "prev_entry_sha256": h("fixture log, line 7")}
fa["host"] = dict(fa["host"], name="EXAM-WS-11")
with open(os.path.join(OUT, "fixtures", f"{MD_NAME}.custody.json"), "w") as f_:
    f_.write(pretty(fx))
with open(os.path.join(OUT, "fixtures", "export-after-custody-off.b7.txt"), "w") as f_:
    f_.write("\n".join(question_lines(fr, fa)) + "\n")

# ------------------------------------------------ fixture: review 3 M3
# The same record with two more damaged image streams: one that decodes but fails its
# Adler-32 with no repair (ChecksumMismatch) and one cut short (Prefix). Part B only matters
# here; validate.py checks the counters add up, the names carry the status and the this-run
# sentences follow from Part B.
sx = copy.deepcopy(record)
sr_ = sx["reproducible"]
CM_SHA, PX_SHA = h("unverified png bytes"), h("prefix png bytes")
search_cm = {
    "outcome": "checksum_mismatch", "grade": None, "adler_rerun": False, "trailer_edit": False,
    "predictor_stopped": False, "prefix": None,
    "edit_domain": {"kind": "raw_stream_data", "filters_before": []},
    "passes": [{"mode": "zlib", "damage": {"kind": "adler_mismatch", "at": 30871},
                "localizer": "adler_only", "check": "adler32", "deep": True,
                "window": [span(2, 30871)], "candidates_in_window": 255 * 30869,
                "candidates_tried": 255 * 30869}],
    "passing_edits": 0, "distinct_outputs": 0, "edits": [],
}
search_px = {
    "outcome": "prefix", "grade": None, "adler_rerun": False, "trailer_edit": False,
    "predictor_stopped": False, "prefix": {"in_used": 6120, "in_total": 6120},
    "edit_domain": {"kind": "raw_stream_data", "filters_before": []},
    "passes": [], "passing_edits": 0, "distinct_outputs": 0, "edits": [],
}
img3 = image("img-0003", f"{CM_SHA[:16]}.unverified.png", CM_SHA, 30112, "png", "51 0", "17 0",
             span(270100, 300975), h("cm raw"), ["FlateDecode"], [None], None,
             description(400, 300, 8, 1, "/DeviceGray"),
             [placement(3, 2, [50000, 400000, 250000, 550000],
                        [200000000, 0, 0, 150000000, 50000000, 400000000])],
             integrity("unverified", 300, search_cm, None, h("cm samples")))
img4 = image("img-0004", f"{PX_SHA[:16]}.partial.png", PX_SHA, 4410, "png", "55 0", "19 0",
             span(301200, 307320), h("px raw"), ["FlateDecode"], [None], None,
             description(400, 300, 8, 1, "/DeviceGray"),
             [placement(3, 3, [300000, 400000, 500000, 550000],
                        [200000000, 0, 0, 150000000, 300000000, 400000000])],
             integrity("partial", None, search_px, None, h("px samples")))
img4["integrity"]["rows_written"], img4["integrity"]["rows_declared"] = 61, 300
sr_["images"]["records"] += [img3, img4]
sr_["stream_salvage"]["streams"] += [{"object": "51 0", "search": search_cm},
                                     {"object": "55 0", "search": search_px}]
sr_["stream_salvage"]["summary"].update(streams_damaged=3, checksum_mismatch=1, prefix=1)
c9_pass = [p for p in sr_["passes"] if p["class"] == "C9"][0]
c9_pass["outcome"] = "partial"
c9_pass["note"] = ("51 0 obj: checksum mismatch: the decoded bytes are unverified; "
                   "55 0 obj: only the bytes before the damage decode")
c9_pass["actions"] += [
    {"object": "51 0", "what": "written decoded: its Adler-32 disagrees and no repair was found, "
                               "so the bytes are unverified", "grade": None},
    {"object": "55 0", "what": "written decoded up to input byte 6120 of 6120; the rest is lost",
     "grade": None}]
sr_["products"][3:3] = [{"role": "image", "file": img3["file"], "sha256": CM_SHA, "size_bytes": 30112},
                        {"role": "image", "file": img4["file"], "sha256": PX_SHA, "size_bytes": 4410}]
sr_["outcome"] = {"status": "partial",
                  "reasons": ["C9: 51 0 obj: checksum mismatch: the decoded bytes are unverified; "
                              "55 0 obj: only the bytes before the damage decode"],
                  "chosen_toolpath": "template_assemble"}
sr_["limitations"]["this_run"] = T.this_run(sr_)
sx["reproducible_sha256"] = sha(compact(sr_).encode())
with open(os.path.join(OUT, "fixtures", "salvage-outcomes.custody.json"), "w") as f_:
    f_.write(pretty(sx))
