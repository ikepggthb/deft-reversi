#!/usr/bin/env python3
"""Per-iteration node deltas from DIAG lines: 1 thread vs median of 8-thread runs."""
import re, statistics, sys
from pathlib import Path

def load(path):
    its = []
    for line in path.read_text().splitlines():
        if not line.startswith('DIAG'):
            continue
        kv = dict(re.findall(r'(\w+)=(\[[^\]]*\]|\S+)', line))
        its.append({k: (eval(v) if v.startswith('[') else v) for k, v in kv.items()})
    return its
