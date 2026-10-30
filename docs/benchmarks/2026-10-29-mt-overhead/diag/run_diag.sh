#!/bin/sh
# usage: run_diag.sh BINARY OUTDIR ROUNDS8 ; 1 thread on CPU 4 once, 8 threads on CPU 0-7 ROUNDS8 times
B=/home/azure/dev/bench-engines; bin=$1; out=$2; n=$3; mkdir -p $out
for f in $(seq 40 49); do taskset -c 4 $bin $B/model/deft-eval-main.dpr2 $B/cases/ffo$f.obf exact 1 256 --legacy-main-mpc > $out/t1-r1-ffo$f.out 2> $out/t1-r1-ffo$f.err; done
for r in $(seq 1 $n); do for f in $(seq 40 49); do taskset -c 0-7 $bin $B/model/deft-eval-main.dpr2 $B/cases/ffo$f.obf exact 8 256 --legacy-main-mpc > $out/t8-r$r-ffo$f.out 2> $out/t8-r$r-ffo$f.err; done; done
