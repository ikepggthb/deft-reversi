#!/bin/sh
# The tools solve with the 1000-position calibration, measured multiples.
m=$1; shift
exec /home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-01-mpc-tails/tools/target/release/solve "$m@/home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-01-mpc-tails/rules.json" "$@"
