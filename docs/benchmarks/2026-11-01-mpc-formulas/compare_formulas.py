"""Compare ProbCut formulas on positions measured by `deft-reversi-train mpc`.

usage: python3 compare_formulas.py positions.tsv [far-exact-positions.tsv] > results.txt

The optional second file holds exact solves beyond the fitted range (25 to 28
empties); every model is fitted on the first file and tested on them.

Positions with an even index fit every model; odd ones test it. Each model
predicts the deep value's mean (slope, intercept) and deviation for the pairs
the engine uses: midgame depth d with the reference shallow depth, and exact
solves with the main-branch shallow depth. The engine now cuts at measured
multiples t of the deviation, so a good deviation makes |error| / deviation
the same distribution in every cell; the checks look at that.
"""
import math
import sys

import numpy as np

SHALLOW = [0, 0, 0, 0, 0, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 5, 6, 5, 6, 5, 6, 5, 6, 5, 6]
STAGES = [0.68, 0.77, 0.85, 0.95, 0.98, 0.99]


def exact_shallow(e):
    if e in (12, 14, 16):
        return 4
    if e in (13, 15):
        return 3
    if e in (17, 19):
        return 5
    if e in (18, 20):
        return 6
    return 8 - e % 2 if e >= 29 else 6 - e % 2


def load(path):
    empties, index, exact, values = [], [], [], []
    with open(path) as f:
        next(f), next(f)
        for line in f:
            e, i, _, _, x, v = line.rstrip("\n").split("\t")
            empties.append(int(e))
            index.append(int(i))
            exact.append(math.nan if x == "-" else float(x))
            v = [float(t) for t in v.split(",")]
            values.append(v + [math.nan] * (15 - len(v)))
    return np.array(empties), np.array(index), np.array(exact), np.array(values)


def pairs(E, X, V, every=False):
    """Columns: exact, empties, depth, shallow depth, shallow value, deep value.
    `every` adds all same-parity pairs (for fitting the G model)."""
    out = []
    for d in range(1, 15):
        shallows = range(d % 2, d - 1, 2) if every else ([SHALLOW[d]] if d >= 4 else [])
        for s in shallows:
            m = ~np.isnan(V[:, d])
            n = m.sum()
            out.append(np.stack([np.zeros(n), E[m], np.full(n, d), np.full(n, s), V[m, s], V[m, d]], 1))
    m = ~np.isnan(X)
    for e in np.unique(E[m]):
        k = m & (E == e)
        shallows = range(int(e) % 2, 15, 2) if every else [exact_shallow(int(e))]
        for s in shallows:
            if s < e:
                n = k.sum()
                out.append(np.stack([np.ones(n), E[k], E[k], np.full(n, s), V[k, s], X[k]], 1))
    return np.concatenate(out)


def engine_pair(S):
    sh = np.array([SHALLOW[int(d)] if d < len(SHALLOW) else -1 for d in S[:, 2]])
    ex = np.array([exact_shallow(int(e)) for e in S[:, 1]])
    return np.where(S[:, 0] == 1, S[:, 3] == ex, (S[:, 2] >= 4) & (S[:, 3] == sh))


def cells(S, values):
    """Group rows by (exact, empties, depth, shallow depth)."""
    keys, inverse = np.unique(S[:, :4], axis=0, return_inverse=True)
    return keys, inverse.ravel()


# ---------- mean models ----------

def mean_features(S):
    x, e, d, s = S[:, 0], S[:, 1] / 60, S[:, 2] / 60, S[:, 3] / 60
    odd = S[:, 3] % 2
    return np.stack([np.ones_like(e), e, d, s, odd, x, x * odd, x * e, x * e * e], 1)


class SharedMean:
    def __init__(self, S):
        f = mean_features(S)
        self.beta = np.linalg.lstsq(np.concatenate([f * S[:, 4:5], f], 1), S[:, 5], rcond=None)[0]

    def predict(self, S):
        f = mean_features(S)
        return np.concatenate([f * S[:, 4:5], f], 1) @ self.beta


# ---------- deviation models ----------

def cell_stats(S, r, min_n=8):
    keys, inv = cells(S, r)
    n = np.bincount(inv)
    var = np.bincount(inv, r * r) / n
    keep = n >= min_n
    return keys[keep], var[keep], n[keep]


def gauss_newton(func, p, K, y, w, iters=300):
    p = np.array(p, dtype=float)
    lam = 1e-3
    def cost(q):
        return float(np.sum(w * (y - func(q, K)) ** 2))
    c = cost(p)
    for _ in range(iters):
        f = func(p, K)
        J = np.stack([(func(p + h, K) - f) / 1e-7 for h in np.eye(len(p)) * 1e-7], 1)
        A = (J * w[:, None]).T @ J
        g = (J * w[:, None]).T @ (y - f)
        while True:
            step = np.linalg.solve(A + lam * np.diag(np.diag(A) + 1e-12), g)
            c2 = cost(p + step)
            if c2 < c:
                p, c, lam = p + step, c2, lam * 0.3
                break
            lam *= 10
            if lam > 1e12:
                return p
        if np.abs(step).max() < 1e-10:
            break
    return p


class Reference:
    """Model A, as `deft-reversi-train mpc` fits it today: per-cell lines,
    the midgame regressed linearly in (empties, depth, shallow depth) with the
    deviation clamped to the measured range, and the exact table extended
    with the last measured count of each parity."""

    def __init__(self, S, exact_max=60):
        self.mid, self.ex = {}, {}
        keys, inv = cells(S, None)
        rows = []
        for k, key in enumerate(keys):
            m = inv == k
            if m.sum() < 8:
                continue
            x, y = S[m, 4], S[m, 5]
            a = np.cov(x, y, bias=True)[0, 1] / max(np.var(x), 1e-12)
            b = y.mean() - a * x.mean()
            dev = math.sqrt(np.sum((y - a * x - b) ** 2) / (m.sum() - 2))
            if key[0] == 1:
                if key[1] <= exact_max:
                    self.ex[int(key[1])] = (a, b, dev)
            else:
                rows.append((key[1], key[2], key[3], a, b, dev))
        R = np.array(rows)
        F = np.stack([np.ones(len(R)), R[:, 0], R[:, 1], R[:, 2]], 1)
        self.coef = [np.linalg.lstsq(F, R[:, j], rcond=None)[0] for j in (3, 4, 5)]
        self.lo, self.hi = R[:, 5].min(), R[:, 5].max()

    def exact_rule(self, e):
        if e in self.ex:
            return self.ex[e]
        same = [k for k in self.ex if k % 2 == e % 2 and k < e]
        return self.ex[max(same)]

    def predict(self, S):
        mean, dev = np.empty(len(S)), np.empty(len(S))
        mid = S[:, 0] == 0
        F = np.stack([np.ones(mid.sum()), S[mid, 1], S[mid, 2], S[mid, 3]], 1)
        a, b, d = (F @ c for c in self.coef)
        mean[mid] = a * S[mid, 4] + b
        dev[mid] = np.clip(d, self.lo, self.hi)
        for i in np.flatnonzero(~mid):
            a, b, d = self.exact_rule(int(S[i, 1]))
            mean[i], dev[i] = a * S[i, 4] + b, d
        return mean, dev


def edax_form(p, K):
    u = p[0] * K[:, 1] + p[1] * K[:, 2] + p[2] * K[:, 3]
    return p[3] * u * u + p[4] * u + p[5]


def egaroucid_form(p, K):
    u = p[0] * (64 - K[:, 1]) / 64 + p[1] * K[:, 3] / 60 + p[2] * K[:, 2] / 60
    return p[3] * u ** 3 + p[4] * u * u + p[5] * u + p[6]


class CellFit:
    """Models B and C: a formula in (empties, depth, shallow depth) fitted to
    the cells' standard deviations, weighted by their sizes."""

    def __init__(self, S, r, func, p0, exact_weight=1.0):
        K, var, n = cell_stats(S, r)
        w = n * np.where(K[:, 0] == 1, exact_weight, 1.0)
        self.func, self.p = func, gauss_newton(func, p0, K, np.sqrt(var), w)

    def deviation(self, S):
        return np.maximum(0.5, self.func(self.p, S[:, :4]))


def loglin_features(S):
    x, e, d, s = S[:, 0], S[:, 1] / 60, S[:, 2] / 60, S[:, 3] / 60
    odd = S[:, 3] % 2
    gap = np.log(S[:, 2] - S[:, 3])
    return np.stack([np.ones_like(e), e, e * e, s, s * s, gap, odd, odd * s, x, x * e, x * s], 1)


class LogLinear:
    """Model D: log deviation linear in features, by Gaussian maximum
    likelihood over all pairs."""

    def __init__(self, S, r):
        F = loglin_features(S)
        g = np.linalg.lstsq(F, 0.5 * np.log(r * r + 1.0), rcond=None)[0]
        for _ in range(100):
            z = r * r / np.exp(2 * F @ g)
            step = np.linalg.solve((F * (2 * z)[:, None]).T @ F + 1e-6 * np.eye(F.shape[1]), F.T @ (1 - z))
            g -= step
            if np.abs(step).max() < 1e-10:
                break
        self.g = g

    def deviation(self, S):
        return np.exp(loglin_features(S) @ self.g)


def g_form(p, e, k):
    """G(e, k): the variance of a depth-k value around the exact score of a
    position with e empties. A depth of e - 1 already solves the position,
    so G rises from 0 with the plies r = e - k - 1 left beyond the search
    and levels off at A(e, k): G = A (1 - exp(-r / lambda))."""
    r = np.maximum(e - k - 1, 0)
    ee = e / 60
    loga = p[0] + p[1] * k / 10 + p[2] * ee + p[3] * ee * ee + p[4] * k / 10 * ee + p[5] * (k % 2)
    lam = np.exp(p[6] + p[7] * k / 10)
    return np.exp(loga) * (1 - np.exp(-r / lam))


def g_variance(p, K):
    return np.maximum(g_form(p, K[:, 1], K[:, 3]) - g_form(p, K[:, 1], K[:, 2]), 1e-6)


class GModel:
    """Model E: Var(deep - predicted) = G(e, shallow) - G(e, deep), with
    G(e, e) = 0, so exact rules are the d = e case of the midgame ones.
    Fitted to log cell variances."""

    def __init__(self, S, r):
        K, var, n = cell_stats(S, r)
        f = lambda p, K: np.log(g_variance(p, K))
        self.p = gauss_newton(f, [3.7, -1.0, 0, 0, 0, 0, 1.5, 0], K, np.log(var), n)

    def deviation(self, S):
        return np.sqrt(g_variance(self.p, S[:, :4]))


# ---------- checks ----------

def thresholds(z, exact):
    """Measured multiples per stage for midgame and exact pairs."""
    return {x: np.quantile(np.abs(z[exact == x]), STAGES) for x in (0, 1)}


def nll(z, dev):
    return float(np.mean(np.log(dev) + 0.5 * z * z + 0.5 * math.log(2 * math.pi)))


def report(name, train, test, mean_dev):
    mtr, dtr = mean_dev(train)
    mte, dte = mean_dev(test)
    ztr = (train[:, 5] - mtr) / dtr
    zte = (test[:, 5] - mte) / dte
    t = thresholds(ztr, train[:, 0])
    out = {"name": name}
    for x, label in ((0, "mid"), (1, "exact")):
        m = test[:, 0] == x
        out[f"nll_{label}"] = nll(zte[m], dte[m])
        tt = t[x]
        beyond = [(np.abs(zte[m]) > tt[i]).mean() for i in range(6)]
        out[f"beyond_{label}"] = beyond
        # Per-cell spread: how far each (empties, depth) cell's rate at 85%
        # and 98% is from the target, relative to it (RMS over cells).
        keys, inv = cells(test[m], None)
        spread = []
        for i in (2, 4):
            hit = (np.abs(zte[m]) > tt[i]).astype(float)
            rate = np.bincount(inv, hit) / np.bincount(inv)
            target = 1 - STAGES[i]
            spread.append(math.sqrt(np.mean(((rate - target) / target) ** 2)))
        out[f"spread_{label}"] = spread
    return out


def print_rows(title, rows, keys):
    print(f"\n## {title}\n")
    for r in rows:
        line = f"{r['name']:<34}"
        for k in keys:
            v = r[k]
            line += "  " + (" ".join(f"{x:.3f}" for x in v) if isinstance(v, list) else f"{v:.4f}")
        print(line)


def main():
    E, I, X, V = load(sys.argv[1])
    tr, te = I % 2 == 0, I % 2 == 1
    train_all = pairs(E[tr], X[tr], V[tr], every=True)
    train = train_all[engine_pair(train_all)]
    test = pairs(E[te], X[te], V[te])
    print(f"train engine pairs {len(train)} (all pairs {len(train_all)}), test {len(test)}")

    def models(train, train_all):
        shared = SharedMean(train)
        r = train[:, 5] - shared.predict(train)
        r_all = train_all[:, 5] - shared.predict(train_all)
        ref = Reference(train)
        b = CellFit(train, r, edax_form, [0.05, 0.02, -0.05, 0.0, 1.0, 2.0])
        c = CellFit(train, r, egaroucid_form, [0.7, -4.5, 1.1, -0.8, 4.6, 3.8, 1.8], 10.0)
        d = LogLinear(train, r)
        e = GModel(train, r)
        e_all = GModel(train_all, r_all)
        with_mean = lambda dev: (lambda S: (shared.predict(S), dev(S)))
        return [
            ("A today (cells + linear + table)", ref.predict),
            ("B Edax form (quadratic)", with_mean(b.deviation)),
            ("C Egaroucid form (cubic, exact x10)", with_mean(c.deviation)),
            ("D log-linear", with_mean(d.deviation)),
            ("E G model (engine pairs)", with_mean(e.deviation)),
            ("E' G model (all pairs)", with_mean(e_all.deviation)),
        ], e_all

    built, e_all = models(train, train_all)
    rows = [report(n, train, test, f) for n, f in built]
    print("\nStages 68 77 85 95 98 99%; wanted beyond rates 0.320 0.230 0.150 0.050 0.020 0.010")
    print_rows("Held-out: Gaussian NLL (mid, exact)", rows, ["nll_mid", "nll_exact"])
    print_rows("Held-out: beyond measured t (mid)", rows, ["beyond_mid"])
    print_rows("Held-out: beyond measured t (exact)", rows, ["beyond_exact"])
    print_rows("Held-out: per-cell RMS relative error of the rate at 85%, 98% (mid | exact)", rows, ["spread_mid", "spread_exact"])

    # Extrapolation 1: exact rules fitted on 12..20 empties, tested on 21..24.
    keep = lambda S: (S[:, 0] == 0) | (S[:, 1] <= 20)
    sub_all = train_all[keep(train_all)]
    built, _ = models(train[keep(train)], sub_all)
    far = test[(test[:, 0] == 0) | (test[:, 1] >= 21)]
    rows = [report(n, train[keep(train)], far, f) for n, f in built]
    print_rows("Extrapolation: exact fitted on 12..20, tested on 21..24 (beyond t, exact)", rows, ["beyond_exact", "nll_exact"])

    # Extrapolation 2: midgame depths fitted up to 11, tested on 12..14.
    keep = lambda S: (S[:, 0] == 1) | (S[:, 2] <= 11)
    built, _ = models(train[keep(train)], train_all[keep(train_all)])
    deep = test[(test[:, 0] == 1) | (test[:, 2] >= 12)]
    rows = [report(n, train[keep(train)], deep, f) for n, f in built]
    print_rows("Extrapolation: midgame fitted on depth <= 11, tested on 12..14 (beyond t, mid)", rows, ["beyond_mid", "nll_mid"])

    if len(sys.argv) > 2:
        E2, I2, X2, V2 = load(sys.argv[2])
        far = pairs(E2, X2, V2)
        far = far[far[:, 0] == 1]
        built, _ = models(train, train_all)
        rows = []
        for n, f in built:
            r = report(n, train, np.concatenate([test[test[:, 0] == 0], far]), f)
            m, d = f(far)
            r["far_mean"] = float(np.mean(far[:, 5] - m))
            r["far_rms_ratio"] = float(np.sqrt(np.mean(((far[:, 5] - m) / d) ** 2)))
            rows.append(r)
        print_rows(f"Beyond the data: exact at 25..28 empties ({len(far)} positions), beyond t, mean error, RMS(z)", rows, ["beyond_exact", "far_mean", "far_rms_ratio"])
        for e in range(25, 29):
            rows = []
            fe = far[far[:, 1] == e]
            for n, f in built:
                m, d = f(fe)
                rows.append({"name": n, "dev": float(np.mean(d)), "sd": float(np.std(fe[:, 5] - m)), "mean": float(np.mean(fe[:, 5] - m))})
            print_rows(f"exact {e}: model deviation, measured sd of the error, mean error", rows, ["dev", "sd", "mean"])

    print(f"\nG model (all pairs) parameters: {np.round(e_all.p, 4).tolist()}")


main()
