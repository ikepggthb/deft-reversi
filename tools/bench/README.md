# 計測ツール

Deft の速さと強さを測るスクリプトです。測った結果は `docs/benchmarks/<日付>-<名前>/` に、README（何を、どのコマンドで測ったか、結果と結論）と結果のデータを置きます。スクリプトは測定のたびに写さず、ここのものを使います。

Linux の `taskset` と Python 3.11 以上の標準ライブラリを使います。

## 準備

```sh
RUSTFLAGS="-Ctarget-cpu=native" CARGO_TARGET_DIR=target-native \
  cargo build --release -p deft_reversi_engine --example solve --example gtp
```

エンジン・モデルの場所は [common.py](common.py) の `DEFAULTS` にあります。どれも `.gitignore` で除外した `local/` の下です（`local/README.md`、ローカルにだけある）。

| キー | 既定の場所 |
|---|---|
| `solve`・`gtp` | `target-native/release/examples/` |
| `model` | `local/release/model-v3-pen/deft-eval-v3-pen.dpr2`（今の Release のモデル） |
| `edax` | `local/edax-4.5.5/bin/lEdax-x86-64-v3`（Edax 4.5.5 の配布版） |
| `egaroucid` | `local/bench-engines/dl-egaroucid-7.8.1/…/Egaroucid_for_Console_clang.out`（Egaroucid 7.8.1、公式の clang の手順でビルド） |

場所を変えるときは `local/bench.toml` に同じキーで書きます（リポジトリからの相対パスか絶対パス）。

## 完全読み：[ffo.py](ffo.py)

FFO 40〜59 を、Deft の設定・Edax・Egaroucid で解いて比べます。1問ごとに新しいプロセスで解き、エンジンの順番を問題と回ごとに回します。石差はすべて正解と照らし合わせ、違えば止まります。

```sh
# 1スレッド（CPU 4）と20スレッドで3エンジン
python3 tools/bench/ffo.py --threads 1 --cpus 4 --rounds 3 --out OUT/t1 deft edax egaroucid
python3 tools/bench/ffo.py --threads 20 --cpus 0-19 --rounds 5 --out OUT/t20 deft edax egaroucid
# Deft の2つのビルドを比べる
python3 tools/bench/ffo.py --threads 20 --cpus 0-19 --rounds 5 --out OUT \
  base:solve=/path/to/solve-base new:solve=/path/to/solve-new
```

Deft の設定は `NAME:key=value,...` で書きます。キーは `solve`（バイナリ）、`model`、`tt`（MiB、既定 256）。大文字のキーは環境変数として渡します（一時的な切り替え用）。`--only 40-49` で一部の問題だけを解きます。

結果は `OUT/results.tsv`（全部の解）と `OUT/summary.txt`（問題ごとの中央値、エンジンごとの合計の中央値と、最初のエンジンに対する比）です。

## 中盤：[midgame.py](midgame.py)

中盤150局面（[positions/mid150.obf](positions/mid150.obf)、空き32・40・48 を各50）を、レベル21 と18 で読む時間と、選んだ手の質を比べます。手の質は、レベル24 の結果（[positions/mid150-level24.tsv](positions/mid150-level24.tsv)、PEN のモデル・MPC 250局面校正）と同じ手を選んだ割合と、石差の差の平均です。

```sh
python3 tools/bench/midgame.py --cpus 2 --rounds 3 --out OUT base:solve=/path/to/solve-base new:solve=/path/to/solve-new
```

## 対局：[match.py](match.py)

Deft（`gtp`）と Edax・Egaroucid・別のモデルの Deft を、GTP で対局させます。開局は XOT から選び、色を入れ替えて2局ずつ打ちます。

```sh
XOT=tools/bench/positions/xot-openingslarge.txt python3 tools/bench/match.py edax level12 250 OUT.tsv
DEFT_MODEL=NEW.dpr2 OPP_MODEL=OLD.dpr2 XOT=tools/bench/positions/xot-openingslarge.txt \
  python3 tools/bench/match.py deft time1 250 OUT.tsv
```

使い方の詳細はファイルの先頭にあります。

## 局面

| ファイル | 中身 |
|---|---|
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
