# 77〜95%の6候補の総当たり

候補は **77・81・85・89・92・95%**。各組100開局で先後を交換し、15組×100開局×2局＝3,000局を比較する。各候補は他5候補に対して1,000局ずつ打つ。

中盤は空き19以上で `Depth(60)`、100 ms／手・1手刻み。空き18以下は全候補共通の `Endgame/Full`、25 ms／手。1スレッド・TT 64 MiB・CPU 2で1局ずつ実行する。開始時と終盤への境界でTTを消去し、中盤の各手・反復間では再利用する。

## 実測分位点による倍率

現行のPENモデルは `local/data/pen-exp/mpc-pen-1000/rules.json` と回帰規則・12個の倍率がすべて一致した。同じ校正の41,000局面・445,000組の中盤誤差から、`|deep - slope*shallow - intercept| / deviation` の分位点を求めた。元の68・77・85・95・98・99%の倍率もすべて再現した。新しい確率の倍率は補間や正規分布の近似ではない。

| 確率 | 中盤の倍率 |
|---|---:|
| 77% | 1.0904432767305599 |
| 81% | 1.20365038009303 |
| 85% | 1.343916580839068 |
| 89% | 1.522878873567099 |
| 92% | 1.7082391076343133 |
| 95% | 1.9848425720756617 |

出典・SHA256・元倍率の再現値は [mid-sigmas.json](mid-sigmas.json) にある。モデルの重み・回帰係数・確認探索の深さは変えない。

実験用GTPの `--mid-sigma` で中盤の倍率を指定する。段階表の単調性を保つため中盤の6欄を同じ倍率にするが、中盤の各反復が使うのは指定段階の1欄だけ。81・89・92%の内部タグは既存の85%を使い、実効確率と内部タグはログで区別する。標準の77・85・95%は元のタグを維持し、実際に使う倍率も元と同じ。

終盤へ切り替えるときに元の規則・中盤倍率・終盤倍率をすべて復元してから共通の終盤探索をするため、中盤準備探索の85%を含め、実験倍率が終盤へ混ざることはない。再対局の `clear_board` では中盤の実験倍率を再適用する。

## 開局・検証

XOT原本をseed 4でシャッフルし、前の予備測定・600局・3,000局で使った局面と、校正用の空き52の局面を除外した。対称形を含め重複しない100開局を [xot-100.txt](xot-100.txt) に保存した。選択条件は [opening-selection.json](opening-selection.json)。全15組でこの100開局を共有する。

本対戦前に6候補・1開局・先後交換の30局を2 ms／手で動作確認し、確率・倍率・境界の検証と自動集計・完了記録を通過した。Rustの校正復元テスト、Pythonのテスト9件、Clippy・fmt・差分の空白検査も成功した。短い動作確認の勝敗は候補選択には使わない。

## 再現コマンド

```sh
python3 tools/bench/mpc_sigmas.py \
  --model local/release/model-v3-pen/deft-eval-v3-pen.dpr2 \
  --rules local/data/pen-exp/mpc-pen-1000/rules.json \
  --positions local/data/pen-exp/mpc-pen-1000/positions.tsv \
  --out docs/benchmarks/2026-11-08-midgame-mpc/tournament-77-95/mid-sigmas.json
RUSTFLAGS='-Ctarget-cpu=native' CARGO_TARGET_DIR=target-native \
  cargo build --locked --release -p deft_reversi_engine --example gtp
python3 tools/bench/mpc_match.py \
  --out docs/benchmarks/2026-11-08-midgame-mpc/tournament-77-95 \
  --round-robin --candidates 77,81,85,89,92,95 --openings 100 --seed 4 \
  --xot docs/benchmarks/2026-11-08-midgame-mpc/tournament-77-95/xot-100.txt \
  --mid-sigmas docs/benchmarks/2026-11-08-midgame-mpc/tournament-77-95/mid-sigmas.json \
  --budgets 100 --endgame-empties 18 --endgame-ms 25 --cpu 2 --report-on-completion
```

同じ設定・バイナリ・モデル・ランナーなら保存済みの対局を除いて再開できる。各手のログは `logs/`（git で管理せず、手元の `local/bench-runs/docs-raw/2026-11-08-midgame-mpc/tournament-77-95/` に置いた。`run.log` も同じ）、組別は `summary.json`、総合は `tournament.json`、時間と深さは `move-statistics.md`・`move-statistics.tsv` に出力する。全件検証と集計まで成功した場合に `completion.json` ができる。終了後の結果は [results.md](results.md) に自動出力する。

区間は先後ペア・開局を単位に求める。今回の開局での候補選びと、さらに独立した開局での最終確認は区別する。

## 完了結果と判断

3,000局と全件検証・自動集計が完了した。`completion.json` に完了を記録済み。15組それぞれ200局、100開局すべての先後ペア、重複IDなしを再確認した。対局時間の合計は10,736.10秒（2時間58分56秒）。

| 順位 | 確率 | 勝 / 分 / 負 | 総合得点率 | 開局単位の95%区間 |
|---:|---|---:|---:|---:|
| 1 | 81% | 443 / 161 / 396 | 52.35% | 49.35〜55.20% |
| 2 | 85% | 428 / 188 / 384 | 52.20% | 49.20〜55.30% |
| 3 | 89% | 422 / 170 / 408 | 50.70% | 47.55〜53.90% |
| 4 | 95% | 425 / 149 / 426 | 49.95% | 46.65〜53.30% |
| 5 | 77% | 401 / 161 / 438 | 48.15% | 44.85〜51.45% |
| 6 | 92% | 389 / 155 / 456 | 46.65% | 43.85〜49.50% |

最高点は81%だが、85%との差は0.15ポイントにとどまる。両者の総合得点率の差を共通の100開局を単位に10,000回のbootstrap（seed 1）で調べると、95%区間は−4.25〜+4.45ポイント。直接対戦200局も81%側53.75%（先後ペア単位の95%区間48.25〜59.00%）で、85%より強いとはまだ確認できない。比較結果は [top-paired-comparison.json](top-paired-comparison.json) に保存した。

この測定では81〜85%付近が次の確認候補になる。既存の85%を81%へ変更する根拠としては不十分で、選択確率の採用変更は行っていない。次に調べるなら、独立した開局で81%対85%の対戦数を増やす。

中盤102,000着手で境界・確率・倍率の検証を通過した。共通終盤53,952着手のうち53,741着手が完全読みを完了し、211着手は上限内に完了しなかった。順位は共通終盤25 ms上限を含む条件の結果である。

[総合結果と平均完了深さ](results.md)、[1手の時間と深さの全統計](move-statistics.md)、[組ごとの結果](summary.json)、[全件検証](diagnostics.json)を参照。

## 詳細データ

追加対戦なしで元ログを展開した[詳細レポート](details/report.md)と[HTML版](details/report.html)を追加した。[集計CSVの軽量ZIP](details/summary-data.zip)は約0.7 MiB、全生データを含むZIP（`detailed-data.zip`、約76.7 MiB）と `moves.csv.gz`・`iterations.csv.gz` は git で管理せず、手元の `local/bench-runs/docs-raw/2026-11-08-midgame-mpc/tournament-77-95/details/` にある。

対戦表、先後別成績、全1,500開局ペアの結果、各開局の成績、空き数ごとの時間・深さ・MPC、反復の完了時刻、85%との差の区間と多重比較調整を収録した。生CSVは155,952着手と2,137,796完了反復。全棋譜を再生して合法手・盤面・最終石差を照合し、元の総合成績と一致することを確認した。着手CSVには盤面OBF・選んだ手・合法手数もある。単位・結合キー・未記録項目は[CSV仕様](details/schema.md)を参照。

再生成は次のコマンド。元の成績ファイルや探索設定は変更しない。

    python3 tools/bench/mpc_detailed_report.py docs/benchmarks/2026-11-08-midgame-mpc/tournament-77-95
