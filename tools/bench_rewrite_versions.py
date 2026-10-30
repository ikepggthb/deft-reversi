#!/usr/bin/env python3
"""Compatibility entry point; current implementation lives in tools/bench."""
import runpy
from pathlib import Path

if __name__ == '__main__':
    runpy.run_path(str(Path(__file__).parent / 'bench' / 'compare_versions.py'), run_name='__main__')
