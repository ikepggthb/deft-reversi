#!/bin/sh
# run.sh NAME MODEL SET CPU [ENV=VALUE...]: scores the 98% selective endgame
# of MODEL on SET (pos-22-24 or games-22-24) into runs/NAME.SET.tsv.
# Each SET has its own exact-value cache, copied per run and merged after.
cd "$(dirname "$0")"
name=$1; model=$2; set=$3; cpu=$4; shift 4
cache=runs/exact-$set.tsv
tmp=runs/.cache-$name-$set.tsv
cp "$cache" "$tmp" 2>/dev/null || : > "$tmp"
env "$@" taskset -c "$cpu" tools/target/release/mpc-quality "$model" "$set.obf" "${SEL:-98}" "$tmp" "runs/$name.$set.tsv"
cat "$tmp" >> "$cache" && sort -u -o "$cache" "$cache" && rm "$tmp"
