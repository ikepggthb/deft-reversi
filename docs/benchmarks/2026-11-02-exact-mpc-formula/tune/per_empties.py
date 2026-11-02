#!/usr/bin/env python3
"""Shallow depth per empties count, one count at a time.

usage: python3 per_empties.py SOLVE MODEL BASE_RULES.json OUT_DIR ROOTS FIRST LAST DEPTHS BASE_TABLE

For each empties count FIRST..LAST (others fixed at BASE_TABLE, given as
`e:s,e:s,...`), every depth in DEPTHS (`0,1,2,3,4`) is timed on ROOTS;
the fastest depth by the geometric mean of per-position times is taken
when its 95% interval against the base depth is below 1, and fixed before
the next count. Appends to OUT_DIR/log.txt."""
import sys
from pathlib import Path
import numpy as np
import tune

def main():
    solver, model, base, out, roots, first, last, depths, start = sys.argv[1:10]
    out = Path(out); out.mkdir(exist_ok=True)
    table = {int(k): int(v) for k, v in (p.split(":") for p in start.split(","))}
    depths = [int(d) for d in depths.split(",")]
    log = open(out / "log.txt", "a")
    say = lambda s: (print(s, flush=True), log.write(s + "\n"), log.flush())
    rng = np.random.default_rng(0)
    spec = lambda t: ";".join(f"{e}-{e}:{s}" for e, s in sorted(t.items()))
    def run(t):
        name, packed = tune.build_spec(spec(t), model, base, out)
        return tune.solve(solver, packed, roots, out, name)
    for e in range(int(first), int(last) + 1):
        say(f"\n## empties {e} (others {spec({k: v for k, v in table.items() if k != e})}), roots {roots}")
        base_s = table[e]
        res = {}
        for s in depths:
            if s > e - 4:
                continue
            res[s] = run({**table, e: s})
        t0, sc0 = res[base_s]
        stats = {}
        for s, (t, sc) in sorted(res.items()):
            assert sc == sc0, f"depth {s} changed an exact score"
            g, lo, hi = tune.compare(t, t0, rng) if s != base_s else (1.0, 1.0, 1.0)
            stats[s] = (g, lo, hi)
            say(f"  s={s}: total {t.sum():8.2f} s  geo {g:.3f} [{lo:.3f}, {hi:.3f}]")
        winners = [s for s, (g, lo, hi) in stats.items() if s != base_s and hi < 1]
        best = min(winners, key=lambda s: stats[s][0]) if winners else base_s
        table[e] = best
        say(f"  chosen s={best}")
    say("\ntable: " + ",".join(f"{e}:{s}" for e, s in sorted(table.items())))

if __name__ == "__main__":
    main()
