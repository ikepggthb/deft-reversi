#!/bin/sh
# usage: build_trial.sh NAME PATCH  -> /home/azure/dev/bench-engines/bin/deft-NAME (worktree restored afterwards)
set -e
patch=$(realpath "$2")
repo=/home/azure/dev/deft-reversi-rewrite
cd $repo
git diff --quiet deft-reversi-engine || { echo "engine tree not clean" >&2; exit 1; }
git apply "$patch"
RUSTFLAGS="-C target-cpu=native" cargo build --locked --release --example solve 2>&1 | grep -E "^(error|warning)" -A7 || true
cp target/release/examples/solve /home/azure/dev/bench-engines/bin/deft-$1
git checkout -q deft-reversi-engine
sha256sum /home/azure/dev/bench-engines/bin/deft-$1
