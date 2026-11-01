#!/bin/sh
# Normal vs measured multiples with the same 1000-position rules.
set -e
M=/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/e8.dpr2
B=../../../tools/bench
python3 $B/compare_versions.py --before ./solve-normal.sh --after ./solve-measured.sh \
  --model $M --positions $B/positions/ffo40-49.obf --out ffo.tsv --rounds 6 --cpu 2   # also writes ffo.json
S=tools/target/release/selfplay
$S $M@rules.json $M@rules.json:normal 11 1000 8 4 1 selfplay-level11.tsv | tee selfplay-level11.txt
$S $M@rules.json $M@rules.json:normal 14 500 8 4 1 selfplay-level14.tsv | tee selfplay-level14.txt
