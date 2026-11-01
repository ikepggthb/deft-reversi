"""Paired differences of the engine-level efficiency between models at the
same false-cut calibration (t: fc), with standard errors over positions.
usage: python3 paired.py positions.tsv exact25-28-positions.tsv"""
import sys
import numpy as np
src = open(__import__("os").path.dirname(__file__) + "/compare_models.py").read().split("\ndef main():")[0]
exec(src.replace('HERE = __import__("os").path.dirname(__file__) or "."', 'HERE = "' + (__import__("os").path.dirname(__file__) or ".") + '"'))


def per_sample(model, T, p, x):
    a, b = model.mean.ab(T)
    sg = model.sigma(T)
    v, deep = T[:, 4], T[:, 5]
    tu, tl = model.thresholds("fc", p, x)
    eff = np.zeros(len(T)); fc = np.zeros(len(T))
    for k in range(1, 9):
        eff += (v >= np.ceil((deep - k - b + tu * sg) / a)).astype(float) + (v <= np.floor((deep + k - b - tl * sg) / a))
    fc = (v >= np.ceil((deep + 1 - b + tu * sg) / a)).astype(float) + (v <= np.floor((deep - 1 - b - tl * sg) / a))
    return eff / 16, fc / 2


E1, I1, X1, V1 = load(sys.argv[1]); K1 = load_boards(sys.argv[1])
E2, I2, X2, V2 = load(sys.argv[2]); K2 = load_boards(sys.argv[2])
held = set(K2)
test_pos = np.array([in_test(k) for k in K1]); blocked = np.array([k in held for k in K1])
trp = ~test_pos & ~blocked
train = pairs(E1[trp], X1[trp], V1[trp])
# keep the position id of every test pair, to resample positions
ids = np.flatnonzero(test_pos)
test_parts, pid = [], []
for i in ids:
    P = pairs(E1[i:i + 1], X1[i:i + 1], V1[i:i + 1])
    test_parts.append(P); pid.append(np.full(len(P), i))
test = np.concatenate(test_parts); pid = np.concatenate(pid)
far = pairs(E2, X2, V2); far = far[far[:, 0] == 1]
models = {k: Model(*k, train) for k in [("none", "table"), ("parity", "table"), ("cell", "table"), ("cell", "loglin"), ("cell", "quad"), ("cell", "G")]}
for x, name, T, P in ((0, "midgame", test[test[:, 0] == 0], pid[test[:, 0] == 0]),
                      (1, "exact 12..24", test[test[:, 0] == 1], pid[test[:, 0] == 1]),
                      (1, "exact 25..28", far, np.arange(len(far)))):
    for p in (0.85, 0.98):
        base_eff, base_fc = per_sample(models[("cell", "table")], T, p, x)
        print(f"{name} {int(p * 100)}%: cell/table eff {base_eff.mean():.4f} false {base_fc.mean():.4f}")
        uniq, inv = np.unique(P, return_inverse=True)
        for k, m in models.items():
            if k == ("cell", "table"):
                continue
            e, f = per_sample(m, T, p, x)
            d = np.bincount(inv, e - base_eff)  # per-position sums
            df = np.bincount(inv, f - base_fc)
            n = len(T); np_ = len(uniq)
            se = np.sqrt(np_ * np.var(d)) / n
            sef = np.sqrt(np_ * np.var(df)) / n
            print(f"   {k[0]}/{k[1]} - cell/table: eff {(e - base_eff).mean():+.4f} ± {se:.4f}   false {(f - base_fc).mean():+.4f} ± {sef:.4f}")
