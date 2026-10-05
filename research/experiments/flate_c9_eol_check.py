"""List C9 changed bytes that fall in a Flate stream span but after its trimmed zlib body (the EOL before
`endstream`). Explains OBS-0302 rev 2: 1446 Flate-span bytes vs 1445 damaged streams. Chair, 2026-10-05.
Usage: python3 research/experiments/flate_c9_eol_check.py /home/user/dfrc-korea/repdf"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import flate_c9_probe as p
corpus=Path(sys.argv[1])
for f in sorted((corpus/"corrupted").rglob("*_stream_zlib.pdf")):
    mode,kind=f.parent.parent.name,f.parent.name
    base=f.name[:-len("_stream_zlib.pdf")]
    a=(corpus/"original"/mode/kind/f"{base}.pdf").read_bytes(); b=f.read_bytes()
    if len(a)!=len(b): continue
    for s,e in p.body_spans(a):
        ob=p.trim_eol(a[s:e])
        if p.inflate(ob,False)["ret"]!=p.Z_STREAM_END: continue
        for i in range(s+len(ob),e):
            if a[i]!=b[i]: print(f.relative_to(corpus), 'span',s,e,'trimmed_len',len(ob),'pos',i-s,'orig',a[i],'new',b[i])
