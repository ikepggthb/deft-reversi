#!/usr/bin/env python3
"""Expected loss of a time allocation over one side's moves, from a loss table.

usage: allocate.py [--max-depth D] [--scale K] TOTAL_S... -- REF... -- PROBE... [-- GTP_LOG...]

REF is output of `probe/target/release/refer` (or `truth`), PROBE the
output of `probe` for the same positions. For one position the loss after
t seconds is the loss of the last iteration that finished by t (the first
iteration's loss before that). Averaging over the positions with e empties
gives L_e(t), the expected loss of a move given t seconds, which is then
fitted as a non-increasing function (rises are sampling noise).

Midgame references are DEPTH-ply searches, so iterations deeper than
--max-depth (default 22) are ignored: the curve stays flat after the last
trusted iteration, which undervalues extra midgame time.

Probe times come from a cold transposition table; --scale K multiplies them
(K < 1 when moves in a game are faster than cold probes).

For each TOTAL_S (seconds for one side's moves from 52 to 20 empties) it
prints the allocation that minimises the summed expected loss (dynamic
programming over a time grid) and, when GTP logs are given, the summed
loss of their mean time per empties scaled to the same total.
"""
import bisect
import statistics as st
import sys
from collections import defaultdict

MOVES = list(range(52, 19, -2))  # one side's turns from the XOT start


def parse(args):
    max_depth, scale = 22, 1.0
    while args and args[0].startswith("--"):
        if args[0] == "--max-depth":
            max_depth, args = int(args[1]), args[2:]
        elif args[0] == "--scale":
            scale, args = float(args[1]), args[2:]
        else:
            sys.exit(__doc__)
    groups, cur = [], []
    for a in args:
        if a == "--":
            groups.append(cur)
            cur = []
        else:
            cur.append(a)
    groups.append(cur)
    totals = [float(x) for x in groups[0]]
    return max_depth, scale, totals, groups[1], groups[2], groups[3] if len(groups) > 3 else []


def reference(paths):
    out = {}
    for path in paths:
        for line in open(path):
            w = line.split()
            if len(w) >= 3 and w[0].startswith("g"):
                out[(w[0], w[1])] = {m.split(":")[0].lower(): int(m.split(":")[1]) for m in w[2:]}
    return out


def curves(ref, probes, max_depth, scale):
    """empties -> list of per-position step curves [(time, loss), ...]."""
    out = defaultdict(list)
    for path in probes:
        for line in open(path):
            w = line.split()
            if not line.startswith("g") or "stop=" not in line or (w[0], w[1]) not in ref:
                continue
            values = ref[(w[0], w[1])]
            best = max(values.values())
            steps = []
            for s in w[5:]:
                tag, t, _score, move = s.split(":", 3)
                if tag.startswith("d") and int(tag[1:]) > max_depth:
                    break
                move = move.strip('Some()"').lower()
                if move not in values:
                    break
                steps.append((float(t) * scale, best - values[move]))
            if steps:
                out[int(w[1][1:])].append((int(w[0][1:]), steps))
    return out


def expected(curve_list, t):
    return sum(loss_at(steps, t) for steps in curve_list) / len(curve_list)


def loss_at(steps, t):
    k = bisect.bisect_right([s[0] for s in steps], t) - 1
    return steps[max(k, 0)][1]


def decreasing(values):
    """Least-squares non-increasing fit (pool adjacent violators): more time
    should not raise the expected loss, so rises are sampling noise."""
    blocks = []  # [sum, count]
    for v in values:
        blocks.append([v, 1])
        while len(blocks) > 1 and blocks[-2][0] / blocks[-2][1] < blocks[-1][0] / blocks[-1][1]:
            s, c = blocks.pop()
            blocks[-1][0] += s
            blocks[-1][1] += c
    out = []
    for s, c in blocks:
        out += [s / c] * c
    return out


def table_for(data):
    """Turn empties -> L(t) for every turn, using the nearest measured empties."""
    measured = sorted(data)
    return {e: data[min(measured, key=lambda m: (abs(m - e), -m))] for e in MOVES}


def optimise(table, total, cells=400):
    grid = [total * k / cells for k in range(cells + 1)]
    cost = {e: decreasing([expected(table[e], t) for t in grid]) for e in MOVES}
    best = [0.0] * (cells + 1)
    choice = []
    for e in MOVES:
        new, pick = [float("inf")] * (cells + 1), [0] * (cells + 1)
        for b in range(cells + 1):
            for k in range(b + 1):
                v = best[b - k] + cost[e][k]
                if v < new[b] - 1e-12:
                    new[b], pick[b] = v, k
        best = new
        choice.append(pick)
    plan, b = [], cells
    for e, pick in zip(reversed(MOVES), reversed(choice)):
        plan.append((e, grid[pick[b]]))
        b -= pick[b]
    return best[cells], dict(plan)


def smooth(curve_list, total, t, cells=400):
    """The fitted (non-increasing) L(t) on the optimiser's grid."""
    grid = [total * k / cells for k in range(cells + 1)]
    fit = decreasing([expected(curve_list, x) for x in grid])
    return fit[min(cells, bisect.bisect_right(grid, t) - 1)]


def log_shares(paths):
    used = defaultdict(list)
    for path in paths:
        for line in open(path):
            w = line.split()
            if "empties" in w:
                i = w.index("empties")
                used[int(w[i + 1])].append(float(w[i + 5]))
    mean = {e: st.fmean(used[e]) if used[e] else 0.0 for e in range(61)}
    # One side moves on every other empties count; pair odd with even.
    per = {e: (mean[e] + mean[e - 1]) / 2 for e in MOVES}
    s = sum(per.values())
    return {e: v / s for e, v in per.items()}


def held_out(fit, test, total, shares):
    """Fit the plan on `fit`, then compare it with the current shares on
    `test`: summed loss difference and its standard error over positions."""
    _, plan = optimise(table_for(fit), total)
    cur = {e: total * shares[e] for e in MOVES}
    measured = sorted(test)
    diff = var = 0.0
    for b in measured:
        moves = [e for e in MOVES if min(measured, key=lambda m: (abs(m - e), -m)) == b]
        if not moves:
            continue
        d = [sum(loss_at(steps, plan[e]) - loss_at(steps, cur[e]) for e in moves)
             for _, steps in test[b]]
        diff += st.fmean(d)
        var += st.variance(d) / len(d) if len(d) > 1 else 0.0
    return diff, var ** 0.5


def main():
    max_depth, scale, totals, refs, probes, logs = parse(sys.argv[1:])
    raw = curves(reference(refs), probes, max_depth, scale)
    data = {e: [steps for _, steps in v] for e, v in raw.items()}
    print("positions per empties: " + " ".join(f"{e}:{len(data[e])}" for e in sorted(data)))
    table = table_for(data)
    shares = log_shares(logs) if logs else None
    for total in totals:
        loss, plan = optimise(table, total)
        print(f"\n== {total:g} s for turns 52..20 empties (probe seconds x {scale:g})")
        line = f"optimal  loss {loss:5.2f}  "
        line += " ".join(f"{e}:{plan[e]:.2f}" for e in MOVES)
        print(line)
        if shares:
            cur = {e: total * shares[e] for e in MOVES}
            cl = sum(smooth(table[e], total, cur[e]) for e in MOVES)
            print(f"current  loss {cl:5.2f}  " + " ".join(f"{e}:{cur[e]:.2f}" for e in MOVES))
            flat = sum(smooth(table[e], total, total / len(MOVES)) for e in MOVES)
            print(f"equal    loss {flat:5.2f}")
            for name, k in (("even->odd", 0), ("odd->even", 1)):
                fit = {e: [st_ for g, st_ in v if g % 2 == k] for e, v in raw.items()}
                test = {e: [(g, st_) for g, st_ in v if g % 2 != k] for e, v in raw.items()}
                d, se = held_out(fit, test, total, shares)
                print(f"held out {name}: optimal - current {d:+.2f} discs (SE {se:.2f})")


if __name__ == "__main__":
    main()
