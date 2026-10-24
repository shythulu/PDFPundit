#!/usr/bin/env python3
"""Follow-up to c9_replay_correct.py: how often is a REPDF C9 damaged Flate stream *itself* a canonical
stock-zlib stream (its DEFLATE body is exactly what zlib re-compression of its own, wrong, decoded output
produces), so that only the Adler-32 trailer betrays the damage? For such a stream the encoder-replay
oracle (GAP-154) cannot localise the damage, and a 'correction' that rewrites the trailer to match the
damaged output passes inflate, Adler-32 and replay at once (found in the mobile_payment... ToUnicode
stream of c9_replay_correct.csv). Also counts how many of those trailer rewrites need only one byte,
i.e. would be found by a single-byte-substitution search.

    python3 c9_trailer_trap.py /home/user/dfrc-korea/repdf results/c9_trailer_trap [--max-comp 10000000]

Every damaged stream (all sizes; same selection as c9_replay_correct.jobs_for with every=1) whose
original is replayable by stock zlib (level 6, memLevel 1-9, default/filtered strategy).
Outputs <out>.csv and <out>.summary.json.
"""
import csv
import json
import sys
import zlib
from collections import Counter
from pathlib import Path

from c9_replay_correct import jobs_for, replays, trim, zcompress


def inflate_lenient(data: bytes) -> tuple[bytes, bool]:
    """Raw-inflate the body (skip the 2-byte header, ignore the trailer); True if it reaches the end."""
    d = zlib.decompressobj(-15)
    try:
        out = d.decompress(data[2:]) + d.flush()
    except zlib.error:
        return b"", False
    return out, d.eof


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    max_comp = int(sys.argv[sys.argv.index("--max-comp") + 1]) if "--max-comp" in sys.argv else 10 ** 7
    rows = []
    for job in jobs_for(corpus, max_comp, 1, 0):
        a, b = Path(job["orig"]).read_bytes(), Path(job["dam"]).read_bytes()
        s, e = job["span"]
        ob, db = trim(a[s:e]), trim(b[s:e])
        setting = replays(zlib.decompress(ob), ob)
        if not setting:
            continue
        pos = next(i for i in range(len(ob)) if ob[i] != db[i])
        dout, eof = inflate_lenient(db)
        mem, strat = map(int, setting.split("/"))
        self_consistent = eof and zcompress(dout, mem, strat)[:-4] == db[:-4]
        trailer_diff = None
        if self_consistent:
            want = zlib.adler32(dout).to_bytes(4, "big")
            trailer_diff = sum(x != y for x, y in zip(want, db[-4:]))
        rows.append({"file": job["rel"], "mode": job["mode"], "stream_start": s, "comp_len": len(db),
                     "true_pos": pos, "damage_in_trailer": pos >= len(db) - 4, "damaged_inflates_to_end": eof,
                     "setting": setting, "damaged_body_replay_consistent": self_consistent,
                     "trailer_bytes_to_rewrite": trailer_diff if trailer_diff is not None else "",
                     "decoded_len_orig": len(zlib.decompress(ob)), "decoded_len_damaged": len(dout)})
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(f"{out}.csv", "w", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    sc = [r for r in rows if r["damaged_body_replay_consistent"]]
    res = {"zlib": zlib.ZLIB_RUNTIME_VERSION, "replayable_damaged_streams": len(rows),
           "damage_in_trailer": sum(r["damage_in_trailer"] for r in rows),
           "damaged_inflates_to_end": sum(r["damaged_inflates_to_end"] for r in rows),
           "damaged_body_replay_consistent": len(sc),
           "replay_consistent_excluding_trailer_damage": sum(not r["damage_in_trailer"] for r in sc),
           "trailer_bytes_to_rewrite_counts": dict(sorted(Counter(r["trailer_bytes_to_rewrite"] for r in sc).items())),
           "by_mode": dict(Counter(r["mode"] for r in rows))}
    Path(f"{out}.summary.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
