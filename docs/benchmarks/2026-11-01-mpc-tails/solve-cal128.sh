#!/bin/sh
# The 64/128-position calibration of 2026-10-31 (normal multiples).
m=$1; shift
exec /home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-01-mpc-tails/tools/target/release/solve "$m@/home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-01-mpc-tails/../2026-10-31-mpc-calibration/rules.json" "$@"
