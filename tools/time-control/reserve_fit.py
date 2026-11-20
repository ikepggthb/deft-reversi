#!/usr/bin/env python3
"""How well the future-solve reserve predicts the time of late moves.

usage: reserve_fit.py LOG...

For every move with 26 or fewer empties in DEFT_GTP_LOG files, compares the
time it used with the reserve's estimate 2ms + 3 * 6ms * 2^(e-17), and fits
ln(used) on held-out games (even/odd) with features known before the move:
  e           empties
  solved      the side's previous move (2 more empties) ended with an exact
              solve
  prev        ln of that previous move's used time
Prints, per empties, the median used/estimate split by `solved`, then the
RMSE of ln(used) for each model.
"""
import math
import statistics as st
import sys
from collections import defaultdict

import numpy as np


def moves(paths):
    gid = 0
    for path in paths:
        prev_e, last = 99, {}
        for line in open(path):
            w = line.split()
            if "empties" not in w or "steps" not in w:
                continue
            i = w.index("empties")
            e = int(w[i + 1])
            if e > prev_e:
                gid += 1
                last = {}
            prev_e = e
            used = float(w[i + 5])
            steps = w[w.index("steps") + 1:]
            solved = bool(steps) and steps[-1].startswith("e100")
            before = last.get(e + 2)
            if e <= 26 and before is not None:
                yield gid, e, used, before
            last[e] = (used, solved)
        gid += 1


def estimate(e):
    return 0.002 + 3 * 0.006 * 2 ** (e - 17)


def main():
    rows = list(moves(sys.argv[1:]))
    by = defaultdict(list)
    for g, e, used, (pu, ps) in rows:
        by[(e, ps)].append(used / estimate(e))
    print("used / reserve estimate (median, n) by empties; previous own move solved exactly or not")
    for e in sorted({k[0] for k in by}, reverse=True):
        cells = []
        for ps in (True, False):
            v = by.get((e, ps), [])
            cells.append(f"{'solved' if ps else 'not'} {st.median(v):7.3f} ({len(v):4d})" if v else f"{'solved' if ps else 'not'}       - (   0)")
        print(f"  e{e:2d}  " + "   ".join(cells))
    y = np.array([math.log(max(u, 1e-4)) for _, _, u, _ in rows])
    feats = {
        "estimate": lambda e, s, p: [math.log(estimate(e))],
        "e": lambda e, s, p: [e],
        "e+solved": lambda e, s, p: [e, s],
        "e+solved+prev": lambda e, s, p: [e, s, math.log(max(p, 1e-4))],
        "e+solved+prev+inter": lambda e, s, p: [e, s, math.log(max(p, 1e-4)), s * math.log(max(p, 1e-4))],
    }
    print("ln(used) on held-out games (RMSE, bias); `estimate` is the reserve itself, not fitted")
    est = np.array([math.log(estimate(e)) for _, e, _, _ in rows])
    print(f"  {'reserve as is':<22} rmse {np.sqrt(np.mean((y - est) ** 2)):.3f}  bias {np.mean(y - est):+.3f}")
    for name, f in feats.items():
        x = np.array([[1.0] + f(e, float(ps), pu) for _, e, _, (pu, ps) in rows])
        err = np.zeros(len(rows))
        for k in (0, 1):
            a = np.array([r[0] % 2 == k for r in rows])
            beta, *_ = np.linalg.lstsq(x[a], y[a], rcond=None)
            err[~a] = y[~a] - x[~a] @ beta
        print(f"  {name:<22} rmse {np.sqrt(np.mean(err ** 2)):.3f}  bias {np.mean(err):+.3f}")
    x = np.array([[1.0] + feats["e+solved+prev"](e, float(ps), pu) for _, e, _, (pu, ps) in rows])
    beta, *_ = np.linalg.lstsq(x, y, rcond=None)
    print("  coefficients e+solved+prev: " + " ".join(f"{n}={b:+.3f}" for n, b in zip(["const", "e", "solved", "ln_prev"], beta)))


if __name__ == "__main__":
    main()
