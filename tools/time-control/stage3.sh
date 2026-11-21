#!/usr/bin/env bash
# Stage 3-1 of the time-control roadmap: self-play with the clock log, normal
# games and extension mode, one condition after another.
#
# usage: tools/time-control/stage3.sh BINARY OUT_DIR [CONDITION...]
#
# Conditions (default: all, in this order):
#   n-gtp2 n-fs6 n-gtp20 n-gtp60 n-gtp20t4   normal games, openings 200..299
#   x-gtp2 x-gtp20 x-gtp60 x-gtp20t4         extension, openings 300..449
#
# Stopping: `touch OUT_DIR/STOP` ends the run after the games in progress
# (at most one game per worker). Killing it is also safe: rows are written
# only for finished games. Rerunning the same command resumes; remove STOP
# first. A condition whose rows are all present is skipped by the referee.
#
# Uses P cores only (CPU 0-7): eight 1-thread workers or two 4-thread ones.
set -euo pipefail
bin=$(realpath "$1"); out=$(realpath -m "$2"); shift 2
here=$(cd "$(dirname "$0")" && pwd)
: "${DEFT_MODEL:?set DEFT_MODEL}"
export DEFT_MODEL STOP_FILE="$out/STOP"
mkdir -p "$out"
conditions=("$@")
[ ${#conditions[@]} -gt 0 ] || conditions=(n-gtp2 n-fs6 n-gtp20 n-gtp60 n-gtp20t4
                                         x-gtp2 x-gtp20 x-gtp60 x-gtp20t4)
one="0;1;2;3;4;5;6;7"; four="0-3;4-7"
for c in "${conditions[@]}"; do
  if [ -e "$STOP_FILE" ]; then echo "STOP found; not starting $c"; exit 0; fi
  case $c in
    n-gtp2)    a=(gtp 2 200:299);       cpus=$one;  t=1; ext= ;;
    n-fs6)     a=(flipsight 6 200:299); cpus=$one;  t=1; ext= ;;
    n-gtp20)   a=(gtp 20 200:299);      cpus=$one;  t=1; ext= ;;
    n-gtp60)   a=(gtp 60 200:299);      cpus=$one;  t=1; ext= ;;
    n-gtp20t4) a=(gtp 20 200:299);      cpus=$four; t=4; ext= ;;
    x-gtp2)    a=(gtp 2 300:449);       cpus=$one;  t=1; ext=20-40:0.7 ;;
    x-gtp20)   a=(gtp 20 300:449);      cpus=$one;  t=1; ext=20-40:5 ;;
    x-gtp60)   a=(gtp 60 300:449);      cpus=$one;  t=1; ext=20-40:15 ;;
    x-gtp20t4) a=(gtp 20 300:449);      cpus=$four; t=4; ext=20-40:5 ;;
    *) echo "unknown condition $c"; exit 2 ;;
  esac
  echo "$(date '+%F %T') start $c"
  CPUS=$cpus THREADS=$t EXTEND=$ext CLOCK_LOG_DIR="$out/$c" \
    python3 "$here/referee.py" play "${a[0]}" "$bin" "$bin" "${a[1]}" "${a[2]}" "$out/$c.tsv"
  python3 "$here/referee.py" summary "$out/$c.tsv"
done
echo "$(date '+%F %T') done"
