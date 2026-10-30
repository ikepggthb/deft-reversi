#!/usr/bin/env python3
"""Midgame fixed-depth check for deft binaries: fresh process per position,
rotated order; reports per-round totals and the median ratio to the first."""
import argparse, csv, io, statistics, subprocess, tempfile
from pathlib import Path
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('variants', nargs='+', help='NAME=BINARY')
p.add_argument('--model', required=True); p.add_argument('--positions', type=Path, required=True)
p.add_argument('--depth', default='16'); p.add_argument('--threads', default='8'); p.add_argument('--cpuset', default='0-7')
p.add_argument('--rounds', type=int, default=4); p.add_argument('--out', type=Path, required=True)
a = p.parse_args()
vs = [tuple(v.split('=', 1)) for v in a.variants]; names = [n for n, _ in vs]
pos = [l.strip() for l in a.positions.read_text().splitlines() if l.strip() and not l.startswith('#')]
a.out.parent.mkdir(parents=True, exist_ok=True)
tot = {n: [] for n in names}
with open(a.out, 'w') as out, tempfile.TemporaryDirectory() as tmp:
    out.write('round\tindex\tvariant\tscore\tdepth\tnodes\tseconds\n')
    case = Path(tmp) / 'case.obf'
    for r in range(a.rounds):
        el = dict.fromkeys(names, 0.0)
        for i, line in enumerate(pos):
            case.write_text(line + '\n')
            s = (r + i) % len(names); order = names[s:] + names[:s]
            for n in order:
                res = subprocess.run(['taskset', '-c', a.cpuset, dict(vs)[n], a.model, str(case), a.depth, a.threads, '256', '--legacy-main-mpc'],
                                     capture_output=True, text=True, check=True, timeout=600)
                row = next(csv.DictReader(io.StringIO(res.stdout), delimiter='\t'))
                assert row['stop'] == 'Completed' and row['depth'] == a.depth, row
                el[n] += float(row['seconds'])
                out.write(f"{r+1}\t{i}\t{n}\t{row['score']}\t{row['depth']}\t{row['nodes']}\t{row['seconds']}\n")
        for n in names: tot[n].append(el[n])
        print(f'round={r+1} ' + ' '.join(f'{n}={el[n]:.4f}' for n in names), flush=True)
for n in names:
    ratios = [x / y for x, y in zip(tot[n], tot[names[0]])]
    print(f'{n:>10}: median {statistics.median(tot[n]):.4f}s ratio {statistics.median(ratios):.4f} faster {sum(x < 1 for x in ratios)}/{len(ratios)}')
