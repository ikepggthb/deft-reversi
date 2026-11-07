#!/bin/bash
# usage: bench.sh NAME [ENV=VALUE ...]  (one config; CPU 2, one at a time)
# Midgame: mid150 at levels 21 and 18; endgame: FFO 40-59 exact. PEN model.
set -e
cd "$(dirname "$0")"
REPO=$(cd ../../.. && pwd)
SOLVE=${SOLVE:-$REPO/target-native/release/examples/solve}
MODEL=${MODEL:?set MODEL}
MID=$REPO/docs/benchmarks/2026-11-03-midgame-speed/mid150.obf
FFO=$REPO/docs/benchmarks/2026-11-06-pen-tuning/ffo40-59.obf
name=$1; shift
for l in ${LEVELS:-21 18}; do
  env "$@" taskset -c 2 $SOLVE $MODEL $MID level$l 1 256 > results/$name-l$l.tsv
done
if [ -z "$NO_FFO" ]; then env "$@" taskset -c 2 $SOLVE $MODEL $FFO exact 1 256 > results/$name-ffo.tsv; fi
python3 summarize.py $name
