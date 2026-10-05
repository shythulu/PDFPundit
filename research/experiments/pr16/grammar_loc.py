#!/usr/bin/env python3
"""Grammar-based localizer for Adler-only C9 cases in content/cmap/text streams.
Finds the first ungrammatical token in the corrupted output, maps it to an input offset via
a byte-feed trace, and compares with the true corrupted input offset."""
import glob, json, re, zlib, collections, bisect, sys, os
# PATCH (pr16-ingest, 2026-10-05): cases directory from argv[1] (upstream hard-codes ./cases).
CASES = sys.argv[1] if len(sys.argv) > 1 else "cases"

OPS = set(b"b B b* B* BDC BI BMC BT BX c cm CS cs d d0 d1 Do DP EI EMC ET EX f F f* G g gs h i ID j J K k l m M MP n q Q re RG rg ri s S SC sc SCN scn sh T* Tc Td TD Tf TJ Tj TL Tm Tr Ts Tw Tz v w W W* y ' \"".split())
CMAPW = set(b"begincmap endcmap beginbfchar endbfchar beginbfrange endbfrange begincodespacerange endcodespacerange findresource begin dict def end currentdict defineresource pop usecmap beginnotdefrange endnotdefrange begincidrange endcidrange begincidchar endcidchar".split())
WS = b"\x00\t\n\x0c\r "
DELIM = b"()<>[]{}/%"
NUM = re.compile(rb"[+-]?(\d+\.?\d*|\.\d+)$")

def first_bad(buf, kind):
    i, n = 0, len(buf)
    while i < n:
        c = buf[i]
        if c in WS: i += 1; continue
        if c == 0x25:  # % comment
            j = buf.find(b"\n", i); i = n if j < 0 else j + 1; continue
        if c == 0x28:  # literal string
            depth, j = 1, i + 1
            while j < n and depth:
                if buf[j] == 0x5c: j += 2; continue
                if buf[j] == 0x28: depth += 1
                elif buf[j] == 0x29: depth -= 1
                j += 1
            if depth: return i
            i = j; continue
        if c == 0x3c:
            if i + 1 < n and buf[i+1] == 0x3c: i += 2; continue
            j = buf.find(b">", i)
            if j < 0: return i
            body = buf[i+1:j]
            if not re.fullmatch(rb"[0-9A-Fa-f\s]*", body): return i
            i = j + 1; continue
        if c == 0x3e:
            if i + 1 < n and buf[i+1] == 0x3e: i += 2; continue
            return i
        if c in b"[]{}": i += 1; continue
        if c == 0x29: return i
        if c == 0x2f:  # name
            j = i + 1
            while j < n and buf[j] not in WS and buf[j] not in DELIM: j += 1
            if any(x < 0x21 or x > 0x7e for x in buf[i+1:j]): return i
            i = j; continue
        j = i
        while j < n and buf[j] not in WS and buf[j] not in DELIM: j += 1
        tok = buf[i:j]
        if NUM.match(tok) or tok in (b"true", b"false", b"null"): i = j; continue
        if kind.startswith("content") and tok in OPS: i = j; continue
        if kind == "cmap" and (tok in CMAPW or tok.isalpha()): i = j; continue
        if kind == "text-other" and (tok in OPS or tok in CMAPW or tok.isalpha()): i = j; continue
        return i
    return None

def trace_decode(z):
    d = zlib.decompressobj(); outl = []; out = bytearray()
    for k in range(len(z)):
        try: out += d.decompress(z[k:k+1])
        except zlib.error: break
        outl.append(len(out))
    return bytes(out), outl

res = collections.Counter(); dists = []
for f in sorted(glob.glob(os.path.join(CASES, "*.meta"))):
    m = json.load(open(f))
    if m["status"] != "adler" or m["kind"] not in ("content", "content-graphics", "cmap", "text-other"): continue
    z = open(f[:-5] + ".z", "rb").read(); orig = open(f[:-5] + ".orig", "rb").read()
    out, outl = trace_decode(z)
    # sanity: original must be grammatical under our tokenizer
    if first_bad(orig, m["kind"]) is not None: res["orig_flagged"] += 1; continue
    fb = first_bad(out, m["kind"])
    if fb is None: res["no_flag"] += 1; continue
    kin = bisect.bisect_left(outl, fb + 1)  # input bytes needed to emit output byte fb
    d = kin - m["off"]
    dists.append(d)
    res["flagged"] += 1
    if d >= -8: res["true_before_flag"] += 1
print(res)
dists.sort()
q = lambda p: dists[int(p * (len(dists) - 1))]
print("input distance flag - true_off quantiles:", [q(p) for p in (0, .05, .1, .25, .5, .75, .9, .95, 1)])
for W in (64, 256, 1024, 4096):
    print(f"true off within [flag-{W}, flag+8]:", round(sum(-8 <= d <= W for d in dists) / len(dists), 3))

# write flags for the Rust bench
for f in sorted(glob.glob(os.path.join(CASES, "*.meta"))):
    m = json.load(open(f))
    if m["status"] != "adler" or m["kind"] not in ("content", "content-graphics", "cmap", "text-other"): continue
    z = open(f[:-5] + ".z", "rb").read(); orig = open(f[:-5] + ".orig", "rb").read()
    if first_bad(orig, m["kind"]) is not None: continue
    out, outl = trace_decode(z)
    fb = first_bad(out, m["kind"])
    if fb is None: continue
    open(f[:-5] + ".flag", "w").write(str(bisect.bisect_left(outl, fb + 1)))
