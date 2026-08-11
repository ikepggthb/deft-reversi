#!/bin/bash
# deft / edax / Egaroucid を同一環境でラウンドロビン比較する。
#
# エンジンごとにまとめて回すと、測定中のマシン負荷の変動がそのまま
# エンジン間の差に化ける。1 ラウンドで全エンジンを 1 回ずつ回し、
# ラウンドを重ねて中央値を取ること。
#
# セッションをまたいだ時間の比較はできない (docs/egaroucid-comparison.md 参照)。
# 比較したい対象は必ず同じ実行の中に入れること。旧コミットと比べるなら
# `git worktree add` でチェックアウトしてそれぞれビルドし、DEFT_BINS に足す。
#
# 使い方:
#   DEFT_BINS="旧=/path/a/deft-reversi-cli 新=/path/b/deft-reversi-cli" \
#   EDAX_DIR=/path/edax-reversi EGAROUCID_DIR=/path/Egaroucid \
#   PROBLEMS_DIR=/path/to/obf tools/crossbench.sh [ラウンド数]
#
# 問題ファイル (PROBLEMS_DIR):
#   deft.obf … "盤面64文字 手番" だけの形式 (deft 用)
#   full.obf … 解の注釈付き FFO 形式 (edax 用)
#   ega.obf  … "盤面64文字 手番" (Egaroucid 用。注釈があると弾かれる)
# もとの ffo40-49.obf からは次で作れる。
#   awk '{print $1" "$2}' ffo40-49.obf | sed 's/;$//' > deft.obf   # ega.obf も同じ
#
# deft はビルド時に RUSTFLAGS="-Ctarget-cpu=native" が必須。
# 付けないと flip / moves がスカラー版になり 1.75 倍遅くなる
# (docs/single-thread-profile.md)。
set -u

ROUNDS=${1:-5}
EVAL=${EVAL:-data/eval/eval-legacy.json}
LEVEL=${LEVEL:-60}
THREAD_LIST=${THREAD_LIST:-"1 4"}
PROBLEMS_DIR=${PROBLEMS_DIR:?PROBLEMS_DIR を指定してください}
OUT=${OUT:-crossbench.tsv}
: > "$OUT"

# HH:MM:SS.mmm / M:SS.mmm / SS.mmm を秒に変換
to_sec() {
  awk -F: '{n=NF; s=$n; if(n>=2) s+=$(n-1)*60; if(n>=3) s+=$(n-2)*3600; printf "%.3f", s}' <<< "$1"
}

run_deft() { # $1=binary
  local o
  o=$("$1" -s "$PROBLEMS_DIR/deft.obf" -e "$EVAL" -l "$LEVEL" 2>/dev/null)
  echo "$(to_sec "$(awk '/^total/{print $NF}' <<< "$o")") $(awk '/^total/{print $2}' <<< "$o")"
}

run_edax() { # $1=threads
  local o
  o=$(cd "$EDAX_DIR" && ./bin/lEdax-native -eval-file bin/data/eval.dat -solve "$PROBLEMS_DIR/full.obf" \
        -level "$LEVEL" -n "$1" 2>/dev/null)
  echo "$(to_sec "$(grep -oP 'nodes in\s+\K[0-9:.]+' <<< "$o" | tail -1)") $(grep -oP '\K[0-9]+(?= nodes in)' <<< "$o" | tail -1)"
}

run_ega() { # $1=threads
  local o
  o=$(cd "$EGAROUCID_DIR" && ./bin/Egaroucid_for_Console.out -solve "$PROBLEMS_DIR/ega.obf" \
        -level "$LEVEL" -t "$1" 2>/dev/null)
  echo "$(grep -oP 'nodes in \K[0-9.]+(?=s)' <<< "$o" | tail -1) $(grep -oP '^total \K[0-9]+' <<< "$o" | tail -1)"
}

for th in $THREAD_LIST; do
  for r in $(seq 1 "$ROUNDS"); do
    echo "round $r threads=$th" >&2
    if [ "$th" = 1 ]; then
      for spec in ${DEFT_BINS:-}; do
        printf '%s\t%s\t%s\n' "$th" "${spec%%=*}" "$(run_deft "${spec#*=}")" >> "$OUT"
      done
    fi
    [ -n "${EDAX_DIR:-}" ] && printf '%s\t%s\t%s\n' "$th" edax "$(run_edax "$th")" >> "$OUT"
    [ -n "${EGAROUCID_DIR:-}" ] && printf '%s\t%s\t%s\n' "$th" Egaroucid "$(run_ega "$th")" >> "$OUT"
  done
done

echo "--- 中央値 (${ROUNDS} 回) ---" >&2
sort -t"$(printf '\t')" -k1,1n -k2,2 "$OUT" | awk -F'\t' '
  { split($3, a, " "); key = $1 "\t" $2; times[key] = times[key] " " a[1]; nodes[key] = a[2] }
  END {
    for (k in times) {
      c = split(times[k], s, " ")
      for (i = 2; i <= c; i++) { v = s[i]; j = i - 1
        while (j > 0 && s[j] > v) { s[j+1] = s[j]; j-- }
        s[j+1] = v }
      med = (c % 2) ? s[(c+1)/2] : (s[c/2] + s[c/2+1]) / 2
      printf "%s\t%.3f\t%s\n", k, med, nodes[k]
    }
  }' | sort -n
