#!/usr/bin/env python3
"""Sample REPDF original "Save As" Flate streams that stock zlib 1.3 reproduces (level 6, some memLevel/strategy),
and write (decoded bytes, original zlib stream) pairs plus an index for rust_replay/ to re-encode with
pure-Rust encoders (zlib-rs through libz-rs-sys, miniz_oxide).

    python3 rust_replay_prep.py /home/user/dfrc-korea/repdf <outdir> [--max 400]
"""
import re
import sys
import zlib
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)


def trim(b: bytes) -> bytes:
    return b[:-2] if b.endswith(b"\r\n") else b[:-1] if b.endswith((b"\n", b"\r")) else b


def main() -> None:
    corpus, out = Path(sys.argv[1]), Path(sys.argv[2])
    cap = int(sys.argv[sys.argv.index("--max") + 1]) if "--max" in sys.argv else 400
    out.mkdir(parents=True, exist_ok=True)
    rows, seen = [], 0
    for f in sorted((corpus / "original" / "saveas").rglob("*.pdf")):
        for m in STREAM_RE.finditer(f.read_bytes()):
            z = trim(m.group(1))
            try:
                raw = zlib.decompress(z)
            except zlib.error:
                continue
            seen += 1
            for mem in (8, 7, 9, 6, 5, 4, 3, 2, 1):
                for strat in (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED):
                    c = zlib.compressobj(6, zlib.DEFLATED, 15, mem, strat)
                    if c.compress(raw) + c.flush() == z:
                        break
                else:
                    continue
                break
            else:
                continue
            name = f"{len(rows):04d}"
            (out / f"{name}.raw").write_bytes(raw)
            (out / f"{name}.z").write_bytes(z)
            rows.append(f"{name}\t{mem}\t{strat}\t{f.relative_to(corpus)}\t{m.start(1)}")
            if len(rows) >= cap:
                break
        if len(rows) >= cap:
            break
    (out / "index.tsv").write_text("\n".join(rows) + "\n")
    print(f"streams seen {seen}, zlib-replayable written {len(rows)}")


if __name__ == "__main__":
    main()
