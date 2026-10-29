#!/usr/bin/env python3
"""Idle thread time per iteration and over the final exact iteration (busy diag).
usage: busy_summary.py ERRFILE THREADS"""
import re, sys
lines = open(sys.argv[1]).read().splitlines(); T = int(sys.argv[2])
busy = {int(m[1]): float(m[2]) for m in (re.match(r'BUSY (\d+) ([\d.]+)', l) for l in lines) if m}
iters = [dict(re.findall(r'(\w+)=(\S+)', l)) for l in lines if l.startswith('ITER')]
def idle(a, b):
    ms = [m for m in busy if a <= m < b]
    return sum(T - min(busy[m], T) for m in ms), len(ms)
tot_idle = tot = 0
for it in iters:
    a, b = float(it['start']), float(it['end'])
    i, n = idle(int(a), int(b) + 1)
    tot_idle += i; tot += n * T
    print(f"depth={it['depth']:>2} final={it['final']:5} q={it['quality']} {a:7.1f}-{b:7.1f} ms  idle={i:7.1f} thread-ms ({i/max(n*T,1):.2f} of span)")
print(f'all: idle share {tot_idle/tot:.3f}')
it = iters[-1]; a, b = float(it['start']), float(it['end'])
print('final iteration, idle share by tenth:', ' '.join(f"{idle(int(a+(b-a)*k/10), int(a+(b-a)*(k+1)/10))[0]/max(1,idle(int(a+(b-a)*k/10), int(a+(b-a)*(k+1)/10))[1]*T):.2f}" for k in range(10)))
