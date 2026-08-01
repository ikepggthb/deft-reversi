# NNUE v2 vs パターン評価 比較実験(2026-07-12〜13)

NNUE v2(特徴拡張+フェーズバケット+ペアワイズ乗算)を学習し、従来のパターン評価と
静的精度・直接対戦の両面で比較した記録。ブランチ: `feature-new-engine`。

## TL;DR

- **NNUE v2(100Mサンプル学習)は、パターン評価のあらゆる学習条件・全49フェーズに対して静的精度で優位**(全体MAE 4.72 vs 最良5.63)
- 直接対戦(level 5、200局)で **勝率 67.8%、平均 +11.2 石**
- この結果を受け、`deft-reversi-cli` のデフォルト評価を NNUE v2 に切替済み
- パターン評価は今回のトレーナーでは全体MAE 5.6 前後で頭打ち(lr調整・データ100M→13.5億・3→8ep いずれも突破できず)。ただし旧 eval.json は中終盤に限りさらに良く(4.7〜4.9)、旧学習レシピには未再現の要素がある(それでも同帯の NNUE 3.8〜4.1 には及ばない)
- 未解決: NNUE は差分更新未実装のため同一レベル探索の思考時間が約44倍(0.23ms vs 10.1ms/手)

## 共通ベンチマーク

- **静的精度**: `deft-reversi-cli eval-mae` — `egaroucid_rd_data_set/phase_N/valid.rd`(N=12..60)の各先頭5,000件 = **245,000局面**に対し `|evaluate_board_slow(board) − 最終石差|` の平均。学習には未使用の valid 分割
- **対戦**: `deft-reversi-cli match` — 同一ランダム序盤(8手)から先後入替の2局を1ペアとして対戦

再現:

```bash
cargo build --release -p deft-reversi-cli
./target/release/deft-reversi-cli eval-mae --eval <評価ファイル> --data egaroucid_rd_data_set --split valid
./target/release/deft-reversi-cli match --eval-a <A> --eval-b <B> --games 100 --level 5 --seed 42
```

## 評価対象と結果(全体MAE)

| 評価関数 | 学習データ / 条件 | 全体MAE | CSV |
|---|---|---|---|
| **NNUE v2** | 100M均等 × 8ep, GPU, cosine LR, 対称性拡張 | **4.719** | eval-mae_nnue-v2-100m.csv |
| 旧 eval.json | 不明(リポジトリ既存、メタデータ空) | 5.631 | eval-mae_old-eval-json.csv |
| パターン 100M lr0.15 | 100M均等 × 8ep | 5.632 | eval-mae_pattern-100m-lr015.csv |
| パターン 100M lr0.3 | 100M均等 × 8ep | 5.676 | eval-mae_pattern-100m-lr03.csv |
| パターン 100M lr0.05 | 100M均等 × 8ep | 5.746 | eval-mae_pattern-100m-balanced.csv |
| パターン 全データ ×8ep | 13.5億 × 8ep, lr0.05 | 5.740 | eval-mae_pattern-full-e8.csv |
| パターン 全データ ×3ep | 13.5億 × 3ep, lr0.05 | 5.897 | eval-mae_pattern-full-e3.csv |

各 CSV は `empties,samples,mae` の行を含む。**フェーズ(石数)= 64 − empties** で読み替え可能
(phase_N ディレクトリ = 石数 N の局面であることは実データで検証済み)。

## 直接対戦

`match --eval-a data/eval/eval.json --eval-b runs/nnue-v2-100m/nnue-eval.bin --games 100 --level 5 --seed 42`

| 指標 | 結果(B = NNUE v2 視点) |
|---|---|
| 勝-分-負 | 134-3-63(200局) |
| 勝率 | 67.8% |
| 平均石差 | +11.2 |
| 平均思考時間 | A: 0.23ms/手、B: 10.1ms/手(**44倍差**、差分更新未実装のため) |

## モデル・学習条件の詳細

### NNUE v2(`runs/nnue-v2-100m/`)

- 入力 944,956 次元(石128+合法手128+象限空き数68+パリティ10+パターン54インスタンス)。
  パターン: v1 の42(2x4, 行1-4, corner3x3, 対角4-8)+ edge+2X ×4 + corner2×5 ×8(3^10)
- FT 256×2視点(回転重み非共有)+ PSQT 8ch → ペアワイズ乗算(128⊙128)→
  dense 32-32-1 ×8フェーズバケット(empties//8)
- 学習: `train_nnue_torch.py`、Huber δ=4.0、cosine LR(lr 4, warmup 1000)、
  対称性データ拡張、TRAIN_LIMIT=100M(フェーズ均等)× 8 epochs、batch 8196、GPU(ROCm)
- 学習時 valid MAE: 4.517(epoch 8、単調改善でプラトーなし)
- 成果物: nnue-eval.bin 514MB、checkpoint 3.0GB

### パターン評価(`runs/pattern-*/`)

- エンジン本体の `PatternEvaluator` と同一構造: 11パターン(6〜10マス、edge+2X・corner2x5系
  を含む)× 4回転(**回転間重み共有**)+ mobility 128 + bias、**60フェーズ全複製**
  ≈ 1,790万パラメータ
- 学習: `deft-reversi-train train pattern`(AdaGrad、L2 1e-7、count-shrink k=32、
  Huber δ=4.0、batch 1024、seed 1)。ラベルは NNUE と同一(手番側最終石差)

## フェーズ別の要点(詳細は各CSV)

- NNUE v2 は全49フェーズ(石数12〜60)で最良
- 差が最大なのは石数41〜50(中終盤、探索が評価を呼ぶ帯)で対旧eval −0.7〜−1.1石
- 旧 eval.json は石数12〜16(序盤)で 7.3〜9.2 と崩壊(序盤データ不足の旧レシピと推測)。
  中終盤は今回の再学習版より良い
- パターン全データ×3ep は石数38〜51 で奇数フェーズだけ悪化する縞模様(収束不足の
  アーティファクト)。8ep で解消(例: empties 19 で 5.98→5.66)
- **未解明**: 旧 eval.json の中終盤(empties 15〜23 で 4.7〜4.9)は、今回の
  どのレシピ(lr 0.05〜0.3、100M〜13.5億、3〜8ep)でも再現できず(最良 5.6 前後)。
  旧レシピはフェーズ毎に収束まで回す等、より長い最適化を行っていたと推測される。
  ただしその帯でも NNUE v2 は 3.8〜4.1 であり、序列には影響しない

## 手番別 MAE(石数パリティによる近似)

.rd レコードに手番の色情報はないため、**石数の偶奇を手番の代理指標**とした
(パスがなければ偶数石=黒番、奇数石=白番。パス発生後はずれるため近似)。
集計データ: `eval-mae_by-side-to-move.csv`、グラフ: `mae_by_side.svg`。

| 評価関数 | 黒番(偶、125,000局面) | 白番(奇、120,000局面) | 差(白−黒) |
|---|---|---|---|
| NNUE v2 | 4.722 | 4.716 | −0.006 |
| パターン lr0.15 | 5.590 | 5.677 | +0.087 |
| 旧 eval.json | 5.707 | 5.553 | −0.154 |

- **NNUE v2 は手番バイアスなし**(差 0.006)。手番側視点の入力表現+対称性拡張が効いている
- パターン lr0.15 は白番がやや苦手(+0.09)、旧 eval.json は逆に黒番が苦手(−0.15)。
  どちらも偶奇フェーズの学習量・収束の非対称に起因するとみられる(全データ×3ep で
  観測された奇数フェーズの縞模様と同根)

## グラフ(論文品質、SVG + PDF)

- `mae_by_phase.{svg,pdf}` — フェーズ(石数)別 MAE 曲線: NNUE v2 / パターン lr0.15 / 旧 eval.json
- `mae_by_side.{svg,pdf}` — 手番別 MAE スロープチャート(数値ラベル付き)
- スタイル: Okabe-Ito 配色(カラーブラインド対応)+マーカー・線種併用(モノクロ印刷可)、
  セリフ体、タイトルなし(キャプション前提)、PDF は LaTeX の `\includegraphics` 用
- Y軸は0始まり
- 再生成(要 matplotlib): `python3 docs/experiments/2026-07-nnue-v2-vs-pattern/make_graphs.py`

## 実験過程で発見・修正した問題

1. **eval-mae の target スケールバグ**(codex 実装時): .rd の value(生の石差)を
   SCORE_SCALE=128 で割っていた → 修正済み(`deft-reversi-cli/src/eval_mae.rs`)。
   修正前の数値(12.7/13.6)は無効
2. **pattern trainer の --train-limit フェーズ偏り**: ファイル順シャッフル+先頭打ち切りの
   ため一部フェーズしか学習されなかった(当初の 100M 比較で MAE 9.34 という壊れた結果)。
   Python 側と同じフェーズ均等配分に修正済み(`pattern_train.rs`)。9.34 は無効データ
3. **self_play モードの既存バグ**(未修正): 「ゲームが終局ではありません」で棋譜0件。
   評価関数と無関係(パターン評価でも再現)。対戦には `match` サブコマンドを使うこと

## 採用状況(2026-07-13)

- `data/eval/nnue-eval.bin` に配置(data/eval/* は gitignore)し、CLI デフォルト評価を
  `../data/eval/nnue-eval.bin` に変更(`deft-reversi-cli/src/main.rs`)
- FFO #1(+18 G8)/#2(+10 A4)正答を確認
- web(WASM)は未対応: 514MB は配布不可。軽量版(回転共有・corner2×5削除・acc128/64、
  gzip後 7〜13MB 目標)を検討中

## 今後の課題

1. エンジンの NNUE 差分更新+SIMD(44倍の思考時間差の解消)— 最優先
2. web 用軽量 NNUE の学習と配信形式(バイナリ+gzip)
3. NNUE の全データ(13.5億)学習 — 100M で
   epoch 8 でも改善が続いていたため、さらの向上余地あり
