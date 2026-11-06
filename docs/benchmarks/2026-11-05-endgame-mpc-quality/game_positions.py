#!/usr/bin/env python3
"""Positions with Deft to move at 22-24 empties in the level-12 games.
usage: game_positions.py GAMES.tsv OUT.obf"""
import csv, sys
sys.path.insert(0, "../2026-11-03-engine-match")
from match import moves, flips, square

seen = set()
out = open(sys.argv[2], "w")
for r in csv.DictReader(open(sys.argv[1]), delimiter="\t"):
    deft_black = r["black"] == "Deft"
    p, o, black = 0x0000000810000000, 0x0000001008000000, True
    seq = r["moves"]
    k = 0
    while True:
        if moves(p, o) == 0:
            if moves(o, p) == 0:
                break
            p, o, black = o, p, not black
            continue
        empties = 64 - bin(p | o).count("1")
        if black == deft_black and 22 <= empties <= 24:
            bp, wp = (p, o) if black else (o, p)
            s = "".join("X" if bp >> i & 1 else "O" if wp >> i & 1 else "-" for i in range(64))
            line = f"{s} {'X' if black else 'O'}"
            if line not in seen:
                seen.add(line)
                out.write(line + "\n")
        if k >= len(seq):
            break
        sq = square(seq[k:k + 2]); k += 2
        f = flips(p, o, sq)
        p, o, black = o ^ f, p | f | (1 << sq), not black
print(len(seen))
