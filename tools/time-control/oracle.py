#!/usr/bin/env python3
"""Stopping rules compared with the iteration times known in advance.

usage: oracle.py PROBE REF LOG_FOR_SOFT_SHAPE... -- TOTAL...

PROBE and REF are the loss-table data (probe and refer output). A "game" is
the positions taken from one self-play game (one per empties count). Every
rule sees the exact completion time of each iteration of the position, so
no iteration is started that does not finish; the rules differ only in when
they stop. Midgame positions (more than 28 empties) keep iterations up to
depth 22, below the depth-26 reference.

  current   game_clock as now, with a perfect forecast: start the next
            iteration if it ends within min(target * (1 + instability),
            2 * target); target = c * soft(e), soft's shape over the empties
            from the median soft in the given GTP logs; instability 1 on a
            best-move change, halved on each confirmation
  flat      the same without the instability extension
  value     continue if, for some later iteration, the expected loss
            reduction up to it divided by the time to reach it is at least
            lambda (looking ahead, so cheap low-value steps before a valuable
            one, such as the midgame depths before the first endgame stage,
            are not a dead end). The expected reduction of a step is the mean
            over iterations of the same kind (midgame step, endgame entry,
            endgame stage); for the next step also of the same recent history
            (best move changed in the last two iterations or not). Means are
            learned on the other half of the games

  kind      the value rule without the history: every step at the mean of
            its kind

c and lambda are set on the training half so that the mean time per game is
TOTAL (probe seconds); the rules are then compared on the other half, and
again with the halves swapped. Printed: mean summed loss and time per game
on held-out games, and the paired difference to `current` with its standard
error over games.
"""
import bisect
import statistics as st
import sys
from collections import defaultdict


def load(probe, ref_path):
    ref = {}
    for line in open(ref_path):
        w = line.split()
        ref[(w[0], w[1])] = {m.split(":")[0].lower(): int(m.split(":")[1]) for m in w[2:]}
    games = defaultdict(list)
    for line in open(probe):
        w = line.split()
        if not line.startswith("g") or (w[0], w[1]) not in ref:
            continue
        values = ref[(w[0], w[1])]
        best = max(values.values())
        e = int(w[1][1:])
        steps = []
        for s in w[5:]:
            tag, t, _score, move = s.split(":", 3)
            move = move.strip('Some()"').lower()
            if (e > 28 and tag[0] == "d" and int(tag[1:]) > 22) or move not in values:
                break
            steps.append((tag, float(t), move, best - values[move]))
        if steps:
            games[int(w[0][1:])].append((e, steps))
    return games


def soft_shape(paths):
    soft = defaultdict(list)
    for path in paths:
        for line in open(path):
            w = line.split()
            if "empties" in w:
                i = w.index("empties")
                # The logged budget is hard; in the midgame hard = 2 soft.
                soft[int(w[i + 1])].append(float(w[i + 3]) / 2)
    med = {e: st.median(v) for e, v in soft.items() if v}
    mean = st.fmean(med.values())
    return lambda e: med.get(e, med[min(med, key=lambda m: abs(m - e))]) / mean


def kind(prev_tag, tag):
    if tag[0] == "d":
        return "mid"
    return "entry" if prev_tag[0] == "d" else "stage"


def history(steps, k):
    """Whether the best move changed in the two iterations up to k."""
    if k < 2:
        return "early"
    return "changed" if steps[k][2] != steps[k - 1][2] or steps[k - 1][2] != steps[k - 2][2] else "stable"


def train_gains(games):
    gains = defaultdict(list)
    for positions in games:
        for _e, steps in positions:
            for k in range(len(steps) - 1):
                key = (kind(steps[k][0], steps[k + 1][0]), history(steps, k))
                gains[key].append(steps[k][3] - steps[k + 1][3])
                gains[(key[0], "any")].append(steps[k][3] - steps[k + 1][3])
    return {key: st.fmean(v) for key, v in gains.items()}


def run_current(steps, target, instability_on=True):
    instability, k = 0.0, 0
    while k + 1 < len(steps):
        if k > 0:
            changed = steps[k][2] != steps[k - 1][2]
            instability = 1.0 if changed else instability * 0.5
        limit = min(target * (1 + (instability if instability_on else 0)), 2 * target)
        if steps[k + 1][1] > limit:
            break
        k += 1
    return k


def expected_gain(gains, key):
    # Loss reductions are non-negative in expectation; a negative mean is noise.
    return max(gains.get(key, 0.0), 1e-4)


def run_value(steps, lam, gains, use_history=True):
    """Continue while some later iteration j gives an expected gain per
    second, from now to j, of at least lambda: the next step uses the recent
    history, later steps the mean over histories."""
    k = 0
    while k + 1 < len(steps):
        go, total = False, 0.0
        for j in range(k + 1, len(steps)):
            key = kind(steps[j - 1][0], steps[j][0])
            recent = history(steps, k) if use_history and j == k + 1 else "any"
            total += expected_gain(gains, (key, recent))
            if total / max(steps[j][1] - steps[k][1], 1e-6) >= lam:
                go = True
                break
        if not go:
            break
        k += 1
    return k


def play(games, rule, param, shape, gains):
    out = []
    for positions in games:
        loss = time = 0.0
        for e, steps in positions:
            if rule in ("value", "kind"):
                k = run_value(steps, param, gains, rule == "value")
            else:
                k = run_current(steps, param * shape(e), rule == "current")
            loss += steps[k][3]
            time += steps[k][1]
        out.append((loss, time))
    return out


def fit(games, rule, total, shape, gains):
    """The parameter giving `total` seconds per game on these games."""
    lo, hi = (1e-4, 1e4)
    for _ in range(80):
        mid = (lo * hi) ** 0.5
        t = st.fmean(x[1] for x in play(games, rule, mid, shape, gains))
        # Time rises with c and falls with lambda.
        if (t < total) == (rule not in ("value", "kind")):
            lo = mid
        else:
            hi = mid
    return (lo * hi) ** 0.5


def main():
    cut = sys.argv.index("--")
    probe, ref, logs = sys.argv[1], sys.argv[2], sys.argv[3:cut]
    totals = [float(x) for x in sys.argv[cut + 1:]]
    data = load(probe, ref)
    shape = soft_shape(logs)
    ids = sorted(data)
    print(f"games {len(ids)}, positions per game {st.fmean(len(data[g]) for g in ids):.1f}")
    for total in totals:
        results = defaultdict(list)  # rule -> [(loss, time)] over held-out games, both folds
        for k in (0, 1):
            train = [data[g] for g in ids if g % 2 == k]
            test = [data[g] for g in ids if g % 2 != k]
            gains = train_gains(train)
            for rule in ("current", "flat", "kind", "value"):
                param = fit(train, rule, total, shape, gains)
                results[rule] += play(test, rule, param, shape, gains)
        print(f"\n== {total:g} probe seconds per game")
        base = results["current"]
        for rule in ("current", "flat", "kind", "value"):
            r = results[rule]
            line = (f"  {rule:<8} loss {st.fmean(x[0] for x in r):5.2f}  "
                    f"time {st.fmean(x[1] for x in r):6.2f}s")
            if rule != "current":
                d = [a[0] - b[0] for a, b in zip(r, base)]
                line += f"  minus current {st.fmean(d):+.2f} (SE {st.stdev(d) / len(d) ** 0.5:.2f})"
            print(line)


if __name__ == "__main__":
    main()
