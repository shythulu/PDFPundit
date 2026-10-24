#!/usr/bin/env python3
"""pr16-ingest: rebuild the cases/ directory that PR #16's mzbench and grammar_loc.py read.

PR #16 says the builder "was never saved" (experiments/README.md); only the file format is described:
  <id>.z    the corrupted zlib stream (exact compressed bytes from the C9 file)
  <id>.orig the original decoded bytes
  <id>.meta JSON with at least "status", "kind" and "off" (true damaged input offset)
This is a reconstruction from that description, not the original builder.  mzbench parses the meta
with string matching, so it is written with json.dumps' default separators and with "off" not last.

usage: python3 build_cases.py <repdf-root> <partA.json> <cases-dir>
"""
import json, os, sys, zlib, re
root, pa, outd = sys.argv[1:4]
os.makedirs(outd, exist_ok=True)
rows = [r for r in json.load(open(pa)) if 'status' in r]
cache = {}
n = 0
for r in rows:
    corr = os.path.join(root, 'corrupted', r['file'])
    orig = os.path.join(root, 'original', r['file'].replace('_stream_zlib.pdf', '.pdf'))
    for p in (corr, orig):
        if p not in cache: cache[p] = open(p, 'rb').read()
    s = r['pos'] - r['off']
    z = cache[corr][s:s + r['clen']]
    dec = zlib.decompress(cache[orig][s:s + r['clen']])
    assert len(dec) == r['dlen']
    cid = re.sub(r'[^A-Za-z0-9]+', '_', r['file'][:-4]) + f'_{r["pos"]}'
    open(os.path.join(outd, cid + '.z'), 'wb').write(z)
    open(os.path.join(outd, cid + '.orig'), 'wb').write(dec)
    meta = {k: r[k] for k in ('file', 'pos', 'clen', 'dlen', 'off', 'status', 'k', 'kind', 'msg')}
    open(os.path.join(outd, cid + '.meta'), 'w').write(json.dumps(meta))
    n += 1
print('cases', n)
