# パターン評価の学習手順

`deft-reversi-train` でエンジンのパターン評価（DPR2）を学習し、評価し、採用するまでの手順。学習器の設計とファイル形式は [deft-reversi-train/README.md](../deft-reversi-train/README.md) にある。

## 準備

- データセット：`egaroucid_rd_data_set/phase_<石数>/{train,valid,test}.rd`（石数12〜63）。train は約13.5億局面、valid と test は各約1,400万局面。
- メモリ：1局面24バイト。同時に学習する段階の数（`-t`）だけデータを載せる。最大の段階は約1.2 GB で、16スレッドでも約20 GB に収まる。
- 時間：Core Ultra 7 265K・16スレッドで、全データ1エポックあたり約2分40秒。段階ごとの所要時間はデータ量に比例し、石数の多い段階ほど長い。
- ビルド：

```sh
RUSTFLAGS="-Ctarget-cpu=native" cargo build --release -p deft_reversi_train
T=target/release/deft-reversi-train
D=egaroucid_rd_data_set
```

## 1. 学習する

```sh
$T train --data $D --work runs/NAME/work --out runs/NAME/model.dpr2 --epochs 12 -t 16
```

- `--work` には段階ごとのチェックポイント（`phase_<石数>.ckpt`）と `metrics.jsonl` が残る。作業ディレクトリは1つの設定につき1つ使う。
- 中断しても、同じコマンドで続きから学習する。中断せずに学習した場合と同じモデルになる。
- 終わった学習の `--epochs` を増やして実行すると、その続きを学習する。途中のエポック数のモデルも残したいときは、`--epochs 4`、`8`、`12` と順に実行し、`--out` を変える。
- 学習率・L2・Huber の幅・`--limit`・`--seed` を変えて同じ `--work` で再開しようとすると、エラーになる。設定を変えるときは新しい作業ディレクトリを使う。
- 試し学習には `--limit`（1段階あたりの局面数）と `--stones`（例：`--stones 36`、`--stones 30..=40`）を使う。全段階の傾向を見るなら、石数20・36・52 のように離れた段階をいくつか選ぶ。

## 2. 学習の進み方を見る

`metrics.jsonl` は1行が1段階の1エポックで、train と valid の Huber 損失・平均絶対誤差（mae）、局面数、秒数を持つ。

```sh
# 段階ごとの valid mae をエポック順に並べる
jq -r 'select(.valid) | [.stones, .epoch, .valid.mae] | @tsv' runs/NAME/work/metrics.jsonl | sort -n -k1 -k2
```

見るところ：

- **valid の下がり幅**：前のエポックからの改善が 0.005 石程度まで小さくなれば、それ以上回しても効果は小さい。
- **train と valid の差**：train の mae はエポック中の更新前の誤差なので、valid とほぼ同じなら過学習していない。valid が train より明らかに大きくなり、valid が上がり始めたら回しすぎ。

## 3. 書き出し直す（shrink の調整）

`--shrink` は学習し直さずに変えられる。使われた回数が少ない重みを0に寄せる強さで、既定は32。

```sh
for k in 0 8 32 128; do
  $T export --work runs/NAME/work --out runs/NAME/model-k$k.dpr2 --shrink $k
done
```

## 4. 誤差を比べる

調整（shrink の選択など）は valid で行い、最後に test で1回だけ比べる。test を見ながら選ぶと、test の数値が楽観的になる。

```sh
$T eval --split valid -m runs/NAME/model-k8.dpr2 -m runs/NAME/model-k32.dpr2 --data $D
$T eval -m CURRENT.dpr2 -m runs/NAME/model.dpr2 --data $D > eval-test.txt
```

`eval` はエンジンの `PatternModel::evaluate`（整数に丸め、±64に制限した値）と最終石差の平均絶対誤差を石数ごとに出す。

## 5. MPC を校正する

ProbCut の係数は評価の精度で変わるので、採用するモデルごとに測る（[設計](../deft-reversi-train/README.md#mpc-の校正)）。

```sh
$T mpc -m runs/NAME/model.dpr2 --data $D --out runs/NAME/mpc --positions 1000 -t 8
```

できた `runs/NAME/mpc/model.dpr2`（重みと校正の1ファイル）を、そのままエンジンに渡して使う。止めても同じコマンドで続きから測る。比べるときは、同じ規則どうし、または校正後と main の規則（`set_legacy_main_mpc`）を並べる。lr 0.3 のモデルでは、固定レベルの探索が約6%速くなり、強さと完全読みの速さは変わらなかった（[記録](benchmarks/2026-10-31-mpc-calibration/README.md)）。

## 6. 探索の強さを確かめる

誤差が小さいことと探索が強いことは同じではない。採用する前に次を確かめる。

- FFO 40〜49：`tools/bench/positions/ffo40-49-answers.obf` と全問の石差が一致すること、ノード数と時間が今のモデルから悪化していないこと。
- FFO は `tools/bench/compare_versions.py` で測る。`--before`・`--after` には、同じ `solve` にそれぞれのモデルを渡すシェルスクリプトを指定する（[例](benchmarks/2026-10-31-pattern-lr0.3/README.md#ffo-4049)）。
- 自己対戦：今のモデルと、同じレベルで、先手後手を入れ替えて対局する。[benchmarks/2026-11-01-mpc-tails/tools/](benchmarks/2026-11-01-mpc-tails/tools/) の `selfplay` を使う。`selfplay 新モデル[@rules.json] 今のモデル レベル[/ミリ秒] 序盤の数 序盤の手数 スレッド数 seed 出力.tsv`。`@rules.json` を付けると校正した規則と段ごとの倍率、`@rules.json:normal` は規則だけ（倍率は正規分布の値）、付けないと main の規則を使う。`/ミリ秒` を付けると1手ごとに時間を区切る。同じディレクトリの `solve` は、エンジンの `solve` と同じ出力で、モデルを同じ書き方で受け取る。レベル11の1局は1スレッドで約2秒、レベル14は約8秒。

## 7. 記録する

`docs/benchmarks/<日付>-<名前>/` に次を置く。

- `README.md`：コマンド、設定、実行環境、所要時間、誤差の表、結論
- `metrics.jsonl`：学習の記録
- `eval-test.txt`：`eval` の出力

モデル本体（約36 MB）はリポジトリに入れず、置き場所を README に書く。

## パラメータ

| オプション | 既定 | 意味 |
|---|---:|---|
| `--epochs` | 4 | データを何周するか |
| `--lr` | 0.05 | AdaGrad の学習率（石） |
| `--l2` | 1e-4 | 重みに比例して勾配に足す量 |
| `--huber-delta` | 4 | これを超える誤差（石）は線形に数える |
| `--shrink` | 32 | 書き出し時、n 回使われた重みを n / (n + shrink) 倍する |
| `--seed` | 1 | `--limit` の選択とシャッフルの乱数 |
| `--limit` | なし | 1段階あたりの train 局面数の上限 |
| `-t` | コア数 | 同時に学習する段階の数 |

## 推奨する設定

[2026-10-31 の比較](benchmarks/2026-10-31-pattern-lr/README.md)から、本格的な学習には次を使う。

```sh
$T train --data $D --work runs/NAME/work --out runs/NAME/model.dpr2 --lr 0.3 --epochs 6 -t 16
```

- **`--lr 0.3`**：既定の 0.05 は小さすぎる。AdaGrad は使われるたびに歩幅が小さくなるので、学習率が小さいと最適な値に届かない。lr 0.05 では12エポック目でも誤差が下がり続けた。一方 lr 0.2〜0.4 では、2エポックで lr 0.05 の12エポックより誤差が小さくなった。0.2〜0.8 の差は 0.015 石以内で、0.8 では石数の多い段階がわずかに悪くなる。
- **`--epochs 6`**：lr 0.2〜0.4 では、4エポック目以降の改善は1エポックあたり約 0.005 石以下。時間に余裕があれば8でもよい。全段階・16スレッドで1エポック約2分40秒。lr 0.3 の全段階の valid 平均は 4・6・8エポックで 5.184・5.178・5.175 だった（[記録](benchmarks/2026-10-31-pattern-lr0.3/README.md)）。
- **`--shrink`**：0〜32 の差は 0.003 石以内。既定の32のままでよい。
- **`--l2`・`--huber-delta`**：まだ比べていない。学習率を上げると train と valid の差が少し広がる（lr 0.4 で 0.02〜0.04 石）。次に調整するなら L2 を強める（1e-3 程度）のを試す。

学習率などを変えるときは、まず石数20・36・52 だけを学習して比べる（各段階1スレッド、8エポックで約10分）。

```sh
for lr in 0.2 0.4; do
  for s in 20 36 52; do
    $T train --data $D --work runs/lr$lr --out runs/lr$lr/model-$s.dpr2 --stones $s --epochs 8 --lr $lr -t 1 &
  done
done
wait
```

段階ごとに別のプロセスで動かすときは、`--out` を段階ごとに変える。同じファイルに同時に書き出すと衝突する。
