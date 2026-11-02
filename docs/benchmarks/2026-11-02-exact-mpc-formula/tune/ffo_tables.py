#!/usr/bin/env python3
"""Times exact solves of FFO 40-59 for shallow-depth tables, one table
after another on CPU 2, and checks every score against the answers.
usage: python3 ffo_tables.py SOLVE MODEL BASE_RULES.json OUT_DIR TABLE...
(TABLE in ../make_rules.py syntax)"""
import re, sys
from pathlib import Path
import tune

def main():
    solver, model, base, out, *tables = sys.argv[1:]
    out = Path(out); out.mkdir(exist_ok=True)
    answers = [max(map(int, re.findall(r":\s*([+-]\d+)", l))) for l in open(tune.HERE / "ffo40-59-answers.obf") if l.strip()]
    log = open(out / "ffo-log.txt", "a")
    for spec in tables:
        name, packed = tune.build_spec(spec, model, base, out)
        t, scores = tune.solve(solver, packed, "ffo40-59.obf", out, name)
        assert [int(s) for s in scores] == answers, f"{spec}: wrong score"
        line = f"{spec:40s} total {t.sum():8.2f} s  " + " ".join(f"{x:.2f}" for x in t)
        print(line, flush=True); log.write(line + "\n"); log.flush()

if __name__ == "__main__":
    main()
