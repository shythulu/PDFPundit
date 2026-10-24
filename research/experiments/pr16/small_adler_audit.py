#!/usr/bin/env python3
"""pr16-ingest: explain mzbench's non-exact and missing accepts on Adler-only C9 streams <= max_clen.

Added by pr16-ingest (not part of PR #16).  For every Adler-only case in <cases-dir> (built by
build_cases.py) with compressed length <= max_clen it reports:
  * trailer_damaged      - the true changed byte lies in the 4-byte Adler-32 trailer.  The committed
                           mzbench excludes those bytes from the search, so such cases cannot be fixed.
  * true_offset_collision- at the true offset, trying replacement values in ascending order (mzbench's
                           order), the first value that inflates to the end with a valid Adler-32 does
                           NOT reproduce the original decoded bytes (a genuine Adler-32 collision).
  * one_trailer_byte_fix - the corrupted stream's decoded output has an Adler-32 that differs from the
                           stored trailer in exactly one byte, so a search that includes trailer bytes
                           and scans backward from the end accepts a trailer edit (a false accept).
Uses CPython's zlib (raw inflate for the output, full zlib inflate for candidates).

usage: python3 small_adler_audit.py <cases-dir> <max_clen> <out.json>
"""
import glob, json, os, sys, zlib

cases, max_clen, out_path = sys.argv[1], int(sys.argv[2]), sys.argv[3]
rows = []
for m in sorted(glob.glob(os.path.join(cases, '*.meta'))):
    meta = json.load(open(m))
    if meta['status'] != 'adler':
        continue
    stem = m[:-5]
    d = open(stem + '.z', 'rb').read()
    if len(d) > max_clen:
        continue
    orig = open(stem + '.orig', 'rb').read()
    off = meta['off']
    r = {'id': os.path.basename(stem), 'kind': meta['kind'], 'clen': len(d), 'dlen': len(orig), 'off': off,
         'trailer_damaged': off >= len(d) - 4}
    if not r['trailer_damaged']:
        for v in range(256):
            if v == d[off]:
                continue
            c = bytearray(d); c[off] = v
            try:
                z = zlib.decompressobj(); out = z.decompress(bytes(c))
            except zlib.error:
                continue
            if z.eof:
                r['first_accepted_value'] = v
                r['true_offset_collision'] = out != orig
                r['accepted_output_len'] = len(out)
                break
    z = zlib.decompressobj(-15); out = z.decompress(d[2:])
    stored = z.unused_data[:4]
    r['trailer_bytes_differing'] = sum(x != y for x, y in zip(zlib.adler32(out).to_bytes(4, 'big'), stored))
    r['one_trailer_byte_fix'] = r['trailer_bytes_differing'] == 1 and not r['trailer_damaged']
    rows.append(r)
summary = {'cases': len(rows),
           'trailer_damaged': [x for x in rows if x['trailer_damaged']],
           'true_offset_collision': [x for x in rows if x.get('true_offset_collision')],
           'one_trailer_byte_fix': [x for x in rows if x['one_trailer_byte_fix']],
           'true_offset_exact': sum(1 for x in rows if x.get('true_offset_collision') is False)}
json.dump(summary, open(out_path, 'w'), indent=1)
print(json.dumps(summary, indent=1))
