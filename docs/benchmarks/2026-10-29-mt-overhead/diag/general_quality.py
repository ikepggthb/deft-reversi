#!/usr/bin/env python3
"""Null-window general nodes (14+) and local child-probe cuts, final exact
iteration (or 'select' = the selective exact iteration): 1 vs N threads.
usage: general_quality.py DIR [exact|select]
"""
import re, sys
from collections import defaultdict
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from analyze_lib import load
d = Path(sys.argv[1])
idx = -1 if (sys.argv[2] if len(sys.argv) > 2 else 'exact') == 'exact' else -2
def delta(a, b, k):
    return [x - (a[k][i] if i < len(a[k]) else 0) for i, x in enumerate(b[k])]
gen = defaultdict(lambda: [0.0] * 64 * 10)
ordc = defaultdict(lambda: [0.0] * 14 * 4)
runs = defaultdict(set)
for p in d.glob('t*-r*-ffo*.err'):
    t, r, f = map(int, re.match(r't(\d+)-r(\d+)-ffo(\d+)', p.name).groups())
    its = load(p)
    a, b = its[idx - 1], its[idx]
    runs[t].add(r)
    for i, x in enumerate(delta(a, b, 'gen')): gen[t][i] += x
    for i, x in enumerate(delta(a, b, 'ordc')[:56]): ordc[t][i] += x
T = sorted(gen)
for t in T:
    n = len(runs[t]); gen[t] = [x / n for x in gen[t]]; ordc[t] = [x / n for x in ordc[t]]
print('general null-window exact nodes')
print('empt thr  entries(ratio)  reached  TTmove  cut   1st|cut  ETC|cut  1st|cut,TT  1st|cut,noTT')
for e in range(26, 13, -1):
    for t in T:
        v = gen[t][e * 10:e * 10 + 10]
        n, tt, cut, c1, sub, c1tt, ent, cuttt, etc = v[:9]
        if not ent: continue
        e1 = gen[1][e * 10 + 6]
        print(f'{e:4} {t:3} {ent:9.0f}({ent/e1:.3f})  {n/ent:6.3f}  {tt/max(n,1):6.3f} {cut/max(n,1):5.3f}  {c1/max(cut,1):6.3f}  {etc/max(cut,1):6.3f}   {c1tt/max(cuttt,1):7.3f}     {(c1-c1tt)/max(cut-cuttt,1):7.3f}')
print('\nlocal child probes (parent empties): matched rate, parent cut by child bound per ordered node, moves skipped')
for e in range(13, 6, -1):
    for t in T:
        v = ordc[t][e * 4:e * 4 + 4]
        if not v[0]: continue
        print(f'{e:4} {t:3} probes={v[0]/1e3:9.0f}k({v[0]/ordc[1][e*4]:.3f}) matched={v[1]/v[0]:.4f} cut={v[2]/v[0]:.4f} skip={v[3]/v[0]:.4f}')
