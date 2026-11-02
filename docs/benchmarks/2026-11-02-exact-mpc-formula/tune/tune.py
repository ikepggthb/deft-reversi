#!/usr/bin/env python3
"""Helpers for per_empties.py and ffo_tables.py: build a table's rules with
../make_rules.py, pack them into a model, solve a positions file on CPU 2
(one thread, 256 MiB TT) and compare per-position times."""
import csv
import subprocess
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
TRAIN = "/home/azure/dev/deft-reversi-rewrite/target-native/release/deft-reversi-train"


def build_spec(spec, model, base, out):
    name = spec.replace(";", "_").replace(":", "").replace("+", "p").replace("-", "m") or "cur"
    rules, packed = out / f"{name}.json", out / f"{name}.dpr2"
    if not packed.exists():
        subprocess.run([sys.executable, str(HERE.parent / "make_rules.py"), spec, base, str(rules)],
                       check=True, cwd=HERE.parent, stdout=subprocess.DEVNULL)
        subprocess.run([TRAIN, "pack", "--model", model, "--rules", str(rules), "--out", str(packed)],
                       check=True, stdout=subprocess.DEVNULL)
    return name, packed


def solve(solver, packed, roots, out, name):
    tsv = out / f"{name}.{Path(roots).stem}.tsv"
    if not tsv.exists():
        result = subprocess.run(["taskset", "-c", "2", solver, str(packed), str(HERE / roots), "exact", "1", "256"],
                                check=True, capture_output=True, text=True)
        tsv.write_text(result.stdout)
    rows = list(csv.DictReader(open(tsv), delimiter="\t"))
    return np.array([float(r["seconds"]) for r in rows]), [r["score"] for r in rows]


def compare(t, base, rng):
    r = np.log(t / base)
    boot = [np.exp(rng.choice(r, len(r)).mean()) for _ in range(4000)]
    return np.exp(r.mean()), np.percentile(boot, 2.5), np.percentile(boot, 97.5)
