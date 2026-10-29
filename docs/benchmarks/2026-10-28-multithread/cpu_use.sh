#!/bin/bash
# usage: cpu_use.sh FFO -> real/user time for deft and edax at 1,2,4 threads
i=$1
for t in 1 2 4; do
  case $t in 1) c=3;; 2) c=2,3;; 4) c=0-3;; esac
  TIMEFORMAT="deft t=$t real %R user %U sys %S"
  time taskset -c $c /work/bench-root/deft/engine /work/deft-eval-main.dpr2 /work/hist/case$i.obf exact $t 256 --legacy-main-mpc > /dev/null
  TIMEFORMAT="edax t=$t real %R user %U sys %S"
  ( cd /work/bench-root/edax && time taskset -c $c ./engine -solve /work/hist/case$i.obf -n $t -l 60 -h 23 -eval-file /work/dl/data/eval.dat -book-usage off > /dev/null )
done
