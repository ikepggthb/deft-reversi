#!/usr/bin/env python3
"""Deft-only FFO 40-59 exact solves with configs given as env settings.
Fresh process per position; config order rotates per position and round.

usage: tune.py THREADS CPUS ROUNDS OUT.tsv NAME=K=V,K=V NAME2= ...
(an empty setting list is the baseline). SOLVE env overrides the binary."""
import csv, io, os, re, statistics, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SOLVE = os.environ.get("SOLVE", str(REPO / "target-exp/release/examples/solve"))
MODEL = os.environ.get("MODEL", "/home/azure/dev/data/pen-exp/mpc-pen-1000/model.dpr2")
ANSWERS = REPO / "docs/benchmarks/2026-11-01-ffo-engines/ffo40-59-answers.obf"
FIRST = int(os.environ.get("FIRST", "40"))

threads, cpus, rounds, out = sys.argv[1], sys.argv[2], int(sys.argv[3]), Path(sys.argv[4])
configs = {}
for arg in sys.argv[5:]:
    name, _, settings = arg.partition("=")
    configs[name] = dict(kv.split("=", 1) for kv in settings.split(",") if kv)
names = list(configs)
answers = [l for l in ANSWERS.read_text().splitlines() if l.strip()][FIRST - 40:]
positions = [a.split(";")[0].strip() for a in answers]
expected = [max(map(int, re.findall(r":\s*([+-]\d+)", a))) for a in answers]
case = out.with_suffix(".case.obf")
rows = []
with open(out, "w") as f:
    f.write("round\tffo\tconfig\tseconds\tnodes\n")
    for rnd in range(rounds):
        for i, pos in enumerate(positions):
            case.write_text(pos + "\n")
            k = (rnd + i) % len(names)
            order = names[k:] + names[:k]
            if rnd % 2:
                order.reverse()
            for name in order:
                env = {**os.environ, **configs[name]}
                o = subprocess.run(["taskset", "-c", cpus, configs[name].get("SOLVE", SOLVE), MODEL, str(case), "exact", threads, "256"],
                                   env=env, capture_output=True, text=True, check=True).stdout
                r = list(csv.DictReader(io.StringIO(o), delimiter="\t"))[0]
                assert r["stop"] == "Completed" and int(r["score"]) == expected[i], (name, FIRST + i, r)
                rows.append((rnd, FIRST + i, name, float(r["seconds"]), int(r["nodes"])))
                f.write(f"{rnd+1}\t{FIRST+i}\t{name}\t{r['seconds']}\t{r['nodes']}\n")
                f.flush()
        print(f"round {rnd+1}: " + "  ".join(
            f"{n} {sum(r[3] for r in rows if r[0]==rnd and r[2]==n):.2f}s" for n in names), flush=True)
case.unlink()
base = names[0]
for n in names:
    tot = [sum(r[3] for r in rows if r[0] == k and r[2] == n) for k in range(rounds)]
    nodes = statistics.median(sum(r[4] for r in rows if r[0] == k and r[2] == n) for k in range(rounds))
    ratios = [t / sum(r[3] for r in rows if r[0] == k and r[2] == base) for k, t in enumerate(tot)]
    print(f"{n:12} median {statistics.median(tot):7.2f}s  nodes {nodes/1e9:6.2f}G  vs {base} {statistics.median(ratios):.3f} (faster rounds {sum(x<1 for x in ratios)}/{rounds})")
