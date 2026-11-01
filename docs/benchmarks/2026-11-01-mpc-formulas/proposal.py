"""Concrete numbers for the proposed exact rules (README, 提案の式).
usage: python3 proposal.py positions.tsv exact25-28-positions.tsv

Exact rule for e empties with shallow depth s:
  predicted = a * v_s + b(e)
  a: one slope for all e (least squares over every exact pair, after removing b per e)
  b(e): the measured value for e <= 24; for e >= 25 the mean of e = 25..28 of the same parity.
  (A linear trend c0 + c1*odd + c2*(e-20) + c3*odd*(e-20) is also fitted and printed, for comparison.)
  sigma(e, s): the measured sd for e < 18; for e >= 18 one value per shallow depth s,
  pooled over the measured e >= 18 (s = 5, 6 from e = 18..28; s = 7, 8 from e = 25..28).
For e = 25..28 the mean of (exact - v_s) is estimated as
  mean(exact - v14) over the 120 solved positions + mean(v14 - v_s) over all 1000,
so the 120 positions' chance offset in the shallow search does not enter."""
import sys
import numpy as np
exec(open("compare_formulas.py").read().split("def main():")[0])
E1, I1, X1, V1 = load(sys.argv[1])
E2, I2, X2, V2 = load(sys.argv[2])

rows = []  # e, s, n, mean(exact - v_s) estimate, mean(v_s) over 1000, sd
for e in range(12, 29):
    s = exact_shallow(e)
    if e <= 24:
        m = (E1 == e) & ~np.isnan(X1)
        d = X1[m] - V1[m, s]
        rows.append((e, s, m.sum(), d.mean(), V1[m, s].mean()))
    else:
        m2 = E2 == e
        m1 = E1 == e
        est = np.mean(X2[m2] - V2[m2, 14]) + np.mean(V1[m1, 14] - V1[m1, s])
        rows.append((e, s, m2.sum(), est, V1[m1, s].mean()))
R = np.array(rows, float)

# Common slope a from all exact pairs with a per-e intercept removed.
xs, ys = [], []
for e in range(12, 29):
    s = exact_shallow(e)
    if e <= 24:
        m = (E1 == e) & ~np.isnan(X1); x, y = V1[m, s], X1[m]
    else:
        m = E2 == e; x, y = V2[m, s], X2[m]
    xs.append(x - x.mean()); ys.append(y - y.mean())
xs, ys = np.concatenate(xs), np.concatenate(ys)
a = float(xs @ ys / (xs @ xs))
print(f"common slope a = {a:.4f}")

# b(e) = mean(exact) - a*mean(v_s) = mean(exact - v_s) - (a - 1)*mean(v_s)
b = R[:, 3] - (a - 1) * R[:, 4]
e = R[:, 0]; odd = e % 2; w = R[:, 2]
F = np.stack([np.ones_like(e), odd, e - 20, odd * (e - 20)], 1)
c = np.linalg.lstsq(F * np.sqrt(w)[:, None], b * np.sqrt(w), rcond=None)[0]
print("b(e) = {:+.3f} {:+.3f}*odd(e) {:+.4f}*(e-20) {:+.4f}*odd(e)*(e-20)".format(*c))
print("  e  measured_b  fitted_b   (n)")
for i, ee in enumerate(e):
    print(f"  {int(ee):2d}   {b[i]:+.2f}      {F[i] @ c:+.2f}   ({int(w[i])})")
for ee in range(29, 33):
    print(f"  {ee:2d}      -        {np.array([1, ee % 2, ee - 20, (ee % 2) * (ee - 20)]) @ c:+.2f}   (extrapolated)")

print("\nmean of b(e) for e = 18..24 and e = 25..28, by parity")
for par in (0, 1):
    k1 = (e >= 18) & (e <= 24) & (odd == par); k2 = (e >= 25) & (odd == par)
    print(f"  {'odd' if par else 'even'}: 18..24 {np.average(b[k1], weights=w[k1]):+.2f}   25..28 {b[k2].mean():+.2f}")

late = {par: b[(e >= 25) & (odd == par)].mean() for par in (0, 1)}
def proposed_b(ee):
    return late[ee % 2] if ee >= 25 else b[list(e).index(ee)]
print(f"\nproposed b for e >= 25: even {late[0]:+.3f}, odd {late[1]:+.3f}")

print("\nsigma by shallow depth for e >= 18 (sd of exact - a*v_s - b(e), pooled)")
for s in (5, 6, 7, 8):
    res = []
    for ee in range(18, 29):
        if ee <= 24:
            if s not in (5, 6) or exact_shallow(ee) != s:
                continue
            m = (E1 == ee) & ~np.isnan(X1); x, y = V1[m, s], X1[m]
        else:
            # The intercept's sign follows the parity of the shallow depth,
            # which the engine keeps equal to the parity of e.
            if ee % 2 != s % 2:
                continue
            m = E2 == ee; x, y = V2[m, s], X2[m]
            if s in (5, 6) and exact_shallow(ee) != s:
                continue
        bb = proposed_b(ee)
        res.append(y - a * x - bb)
    res = np.concatenate(res)
    print(f"  s={s}: sigma {np.sqrt(np.mean(res ** 2)):.3f}  from {len(res)} pairs")
