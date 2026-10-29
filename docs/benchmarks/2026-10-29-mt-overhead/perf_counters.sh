#!/bin/sh
# usage: perf_counters.sh BINARY FFO THREADS CPUS  -> counters of one exact solve
B=/home/azure/dev/bench-engines
out=$(mktemp); cnt=$(mktemp)
perf stat -x, -e task-clock,cpu_core/cycles/,cpu_core/instructions/,cpu_core/mem_load_l3_hit_retired.xsnp_no_fwd/,cpu_core/machine_clears.memory_ordering/,cpu_core/mem_load_retired.l2_miss/,cpu_core/mem_load_retired.l3_miss/,cpu_core/offcore_requests_outstanding.cycles_with_demand_data_rd/ -o $cnt \
  taskset -c $4 $1 $B/model/deft-eval-main.dpr2 $B/cases/ffo$2.obf exact $3 256 --legacy-main-mpc > $out
printf '%s\tffo%s\tt%s\tnodes=%s\tsearch=%s' "$(basename $1)" $2 $3 "$(awk -F'\t' 'NR==2{print $6}' $out)" "$(awk -F'\t' 'NR==2{print $12}' $out)"
grep -v '^#' $cnt | awk -F, 'NF>2{n=$3; sub(/^cpu_core\//,"",n); sub(/\/u$/,"",n); sub(/:u$/,"",n); printf "\t%s=%s", n, $1} END{print ""}'
rm -f $out $cnt
