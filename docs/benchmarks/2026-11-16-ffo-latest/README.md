# FFO40〜59：Deft・Edax 4.5.6・Egaroucid Console 7.8.2

2026-11-16、Core i3-14100で完全読みを比較した。各条件3回、主結果は各回の20問合計の中央値。主比較の360解（20問 × 3エンジン × 3回 × 2条件）はすべて正解と一致した。

## 結果

| エンジン | 1スレッド | 4スレッド・4物理コア | 1→4の高速化 |
|---|---:|---:|---:|
| Deft | 272.05秒 | 73.23秒 | 3.72倍 |
| Edax 4.5.6 | 313.52秒 | 88.45秒 | 3.54倍 |
| Egaroucid Console 7.8.2 | 254.50秒 | 83.66秒 | 3.04倍 |

- 1スレッド：DeftはEdax 4.5.6より13.2%短時間、Egaroucidより6.9%長時間だった。
- 4スレッド：DeftはEdax 4.5.6より17.2%、Egaroucidより12.5%短時間だった。
- 1スレッドのEgaroucidは合計251.31〜288.21秒と変動した。Deftの272.02〜272.09秒を上回る回もあり、常に同じ順位になる結果ではない。

## 条件と再現

- CPU：Intel Core i3-14100、4コア・8論理CPU。詳細は [machine.txt](machine.txt)。
- 1スレッドはCPU 2、4スレッドはCPU 0,2,4,6に固定した。後者はSMTの兄弟を重複させない4物理コア。
- 入力：[FFO40〜59](../../../tools/bench/positions/ffo40-59.obf)、空き20〜34マス、20問。終局までの石差を確認した。着手の一致までは検査していない。
- 各局面・各回で新規プロセスを起動し、前の問題のTTを再利用しない。定石なし、時間制限なし。各エンジンの報告する探索時間を使い、起動・モデル読込の時間は比較に含めない。
- 計測は1プロセスずつ順番に行い、計測中にビルドや他のベンチは動かしていない。
- Deft：エンジンはdevelop/v2の `c4fc1ff` と同じコード。作業HEADは `ecce031`、Console移植によるエンジン変更はない。既存ベンチ用 `examples/solve` から同じEngineを呼ぶ。`RUSTFLAGS="-Ctarget-cpu=native"`、releaseビルド。
- モデル：`local/models/deft-eval-v3-pen.dpr2`（公開用PEN・MPC2モデルを複製）、SHA-256 `1a6e6b81aa4de564ab47674bc2911c42e1c0c33df2a74bb9e2dc3694f384eab5`。
- Edax：[奥原版 v4.5.6](https://github.com/okuhara/edax-reversi-AVX/releases/tag/v4.5.6) の公式ZIPに含まれる `lEdax-x86-64-v3` と `data/eval.dat`。配布バイナリをそのまま使い、ローカルビルドはしていない。
- Egaroucid：[Console版 v7.8.2](https://github.com/Nyanyan/Egaroucid/releases/tag/console_v7.8.2)。公式ソース同梱の評価データを使い、[公式手順](https://www.egaroucid.nyanyan.dev/en/console/)のClangコマンドでビルドした。GUI版のバージョン番号とは異なる。
- TT設定：Deft `256 MiB`、Edax `-h 23`、Egaroucid `-hash 23`。各実装のエントリや補助表の構成が異なり、総メモリ量をバイト単位で一致させた比較ではない。
- Egaroucidの配布リソースには `hash23.eghs` がなく、ソースの `hash_resize` はファイルを読めない場合 `hash_init_rand(23)` にフォールバックする。追加の固定ハッシュを作らず、公式配布の動作のまま測った。この乱数初期化はノード数の変動要因になる。
- 最初の1スレッド測定ではEdax 4.7を使ったが、ユーザー指定に従い4.5.6を追加測定した。Deft/Egaroucidの1スレッド結果は最初の測定を再利用し、Edaxのみ後続の別バッチで3回測った。4スレッドは最初から4.5.6を含む3エンジンを局面・回ごとに順番を回して測定した。
- 1スレッドの主集計は別バッチの結果を合成したもので、同時期の交互実行で全3エンジンを測り直した結果ではない。各バッチの生データを残している。

ビルド例（詳細な版・SHA-256は [manifest.json](manifest.json)）：

```sh
RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native \
  cargo build --locked --release -p deft_reversi_engine --example solve

# Egaroucid Console v7.8.2の展開先で実行
clang++ -O2 ./src/Egaroucid_for_Console.cpp \
  -o ./bin/Egaroucid_for_Console.out \
  -mtune=native -march=native -pthread -std=c++20
```

実行時のパスは [bench.toml](bench.toml) に保存した。別の作業ディレクトリではパスを書き換え、`local/bench.toml` として置く。

```sh
# 1スレッド初回バッチ（Deft/Egaroucidの採用元。Edax列は4.7の参考値）
python3 tools/bench/ffo.py --threads 1 --cpus 2 --rounds 3 \
  --out docs/benchmarks/2026-11-16-ffo-latest/t1-initial-edax47 deft edax egaroucid

# Edaxのパスを4.5.6に変更して再測定
python3 tools/bench/ffo.py --threads 1 --cpus 2 --rounds 3 \
  --out docs/benchmarks/2026-11-16-ffo-latest/t1-edax456 edax

python3 tools/bench/ffo.py --threads 4 --cpus 0,2,4,6 --rounds 3 \
  --out docs/benchmarks/2026-11-16-ffo-latest/t4 deft edax egaroucid
```

新たに主比較を一括で再現する場合は、Edaxのパスを4.5.6にした状態で、最初のコマンドの出力先を別の新規フォルダにして3エンジンを測る。

## 各回の合計秒

| 条件 | エンジン | 1回目 | 2回目 | 3回目 |
|---|---|---:|---:|---:|
| 1スレッド | Deft | 272.020 | 272.089 | 272.050 |
| 1スレッド | Edax 4.5.6 | 313.582 | 313.516 | 313.262 |
| 1スレッド | Egaroucid Console 7.8.2 | 254.505 | 288.208 | 251.309 |
| 4スレッド | Deft | 73.178 | 73.895 | 73.226 |
| 4スレッド | Edax 4.5.6 | 88.601 | 87.595 | 88.449 |
| 4スレッド | Egaroucid Console 7.8.2 | 85.200 | 83.660 | 82.568 |

## 局面ごとの秒数（3回の中央値）

| FFO | Deft 1T | Edax 1T | Egaroucid 1T | Deft 4T | Edax 4T | Egaroucid 4T |
|---|---:|---:|---:|---:|---:|---:|
| 40 | 0.109 | 0.137 | 0.125 | 0.033 | 0.043 | 0.089 |
| 41 | 0.203 | 0.273 | 0.169 | 0.070 | 0.080 | 0.139 |
| 42 | 0.325 | 0.346 | 0.282 | 0.099 | 0.119 | 0.543 |
| 43 | 0.657 | 1.043 | 0.716 | 0.164 | 0.384 | 0.634 |
| 44 | 0.184 | 0.307 | 0.153 | 0.064 | 0.102 | 0.276 |
| 45 | 2.475 | 3.667 | 2.398 | 0.724 | 1.263 | 1.030 |
| 46 | 0.694 | 0.753 | 0.572 | 0.211 | 0.225 | 0.477 |
| 47 | 0.218 | 0.199 | 0.165 | 0.068 | 0.071 | 0.303 |
| 48 | 1.967 | 2.077 | 2.088 | 0.519 | 0.572 | 0.920 |
| 49 | 2.525 | 4.606 | 2.859 | 0.712 | 1.396 | 1.753 |
| 50 | 12.774 | 15.035 | 10.607 | 3.473 | 3.769 | 3.451 |
| 51 | 2.851 | 3.366 | 2.970 | 0.791 | 1.397 | 1.435 |
| 52 | 5.426 | 6.575 | 3.716 | 1.944 | 2.351 | 2.328 |
| 53 | 23.200 | 30.822 | 19.997 | 6.232 | 8.827 | 7.515 |
| 54 | 37.431 | 45.798 | 35.442 | 9.886 | 12.411 | 12.136 |
| 55 | 132.892 | 139.809 | 123.497 | 34.950 | 38.311 | 32.784 |
| 56 | 12.897 | 16.280 | 11.144 | 3.589 | 4.222 | 4.240 |
| 57 | 18.310 | 19.643 | 18.887 | 5.103 | 6.651 | 7.144 |
| 58 | 16.912 | 22.545 | 16.992 | 4.760 | 6.392 | 6.155 |
| 59 | 0.028 | 0.054 | 0.364 | 0.022 | 0.015 | 0.390 |

各問の中央値の和と、主表の「各回の合計の中央値」は別の統計量なので一致しないことがある。

## データと検証

- [主集計JSON](summary.json)、[1スレッド集計](t1/summary.txt)、[4スレッド集計](t4/summary.txt)。
- [1スレッド主比較データ](t1/results.tsv) はDeft/Egaroucidと4.5.6の結果を合成した。`source_batch` は測定元、`slot` はそのバッチ内の実行順を表す。
- 測定元：[初回3エンジン](t1-initial-edax47/results.tsv)、[Edax 4.5.6の追加測定](t1-edax456/results.tsv)、[4スレッド](t4/results.tsv)。Edax 4.7は参考データとしてのみ保存し、主結果には混ぜていない。
- 比較ツールは [ffo.py](../../../tools/bench/ffo.py)。相対パスの出力先でも、異なる作業ディレクトリで起動する相手エンジンに入力を渡せるよう、出力先を絶対パスへ解決する修正を入れた（[差分](source.diff)）。
- `python3 -m unittest discover -s tools/bench/tests` 成功。全360解の正解一致と、条件・局面・回・エンジンの組が欠けず重複しないことを集計時に再検査した。

この結果はこのCPU・モデル・TT設定でのFFO完全読みの速さを示す。過去のCore Ultra機の数値とは直接比較せず、通常対局の勝率やすべての局面での順位を表すものとも扱わない。
