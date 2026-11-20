#!/usr/bin/env python3
"""How well features predict the loss reduction of the next iteration.

usage: value_fit.py TABLE REF...

TABLE is the output of features.py, REF the refer output for the same game
ids (concatenate with the same offsets). The loss of an iteration is the
reference value of the best move minus that of the move it chose; midgame
iterations deeper than 22 are left out (the reference is a depth-26
search). The target is loss(k) - loss(k+1), the gain of running iteration
k+1 after k. Least squares on held-out games (even/odd); printed: MSE and
R^2 against the mean, and the mean gain where the model predicts the top
and bottom fifth (what a stopping rule could tell apart).
"""
import math
import sys
from collections import defaultdict

import numpy as np


def kind(tag, nxt):
    if nxt[0] == "d":
        return "mid"
    if tag[0] == "d":
        return "entry"
    return "exact" if nxt == "e100" else "stage"


def load(table, refs):
    ref = {}
    for path in refs:
        for line in open(path):
            w = line.split()
            if len(w) > 2 and w[0].startswith("g"):
                ref[(int(w[0][1:]), int(w[1][1:]))] = {m.split(":")[0].lower(): int(m.split(":")[1]) for m in w[2:]}
    pos = defaultdict(list)
    head = None
    for line in open(table):
        w = line.rstrip("\n").split("\t")
        if head is None:
            head = w
            continue
        r = dict(zip(head, w))
        g, e = int(r["game"]), int(r["empties"])
        pos[(g, e)].append(r)
    rows = []
    for (g, e), its in pos.items():
        values = ref.get((g, e))
        if not values:
            continue
        best = max(values.values())
        seq = []
        for r in its:
            if (e > 28 and r["tag"][0] == "d" and int(r["tag"][1:]) > 22) or r["move"] not in values:
                break
            seq.append((r, best - values[r["move"]]))
        changes = 0
        for k in range(len(seq) - 1):
            r, loss = seq[k]
            nr, nloss = seq[k + 1]
            prev = seq[k - 1][0] if k else None
            prev2 = seq[k - 2][0] if k > 1 else None
            changed1 = prev is not None and prev["move"] != r["move"]
            changed2 = changed1 or (prev2 is not None and prev2["move"] != prev["move"])
            changes += changed1
            rows.append({
                "game": g, "e": e, "kind": kind(r["tag"], nr["tag"]),
                "changed1": float(changed1), "changed2": float(changed2), "early": float(k < 2),
                "dscore": abs(int(r["score"]) - int(prev["score"])) if prev else 0.0,
                "changes": float(changes), "abs_score": abs(int(r["score"])),
                "near_zero": float(abs(int(r["score"])) <= 4),
                "side_mob": float(r["side_mob"]), "opp_mob": float(r["opp_mob"]),
                "odd": float(r["odd_regions"]), "loss": float(loss), "y": float(loss - nloss),
            })
    return rows


KINDS = ["mid", "entry", "stage", "exact"]


def x_of(r, names):
    out = [1.0 if r["kind"] == k else 0.0 for k in KINDS]
    for n in names:
        # Interact with the kind: a change before an endgame stage is not a
        # change before a midgame step.
        out += [r[n] * (1.0 if r["kind"] == k else 0.0) for k in KINDS]
    return out


MODELS = [
    ("kind", []),
    ("kind x changed (as oracle.py)", ["changed2", "early"]),
    ("+ changed in the last iteration", ["changed2", "early", "changed1"]),
    ("+ |score change|", ["changed2", "early", "changed1", "dscore"]),
    ("+ changes so far, |score|, near 0", ["changed2", "early", "changed1", "dscore", "changes", "abs_score", "near_zero"]),
    ("+ empties, mobility, parity", ["changed2", "early", "changed1", "dscore", "changes", "abs_score", "near_zero", "e", "side_mob", "opp_mob", "odd"]),
]


def main():
    rows = load(sys.argv[1], sys.argv[2:])
    y = np.array([r["y"] for r in rows])
    print(f"transitions {len(rows)}; mean gain by kind: " + ", ".join(
        f"{k} {np.mean([r['y'] for r in rows if r['kind'] == k]):.3f} (n {sum(r['kind'] == k for r in rows)})" for k in KINDS))
    base = np.mean((y - y.mean()) ** 2)
    for name, names in MODELS:
        x = np.array([x_of(r, names) for r in rows])
        pred = np.zeros(len(rows))
        for k in (0, 1):
            a = np.array([r["game"] % 2 == k for r in rows])
            beta, *_ = np.linalg.lstsq(x[a], y[a], rcond=None)
            pred[~a] = x[~a] @ beta
        mse = np.mean((y - pred) ** 2)
        q = np.quantile(pred, [0.2, 0.8])
        lo, hi = y[pred <= q[0]].mean(), y[pred >= q[1]].mean()
        print(f"  {name:<36} mse {mse:.4f}  R2 {1 - mse / base:+.3f}  gain in predicted bottom 20% {lo:.3f}, top 20% {hi:.3f}")


if __name__ == "__main__":
    main()
