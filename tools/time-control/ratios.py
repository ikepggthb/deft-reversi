#!/usr/bin/env python3
"""Cost ratios between successive search iterations.

usage: ratios.py [--by-empties] FILE...
       ratios.py --waste PLAN_LOG... -- LOG...

FILE is probe output (tools/time-control/probe) or a DEFT_GTP_LOG file of
the referee. For each pair of successive iterations it takes the ratio of
their costs (time since the previous iteration finished) and groups them:

  mid->end    last midgame depth -> first endgame stage
  mid->full   last midgame depth -> exact solve (no selective stage)
  sel->full   last selective endgame stage -> exact solve
  sel->sel    selective stage -> next selective stage
  mid->mid    midgame depth d -> d + 2

When the time limit stopped the pending iteration, the time it ran gives a
lower bound on its ratio (a censored sample). Quantiles are shown twice:
with censored samples at their bound (a lower estimate) and at infinity
(an upper estimate); they agree when nothing is censored.

--waste counts, in DEFT_GTP_LOG files, the moves that used at least 90% of
their hard limit while the pending iteration was the first endgame stage or
the exact solve: time spent on an iteration that did not finish. Logs of
builds without `next` get it from the plan seen in PLAN_LOG (same plan).
"""
import re
import sys
from collections import defaultdict


def kind(tag):
    if tag.startswith("d"):
        return "mid"
    return "full" if tag == "e100" else "sel"


def records(path):
    """Yield (empties, [(tag, seconds)], pending tag or None, stop seconds or None).

    stop is the time the search ended while running `pending`, when the
    time limit (not a forecast) ended it."""
    for line in open(path):
        w = line.split()
        if "steps" in w:  # GTP log: OPENING empties E budget B used U ... next N steps ...
            i = w.index("empties")
            empties, budget, used = int(w[i + 1]), float(w[i + 3]), float(w[i + 5])
            pending = w[w.index("next") + 1]
            steps = [(s.split(":")[0], float(s.split(":")[1])) for s in w[w.index("steps") + 1:]]
            pending = None if pending == "-" else pending
            # A forecast stops right after an iteration; a hard limit stops
            # while the pending iteration runs.
            last = steps[-1][1] if steps else 0.0
            hit = pending is not None and used >= 0.95 * budget and used - last > 0.002
            yield empties, steps, pending, (used if hit else None)
        elif line.startswith("g") and " e" in line and "stop=" in line:
            empties = int(w[1][1:])
            total = float(w[3].split("=")[1])
            pending = w[4].split("=")[1]
            pending = None if pending == "-" else pending
            steps = [(s.split(":")[0], float(s.split(":")[1])) for s in w[5:]]
            hit = "TimeLimit" in w[2] and pending is not None
            yield empties, steps, pending, (total if hit else None)


def collect(paths):
    out = defaultdict(list)  # (transition, empties) -> [(ratio, censored)]
    for path in paths:
        for empties, steps, pending, stop in records(path):
            costs, prev = [], 0.0
            for tag, t in steps:
                costs.append((tag, t - prev))
                prev = t
            for (a, ca), (b, cb) in zip(costs, costs[1:]):
                if ca <= 0:
                    continue
                key = f"{kind(a)}->{kind(b)}"
                if key == "mid->sel":
                    key = "mid->end"
                out[(key, empties)].append((cb / ca, False))
            if stop is not None and costs and costs[-1][1] > 0:
                a, ca = costs[-1]
                key = f"{kind(a)}->{kind(pending)}"
                if key == "mid->sel":
                    key = "mid->end"
                out[(key, empties)].append(((stop - prev) / ca, True))
    return out


def quantile(values, q):
    values = sorted(values)
    if not values:
        return float("nan")
    k = min(len(values) - 1, max(0, round(q * (len(values) - 1))))
    return values[k]


def show(label, samples):
    lo = [r for r, _ in samples]
    hi = [float("inf") if c else r for r, c in samples]
    cens = sum(c for _, c in samples)
    qs = "  ".join(
        f"{quantile(lo, q):6.2f}" + (f"-{quantile(hi, q):<6.2f}" if quantile(hi, q) != quantile(lo, q) else " " * 7)
        for q in (0.25, 0.5, 0.75, 0.9))
    print(f"{label:<16} n {len(samples):4d} cens {cens:3d}   {qs}")


def tag_of(kind):
    m = re.match(r"Depth\{depth:(\d+)", kind)
    if m:
        return f"d{m.group(1)}"
    m = re.search(r"selectivity:(\w+)", kind)
    return None if not m else "e100" if m.group(1) == "Full" else "e" + m.group(1)[7:]


def moves(path):
    """Yield (empties, budget, used, last_done, last tag, pending or '?')."""
    for line in open(path):
        w = line.split()
        if "empties" not in w:
            continue
        i = w.index("empties")
        kind = w[i + 9] if len(w) > i + 9 else ""
        pending = w[w.index("next") + 1] if "next" in w else "?"
        yield (int(w[i + 1]), float(w[i + 3]), float(w[i + 5]), float(w[i + 7]),
               tag_of(kind), pending)


def waste(plan_paths, paths):
    plan = {}
    for path in plan_paths:
        for e, _, _, _, last, pending in moves(path):
            if pending != "?" and last:
                plan[(e, last)] = pending
    total = hits = count = 0
    lost = spent = 0.0
    by_kind = defaultdict(int)
    for path in paths:
        for e, budget, used, done, last, pending in moves(path):
            if pending == "?":
                pending = plan.get((e, last), "-")
            spent += used
            count += 1
            if not last or pending == "-":
                continue
            step = f"{kind(last)}->{kind(pending)}"
            if step not in ("mid->sel", "mid->full", "sel->full"):
                continue
            total += 1
            if budget > 0 and used >= 0.9 * budget:
                hits += 1
                lost += used - done
                by_kind[step] += 1
    print(f"moves {count}  stopped before a transition {total}  ran into hard {hits} ({hits / max(total, 1):.1%})  "
          f"{dict(by_kind)}  unfinished time {lost:.1f}s of {spent:.1f}s used ({lost / max(spent, 1e-9):.1%})")


def main():
    args = sys.argv[1:]
    if args and args[0] == "--waste":
        cut = args.index("--")
        return waste(args[1:cut], args[cut + 1:])
    by_empties = "--by-empties" in args
    paths = [a for a in args if not a.startswith("--")]
    data = collect(paths)
    print(f"{'':<16} {'':>6} {'':>8}   {'q25':<13} {'q50':<13} {'q75':<13} {'q90':<13}")
    for key in ("mid->end", "mid->full", "sel->full", "sel->sel", "mid->mid"):
        rows = sorted(e for k, e in data if k == key)
        if not rows:
            continue
        if by_empties:
            for e in rows:
                show(f"{key} {e}", data[(key, e)])
        else:
            for lo, hi in ((15, 19), (20, 23), (24, 27), (28, 31), (32, 36), (37, 60)):
                samples = [s for e in rows if lo <= e <= hi for s in data[(key, e)]]
                if samples:
                    show(f"{key} {lo}-{hi}", samples)


if __name__ == "__main__":
    main()
