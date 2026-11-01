"""Export the data behind the explainer figures as JSON.
usage: python3 export_figures.py positions.tsv exact25-28-positions.tsv > figures.json"""
import json, sys
import numpy as np
src = open("compare_formulas.py").read().split("def main():")[0]
exec(src)

E, I, X, V = load(sys.argv[1])
E2, I2, X2, V2 = load(sys.argv[2])
tr = I % 2 == 0
train_all = pairs(E[tr], X[tr], V[tr], every=True)
train = train_all[engine_pair(train_all)]
shared = SharedMean(train)
r = train[:, 5] - shared.predict(train)
r_all = train_all[:, 5] - shared.predict(train_all)
ref = Reference(train)
models = {
    "A": lambda S: ref.predict(S)[1],
    "B": CellFit(train, r, edax_form, [0.05, 0.02, -0.05, 0.0, 1.0, 2.0]).deviation,
    "C": CellFit(train, r, egaroucid_form, [0.7, -4.5, 1.1, -0.8, 4.6, 3.8, 1.8], 10.0).deviation,
    "D": LogLinear(train, r).deviation,
    "E": GModel(train, r).deviation,
}

def cell_sd(x, y):
    a = np.cov(x, y, bias=True)[0, 1] / max(np.var(x), 1e-12)
    b = y.mean() - a * x.mean()
    return float(np.sqrt(np.sum((y - a * x - b) ** 2) / (len(x) - 2))), float(a), float(b)

out = {}
# Midgame: deviation by depth at four empties counts.
mid = {}
for e in (20, 30, 40, 50):
    m = E == e
    rows = {"depth": [], "measured": []}
    for d in range(4, 15):
        if np.isnan(V[m, d]).any():
            continue
        s = SHALLOW[d]
        rows["depth"].append(d)
        rows["measured"].append(cell_sd(V[m, s], V[m, d])[0])
    S = np.array([[0, e, d, SHALLOW[d], 0, 0] for d in rows["depth"]], float)
    for k, f in models.items():
        rows[k] = [float(v) for v in f(S)]
    mid[e] = rows
out["mid"] = mid
# Exact: deviation and intercept by empties, 12..28.
ex = {"empties": [], "measured": [], "intercept": [], "n": []}
for EE, XX, VV in ((E, X, V), (E2, X2, V2)):
    for e in sorted(set(EE[~np.isnan(XX)])):
        m = (EE == e) & ~np.isnan(XX)
        s = exact_shallow(int(e))
        sd, a, b = cell_sd(VV[m, s], XX[m])
        ex["empties"].append(int(e)); ex["measured"].append(sd); ex["intercept"].append(b); ex["n"].append(int(m.sum()))
S = np.array([[1, e, e, exact_shallow(e), 0, 0] for e in ex["empties"]], float)
for k, f in models.items():
    ex[k] = [float(v) for v in f(S)]
out["exact"] = ex
# G table and additivity points.
g = {}
for e in range(12, 25):
    m = (E == e) & ~np.isnan(X)
    g[e] = [float(np.var(X[m] - V[m, k])) if k < e else None for k in range(15)]
out["G"] = g
pts = []
for e in range(14, 25, 2):
    m = (E == e) & ~np.isnan(X)
    for s in range(0, 13):
        for d in range(s + 2, min(15, e - 1), 2):
            a = float(np.var(V[m, d] - V[m, s]))
            b = float(np.var(X[m] - V[m, s]) - np.var(X[m] - V[m, d]))
            pts.append([e, s, d, round(a, 2), round(b, 2)])
out["additivity"] = pts
json.dump(out, sys.stdout)
