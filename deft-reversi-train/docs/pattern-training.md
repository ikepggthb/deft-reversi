# パターン評価の学習手順

`deft-reversi-train` を使って、`deft_reversi_engine` のパターン評価モデル（DPR2 形式）を学習・評価・校正し、採用するまでの手順をまとめる。学習器の内部設計やバイナリ形式の詳細は [deft-reversi-train/README.md](../README.md) を参照する。

## 用語と前提

- **`.rd` データセット**：rdg（reversi-dataset-gen）形式のバイナリ局面ファイル群（`phase_<NN>/{train,valid,test}.rd`、`NN` は `石数 - 4` の 2 桁）。1 局面あたり盤面と終局石差を 18 バイトで記録している。
- **フェーズ（段階）**：空きマス数で決まるゲームの進行度（`60 - 空きマス数`、0〜59 の 60 段階）。各段階の重みは独立している。
- **DPR2**：このエンジン専用の評価モデルファイル形式。評価重みと、必要に応じて補助ニューラルネットや ProbCut 校正を 1 ファイルに収める。
- **補助ニューラルネット（`--pen`）**：線形パターン評価を補正する小型ニューラルネット。`train --pen` で線形重みと同時に学習できる。
- **ProbCut / MPC**：浅い探索のスコアから深い探索のスコアを予測し、見込みの薄い枝を確率的に刈る手法。評価関数の精度によって適切な係数が変わるため、モデルごとに実測して校正する。

## 準備

- **データセット**：Egaroucid の学習データから `deft-reversi-train convert` で作る（以前の `egaroucid_rd_data_set` はディレクトリ名が石数の `phase_<石数>` だったため、そのままでは読めない）。
  - v4 モデルでは `train-v4-all` を使った（経緯と比較は研究ノート `local/notes/studies/2026-11-23-v4-eval/` にある）。石数 14〜63 は学習データ v0002 の棋譜からランダム打ち後の全局面（`convert --transcripts … --after-random 0 --stones 12..=63`、約 13.7 億局面）を、石数 4〜13 は 7.4.0 lv.17 の評価を negamax した v0001 の全局面（`convert --boards … --train 1 --valid 0`）を使い、`--no-shrink-stones 4..=13` で縮小せずに書き出している。ラベルは空きマスを勝った側に加える終局石差である。
  - ランダム打ち直後の手を除いたデータ（`--after-random 16 --after-random-ramp 21`）で学習すると中盤の局面数が半分以下になり、v3 より弱く遅くなったため、データは減らさずに使う。
  - 以前の `egaroucid-rd`（v0002 の全局面、空きマスはどちらにも数えない）は v3 まで使った。局面自体は `train-v4-all` の石数 14〜63 と同じである。
- **補助ニューラルネットの追加**：`train --pen` を付けると予測精度が上がる。
- **メモリと所要時間**：学習時は 1 局面 24 バイトを使い、`-t`（スレッド数）で指定した数だけ段階を同時にメモリへ載せる。最大の段階は約 1.2 GB あり、16 スレッドでも約 20 GB に収まる。Core Ultra 7 265K・16 スレッドでは全データ 1 エポック（全データの 1 周）あたり約 2 分 40 秒かかり、データ量の多い石数の段階ほど時間がかかる。
- **ビルド**：

```sh
RUSTFLAGS="-Ctarget-cpu=native" cargo build --release -p deft_reversi_train
T=target/release/deft-reversi-train
D=egaroucid_rd_data_set
```

## 1. 学習する

```sh
$T train --data $D --work runs/NAME/work --out runs/NAME/model.dpr2 --epochs 12 -t 16
```

- `--work` には段階ごとのチェックポイント（`phase_<石数>.ckpt`）と学習ログ（`metrics.jsonl`）が保存される。作業ディレクトリは 1 つの実験設定につき 1 つ用意する。
- 途中で止めても、同じコマンドを実行すればチェックポイントから続きを学習する。中断せずに回した場合とまったく同じモデルになる。
- 完了した学習に対して `--epochs` を増やして再実行すると、そのエポック数まで追加学習する。途中のエポックのモデルも残したいときは、`--epochs 4`、`8`、`12` と順に増やしながら `--out` のファイル名を変えて実行する。
- `--lr`・`--l2`・`--huber-delta`・`--limit`・`--seed` などを変えて同じ `--work` で再開しようとするとエラーになる。設定を変えるときは新しい作業ディレクトリを使う。
- 試し学習には `--limit`（1 段階あたりの train 局面数上限）と `--stones`（例：`--stones 36`、`--stones 30..=40`）を使う。全体の傾向を素早く見たいときは、石数 20・36・52 のように離れた段階をいくつか選ぶ。

## 2. 学習の進み方を見る

`metrics.jsonl` は 1 行が 1 段階の 1 エポックに対応し、`train` と `valid` の Huber 損失（誤差が閾値以下なら二乗、超えたら線形で数える損失）・平均絶対誤差（`mae`：予測と正解のずれの平均）・局面数・秒数を記録している。

```sh
# 段階ごとの valid mae をエポック順に並べる
jq -r 'select(.valid) | [.stones, .epoch, .valid.mae] | @tsv' runs/NAME/work/metrics.jsonl | sort -n -k1 -k2
```

確認するポイント：

- **valid の下がり幅**：前のエポックからの改善が 0.005 石程度まで小さくなれば、それ以上回しても効果は小さい。
- **train と valid の差**：`train` の mae はエポック中の重み更新前の誤差なので、`valid` とほぼ同じなら過学習していない。`valid` が `train` より明らかに大きくなり、`valid` が上がり始めたら回しすぎである。

## 3. 書き出し直す（shrink の調整）

`--shrink` は出現回数が少ない重みを 0（テーブル平均）へ寄せて過学習を抑える強さで、既定値は 32 である。学習し直さずに書き出し（`export`）だけで値を変えて試せる。

```sh
for k in 0 8 32 128; do
  $T export --work runs/NAME/work --out runs/NAME/model-k$k.dpr2 --shrink $k
done
```

## 4. 誤差を比べる

`eval` はエンジンの `PatternModel::evaluate`（整数に丸め、±64 に制限した評価値）と終局石差の MAE を石数ごとに計算する。

`--shrink` の調整などは `valid` 分割で行い、最後に `test` 分割で 1 回だけ比べる。`test` を見ながらパラメータを選ぶと、`test` の数値が実力より良く見えてしまうためである。

```sh
$T eval --split valid -m runs/NAME/model-k8.dpr2 -m runs/NAME/model-k32.dpr2 --data $D
$T eval -m CURRENT.dpr2 -m runs/NAME/model.dpr2 --data $D > eval-test.txt
```

## 5. MPC を校正する

ProbCut の係数は評価関数の精度で変わるため、採用するモデルごとに実測して校正する（設計の詳細は [../README.md#mpc-の校正](../README.md#mpc-の校正)）。

```sh
$T mpc -m runs/NAME/model.dpr2 --data $D --out runs/NAME/mpc --positions 1000 -t 8
```

生成された `runs/NAME/mpc/model.dpr2`（重みと MPC 校正をまとめた 1 ファイル）をそのままエンジンへ渡して使う。途中で止めても同じコマンドで続きから計測できる。すでに `rules.json` がある場合や旧 main の規則を付けたい場合は `pack` サブコマンド（`pack -m モデル --rules rules.json -o 出力` または `--legacy-main`）を使う。

過去のモデルと探索速度・強さを比べるときは、同じ規則どうし、または校正後のモデルと旧 main 規則（`Engine::set_legacy_main_mpc`）を並べて比較する。lr 0.3 のモデルでは、固定レベルの探索が約 6% 速くなり、強さと完全読みの速さは変わらなかった（[記録](https://github.com/ikepggthb/deft-reversi-notes/blob/main/mpc/2026-10-31-mpc-calibration/README.md)）。

## 6. 探索の強さを確かめる

評価誤差が小さいことと、探索で強いことは必ずしも一致しない。モデルを採用する前に次の 2 点を確かめる（計測ツールの詳細は [tools/bench/README.md](../../tools/bench/README.md)）。

- **FFO 40〜59（完全読み）**：[tools/bench/ffo.py](../../tools/bench/ffo.py) と [tools/bench/positions/ffo40-59.obf](../../tools/bench/positions/ffo40-59.obf) を使い、まず最初の 10 問（`--only 40-49`）、次に全問で正解の石差と一致すること、ノード数と所要時間が現行モデルから悪化していないことを確かめる。

```sh
python3 tools/bench/ffo.py --only 40-49 --threads 1 --cpus 2 --rounds 3 --out OUT \
  old:model=CURRENT.dpr2 new:model=runs/NAME/mpc/model.dpr2
```

- **自己対戦（実戦の強さ）**：[tools/bench/match.py](../../tools/bench/match.py) を使い、現行モデルと先手後手を入れ替えて対局する。同じ持ち時間で比べる場合は `level12` の代わりに `time1` などを指定する。

```sh
DEFT_MODEL=runs/NAME/mpc/model.dpr2 OPP_MODEL=CURRENT.dpr2 \
  XOT=large \
  python3 tools/bench/match.py deft level12 250 出力.tsv
```

## 7. 記録する

実験結果は研究ノート `deft-reversi-notes/<テーマ>/<日付>-<名前>/` に残す。

- `README.md`：実行コマンド、設定、実行環境、所要時間、誤差の表、結論
- `metrics.jsonl`：学習の記録
- `eval-test.txt`：`eval` の出力

モデル本体（約 36 MB）はリポジトリに入れず、保管場所を `README.md` に記す。

## 主なパラメータ（`train`）

| オプション | 既定値 | 意味 |
|---|---:|---|
| `--epochs` | 4 | データを何周するか |
| `--lr` | 0.05 | 線形重み（AdaGrad）の学習率（石単位） |
| `--l2` | 1e-4 | 線形重みの L2 正則化係数 |
| `--huber-delta` | 4.0 | これを超える誤差（石数）を線形で数える閾値 |
| `--shrink` | 32.0 | 書き出し時、n 回使われた重みに `n / (n + shrink)` を掛ける（表平均または隣の重みへ寄せる） |
| `--no-shrink-stones` | なし | この石数の段階は縮めずに書き出す（全局面を学習する序盤用） |
| `--no-mirror` | なし | 鏡像どうしで重みを共有しない |
| `--pen` | なし | 線形パターン評価を補う小型ニューラルネットを同時に学習する |
| `--pen-decay` | 0.0 | `--pen` の全結合重みに掛ける減衰（0.01・0.1 では誤差がわずかに増えた） |
| `--seed` | 1 | `--limit` の抽出とシャッフルの乱数シード |
| `--limit` | なし | 1 段階あたりの train 局面数の上限 |
| `-t` / `--threads` | コア数 | 同時に学習する段階の数 |

## 推奨する設定

[2026-10-31 の比較](https://github.com/ikepggthb/deft-reversi-notes/blob/main/evaluation-model/2026-10-31-pattern-lr/README.md)に基づき、本格的な学習には次の設定を勧める。

```sh
$T train --data $D --work runs/NAME/work --out runs/NAME/model.dpr2 --lr 0.3 --epochs 6 -t 16
```

- **`--lr 0.3`**：既定の 0.05 は小さすぎる。AdaGrad は使われるたびに更新幅が小さくなるため、学習率が小さいと最適な値に届かない。lr 0.05 では 12 エポック目でも誤差が下がり続けた一方、lr 0.2〜0.4 では 2 エポックで lr 0.05 の 12 エポックより誤差が小さくなった。0.2〜0.8 の差は 0.015 石以内だが、0.8 では石数の多い段階がわずかに悪化する。
- **`--epochs 6`**：lr 0.2〜0.4 では 4 エポック目以降の改善が 1 エポックあたり約 0.005 石以下に落ち着く。時間に余裕があれば 8 でもよい（全段階・16 スレッドで 1 エポック約 2 分 40 秒）。lr 0.3 の全段階 valid 平均は 4・6・8 エポックで 5.184・5.178・5.175 だった（[記録](https://github.com/ikepggthb/deft-reversi-notes/blob/main/evaluation-model/2026-10-31-pattern-lr0.3/README.md)）。
- **`--shrink`**：0〜32 の差は 0.003 石以内であり、既定の 32 のままでよい。
- **`--l2`・`--huber-delta`**：未検証である。学習率を上げると train と valid の差が少し広がる（lr 0.4 で 0.02〜0.04 石）ため、次に調整するなら `--l2` を強める（1e-3 程度）のを試す。

学習率などの条件を比べるときは、まず石数 20・36・52 だけを学習して様子を見る（各段階 1 スレッド、8 エポックで約 10 分）。

```sh
for lr in 0.2 0.4; do
  for s in 20 36 52; do
    $T train --data $D --work runs/lr$lr --out runs/lr$lr/model-$s.dpr2 --stones $s --epochs 8 --lr $lr -t 1 &
  done
done
wait
```

段階ごとに別プロセスで動かすときは、`--out` のパスを段階ごとに変える。同じファイルへ同時に書き出すと衝突するためである。
