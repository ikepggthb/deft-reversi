#!/usr/bin/env python3
"""Idle thread time split by kind, per stage (busy+open+join diag).
usage: idle_kind.py ERRFILE THREADS"""
import re, sys
lines = open(sys.argv[1]).read().splitlines(); T = int(sys.argv[2])
get = lambda k: {int(m[1]): float(m[2]) for m in (re.match(k + r' (\d+) ([\d.]+)', l) for l in lines) if m}
busy, op, join = get('BUSY'), get('OPEN'), get('JOIN')
its = [dict(re.findall(r'(\w+)=(\S+)', l)) for l in lines if l.startswith('ITER')]
stages = [('prep', 0, float(its[-3]['end'])), ('select', float(its[-2]['start']), float(its[-2]['end'])), ('exact', float(its[-1]['start']), float(its[-1]['end']))]
for name, a, b in stages:
    ms = [m for m in busy if int(a) <= m <= int(b)]
    span = len(ms) * T
    j_open = sum(join[m] for m in ms if op[m] >= 0.5); j_closed = sum(join[m] for m in ms if op[m] < 0.5)
    w_open = sum(T - busy[m] - join[m] for m in ms if op[m] >= 0.5); w_closed = sum(T - busy[m] - join[m] for m in ms if op[m] < 0.5)
    print(f'{name:6} {b-a:6.1f} ms: idle {100*(j_open+j_closed+w_open+w_closed)/span:4.1f}% = join-wait {100*j_open/span:4.1f}%+{100*j_closed/span:4.1f}%  worker {100*w_open/span:4.1f}%+{100*w_closed/span:4.1f}%  (with open splits + without)')
