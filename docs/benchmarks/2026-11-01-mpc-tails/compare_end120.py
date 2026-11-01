"""Exact-solve nodes per position: totals, geometric mean and worst ratio
against a baseline, by empties, after checking that every score agrees."""
import csv, math, sys
def read(p):
    with open(p) as f:
        return {int(r['position']): r for r in csv.DictReader(f, delimiter='\t')}
base_name, *others = sys.argv[1:]
base = read(base_name)
empties = [l.count('-') for l in open('end120.obf').read().split('\n') if l.strip()]
for name in others:
    rows = read(name)
    assert all(rows[k]['score'] == base[k]['score'] for k in base), name
    print(f'{name} / {base_name} (all {len(base)} scores agree)')
    for e in sorted(set(empties)) + [None]:
        keys = [k for k in base if e is None or empties[k - 1] == e]
        r = [int(rows[k]['nodes']) / int(base[k]['nodes']) for k in keys]
        tot = sum(int(rows[k]['nodes']) for k in keys) / sum(int(base[k]['nodes']) for k in keys)
        gm = math.exp(sum(map(math.log, r)) / len(r))
        print(f"  {'all' if e is None else e:>4}: total {tot:.3f}  geomean {gm:.3f}  worst {max(r):.2f}  best {min(r):.2f}")
