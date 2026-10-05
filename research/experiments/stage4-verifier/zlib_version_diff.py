#!/usr/bin/env python3
"""Stage 4 verifier (2026-10-05): does stock zlib 1.3.2 emit the same deflate bytes as zlib 1.3?

TOOL-450 pins stock zlib for encoder replay; Stage 2 measured replay with zlib 1.3 (CPython's runtime,
Ubuntu zlib1g 1:1.3.dfsg) and left open whether the current release, 1.3.2, gives identical output.
For every Flate stream in the 100 REPDF originals (manifest research/experiments/results/
repdf_originals.inputs.sha256), this decodes the stream, re-encodes it with both libraries
(windowBits 15, default strategy) and compares the output byte for byte:
  - level 6 with memLevel 1..9 (the sweep WP-3.3 uses), and
  - levels 1..9 with memLevel 8,
and counts how often each library reproduces the stream's original compressed bytes.

usage: python zlib_version_diff.py <repdf-root> <libz.so.1.3.2> <out-prefix>
zlib 1.3.2 build: github.com/madler/zlib/releases/download/v1.3.2/zlib-1.3.2.tar.gz
  (sha256 bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16),
  ./configure --prefix=$S/pins/zlib-1.3.2 && make -j2 && make install
"""
import ctypes, hashlib, json, re, sys, time, zlib
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
MANIFEST = REPO / "research/experiments/results/repdf_originals.inputs.sha256"


class ZStream(ctypes.Structure):
    _fields_ = [("next_in", ctypes.c_void_p), ("avail_in", ctypes.c_uint), ("total_in", ctypes.c_ulong),
                ("next_out", ctypes.c_void_p), ("avail_out", ctypes.c_uint), ("total_out", ctypes.c_ulong),
                ("msg", ctypes.c_char_p), ("state", ctypes.c_void_p), ("zalloc", ctypes.c_void_p),
                ("zfree", ctypes.c_void_p), ("opaque", ctypes.c_void_p), ("data_type", ctypes.c_int),
                ("adler", ctypes.c_ulong), ("reserved", ctypes.c_ulong)]


class Lib:
    def __init__(self, path):
        self.z = ctypes.CDLL(path)
        self.z.zlibVersion.restype = ctypes.c_char_p
        self.version = self.z.zlibVersion().decode()
        self.z.deflateBound.restype = ctypes.c_ulong
        self.z.deflateBound.argtypes = [ctypes.POINTER(ZStream), ctypes.c_ulong]

    def compress(self, data, level, mem):
        s = ZStream()
        rc = self.z.deflateInit2_(ctypes.byref(s), level, 8, 15, mem, 0, self.version.encode(), ctypes.sizeof(ZStream))
        assert rc == 0, rc
        n = self.z.deflateBound(ctypes.byref(s), len(data)) + 64
        inb = ctypes.create_string_buffer(data, len(data)); outb = ctypes.create_string_buffer(n)
        s.next_in = ctypes.addressof(inb); s.avail_in = len(data)
        s.next_out = ctypes.addressof(outb); s.avail_out = n
        rc = self.z.deflate(ctypes.byref(s), 4)
        assert rc == 1, rc
        out = outb.raw[:s.total_out]
        self.z.deflateEnd(ctypes.byref(s))
        return out


def streams(buf):
    for m in re.finditer(rb"stream(\r\n|\n)", buf):
        s = m.end()
        if buf[s - len(m.group(0)) - 3:s - len(m.group(0))] == b"end":
            continue
        d = zlib.decompressobj()
        try:
            dec = d.decompress(buf[s:s + 50_000_000])
        except zlib.error:
            continue
        if not d.eof:
            continue
        clen = len(buf[s:s + 50_000_000]) - len(d.unused_data)
        yield buf[s:s + clen], dec


def main(root, lib132, out):
    root = Path(root); new = Lib(lib132)
    combos = [(6, m) for m in range(1, 10)] + [(l, 8) for l in range(1, 10) if l != 6]
    files = [ln.split(None, 1) for ln in MANIFEST.read_text().splitlines() if ln.strip()]
    t0 = time.time(); per = {f"L{l}M{m}": Counter() for l, m in combos}; nstreams = Counter(); bad_hash = []
    first_diff = []
    for sha, rel in files:
        buf = (root / rel).read_bytes()
        if hashlib.sha256(buf).hexdigest() != sha:
            bad_hash.append(rel); continue
        prod = rel.split("/")[1]
        for comp, dec in streams(buf):
            nstreams[prod] += 1
            body = comp  # includes the 2-byte zlib header and the Adler-32 trailer
            for l, m in combos:
                c = per[f"L{l}M{m}"]
                co = zlib.compressobj(l, zlib.DEFLATED, 15, m); a = co.compress(dec) + co.flush()
                b = new.compress(dec, l, m)
                c[f"{prod}_streams"] += 1
                c[f"{prod}_identical"] += a == b
                c[f"{prod}_old_reproduces_original"] += a == body
                c[f"{prod}_new_reproduces_original"] += b == body
                if a != b and len(first_diff) < 5:
                    first_diff.append({"file": rel, "level": l, "memLevel": m, "len_old": len(a), "len_new": len(b)})
    res = {"zlib_old": f"{zlib.ZLIB_RUNTIME_VERSION} (CPython runtime; system zlib1g)", "zlib_new": new.version,
           "files": len(files) - len(bad_hash), "hash_mismatch": bad_hash, "streams": dict(nstreams),
           "combos": {k: dict(v) for k, v in per.items()},
           "all_identical": all(v[f"{p}_identical"] == v[f"{p}_streams"] for v in per.values() for p in nstreams),
           "first_differences": first_diff, "wall_s": round(time.time() - t0, 1)}
    Path(out + ".summary.json").write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps({k: res[k] for k in ("zlib_old", "zlib_new", "files", "streams", "all_identical", "wall_s")}))
    print(json.dumps({k: res["combos"][k] for k in ("L6M8", "L6M9", "L6M7")}))


if __name__ == "__main__":
    main(*sys.argv[1:4])
