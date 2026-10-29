#!/usr/bin/env python3
"""Local-endgame ordering quality in the final exact iteration: 1 vs N threads.

usage: order_quality.py DIR  (files t{T}-r{R}-ffo{NN}.err from apply_order_diag.py)
"""
import re, sys
from collections import defaultdict
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from analyze_lib import load
d = Path(sys.argv[1])
which = sys.argv[2] if len(sys.argv) > 2 else 'exact'
idx = -1 if which == 'exact' else -2
acc = defaultdict(lambda: [0.0] * 14 * 8)
nodes = defaultdict(float)
runs = defaultdict(set)
for p in d.glob('t*-r*-ffo*.err'):
    t, r, f = re.match(r't(\d+)-r(\d+)-ffo(\d+)', p.name).groups()
    its = load(p)
    a, b = its[idx - 1], its[idx]
    o = [x - (a['ord'][i] if i < len(a['ord']) else 0) for i, x in enumerate(b['ord'])]
    runs[int(t)].add(r)
    for i, x in enumerate(o[:14 * 8]):
        acc[int(t)][i] += x
    nodes[int(t)] += int(b['nodes']) - int(a['nodes'])
T = sorted(acc)
for t in T:
    n = len(runs[t])
    acc[t] = [x / n for x in acc[t]]
    nodes[t] /= n
print(f'{which} iteration, sum of FFO40-49 (mean over runs)')
print('total nodes: ' + '  '.join(f'{t}t={nodes[t]/1e6:.1f}M ({nodes[t]/nodes[1]:.3f})' for t in T))
hdr = 'empt thr   nodes(ratio)   TTmove  cut  1st|cut  1st|cut,TT  1st|cut,noTT  sub/TT  sub/noTT  sub-nodes(ratio)'
print(hdr)
for e in range(13, 6, -1):
    for t in T:
        v = acc[t][e * 8:e * 8 + 8]
        n, tt, cut, c1, stt, snott, c1tt, cuttt = v
        if not n:
            continue
        v1 = acc[1][e * 8:e * 8 + 8]
        sub = stt + snott
        print(f'{e:4} {t:3} {n/1e3:9.0f}k({n/v1[0]:.3f}) {tt/n:6.3f} {cut/n:5.3f}  {c1/cut:6.3f}   {c1tt/max(cuttt,1):7.3f}    {(c1-c1tt)/max(cut-cuttt,1):8.3f}  {stt/max(tt,1):7.1f}  {snott/max(n-tt,1):7.1f}   {sub/1e6:7.1f}M({sub/(v1[4]+v1[5]):.3f})')
