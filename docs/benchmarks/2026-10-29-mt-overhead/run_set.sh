#!/bin/sh
# usage: run_set.sh RUNNAME ROUNDS "THREADS:CPUSET ..." NAME ... ; binaries are bench-engines/bin/deft-NAME
set -e
run=$1; rounds=$2; sets=$3; shift 3
B=/home/azure/dev/bench-engines; R=/home/azure/dev/deft-reversi-rewrite
vs=""; for n in "$@"; do vs="$vs $n=$B/bin/deft-$n"; done
for s in $sets; do
  t=${s%%:*}; c=${s#*:}
  python3 $R/docs/benchmarks/2026-10-29-mt-overhead/compare_many_threads.py $vs --model $B/model/deft-eval-main.dpr2 \
    --positions $R/tools/bench/positions/ffo40-49.obf --answers $R/tools/bench/positions/ffo40-49-answers.obf \
    --out $R/docs/benchmarks/2026-10-29-mt-overhead/runs/$run-t$t --rounds $rounds --threads $t --cpuset $c
done
echo exit=0
