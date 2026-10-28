import subprocess, pathlib, tempfile, csv, re, statistics, json, hashlib
D=pathlib.Path('/workspace/deft-reversi'); E=pathlib.Path('/workspace/edax-reversi/bin'); G=pathlib.Path('/workspace/Egaroucid/bin')
positions=[x for x in (D/'deft-reversi-engine/tests/fixtures/ffo40-44.obf').read_text().splitlines() if x and not x.startswith('#')]
obf=(pathlib.Path('/workspace/edax-reversi/problem/fforum-40-59.obf').read_text().splitlines()[:5])
raw=[]
with tempfile.TemporaryDirectory() as t:
 for rnd in range(1,6):
  for i,(board,answer) in enumerate(zip(positions,obf),1):
   p=pathlib.Path(t)/'board'; p.write_text(board+'\n')
   q=pathlib.Path(t)/'obf'; q.write_text(answer+'\n')
   cmds={
    'deft':(['/tmp/deft-main-mpc-baseline','/tmp/deft-eval-main.dpr2',str(p),'exact','1','256','--legacy-main-mpc'],D),
    'edax':([str(E/'lEdax-native'),'-solve',str(q),'-n','1','-l','60','-h','23','-eval-file','data/eval.dat'],E),
    'egaroucid':([str(G/'Egaroucid_for_Console.out'),'-t','1','-l','60','-nobook','-hash','23','-s',str(p)],G),
   }
   names=['deft','edax','egaroucid']; names=names[(rnd-1)%3:]+names[:(rnd-1)%3]
   expected=None
   for name in names:
    cmd,cwd=cmds[name]; result=subprocess.run(['taskset','-c','0',*cmd],cwd=cwd,text=True,capture_output=True,errors="replace",timeout=120,check=True)
    if name=='deft':
     row=list(csv.DictReader(result.stdout.splitlines(),delimiter='\t'))[0]
     if row['stop']!='Completed' or row['mode']!='Exact': raise Exception(row)
     score=int(row['score']); secs=float(row['seconds']); nodes=int(row['nodes'])
    elif name=='edax':
     m=re.search(r'^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)',result.stdout,re.M)
     if not m: raise Exception(result.stdout+result.stderr)
     score=int(m[2]); secs=int(m[3])*60+float(m[4]); nodes=int(m[5]);
     if int(m[1])!=board.count('-'): raise Exception('Edax depth')
    else:
     m=re.search(r'^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)',result.stdout,re.M)
     if not m: raise Exception(result.stdout+result.stderr)
     if int(m[1])!=board.count('-'): raise Exception('Egaroucid depth')
     score=int(m[2]); secs=int(m[3])*3600+int(m[4])*60+float(m[5]); nodes=int(m[6])
    if expected is not None and score!=expected: raise Exception(('score mismatch',rnd,i,name,score,expected))
    expected=score; raw.append(dict(round=rnd,position=i,engine=name,seconds=secs,score=score,nodes=nodes))
  print('round',rnd, {n:round(sum(r['seconds'] for r in raw if r['round']==rnd and r['engine']==n),3) for n in names},flush=True)
with open('/tmp/deft-three-way-baseline.tsv','w') as f:
 w=csv.DictWriter(f,fieldnames=raw[0].keys(),delimiter='\t');w.writeheader();w.writerows(raw)
print({n:statistics.median(sum(r['seconds'] for r in raw if r['round']==rnd and r['engine']==n) for rnd in range(1,6)) for n in ['deft','edax','egaroucid']})
