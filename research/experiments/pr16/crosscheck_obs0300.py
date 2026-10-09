#!/usr/bin/env python3
"""pr16-ingest: match PR #16's damaged C9 Flate streams (c9exp.py partA.json) against the KB's own probe
(research/experiments/results/flate_c9_probe.csv, OBS-0300) stream by stream, and explain any difference.

Added by pr16-ingest (not part of PR #16).

usage: python3 crosscheck_obs0300.py <repdf-root> <partA.json> <flate_c9_probe.csv> <out.json>

Match key: (corrupted file, offset of the changed byte inside the stream, compressed length).
For streams only PR #16 has, it re-applies flate_c9_probe.py's body extraction (regex
`stream\\r?\\n(.*?)endstream` + trim of one trailing EOL) to the ORIGINAL file and reports whether
that body still inflates to Z_STREAM_END (the probe skips streams whose original does not).
"""
import collections, csv, json, os, re, sys, zlib

root, part_a, probe_csv, out = sys.argv[1:5]
STREAM_RE = re.compile(rb"stream\r?\n(.*?)endstream", re.S)   # as in flate_c9_probe.py


def trim_eol(body: bytes) -> bytes:                              # as in flate_c9_probe.py
    return body[:-2] if body.endswith(b"\r\n") else body[:-1] if body.endswith((b"\n", b"\r")) else body


a = [r for r in json.load(open(part_a)) if 'status' in r]
k = list(csv.DictReader(open(probe_csv)))
kk = {(r['file'].replace('corrupted/', '', 1), int(r['first_change']), int(r['comp_len'])): r for r in k}
pa = {(r['file'], r['off'], r['clen']): r for r in a}
outcome_map = {'adler': 'check-only', 'err': 'data-error', 'trunc': 'incomplete'}
agree = collections.Counter()
for key, r in pa.items():
    if key in kk:
        agree['same_outcome' if outcome_map[r['status']] == kk[key]['outcome'] else 'different_outcome'] += 1
only_pr16 = []
for key, r in sorted(pa.items()):
    if key in kk:
        continue
    o = open(os.path.join(root, 'original', r['file'].replace('_stream_zlib.pdf', '.pdf')), 'rb').read()
    span = next(((m.start(1), m.end(1)) for m in STREAM_RE.finditer(o) if m.start(1) <= r['pos'] < m.end(1)), None)
    info = {'file': r['file'], 'off': r['off'], 'clen': r['clen'], 'dlen': r['dlen'], 'status': r['status'],
            'kind': r['kind']}
    if span:
        raw = o[span[0]:span[1]]
        body = trim_eol(raw)
        d = zlib.decompressobj()
        d.decompress(body)
        info.update({'probe_body_len': len(body), 'raw_body_len': len(raw),
                     'last_zlib_byte': raw[r['clen'] - 1], 'bytes_after_zlib_data': list(raw[r['clen']:]),
                     'probe_body_inflates_to_end': d.eof})
    only_pr16.append(info)
res = {'pr16_damaged_flate_streams': len(pa), 'probe_damaged_flate_streams': len(kk),
       'matched': sum(agree.values()), 'matched_outcomes': dict(agree),
       'only_in_pr16': only_pr16, 'only_in_probe': [list(x) for x in kk if x not in pa],
       'pr16_status': dict(collections.Counter(r['status'] for r in a)),
       'probe_outcome': dict(collections.Counter(r['outcome'] for r in k))}
json.dump(res, open(out, 'w'), indent=1)
print(json.dumps(res, indent=1))
