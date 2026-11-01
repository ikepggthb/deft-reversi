#!/bin/sh
# Selective endgame values against exact ones, normal vs measured multiples.
M=/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/e8.dpr2
S=tools/target/release/solve
run() { [ "$(wc -l < accuracy/$4.tsv 2>/dev/null)" = 901 ] && return; taskset -c $1 $S $2 accuracy900.obf $3 1 64 > accuracy/$4.tsv; }
run 0 $M@rules.json exact exact &
run 1 $M@rules.json:normal end85 normal-85 &
run 2 $M@rules.json end85 measured-85 &
run 3 $M@rules.json:normal end95 normal-95 &
wait
run 0 $M@rules.json end95 measured-95 &
run 1 $M@rules.json:normal end98 normal-98 &
run 2 $M@rules.json end98 measured-98 &
run 3 $M@rules.json:normal end77 normal-77 &
wait
run 0 $M@rules.json end77 measured-77 &
wait
