#!/usr/bin/env python3
"""Midgame speed and move quality of Deft configurations.

Searches the 150 midgame positions (mid150.obf: 50 each at 32, 40 and 48
empties) at the given levels, one process per configuration and level,
one at a time; the order of the configurations alternates per round.
Move quality is the share of moves equal to a reference search's and the
mean |score difference| to it (default: positions/mid150-level24.tsv, the
PEN model of model-v3-pen's training with 250-position MPC at level 24).

Writes OUT/NAME-rR-lL.tsv per search and prints, per configuration and
level, the median seconds, nodes and the quality.

usage: midgame.py [--levels 21,18] [--threads N] [--cpus LIST] [--rounds R] --out DIR CONFIG...
CONFIG: NAME[:key=value,...] as in ffo.py
"""
import argparse
import csv
import statistics
from pathlib import Path

from common import POSITIONS, Deft, paths


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--levels", default="21,18")
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--cpus", default="2")
    parser.add_argument("--rounds", type=int, default=1)
    parser.add_argument("--positions", type=Path, default=POSITIONS / "mid150.obf")
    parser.add_argument("--reference", type=Path, default=POSITIONS / "mid150-level24.tsv")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("configs", nargs="+")
    args = parser.parse_args()

    tools = paths()
    configs = [Deft(spec, tools) for spec in args.configs]
    levels = args.levels.split(",")
    reference = list(csv.DictReader(open(args.reference), delimiter="\t")) if args.reference.exists() else None
    args.out.mkdir(parents=True, exist_ok=True)
    results = {}
    for rnd in range(args.rounds):
        order = configs if rnd % 2 == 0 else configs[::-1]
        for c in order:
            for level in levels:
                rows = c.solve_file(args.positions, f"level{level}", args.threads, args.cpus)
                path = args.out / f"{c.name}-r{rnd + 1}-l{level}.tsv"
                with open(path, "w") as f:
                    w = csv.DictWriter(f, fieldnames=rows[0].keys(), delimiter="\t")
                    w.writeheader()
                    w.writerows(rows)
                results.setdefault((c.name, level), []).append(rows)
                print(f"round {rnd + 1} {c.name} level {level}: {sum(float(r['seconds']) for r in rows):.2f}s",
                      flush=True)
    for c in configs:
        for level in levels:
            runs = results[(c.name, level)]
            seconds = statistics.median(sum(float(r["seconds"]) for r in rows) for rows in runs)
            nodes = statistics.median(sum(int(r["nodes"]) for r in rows) for rows in runs)
            line = f"{c.name:16} level {level}: {seconds:8.2f} s {nodes / 1e6:8.0f}M"
            if reference:
                rows = runs[0]
                same = sum(a["action"] == b["action"] for a, b in zip(rows, reference)) / len(rows)
                diff = sum(abs(int(a["score"]) - int(b["score"])) for a, b in zip(rows, reference)) / len(rows)
                line += f"  same move {same:5.1%}  |d| {diff:4.2f}"
            print(line)


if __name__ == "__main__":
    main()
