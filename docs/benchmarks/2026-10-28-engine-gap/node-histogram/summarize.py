import re,sys,glob
from pathlib import Path
HERE = Path(__file__).resolve().parent
"""Per-empties node counts: deft HIST lines vs Egaroucid STATS lines (final iteration and all iterations)."""
def deft(i):
    rows=[]
    for l in open(HERE / (sys.argv[1] if len(sys.argv) > 1 else 'raw-stageB') / f'ffo{i}.txt'):
        m=re.match(r'HIST depth (\d+) final (\w+) quality (\d+) nodes \[(.*)\]',l)
        if m: rows.append((m.group(2)=='true' and m.group(3)=='0', [int(x) for x in m.group(4).split(',')]))
    return rows
def egar(i):
    rows=[]
    for l in open(HERE / 'raw-egaroucid' / f'ffo{i}.txt'):
        if l.startswith('STATS'):
            p=l.split(); mpc=int(p[4]); vals=[int(x) for x in p[7:]]
            byempt=[0]*65
            for d,v in enumerate(vals): byempt[64-d]+=v
            rows.append((mpc==6 and p[6]=='1', byempt))
    return rows
tot={'deft':[0]*65,'egar':[0]*65,'deftall':[0]*65,'egarall':[0]*65}
for i in range(40,50):
    for name,rows in (('deft',deft(i)),('egar',egar(i))):
        fin=[r for f,r in rows if f]; assert len(fin)==1,(name,i,len(fin))
        for e in range(65):
            tot[name][e]+=fin[0][e]
            tot[name+'all'][e]+=sum(r[e] for f,r in rows)
print('empties  deft_final  egar_final  ratio   | deft_all  egar_all ratio')
for e in range(4,27):
    a,b=tot['deft'][e],tot['egar'][e]; c,d=tot['deftall'][e],tot['egarall'][e]
    print(f'{e:3d} {a:12d} {b:12d} {a/max(b,1):6.2f} | {c:12d} {d:12d} {c/max(d,1):6.2f}')
s=lambda k,lo,hi: sum(tot[k][lo:hi+1])
for lo,hi in ((4,4),(5,6),(7,10),(11,13),(14,30),(7,30),(4,30)):
    print(f'{lo}-{hi}: deft {s("deft",lo,hi)} egar {s("egar",lo,hi)} ratio {s("deft",lo,hi)/max(1,s("egar",lo,hi)):.3f}; all-iter ratio {s("deftall",lo,hi)/max(1,s("egarall",lo,hi)):.3f}')
