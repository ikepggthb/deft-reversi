"""Print every model's fitted coefficients and a worked example.
usage: python3 coefficients.py positions.tsv exact25-28-positions.tsv"""
import sys
import numpy as np
exec(open("compare_formulas.py").read().split("def main():")[0])
E, I, X, V = load(sys.argv[1])
tr = I % 2 == 0
train_all = pairs(E[tr], X[tr], V[tr], every=True)
train = train_all[engine_pair(train_all)]
np.set_printoptions(precision=4, suppress=True)

shared = SharedMean(train)
r = train[:, 5] - shared.predict(train)
names = ["1", "e/60", "d/60", "s/60", "odd(s)", "x", "x*odd", "x*e/60", "x*(e/60)^2"]
print("== shared mean: deep = a*shallow + b,  a = sum(alpha_i f_i), b = sum(beta_i f_i)")
for n, a, b in zip(names, shared.beta[:9], shared.beta[9:]):
    print(f"  {n:<12} alpha {a:+.4f}   beta {b:+.4f}")

ref = Reference(train)
print("\n== A midgame regressions on [1, e, d, s]")
for n, c in zip(("slope", "intercept", "deviation"), ref.coef):
    print(f"  {n:<9} {c}")
print(f"  deviation clamp [{ref.lo:.3f}, {ref.hi:.3f}]")
print("== A exact table (slope, intercept, deviation)")
for e in sorted(ref.ex):
    a, b, d = ref.ex[e]
    print(f"  e={e}: {a:.4f} {b:+.3f} {d:.3f}")

b_ = CellFit(train, r, edax_form, [0.05, 0.02, -0.05, 0.0, 1.0, 2.0])
c_ = CellFit(train, r, egaroucid_form, [0.7, -4.5, 1.1, -0.8, 4.6, 3.8, 1.8], 10.0)
d_ = LogLinear(train, r)
e_ = GModel(train, r)
print("\n== B Edax form: u = p0*e + p1*d + p2*s;  sigma = p3*u^2 + p4*u + p5")
print("  p =", b_.p)
print("== C Egaroucid form: u = p0*(64-e)/64 + p1*s/60 + p2*d/60;  sigma = p3*u^3 + p4*u^2 + p5*u + p6  (exact cells weight 10)")
print("  p =", c_.p)
print("== D log-linear: log sigma = sum g_i f_i, f = [1, e', e'^2, s', s'^2, log(d-s), odd(s), odd(s)*s', x, x*e', x*s']  (e'=e/60, s'=s/60)")
print("  g =", d_.g)
print("== E G model: r = e-k-1, logA = q0 + q1*k/10 + q2*e' + q3*e'^2 + q4*(k/10)*e' + q5*odd(k), lambda = exp(q6 + q7*k/10)")
print("   G(e,k) = exp(logA) * (1 - exp(-r/lambda)),  sigma^2 = G(e,s) - G(e,d)")
print("  q =", e_.p)

def show(label, S):
    S = np.array([S], float)
    m_ref, d_ref = ref.predict(S)
    m_sh = shared.predict(S)
    print(f"\n== worked example: {label}  (shallow value v = {S[0,4]:+.0f})")
    print(f"  A: mean {m_ref[0]:+.3f}  sigma {d_ref[0]:.3f}")
    for k, f in (("B", b_.deviation), ("C", c_.deviation), ("D", d_.deviation), ("E", e_.deviation)):
        print(f"  {k}: mean {m_sh[0]:+.3f}  sigma {f(S)[0]:.3f}")
    if label.startswith("exact"):
        e, s = int(S[0, 1]), int(S[0, 3])
        q = e_.p
        for k in (s, e):
            rr = max(e - k - 1, 0); ee = e / 60
            loga = q[0] + q[1] * k / 10 + q[2] * ee + q[3] * ee * ee + q[4] * k / 10 * ee + q[5] * (k % 2)
            lam = np.exp(q[6] + q[7] * k / 10)
            print(f"    E detail: k={k} r={rr} A=exp({loga:.4f})={np.exp(loga):.3f} lambda={lam:.3f} G={np.exp(loga) * (1 - np.exp(-rr / lam)):.3f}")

show("midgame e=30 d=10 s=2", [0, 30, 10, 2, 8, 0])
show("exact e=26 s=6", [1, 26, 26, 6, 8, 0])
