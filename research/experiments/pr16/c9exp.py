#!/usr/bin/env python3
"""C9 experiment on the REPDF corpus.
Part A: diff each *_stream_zlib.pdf against its original, locate the modified byte(s),
        map to the Flate stream, and characterise what inflate does.
Part B: synthetic random-byte replacement in the originals' Flate streams to measure
        error-surfacing distance, Adler-only detection, prefix recovery, window coverage.
"""
import os, re, sys, zlib, random, json, glob, time
from collections import Counter

ROOT = sys.argv[1]
random.seed(1234)
# PATCH (pr16-ingest, 2026-10-05): outputs go to $C9_OUT (default: next to the script, as upstream).
OUTDIR = os.environ.get('C9_OUT', os.path.dirname(os.path.abspath(__file__)))

def find_streams(buf):
    """Return list of (data_start, comp_len, decoded) for zlib streams following 'stream' keyword."""
    out = []
    for m in re.finditer(rb'stream(\r\n|\n)', buf):
        s = m.end()
        if buf[s-len(m.group(0))-3:s-len(m.group(0))] == b'end':
            continue
        d = zlib.decompressobj()
        try:
            dec = d.decompress(buf[s:s+50_000_000])
        except zlib.error:
            continue
        if not d.eof:
            continue
        clen = len(buf[s:s+50_000_000]) - len(d.unused_data)
        out.append((s, clen, dec))
    return out

def feed_bytewise(data):
    """Inflate one byte at a time. Returns (status, k, out, trace)
    status in {'ok','err','adler','trunc'}; k = input bytes fed when error raised.
    trace[i] = output length after feeding i+1 input bytes."""
    d = zlib.decompressobj()
    out = bytearray(); trace = []
    for i in range(len(data)):
        try:
            out += d.decompress(data[i:i+1])
        except zlib.error as e:
            msg = str(e)
            st = 'adler' if 'incorrect data check' in msg else 'err'
            return st, i + 1, bytes(out), trace, msg
        trace.append(len(out))
        if d.eof:
            return 'ok', i + 1, bytes(out), trace, ''
    return 'trunc', len(data), bytes(out), trace, ''

def common_prefix(a, b):
    n = min(len(a), len(b)); i = 0
    # fast chunked compare
    step = 4096
    while i + step <= n and a[i:i+step] == b[i:i+step]:
        i += step
    while i < n and a[i] == b[i]:
        i += 1
    return i

def classify(dec):
    head = dec[:4096]
    if b'BT' in head and (b'Tj' in head or b'TJ' in head or b'Tf' in head):
        return 'content'
    if re.search(rb'(^|\s)(re|cm|Do|m|l|f|S|q|Q)(\s|$)', head) and sum(32 <= c < 127 or c in (9,10,13) for c in head) > 0.95*len(head):
        return 'content-graphics'
    if head[:4] in (b'\x00\x01\x00\x00', b'OTTO', b'true', b'ttcf'):
        return 'font'
    if b'begincmap' in head:
        return 'cmap'
    if b'<?xpacket' in head or b'x:xmpmeta' in head:
        return 'xmp'
    if sum(32 <= c < 127 or c in (9,10,13) for c in head) > 0.95*max(1,len(head)):
        return 'text-other'
    return 'binary'

def partA():
    rows = []
    for cor in sorted(glob.glob(os.path.join(ROOT, 'corrupted/*/*/*_stream_zlib.pdf'))):
        rel = os.path.relpath(cor, os.path.join(ROOT, 'corrupted'))
        orig = os.path.join(ROOT, 'original', rel.replace('_stream_zlib.pdf', '.pdf'))
        if not os.path.exists(orig):
            continue
        a = open(orig, 'rb').read(); b = open(cor, 'rb').read()
        if len(a) != len(b):
            rows.append({'file': rel, 'note': f'len differs {len(a)} {len(b)}'}); continue
        diffs = [i for i in range(len(a)) if a[i] != b[i]] if a != b else []
        streams = find_streams(a)
        for pos in diffs:
            hit = None
            for (s, clen, dec) in streams:
                if s <= pos < s + clen:
                    hit = (s, clen, dec); break
            r = {'file': rel, 'pos': pos, 'old': a[pos], 'new': b[pos],
                 'hamming': bin(a[pos] ^ b[pos]).count('1'), 'ndiffs': len(diffs)}
            if hit:
                s, clen, dec = hit
                off = pos - s
                data = b[s:s+clen]
                st, k, out, trace, msg = feed_bytewise(data)
                cp = common_prefix(out, dec)
                r.update({'clen': clen, 'dlen': len(dec), 'off': off, 'rel_off': round(off/clen, 3),
                          'status': st, 'k': k, 'dist': k - 1 - off, 'msg': msg,
                          'prefix_correct': cp, 'prefix_frac': round(cp/len(dec), 4),
                          'out_len': len(out), 'kind': classify(dec),
                          'in_window': (k - 1 - 4096) <= off <= (k - 1 + 64),
                          'in_trailer': off >= clen - 4, 'in_header': off < 2})
                # whole-output correctness if we just keep everything (Adler-only case)
                if st == 'adler':
                    same = sum(1 for x, y in zip(out, dec) if x == y)
                    r['adler_only_bytes_correct_frac'] = round(same / len(dec), 4) if len(out) == len(dec) else None
            else:
                r['note'] = 'diff not inside a parsed zlib stream'
            rows.append(r)
    return rows

def synth(streams_all, trials=3000):
    res = []
    for t in range(trials):
        s, clen, dec, kind = random.choice(streams_all)
        data = bytearray(streams_all_data[(s, clen, id(dec))])
        off = random.randrange(clen)
        old = data[off]
        new = random.choice([v for v in range(256) if v != old])
        data[off] = new
        st, k, out, trace, msg = feed_bytewise(bytes(data))
        cp = common_prefix(out, dec)
        # output offset where divergence starts vs input position of corruption
        r = {'clen': clen, 'dlen': len(dec), 'off': off, 'status': st, 'k': k,
             'dist': k - 1 - off, 'prefix_frac': cp / len(dec), 'kind': kind,
             'hamming': bin(old ^ new).count('1'), 'msg': msg}
        if st == 'adler' and len(out) == len(dec):
            r['adler_same_frac'] = sum(1 for x, y in zip(out, dec) if x == y) / len(dec)
            r['adler_same_len'] = True
        elif st == 'adler':
            r['adler_same_len'] = False
        res.append(r)
    return res

if __name__ == '__main__':
    t0 = time.time()
    if os.environ.get('C9_SKIP_A') and os.path.exists(os.path.join(OUTDIR, 'partA.json')):
        rowsA = json.load(open(os.path.join(OUTDIR, 'partA.json')))  # PATCH: allow re-running Part B only
    else:
        rowsA = partA()
    json.dump(rowsA, open(os.path.join(OUTDIR, 'partA.json'), 'w'), indent=1)
    print('PartA rows', len(rowsA), 'time', round(time.time()-t0, 1))
    # Part B corpus of streams from originals
    streams_all = []; streams_all_data = {}
    for orig in sorted(glob.glob(os.path.join(ROOT, 'original/*/*/*.pdf'))):
        a = open(orig, 'rb').read()
        for (s, clen, dec) in find_streams(a):
            if clen < 64 or clen > 300_000:
                continue
            kind = classify(dec)
            key = (s, clen, id(dec))
            streams_all_data[key] = a[s:s+clen]
            streams_all.append((s, clen, dec, kind))
    print('streams', len(streams_all), Counter(k for *_, k in streams_all))
    rowsB = synth(streams_all, trials=int(sys.argv[2]) if len(sys.argv) > 2 else 2000)
    json.dump(rowsB, open(os.path.join(OUTDIR, 'partB.json'), 'w'))
    print('PartB done', round(time.time()-t0, 1))
