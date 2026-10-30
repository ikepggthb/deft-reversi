#!/bin/bash
# usage: cg.sh NAME [positions...]  -> instruction / mispredict counts (haswell build)
name=$1; shift; pos=${@:-40 44 47}
if [ "$name" = C ]; then src=/home/user/deft-reversi; else src=/work/wt/$name; fi
(cd $src && RUSTFLAGS="-C target-cpu=haswell" CARGO_TARGET_DIR=/work/tgt-hsw/$name taskset -c 0 cargo build --locked --release --example solve 2>&1 | grep -E "^error" )
for i in $pos; do
  taskset -c 1 valgrind --tool=cachegrind --cache-sim=no --branch-sim=yes --cachegrind-out-file=/dev/null /work/tgt-hsw/$name/release/examples/solve /work/deft-eval-main.dpr2 /work/hist/case$i.obf exact 1 256 --legacy-main-mpc 2>&1 | awk -v n=$name -v i=$i '/I *refs/{gsub(",","",$4); ir=$4} /Mispredicts:/{gsub(",","",$3); mp=$3} /^1\t/{nodes=$6} END{printf "%s FFO%s Ir=%d mispred=%d\n", n, i, ir, mp}'
done
