"""JSON Schema 2020-12 for the PDFPundit custody record, log entry and images index.

Writes nimbalyst-local/plans/implementation/evidence-schema/example/pdfpundit-custody.schema.json.
"""
import json
import sys

HEX64 = {"type": "string", "pattern": "^[0-9a-f]{64}$"}
HEX40 = {"type": "string", "pattern": "^[0-9a-f]{40}$"}
HEX32 = {"type": "string", "pattern": "^[0-9a-f]{32}$"}
U32 = {"type": "integer", "minimum": 0, "maximum": 4294967295}
U64 = {"type": "integer", "minimum": 0}
I64 = {"type": "integer"}
# Part B strings are printable ASCII only (review finding 8): jq, Python and serde_json
# then write the same bytes. Part A and the log may hold any UTF-8 text.
STR = {"type": "string", "pattern": "^[ -~]*$"}
NSTR = {"type": ["string", "null"], "pattern": "^[ -~]*$"}
TSTR = {"type": "string"}
NTSTR = {"type": ["string", "null"]}
UTC = {"type": "string", "pattern": r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"}


def obj(props, required=None, extra=False):
    return {
        "type": "object",
        "properties": props,
        "required": list(props) if required is None else required,
        "additionalProperties": extra,
    }


def nullable(ref):
    return {"oneOf": [{"type": "null"}, ref]}


def enum(*vals):
    return {"enum": list(vals)}


R = lambda name: {"$ref": f"#/$defs/{name}"}
SEQ = {"type": "integer", "minimum": 1}
TOOL_ID = obj({"version": STR, "build_commit": nullable(HEX40),
               "build_origin": enum("release", "local", "unknown")})
FILE_ROLE = enum("json", "text", "checksums", "case_jsonld")

# Log entries carry no case details, file names or paths (review finding 7).
LOG_KINDS = {
    # Review 3 n4: a nonce, so two logs created in the same second by one build differ.
    "log_created": {"tool": TOOL_ID, "nonce": HEX32},
    # Review 2 finding 11: a missing or swapped log is caught against the head kept in history.
    "log_continued": {"reason": enum("chain_broken", "previous_log_missing", "head_mismatch"),
                      "previous_log_id": HEX64, "previous_seq": SEQ,
                      "previous_head_sha256": HEX64,
                      "known_from": enum("log_walk", "history"), "tool": TOOL_ID},
    "batch_opened": {"files": SEQ, "prompt_shown": {"type": "boolean"}, "tool": TOOL_ID},
    "record": {"batch_seq": SEQ, "input_sha256": HEX64,
               "outcome": enum("repaired", "partial", "clean", "failed", "encrypted", "not_run"),
               "input_unchanged": enum("yes", "no", "not_rechecked"),
               "files": {"type": "array", "items": obj({"role": FILE_ROLE, "sha256": HEX64})}},
    "record_write_failed": {"batch_seq": SEQ, "input_sha256": HEX64,
                            "outputs": {"type": "array", "items": obj({"role": STR, "sha256": HEX64})},
                            "error_kind": STR},
    # Revision 5 (decision Q9): the verify file is <primary>.custody.verify-<n>.txt; the entry
    # carries n and the file's hash, so a file and its entry pair by hash, never by name alone.
    "verify": {"record_seq": SEQ, "record_json_sha256": HEX64, "verify_n": SEQ,
               "text_sha256": HEX64, "checks": {"type": "object"}, "head_after_record": HEX64},
    "batch_closed": {"batch_seq": SEQ, "records": U32, "failed": U32},
    # Review 2 finding 6: a cancelled batch logs a count only; the dropped files are never read.
    # Review 3 M1: `received` counts the files of `files` that arrived whole at the drop (kitty)
    # and were discarded unhashed.
    "batch_cancelled": {"prompt_shown": {"type": "boolean"}, "files": SEQ, "received": U32},
    # Review 3 m11: a batch the app never closed (it quit), noted at the next start.
    "batch_unclosed": {"batch_seq": SEQ, "files": SEQ, "records": U32, "failed": U32},
}


def log_kind(kind, extra):
    props = {"seq": SEQ, "kind": {"const": kind}, "time_utc": UTC, "prev": HEX64}
    props.update(extra)
    return obj(props)


defs = {
    "hex64": HEX64,
    "objref": {"type": "string", "pattern": "^[0-9]+ [0-9]+$",
               "description": "PDF object number and generation, 'num gen'"},
    "ratio": obj({"num": U64, "den": U64}),
    "span": obj({"start": U64, "end": U64}),
    "hashes": obj({"sha256": HEX64, "sha1": nullable(HEX40), "md5": nullable(HEX32)}),
    "class": enum("C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10"),
    "grade": enum("exact", "accepted", "ambiguous", "unsearched"),
    "location": {"oneOf": [
        obj({"type": {"const": "file"}}),
        obj({"type": {"const": "span"}, "span": R("span")}),
        obj({"type": {"const": "object"}, "object": R("objref"), "span": nullable(R("span"))}),
        obj({"type": {"const": "page"}, "page": {"type": "integer", "minimum": 1},
             "object": nullable(R("objref"))}),
    ]},
    # Review 3 M5: every FindingKind, with its payload in `detail`.
    "finding_detail": {"oneOf": [
        {"type": "null"},
        obj({"type": {"const": "signed"}, "fields": U32}),
        obj({"type": {"const": "outlined_text"}, "glyph_runs": U32, "contours": U32}),
        obj({"type": {"const": "type3_text"}, "font": R("objref")}),
        obj({"type": {"const": "font_not_embedded"}, "font": R("objref"), "base_font": STR}),
        obj({"type": {"const": "pages_outside_tree"}, "pages": SEQ}),
    ]},
    "finding": obj({
        "id": STR,
        "kind": enum("corruption", "encrypted", "signed", "outlined_text", "type3_text",
                     "font_not_embedded", "pages_outside_tree"),
        "class": {"oneOf": [{"type": "null"}, R("class")]},
        "detail": R("finding_detail"),
        "severity": enum("info", "warning", "error"),
        "location": R("location"),
        "summary": STR,
    }),
    "action": obj({"object": nullable(R("objref")), "what": STR,
                   "grade": {"oneOf": [{"type": "null"}, R("grade")]}}),
    "pass": obj({
        "class": R("class"),
        "outcome": enum("fixed", "partial", "skipped"),
        "note": NSTR,
        "actions": {"type": "array", "items": R("action")},
    }),
    "reply": {"oneOf": [
        obj({"type": {"const": "pick"}, "font_id": STR}),
        obj({"type": {"const": "use_best"}}),
        obj({"type": {"const": "skip"}}),
        obj({"type": {"const": "substitute"}, "choice": STR}),
        obj({"type": {"const": "text_only"}}),
    ]},
    "interaction": obj({
        "seq": {"type": "integer", "minimum": 1},
        "kind": enum("font_pick", "font_unreproducible"),
        "page": {"type": ["integer", "null"], "minimum": 1},
        "slot": NSTR,
        "candidates": {"type": "array", "items": STR},
        "reply": R("reply"),
        "source": enum("user", "batched", "policy"),
        # Review 3 M4: a carried answer names its origin: an answer in this record, or the
        # answer to another file's question ("apply best to all").
        "carried_from": {"oneOf": [
            {"type": "null"},
            obj({"type": {"const": "this_record"}, "seq": SEQ}),
            obj({"type": {"const": "another_file"}, "input_sha256": HEX64}),
        ]},
        "policy_setting": NSTR,
    }),
    "edit_domain": obj({
        "kind": enum("raw_stream_data", "flate_input"),
        "filters_before": {"type": "array", "items": STR},
    }),
    # Review 2 finding 1: what the C9 search did, from salvage.rs and ttf.rs.
    # Review 3 m10: one entry per search pass. A pass starts at the first window, at a widening
    # and at the move to raw deflate; the deep draw extends the pass it lands in.
    "search_pass": obj({
        "mode": enum("zlib", "raw_deflate"),
        "damage": obj({"kind": enum("inflate_error", "adler_mismatch", "header_rejected"),
                       "at": U64}),
        "localizer": enum("error_window", "adler_only", "ttf_tables", "widened"),
        "check": enum("adler32", "adler32_after_ttf_table_checksum", "none_raw_deflate"),
        "deep": {"type": "boolean"},
        "window": {"type": "array", "items": R("span")},
        "candidates_in_window": U64,
        "candidates_tried": U64,
    }),
    "salvage_search": obj({
        # Review 3 M3: every Salvage variant but Clean.
        "outcome": enum("repaired", "checksum_mismatch", "prefix", "unrecoverable", "unsearched"),
        "grade": {"oneOf": [{"type": "null"}, R("grade")]},
        "adler_rerun": {"type": "boolean"},
        "trailer_edit": {"type": "boolean"},
        "predictor_stopped": {"type": "boolean"},
        "prefix": nullable(obj({"in_used": U64, "in_total": U64})),
        "edit_domain": R("edit_domain"),
        "passes": {"type": "array", "items": R("search_pass")},
        "passing_edits": U32,
        "distinct_outputs": U32,
        "edits": {"type": "array", "items": {"type": "array", "items": U64,
                                             "minItems": 3, "maxItems": 3}},
    }),
    "ftime": obj({"sec": I64, "nsec": {"type": "integer", "minimum": 0, "maximum": 999999999}}),
    "file_stat": obj({
        "size_bytes": U64,
        "modified": R("ftime"),
        "changed": nullable(R("ftime")),
        "accessed": nullable(R("ftime")),
        "created": nullable(R("ftime")),
        "read_only": {"type": "boolean"},
        "unix_mode": {"type": ["integer", "null"], "minimum": 0},
        "source": enum("unix_stat", "windows_file_info"),
    }),
    # Review 2 finding 8: everything fontdb/score.rs resolve() decides on.
    "font_evidence": obj({
        "top_candidate": NSTR,
        "confidence": nullable(R("ratio")),
        "hit": nullable(R("ratio")),
        "margin": nullable(R("ratio")),
        "threshold": R("ratio"),
        "min_hit": R("ratio"),
        "inference": obj({"lang": STR, "script": STR, "n_chars": U64, "n_runs": U64}),
        "sample_floor": U64,
        "tounicode": enum("present", "missing", "unparsable"),
        "best_coverage": R("ratio"),
        "best_coverage_font": NSTR,
        "min_coverage": R("ratio"),
        "decision": enum("auto_accept", "ask", "unreproducible", "text_only"),
    }),
    "font_decision": obj({
        "page": {"type": ["integer", "null"], "minimum": 1},
        "slot": NSTR,
        "object": R("objref"),
        "basis": enum("named_match", "auto_accept", "user", "batched", "policy", "unsupported"),
        "font_id": NSTR,
        "interaction_seq": {"type": ["integer", "null"], "minimum": 1},
        "evidence": nullable(R("font_evidence")),
        "reason": NSTR,
    }),
    "placement": obj({
        "page_in": {"type": ["integer", "null"], "minimum": 1},
        "page_out": {"type": "integer", "minimum": 1},
        "paint_order": U32,
        "bbox_mpt": {"type": "array", "items": I64, "minItems": 4, "maxItems": 4},
        "matrix_micro": {"type": "array", "items": I64, "minItems": 6, "maxItems": 6},
        "form_chain": {"type": "array", "items": R("objref")},
    }),
    "image": obj({
        "id": {"type": "string", "pattern": "^img-[0-9]{4,}$"},
        "file": NSTR,
        "file_sha256": nullable(HEX64),
        "file_len": {"type": ["integer", "null"], "minimum": 0},
        "format": enum("jpg", "jp2", "j2k", "png", "tif", "ccitt", "jb2e", "jb2g", "bin", "none"),
        "view_file": NSTR,
        "view_sha256": nullable(HEX64),
        "source": obj({
            "kind": enum("xobject", "inline", "smask", "mask", "stencil", "thumb", "jbig2_globals"),
            "copy": enum("winner", "shadow", "headerless", "unreferenced"),
            "object": nullable(R("objref")),
            "output_object": nullable(R("objref")),
            "parent": NSTR,
            "inline_origin": nullable(obj({"stream_object": R("objref"), "bi_ordinal": U32,
                                           "bi_offset": U64})),
            # Review 2 finding 7: a span is in the file or in a decoded content stream.
            "raw": {"oneOf": [
                obj({"domain": {"const": "file"}, "span": R("span"),
                     "container_object": {"type": "null"},
                     "container_file_span": {"type": "null"}}),
                obj({"domain": {"const": "decoded_content_stream"}, "span": R("span"),
                     "container_object": R("objref"),
                     "container_file_span": R("span")}),
            ]},
            "raw_len": U64,
            "raw_sha256": HEX64,
            "length_source": enum("declared", "inflate_probe", "scanned_endstream", "truncated_at_eof"),
        }),
        "encoding": obj({
            "filters": {"type": "array", "items": STR},
            "decode_parms": {"type": "array", "items": NSTR},
            "codec": {"oneOf": [{"type": "null"}, enum("jpeg", "jpeg2000", "ccitt", "jbig2")]},
            "jbig2_globals": NSTR,
        }),
        "description": obj({
            "width": {"type": ["integer", "null"], "minimum": 0},
            "height": {"type": ["integer", "null"], "minimum": 0},
            "actual_width": {"type": ["integer", "null"], "minimum": 0},
            "actual_height": {"type": ["integer", "null"], "minimum": 0},
            "bits_per_component": {"type": ["integer", "null"], "minimum": 1},
            "components": {"type": ["integer", "null"], "minimum": 1},
            "colour_space": NSTR,
            "icc": nullable(obj({"object": R("objref"), "sha256": HEX64})),
            "palette_entries": {"type": ["integer", "null"], "minimum": 0},
            "decode": NSTR,
            "image_mask": {"type": "boolean"},
            "interpolate": {"type": "boolean"},
            "intent": NSTR,
            "needs_pdf_context": {"type": "boolean"},
        }),
        "placements": {"type": "array", "items": R("placement")},
        "integrity": obj({
            "status": enum("intact", "repaired", "partial", "truncated", "unverified",
                           "undecoded", "unrecoverable", "over_cap", "no_dimensions"),
            "rows_written": {"type": ["integer", "null"], "minimum": 0},
            "rows_declared": {"type": ["integer", "null"], "minimum": 0},
            "salvage": nullable(R("salvage_search")),
            "repaired_stream_sha256": nullable(HEX64),
            "samples_sha256": nullable(HEX64),
            "warnings": {"type": "array", "items": STR},
        }),
    }),
    "product": obj({
        "role": enum("repaired_pdf", "markdown", "image", "image_view", "images_index",
                     "images_checksums"),
        "file": NSTR,
        "sha256": HEX64,
        "size_bytes": U64,
    }),
    "settings": obj({
        "salvage_work": U64, "salvage_deep_work": U64, "salvage_deep_pool": U64,
        "max_search_stream": U64,
        "passes": {"oneOf": [{"type": "null"}, {"type": "array", "items": R("class")}]},
        "font_policy": enum("bundled", "bundled_then_system"),
        "font_source_effective": enum("bundled", "bundled_then_system"),
        "prompt_unresolved": {"type": "boolean"},
        "derived_image_views": {"type": "boolean"},
        "unreproducible": enum("ask", "substitute_generic", "text_only"),
        "default_page_size": {"oneOf": [enum("a4", "letter"),
                                        obj({"custom": obj({"w_pt": U32, "h_pt": U32})})]},
        "extract_unplaceable_images": {"type": "boolean"},
        "extract_type3_glyph_images": {"type": "boolean"},
        "auto_accept_confidence": R("ratio"),
        "max_font_candidates": U32,
    }),
    "verification": obj({
        "baseline": enum("extracted", "carve_proxy", "none"),
        "gates": obj({"lopdf_reload": {"type": "boolean"}, "hayro_render_all_pages": {"type": "boolean"},
                      "rediagnose_clean": {"type": "boolean"}, "page_count_ok": {"type": "boolean"},
                      "root_and_pages_present": {"type": "boolean"}}),
        "retention": obj({"text_ops": R("ratio"), "path_fills": R("ratio"), "images": R("ratio"),
                          "content_bytes": R("ratio"), "blank_pages": U32, "glyph_count": R("ratio")}),
        "plausibility": obj({"unmapped_glyph": R("ratio"), "fffd": R("ratio"),
                             "script_consistency": R("ratio"),
                             "dictionary_hit": nullable(R("ratio"))}),
    }),
    "reproducible": obj({
        "job": enum("repair", "repair_and_export", "export"),
        # Review 3 m14: the platform is part of "the same build".
        "tool": obj({"name": {"const": "PDFPundit"}, "version": STR, "build_commit": nullable(HEX40),
                     "build_origin": enum("release", "local", "unknown"),
                     "platform": obj({"os": enum("macos", "windows", "linux"),
                                      "arch": enum("aarch64", "x86_64")}),
                     "report_schema_version": U32, "font_db_sha256": HEX64}),
        "settings": R("settings"),
        "input": obj({"sha256": HEX64, "size_bytes": U64, "pdf_version": NSTR,
                      "pages_discovered": U32, "signed_fields": U32, "encrypted": {"type": "boolean"}}),
        "interactions": {"type": "array", "items": R("interaction")},
        "font_decisions": {"type": "array", "items": R("font_decision")},
        "outcome": obj({
            "status": enum("repaired", "partial", "clean", "failed", "encrypted"),
            "reasons": {"type": "array", "items": STR},
            "chosen_toolpath": {"oneOf": [{"type": "null"}, enum("resave", "template_assemble")]},
        }),
        "findings_before": {"type": "array", "items": R("finding")},
        "findings_after": {"type": "array", "items": R("finding")},
        "passes": {"type": "array", "items": R("pass")},
        "candidates": {"type": "array", "items": obj({
            "toolpath": enum("resave", "template_assemble"), "chosen": {"type": "boolean"},
            "verification": R("verification")})},
        "stream_salvage": obj({
            "summary": obj({k: U32 for k in ["streams_damaged", "repaired", "exact", "accepted",
                                             "ambiguous", "checksum_mismatch", "prefix",
                                             "unrecoverable", "unsearched"]}),
            "streams": {"type": "array", "items": obj({
                "object": R("objref"), "search": R("salvage_search")})},
            "filter_rewrites": {"type": "array", "items": obj({
                "object": R("objref"), "before": STR, "after": STR})},
        }),
        "shadows": {"type": "array", "items": obj({"object": R("objref"), "span": R("span")})},
        "reals_narrowed": U32,
        # Review 3 m16: text.rs's file-dependent warnings, from the input baseline or the output.
        "text_warnings": {"type": "array", "items": obj({
            "source": enum("input", "output"),
            "warning": enum("hidden_layers", "scanned_pages")})},
        "products": {"type": "array", "items": R("product")},
        "images": obj({
            "dir": NSTR,
            "type3_glyph_images": U32,
            "records": {"type": "array", "items": R("image")},
        }),
        "limitations": obj({"standing": {"type": "array", "items": STR},
                            "this_run": {"type": "array", "items": STR}}),
        "statements": obj({k: STR for k in ["role", "reproducibility", "validation", "tool_identity",
                                            "input_file", "case_details", "times", "log",
                                            "legacy_hashes"]}),
    }),
    "record_ref": obj({"record": TSTR, "sha256": HEX64, "log_id": HEX64,
                       "log_seq": {"type": "integer", "minimum": 1}}),
    # Review 2 finding 3: never null when an answer was replayed.
    "replay_source": {"oneOf": [
        obj({"type": {"const": "none"}}),
        obj({"type": {"const": "custody_record"}, "record": TSTR, "sha256": HEX64,
             "log_id": HEX64, "log_seq": SEQ,
             "seqs": {"type": "array", "items": SEQ, "minItems": 1}}),
        obj({"type": {"const": "run_without_record"}, "input_sha256": HEX64,
             "started_utc": nullable(UTC),
             "history_run": {"type": "boolean"},
             "seqs": {"type": "array", "items": SEQ, "minItems": 1}}),
    ]},
    "placed_file": obj({
        "role": enum("repaired_pdf", "markdown", "image", "image_view", "images_index",
                     "images_checksums"),
        "name": TSTR,
        "path": NTSTR,
        "size_bytes": U64,
        "hashes": R("hashes"),
        "placement": {"oneOf": [{"type": "null"}, enum("atomic", "linked", "claimed_then_renamed",
                                                       "create_new", "reused_identical")]},
        "matches_product": {"type": "boolean"},
        "note": NTSTR,
    }),
    "run": obj({
        "job_status": enum("completed", "cancelled", "failed_before_engine", "input_changed",
                           "record_only"),
        "batch": obj({"log_seq": {"type": "integer", "minimum": 1},
                      "position": {"type": "integer", "minimum": 1},
                      "count": {"type": "integer", "minimum": 1}}),
        "case": obj({"prompt_shown": {"type": "boolean"}, "case_reference": NTSTR,
                     "examiner": NTSTR, "submission_reference": NTSTR, "notes": NTSTR}),
        "clock": obj({"started_utc": UTC, "ended_utc": UTC,
                      # Revision 5 (decision Q7): the offset at started_utc, on every platform.
                      # Null only when the zone source is missing or does not parse.
                      "host_utc_offset_minutes": {"type": ["integer", "null"], "minimum": -1440, "maximum": 1440},
                      "host_utc_offset_from": {"oneOf": [{"type": "null"}, enum(
                          "etc_localtime_tzif", "windows_local_file_time")]},
                      "host_zone_hint": nullable(obj({"name": TSTR, "from": enum(
                          "etc_localtime_link", "windows_time_zone_info")})),
                      "source": {"const": "host_clock_unverified"}}),
        # Review 3 m1: shown_utc is null for an answer nobody saw (batched, policy).
        "answer_times": {"type": "array", "items": obj({
            "seq": SEQ, "shown_utc": nullable(UTC), "answered_utc": UTC})},
        "host": obj({"name": NTSTR, "os": enum("macos", "windows", "linux"),
                     "tool_binary_sha256": nullable(HEX64)}),
        "custody_settings": obj({
            "hashes": {"type": "array", "items": enum("sha256", "sha1", "md5"), "minItems": 1},
            "ask_case_details": {"type": "boolean"},
            "require_case_details": {"type": "boolean"},
            "record_host_name": {"type": "boolean"},
            "case_jsonld": {"type": "boolean"}}),
        "input_file": obj({
            "name": TSTR, "path": NTSTR,
            # Review 3 M1: JobInput::File, ::Dropped (kitty: bytes read whole at the drop),
            # ::Memory (macOS file promise: bytes and a name, no path).
            "path_kind": enum("file", "dropped", "memory"),
            "first_read": enum("job_start", "drop"),
            "size_bytes": U64,
            # Review 3 m11: null when the job was cancelled before it read the input.
            "hashes_before": nullable(R("hashes")),
            "hashes_after": nullable(R("hashes")),
            "unchanged": enum("yes", "no", "not_rechecked"),
            "not_rechecked_reason": NTSTR,
            "stat_before": nullable(R("file_stat")),
            "stat_after": nullable(R("file_stat")),
            "stat_changed": {"type": "array", "items": enum(
                "size_bytes", "modified", "changed", "accessed", "created", "read_only",
                "unix_mode")}}),
        # Revision 5 (decision Q16): custody mode needs [general] output_dir, never an input's
        # own folder, so the destination is always that folder.
        "destination": obj({"path": TSTR}),
        "placed": {"type": "array", "items": R("placed_file")},
        "images_dir": nullable(obj({"path": TSTR, "name": TSTR,
                                    "outcome": enum("created", "reused_identical", "renamed_after_clash")})),
        "processing": obj({"analysis_state": enum("reused", "rebuilt"), "rebuilt_reason": NTSTR,
                           "config_path": NTSTR}),
        "record_files": obj({"json": TSTR, "text": TSTR, "checksums": TSTR, "case_jsonld": NTSTR}),
        "earlier_records": {"type": "array", "items": R("record_ref")},
        # Review 3 m15: the logs searched, this one first, then back through log_continued.
        "earlier_records_logs": obj({"searched": {"type": "array", "items": HEX64, "minItems": 1},
                                     "missing": nullable(HEX64)}),
        "answers_replayed_from": R("replay_source"),
        "log": obj({"path": TSTR, "log_id": HEX64, "seq": {"type": "integer", "minimum": 1},
                    "prev_entry_sha256": HEX64}),
    }),
    "log_entry": {"oneOf": [log_kind(k, extra) for k, extra in LOG_KINDS.items()]},
    "images_index": obj({
        "format": {"const": "pdfpundit-images"},
        "images_schema_version": {"const": 1},
        "input_sha256": HEX64,
        "tool": obj({"name": {"const": "PDFPundit"}, "version": STR}),
        "settings_sha256": HEX64,
        "type3_glyph_images": U32,
        "records": {"type": "array", "items": R("image")},
    }),
}

schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "urn:pdfpundit:schema:custody:1",
    "title": "PDFPundit tool run record (custody record)",
    "type": "object",
    "properties": {
        "format": {"const": "pdfpundit-custody"},
        "custody_schema_version": {"const": 1},
        "reproducible_sha256": nullable(HEX64),
        "reproducible": nullable(R("reproducible")),
        "run": R("run"),
    },
    "required": ["format", "custody_schema_version", "reproducible_sha256", "reproducible", "run"],
    "additionalProperties": False,
    "$defs": defs,
}

if __name__ == "__main__":
    out = sys.argv[1]
    with open(out, "w") as f:
        json.dump(schema, f, indent=2)
        f.write("\n")
