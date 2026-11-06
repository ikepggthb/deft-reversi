#!/usr/bin/env python3
"""usage: summarize.py RUN.tsv...  -> blunder rates (loss >= 4, >= 2, > 0), mean loss, time"""
import csv, sys, math
base = None
for f in sys.argv[1:]:
    r = list(csv.DictReader(open(f), delimiter="\t"))
    n = len(r)
    loss = [int(x["loss"]) for x in r]
    t = sum(float(x["seconds"]) for x in r)
    nodes = sum(int(x["nodes"]) for x in r)
    b4 = sum(l >= 4 for l in loss)
    if base is None:
        base = (t, nodes)
    print(f"{f.split('/')[-1]:28s} n {n}  loss>=4 {b4:3d} ({b4/n:.2%})  loss>=2 {sum(l>=2 for l in loss)/n:.2%}  "
          f"loss>0 {sum(l>0 for l in loss)/n:.2%}  mean {sum(loss)/n:.3f}  time {t:7.1f}s ({t/base[0]:.2f}x)  nodes {nodes/base[1]:.2f}x")
