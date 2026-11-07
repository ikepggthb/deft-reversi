#!/bin/bash
# 250 vs 1000-position MPC calibration, alternating order, one run at a time.
cd "$(dirname "$0")"
A=$HOME/dev/data/pen-exp/mpc-pen/model.dpr2
B=$HOME/dev/data/pen-exp/mpc-pen-1000/model.dpr2
run() { date '+%T start '$1; MODEL=$2 ./bench.sh $1 | tee -a bench.log; }
run cal250-r1 $A;  run cal1000-r1 $B
run cal1000-r2 $B; run cal250-r2 $A
run cal250-r3 $A;  run cal1000-r3 $B
echo QUEUE-DONE
