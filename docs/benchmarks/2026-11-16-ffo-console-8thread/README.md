# FFO40〜59：CLI経由・8スレッドの確認

名称について：記録中の `deft-reversi-console` は測定時に使っていた名前。現在のアプリ名・パスは `deft-reversi-cli`。コマンドとハッシュの記録は測定時のものを保持する。

2026-11-16、各エンジン1回、4物理コア・SMTありの8スレッドで確認した。Deftは `deft-reversi-console solve --tsv` 経由。全60解（20問 × 3エンジン）は正解の石差と一致した。

## 結果

20問の探索時間の合計。[直前の4スレッド](../2026-11-16-ffo-console-4core/README.md)と同一バイナリ・モデル・入力ファイルであることをSHA-256で確認した。

| エンジン | 4スレッド・4物理コア | 8スレッド・4物理コア＋SMT | 時間短縮 | 倍率 |
|---|---:|---:|---:|---:|
| Deft CLI | 73.811秒 | 63.425秒 | 14.1% | 1.16倍 |
| Edax 4.5.6 | 88.675秒 | 68.166秒 | 23.1% | 1.30倍 |
| Egaroucid Console 7.8.2 | 84.481秒 | 65.387秒 | 22.6% | 1.29倍 |

今回の8スレッドではDeftが合計で最短で、Edaxより7.0%、Egaroucidより3.0%短時間だった。Deftの4→8スレッドによる短縮は14.1%。8つの物理コアでの測定ではなく、同じ4物理コアでSMTを追加した比較である。

## 条件

- CPU：Core i3-14100、4物理コア・8論理CPU。`taskset -c 0-7`、各エンジン8スレッド。
- 各1回の確認値。ばらつきの評価や統計的な順位の確定は行っていない。特にEgaroucidとの差は約1.96秒なので、1回の実測として扱う。
- Deft：`target-native/release/deft-reversi-console`。`RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native cargo build --locked --release -p deft-reversi-console` で最新ビルドを確認してから計測した。
- Deftの実コマンド：`taskset -c 0-7 target-native/release/deft-reversi-console solve CASE.obf --model MODEL.dpr2 --exact --threads 8 --tt-mib 256 --tsv`。
- モデルは `local/models/deft-eval-v3-pen.dpr2`、PEN・MPC2。Edaxは奥原版4.5.6公式Linux x86-64-v3配布版、EgaroucidはConsole 7.8.2の公式ソースをClangでビルドしたもの。ソース・バイナリ・重みのSHA-256と実行条件は [manifest.json](manifest.json)。
- TTはDeft 256 MiB、Edax `-h 23`、Egaroucid `-hash 23`。補助表などの違いがあるため総メモリ量は完全には一致させていない。
- 完全読み、定石なし、時間制限なし。局面ごとに新規プロセスを起動し、エンジンの順番を局面ごとに回した。ビルドや別ベンチは並行していない。
- 各エンジンの報告する探索時間を使用し、起動・モデル読込は含めない。DeftのCLI出力から探索完了・Exact・終局までの深さも計測ツールで確認した。
- Egaroucidには `hash23.eghs` が同梱されていないため、標準の乱数初期化を使った。前回と同じ設定。

## 再現

[bench.toml](bench.toml) のパスを環境に合わせて `local/bench.toml` に設定して実行する。

```sh
python3 tools/bench/ffo.py --threads 8 --cpus 0-7 --rounds 1 --out OUT deft edax egaroucid
```

## 局面ごとの秒数

| FFO | Deft CLI | Edax 4.5.6 | Egaroucid Console 7.8.2 |
|---|---:|---:|---:|
| 40 | 0.034 | 0.054 | 0.040 |
| 41 | 0.069 | 0.071 | 0.054 |
| 42 | 0.106 | 0.157 | 0.148 |
| 43 | 0.277 | 0.306 | 0.541 |
| 44 | 0.060 | 0.093 | 0.041 |
| 45 | 0.641 | 0.825 | 0.716 |
| 46 | 0.210 | 0.180 | 0.154 |
| 47 | 0.067 | 0.074 | 0.090 |
| 48 | 0.498 | 0.451 | 0.479 |
| 49 | 0.663 | 1.103 | 1.947 |
| 50 | 3.041 | 2.807 | 2.531 |
| 51 | 0.712 | 1.185 | 1.080 |
| 52 | 1.730 | 1.543 | 1.877 |
| 53 | 5.179 | 6.927 | 5.935 |
| 54 | 8.087 | 9.325 | 8.951 |
| 55 | 30.273 | 29.639 | 27.562 |
| 56 | 3.049 | 3.328 | 3.295 |
| 57 | 4.566 | 5.077 | 5.447 |
| 58 | 4.151 | 5.006 | 4.213 |
| 59 | 0.014 | 0.015 | 0.286 |

[全60解](results/results.tsv)・[計測ツールの集計](results/summary.txt)・[合計・4スレッド比較JSON](summary.json)・[機械の情報](machine.txt)。
