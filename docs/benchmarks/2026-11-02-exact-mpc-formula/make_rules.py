#!/usr/bin/env python3
"""Exact ProbCut rules from the formula, for any shallow-depth table.

usage: python3 make_rules.py TABLE BASE_RULES.json OUT.json [positions-exact.tsv]

TABLE gives the shallow depth per empties count as `FROM-TO:EXPR;...`
over the current table, EXPR in e (empties) and c (current depth), e.g.
`cur`, `12-60:c+2`, `12-60:((e//3)&~1)+(e&1)`. Depths are capped at 14
(the measured range) and at most e - 4 (keeping their parity), and at
least 0; `FROM-TO:N` sets a depth directly.

Midgame rules and multiples come from BASE_RULES.json unchanged. The exact
rules come from the formula of fit_formula.py fitted on every cell, with
a and b evaluated at min(e, 24) (no data beyond 24), and the exact
multiples are the pooled quantiles of |z| over every cell.
"""
import json
import sys

import numpy as np

from fit_formula import HERE, MEAN_FORM, SIGMA_FORMS, STAGES, load, make_cells

E_MEASURED = 24


def current(e):
    if e in (12, 14, 16):
        return 4
    if e in (13, 15):
        return 3
    if e in (17, 19):
        return 5
    if e in (18, 20):
        return 6
    if e >= 29:
        return 8 - e % 2
    return 6 - e % 2


def capped(e, s):
    while s > 14 or s > e - 4:
        s -= 2
    return max(s, 0)


def table(spec):
    t = {e: current(e) for e in range(12, 61)}
    if spec != "cur":
        for part in spec.split(";"):
            span, expr = part.split(":")
            lo, hi = (span.split("-") + [span])[:2]
            for e in range(int(lo), int(hi) + 1):
                t[e] = capped(e, int(eval(expr, {"e": e, "c": current(e)})))
    return t


def main():
    spec, base, out = sys.argv[1:4]
    path = sys.argv[4] if len(sys.argv) > 4 else HERE / "positions-exact.tsv"
    cells = make_cells(load(path))
    sigma_form = SIGMA_FORMS[next(iter(SIGMA_FORMS))]
    fits = {}
    for par in (0, 1):
        tr = [c for c in cells if c["s"] % 2 == par]
        fits[par] = (MEAN_FORM(tr, "a"), MEAN_FORM(tr, "b"), sigma_form(tr, "d"))
    z = []
    for c in cells:
        fa, fb, fd = fits[c["s"] % 2]
        z.append(np.abs(c["y"] - fa(c["e"], c["s"]) * c["x"] - fb(c["e"], c["s"])) / fd(c["e"], c["s"]))
    z = np.concatenate(z)
    multiples = [float(np.quantile(z, p)) for p in STAGES]
    for i in range(1, 6):
        multiples[i] = max(multiples[i], multiples[i - 1])
    calibration = json.load(open(base))
    rules = [r for r in calibration["rules"] if not r["exact"]]
    t = table(spec)
    for e in range(12, 61):
        s = t[e]
        fa, fb, fd = fits[s % 2]
        em = min(e, E_MEASURED)
        rules.append(dict(exact=True, depth=e, empties=e, shallow_depth=s,
                          slope=fa(em, s), intercept=fb(em, s), deviation=fd(e, s)))
    calibration["rules"] = rules
    calibration["sigmas"]["exact"] = multiples
    json.dump(calibration, open(out, "w"), indent=1)
    print(" ".join(f"{e}:{t[e]}" for e in range(12, 33)), "t=", [round(v, 3) for v in multiples])


if __name__ == "__main__":
    main()
