import sys,csv,subprocess,tempfile,io,json
from pathlib import Path
before,after,positions,out=sys.argv[1:]
rows=[]
with tempfile.TemporaryDirectory() as d:
 case=Path(d)/'case.obf'
 for i,p in enumerate(x for x in Path(positions).read_text().splitlines() if x.strip() and not x.startswith('#')):
  case.write_text(p+'\n')
  results=[]
  for version,binary in [('before',before),('after',after)]:
   run=subprocess.run(['taskset','-c','0',binary,'/tmp/deft-eval-main.dpr2',str(case),'exact','1','256','--legacy-main-mpc'],check=True,text=True,capture_output=True,timeout=120)
   r=next(csv.DictReader(io.StringIO(run.stdout),delimiter='\t'))
   assert r['stop']=='Completed'
   rows.append(dict(r,case=i+1,version=version))
   results.append(r)
  assert results[0]['score']==results[1]['score']
  print(i+1,[(int(r['nodes']),float(r['seconds'])) for r in results],flush=True)
with open(out,'w') as f:
 w=csv.DictWriter(f,fieldnames=list(rows[0]),delimiter='\t');w.writeheader();w.writerows(rows)
for v in ['before','after']:
 print(v,sum(int(r['nodes']) for r in rows if r['version']==v),sum(float(r['seconds']) for r in rows if r['version']==v),flush=True)
