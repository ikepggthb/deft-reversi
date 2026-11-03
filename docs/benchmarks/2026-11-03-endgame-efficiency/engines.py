#!/usr/bin/env python3
"""Exact solves of positions files by Deft, Edax 4.5.5 and Egaroucid 7.8.1,
one thread on CPU 2, a fresh process per position, the engine order
rotated per position; every engine must give the same score.
usage: engines.py OUT.tsv POSITIONS.obf...   (resumes OUT.tsv)"""
import csv, io, re, subprocess, sys
from pathlib import Path
REPO = Path(__file__).resolve().parents[3]
# The engine at c433a80 (examples/solve, -Ctarget-cpu=native), copied so
# that rebuilding the working tree does not change it mid-run.
DEFT = ["/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/3d35d250-9f8d-59a2-bf5d-e57cce4c0696/scratchpad/solve-c433a80",
        "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/mpc-formula/model.dpr2"]
EDAX = "/home/azure/dev/edax-4.5.5/bin/lEdax-x86-64-v3"
EGAR = "/home/azure/dev/bench-engines/dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out"
NAMES = ["deft", "edax", "egaroucid"]

def run(name, case):
    if name == "deft":
        out = subprocess.run(["taskset", "-c", "2", *DEFT[:1], DEFT[1], str(case), "exact", "1", "256"],
                             capture_output=True, text=True, check=True).stdout
        r = list(csv.DictReader(io.StringIO(out), delimiter="\t"))[0]
        return int(r["score"]), float(r["seconds"]), int(r["nodes"])
    if name == "edax":
        out = subprocess.run(["taskset", "-c", "2", EDAX, "-solve", str(case), "-n", "1", "-l", "60", "-h", "23",
                              "-eval-file", "data/eval.dat", "-book-usage", "off"],
                             cwd=Path(EDAX).parent, capture_output=True, text=True, check=True).stdout
        d, s, mi, se, n = re.findall(r"^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)", out, re.M)[0]
        return int(s), int(mi) * 60 + float(se), int(n)
    out = subprocess.run(["taskset", "-c", "2", EGAR, "-t", "1", "-l", "60", "-nobook", "-hash", "23", "-s", str(case)],
                         cwd=Path(EGAR).parent, capture_output=True, text=True, check=True).stdout
    d, s, h, mi, se, n = re.findall(r"^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)", out, re.M)[0]
    return int(s), int(h) * 3600 + int(mi) * 60 + float(se), int(n)

def main():
    out = Path(sys.argv[1]).resolve()
    done = set()
    if out.exists():
        done = {(r["file"], int(r["index"]), r["engine"]) for r in csv.DictReader(open(out), delimiter="\t")}
    else:
        out.write_text("file\tindex\tempties\tengine\tscore\tseconds\tnodes\n")
    case = out.parent / "case.obf"
    k = 0
    for f in sys.argv[2:]:
        for i, line in enumerate(l.strip() for l in open(f) if l.strip()):
            case.write_text(line + "\n")
            empties = line.split()[0].count("-")
            order = NAMES[k % 3:] + NAMES[:k % 3]; k += 1
            scores = {}
            for name in order:
                if (Path(f).name, i, name) in done:
                    continue
                s, t, n = run(name, case)
                scores[name] = s
                with open(out, "a") as w:
                    w.write(f"{Path(f).name}\t{i}\t{empties}\t{name}\t{s}\t{t:.6f}\t{n}\n")
            assert len(set(scores.values())) <= 1, (f, i, scores)
    case.unlink(missing_ok=True)

main()
