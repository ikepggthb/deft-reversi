# deft-reversi-cli

`deft_reversi_engine` と `deft_reversi_book` を使う端末アプリ。対局・局面の読み・自己対戦・perft の4つのサブコマンドを持つ。

モデルは `--model FILE` か環境変数 `DEFT_MODEL` で指定する。`--threads` を省くと全コアを使い、`--tt-mib` の既定は 256。

```sh
cargo build --release -p deft-reversi-cli
export DEFT_MODEL=path/to/model.dpr2

deft-reversi-cli play --book openings.dbook --level 18
deft-reversi-cli solve ../tools/bench/positions/ffo40-49-answers.obf --threads 8
deft-reversi-cli self-play 100 --level 16 --random 20 --out games.txt
deft-reversi-cli perft 11
```

探索の強さは `--level N`・`--depth N`・`--exact` のどれか1つで指定し、`--time-ms` で1回の探索の時間を区切る。どれも指定しなければ、play はレベル 10、solve は完全読み、self-play はレベル 16 で読む。探索中に Ctrl-C を押すとその探索を止める。play と solve は止めた時点の最善手を使い、self-play は書き終えた対局までで終わる。

## play

既定（`--mode 0`）では人が黒、エンジンが白を持つ。盤面の `*` は合法手で、パスは自動で打つ。`--book FILE` を渡すと、定石に使える手があればその手を打ち、無ければ探索する。`--book-loss N` を付けると、最善から N 石以内の定石手を無作為に選ぶ。

エンジンが探索した手では、反復ごとの深さ・評価値・ノード数・PV と、最後に選んだ手の PV を表示する。

| コマンド | 内容 |
|---|---|
| `f5` | 着手 |
| `play f5d6c3` | 棋譜を続けて打つ |
| `undo` / `redo` | 1手戻す（エンジンの手番も飛ばして人の手番まで戻す）／やり直す |
| `go` | エンジンに今の手番を打たせる |
| `new` | 新しい対局（置換表も消す） |
| `mode 0-3` | 0 人が黒、1 人が白、2 エンジン同士、3 人同士 |
| `level N` / `depth N` / `exact` / `time MS` | 読みの設定。`time 0` で時間制限なし |
| `book on` / `book off` | 定石を使うか |
| `help` / `quit` | |

## solve

1行に1局面（64マスと手番）を読む。`; A2:+38; C7:+36; ...` のように正解が続く行では、評価値と、最善の値に届く手を選んだかを照らし合わせ、`ok` か `NG` を出す。`NG` が1つでもあれば終了コード 1 で終わる。各行に評価値・手・種別（`d12@95%`・`exact` など）・ノード数・NPS・時間・PV を出す。既定では局面の間で置換表を残す。`--clear-tt` を付けると局面ごとに消す。

## self-play

各対局の最初の `--random` 手を無作為に打ち、残りをエンジンが打つ。1行1局の棋譜（`f5d6...`）を `--out` に書く。`--seed` を付けると序盤が再現できる。

## perft

初期局面から深さ 1〜N の葉の数・パス・終局の数を出す。既定ではパスを手数に数えない。`--count-pass` で、パスを1手として数えるエンジンの `perft` と同じ数え方になる。
