"""Diagnostics behind the formula comparison (see README, 考察).
usage: python3 why.py positions.tsv exact25-28-positions.tsv"""
import sys
import numpy as np
exec(open("compare_formulas.py").read().split("def main():")[0])
E1, I1, X1, V1 = load(sys.argv[1])
E2, I2, X2, V2 = load(sys.argv[2])
E = np.concatenate([E1, E2]); X = np.concatenate([X1, X2]); V = np.concatenate([V1, V2])
src = np.concatenate([np.zeros(len(E1)), np.ones(len(E2))])

print("== 1. Bias of a depth-k value: mean(exact - v_k), by empties e (rows) and k (cols)")
print("    e   n  " + " ".join(f"{k:>5d}" for k in range(15)))
for e in range(12, 29):
    m = (E == e) & ~np.isnan(X)
    print(f"  {e:3d} {m.sum():4d}  " + " ".join(f"{np.mean(X[m] - V[m, k]):+5.2f}" if k < e - 1 else "    -" for k in range(15)))

print("\n== 2. Same bias by the leaf's empties L = e - k (k = 4, 5, 6)")
for k in (4, 5, 6):
    row = []
    for e in range(12, 29):
        m = (E == e) & ~np.isnan(X)
        row.append(f"L{e - k}:{np.mean(X[m] - V[m, k]):+.2f}")
    print(f"  k={k}: " + "  ".join(row))

print("\n== 3. Midgame check with 1000 positions each: mean(v14 - v_s) and its SE")
for s in (5, 6):
    out = []
    for e in range(19, 33):
        m = (E1 == e)
        dlt = V1[m, 14] - V1[m, s]
        out.append(f"{e}:{dlt.mean():+.2f}")
    print(f"  s={s}: " + " ".join(out) + f"   (SE ~ {np.std(V1[E1 == 25, 14] - V1[E1 == 25, s]) / np.sqrt(1000):.2f})")

print("\n== 4. Additivity ratio Var(v_d - v_s) / (G(s) - G(d)) pooled over pairs, by e")
for e in range(12, 29):
    m = (E == e) & ~np.isnan(X)
    rs = []
    for s in range(0, 12):
        for d in range(s + 2, min(15, e - 1), 2):
            g = np.var(X[m] - V[m, s]) - np.var(X[m] - V[m, d])
            if g > 2:
                rs.append(np.var(V[m, d] - V[m, s]) / g)
    print(f"  e={e:2d} n={m.sum():4d} median ratio {np.median(rs):.3f}  ({len(rs)} pairs)")

print("\n== 5. Exact sd of (exact - v_s) at the engine shallow depth, and at a fixed depth 6 and 8")
for e in range(12, 29):
    m = (E == e) & ~np.isnan(X)
    s = exact_shallow(e)
    print(f"  e={e:2d} s={s}  sd(s) {np.std(X[m] - V[m, s]):.2f}  sd(6) {np.std(X[m] - V[m, 6]):.2f}  sd(8) {np.std(X[m] - V[m, 8]):.2f}")

print("\n== 6. Spread of the scores themselves: sd(exact), sd(v14), by e")
for e in (12, 16, 20, 24, 26, 28):
    m = (E == e) & ~np.isnan(X)
    print(f"  e={e}: sd(exact) {np.std(X[m]):.1f}")
for e in (20, 30, 40, 50):
    m = E1 == e
    print(f"  e={e}: sd(v14) {np.std(V1[m, 14]):.1f}  sd(v0) {np.std(V1[m, 0]):.1f}")

print("\n== 7. Midgame error by parity of the shallow depth (deviation of v_d - v_s, d=s+6)")
for e in (24, 32, 40, 48):
    m = E1 == e
    print(f"  e={e}: " + "  ".join(f"s={s}:{np.std(V1[m, s + 6] - V1[m, s]):.2f}" for s in range(0, 8)))

print("\n== 8. Is the 120-position sample at 25..28 typical? mean(v14 - v_s) in those 120 vs all 1000,")
print("      and the split exact - v_s = (exact - v14)[120] + (v14 - v_s)[1000]")
for e in range(23, 29):
    m1 = E1 == e
    first = m1 & (I1 < 120)
    s = exact_shallow(e)
    a120 = np.mean(V1[first, 14] - V1[first, s]); a1000 = np.mean(V1[m1, 14] - V1[m1, s])
    m = (E == e) & ~np.isnan(X)
    if e <= 24:
        m120 = m & (src == 0) & (np.concatenate([I1, I2]) < 120)
    else:
        m120 = m
    ex14 = np.mean(X[m120] - V[m120, 14]); se14 = np.std(X[m120] - V[m120, 14]) / np.sqrt(m120.sum())
    direct = np.mean(X[m120] - V[m120, s])
    print(f"  e={e} s={s}: (v14-v_s) first120 {a120:+.2f} all1000 {a1000:+.2f} | (exact-v14)[120] {ex14:+.2f}±{se14:.2f} | exact-v_s: 120 only {direct:+.2f}, combined {ex14 + a1000:+.2f}")
    if e <= 24:
        full = np.mean(X[m] - V[m, s])
        print(f"           (1000 exact available: exact - v_s over 1000 = {full:+.2f})")

print("\n== 9. Midgame error vs the spread of scores: sd(v10 - v2) and sd(v14), by e")
for e in range(16, 53, 4):
    m = E1 == e
    err = np.std(V1[m, 10] - V1[m, 2]); spread = np.std(V1[m, 14])
    print(f"  e={e}: sd(v10-v2) {err:.2f}  sd(v14) {spread:5.1f}  ratio {err / spread:.3f}")

print("\n== 10. Midgame error by |score|: sd(v10 - v2) in bins of |v14| (e = 30..40)")
m = (E1 >= 30) & (E1 <= 40)
a = np.abs(V1[m, 14]); dlt = V1[m, 10] - V1[m, 2]
for lo, hi in ((0, 4), (4, 10), (10, 20), (20, 40), (40, 65)):
    k = (a >= lo) & (a < hi)
    print(f"  |v14| {lo:2d}-{hi:2d}: n {k.sum():5d}  sd {np.std(dlt[k]):.2f}")

print("\n== 11. Do lopsided positions make the heavy tails? Midgame engine pairs, residual / cell sd,")
print("       then also divided by a factor per |shallow value| bin (fitted on even positions, applied to odd)")
S = pairs(E1, X1, V1)
S = S[S[:, 0] == 0]
par = np.concatenate([I1[E1 == 0]]) if False else None
keys, inv = cells(S, None)
z = np.empty(len(S))
for k in range(len(keys)):
    m = inv == k
    x, y = S[m, 4], S[m, 5]
    a = np.cov(x, y, bias=True)[0, 1] / np.var(x); b = y.mean() - a * x.mean()
    r = y - a * x - b
    z[m] = r / r.std()
av = np.abs(S[:, 4])
bins = [0, 4, 10, 20, 30, 40, 50, 65]
which = np.digitize(av, bins) - 1
rng = np.random.default_rng(1)
half = rng.random(len(S)) < 0.5
factor = np.array([np.std(z[half & (which == i)]) for i in range(len(bins) - 1)])
z2 = z / factor[which]
def summary(name, zz):
    zz = zz[~half]
    k = np.mean(zz ** 4) / np.mean(zz ** 2) ** 2
    zz = zz / zz.std()
    print(f"  {name}: kurtosis {k:.2f} (normal 3)  beyond 1.0: {np.mean(abs(zz) > 1):.3f} (0.317)  1.96: {np.mean(abs(zz) > 1.96):.4f} (0.050)  2.576: {np.mean(abs(zz) > 2.576):.4f} (0.010)")
print("  factor per |v| bin " + " ".join(f"{bins[i]}-{bins[i+1]}:{factor[i]:.2f}" for i in range(len(factor))))
summary("cell sd only       ", z)
summary("cell sd x |v| factor", z2)
