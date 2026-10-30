import os; HERE = os.path.dirname(os.path.abspath(__file__)); DEFT_DIR = os.environ.get('DEFT_DIR', os.path.join(HERE, 'raw-deft-f790b3f'))
import re,sys
def deft(i):
    rows=[]
    for l in open(f'{DEFT_DIR}/ffo{i}.txt'):
        m=re.match(r'HIST depth (\d+) final (\w+) quality (\d+) nodes \[(.*)\]',l)
        rows.append((m.group(2)=='true' and m.group(3)=='0', [int(x) for x in m.group(4).split(',')]))
    return rows
def edax(i):
    rows=[]
    for l in open(os.path.join(HERE, 'raw-edax', f'ffo{i}.txt')):
        if l.startswith('HISTE'):
            p=l.split(); rows.append((p[4]=='5', [int(x) for x in p[6:6+65]]))
    return rows
for i in map(int, sys.argv[1:]):
    d=deft(i); e=edax(i)
    df=[r for f,r in d if f][0]; ef=[r for f,r in e if f][0]
    da=[sum(r[k] for f,r in d) for k in range(65)]; ea=[sum(r[k] for f,r in e) for k in range(65)]
    print(f'FFO{i}  empties: deft_final edax_final ratio | deft_all edax_all ratio')
    for k in range(4,27):
        if df[k] or ef[k]: print(f'{k:3d} {df[k]:10d} {ef[k]:10d} {df[k]/max(ef[k],1):6.2f} | {da[k]:10d} {ea[k]:10d} {da[k]/max(ea[k],1):6.2f}')
    for lo,hi in ((4,4),(5,6),(7,10),(11,14),(15,30),(4,30)):
        a=sum(da[lo:hi+1]); b=sum(ea[lo:hi+1]); print(f'  all-iter {lo}-{hi}: {a} {b} {a/max(b,1):.2f}')
