# FFO40〜59：CLI経由・4物理コアの確認

名称について：記録中の `deft-reversi-console` は測定時に使っていた名前。現在のアプリ名・パスは `deft-reversi-cli`。コマンドとハッシュの記録は測定時のものを保持する。

2026-11-16、`deft-reversi-console solve --tsv` 経由で4物理コアのFFOを確認した。ユーザー指定により各エンジン1回だけ測定。20問 × 3エンジン = 60解はすべて正解の石差と一致した。

## 結果

20問の探索時間の合計。中央値ではなく1回の実測値。

| エンジン | 合計秒 | ノード数 |
|---|---:|---:|
| Deft CLI | 73.811 | 12,757,643,803 |
| Edax 4.5.6 | 88.675 | 25,247,465,259 |
| Egaroucid Console 7.8.2 | 84.481 | 12,516,511,211 |

今回のDeftはEdaxより16.8%、Egaroucidより12.6%短時間だった。1回の動作・性能確認なので、ばらつきや常時の順位までは評価しない。

## 条件

- CPU：Core i3-14100。`taskset -c 0,2,4,6` で4つの異なる物理コアに固定、各エンジン4スレッド。SMTの兄弟コアは指定していない。
- Deftの実行バイナリ：`target-native/release/deft-reversi-console`。`RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native cargo build --locked --release -p deft-reversi-console` で最新のソースからビルド確認した。
- Deftの実コマンド：`taskset -c 0,2,4,6 target-native/release/deft-reversi-console solve CASE.obf --model MODEL.dpr2 --exact --threads 4 --tt-mib 256 --tsv`。エンジンのexampleは呼んでいない。
- モデル：`local/models/deft-eval-v3-pen.dpr2`、PEN・MPC2。重み、CLIソース、比較用バイナリ等のSHA-256は [manifest.json](manifest.json)。作業HEADは `ecce031`、CLIの未コミットのソースもファイル単位でハッシュを記録した。
- Edaxは奥原版4.5.6の公式 `lEdax-x86-64-v3` と同梱の評価データ、EgaroucidはConsole 7.8.2を公式Clang手順でビルドしたもの。同じ比較用バイナリを[前回](../2026-11-16-ffo-latest/README.md)から引き継いだ。
- TT指定はDeft 256 MiB、Edax `-h 23`、Egaroucid `-hash 23`。補助表などが異なるため総メモリ量を完全一致させた比較ではない。
- 定石なし、終局までの完全読み、時間制限なし。各局面で新規プロセスを起動し、エンジンの順番は局面ごとに回した。ビルドや別のベンチは計測と並行していない。
- 時間は各エンジンが報告する探索時間。CLIの起動やモデル読込の時間を含めない。DeftのCLIは小数点以下9桁で出力し、探索完了・Exact・終局までの深さを計測ツールで確認した。
- Egaroucidの配布リソースには今回のサイズ用の `hash23.eghs` がなく、標準動作の乱数初期化を使っている。
- 最初は3回指定で起動したが、途中の「確認なので1回」という指示に従って1回目の全60解取得後に停止した。2回目の途中で得られた7解は集計から除外した。

## 再現

[bench.toml](bench.toml) のパスを環境に合わせて `local/bench.toml` に設定し、次を実行する。

```sh
python3 tools/bench/ffo.py --threads 4 --cpus 0,2,4,6 --rounds 1 --out OUT deft edax egaroucid
```

## 各局面の秒数

| FFO | Deft CLI | Edax 4.5.6 | Egaroucid Console 7.8.2 |
|---|---:|---:|---:|
| 40 | 0.033 | 0.045 | 0.100 |
| 41 | 0.070 | 0.082 | 0.140 |
| 42 | 0.099 | 0.120 | 0.555 |
| 43 | 0.189 | 0.462 | 0.611 |
| 44 | 0.064 | 0.097 | 0.265 |
| 45 | 0.728 | 1.249 | 1.037 |
| 46 | 0.208 | 0.227 | 0.424 |
| 47 | 0.069 | 0.075 | 0.348 |
| 48 | 0.507 | 0.569 | 0.889 |
| 49 | 0.712 | 1.397 | 1.751 |
| 50 | 3.483 | 3.783 | 3.297 |
| 51 | 0.781 | 1.397 | 1.413 |
| 52 | 1.908 | 2.386 | 2.253 |
| 53 | 6.233 | 8.836 | 6.877 |
| 54 | 10.407 | 12.381 | 11.779 |
| 55 | 34.926 | 38.034 | 34.091 |
| 56 | 3.600 | 4.341 | 4.363 |
| 57 | 5.031 | 6.741 | 7.536 |
| 58 | 4.739 | 6.438 | 6.357 |
| 59 | 0.023 | 0.015 | 0.395 |

[全60解のデータ](results/results.tsv)・[計測ツールの集計](results/summary.txt)・[合計JSON](summary.json)・[機械の情報](machine.txt)。
