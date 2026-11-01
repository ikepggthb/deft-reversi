"""MPC error models, separated into their parts: the mean correction, the
scale (standard deviation or RMS) and its functional form, and how the cut
multiple t is calibrated. Checked at the level the engine cuts.

usage: python3 compare_models.py positions.tsv exact25-28-positions.tsv > models.txt

Parts
  mean   none   : predicted = v                     (a = 1, b = 0)
         parity : predicted = v + b, b per (exact or not, parity of the shallow
                  depth), the mean residual of the training pairs (a = 1)
         shared : a, b linear in features (shared formula of compare_formulas.py)
         cell   : per-cell lines, midgame regressed on [1, e, d, s], exact table
                  with the last measured count of each parity reused (model A)
  scale  the RMS of the residual deep - predicted, so with mean = none it is
         the RMS of deep - v (model F when combined with mean none)
         table  : per-cell RMS, midgame regressed on [1, e, d, s], exact table (A)
         quad   : Edax form;  cubic : Egaroucid form;  loglin : D;  G : E
  t      normal : two-sided normal quantile, same t on both sides
         abs    : quantile of |z| on the training pairs (what the engine does now)
         side   : separate quantiles of z for the upper and the lower cut
         fc     : t per side such that the engine-level false-cut rate on the
                  training pairs (see below) equals (1 - p) / 2 on each side

Split: positions are assigned to train or test by a hash of the board, so
identical boards never straddle the split. Boards of the 25..28 exact set
are removed from training (their midgame values are in positions.tsv).

Engine-level checks, with integer bounds as the engine computes them:
  upper cut at beta  if v >= ceil((beta - b + t_up * sigma) / a)
  lower cut at alpha if v <= floor((alpha - b - t_lo * sigma) / a)
  false cut  : upper at beta = deep + 1, lower at alpha = deep - 1
  efficiency : mean probability of a (correct) upper cut at beta = deep - k
               and lower cut at alpha = deep + k, over k = 1..8
"""
import hashlib
import math
import sys
from statistics import NormalDist

import numpy as np

HERE = __import__("os").path.dirname(__file__) or "."
exec(open(HERE + "/../2026-11-01-mpc-formulas/compare_formulas.py").read().split("def main():")[0])

STAGE_CHECK = [0.85, 0.98]


def load_boards(path):
    keys = []
    with open(path) as f:
        next(f), next(f)
        for line in f:
            p = line.split("\t")
            keys.append(p[2] + p[3])
    return keys


def in_test(key):
    return hashlib.md5(key.encode()).digest()[0] & 1 == 1


# ---------- mean models: return (a, b) per sample ----------

class NoMean:
    def __init__(self, S):
        pass

    def ab(self, S):
        return np.ones(len(S)), np.zeros(len(S))


class ParityMean:
    def __init__(self, S):
        r = S[:, 5] - S[:, 4]
        self.b = {(x, par): float(r[(S[:, 0] == x) & (S[:, 3] % 2 == par)].mean()) for x in (0, 1) for par in (0, 1)}

    def ab(self, S):
        b = np.array([self.b[(int(x), int(s) % 2)] for x, s in zip(S[:, 0], S[:, 3])])
        return np.ones(len(S)), b


class Shared:
    def __init__(self, S):
        self.m = SharedMean(S)

    def ab(self, S):
        f = mean_features(S)
        k = f.shape[1]
        return f @ self.m.beta[:k], f @ self.m.beta[k:]


class CellMean:
    def __init__(self, S):
        self.ref = Reference(S)

    def ab(self, S):
        a, b = np.empty(len(S)), np.empty(len(S))
        mid = S[:, 0] == 0
        F = np.stack([np.ones(mid.sum()), S[mid, 1], S[mid, 2], S[mid, 3]], 1)
        a[mid] = F @ self.ref.coef[0]
        b[mid] = F @ self.ref.coef[1]
        for i in np.flatnonzero(~mid):
            aa, bb, _ = self.ref.exact_rule(int(S[i, 1]))
            a[i], b[i] = aa, bb
        return a, b


# ---------- scale models: sigma per sample from residuals r ----------

class TableScale:
    def __init__(self, S, r):
        keys, inv = cells(S, None)
        n = np.bincount(inv)
        rms = np.sqrt(np.bincount(inv, r * r) / n)
        mid = (keys[:, 0] == 0) & (n >= 8)
        F = np.stack([np.ones(mid.sum()), keys[mid, 1], keys[mid, 2], keys[mid, 3]], 1)
        self.coef = np.linalg.lstsq(F, rms[mid], rcond=None)[0]
        self.lo, self.hi = rms[mid].min(), rms[mid].max()
        self.ex = {int(k[1]): v for k, v, nn in zip(keys, rms, n) if k[0] == 1 and nn >= 8}

    def __call__(self, S):
        out = np.empty(len(S))
        mid = S[:, 0] == 0
        F = np.stack([np.ones(mid.sum()), S[mid, 1], S[mid, 2], S[mid, 3]], 1)
        out[mid] = np.clip(F @ self.coef, self.lo, self.hi)
        for i in np.flatnonzero(~mid):
            e = int(S[i, 1])
            if e not in self.ex:
                e = max(k for k in self.ex if k % 2 == e % 2 and k < e)
            out[i] = self.ex[e]
        return out


def scale_model(kind, S, r):
    if kind == "table":
        return TableScale(S, r)
    if kind == "quad":
        return CellFit(S, r, edax_form, [0.05, 0.02, -0.05, 0.0, 1.0, 2.0]).deviation
    if kind == "cubic":
        return CellFit(S, r, egaroucid_form, [0.7, -4.5, 1.1, -0.8, 4.6, 3.8, 1.8], 10.0).deviation
    if kind == "loglin":
        return LogLinear(S, r).deviation
    if kind == "G":
        return GModel(S, r).deviation
    raise ValueError(kind)


MEANS = {"none": NoMean, "parity": ParityMean, "shared": Shared, "cell": CellMean}
SCALES = ["table", "quad", "cubic", "loglin", "G"]


class Model:
    def __init__(self, mean, scale, train):
        self.mean = MEANS[mean](train)
        a, b = self.mean.ab(train)
        r = train[:, 5] - a * train[:, 4] - b
        self.sigma = scale_model(scale, train, r)
        z = r / self.sigma(train)
        self.z_train = z
        self.train_kind = train[:, 0]
        self.train = train
        self._fc = {}

    def thresholds(self, method, p, x):
        z = self.z_train[self.train_kind == x]
        if method == "normal":
            t = NormalDist().inv_cdf(1 - (1 - p) / 2)
            return t, t
        if method == "abs":
            t = float(np.quantile(np.abs(z), p))
            return t, t
        if method == "side":
            # An upper cut is wrong when deep < predicted - t_up * sigma, that
            # is z < -t_up: the lower tail sets t_up, the upper tail t_lo.
            return float(-np.quantile(z, (1 - p) / 2)), float(np.quantile(z, 1 - (1 - p) / 2))
        if method == "fc":
            if (p, x) not in self._fc:
                T = self.train[self.train_kind == x]
                a, b = self.mean.ab(T)
                sg = self.sigma(T)
                v, deep = T[:, 4], T[:, 5]
                target = (1 - p) / 2
                def solve(rate):
                    lo_t, hi_t = -2.0, 8.0
                    for _ in range(50):
                        mid = (lo_t + hi_t) / 2
                        if rate(mid) > target:
                            lo_t = mid
                        else:
                            hi_t = mid
                    return hi_t
                tu = solve(lambda t: np.mean(v >= np.ceil((deep + 1 - b + t * sg) / a)))
                tl = solve(lambda t: np.mean(v <= np.floor((deep - 1 - b - t * sg) / a)))
                self._fc[(p, x)] = (tu, tl)
            return self._fc[(p, x)]
        raise ValueError(method)

    def check(self, T, method):
        """Engine-level numbers on test pairs T, per kind and stage."""
        a, b = self.mean.ab(T)
        sg = self.sigma(T)
        v, deep = T[:, 4], T[:, 5]
        r = deep - a * v - b
        out = {}
        for x in (0, 1):
            m = T[:, 0] == x
            if not m.any():
                continue
            row = {"n": int(m.sum()), "bias": float(r[m].mean()),
                   "rmsz": float(np.sqrt(np.mean((r[m] / sg[m]) ** 2)) /
                                 np.sqrt(np.mean(self.z_train[self.train_kind == x] ** 2)))}
            for p in STAGE_CHECK:
                tu, tl = self.thresholds(method, p, x)
                am, bm, sm, vm, dm = a[m], b[m], sg[m], v[m], deep[m]
                hi = lambda beta: np.ceil((beta - bm + tu * sm) / am)
                lo = lambda alpha: np.floor((alpha - bm - tl * sm) / am)
                fc_up = float(np.mean(vm >= hi(dm + 1)))
                fc_lo = float(np.mean(vm <= lo(dm - 1)))
                eff = float(np.mean([np.mean(vm >= hi(dm - k)) + np.mean(vm <= lo(dm + k)) for k in range(1, 9)]) / 2)
                row[p] = (fc_up, fc_lo, eff)
            out[x] = row
        return out


def table(title, rows, x, note=""):
    print(f"\n## {title}{note}\n")
    print("model (mean / scale / t)          n     bias  rms(z)  | 85%: false up  false low  eff  | 98%: false up  false low  eff")
    for name, res in rows:
        if x not in res:
            continue
        r = res[x]
        s = f"{name:<32} {r['n']:6d} {r['bias']:+6.2f}  {r['rmsz']:5.3f}  |"
        for p in STAGE_CHECK:
            fu, fl, ef = r[p]
            s += f"   {fu:.4f}    {fl:.4f}   {ef:.3f} |"
        print(s)


def distribution(name, z, s):
    def mom(v):
        v = v - v.mean()
        return float(np.mean(v ** 3) / np.mean(v ** 2) ** 1.5), float(np.mean(v ** 4) / np.mean(v ** 2) ** 2)
    sk, ku = mom(z)
    zs = z / z.std()
    print(f"  {name:<34} n {len(z):6d}  skew {sk:+.3f}  kurt {ku:5.2f}  "
          f"P(z>2) {np.mean(zs > 2):.4f} P(z<-2) {np.mean(zs < -2):.4f}  "
          f"P(z>3) {np.mean(zs > 3):.4f} P(z<-3) {np.mean(zs < -3):.4f}")


def main():
    E1, I1, X1, V1 = load(sys.argv[1])
    K1 = load_boards(sys.argv[1])
    E2, I2, X2, V2 = load(sys.argv[2])
    K2 = load_boards(sys.argv[2])
    held = set(K2)
    test_pos = np.array([in_test(k) for k in K1])
    blocked = np.array([k in held for k in K1])
    trp = ~test_pos & ~blocked
    train = pairs(E1[trp], X1[trp], V1[trp])
    test = pairs(E1[test_pos], X1[test_pos], V1[test_pos])
    far = pairs(E2, X2, V2)
    far = far[far[:, 0] == 1]
    print(f"positions: train {trp.sum()}, test {test_pos.sum()}, removed from training (in the 25..28 set) {(blocked & ~test_pos).sum()}")
    print(f"pairs: train {len(train)}, test {len(test)}, exact 25..28 {len(far)}")
    print("targets per side: 85% -> 0.075, 98% -> 0.010")

    combos = [(m, s) for m in MEANS for s in SCALES]
    built = {(m, s): Model(m, s, train) for m, s in combos}

    for method in ("fc", "side", "abs", "normal"):
        rows = [(f"{m} / {s} / {method}", built[(m, s)].check(test, method)) for m, s in combos]
        table("Midgame, held-out positions", rows, 0, f"  (t: {method})")
        table("Exact 12..24, held-out positions", rows, 1, f"  (t: {method})")
        rows = [(f"{m} / {s} / {method}", built[(m, s)].check(far, method)) for m, s in combos]
        table("Exact 25..28 (not used for fitting)", rows, 1, f"  (t: {method})")

    # Extrapolation of exact rules: fit with exact <= 20, test 21..24 and 25..28.
    keep = (train[:, 0] == 0) | (train[:, 1] <= 20)
    sub = {(m, s): Model(m, s, train[keep]) for m, s in combos}
    near = test[(test[:, 0] == 1) & (test[:, 1] >= 21)]
    rows = [(f"{m} / {s} / fc", sub[(m, s)].check(near, "fc")) for m, s in combos]
    table("Extrapolation: exact fitted on 12..20, tested on 21..24 (1..4 beyond)", rows, 1)
    rows = [(f"{m} / {s} / fc", sub[(m, s)].check(far, "fc")) for m, s in combos]
    table("Extrapolation: exact fitted on 12..20, tested on 25..28 (5..8 beyond)", rows, 1)

    # Extrapolation of midgame depth: fit d <= 11, test 12..14.
    keep = (train[:, 0] == 1) | (train[:, 2] <= 11)
    sub = {(m, s): Model(m, s, train[keep]) for m, s in combos}
    deep = test[(test[:, 0] == 0) & (test[:, 2] >= 12)]
    rows = [(f"{m} / {s} / fc", sub[(m, s)].check(deep, "fc")) for m, s in combos]
    table("Extrapolation: midgame fitted on depth <= 11, tested on 12..14", rows, 0)

    # False cuts and efficiency by the parity of the shallow depth (t: fc).
    for x, kind in ((0, "Midgame"), (1, "Exact 12..24")):
        for par in (0, 1):
            T = test[(test[:, 0] == x) & (test[:, 3] % 2 == par)]
            rows = [(f"{m} / {s} / fc", built[(m, s)].check(T, "fc")) for m, s in combos if s in ("table", "loglin")]
            table(f"{kind}, shallow depth {'odd' if par else 'even'}, held-out", rows, x)

    print("\n## Residual distribution (test pairs, z = residual / sigma)\n")
    for m, s in (("none", "table"), ("cell", "table")):
        mod = built[(m, s)]
        a, b = mod.mean.ab(test)
        z = (test[:, 5] - a * test[:, 4] - b) / mod.sigma(test)
        for x, kind in ((0, "mid"), (1, "exact")):
            for par in (0, 1):
                k = (test[:, 0] == x) & (test[:, 3] % 2 == par)
                distribution(f"{m}/{s} {kind} shallow {'odd' if par else 'even'}", z[k], test[k])
        az, bz = mod.mean.ab(far)
        zf = (far[:, 5] - az * far[:, 4] - bz) / mod.sigma(far)
        distribution(f"{m}/{s} exact 25..28", zf, far)

    print("\n## Fitted per-side t on training pairs (mean / scale): 85% up, low; 98% up, low")
    for (m, s), mod in built.items():
        line = f"  {m}/{s}:"
        for x in (0, 1):
            ts = [mod.thresholds("side", p, x) for p in STAGE_CHECK]
            line += f"  {'mid' if x == 0 else 'exact'} " + " ".join(f"{u:.2f}/{l:.2f}" for u, l in ts)
        print(line)


main()
