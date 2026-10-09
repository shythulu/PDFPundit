"""Fixed text for the custody record, in one place.

example.py writes these strings into Part B; splice.py prints the same strings into the
design document; validate.py recomputes the this-run list from Part B with this_run().
So the document, the example and the check can never quote different text.

Plural rule (review 3, finding m2): a token [x:one|many] gives `one` when the count x is
exactly 1 and `many` otherwise. A sentence appears only when its count is above zero.
"""
import re

# ---------------------------------------------------------------- statements (12.1)

VALIDATION_WITH_COMMIT = (
    "PDFPundit has not been validated by an independent body, such as NIST CFTT, and claims no "
    "accreditation. Its test suite is in its source repository at the build commit recorded in "
    "part B. Its corpus tests use a third-party corpus that the repository names but does not "
    "contain.")
VALIDATION_NO_COMMIT = (
    "PDFPundit has not been validated by an independent body, such as NIST CFTT, and claims no "
    "accreditation. This build records no build commit, so this record cannot name the source "
    "its tests ran on. Its corpus tests use a third-party corpus that the repository names but "
    "does not contain.")

STATEMENTS = {
    "role": "This is a tool run record. It supports an examiner's report and is not one. "
            "It states no opinion on the document's content, authorship or authenticity.",
    "reproducibility": "Part B is identical, byte for byte, whenever the same input bytes are "
                       "processed by the same PDFPundit build with the same settings and the same "
                       "recorded answers. The same build means the same version and build commit, "
                       "compiled for the same operating system and processor architecture; part B "
                       "records all four. Identity across platforms is not claimed. This shows the "
                       "result is repeatable. It does not show the repair is correct.",
    "validation": None,  # VALIDATION_WITH_COMMIT or VALIDATION_NO_COMMIT, from tool.build_commit
    "tool_identity": "Version, build commit and build origin are reported by the program itself, "
                     "and whoever builds it can set them. Releases are unsigned. Each release "
                     "publishes the SHA-256 of every executable it ships. To confirm the build, "
                     "compare the binary SHA-256 in part A with the value published for this "
                     "version and platform.",
    "input_file": "PDFPundit cannot tell whether its input is the original evidence or a working "
                  "copy. It opens the input read-only and never writes to it. It is not a write "
                  "blocker. Unchanged means the SHA-256 of the input's content was the same before "
                  "and after the run. File-system times before and after the run are recorded in "
                  "part A. Reading a file can itself update its access time. In custody mode, "
                  "outputs and records are written only to the output folder named in part A. "
                  "Custody mode does not start while no output folder is set, or while it is the "
                  "folder of an input.",
    "case_details": "Case reference, examiner, submission reference and notes are typed at the "
                    "custody prompt, once per batch. PDFPundit records them as typed and does not "
                    "verify them.",
    "times": "Times come from the host clock, in UTC, and are not independently verified. "
             "Started is read just before the job first reads the input's file state. An input "
             "received whole at the drop was read before that, before the custody prompt; part A "
             "says how the input was received. Ended is read just after the input's file state is "
             "read again, before the record files are written. The log entry for this record is "
             "timed after those files are written.",
    "log": "The custody log is hash-chained. It is tamper-evident, not tamper-proof. The chain "
           "shows a changed or removed entry only while the entries after it are unchanged: "
           "anyone who can write the file can recompute every later entry, and the chain then "
           "verifies. Only a log head copied outside the log, such as the one shown when a batch "
           "closes, or an unedited copy of the log catches that; such a head pins the log up to "
           "its entry. PDFPundit also compares the log with the last head kept in its history when "
           "custody mode starts, and starts a new log naming the old one when they differ; a "
           "change made to both is not detected.",
    "legacy_hashes": "MD5 and SHA-1 are included only to match older case systems. Neither is "
                     "collision-resistant. SHA-256 is the integrity hash.",
}


def statements(build_commit):
    s = dict(STATEMENTS)
    s["validation"] = VALIDATION_WITH_COMMIT if build_commit else VALIDATION_NO_COMMIT
    return s


# ---------------------------------------------------------------- standing limits (12.2)

# text.rs KNOWN_LIMITS, verbatim and in order, then two image and repair sentences.
KNOWN_LIMITS = [
    "text marked with ActualText is extracted as drawn; the ActualText is not used",
    "glyphs carry an origin and a size but no bounding box",
    "text drawn in render mode 7 (clip only) is not extracted",
    "the interpreter gives no warning for a dropped compressed stream or an unresolved font; "
    "unmapped glyphs and the glyph count per page are the integrity signals",
]
STANDING = KNOWN_LIMITS + [
    "image files hold the image as stored, or its decoded samples re-encoded as PNG or TIFF "
    "with the same sample values; colour spaces, /Decode arrays and masks are recorded, not "
    "applied; an image whose stream was repaired, cut short or left unverified holds those "
    "samples, its file name says so, and its record gives the search",
    "a repaired file is a new document: it is not the input and carries none of the input's "
    "signatures",
]

# text.rs warnings, verbatim, used as this-run sentences when text extraction reported them.
HIDDEN_LAYERS = "the document has optional content: text in hidden layers is not extracted"
SCANNED_PAGES = "the page tree is unreadable: pages were found by scanning and are in file order"

# ---------------------------------------------------------------- this-run sentences (12.3)

# (key, condition in words, template). Order here is the order in limitations.this_run.
THIS_RUN = [
    ("c9_exact", "`stream_salvage.summary.exact` > 0",
     "{n} compressed [n:stream was|streams were] graded Exact: every single-byte replacement in "
     "the searched window was tried, and every one that passed the check gave the same output; "
     "damage of other kinds is not searched, and the grade does not show the repaired bytes "
     "equal the original bytes"),
    ("c9_accepted", "`summary.accepted` > 0",
     "{n} compressed [n:stream was|streams were] graded Accepted: the work limit ran out before "
     "every single-byte replacement in the window was tried; every one that passed the check up "
     "to then gave the same output, and a different one may exist"),
    ("c9_ambiguous", "`summary.ambiguous` > 0",
     "{n} compressed [n:stream was|streams were] graded Ambiguous: replacements that passed the "
     "check gave two or more different outputs, or the stream is raw deflate with no Adler-32, "
     "and a fixed order chose one"),
    ("c9_trailer", "streams whose `search.trailer_edit` is true > 0",
     "for {n} compressed [n:stream|streams] no replacement in the data passed the check, so the "
     "stored Adler-32 was rewritten to match the decoded data; that data is the damaged "
     "stream's own and no checksum covers it, whatever the grade says"),
    ("c9_widened", "streams whose `search.adler_rerun` is true > 0",
     "{n} font [n:stream|streams] failed [n:its|their] own table checksum, so the search widened "
     "and the repair passed the Adler-32 check alone"),
    ("c9_unverified", "`summary.checksum_mismatch` > 0",
     "{n} compressed [n:stream was|streams were] written decoded with no grade: no replacement "
     "passed the Adler-32 check, or one that passed held an invalid predictor row and only the "
     "rows before it were kept; these bytes are unverified"),
    ("c9_prefix", "`summary.prefix` > 0",
     "{n} compressed [n:stream was|streams were] cut short: only the bytes before the damage "
     "decode, and the rest of [n:the stream is|each stream is] lost"),
    ("c9_unrecoverable", "`summary.unrecoverable` > 0",
     "{n} compressed [n:stream|streams] could not be repaired and [n:was|were] written as found"),
    ("c9_unsearched", "`summary.unsearched` > 0",
     "{n} compressed [n:stream was|streams were] over the search size limit and not searched"),
    ("fonts_substituted", "font objects in `font_decisions` with a `font_id` > 0",
     "{n} font [n:program was|programs were] substituted from the bundled font database; glyph "
     "shapes are the bundled font's, not those of the font the input embedded"),
    ("font_substitute_or_text_only", "interactions whose reply is `substitute` or `text_only` > 0",
     "{n} font [n:question was|questions were] answered with a substitute or text only, as part "
     "B.7 shows"),
    ("font_auto_accept", "slots with basis `auto_accept` > 0; m = slots in `font_decisions`",
     "{n} of {m} font [m:slot|slots] [n:was|were] matched without asking, because [n:its score|"
     "their scores] cleared the auto-accept setting; part B.7 gives the basis for every slot"),
    ("answers_batched", "interactions with source `batched` > 0; m = interactions",
     "{n} of {m} [m:answer|answers] [n:was|were] carried from an earlier answer and not shown to "
     "the user"),
    ("answers_policy", "interactions with source `policy` > 0; m = interactions",
     "{n} of {m} [m:answer|answers] came from settings, not from the user"),
    ("signed", "`input.signed_fields` > 0",
     "the input has {n} digital signature [n:field|fields]; the repaired file carries none"),
    ("pages_appended", "sum of `pages` in `pages_outside_tree` findings > 0",
     "{n} [n:page was|pages were] found only outside the page tree and [n:was|were] appended "
     "after the tree's pages"),
    ("scanned_pages", "`text_warnings` holds `scanned_pages`",
     SCANNED_PAGES + "; text counts taken that way follow file order, while every page number in "
     "this record follows the rebuilt page order"),
    ("hidden_layers", "`text_warnings` holds `hidden_layers`", HIDDEN_LAYERS),
    ("reals_narrowed", "`reals_narrowed` > 0",
     "{n} [n:number|numbers] lost precision when written as 32-bit values"),
    ("images_incomplete", "images with status `partial` or `truncated` > 0",
     "{n} [n:image is|images are] incomplete; only whole rows that decoded are kept"),
    ("images_unverified", "images with status `unverified` > 0",
     "{n} image [n:file holds|files hold] samples decoded from a stream that failed its check; "
     "[n:its name says|their names say] unverified"),
    ("images_need_context", "images with `needs_pdf_context` > 0",
     "{n} image [n:file displays|files display] wrongly on [n:its|their] own; the PDF settings "
     "recorded here are needed to read [n:it|them]"),
    ("type3_counted", "`images.type3_glyph_images` > 0 and `extract_type3_glyph_images` false",
     "{n} glyph [n:image|images] in Type3 fonts [n:was|were] counted, not extracted"),
    ("outcome_reasons", "`outcome.reasons` not empty", "each engine reason, verbatim, in order"),
]

_TOKEN = re.compile(r"\[(n|m):([^|\]]*)\|([^\]]*)\]")


def fill(template, n, m=None):
    counts = {"n": n, "m": m}
    out = _TOKEN.sub(lambda t: t.group(2) if counts[t.group(1)] == 1 else t.group(3), template)
    return out.replace("{n}", str(n)).replace("{m}", str(m))


def this_run(R):
    """The this-run sentences for Part B `R`, computed from Part B alone."""
    ss = R["stream_salvage"]["summary"]
    searches = [s["search"] for s in R["stream_salvage"]["streams"]]
    fd = R["font_decisions"]
    inter = R["interactions"]
    imgs = R["images"]["records"]
    tw = {w["warning"] for w in R["text_warnings"]}
    counts = {
        "c9_exact": (ss["exact"], None),
        "c9_accepted": (ss["accepted"], None),
        "c9_ambiguous": (ss["ambiguous"], None),
        "c9_trailer": (sum(s["trailer_edit"] for s in searches), None),
        "c9_widened": (sum(s["adler_rerun"] for s in searches), None),
        "c9_unverified": (ss["checksum_mismatch"], None),
        "c9_prefix": (ss["prefix"], None),
        "c9_unrecoverable": (ss["unrecoverable"], None),
        "c9_unsearched": (ss["unsearched"], None),
        "fonts_substituted": (len({d["object"] for d in fd if d["font_id"]}), None),
        "font_substitute_or_text_only": (
            sum(q["reply"]["type"] in ("substitute", "text_only") for q in inter), None),
        "font_auto_accept": (sum(d["basis"] == "auto_accept" for d in fd), len(fd)),
        "answers_batched": (sum(q["source"] == "batched" for q in inter), len(inter)),
        "answers_policy": (sum(q["source"] == "policy" for q in inter), len(inter)),
        "signed": (R["input"]["signed_fields"], None),
        "pages_appended": (sum(f["detail"]["pages"] for f in R["findings_before"]
                               if f["kind"] == "pages_outside_tree"), None),
        "scanned_pages": (int("scanned_pages" in tw), None),
        "hidden_layers": (int("hidden_layers" in tw), None),
        "reals_narrowed": (R["reals_narrowed"], None),
        "images_incomplete": (sum(i["integrity"]["status"] in ("partial", "truncated")
                                  for i in imgs), None),
        "images_unverified": (sum(i["integrity"]["status"] == "unverified" for i in imgs), None),
        "images_need_context": (sum(i["description"]["needs_pdf_context"] for i in imgs), None),
        "type3_counted": (0 if R["settings"]["extract_type3_glyph_images"]
                          else R["images"]["type3_glyph_images"], None),
    }
    out = []
    for key, _, tpl in THIS_RUN:
        if key == "outcome_reasons":
            out += R["outcome"]["reasons"]
            continue
        n, m = counts[key]
        if n > 0:
            out.append(fill(tpl, n, m))
    return out


# ---------------------------------------------------------------- engine strings the example quotes

# As written at HEAD (f5ddfa6). The example builds its findings and actions from these, never
# from invented text. Line numbers are at that commit.
ENGINE = {
    # src/pdf/repair.rs 308 (ReEmitted): every C1-C3 finding.
    "reemitted": "{fid}: resolved by re-emission",
    # src/pdf/diagnose.rs 530: C2.
    "c2_summary": "no cross-reference table or stream",
    # src/pdf/diagnose.rs 1644 and 1688: C7, a /FontFile* that names a non-stream.
    "c7_summary": "the font program of {name} is gone: its /{key} names {n} {g} R, which is not "
                  "a stream",
    # src/pdf/diagnose.rs 1620: FontNotEmbedded.
    "noembed_summary": "the font {name} is not embedded",
    # src/pdf/repair/fonts.rs 1143: a substitution.
    "font_substituted": "font program substituted: {id}",
    # src/pdf/repair.rs 690 and diagnose.rs 1726: C9 Exact. Overclaims (review 3, finding M2).
    "c9_exact_action_today": "Flate data repaired ({edits}); no other repair fits its window",
    "c9_exact_summary_today": "the Flate data of {n} {g} obj is damaged: repaired; no other "
                              "repair fits its window",
    # report.rs 25 SIGNED_NOTE and diagnose.rs 134 SIGNED_SUMMARY. Say "original" (finding m6).
    "signed_note_today": "the original is digitally signed; the repaired file is not",
    "signed_summary_today": "the original is digitally signed; a repaired file is not",
}

# The text section 14 item 24 asks the engine to write instead. The example uses these.
ENGINE_FIX = {
    "c9_exact_action": "Flate data repaired ({edits}); every single-byte replacement in its "
                       "window was tried, and every one that passed the Adler-32 gave this output",
    "c9_exact_summary": "the Flate data of {n} {g} obj is damaged: repaired; every single-byte "
                        "replacement in its window that passed the Adler-32 gave the same output",
    "c9_trailer_action": "Adler-32 trailer rewritten ({edits}) to match the decoded data; no "
                         "replacement in the data passed the check, so no checksum covers the "
                         "data",
    "c9_trailer_summary": "the Flate data of {n} {g} obj is damaged: its stored Adler-32 was "
                          "rewritten to match the decoded data, which no checksum covers",
    "signed_note": "the input is digitally signed; the repaired file is not",
    "signed_summary": "the input is digitally signed; a repaired file is not",
}


def edit_list(edits):
    """repair.rs edit_list: `byte 40 0x31 -> 0x30, ...`."""
    return ", ".join(f"byte {at} 0x{a:02x} -> 0x{b:02x}" for at, a, b in edits)
