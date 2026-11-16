# 計測ツール

Deft の速さと強さを測るスクリプトです。測った結果は `docs/benchmarks/<日付>-<名前>/` に、README（何を、どのコマンドで測ったか、結果と結論）と結果のデータを置きます。スクリプトは測定のたびに写さず、ここのものを使います。

性能・局面解析・対局の測定は `deft-reversi-cli` の公開コマンドを使います。CLIに必要な出力や機能がなければCLIに追加し、エンジンのexampleを計測用の入口には使いません。過去の記録の実行条件は当時のものとして保持します。

Linux の `taskset` と Python 3.11 以上の標準ライブラリを使います。

## 準備

```sh
RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native \
  cargo build --locked --release -p deft-reversi-cli
```

エンジン・モデルの場所は [common.py](common.py) の `DEFAULTS` にあります。どれも `.gitignore` で除外した `local/` の下です（`local/README.md`、ローカルにだけある）。

| キー | 既定の場所 |
|---|---|
| `solve`・`gtp` | `target-native/release/deft-reversi-cli`（それぞれ `solve --tsv`・`gtp` を呼ぶ） |
| `model` | `local/release/model-v3-pen-mpc2/deft-eval-v3-pen.dpr2`（今の Release のモデルと同じ重み・規則で、校正を今の形式にしたもの） |
| `edax` | `local/bench-engines/edax-4.5.6/bin/lEdax-x86-64-v3`（Edax 4.5.6 の配布版） |
| `egaroucid` | `local/bench-engines/Egaroucid-console_v7.8.2/bin/Egaroucid_for_Console.out`（Egaroucid 7.8.2、公式の clang の手順でビルド） |

`solve`・`gtp` の値はどちらもCLIバイナリのパスです。`solve --tsv` は秒数を小数点以下9桁で出し、通常表示のミリ秒への丸めを避けます。探索の完了・深さ・完全読みの種別も出力し、不完全なFFO結果は計測ツールが拒否します。

場所を変えるときは `local/bench.toml` に同じキーで書きます（リポジトリからの相対パスか絶対パス）。

## 完全読み：[ffo.py](ffo.py)

FFO 40〜59 を、Deft の設定・Edax・Egaroucid で解いて比べます。1問ごとに新しいプロセスで解き、エンジンの順番を問題と回ごとに回します。石差はすべて正解と照らし合わせ、違えば止まります。

```sh
# 1スレッド（CPU 2）と4物理コアで3エンジン
python3 tools/bench/ffo.py --threads 1 --cpus 2 --rounds 3 --out OUT/t1 deft edax egaroucid
python3 tools/bench/ffo.py --threads 4 --cpus 0,2,4,6 --rounds 3 --out OUT/t4 deft edax egaroucid
# Deft の2つのビルドを比べる
python3 tools/bench/ffo.py --threads 4 --cpus 0,2,4,6 --rounds 3 --out OUT \
  base:solve=/path/to/cli-base new:solve=/path/to/cli-new
```

Deft の設定は `NAME:key=value,...` で書きます。キーは `solve`（バイナリ）、`model`、`tt`（MiB、既定 256）。大文字のキーは環境変数として渡します（一時的な切り替え用）。`--only 40-49` で一部の問題だけを解きます。

結果は `OUT/results.tsv`（全部の解）と `OUT/summary.txt`（問題ごとの中央値、エンジンごとの合計の中央値と、最初のエンジンに対する比）です。

## 中盤：[midgame.py](midgame.py)

中盤150局面（[positions/mid150.obf](positions/mid150.obf)、空き32・40・48 を各50）を、レベル21 と18 で読む時間と、選んだ手の質を比べます。手の質は、レベル24 の結果（[positions/mid150-level24.tsv](positions/mid150-level24.tsv)、PEN のモデル・MPC 250局面校正）と同じ手を選んだ割合と、石差の差の平均です。

```sh
python3 tools/bench/midgame.py --cpus 2 --rounds 3 --out OUT base:solve=/path/to/cli-base new:solve=/path/to/cli-new
```

## 対局：[match.py](match.py)

Deft（CLIの `gtp` サブコマンド）と Edax・Egaroucid・別のモデルの Deft を、GTP で対局させます。開局は XOT から選び、色を入れ替えて2局ずつ打ちます。

```sh
XOT=tools/bench/positions/xot-openingslarge.txt python3 tools/bench/match.py edax level12 250 OUT.tsv
DEFT_MODEL=NEW.dpr2 OPP_MODEL=OLD.dpr2 XOT=tools/bench/positions/xot-openingslarge.txt \
  python3 tools/bench/match.py deft time1 250 OUT.tsv
```

別のビルドの Deft を相手にするときは、`OPP_GTP` にそのCLIバイナリを指定します。`MATCH_THREADS=N` で各エンジンを N スレッドで動かします（1局ずつ）。使い方の詳細はファイルの先頭にあります。

## 局面

| ファイル | 中身 |
|---|---|
| [ffo33-51.obf](positions/ffo33-51.obf) | FFO 33〜51 と全部の手の正解（`--positions FILE --first 33` で指定） |
| [ffo40-59.obf](positions/ffo40-59.obf) | FFO 40〜59 と全部の手の正解 |
| [mid150.obf](positions/mid150.obf) | 中盤150局面 |
| [mid150-level24.tsv](positions/mid150-level24.tsv) | 中盤150局面のレベル24 の結果（手の質の基準） |
| [xot-openingslarge.txt](positions/xot-openingslarge.txt) | XOT の開局（large） |

## 決まり

- 時間の比較は、同じ機械・同じスレッド数・同じ TT で測った値どうしだけで行う。
- 時間を測っている間は、ビルドやほかの重い処理を動かさない。測定は1つずつ順に動かす。
- 結果は `docs/benchmarks/<日付>-<名前>/` に置く。README には、使ったコマンド、機械、コミット、モデルを書く。

## ツール自身の確認

```sh
python3 -m unittest discover -s tools/bench/tests
```
