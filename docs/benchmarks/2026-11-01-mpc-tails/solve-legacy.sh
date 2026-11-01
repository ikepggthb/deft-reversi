#!/bin/sh
# Legacy main-branch MPC rules.
m=$1; shift
exec /home/azure/dev/deft-reversi-rewrite/docs/benchmarks/2026-11-01-mpc-tails/tools/target/release/solve "$m" "$@"
