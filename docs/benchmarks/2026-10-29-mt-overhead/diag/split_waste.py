#!/usr/bin/env python3
"""Split points and their waste by empties and window (apply_split_diag.py).
usage: split_waste.py DIR [exact|select]"""
import re, sys
from collections import defaultdict
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from analyze_lib import load
d = Path(sys.argv[1]); idx = -1 if (sys.argv[2] if len(sys.argv) > 2 else 'exact') == 'exact' else -2
acc = defaultdict(lambda: [0.0] * 130 * 6); nodes = defaultdict(float); runs = defaultdict(set)
for p in d.glob('t*-r*-ffo*.err'):
    t, r, f = map(int, re.match(r't(\d+)-r(\d+)-ffo(\d+)', p.name).groups())
    its = load(p); a, b = its[idx - 1], its[idx]; runs[t].add(r)
    for i, x in enumerate(b['sp']): acc[t][i] += x - (a['sp'][i] if i < len(a['sp']) else 0)
    nodes[t] += int(b['nodes']) - int(a['nodes'])
for t in sorted(acc):
    n = len(runs[t]); v = [x / n for x in acc[t]]; N = nodes[t] / n
    print(f'{t} threads: iteration nodes {N/1e6:.1f}M; waste as % of these nodes')
    print(' empt win   splits  cut  helpers/split  running  done-later  inherited')
    tot = [0, 0, 0]
    for key in range(129, -1, -1):
        s = v[key * 6:key * 6 + 6]
        if not s[0]: continue
        e, wide = divmod(key, 2)
        tot = [tot[0] + s[3], tot[1] + s[4], tot[2] + s[5]]
        print(f' {e:4} {"wide" if wide else "nws ":4} {s[0]:8.0f} {s[1]:5.0f}  {s[2]/s[0]:6.2f}   {100*s[3]/N:7.2f}%  {100*s[4]/N:7.2f}%  {100*s[5]/N:7.2f}%')
    print(f' total running {100*tot[0]/N:.2f}%  done-later {100*tot[1]/N:.2f}%  inherited {100*tot[2]/N:.2f}%')
