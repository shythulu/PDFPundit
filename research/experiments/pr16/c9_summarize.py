#!/usr/bin/env python3
"""pr16-ingest: summarise c9exp.py's partA.json / partB.json into the numbers PR #16 reports (E1-E3).

Added by pr16-ingest (not part of PR #16). It recomputes, from the raw rows written by c9exp.py
and the REPDF corpus, every E1-E3 figure the PR #16 report quotes, so that each can be checked.

usage: python3 c9_summarize.py <repdf-root> <dir-with-partA.json-and-partB.json> <out.json>

Definitions (chosen to match the PR #16 text; where the text is ambiguous the choice is stated):
  * 'Flate row'      = partA row that has a 'status' field (c9exp mapped the diff into a zlib stream).
  * dist             = k - 1 - off (c9exp's definition; k = input bytes fed when zlib raised).
  * prefix+suffix    = (common prefix + common suffix of corrupted output vs original, capped so the
                       two never overlap) / original decoded length.  Recomputed here from the corpus;
                       the corrupted output is every byte zlib emits before it raises (feed_keep).
                       PATCH 2026-10-05: an earlier version fed 64 KiB chunks without the trailer and lost
                       the output of the chunk that raised (it gave 0.5637 / 0.6641).
  * Part B 'damage lands in first 4 KiB of output' uses the common-prefix length (prefix_frac * dlen)
    as the output position of the damage, because partB.json stores nothing else (see c9exp.py line 127).
"""
import json, os, sys, zlib, statistics, collections, re, glob

root, rdir, outp = sys.argv[1], sys.argv[2], sys.argv[3]
A = json.load(open(os.path.join(rdir, 'partA.json')))
B = json.load(open(os.path.join(rdir, 'partB.json')))

def q(v, p):
    v = sorted(v); return v[int(p * (len(v) - 1))] if v else None

S = {}
files = sorted({r['file'] for r in A})
S['A_files'] = len(files)
S['A_rows'] = len(A)
nd = {}
for r in A:
    nd[r['file']] = r.get('ndiffs', nd.get(r['file']))
per_file = [nd[f] for f in files]
S['A_bytes_per_file_min_max_mean'] = [min(per_file), max(per_file), round(statistics.mean(per_file), 2)]
S['A_len_differs_rows'] = sum(1 for r in A if str(r.get('note', '')).startswith('len differs'))
F = [r for r in A if 'status' in r]
S['A_flate_rows'] = len(F)
S['A_flate_share'] = round(len(F) / len(A), 4)
S['A_nonflate_rows'] = len(A) - len(F)
streams_hit = collections.Counter((r['file'], r['pos'] - r['off']) for r in F)
S['A_max_diffs_per_damaged_stream'] = max(streams_hit.values())
S['A_damaged_streams'] = len(streams_hit)
dpf = collections.Counter(f for f, _ in streams_hit)
dpf_l = [dpf.get(f, 0) for f in files]
S['A_damaged_streams_per_file_min_max_mean'] = [min(dpf_l), max(dpf_l), round(statistics.mean(dpf_l), 2)]
ham = collections.Counter(r['hamming'] for r in A if 'hamming' in r)
S['A_hamming_hist_all_rows'] = dict(sorted(ham.items()))
S['A_single_bit'] = ham.get(1, 0)
S['A_single_bit_share'] = round(ham.get(1, 0) / sum(ham.values()), 4)
S['A_kinds'] = dict(collections.Counter(r['kind'] for r in F))
st = collections.Counter(r['status'] for r in F)
S['A_status'] = dict(st)
S['A_status_share'] = {k: round(v / len(F), 4) for k, v in st.items()}
E = [r for r in F if r['status'] == 'err']
S['A_err_msgs'] = dict(collections.Counter(r['msg'] for r in E))
d = [r['dist'] for r in E]
S['A_err_dist_quantiles_p10_p25_p50_p75_p90_p95_p99_max'] = [q(d, p) for p in (.1, .25, .5, .75, .9, .95, .99, 1)]
S['A_err_dist_median_statistics'] = statistics.median(d)
S['A_err_dist_min'] = min(d)
S['A_err_within_8_64_1024'] = [round(sum(x <= w for x in d) / len(d), 4) for w in (8, 64, 1024)]
S['A_err_in_window'] = round(sum(r['in_window'] for r in E) / len(E), 4)
S['A_err_by_kind'] = {k: [sum(1 for r in E if r['kind'] == k), sum(1 for r in F if r['kind'] == k)] for k in sorted({r['kind'] for r in F})}
AD = [r for r in F if r['status'] == 'adler']
same_len = [r for r in AD if r.get('adler_only_bytes_correct_frac') is not None]
S['A_adler_same_len'] = [len(same_len), len(AD), round(len(same_len) / len(AD), 4)]
S['A_adler_same_len_median_correct_frac'] = statistics.median(r['adler_only_bytes_correct_frac'] for r in same_len)
S['A_mean_prefix_frac_all'] = round(statistics.mean(r['prefix_frac'] for r in F), 4)
S['A_mean_prefix_frac_adler'] = round(statistics.mean(r['prefix_frac'] for r in AD), 4)
fonts_ad = [r for r in AD if r['kind'] == 'font']
S['A_adler_fonts'] = len(fonts_ad)
S['A_adler_fonts_clen_median'] = statistics.median(r['clen'] for r in fonts_ad)
S['A_adler_fonts_le16KiB_share'] = round(sum(r['clen'] <= 16384 for r in fonts_ad) / len(fonts_ad), 4)
S['A_adler_fonts_le4KiB_share'] = round(sum(r['clen'] <= 4096 for r in fonts_ad) / len(fonts_ad), 4)
S['A_adler_le4KiB'] = sum(r['clen'] <= 4096 for r in AD)
S['A_adler_4to16KiB'] = sum(4096 < r['clen'] <= 16384 for r in AD)
S['A_adler_grammar_kinds'] = sum(r['kind'] in ('content', 'content-graphics', 'cmap', 'text-other') for r in AD)
S['A_in_trailer'] = sum(r['in_trailer'] for r in F)
S['A_in_header'] = sum(r['in_header'] for r in F)

# Recompute exact wrong-byte counts and prefix+suffix from the corpus (c9exp stores only rounded fractions).
cache = {}
def load(p):
    if p not in cache: cache[p] = open(p, 'rb').read()
    return cache[p]
def feed_keep(data, chunk=4096):
    # Inflate and keep every byte emitted before zlib raises (a raising decompress() call loses its
    # own output, so a failing chunk is replayed byte by byte from a snapshot of the decoder).
    d = zlib.decompressobj(); out = bytearray(); i = 0
    while i < len(data) and not d.eof:
        c = data[i:i + chunk]; snap = d.copy()
        try:
            out += d.decompress(c); i += len(c)
        except zlib.error:
            d = snap
            for j in range(len(c)):
                try: out += d.decompress(c[j:j + 1])
                except zlib.error: break
            break
    return bytes(out)
wrong_same_len, ps_all, ps_ad = [], [], []
for r in F:
    a = load(os.path.join(root, 'original', r['file'].replace('_stream_zlib.pdf', '.pdf')))
    b = load(os.path.join(root, 'corrupted', r['file']))
    s = r['pos'] - r['off']
    dec = zlib.decompress(a[s:s + r['clen']])
    out = feed_keep(b[s:s + r['clen']])
    n = min(len(out), len(dec)); cp = 0
    while cp < n and out[cp] == dec[cp]: cp += 1
    cs = 0
    while cs < n - cp and out[-1 - cs] == dec[-1 - cs]: cs += 1
    frac = (cp + cs) / len(dec)
    ps_all.append(frac)
    if r['status'] == 'adler':
        ps_ad.append(frac)
        if len(out) == len(dec):
            wrong_same_len.append(sum(1 for x, y in zip(out, dec) if x != y))
S['A_mean_prefix_plus_suffix_all'] = round(statistics.mean(ps_all), 4)
S['A_mean_prefix_plus_suffix_adler'] = round(statistics.mean(ps_ad), 4)
S['A_adler_same_len_wrong_bytes_p10_p50_p75_p90'] = [q(wrong_same_len, p) for p in (.1, .5, .75, .9)]

# Flate streams per original (all zlib streams that c9exp.find_streams accepts, any length)
import importlib.util
spec = importlib.util.spec_from_file_location('c9', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'c9exp.py'))
c9 = importlib.util.module_from_spec(spec); sys.argv = ['x', root]; spec.loader.exec_module(c9)
nst = [len(c9.find_streams(load(p))) for p in sorted(glob.glob(os.path.join(root, 'original/*/*/*.pdf')))]
S['originals'] = len(nst)
S['flate_streams_per_original_mean'] = round(statistics.mean(nst), 2)
S['flate_streams_total_originals'] = sum(nst)

# Part B
S['B_trials'] = len(B)
stB = collections.Counter(r['status'] for r in B)
S['B_status_share'] = {k: round(v / len(B), 4) for k, v in stB.items()}
EB = [r for r in B if r['status'] == 'err']
dB = [r['dist'] for r in EB]
S['B_err_dist_p50_p90_p95_p99_max'] = [q(dB, p) for p in (.5, .9, .95, .99, 1)]
S['B_err_in_window'] = round(sum(-64 <= x <= 4096 for x in dB) / len(dB), 4)
def band(r):
    o = r['prefix_frac'] * r['dlen']
    return '0-4KiB' if o < 4096 else ('4-32KiB' if o < 32768 else '>32KiB')
bands = collections.defaultdict(lambda: [0, 0])
for r in B:
    b_ = band(r); bands[b_][1] += 1; bands[b_][0] += r['status'] == 'err'
S['B_early_error_rate_by_output_band'] = {k: [v[0], v[1], round(v[0] / v[1], 4)] for k, v in sorted(bands.items())}
kinds = collections.defaultdict(lambda: [0, 0])
for r in B:
    kinds[r['kind']][1] += 1; kinds[r['kind']][0] += r['status'] == 'err'
S['B_early_error_rate_by_kind'] = {k: [v[0], v[1], round(v[0] / v[1], 4)] for k, v in sorted(kinds.items())}
json.dump(S, open(outp, 'w'), indent=1)
print(json.dumps(S, indent=1))
