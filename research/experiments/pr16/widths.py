import sys, math, collections, os
# PATCH (pr16-ingest, 2026-10-05): fontTools path from $PYLIB (upstream: ./pylib).
sys.path.insert(0, os.environ.get('PYLIB', 'pylib'))
from fontTools.ttLib import TTFont
# approx English letter freqs (%), Wikipedia "Letter frequency" table
F=dict(zip("abcdefghijklmnopqrstuvwxyz",[8.2,1.5,2.8,4.3,12.7,2.2,2.0,6.1,7.0,0.15,0.77,4.0,2.4,6.7,7.5,1.9,0.095,6.0,6.3,9.1,2.8,0.98,2.4,0.15,2.0,0.074]))
fonts={'LiberationSans':'/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf',
'LiberationSerif':'/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf',
'DejaVuSans':'/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
'FreeSans':'/usr/share/fonts/truetype/freefont/FreeSans.ttf'}
charset="abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,;:!?'\"()-"
vecs={}
for name,p in fonts.items():
    f=TTFont(p); cm=f.getBestCmap(); hm=f['hmtx']; upm=f['head'].unitsPerEm
    w={c: round(hm[cm[ord(c)]][0]*1000/upm) for c in charset if ord(c) in cm}
    vecs[name]=w
    lw={c:w[c] for c in "abcdefghijklmnopqrstuvwxyz"}
    cls=collections.defaultdict(list)
    for c,v in lw.items(): cls[v].append(c)
    uniq=sum(1 for v in cls.values() if len(v)==1)
    tot=sum(F.values()); H=-sum(F[c]/tot*math.log2(F[c]/tot) for c in F)
    Hc=0
    for v,cs in cls.items():
        pv=sum(F[c] for c in cs)/tot
        Hc+= -sum((F[c]/tot)*math.log2((F[c]/tot)/pv) for c in cs)
    allcls=collections.defaultdict(list)
    for c,v in w.items(): allcls[v].append(c)
    print(f"{name}: glyphs={len(f.getGlyphOrder())} lowercase width classes={len(cls)} uniquely-identified={uniq}/26 largest class={max(len(v) for v in cls.values())} H(letter)={H:.2f} bits H(letter|width)={Hc:.2f} bits; all {len(w)} chars -> {len(allcls)} width classes")
    print("   classes:", sorted(((v,''.join(cs)) for v,cs in cls.items()), key=lambda x:-len(x[1]))[:4])
# font discrimination: how many chars must be seen before widths separate fonts
names=list(vecs)
for i in range(len(names)):
    for j in range(i+1,len(names)):
        a,b=vecs[names[i]],vecs[names[j]]
        diff=sum(1 for c in charset if c in a and c in b and abs(a[c]-b[c])>1)
        print(f"{names[i]} vs {names[j]}: {diff}/{len(charset)} chars differ in width (>1/1000 em)")
# CJK
f=TTFont('/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc',fontNumber=0); cm=f.getBestCmap(); hm=f['hmtx']; upm=f['head'].unitsPerEm
han=[u for u in cm if 0x4E00<=u<=0x9FFF]
ws=collections.Counter(round(hm[cm[u]][0]*1000/upm) for u in han)
print("WQY ZenHei Han chars:",len(han),"distinct widths:",ws.most_common(3))
# Arabic in DejaVu Sans
f=TTFont(fonts['DejaVuSans']); cm=f.getBestCmap(); hm=f['hmtx']; upm=f['head'].unitsPerEm
ar=[u for u in cm if 0x0621<=u<=0x064A]
ws=collections.Counter(round(hm[cm[u]][0]*1000/upm) for u in ar)
print("DejaVuSans Arabic base letters:",len(ar),"distinct widths:",len(ws), ws.most_common(4))
pf=[u for u in cm if 0xFE70<=u<=0xFEFC]
ws=collections.Counter(round(hm[cm[u]][0]*1000/upm) for u in pf)
print("DejaVuSans Arabic presentation forms:",len(pf),"distinct widths:",len(ws))
