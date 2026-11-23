#!/usr/bin/env python3
"""Positions of logged games for the reference search (stage 3-2).

usage: refer_input.py [--missing REFER_DIR] [--table-moves] FIRST:LAST OUT.tsv... > positions.tsv

For every game of the given openings in each referee OUT.tsv (with its
OUT.logs.tsv and clock logs from CLOCK_LOG_DIR), replays the game record
and writes one line per searched turn of either side:

  key  black  white  side  empties  moves  cond  opening  new_black  player

key is the position (black and white bits in hex and the side to move), so
a position met in several games (colour-swapped self-play games nearly
repeat each other) is searched once; later lines with the same key only add
their chosen moves to it. `moves` are the squares (a1..h8) that any
iteration of the turn chose as its best move, and the move played,
comma-separated, merged over all turns at that position. (The move played
was added after the stage 3-2 run: a turn whose first iteration did not
finish before hard has no iteration move, but a move played.)

With --table-moves, a turn after a searched turn whose PV the opponent
followed (`reply_matched`) also lists the PV's third move: the move the
table holds there, which a warm-started turn plays if its first iteration
is cut (stage 4-1, sim.py).

With --missing, only the moves that REFER_DIR's results (refer_loss.py's
`reference`) do not have yet are listed, for a supplementary run. The output lists each key once, on its first
turn, followed by `# also cond opening new_black player` lines for the
other turns there (for joining the results back).

Of extension games (conditions named x-*), only the extended turn is
taken: the turns before it ran as in a normal game, and the extended turn
holds the iterations the clock would not have run.
"""
import json, sys
from collections import OrderedDict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "bench"))
from match import flips, moves, square  # noqa: E402

START = (0x0000000810000000, 0x0000001008000000)  # black, white


def positions(record):
    """{(empties, black to move): (black, white)} before each move."""
    b, w, black = START[0], START[1], True
    out = {}
    for k in range(0, len(record), 2):
        p, o = (b, w) if black else (w, b)
        if moves(p, o) == 0:
            black = not black
            p, o = o, p
        sq = square(record[k:k + 2])
        out[(64 - bin(b | w).count("1"), black)] = (b, w)
        f = flips(p, o, sq)
        assert f, f"illegal {record[k:k + 2]} in {record}"
        p, o = p ^ f, o ^ f
        p |= 1 << sq
        b, w = (p, o) if black else (o, p)
        black = not black
    # After the last move: the turn an extension game ended on.
    p, o = (b, w) if black else (w, b)
    if moves(p, o) == 0:
        black = not black
    out[(64 - bin(b | w).count("1"), black)] = (b, w)
    return out


def turns(path, start, end):
    with open(path, "rb") as f:
        f.seek(start)
        data = f.read(end - start)
    its = []
    for line in data.splitlines():
        r = json.loads(line)
        if r["record"] == "iteration":
            its.append(r)
        else:
            yield its, r
            its = []


def main():
    args = sys.argv[1:]
    have = {}
    if args[0] == "--missing":
        from refer_loss import reference
        have = {k: set(v[1]) for k, v in reference(args[1]).items()}
        args = args[2:]
    table_moves = args[0] == "--table-moves"
    if table_moves:
        args = args[1:]
    first, last = (int(x) for x in args[0].split(":"))
    found = OrderedDict()
    for out in map(Path, args[1:]):
        cond = out.name.removesuffix(".tsv")
        records = {}
        for line in out.read_text().splitlines()[1:]:
            f = line.split("\t")
            records[(int(f[0]), f[1])] = f[6]
        for line in out.with_suffix(".logs.tsv").read_text().splitlines()[1:]:
            w = line.split("\t")
            opening, nb = int(w[0]), w[1]
            if not first <= opening <= last:
                continue
            pos = positions(records[(opening, nb)])
            for player, (name, a, b) in (("new", w[3:6]), ("old", w[6:9])):
                black = (player == "new") == (nb == "1")
                pv = []
                for its, turn in turns(out.parent / cond / name, int(a), int(b)):
                    if not turn["searched"] or turn["after_extension"]:
                        continue
                    prev_pv, pv = pv, next((i["pv"] for i in reversed(its) if i["best_move"]), [])
                    if cond.startswith("x-") and not turn["extended"]:
                        continue
                    chosen = {i["best_move"] for i in its if i["best_move"]}
                    if turn["played"] and turn["played"] != "pass":
                        chosen.add(turn["played"])
                    if table_moves and turn["reply_matched"] and len(prev_pv) >= 3:
                        chosen.add(prev_pv[2])
                    e = turn["empties"]
                    bw = pos.get((e, black))
                    key = f"{bw[0]:016x}{bw[1]:016x}{'b' if black else 'w'}"
                    chosen -= have.get(key, set())
                    if not chosen:
                        continue
                    entry = found.setdefault(key, {"bw": bw, "black": black, "e": e,
                                                    "moves": set(), "turns": []})
                    entry["moves"] |= chosen
                    entry["turns"].append(f"{cond}\t{opening}\t{nb}\t{player}")
    for key, v in found.items():
        print(f"{key}\t{v['bw'][0]:016x}\t{v['bw'][1]:016x}\t{'b' if v['black'] else 'w'}"
              f"\t{v['e']}\t{','.join(sorted(v['moves']))}\t{v['turns'][0]}")
        for t in v["turns"][1:]:
            print(f"# also\t{key}\t{t}")


if __name__ == "__main__":
    main()
