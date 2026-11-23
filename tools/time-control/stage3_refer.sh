#!/usr/bin/env bash
# Stage 3-2 of the time-control roadmap: reference scores of the moves the
# iterations chose in the stage 3-1 games.
#
# usage: tools/time-control/stage3_refer.sh RUN_DIR OUT_DIR [DEPTH]
#
# RUN_DIR is a stage3.sh output (n-*.tsv, x-*.tsv and their clock logs).
# Positions come from the normal games of openings 200..299 and the
# extended turns of the extension games (refer_input.py), each position
# once. 20 single-thread refer_log processes (CPU 0-19; this measures no
# time, so E cores are fine). DEPTH (default 26) is the reference depth
# above 28 empties.
#
# Stopping: `touch OUT_DIR/STOP` ends each process after its current
# position; killing is also safe. Rerunning resumes (remove STOP first).
set -euo pipefail
run=$(realpath "$1"); out=$(realpath -m "$2"); depth=${3:-26}
here=$(cd "$(dirname "$0")" && pwd)
: "${DEFT_MODEL:?set DEFT_MODEL}"
mkdir -p "$out"
bin="$here/probe/target/release/refer_log"
[ -x "$bin" ] || (cd "$here/probe" && cargo build --release --bin refer_log)
if [ ! -s "$out/positions.tsv" ]; then
  n=(); x=()
  for c in gtp2 fs6 gtp20 gtp60 gtp20t4; do [ -e "$run/n-$c.tsv" ] && n+=("$run/n-$c.tsv"); done
  for c in gtp2 gtp20 gtp60 gtp20t4; do [ -e "$run/x-$c.tsv" ] && x+=("$run/x-$c.tsv"); done
  python3 "$here/refer_input.py" 200:299 "${n[@]}" > "$out/n.tsv.tmp"
  python3 "$here/refer_input.py" 300:449 "${x[@]}" > "$out/x.tsv.tmp"
  # One list; a position in both keeps the normal game's line first.
  python3 - "$out/n.tsv.tmp" "$out/x.tsv.tmp" > "$out/positions.tsv" <<'PY'
import sys
seen = set()
for path in sys.argv[1:]:
    for line in open(path):
        w = line.rstrip("\n").split("\t")
        if w[0] == "# also":
            print(line, end="")
        elif w[0] not in seen:
            seen.add(w[0])
            print(line, end="")
        else:
            print("# also\t" + w[0] + "\t" + "\t".join(w[6:]))
PY
  mv "$out/n.tsv.tmp" "$out/positions-n.tsv"; mv "$out/x.tsv.tmp" "$out/positions-x.tsv"
fi
echo "$(date '+%F %T') start: $(grep -vc '^#' "$out/positions.tsv") positions"
for i in $(seq 0 19); do
  taskset -c "$i" "$bin" "$out/positions.tsv" "$out/out-$i.tsv" "$depth" "$i/20" &
done
wait
done_n=$(cat "$out"/out-*.tsv | wc -l)
echo "$(date '+%F %T') end: $done_n positions done"
