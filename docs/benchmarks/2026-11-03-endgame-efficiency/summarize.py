#!/usr/bin/env python3
"""Per empties count: geometric means of Deft's node and time ratios to
Edax and Egaroucid, and the summed seconds. usage: summarize.py solves.tsv"""
import csv, collections, math, sys
rows = list(csv.DictReader(open(sys.argv[1]), delimiter="\t"))
by = collections.defaultdict(dict)
for x in rows:
    by[(x["file"], x["index"])][x["engine"]] = x
agg = collections.defaultdict(lambda: collections.defaultdict(list))
for v in by.values():
    if len(v) < 3:
        continue
    e = int(v["deft"]["empties"])
    for n in ("edax", "egaroucid"):
        agg[e][n + "_nodes"].append(math.log(int(v["deft"]["nodes"]) / int(v[n]["nodes"])))
        t = lambda name: max(float(v[name]["seconds"]), 0.001)
        agg[e][n + "_time"].append(math.log(t("deft") / t(n)))
    for n in ("deft", "edax", "egaroucid"):
        agg[e][n + "_sum"].append(float(v[n]["seconds"]))
print("empties positions | nodes deft/edax deft/egaroucid | time deft/edax deft/egaroucid | total s deft edax egaroucid")
for e in sorted(agg):
    a = agg[e]
    g = lambda k: math.exp(sum(a[k]) / len(a[k]))
    print(f"{e:7d} {len(a['edax_nodes']):9d} | {g('edax_nodes'):15.2f} {g('egaroucid_nodes'):14.2f} | {g('edax_time'):14.2f} {g('egaroucid_time'):14.2f} | "
          f"{sum(a['deft_sum']):8.1f} {sum(a['edax_sum']):6.1f} {sum(a['egaroucid_sum']):9.1f}")
