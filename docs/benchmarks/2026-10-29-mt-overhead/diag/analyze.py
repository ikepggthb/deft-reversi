#!/usr/bin/env python3
"""Per-iteration node deltas from DIAG lines: 1 thread vs median of 8-thread runs."""
import re, statistics, sys
from pathlib import Path
d = Path(sys.argv[1])
def load(path):
    its = []
    for line in path.read_text().splitlines():
        if not line.startswith('DIAG'):
            continue
        kv = dict(re.findall(r'(\w+)=(\[[^\]]*\]|\S+)', line))
        its.append({k: (eval(v) if v.startswith('[') else v) for k, v in kv.items()})
    return its
def delta(its, key):
    out, prev = [], 0
    for it in its:
        v = int(it[key]); out.append(v - prev); prev = v
    return out
tot1 = tot8 = 0
agg = {}
for f in range(40, 50):
    one = load(d / f't1-r1-ffo{f}.err')
    runs = sorted(d.glob(f't8-r*-ffo{f}.err'))
    eights = [load(p) for p in runs]
    n1 = delta(one, 'nodes')
    n8 = [statistics.median(x) for x in zip(*[delta(e, 'nodes') for e in eights])]
    w8 = [statistics.median(x) for x in zip(*[delta(e, 'wasted') for e in eights])]
    labels = [f"{it['depth']}{'F' if it['final']=='true' else ''}q{it['quality']}" for it in one]
    print(f'FFO{f}: total 8t/1t={sum(n8)/sum(n1):.3f}  ' + ' '.join(
        f'{l}:{b/a:.2f}(w{w/b*100:.0f}%)' for l, a, b, w in zip(labels, n1, n8, w8) if a > sum(n1) * 0.01))
    tot1 += sum(n1); tot8 += sum(n8)
    for l, a, b, w in zip(['prep'] * (len(labels) - 2) + ['sel', 'exact'], n1, n8, w8):
        x = agg.setdefault(l, [0, 0, 0]); x[0] += a; x[1] += b; x[2] += w
print(f'ALL 8t/1t={tot8/tot1:.3f}')
for k, (a, b, w) in agg.items():
    print(f'  {k}: 1t={a:,} 8t={b:,.0f} ratio={b/a:.3f} share_of_extra={(b-a)/(tot8-tot1)*100:.0f}% wasted8={w/b*100:.1f}%')
