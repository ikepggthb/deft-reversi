#!/usr/bin/env python3
"""How well the game clock works, from the DEFT_GTP_LOG files of referee games.

usage: audit.py SECONDS LOG...

Games are cut where the empties count rises again. The remaining clock is
rebuilt from the time each move used, and soft/hard are recomputed with the
Budget formula of game_clock.rs (one thread; builds without thread scaling).
The forecast ratios are those of 0c53a14; for logs of other builds the
"predicted" columns describe what 0c53a14 would have predicted.

It prints:
  clock     time left at the end of the game, and when the first exact solve
            finished (time after that point has almost no use)
  reserve   the solve reserve booked at the first move against the time all
            moves after the first exact solve actually took
  midgame   used / soft on turns with 34+ empties, and why the search ended:
            forecast (stopped right after an iteration), hard (an iteration
            was running at the limit), done (the plan ran out)
  forecast  actual / predicted cost of the next iteration, by transition,
            over transitions where both iterations finished
  missed    forecast stops whose next iteration would probably have finished
            before hard: the empirical share of actual/predicted ratios that
            fit in the time left to hard, summed over stops
"""
import math
import re
import statistics as st
import sys
from collections import defaultdict


def budget(empties, left):
    turns = max(1, (empties + 1) // 2)
    pool, turns_left, reserve = left, turns, 0.0
    for distance in range(turns - 1, 0, -1):
        solve = 0.002 + 3 * 0.006 * 2 ** (empties - 2 * distance - 17)
        if solve >= pool / turns_left:
            break
        pool -= solve
        reserve += solve
        turns_left -= 1
    share = pool / turns_left
    hard = min(share + min(share, (pool - share) / 2), left)
    return min(share, hard), hard, reserve


def ratio(base, pivot, per, floor, e):
    return max(base * per ** (e - pivot), floor)


def entry(e):
    return ratio(5.0, 24, 1.06, 2.0, e)


def exact(e):
    return ratio(3.0, 22, 1.15, 1.0, e)


def kind(tag):
    return "mid" if tag[0] == "d" else ("full" if tag == "e100" else "sel")


def growth(e, tag, nxt, cost, last_cost, last_depth):
    if tag[0] == "d" and nxt[0] == "d":
        d, dn = int(tag[1:]), int(nxt[1:])
        per = 3.0
        if last_depth is not None and last_cost > 0 and d > last_depth:
            per = min(max((cost / last_cost) ** (2 / (d - last_depth)), 1.5), 6.0)
        return per ** ((dn - d) / 2)
    if tag[0] == "d":
        return entry(e) * exact(e) if nxt == "e100" else entry(e)
    if nxt == "e100":
        return exact(e)
    return entry(e) if nxt[0] == "d" else 1.25


def games(paths):
    for path in paths:
        game, prev = [], 99
        for line in open(path):
            w = line.split()
            if "empties" not in w:
                continue
            i = w.index("empties")
            e = int(w[i + 1])
            if e > prev and game:
                yield game
                game = []
            prev = e
            steps = []
            if "steps" in w:
                steps = [(s.split(":")[0], float(s.split(":")[1])) for s in w[w.index("steps") + 1:]]
            nxt = w[w.index("next") + 1] if "next" in w else "?"
            game.append({"e": e, "hard_log": float(w[i + 3]), "used": float(w[i + 5]),
                         "done": float(w[i + 7]), "steps": steps, "next": nxt})
        if game:
            yield game


def main():
    seconds = float(sys.argv[1])
    paths = sys.argv[2:]
    end_left, solve_left, solve_at, reserve_booked, after_solve = [], [], [], [], []
    mid_ratio, why = [], defaultdict(int)
    errors = defaultdict(list)
    stops = []  # (transition, time left to hard / predicted cost)
    for game in games(paths):
        left = seconds
        first_solve = None
        for k, mv in enumerate(game):
            soft, hard, reserve = budget(mv["e"], max(left - 0.01, 0))
            if k == 0:
                reserve_booked.append(reserve)
            steps, e = mv["steps"], mv["e"]
            if first_solve is None and steps and steps[-1][0] == "e100":
                first_solve = k
                solve_left.append(left - mv["used"])
                solve_at.append(e)
            # forecast replay
            prev_t, last_cost, last_depth, pred = 0.0, 0.0, None, None
            for j, (tag, t) in enumerate(steps):
                cost = t - prev_t
                nxt = steps[j + 1][0] if j + 1 < len(steps) else (mv["next"] if mv["next"] not in ("-", "?") else None)
                if pred is not None and cost > 0:
                    errors[pred[0]].append(cost / pred[1])
                pred = None
                if nxt:
                    g = growth(e, tag, nxt, cost, last_cost, last_depth)
                    pred = (f"{kind(tag)}->{kind(nxt)}", max(cost, 1e-6) * g)
                last_cost, last_depth = cost, (int(tag[1:]) if tag[0] == "d" else None)
                prev_t = t
            ended_by = "done" if mv["next"] == "-" else (
                "hard" if mv["used"] >= 0.9 * mv["hard_log"] and mv["used"] - mv["done"] > 0.002 else "forecast")
            if e >= 34 and steps:
                mid_ratio.append(mv["used"] / soft if soft > 0 else float("nan"))
                why[ended_by] += 1
            if ended_by == "forecast" and pred is not None and mv["hard_log"] > 0:
                stops.append((pred[0], (mv["hard_log"] - mv["used"]) / pred[1]))
            left -= mv["used"]
        end_left.append(left)
        if first_solve is not None:
            after_solve.append(sum(m["used"] for m in game[first_solve + 1:]))
    n = len(end_left)
    pct = lambda x: f"{100 * x / seconds:.1f}%"
    print(f"games {n}")
    print(f"clock     left at end: mean {st.fmean(end_left):.3f}s ({pct(st.fmean(end_left))}), "
          f"median {st.median(end_left):.3f}s")
    if solve_left:
        print(f"          first exact solve finished at {st.median(solve_at)} empties (median; "
              f"q25 {sorted(solve_at)[len(solve_at) // 4]} q75 {sorted(solve_at)[3 * len(solve_at) // 4]}) "
              f"with {st.fmean(solve_left):.3f}s left ({pct(st.fmean(solve_left))}) in {len(solve_left)}/{n} games")
        print(f"reserve   booked at the first move {st.fmean(reserve_booked):.3f}s; "
              f"used after the first solve {st.fmean(after_solve):.3f}s")
    mr = sorted(x for x in mid_ratio if not math.isnan(x))
    tot = sum(why.values())
    if not mr:
        print("(no iteration steps in these logs: older build)")
        return
    print(f"midgame   used/soft median {st.median(mr):.2f} (q25 {mr[len(mr) // 4]:.2f}, q75 {mr[3 * len(mr) // 4]:.2f}); "
          f"ended by " + ", ".join(f"{k} {v / tot:.0%}" for k, v in sorted(why.items())))
    print("forecast  actual/predicted, transitions with both iterations finished:")
    for key in ("mid->mid", "mid->sel", "mid->full", "sel->sel", "sel->full"):
        r = sorted(errors.get(key, []))
        if len(r) < 5:
            continue
        q = lambda p: r[min(len(r) - 1, int(p * len(r)))]
        over = sum(x > 1 for x in r) / len(r)
        print(f"  {key:<9} n {len(r):5d}  q10 {q(.1):.2f}  q25 {q(.25):.2f}  median {q(.5):.2f}  "
              f"q75 {q(.75):.2f}  q90 {q(.9):.2f}  heavier than predicted {over:.0%}")
    # A finished transition is a lower-biased sample (heavy ones are cut);
    # use it as an optimistic estimate of the chance a stopped one fits.
    missed = defaultdict(float)
    count = defaultdict(int)
    for key, room in stops:
        r = errors.get(key)
        if not r:
            continue
        count[key] += 1
        missed[key] += sum(x <= room for x in r) / len(r)
    print("missed    forecast stops; expected number whose next iteration fits before hard:")
    for key in sorted(count):
        print(f"  {key:<9} stops {count[key]:5d}  would fit {missed[key]:7.1f} ({missed[key] / count[key]:.0%})")


if __name__ == "__main__":
    main()
