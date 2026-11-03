#!/usr/bin/env python3
"""Exact solves of a whole positions file in one process per engine (for
small empties, where a process per position would time start-up), on CPU
CPU. Prints per-position nodes and seconds; scores must agree.
usage: batch.py CPU OUT.tsv POSITIONS.obf..."""
import csv, io, re, subprocess, sys
from pathlib import Path
S = "/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/3d35d250-9f8d-59a2-bf5d-e57cce4c0696/scratchpad"
ENGINES = {
    "deft": [f"{S}/solve-c433a80", "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/mpc-formula/model.dpr2"],
    "deft-noprep": [f"{S}/solve-c433a80", "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/e8.dpr2"],
}
EDAX = "/home/azure/dev/edax-4.5.5/bin/lEdax-x86-64-v3"
EGAR = "/home/azure/dev/bench-engines/dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out"

def solve(name, cpu, f):
    f = str(Path(f).resolve())
    if name.startswith("deft"):
        b, m = ENGINES[name]
        out = subprocess.run(["taskset", "-c", cpu, b, m, f, "exact", "1", "256"], capture_output=True, text=True, check=True).stdout
        return [(int(r["score"]), float(r["seconds"]), int(r["nodes"])) for r in csv.DictReader(io.StringIO(out), delimiter="\t")]
    if name == "edax":
        out = subprocess.run(["taskset", "-c", cpu, EDAX, "-solve", f, "-n", "1", "-l", "60", "-h", "23", "-eval-file", "data/eval.dat", "-book-usage", "off"],
                             cwd=Path(EDAX).parent, capture_output=True, text=True, check=True).stdout
        return [(int(s), int(mi) * 60 + float(se), int(n)) for _, s, mi, se, n in
                re.findall(r"^\s*\d+\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)", out, re.M)]
    out = subprocess.run(["taskset", "-c", cpu, EGAR, "-t", "1", "-l", "60", "-nobook", "-hash", "23", "-s", f],
                         cwd=Path(EGAR).parent, capture_output=True, text=True, check=True).stdout
    return [(int(s), int(h) * 3600 + int(mi) * 60 + float(se), int(n)) for _, s, h, mi, se, n in
            re.findall(r"^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)", out, re.M)]

def main():
    cpu, out = sys.argv[1], sys.argv[2]
    with open(out, "w") as w:
        w.write("file\tindex\tempties\tengine\tscore\tseconds\tnodes\n")
        for f in sys.argv[3:]:
            lines = [l for l in open(f) if l.strip()]
            res = {n: solve(n, cpu, f) for n in ["deft", "deft-noprep", "edax", "egaroucid"]}
            for n, r in res.items():
                assert len(r) == len(lines), (n, f, len(r))
            for i, line in enumerate(lines):
                assert len({res[n][i][0] for n in res}) == 1, (f, i, {n: res[n][i] for n in res})
                for n in res:
                    s, t, nodes = res[n][i]
                    w.write(f"{Path(f).name}\t{i}\t{line.split()[0].count('-')}\t{n}\t{s}\t{t:.6f}\t{nodes}\n")
            tot = {n: (sum(x[2] for x in r), sum(x[1] for x in r)) for n, r in res.items()}
            print(f, " ".join(f"{n}: {a/1e6:.1f}M {b:.2f}s" for n, (a, b) in tot.items()), flush=True)

main()
