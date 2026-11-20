#!/usr/bin/env python3
"""Loss of each iteration of a timed search against exact move values.

usage: losses.py TRUTH... -- PROBE...

TRUTH is output of `probe/target/release/truth` (exact score of every move),
PROBE the output of `probe` for the same games and empties. For each position
and each completed iteration, the loss is the exact value of the best move
minus the exact value of the move that iteration chose.

For every empties count and iteration kind it prints the median completion
time, the mean loss and the share of positions where the iteration finished
(means over the positions where it finished; `all` counts unfinished
iterations at the loss of the last finished one). Between successive kinds it
prints the loss saved per second of extra time, -dL/dt: the allocation that
minimises the total loss gives every move the same -dL/dt.
"""
import statistics as st
import sys
from collections import defaultdict


def truth(paths):
    out = {}
    for path in paths:
        for line in open(path):
            w = line.split()
            if len(w) < 3 or not w[0].startswith("g"):
                continue
            out[(w[0], w[1])] = {m.split(":")[0].lower(): int(m.split(":")[1]) for m in w[2:]}
    return out


def order(tag):
    return (0, int(tag[1:])) if tag.startswith("d") else (1, int(tag[1:]))


def main():
    args = sys.argv[1:]
    cut = args.index("--")
    exact = truth(args[:cut])
    # (empties, tag) -> list of (time, loss); per position the last midgame
    # depth is labelled "dmax" so positions with different plans line up.
    rows = defaultdict(list)
    positions = defaultdict(int)
    for path in args[cut + 1:]:
        for line in open(path):
            w = line.split()
            if not line.startswith("g") or "stop=" not in line:
                continue
            key = (w[0], w[1])
            if key not in exact:
                continue
            values = exact[key]
            best = max(values.values())
            e = int(w[1][1:])
            positions[e] += 1
            steps = []
            for s in w[5:]:
                tag, t, _score, move = s.split(":", 3)
                move = move.strip('Some()"').lower()
                steps.append((tag, float(t), best - values[move]))
            depths = [s for s in steps if s[0].startswith("d")]
            for tag, t, loss in steps:
                rows[(e, tag)].append((t, loss))
            if depths:
                rows[(e, "dmax")].append(depths[-1][1:])
    for e in sorted(positions):
        n = positions[e]
        tags = sorted({t for (ee, t) in rows if ee == e and t != "dmax"}, key=order)
        print(f"== {e} empties, {n} positions")
        prev = None
        for tag in tags:
            done = rows[(e, tag)]
            if len(done) < max(3, n // 2):
                continue
            t = st.median(x for x, _ in done)
            loss = st.fmean(l for _, l in done)
            line = f"  {tag:>5}  done {len(done):3d}/{n}  median {t:8.3f}s  mean loss {loss:5.2f}"
            if prev and t > prev[0]:
                line += f"  saves {(prev[1] - loss) / (t - prev[0]):7.2f} discs/s"
            print(line)
            prev = (t, loss)


if __name__ == "__main__":
    main()
