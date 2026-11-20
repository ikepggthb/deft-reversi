# FFO33〜51：CLI経由・8スレッドの確認

名称について：記録中の `deft-reversi-console` は測定時に使っていた名前。現在のアプリ名・パスは `deft-reversi-cli`。コマンドとハッシュの記録は測定時のものを保持する。

2026-11-16、直前の測定と同じCLI経由・8スレッド・各1回でFFO33〜51の19問を比較した。全57解（19問 × 3エンジン）は正解の石差と一致した。

## 結果

19問の探索時間の合計。1回の実測値であり、中央値ではない。

| エンジン | 合計秒 | ノード数 |
|---|---:|---:|
| Deft CLI | 6.589 | 1,086,188,562 |
| Edax 4.5.6 | 7.795 | 2,435,896,088 |
| Egaroucid Console 7.8.2 | 8.719 | 1,433,695,674 |

今回の合計ではDeftが最短で、Edaxより15.5%、Egaroucidより24.4%短時間だった。短時間で解ける問題が多く、各1回の確認なので、安定した順位やばらつきまでは評価しない。

## 条件と入力

- Core i3-14100、4物理コア＋SMTの8スレッド。CPU `0-7` に固定し、各エンジンを8スレッドで実行した。8物理コアでの測定ではない。
- Deftは `target-native/release/deft-reversi-console solve CASE.obf --model MODEL.dpr2 --exact --threads 8 --tt-mib 256 --tsv`。`RUSTFLAGS="-Ctarget-cpu=native"` のreleaseビルドを確認してから計測した。
- モデルはPEN・MPC2の `local/models/deft-eval-v3-pen.dpr2`。Edaxは奥原版4.5.6の公式Linux x86-64-v3配布版、EgaroucidはConsole 7.8.2を公式Clang手順でビルドしたもの。バイナリ・モデル・ソースのSHA-256は [manifest.json](manifest.json)。
- TTはDeft 256 MiB、Edax `-h 23`、Egaroucid `-hash 23`。補助表等が異なるため総メモリ量の完全一致ではない。
- 完全読み、定石なし、時間制限なし。各局面で新規プロセスを起動し、エンジンの実行順を局面ごとに回した。ビルドや他のベンチは計測と並行していない。
- 各エンジンが報告する探索時間で比較し、起動・モデル読込は含めない。DeftはCLIのTSVから探索完了・Exact・終局までの深さを検査した。
- Egaroucidの `hash23.eghs` は配布ソースに同梱されておらず、標準の乱数初期化を使った。これまでと同じ設定。
- 入力は [ffo33-51.obf](../../../tools/bench/positions/ffo33-51.obf)。FFO33〜39は [旧CLI付属のFFO20〜39](../../../deft-reversi-cli/problem/fforum-20-39.obf) の14〜20行目、FFO40〜51は [FFO40〜59](../../../tools/bench/positions/ffo40-59.obf) の先頭12行から作った。旧形式の正解の区切りをセミコロンに統一し、盤面・手番・正解の値は保持している。

## 再現

[bench.toml](bench.toml) のパスを環境に合わせて `local/bench.toml` に置き、次を実行する。

```sh
python3 tools/bench/ffo.py --threads 8 --cpus 0-7 --rounds 1 --positions tools/bench/positions/ffo33-51.obf --first 33 --out OUT deft edax egaroucid
```

## 局面ごとの秒数

| FFO | Deft CLI | Edax 4.5.6 | Egaroucid Console 7.8.2 |
|---|---:|---:|---:|
| 33 | 0.023 | 0.027 | 0.055 |
| 34 | 0.019 | 0.029 | 0.029 |
| 35 | 0.013 | 0.046 | 0.017 |
| 36 | 0.034 | 0.037 | 0.148 |
| 37 | 0.090 | 0.095 | 0.163 |
| 38 | 0.098 | 0.097 | 0.196 |
| 39 | 0.003 | 0.023 | 0.012 |
| 40 | 0.030 | 0.033 | 0.043 |
| 41 | 0.067 | 0.068 | 0.081 |
| 42 | 0.106 | 0.166 | 0.122 |
| 43 | 0.189 | 0.292 | 0.450 |
| 44 | 0.061 | 0.097 | 0.051 |
| 45 | 0.640 | 1.072 | 0.617 |
| 46 | 0.211 | 0.180 | 0.346 |
| 47 | 0.064 | 0.071 | 0.175 |
| 48 | 0.480 | 0.439 | 0.525 |
| 49 | 0.715 | 1.120 | 2.023 |
| 50 | 3.046 | 2.811 | 2.574 |
| 51 | 0.700 | 1.092 | 1.092 |

[全57解](results/results.tsv)・[計測ツールの集計](results/summary.txt)・[合計JSON](summary.json)・[機械の情報](machine.txt)。
