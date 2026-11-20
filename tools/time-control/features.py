#!/usr/bin/env python3
"""One row per iteration of the probe data, with board features.

usage: features.py GAMES PROBE [GAMES PROBE ...] > table.tsv

GAMES and PROBE are pairs as in the benchmark records (game g of PROBE is
line g of GAMES). Pass `--offset N` before a pair to add N to its game ids.
The probed position is the first one of the game with that many empties
whose side to move has a move (as `probe` picks it).

Columns:
  game empties side_mob opp_mob odd_regions regions corners_empty disc_diff
  tag t cost score move prev_tag prev_cost next_tag next_cost censored
cost is the iteration's own time (t minus the previous iteration's t);
next_cost is the following iteration's time, or, when it did not finish
(censored = 1), the time it had run when the probe stopped (a lower bound).
"""
import sys

DIRS = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)]


def square(s):
    return (ord(s[0].lower()) - ord("a"), int(s[1]) - 1)


class Board:
    def __init__(self):
        self.cells = {}
        self.cells[(3, 3)] = self.cells[(4, 4)] = "W"
        self.cells[(3, 4)] = self.cells[(4, 3)] = "B"
        self.turn = "B"

    def flips(self, sq, me):
        if sq in self.cells:
            return []
        opp = "W" if me == "B" else "B"
        out = []
        for dx, dy in DIRS:
            x, y, run = sq[0] + dx, sq[1] + dy, []
            while 0 <= x < 8 and 0 <= y < 8 and self.cells.get((x, y)) == opp:
                run.append((x, y))
                x, y = x + dx, y + dy
            if run and 0 <= x < 8 and 0 <= y < 8 and self.cells.get((x, y)) == me:
                out += run
        return out

    def moves(self, me):
        return [(x, y) for x in range(8) for y in range(8) if self.flips((x, y), me)]

    def play(self, sq):
        if not self.moves(self.turn):
            self.turn = "W" if self.turn == "B" else "B"
        f = self.flips(sq, self.turn)
        assert f, f"illegal move {sq}"
        for c in f + [sq]:
            self.cells[c] = self.turn
        self.turn = "W" if self.turn == "B" else "B"

    def empties(self):
        return 64 - len(self.cells)

    def features(self):
        me = self.turn
        opp = "W" if me == "B" else "B"
        empty = {(x, y) for x in range(8) for y in range(8)} - set(self.cells)
        seen, regions, odd = set(), 0, 0
        for c in empty:
            if c in seen:
                continue
            stack, size = [c], 0
            seen.add(c)
            while stack:
                x, y = stack.pop()
                size += 1
                for dx, dy in DIRS[:4]:
                    n = (x + dx, y + dy)
                    if n in empty and n not in seen:
                        seen.add(n)
                        stack.append(n)
            regions += 1
            odd += size % 2
        corners = sum((x, y) in empty for x in (0, 7) for y in (0, 7))
        diff = sum(1 if v == me else -1 for v in self.cells.values())
        return [len(self.moves(me)), len(self.moves(opp)), odd, regions, corners, diff]


def positions(line):
    """empties -> features of the first position with that many empties whose
    side to move has a move."""
    b, out = Board(), {}
    line = line.strip()
    for i in range(0, len(line), 2):
        b.play(square(line[i:i + 2]))
        if not b.moves(b.turn):
            if not b.moves("W" if b.turn == "B" else "B"):
                continue
            b.turn = "W" if b.turn == "B" else "B"
        out.setdefault(b.empties(), b.features())
    return out


def main():
    args, offset = sys.argv[1:], 0
    print("game empties side_mob opp_mob odd_regions regions corners_empty disc_diff "
          "tag t cost score move prev_tag prev_cost next_tag next_cost censored".replace(" ", "\t"))
    while args:
        if args[0] == "--offset":
            offset, args = int(args[1]), args[2:]
            continue
        games, probe, args = args[0], args[1], args[2:]
        lines = open(games).read().splitlines()
        cache = {}
        for row in open(probe):
            w = row.split()
            if not row.startswith("g") or "stop=" not in row:
                continue
            g, e = int(w[0][1:]), int(w[1][1:])
            total = float(w[3].split("=")[1])
            pending = w[4].split("=")[1]
            if g not in cache:
                cache[g] = positions(lines[g])
            feats = cache[g].get(e)
            if feats is None:
                continue
            steps = []
            for s in w[5:]:
                tag, t, score, move = s.split(":", 3)
                steps.append((tag, float(t), int(score), move.strip('Some()"').lower()))
            prev_t = 0.0
            for k, (tag, t, score, move) in enumerate(steps):
                cost = t - prev_t
                prev = steps[k - 1] if k else None
                prev_cost = (prev[1] - (steps[k - 2][1] if k > 1 else 0.0)) if prev else ""
                if k + 1 < len(steps):
                    nxt, ncost, cens = steps[k + 1][0], steps[k + 1][1] - t, 0
                elif pending != "-":
                    nxt, ncost, cens = pending, max(total - t, 0.0), 1
                else:
                    nxt, ncost, cens = "-", "", ""
                vals = [g + offset, e] + feats + [tag, f"{t:.4f}", f"{cost:.4f}", score, move,
                        prev[0] if prev else "-",
                        f"{prev_cost:.4f}" if prev else "", nxt,
                        f"{ncost:.4f}" if ncost != "" else "", cens]
                print("\t".join(map(str, vals)))
                prev_t = t


if __name__ == "__main__":
    main()
