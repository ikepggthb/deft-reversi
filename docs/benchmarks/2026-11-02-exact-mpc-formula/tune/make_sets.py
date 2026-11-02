#!/usr/bin/env python3
"""Benchmark positions for tuning the exact shallow depths: valid-split
positions not among the ProbCut calibration positions, chosen with seed 2.
usage: make_sets.py DATA_DIR CALIBRATION_POSITIONS.tsv  -> roots-EE.obf"""
import random, sys
from pathlib import Path
data, cal = Path(sys.argv[1]), sys.argv[2]
used = set()
for line in open(cal):
    f = line.split("\t")
    if f[0].isdigit():
        used.add((int(f[2], 16), int(f[3], 16)))
SETS = {18: 300, 22: 200}
for empties, n in SETS.items():
    raw = (data / f"phase_{64 - empties}" / "valid.rd").read_bytes()[10:]
    count = len(raw) // 18
    rng = random.Random(2 * 1000 + empties)
    out = []
    while len(out) < n:
        i = rng.randrange(count)
        r = raw[18 * i: 18 * i + 18]
        p, o = int.from_bytes(r[0:8], "little"), int.from_bytes(r[8:16], "little")
        if (p, o) in used or any((p, o) == q for q in out):
            continue
        out.append((p, o))
    with open(f"roots-{empties}.obf", "w") as w:
        for p, o in out:
            w.write("".join("X" if p >> (63 - k) & 1 else "O" if o >> (63 - k) & 1 else "-" for k in range(64)) + " X\n")
    print(empties, len(out))
