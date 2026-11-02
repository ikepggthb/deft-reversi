#!/usr/bin/env python3
"""Exact-solve ProbCut rules as formulas in (empties e, shallow depth s).

usage: python3 fit_formula.py [positions-exact.tsv] > results.txt

Each cell (e, s) is the regression `exact ≈ a·v_s + b` over the positions
with e empties, where v_s is the full-width depth-s value; σ is the
standard deviation of its residuals. Cells with remaining depth
r = e - s >= 4 and 0 <= s <= 14 are used (the range a rule can need).

Every formula is fitted separately for even and odd s, weighted by the
number of positions, and judged on cells it was not fitted on:
  1. larger empties   (fit e <= 20 -> test 21..24, fit e <= 16 -> 17..24)
  2. deeper shallow   (fit s <= 8 -> test 9..14, fit s <= 6 -> 7..14)
  3. both at once     (fit e <= 20 and s <= 8 -> test e 21..24, s 9..14)
  4. each s left out  (fit the other depths)
  5. per-s calibration: how often |z| passes each stage multiple when the
     multiples are pooled over all depths, with the formula fitted on
     e <= 20 and z measured at e 21..24.
"""
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
STAGES = [0.68, 0.77, 0.85, 0.95, 0.98, 0.99]


def load(path):
    pos = {}
    for line in open(path):
        f = line.rstrip("\n").split("\t")
        if line.startswith("#") or f[0] == "empties" or f[4] in ("", "-"):
            continue
        pos.setdefault(int(f[0]), []).append(
            (int(f[4]), np.array(list(map(int, f[5].split(","))), float)))
    return pos


def make_cells(pos):
    cells = []
    for e, rows in sorted(pos.items()):
        y = np.array([r[0] for r in rows], float)
        for s in range(0, min(14, e - 4) + 1):
            x = np.array([r[1][s] for r in rows])
            a, b = np.polyfit(x, y, 1)
            d = np.std(y - a * x - b, ddof=2)
            cells.append(dict(e=e, s=s, r=e - s, a=a, b=b, d=d, n=len(y), x=x, y=y))
    return cells


def wls(X, y, w):
    sw = np.sqrt(w)
    return np.linalg.lstsq(X * sw[:, None], y * sw, rcond=None)[0]


def linear_form(features):
    """A value as a linear combination of features of (e, s)."""
    def fit(cells, key):
        X = np.array([features(c["e"], c["s"]) for c in cells], float)
        co = wls(X, np.array([c[key] for c in cells]), np.array([c["n"] for c in cells], float))
        return lambda e, s: float(np.dot(co, features(e, s)))
    return fit


def saturating_form(s_terms):
    """σ² = (Σ c_k s^k) · (1 - exp(-(e - s)/λ)); λ by grid search, c by least squares."""
    def fit(cells, key="d"):
        w = np.array([c["n"] for c in cells], float)
        y = np.array([c[key] for c in cells])
        best = None
        for lam in np.geomspace(0.3, 60, 300):
            basis = lambda e, s: [s ** k * (1 - math.exp(-(e - s) / lam)) for k in range(s_terms)]
            X = np.array([basis(c["e"], c["s"]) for c in cells])
            co = wls(X, y ** 2, w)
            pred = np.sqrt(np.maximum(X @ co, 1e-9))
            err = float((w * (pred - y) ** 2).sum())
            if best is None or err < best[0]:
                best = (err, lam, co)
        _, lam, co = best
        f = lambda e, s: math.sqrt(max(sum(co[k] * s ** k for k in range(s_terms)), 1e-9)
                                   * (1 - math.exp(-(e - s) / lam)))
        f.params = dict(lam=lam, c=list(co))
        return f
    return fit


SIGMA_FORMS = {
    "sat-quad  σ²=(c0+c1·s+c2·s²)(1-exp(-r/λ))": saturating_form(3),
    "sat-lin   σ²=(c0+c1·s)(1-exp(-r/λ))": saturating_form(2),
    "lin       σ=c0+c1·e+c2·s (Deft midgame form)": linear_form(lambda e, s: [1, e, s]),
    "quad      σ=quadratic in (e,s) (Edax-like)": linear_form(lambda e, s: [1, e, s, e * e, e * s, s * s]),
    "cubic     σ=cubic in (e,s) (Egaroucid-like)": linear_form(
        lambda e, s: [1, e, s, e * e, e * s, s * s, e ** 3, e * e * s, e * s * s, s ** 3]),
}
MEAN_FORM = linear_form(lambda e, s: [1, e, s])


def table_copy(train, key):
    """The current method: the last fitted empties count of the same parity, same s."""
    def f(e, s):
        src = [c for c in train if c["s"] == s and c["e"] % 2 == e % 2]
        return max(src, key=lambda c: c["e"])[key] if src else float("nan")
    return f


def errors(pred, test, key):
    r = np.array([pred(c["e"], c["s"]) - c[key] for c in test])
    r = r[~np.isnan(r)]
    return f"{r.mean():+.3f}/{math.sqrt((r ** 2).mean()):.3f}" if len(r) else "   -   "


def split_report(cells, title, train_of, test_of, with_table):
    print(f"\n## {title}\n")
    print("mean error / RMS against the measured cells (σ and b in discs)\n")
    print(f"{'formula':50s} {'even s':>14s} {'odd s':>14s}")
    for par in (0, 1):
        pass
    rows = {}
    for par in (0, 1):
        tr = [c for c in cells if c["s"] % 2 == par and train_of(c)]
        te = [c for c in cells if c["s"] % 2 == par and test_of(c)]
        for name, form in SIGMA_FORMS.items():
            rows.setdefault("σ " + name, []).append(errors(form(tr, "d"), te, "d"))
        if with_table:
            rows.setdefault("σ table (copy the last measured empties)", []).append(
                errors(table_copy(tr, "d"), te, "d"))
        rows.setdefault("b b=c0+c1·e+c2·s", []).append(errors(MEAN_FORM(tr, "b"), te, "b"))
        if with_table:
            rows.setdefault("b table (copy the last measured empties)", []).append(
                errors(table_copy(tr, "b"), te, "b"))
        rows.setdefault("a a=c0+c1·e+c2·s", []).append(errors(MEAN_FORM(tr, "a"), te, "a"))
    for name, v in rows.items():
        print(f"{name:50s} {v[0]:>14s} {v[1]:>14s}")


def leave_one_s_out(cells):
    print("\n## 4. each shallow depth left out (σ, mean error / RMS over e = 12..24)\n")
    names = list(SIGMA_FORMS)[:2] + [list(SIGMA_FORMS)[2]]
    print(f"{'s':>3s} " + " ".join(f"{n.split()[0]:>14s}" for n in names) + f" {'b':>14s}")
    for s in range(0, 15):
        par = s % 2
        tr = [c for c in cells if c["s"] % 2 == par and c["s"] != s]
        te = [c for c in cells if c["s"] == s]
        out = [errors(SIGMA_FORMS[n](tr, "d"), te, "d") for n in names]
        out.append(errors(MEAN_FORM(tr, "b"), te, "b"))
        print(f"{s:3d} " + " ".join(f"{v:>14s}" for v in out))


def calibration(cells):
    print("\n## 5. per-s calibration out of sample (fit e <= 20, z at e = 21..24)\n")
    print("share of |z| beyond the pooled multiple of each stage; the target is 1 - p\n")
    fits = {}
    for par in (0, 1):
        tr = [c for c in cells if c["s"] % 2 == par and c["e"] <= 20]
        fits[par] = (MEAN_FORM(tr, "a"), MEAN_FORM(tr, "b"),
                     SIGMA_FORMS[list(SIGMA_FORMS)[0]](tr, "d"))
    zs = {}
    for c in cells:
        if c["e"] > 20:
            fa, fb, fd = fits[c["s"] % 2]
            z = np.abs(c["y"] - fa(c["e"], c["s"]) * c["x"] - fb(c["e"], c["s"])) / fd(c["e"], c["s"])
            zs.setdefault(c["s"], []).append(z)
    pooled_in = []
    for c in cells:
        if c["e"] <= 20:
            fa, fb, fd = fits[c["s"] % 2]
            pooled_in.append(np.abs(c["y"] - fa(c["e"], c["s"]) * c["x"] - fb(c["e"], c["s"])) / fd(c["e"], c["s"]))
    pooled_in = np.concatenate(pooled_in)
    t = [float(np.quantile(pooled_in, p)) for p in STAGES]
    print("multiples from e <= 20: " + " ".join(f"{p:.0%}:{v:.2f}" for p, v in zip(STAGES, t)))
    print(f"{'s':>3s} {'pairs':>6s} " + " ".join(f"{p:>6.0%}" for p in STAGES))
    print(f"{'':>3s} {'target':>6s} " + " ".join(f"{1 - p:6.3f}" for p in STAGES))
    for s in sorted(zs):
        z = np.concatenate(zs[s])
        print(f"{s:3d} {len(z):6d} " + " ".join(f"{(z > v).mean():6.3f}" for v in t))


def final_fit(cells):
    print("\n## 6. the formula fitted on every cell (e = 12..24)\n")
    for par in (0, 1):
        tr = [c for c in cells if c["s"] % 2 == par]
        fa, fb = MEAN_FORM(tr, "a"), MEAN_FORM(tr, "b")
        name = list(SIGMA_FORMS)[0]
        fd = SIGMA_FORMS[name](tr, "d")
        print(f"{'even' if par == 0 else 'odd'} s: σ {name.split()[0]} λ={fd.params['lam']:.3f} "
              f"c={[round(v, 4) for v in fd.params['c']]}")
        print("  e\\s " + " ".join(f"{s:>11d}" for s in range(par, 15, 2)))
        for e in (12, 16, 20, 24, 28, 32, 36):
            print(f"  {e:3d} " + " ".join(
                f"{fa(e, s):.3f}/{fb(e, s):+.2f}/{fd(e, s):.2f}" if e - s >= 4 else f"{'':>11s}"
                for s in range(par, 15, 2)).replace("/", " "))
    print("  (each entry: a b σ)")


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else HERE / "positions-exact.tsv"
    cells = make_cells(load(path))
    print(f"# {len(cells)} cells from {path}")
    split_report(cells, "1a. larger empties: fit e <= 20, test e = 21..24",
                 lambda c: c["e"] <= 20, lambda c: c["e"] > 20, True)
    split_report(cells, "1b. larger empties: fit e <= 16, test e = 17..24",
                 lambda c: c["e"] <= 16, lambda c: c["e"] > 16, True)
    split_report(cells, "2a. deeper shallow search: fit s <= 8, test s = 9..14",
                 lambda c: c["s"] <= 8, lambda c: c["s"] > 8, False)
    split_report(cells, "2b. deeper shallow search: fit s <= 6, test s = 7..14",
                 lambda c: c["s"] <= 6, lambda c: c["s"] > 6, False)
    split_report(cells, "3. both: fit e <= 20 and s <= 8, test e = 21..24 and s = 9..14",
                 lambda c: c["e"] <= 20 and c["s"] <= 8, lambda c: c["e"] > 20 and c["s"] > 8, False)
    leave_one_s_out(cells)
    calibration(cells)
    final_fit(cells)


if __name__ == "__main__":
    main()
