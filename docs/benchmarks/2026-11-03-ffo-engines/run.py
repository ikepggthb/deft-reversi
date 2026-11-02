#!/usr/bin/env python3
"""FFO exact solves, one thread, fresh process per position, every
engine on the same CPU; the order of the engines rotates per round and
position. Writes results.tsv (every solve) and summary.txt (per problem).

usage: python3 run.py ROUNDS [POSITIONS_WITH_ANSWERS.obf OUT_DIR]
Without a file, FFO 40-49 from tools/bench/positions, written here."""
import csv, io, re, statistics, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
CPU = 2
# Built with RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native.
DEFT = REPO / "target-native/release/examples/solve"
ENGINES = {
    # This change: main-style midgame search tiers, the endgame formula and
    # the FFO-chosen shallow depths (deft-reversi-train mpc, --fit-only on
    # the 1,000-position measurement).
    "deft": [str(DEFT), "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/mpc-formula/model.dpr2"],
    # Before: the engine at a5e1513 with the released model.
    "deft-a5e1513": ["/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/3d35d250-9f8d-59a2-bf5d-e57cce4c0696/scratchpad/es/solve-old", "/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/e8-mpc.dpr2"],
    "edax-4.5.5": ["/home/azure/dev/edax-4.5.5/bin/lEdax-x86-64-v3"],
    "egaroucid-7.8.1": ["/home/azure/dev/bench-engines/dl-egaroucid-7.8.1/Egaroucid-console_v7.8.1/bin/Egaroucid_for_Console_clang.out"],
}
NAMES = list(ENGINES)


def command(name, case):
    base = ENGINES[name]
    if name.startswith("deft"):
        return [base[0], base[1], str(case), "exact", "1", "256", *base[2:]]
    if name.startswith("edax"):
        return [base[0], "-solve", str(case), "-n", "1", "-l", "60", "-h", "23",
                "-eval-file", "data/eval.dat", "-book-usage", "off"]
    return [base[0], "-t", "1", "-l", "60", "-nobook", "-hash", "23", "-s", str(case)]


def parse(name, out, empties):
    if name.startswith("deft"):
        row = list(csv.DictReader(io.StringIO(out), delimiter="\t"))[0]
        assert row["stop"] == "Completed" and row["mode"] == "Exact" and int(row["depth"]) == empties
        return int(row["score"]), float(row["seconds"]), int(row["nodes"])
    if name.startswith("edax"):
        m = re.findall(r"^\s*1\|\s*(\d+)\s+([+-]\d+)\s+(\d+):(\d+\.\d+)\s+(\d+)", out, re.M)
        d, s, mi, se, n = m[0]
        assert int(d) == empties
        return int(s), int(mi) * 60 + float(se), int(n)
    m = re.findall(r"^\|\s*60\|\s*(\d+)@100%\|\s*\w+\|\s*([+-]?\d+)\|\s*(\d+):(\d+):(\d+\.\d+)\|\s*(\d+)", out, re.M)
    d, s, h, mi, se, n = m[0]
    assert int(d) == empties
    return int(s), int(h) * 3600 + int(mi) * 60 + float(se), int(n)


def main():
    rounds = int(sys.argv[1])
    if len(sys.argv) > 2:
        answers = [l for l in Path(sys.argv[2]).read_text().splitlines() if l.strip()]
        positions = [a.split(";")[0].strip() for a in answers]
        out_dir = Path(sys.argv[3]).resolve()
    else:
        positions = [l.strip() for l in (REPO / "tools/bench/positions/ffo40-49.obf").read_text().splitlines() if l.strip()]
        answers = [l for l in (REPO / "tools/bench/positions/ffo40-49-answers.obf").read_text().splitlines() if l.strip()]
        out_dir = HERE
    out_dir.mkdir(parents=True, exist_ok=True)
    expected = [max(map(int, re.findall(r":\s*([+-]\d+)", a))) for a in answers]
    case = out_dir / "case.obf"
    rows = []
    with open(out_dir / "results.tsv", "w") as f:
        f.write("round\tffo\tengine\tslot\tempties\tscore\tseconds\tnodes\n")
        for rnd in range(rounds):
            for i, pos in enumerate(positions):
                case.write_text(pos + "\n")
                empties = pos.split()[0].count("-")
                k = (rnd + i) % len(NAMES)
                order = NAMES[k:] + NAMES[:k]
                if (rnd // len(NAMES)) % 2:
                    order.reverse()
                for slot, name in enumerate(order):
                    cmd = ["taskset", "-c", str(CPU), *command(name, case)]
                    cwd = Path(ENGINES[name][0]).parent
                    out = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True, timeout=7200).stdout
                    score, sec, nodes = parse(name, out, empties)
                    assert score == expected[i], (name, i + 40, score)
                    rows.append((rnd, i + 40, name, sec, nodes))
                    f.write(f"{rnd + 1}\t{i + 40}\t{name}\t{slot + 1}\t{empties}\t{score}\t{sec:.6f}\t{nodes}\n")
                    f.flush()
            tot = {n: sum(r[3] for r in rows if r[0] == rnd and r[2] == n) for n in NAMES}
            print(f"round {rnd + 1}: " + "  ".join(f"{n} {t:.2f}s" for n, t in tot.items()), flush=True)
    case.unlink()
    with open(out_dir / "summary.txt", "w") as f:
        f.write("Median seconds per problem (nodes in parentheses), 1 thread, CPU %d, %d rounds\n\n" % (CPU, rounds))
        f.write("FFO  empties  " + "  ".join(f"{n:>24}" for n in NAMES) + "\n")
        for i, pos in enumerate(positions):
            cells = []
            for n in NAMES:
                rs = [r for r in rows if r[1] == i + 40 and r[2] == n]
                cells.append(f"{statistics.median(r[3] for r in rs):8.3f} ({rs[0][4] / 1e6:9.1f}M)")
            f.write(f"{i + 40:3d}  {pos.split()[0].count('-'):7d}  " + "  ".join(f"{c:>24}" for c in cells) + "\n")
        tot = {n: statistics.median(sum(r[3] for r in rows if r[0] == k and r[2] == n) for k in range(rounds)) for n in NAMES}
        nodes = {n: sum(next(r[4] for r in rows if r[1] == i + 40 and r[2] == n) for i in range(len(positions))) for n in NAMES}
        f.write("total         " + "  ".join(f"{tot[n]:8.3f} ({nodes[n] / 1e6:9.1f}M)".rjust(24) for n in NAMES) + "\n")
    print((out_dir / "summary.txt").read_text())


main()
