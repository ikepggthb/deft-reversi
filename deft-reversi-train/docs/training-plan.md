# Evaluation Training Plan

## Goal

`deft-reversi-train` では、現行の `deft-reversi-engine` の評価関数を学習する。

最初の目標は、今の evaluator 構造をそのまま学習できる最小構成を作ること。

- phase 別 linear evaluator
- pattern weights
- mobility weights
- bias

差分評価、SIMD、MPC 学習は後回しにする。

## Model

評価関数の形は現行 engine に合わせる。

```text
eval(pos) =
  sum(pattern_weights[phase][pattern_index])
  + mobility_weights[phase][mobility_index]
  + bias[phase]
```

局面表現は side-to-move 視点で扱う。  
`Board` は current-player-relative なので、学習器側で黒視点/白視点へ戻さない。

## Target

教師値は強い探索値を使う。

- 中終盤: 深い探索値
- 終盤: 完全読み値

学習内部では扱いやすさのため、教師値は正規化して使う。

```text
target = score / 64.0
```

最終的に engine 用の重みへ書き戻すときだけ整数スケールへ変換する。

## Feature Extraction

feature 抽出は train 側で二重実装しない。  
`deft-reversi-engine` の現行実装をそのまま使う。

1 局面から必要なのは概念的にはこれだけでよい。

```rust
struct SampleFeatures {
    phase: usize,
    pattern_indexes: [u16; N_FEATURES],
    mobility_index: usize,
}
```

dense vector は作らない。  
出現した特徴だけを参照・更新する。

## Optimizer

最初の optimizer は **AdaGrad** を採用する。

理由:

- pattern の出現頻度差が大きい
- 疎特徴と相性がよい
- 頻出 pattern は実効学習率が自然に下がる
- 低頻度 pattern にも更新が入りやすい

更新対象は、各局面で出現した重みだけ。

## Loss

最初の loss は **Huber loss** を採用する。

理由:

- 探索値教師には外れ値が混ざる
- MSE より大差局面に引っ張られにくい
- 小さな石差の精度を保ちやすい

初期値:

```text
delta = 0.2
```

## Regularization

最初は次の 2 つを入れる。

1. **L2 regularization**
2. **count-based shrink**

count-based shrink は学習後に適用する。

```text
w_i <- w_i * count_i / (count_i + k)
```

初期値:

```text
k = 32
```

## Training Pipeline

全体の流れはこうする。

```text
1. 局面を集める
2. 強い探索・完全読みで教師値を付ける
3. Board から feature を抽出する
4. phase 別に AdaGrad + Huber で学習する
5. L2 / count shrink で過学習を抑える
6. 学習結果を EngineFile に書き出す
7. validation と対局勝率で評価する
8. 必要なら新しい教師データで反復する
```

## Dataset

最初は最小構成でよい。

```text
board
score
```

将来的には以下を追加できる。

```text
board
score
source
game_id
ply
```

局面は自然な自己対戦局面だけに偏らせない。  
将来的には次のような局面も混ぜる。

- PV 上の局面
- 候補手の後の局面
- 悪手後の局面
- ランダム序盤からの局面
- 終盤完全読み局面

## Validation

局面 loss だけで判断しない。

見る指標:

- validation Huber / MSE
- phase 別誤差
- empties 別誤差
- 重み分布
- 対局勝率

最終判断は **探索込みの対局勝率** とする。

## Initial Hyperparameters

初期値はこれで始める。

```text
optimizer: AdaGrad
loss: Huber
lr: 0.01
eps: 1e-8
delta: 0.2
l2: 1e-6
count_shrink_k: 32
epochs: 3
shuffle: on
early_stopping: on
```

## Suggested File Layout

`deft-reversi-train` の最初の構成案:

```text
src/
  main.rs
  dataset.rs
  feature.rs
  model.rs
  optimizer.rs
  loss.rs
  train.rs
  output.rs
  validate.rs
```

## Implementation Order

実装順はこれで進める。

```text
1. dataset format を決める
2. Board -> SampleFeatures を作る
3. predict() を作る
4. Huber + AdaGrad 更新を書く
5. phase 別学習を回す
6. EngineFile に書き出す
7. validation loss を見る
8. engine に組み込んで対局比較する
```

## Notes

- feature 抽出の二重実装は避ける
- evaluator 学習と MPC 学習は分ける
- まずは `pattern + mobility + bias` のみを学習対象にする
- 厳密回帰は後回しにする
- 先に動く学習器を作り、その後で局面生成や教師の質を上げる
