"""Selective endgame values against exact ones: how often they differ, by
how much, and the nodes, for the normal and the measured multiples."""
import csv, math, sys
from pathlib import Path

def read(path):
    with open(path) as f:
        return {int(r['position']): r for r in csv.DictReader(f, delimiter='\t')}

d = Path(sys.argv[1] if len(sys.argv) > 1 else 'accuracy')
exact = read(d / 'exact.tsv')
print('stage\tsetting\tpositions\twrong\twrong_rate\tmean_abs_error\tnodes\tnodes_vs_exact')
exact_nodes = sum(int(r['nodes']) for r in exact.values())
for stage in ('77', '85', '95', '98'):
    for setting in ('normal', 'measured'):
        p = d / f'{setting}-{stage}.tsv'
        if not p.exists():
            continue
        rows = read(p)
        keys = [k for k in rows if k in exact]
        wrong = [k for k in keys if rows[k]['score'] != exact[k]['score']]
        err = sum(abs(int(rows[k]['score']) - int(exact[k]['score'])) for k in keys) / len(keys)
        nodes = sum(int(rows[k]['nodes']) for k in keys)
        en = sum(int(exact[k]['nodes']) for k in keys)
        n = len(keys); w = len(wrong)
        print(f'{stage}%\t{setting}\t{n}\t{w}\t{w/n:.3f}\t{err:.3f}\t{nodes:.4g}\t{nodes/en:.3f}')
