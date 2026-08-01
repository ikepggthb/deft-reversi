#!/usr/bin/env bash
# 小型NNUEバリアント学習キューの進捗表示
cd "$(dirname "$0")/.."

printf "%-22s %-9s %-12s %s\n" "run" "epoch" "valid_mae" "progress"
for dir in runs/small-*/; do
  name=$(basename "$dir")
  csv="$dir/logs/metrics.csv"
  if [ ! -f "$csv" ]; then
    printf "%-22s %s\n" "$name" "(未開始)"
    continue
  fi
  # 完了エポックの valid MAE 一覧
  valids=$(awk -F, '$1=="summary" {printf "%s:%.3f ", $3, $15}' "$csv")
  # 進行中エポックの最新 train 行
  last=$(awk -F, '$1=="train"' "$csv" | tail -1)
  if [ -n "$last" ]; then
    progress=$(echo "$last" | awk -F, '{printf "ep%s %d/%d steps %.0f samples/s ETA(all) %dmin", $3, $5, $6, $17, $20/60}')
  else
    progress=""
  fi
  done_epochs=$(awk -F, '$1=="summary"' "$csv" | wc -l)
  printf "%-22s %-9s %-12s %s\n" "$name" "${done_epochs}/8" "${valids:-—}" "$progress"
done
