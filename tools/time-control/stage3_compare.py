#!/usr/bin/env python3
"""Compare iteration-cost tables of two runs (e.g. two evaluation models)
played on the same openings.

usage: stage3_compare.py A.tsv B.tsv [MIN_COST]

A and B are journal_table.py tables. For each condition and transition
(mid, entry, stage, exact) it prints the mean of ln(next cost / cost) over
finished next iterations whose own iteration took at least MIN_COST seconds
(default 0.002), for A and B, and B - A with a 95% interval from a paired
bootstrap over openings (self-play games of one opening nearly repeat each
other, so the opening is the unit). `nps` is the mean of ln(nodes/s) of
midgame iterations. Censored rows are left out of both runs alike, so the
means are of finished iterations only.
"""
import csv, math, random, sys
from collections import defaultdict


def transition(a, b):
    if a[0] == "d" and b[0] == "d":
        return "mid"
    if a[0] == "d":
        return "entry"
    return "exact" if b == "e100" else "stage"


def load(path, min_cost):
    """{(cond, measure): {opening: [values]}}"""
    out = defaultdict(lambda: defaultdict(list))
    for r in csv.DictReader(open(path), delimiter="\t"):
        if r["extended"] == "1":
            continue
        cost = float(r["cost"])
        if cost < min_cost:
            continue
        o = r["opening"]
        if r["tag"][0] == "d":
            out[(r["cond"], "nps")][o].append(math.log(int(r["nodes"]) / cost))
        if r["censored"] == "1":
            continue
        out[(r["cond"], transition(r["tag"], r["next_tag"]))][o].append(
            math.log(float(r["next_cost"]) / cost))
    return out


def mean(groups, keys):
    s = n = 0
    for k in keys:
        v = groups.get(k, ())
        s += sum(v)
        n += len(v)
    return s / n if n else float("nan")


def main():
    a, b = sys.argv[1], sys.argv[2]
    min_cost = float(sys.argv[3]) if len(sys.argv) > 3 else 0.002
    A, B = load(a, min_cost), load(b, min_cost)
    rng = random.Random(20261123)
    print("cond\tmeasure\tn A/B\tA\tB\tB-A [95%]\texp(B-A)")
    for key in sorted(set(A) & set(B)):
        ga, gb = A[key], B[key]
        openings = sorted(set(ga) | set(gb))
        d = mean(gb, openings) - mean(ga, openings)
        boots = []
        for _ in range(2000):
            s = [rng.choice(openings) for _ in openings]
            boots.append(mean(gb, s) - mean(ga, s))
        boots.sort()
        lo, hi = boots[50], boots[1949]
        na = sum(len(v) for v in ga.values())
        nb = sum(len(v) for v in gb.values())
        print(f"{key[0]}\t{key[1]}\t{na}/{nb}\t{mean(ga, openings):+.3f}\t{mean(gb, openings):+.3f}"
              f"\t{d:+.3f} [{lo:+.3f}, {hi:+.3f}]\t{math.exp(d):.3f}")


if __name__ == "__main__":
    main()
