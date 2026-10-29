#!/usr/bin/env python3
"""Exact-iteration general-node counts by empties: 1 thread vs median of 8-thread runs."""
import statistics, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
from analyze_lib import load
d = Path(sys.argv[1])
which = sys.argv[2] if len(sys.argv) > 2 else 'exact'
def last_delta(its, idx):
    a, b = its[idx - 1], its[idx]
    g = [x - (a['general'][i] if i < len(a['general']) else 0) for i, x in enumerate(b['general'])]
    return int(b['nodes']) - int(a['nodes']), g, int(b['splits']) - int(a['splits']), [x - (a['split_empties'][i] if i < len(a['split_empties']) else 0) for i, x in enumerate(b['split_empties'])]
idx = -1 if which == 'exact' else -2
G1 = [0] * 64; G8 = [0] * 64; T1 = T8 = 0; S8 = [0] * 64
for f in range(40, 50):
    one = load(d / f't1-r1-ffo{f}.err')
    t1, g1, _, _ = last_delta(one, len(one) + idx)
    T1 += t1
    for i, x in enumerate(g1): G1[i] += x
    runs = [load(p) for p in sorted(d.glob(f't8-r*-ffo{f}.err'))]
    ds = [last_delta(r, len(r) + idx) for r in runs]
    T8 += statistics.median(x[0] for x in ds)
    for i in range(64):
        G8[i] += statistics.median((x[1][i] if i < len(x[1]) else 0) for x in ds)
        S8[i] += statistics.median((x[3][i] if i < len(x[3]) else 0) for x in ds)
print(f'{which}: total nodes 1t={T1:,} 8t={T8:,.0f} ratio={T8/T1:.3f}')
low1 = T1 - sum(G1); low8 = T8 - sum(G8)
print(f'  nodes inside local endgame (<=13, beyond entry): 1t={low1:,} 8t={low8:,.0f} ratio={low8/low1:.3f}')
for e in range(10, 30):
    if G1[e] or G8[e]:
        print(f'  empties {e}: general 1t={G1[e]:,} 8t={G8[e]:,.0f} ratio={G8[e]/max(G1[e],1):.3f}  splits8={S8[e]:.0f}')
