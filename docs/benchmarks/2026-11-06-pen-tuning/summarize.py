import csv,sys,os
R='results'
def load(p):
    return list(csv.DictReader(open(p),delimiter='\t')) if os.path.exists(p) else None
ref=load(f'{R}/ref-l24.tsv')
name=sys.argv[1]
out=[name]
for l in ('21','18'):
    rows=load(f'{R}/{name}-l{l}.tsv')
    if not rows: continue
    t=sum(float(r['seconds']) for r in rows); n=sum(int(r['nodes']) for r in rows)
    s=f'L{l} {t:6.2f}s {n/1e6:7.0f}M'
    if ref:
        same=sum(a['action']==b['action'] for a,b in zip(rows,ref))/len(rows)
        d=sum(abs(int(a['score'])-int(b['score'])) for a,b in zip(rows,ref))/len(rows)
        s+=f' same {same:5.1%} |d| {d:4.2f}'
    out.append(s)
rows=load(f'{R}/{name}-ffo.tsv')
if rows:
    out.append(f"FFO {sum(float(r['seconds']) for r in rows):7.2f}s {sum(int(r['nodes']) for r in rows)/1e9:6.2f}G")
print('  '.join(out))
