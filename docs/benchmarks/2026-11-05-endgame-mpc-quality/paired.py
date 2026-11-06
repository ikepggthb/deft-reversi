#!/usr/bin/env python3
"""usage: paired.py BASE.tsv OTHER.tsv... : blunders (loss >= 4) fixed and new, per config."""
import csv, sys
def L(f): return {int(r["index"]): int(r["loss"]) for r in csv.DictReader(open(f), delimiter="\t")}
base = L(sys.argv[1])
bad = {i for i, l in base.items() if l >= 4}
for f in sys.argv[2:]:
    x = L(f)
    fixed = sum(1 for i in bad if x[i] < 4)
    new = sum(1 for i, l in x.items() if l >= 4 and i not in bad)
    worse = sum(1 for i in x if x[i] > base[i]); better = sum(1 for i in x if x[i] < base[i])
    print(f"{f.split('/')[-1]:28s} base blunders {len(bad)}: fixed {fixed}, new {new}; any loss better {better} worse {worse}")
