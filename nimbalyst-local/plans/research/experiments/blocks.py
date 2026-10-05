#!/usr/bin/env python3
"""Minimal DEFLATE block walker: counts blocks and block types per zlib stream (original, uncorrupted)."""
import glob, json, collections, sys

class Bits:
    def __init__(s, d): s.d = d; s.p = 0  # bit position
    def get(s, n):
        v = 0
        for i in range(n):
            byte = s.d[(s.p) >> 3]
            v |= ((byte >> (s.p & 7)) & 1) << i
            s.p += 1
        return v

def build(lengths):
    # canonical Huffman -> dict {(len, code): sym}
    maxl = max(lengths) if lengths else 0
    bl = collections.Counter(l for l in lengths if l)
    code = 0; nxt = {}
    for l in range(1, maxl + 1):
        code = (code + bl.get(l - 1, 0)) << 1
        nxt[l] = code
    t = {}
    for sym, l in enumerate(lengths):
        if l:
            t[(l, nxt[l])] = sym; nxt[l] += 1
    return t, maxl

def dec(b, tab):
    t, maxl = tab; code = 0
    for l in range(1, maxl + 1):
        code = (code << 1) | b.get(1)
        if (l, code) in t: return t[(l, code)]
    raise ValueError("bad code")

LBASE = [3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258]
LEXT = [0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0]
DEXT = [0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13]
ORD = [16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15]
FIXL = build([8]*144 + [9]*112 + [7]*24 + [8]*8); FIXD = build([5]*30)

def walk(z):
    b = Bits(z); b.p = 16  # skip zlib header
    types = []; starts = []
    while True:
        starts.append(b.p)
        final = b.get(1); bt = b.get(2); types.append(bt)
        if bt == 0:
            b.p = (b.p + 7) & ~7
            ln = b.get(16); b.get(16); b.p += 8 * ln
        else:
            if bt == 1: lt, dt = FIXL, FIXD
            elif bt == 2:
                hlit = b.get(5) + 257; hdist = b.get(5) + 1; hclen = b.get(4) + 4
                cl = [0] * 19
                for i in range(hclen): cl[ORD[i]] = b.get(3)
                ct = build(cl); lens = []
                while len(lens) < hlit + hdist:
                    s = dec(b, ct)
                    if s < 16: lens.append(s)
                    elif s == 16: lens += [lens[-1]] * (3 + b.get(2))
                    elif s == 17: lens += [0] * (3 + b.get(3))
                    else: lens += [0] * (11 + b.get(7))
                lt = build(lens[:hlit]); dt = build(lens[hlit:])
            else: raise ValueError("bt3")
            while True:
                s = dec(b, lt)
                if s < 256: continue
                if s == 256: break
                s -= 257; b.get(LEXT[s]); d = dec(b, dt); b.get(DEXT[d])
        if final: break
    return types


import importlib.util, sys, os, random
spec=importlib.util.spec_from_file_location('c9', 'c9exp.py'); c9=importlib.util.module_from_spec(spec); sys.argv=['x','repdf-repo']; spec.loader.exec_module(c9)
random.seed(7)
stats=collections.defaultdict(list)
files=sorted(glob.glob('repdf-repo/original/*/*/*.pdf'))
for fn in files:
    a=open(fn,'rb').read()
    for (s,clen,decoded) in c9.find_streams(a):
        if clen>300000: continue
        kind=c9.classify(decoded)
        try: t=walk(a[s:s+clen])
        except Exception as e: stats[kind].append(None); continue
        stats[kind].append(t)
for kind,v in stats.items():
    ok=[t for t in v if t is not None]
    nb=collections.Counter(min(len(t),5) for t in ok)
    stored=sum(any(x==0 for x in t) for t in ok)
    fixed=sum(any(x==1 for x in t) for t in ok)
    print(kind, 'streams',len(v),'parsed',len(ok),'blocks/stream (5=5+):',dict(sorted(nb.items())),'with stored',stored,'with fixed',fixed)
