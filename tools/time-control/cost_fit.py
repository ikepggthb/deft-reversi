#!/usr/bin/env python3
"""How well features predict the next iteration's time, by transition.

usage: cost_fit.py TABLE [--min-cost S] [--transition mid|entry|stage|exact] [--no-board]

--no-board leaves out the board features (for logtable.py tables).

TABLE is the output of features.py. The target is ln(next_cost / cost),
the log of the ratio the forecast multiplies by. Rows with a censored next
iteration (it did not finish) are counted but left out of the fits; the
share is printed so the bias can be judged. Only iterations that took at
least --min-cost seconds (default 0.01) are used: the forecast's decisions
on faster ones hardly matter.

Models are least squares on the log ratio, fitted on even games and tested
on odd ones and the reverse. Printed per model: RMSE and median absolute
error of the log ratio on held-out games (exp of them is the typical
factor by which a forecast misses) and the mean error (bias).
The `current` model is game_clock.rs as it is (not fitted).
"""
import math
import sys

import numpy as np

ENTRY = lambda e: max(2.0, 5.0 * 1.06 ** (e - 24))
EXACT = lambda e: max(1.0, 3.0 * 1.15 ** (e - 22))


def kind(tag):
    if tag[0] == "d":
        return "mid"
    return "full" if tag == "e100" else "sel"


def transition(tag, nxt):
    a, b = kind(tag), kind(nxt)
    if a == "mid" and b == "mid":
        return "mid"
    if a == "mid":
        return "entry"
    return "exact" if b == "full" else "stage"


def current(r):
    """ln of game_clock.rs's growth for this row."""
    tr = r["tr"]
    e = r["empties"]
    if tr == "mid":
        d, dn = int(r["tag"][1:]), int(r["next_tag"][1:])
        per = 3.0
        if r["prev_tag"].startswith("d") and r["prev_cost"] > 0:
            dp = int(r["prev_tag"][1:])
            per = min(max((r["cost"] / r["prev_cost"]) ** (2 / (d - dp)), 1.5), 6.0)
        return math.log(per) * (dn - d) / 2
    if tr == "entry":
        return math.log(ENTRY(e) * (EXACT(e) if r["next_tag"] == "e100" else 1.0))
    if tr == "exact":
        return math.log(EXACT(e))
    return math.log(1.25)


def load(path, tr_want, min_cost):
    rows, censored = [], 0
    head = None
    for line in open(path):
        w = line.rstrip("\n").split("\t")
        if head is None:
            head = w
            continue
        r = dict(zip(head, w))
        if r["next_tag"] == "-":
            continue
        for k in ("game", "empties", "side_mob", "opp_mob", "odd_regions", "regions",
                  "corners_empty", "disc_diff", "score"):
            r[k] = int(r[k])
        for k in ("t", "cost"):
            r[k] = float(r[k])
        r["prev_cost"] = float(r["prev_cost"]) if r["prev_cost"] else 0.0
        r["next_cost"] = float(r["next_cost"])
        r["tr"] = transition(r["tag"], r["next_tag"])
        if r["tr"] != tr_want or r["cost"] < min_cost:
            continue
        r["cens"] = r["censored"] == "1"
        if r["cens"]:
            censored += 1
        if r["next_cost"] <= 0:
            continue
        r["y"] = math.log(r["next_cost"] / r["cost"])
        rows.append(r)
    return rows, censored


def lnz(x):
    return math.log(x) if x > 0 else 0.0


FEATURES = {
    "prev_ratio": lambda r: lnz(r["cost"] / r["prev_cost"]) if r["prev_cost"] > 0 and r["prev_tag"][0] == r["tag"][0] else 0.0,
    "has_prev": lambda r: 1.0 if r["prev_cost"] > 0 and r["prev_tag"][0] == r["tag"][0] else 0.0,
    "empties": lambda r: r["empties"],
    "depth": lambda r: int(r["tag"][1:]) if r["tag"][0] == "d" else 0,
    "depth_odd": lambda r: int(r["tag"][1:]) % 2 if r["tag"][0] == "d" else 0,
    "log_cost": lambda r: math.log(r["cost"]),
    "side_mob": lambda r: r["side_mob"],
    "opp_mob": lambda r: r["opp_mob"],
    "odd_regions": lambda r: r["odd_regions"],
    "regions": lambda r: r["regions"],
    "corners": lambda r: r["corners_empty"],
    "abs_score": lambda r: abs(r["score"]),
    "next_full": lambda r: 1.0 if r["next_tag"] == "e100" else 0.0,
    "stage": lambda r: int(r["tag"][1:]) if r["tag"][0] == "e" else 0,
}

BOARD = {"side_mob", "opp_mob", "odd_regions", "regions", "corners", "abs_score"}

SHORT = {"prev_ratio": "prev", "has_prev": "", "empties": "e", "depth": "d", "depth_odd": "odd",
         "log_cost": "cost", "side_mob": "mob", "opp_mob": "omob", "odd_regions": "par",
         "regions": "reg", "corners": "cor", "abs_score": "|s|", "next_full": "full", "stage": "stg"}

MODELS = {
    "mid": [
        [],
        ["prev_ratio", "has_prev"],
        ["prev_ratio", "has_prev", "empties", "depth", "depth_odd"],
        ["prev_ratio", "has_prev", "empties", "depth", "depth_odd", "log_cost"],
        ["prev_ratio", "has_prev", "empties", "depth", "depth_odd", "log_cost",
         "side_mob", "opp_mob", "odd_regions", "regions", "corners", "abs_score"],
    ],
    "entry": [
        [],
        ["empties"],
        ["empties", "next_full"],
        ["empties", "next_full", "depth", "depth_odd"],
        ["empties", "next_full", "depth", "depth_odd", "log_cost"],
        ["empties", "next_full", "depth", "depth_odd", "log_cost", "prev_ratio", "has_prev"],
        ["empties", "next_full", "depth", "depth_odd", "log_cost", "prev_ratio", "has_prev",
         "side_mob", "opp_mob", "odd_regions", "regions", "corners", "abs_score"],
    ],
    "exact": [
        [],
        ["empties"],
        ["empties", "stage"],
        ["empties", "stage", "log_cost"],
        ["empties", "stage", "log_cost", "side_mob", "opp_mob", "odd_regions", "regions",
         "corners", "abs_score"],
    ],
    "stage": [
        [],
        ["empties"],
        ["empties", "stage"],
        ["empties", "stage", "log_cost"],
        ["empties", "stage", "log_cost", "side_mob", "opp_mob", "odd_regions", "regions",
         "corners", "abs_score"],
    ],
}


def design(rows, names):
    x = np.array([[1.0] + [FEATURES[n](r) for n in names] for r in rows])
    return x


_erfc = np.vectorize(math.erfc)


def log_sf(z):
    """ln P(Z > z) for a standard normal, stable for large z."""
    out = np.empty_like(z)
    small = z < 5
    out[small] = np.log(0.5 * _erfc(z[small] / math.sqrt(2)))
    zl = z[~small]
    out[~small] = -0.5 * zl ** 2 - np.log(zl * math.sqrt(2 * math.pi))
    return out


def tobit(x, y, cens, iters=4000):
    """Normal regression where censored y are lower bounds: (beta, sigma)."""
    mu, sd = x[:, 1:].mean(0), x[:, 1:].std(0) + 1e-9
    xs = x.copy()
    xs[:, 1:] = (x[:, 1:] - mu) / sd
    beta, *_ = np.linalg.lstsq(xs[~cens], y[~cens], rcond=None)
    theta = np.append(beta, math.log(np.std(y[~cens] - xs[~cens] @ beta) + 1e-6))
    m, v = np.zeros_like(theta), np.zeros_like(theta)
    for t in range(1, iters + 1):
        b, ls = theta[:-1], theta[-1]
        s = math.exp(ls)
        z = (y - xs @ b) / s
        # d/dmu and d/dlog s of the log likelihood
        g_mu = np.where(cens, 0.0, z / s)
        lam = np.exp(-0.5 * z ** 2 - log_sf(z)) / math.sqrt(2 * math.pi)  # hazard
        g_mu = np.where(cens, lam / s, g_mu)
        g_ls = np.where(cens, lam * z, z ** 2 - 1)
        grad = np.append(xs.T @ g_mu, g_ls.sum()) / len(y)
        m = 0.9 * m + 0.1 * grad
        v = 0.999 * v + 0.001 * grad ** 2
        theta += 0.02 * (m / (1 - 0.9 ** t)) / (np.sqrt(v / (1 - 0.999 ** t)) + 1e-8)
    b = theta[:-1].copy()
    b[1:] = b[1:] / sd
    b[0] = b[0] - (b[1:] * mu).sum()
    return b, math.exp(theta[-1])


def loglik(pred, y, cens, sigma):
    z = (y - pred) / sigma
    return np.where(cens, log_sf(z), -0.5 * z ** 2 - math.log(sigma * math.sqrt(2 * math.pi)))


def evaluate(rows, names, use_tobit=False):
    """Held-out errors on finished rows, and held-out log likelihood per row
    (censored rows count by the chance of lasting at least as long)."""
    errs, ll = np.zeros(len(rows)), np.zeros(len(rows))
    for k in (0, 1):
        tr = [i for i, r in enumerate(rows) if r["game"] % 2 == k]
        te = [i for i, r in enumerate(rows) if r["game"] % 2 != k]
        xa, ya = design([rows[i] for i in tr], names), np.array([rows[i]["y"] for i in tr])
        ca = np.array([rows[i]["cens"] for i in tr])
        if use_tobit:
            beta, sigma = tobit(xa, ya, ca)
        else:
            beta, *_ = np.linalg.lstsq(xa[~ca], ya[~ca], rcond=None)
            sigma = float(np.std(ya[~ca] - xa[~ca] @ beta))
        xb = design([rows[i] for i in te], names)
        yb = np.array([rows[i]["y"] for i in te])
        cb = np.array([rows[i]["cens"] for i in te])
        errs[te] = yb - xb @ beta
        ll[te] = loglik(xb @ beta, yb, cb, sigma)
    return errs, ll


def report(name, errs, cens, ll=None):
    e = errs[~cens]
    rmse = float(np.sqrt(np.mean(e ** 2)))
    mae = float(np.median(np.abs(e)))
    # Censored rows: share whose lower bound already exceeds the prediction.
    under = float(np.mean(errs[cens] > 0)) if cens.any() else float("nan")
    line = (f"  {name:<44} rmse {rmse:.3f} (x{math.exp(rmse):.2f})  median|e| {mae:.3f}  "
            f"bias {np.mean(e):+.3f}  censored>pred {under:4.0%}")
    if ll is not None:
        line += f"  loglik {np.mean(ll):+.3f}"
    print(line)


def main():
    path = sys.argv[1]
    args = sys.argv[2:]
    min_cost, trs, board = 0.01, ["mid", "entry", "exact", "stage"], True
    while args:
        if args[0] == "--no-board":
            board, args = False, args[1:]
            continue
        if args[0] == "--min-cost":
            min_cost, args = float(args[1]), args[2:]
        elif args[0] == "--transition":
            trs, args = [args[1]], args[2:]
        else:
            sys.exit(__doc__)
    for tr in trs:
        rows, censored = load(path, tr, min_cost)
        if len(rows) < 20:
            print(f"== {tr}: {len(rows)} rows, skipped")
            continue
        print(f"== {tr}: {len(rows)} rows, censored (left out) {censored} "
              f"({100 * censored / (len(rows) + censored):.0f}%), min cost {min_cost}s")
        cens = np.array([r["cens"] for r in rows])
        cur = np.array([r["y"] - current(r) for r in rows])
        report("current (game_clock.rs)", cur, cens)
        models = MODELS[tr]
        if not board:
            seen, models = set(), []
            for names in MODELS[tr]:
                kept = tuple(n for n in names if n not in BOARD)
                if kept not in seen:
                    seen.add(kept)
                    models.append(list(kept))
        for names in models:
            label = "+".join(x for x in (SHORT.get(n, n) for n in names) if x) if names else "constant"
            for use_tobit in (False, True):
                errs, ll = evaluate(rows, names, use_tobit)
                report(("tobit " if use_tobit else "ols   ") + label, errs, cens, ll)
        full = models[-1]
        y = np.array([r["y"] for r in rows])
        beta, sigma = tobit(design(rows, full), y, cens)
        print("  tobit coefficients of the last model (all rows): " +
              " ".join(f"{n}={b:+.3f}" for n, b in zip(["const"] + full, beta)) + f" sigma={sigma:.3f}")


if __name__ == "__main__":
    main()
