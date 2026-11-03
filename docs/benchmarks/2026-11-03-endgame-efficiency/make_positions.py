#!/usr/bin/env python3
"""Test positions for exact-solve efficiency: valid-split positions not used
for the ProbCut calibration nor for the shallow-depth tuning, seed 3.
usage: make_positions.py DATA_DIR CALIBRATION_POSITIONS.tsv TUNE_DIR [EE:N,...] -> pos-EE.obf
(default 24:30,26:30,28:30,30:12)"""
import random, sys
from pathlib import Path
data, cal, tune = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
def board(line):
    s = line.split()[0]
    p = sum(1 << (63 - k) for k, c in enumerate(s) if c == "X")
    o = sum(1 << (63 - k) for k, c in enumerate(s) if c == "O")
    return p, o
used = set()
for line in open(cal):
    f = line.split("\t")
    if f[0].isdigit():
        used.add((int(f[2], 16), int(f[3], 16)))
for f in tune.glob("roots-*.obf"):
    used.update(board(l) for l in open(f) if l.strip())
def moves(p, o):
    empty = ~(p | o) & (2**64 - 1)
    m = 0
    for shift, mask in ((1, 0xfefefefefefefefe), (-1, 0x7f7f7f7f7f7f7f7f), (8, 2**64 - 1), (-8, 2**64 - 1),
                        (9, 0xfefefefefefefefe), (7, 0x7f7f7f7f7f7f7f7f), (-7, 0xfefefefefefefefe), (-9, 0x7f7f7f7f7f7f7f7f)):
        sh = (lambda x: (x << shift) & mask) if shift > 0 else (lambda x: (x >> -shift) & mask)
        t = sh(p) & o
        for _ in range(5):
            t |= sh(t) & o
        m |= sh(t) & empty
    return m
SETS = {24: 30, 26: 30, 28: 30, 30: 12}
if len(sys.argv) > 4:
    SETS = {int(k): int(v) for k, v in (p.split(':') for p in sys.argv[4].split(','))}
for empties, n in SETS.items():
    raw = (data / f"phase_{64 - empties}" / "valid.rd").read_bytes()[10:]
    count = len(raw) // 18
    rng = random.Random(3000 + empties)
    out = []
    while len(out) < n:
        i = rng.randrange(count)
        r = raw[18 * i: 18 * i + 18]
        p, o = int.from_bytes(r[0:8], "little"), int.from_bytes(r[8:16], "little")
        if (p, o) in used or (p, o) in out or moves(p, o) == 0:
            continue
        out.append((p, o))
    with open(f"pos-{empties}.obf", "w") as w:
        for p, o in out:
            w.write("".join("X" if p >> (63 - k) & 1 else "O" if o >> (63 - k) & 1 else "-" for k in range(64)) + " X\n")
    print(empties, len(out))
