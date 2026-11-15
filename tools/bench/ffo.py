#!/usr/bin/env python3
"""Exact solves of FFO-style positions: Deft configurations, Edax 4.5.5 and
Egaroucid 7.8.1 on the same CPUs and thread count.

Every position is solved in a fresh process; the order of the engines
rotates per position and round, and reverses every other round. Every
score is checked against the answers. Writes OUT/results.tsv (every solve,
after each one) and OUT/summary.txt (median seconds and nodes per
position, the median total per engine, and its ratio to the first engine
in the same rounds).

usage: ffo.py [--threads N] [--cpus LIST] [--rounds R] [--positions FILE] [--only A-B] --out DIR ENGINE...
ENGINE: edax | egaroucid | NAME[:key=value,...] (a Deft configuration;
        keys solve=BINARY, model=FILE, tt=MIB, or UPPER_CASE=value for the
        environment)

examples:
  ffo.py --threads 1 --cpus 4 --rounds 3 --out t1 deft edax egaroucid
  ffo.py --threads 20 --cpus 0-19 --rounds 5 --out t20 \\
      base:solve=/tmp/solve-base new:solve=/tmp/solve-new
"""
import argparse
import statistics
from pathlib import Path

from common import POSITIONS, empties, engine, paths, read_answers


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--cpus", default="4", help="taskset CPU list (default 4, one P-core of the 265K)")
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--positions", type=Path, default=POSITIONS / "ffo40-59.obf",
                        help="positions with answers (default FFO 40-59)")
    parser.add_argument("--first", type=int, default=40, help="number of the file's first position (default 40)")
    parser.add_argument("--only", help="a range of position numbers, e.g. 40-49 (default all)")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("engines", nargs="+")
    args = parser.parse_args()

    tools = paths()
    engines = [engine(spec, tools) for spec in args.engines]
    names = [e.name for e in engines]
    if len(set(names)) != len(names):
        raise SystemExit("engine names must differ")
    positions = list(enumerate(read_answers(args.positions), args.first))
    if args.only:
        low, _, high = args.only.partition("-")
        positions = [p for p in positions if int(low) <= p[0] <= int(high or low)]
    args.out.mkdir(parents=True, exist_ok=True)
    case = args.out / "case.obf"
    rows = []
    with open(args.out / "results.tsv", "w") as f:
        f.write("round\tposition\tengine\tslot\tempties\tscore\tseconds\tnodes\n")
        for rnd in range(args.rounds):
            for i, (number, (pos, expected)) in enumerate(positions):
                case.write_text(pos + "\n")
                k = (rnd + i) % len(engines)
                order = engines[k:] + engines[:k]
                if rnd % 2:
                    order.reverse()
                for slot, e in enumerate(order):
                    score, seconds, nodes = solve(e, case, args.threads, args.cpus, empties(pos))
                    if expected is not None and score != expected:
                        raise SystemExit(f"{e.name} scored {score} on {number}, expected {expected}")
                    rows.append((rnd, number, e.name, seconds, nodes))
                    f.write(f"{rnd + 1}\t{number}\t{e.name}\t{slot + 1}\t{empties(pos)}\t{score}\t{seconds:.6f}\t{nodes}\n")
                    f.flush()
            totals = "  ".join(f"{n} {sum(r[3] for r in rows if r[0] == rnd and r[2] == n):.2f}s" for n in names)
            print(f"round {rnd + 1}: {totals}", flush=True)
    case.unlink()
    summary = summarize(rows, names, args)
    (args.out / "summary.txt").write_text(summary)
    print(summary)


def solve(e, case, threads, cpus, n_empties):
    if e.kind == "deft":
        row = e.solve_file(case, "exact", threads, cpus)[0]
        if row["stop"] != "Completed" or row["mode"] != "Exact" or int(row["depth"]) != n_empties:
            raise SystemExit(f"{e.name}: incomplete solve {row}")
        return int(row["score"]), float(row["seconds"]), int(row["nodes"])
    depth, score, seconds, nodes = e.exact(case, threads, cpus)
    if depth != n_empties:
        raise SystemExit(f"{e.name}: solved to depth {depth}, not {n_empties}")
    return score, seconds, nodes


def summarize(rows, names, args):
    rounds = args.rounds
    numbers = sorted({r[1] for r in rows})
    med = lambda xs: statistics.median(xs)
    lines = [f"Median seconds per position (nodes), {args.threads} threads, CPUs {args.cpus}, {rounds} rounds", ""]
    lines.append("pos  " + "".join(f"{n:>24}" for n in names))
    for p in numbers:
        cells = []
        for n in names:
            rs = [r for r in rows if r[1] == p and r[2] == n]
            cells.append(f"{med(r[3] for r in rs):9.3f} ({med(r[4] for r in rs) / 1e6:9.1f}M)")
        lines.append(f"{p:3d}  " + "".join(f"{c:>24}" for c in cells))
    total = lambda n, k: sum(r[3] for r in rows if r[0] == k and r[2] == n)
    nodes = {n: sum(med(r[4] for r in rows if r[1] == p and r[2] == n) for p in numbers) for n in names}
    lines.append("")
    for n in names:
        ratios = [total(n, k) / total(names[0], k) for k in range(rounds)]
        lines.append(f"{n:16} total {med(total(n, k) for k in range(rounds)):9.2f} s  nodes {nodes[n] / 1e9:7.2f}G  "
                     f"vs {names[0]} {med(ratios):.3f} (faster in {sum(x < 1 for x in ratios)}/{rounds} rounds)")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
