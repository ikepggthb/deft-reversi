"""Check the G model's assumption on exactly solved positions:
Var(v_d - v_s) = G(e, s) - G(e, d), where G(e, k) = Var(exact - v_k).
The difference is 2 Cov(exact - v_d, v_d - v_s); zero if the deeper error
is independent of the shallow-to-deep change. Means are removed per cell."""
import sys
import numpy as np
sys.argv = sys.argv[:2]
exec(open("compare_formulas.py").read().split("def main():")[0])
E, I, X, V = load(sys.argv[1])
print("empties  s  d   Var(vd-vs)  G(s)-G(d)  ratio   G(s)    G(d)")
for e in (14, 18, 20, 22, 24):
    m = (E == e) & ~np.isnan(X)
    for s, d in ((2, 8), (4, 10), (3, 9), (6, 12), (4, 12)):
        if d >= e:
            continue
        a = V[m, d] - V[m, s]
        gs = np.var(X[m] - V[m, s]); gd = np.var(X[m] - V[m, d])
        print(f"{e:7d} {s:2d} {d:2d}   {np.var(a):9.2f}  {gs - gd:9.2f}  {np.var(a) / (gs - gd):5.2f}  {gs:6.2f}  {gd:6.2f}")
