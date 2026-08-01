# オセロNNUE評価関数 v2 技術仕様

## 1. ネットワーク構造

```
疎バイナリ入力 944,956次元、視点ごと最大255 active
→ FT: input_size × 256、重みint16
→ accumulator: 256 × int32、手番側視点/相手側視点の2本
→ clipped ReLU: clamp(x, 0, 4095)
→ pairwise: 前半128 × 後半128 / 4096。視点2本を連結して256次元
→ phase bucket: min(empties / 8, 7)
→ bucket別 dense tower: 256→32→32→1
→ bucket別 PSQT: (手番側PSQT - 相手側PSQT) / (2 * 1024)
→ dense出力 + PSQT を石差 [-64, +64] 相当へ clamp
```

- activation scale は 4096、weight scale は 4096。
- dense tower は8本。局面の空きマス数から選んだ1本だけを使う。
- PSQT は各 feature に8 bucket分の int16 線形スコアを持つ。

## 2. 入力特徴

視点ごとに own=視点側、opp=相手側として feature ID を作る。

| グループ | 次元 | オフセット | active数 |
|---|---:|---:|---:|
| 石プレーン own64 + opp64 | 128 | 0 | 石数 |
| 合法手プレーン own64 + opp64 | 128 | 128 | 合法手数 |
| 象限空き数 one-hot 17×4 | 68 | 256 | 4 |
| 象限パリティ one-hot 2×4 | 8 | 324 | 4 |
| 全体パリティ one-hot 2 | 2 | 332 | 1 |
| パターン 54インスタンス | 944,622 | 334 | 54 |

`NNUE_INPUT_SIZE = 944,956`、`PADDING_FEATURE_ID = NNUE_INPUT_SIZE`。

## 3. パターン定義

v1 の42インスタンス(P1-P11)の順序は維持し、末尾に次を追加する。

| 名称 | 基準形 | セル数 | 状態数 | インスタンス数 |
|---|---|---:|---:|---:|
| P12 edge+2X | a1,b1,c1,d1,e1,f1,g1,h1,b2,g2 | 10 | 59,049 | 4 |
| P13 corner2×5 横 | a1,b1,c1,d1,e1,a2,b2,c2,d2,e2 | 10 | 59,049 | 4 |
| P14 corner2×5 縦 | a1,a2,a3,a4,a5,b1,b2,b3,b4,b5 | 10 | 59,049 | 4 |

回転は既存規約と同じ90度4方向。3進コードの桁順は基準形のセル列挙順。

## 4. ファイル形式

`nnue-eval.json` v2 は以下を持つ。

- `input_size`, `accumulator_size`, `activation_scale`, `weight_scale`
- `psqt_buckets: 8`, `psqt_scale: 1024`, `tower_count: 8`, `pairwise: true`
- `input_weights`: `input_size × 256`
- `psqt_weights`: `input_size × 8`
- `input_bias`: 256
- `towers`: 8要素。それぞれ `dense_layers` と `output_weights`/`output_bias`

v1 の単一 `dense_layers` / `output_weights` 形式は読み込まない。

## 5. 学習

- LR schedule は `constant` または `cosine`。cosine は warmup 後に `lr * final_lr_ratio` へ到達する。
- train collate では dihedral 8変換による対称性データ拡張を既定で有効にする。valid には適用しない。
- 教師ラベルは `.rd` の最終石差のまま変更しない。
