#!/usr/bin/env python3
"""Lost-prefix DEFLATE resynchronisation with two dummy dictionaries (CLM-0337) on REPDF Flate streams.

For every original Flate stream with more than one DEFLATE block, and every block after the first,
pretend everything before that block is lost (head truncation, DMG-003 / GAP-152):
  1. Bit-align the stream at the block header and raw-inflate it twice, with two different 32 KiB
     dummy dictionaries (all 0x00, all 0xFF). Bytes identical in both runs do not depend on the lost
     history; the others are marked unknown.
  2. Score against the true decoded output from that point: precision of the "reliable" bytes
     (should be 1.0 by construction) and recall (share of bytes recovered), overall and in the
     first 4 KiB after the restart.
  3. Blind start search: in the 2,048 bit offsets before the true block start, count offsets that
     raw-inflate to the end of the stream without error (false restarts a blind scan would accept),
     how many of them converge onto the true output, and how many garbage bytes they emit that the
     two-dictionary test still marks as reliable (identical under both dictionaries), and how many
     false starts survive a stricter test (the last block ends exactly at the Adler-32 trailer).

    python3 dict_resync.py /home/user/dfrc-korea/repdf results/dict_resync [--max-streams 300]

Uses the RFC 1951 token parser in c9_replay_correct.py for block positions; zlib does the inflating.
"""
import csv
import json
import re
import statistics
import sys
import zlib
from pathlib import Path

from c9_replay_correct import STREAM_RE, tokens, trim

DICT_A, DICT_B = b"\x00" * 32768, b"\xff" * 32768


def shifted(stream: bytes, bit: int) -> bytes:
    v = int.from_bytes(stream, "little") >> bit
    return v.to_bytes(max(1, len(stream) - bit // 8), "little")


def inflate_raw(data: bytes, zdict: bytes, strict: bool = False) -> tuple[bytes, bool]:
    """Raw inflate. With strict=True, 'ok' also requires the final block to end where the stream
    does (only the 4-byte Adler-32 trailer left over)."""
    d = zlib.decompressobj(-15, zdict=zdict)
    try:
        out = d.decompress(data)
        out += d.flush()
    except zlib.error:
        return b"", False
    return out, d.eof and (not strict or len(d.unused_data) == 4)


def main() -> None:
    corpus, outp = Path(sys.argv[1]), Path(sys.argv[2])
    cap = int(sys.argv[sys.argv.index("--max-streams") + 1]) if "--max-streams" in sys.argv else 300
    rows = []
    streams = 0
    for f in sorted((corpus / "original").rglob("*.pdf")):
        mode = f.parent.parent.name
        for m in STREAM_RE.finditer(f.read_bytes()):
            z = trim(m.group(1))
            toks, out, err = tokens(z)
            if err:
                continue
            blocks = []          # (bit position of header, output offset)
            o = 0
            for bit, t in toks:
                if t[0] == "B":
                    blocks.append((bit, o, t[2]))
                    if t[2] == 0:
                        o += t[3]
                elif t[0] == "L":
                    o += 1
                elif t[0] == "M":
                    o += t[1]
            if len(blocks) < 2:
                continue
            streams += 1
            for bi, (bit, off, btype) in enumerate(blocks[1:], start=1):
                data = shifted(z[2:], bit - 16)    # token bit positions count from the stream start
                a, ea = inflate_raw(data, DICT_A)
                b, eb = inflate_raw(data, DICT_B)
                truth = out[off:]
                _, true_strict = inflate_raw(data, DICT_A, strict=True)
                ok = ea and eb and len(a) == len(b) == len(truth)
                rel = [i for i in range(len(a)) if a[i] == b[i]] if ok else []
                correct = sum(1 for i in rel if a[i] == truth[i]) if ok else 0
                head = min(4096, len(truth))
                rel_head = sum(1 for i in rel if i < head)
                # blind scan: offsets before the true start that inflate cleanly to the end
                false_ok = false_conv = wrong_rel = false_strict = 0
                for s in range(max(0, bit - 16 - 2048), bit - 16):
                    x, ex = inflate_raw(shifted(z[2:], s), DICT_A)
                    if not (ex and len(x) > 0):
                        continue
                    false_ok += 1
                    _, strict_ok = inflate_raw(shifted(z[2:], s), DICT_A, strict=True)
                    false_strict += strict_ok
                    y, _ = inflate_raw(shifted(z[2:], s), DICT_B)
                    # converged: the false start's output ends with the true output from this block on
                    k = 0
                    while k < min(len(x), len(truth)) and x[-1 - k] == truth[-1 - k]:
                        k += 1
                    false_conv += k == len(truth)
                    garbage = len(x) - k
                    wrong_rel += sum(1 for i in range(min(garbage, len(y))) if x[i] == y[i])
                rows.append({"file": str(f.relative_to(corpus)), "mode": mode, "stream_start": m.start(1),
                             "comp_len": len(z), "block_index": bi, "block_type": btype, "blocks": len(blocks),
                             "out_after": len(truth), "decoded_ok": ok, "true_start_ends_at_trailer": true_strict, "reliable": len(rel),
                             "reliable_correct": correct,
                             "recall": round(len(rel) / len(truth), 4) if ok and truth else "",
                             "recall_first4k": round(rel_head / head, 4) if ok and head else "",
                             "false_starts_2048": false_ok, "false_starts_converged": false_conv,
                             "false_starts_ending_at_trailer": false_strict,
                             "false_start_reliable_wrong_bytes": wrong_rel})
            if streams >= cap:
                break
        if streams >= cap:
            break
    outp.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{outp}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    ok = [r for r in rows if r["decoded_ok"]]

    def q(xs):
        xs = sorted(xs)
        return [xs[0], xs[len(xs) // 4], statistics.median(xs), xs[3 * len(xs) // 4], xs[-1]] if xs else None

    res = {"streams_multiblock": streams, "restarts": len(rows), "decoded_ok": len(ok),
           "true_start_ends_at_trailer": sum(r["true_start_ends_at_trailer"] for r in rows),
           "precision_reliable": (sum(r["reliable_correct"] for r in ok) / max(1, sum(r["reliable"] for r in ok))),
           "recall_min_q1_median_q3_max": q([r["recall"] for r in ok]),
           "recall_first4k_min_q1_median_q3_max": q([r["recall_first4k"] for r in ok]),
           "by_mode": {md: {"restarts": len([r for r in ok if r["mode"] == md]),
                            "median_recall": statistics.median([r["recall"] for r in ok if r["mode"] == md] or [0]),
                            "median_recall_first4k": statistics.median([r["recall_first4k"] for r in ok if r["mode"] == md] or [0])}
                       for md in sorted({r["mode"] for r in ok})},
           "blind_scan_offsets_tested": len(rows) * 2048,
           "blind_scan_false_starts_total": sum(r["false_starts_2048"] for r in rows),
           "blind_scan_restarts_with_any_false_start": sum(r["false_starts_2048"] > 0 for r in rows),
           "blind_scan_false_starts_converged": sum(r["false_starts_converged"] for r in rows),
           "blind_scan_false_starts_ending_at_trailer": sum(r["false_starts_ending_at_trailer"] for r in rows),
           "blind_scan_restarts_with_false_start_ending_at_trailer": sum(r["false_starts_ending_at_trailer"] > 0 for r in rows),
           "blind_scan_reliable_but_wrong_bytes_total": sum(r["false_start_reliable_wrong_bytes"] for r in rows),
           "blind_scan_restarts_with_reliable_but_wrong_bytes": sum(r["false_start_reliable_wrong_bytes"] > 0 for r in rows),
           "zlib": zlib.ZLIB_RUNTIME_VERSION}
    Path(f"{outp}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
