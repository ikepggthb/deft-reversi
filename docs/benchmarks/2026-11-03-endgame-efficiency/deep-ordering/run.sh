#!/bin/sh
# node screening on CPU 6: pos-24, pos-26 and FFO 50-58 per DEFT_DEEP_ORD setting
S=/tmp/claude-1000/-home-azure-dev-deft-reversi-rewrite/3d35d250-9f8d-59a2-bf5d-e57cce4c0696/scratchpad
D=/home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-03-endgame-efficiency
M=/home/azure/dev/deft-reversi-feature/runs/v2-pattern-lr0.3/mpc-formula/model.dpr2
sed -n 11,19p /home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-02-exact-mpc-formula/tune/ffo40-59.obf > $S/ffo50-58.obf
mkdir -p $S/deep
for cfg in "$@"; do
  for f in $D/pos-24.obf $D/pos-26.obf $S/ffo50-58.obf; do
    n=$(basename $f .obf)
    DEFT_DEEP_ORD=$cfg taskset -c 6 $S/solve-deep $M $f exact 1 256 > $S/deep/$cfg.$n.tsv
  done
  printf "%-10s" "$cfg"
  for n in pos-24 pos-26 ffo50-58; do awk -F'\t' 'NR>1{s+=$12;t+=$6}END{printf "  %7.1fs %8.0fM", s, t/1e6}' $S/deep/$cfg.$n.tsv; done; echo
done
