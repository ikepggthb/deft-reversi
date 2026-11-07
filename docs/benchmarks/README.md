# 測定の記録

測定ごとに `<日付>-<名前>/` のフォルダを作り、README（何を、どのコマンドで、どの機械・コミット・モデルで測ったか、結果と結論）と結果のデータを置く。計測スクリプトは写さず [tools/bench](../../tools/bench/README.md) のものを使う。

2026-11-07 に整理し、各フォルダの計測スクリプト・一回限りのツール・標準エラー出力・逆アセンブル・テストやビルドのログなどを削除した。整理前のファイルは `git show 04c2ecb:docs/benchmarks/<フォルダ>/<ファイル名>` で見られる。

| フォルダ | 内容 |
|---|---|
| [2026-10-24](2026-10-24/README.md) | リライト検証・性能測定（2026-10-24） |
| [2026-10-24-endgame-restore](2026-10-24-endgame-restore/README.md) | 元実装を参照した終盤探索の復元 |
| [2026-10-24-ffo40-49](2026-10-24-ffo40-49/README.md) | FFO 40〜49 単スレッド比較 |
| [2026-10-24-model-root-tt](2026-10-24-model-root-tt/README.md) | モデル・MPC・ルート探索・置換表の復元 |
| [2026-10-24-public-solve](2026-10-24-public-solve/README.md) | 通常の探索APIでのFFO比較 |
| [2026-10-24-regression-investigation](2026-10-24-regression-investigation/README.md) | 復元後の終盤探索が遅くなった原因の調査 |
| [2026-10-24-single-thread](2026-10-24-single-thread/README.md) | 単一スレッド性能の調査と改善 |
| [2026-10-27-endgame-implementation](2026-10-27-endgame-implementation/README.md) | 2026-10-27 終盤探索の実装と比較 |
| [2026-10-27-endgame-source-review](2026-10-27-endgame-source-review/README.md) | 2026-10-27 終盤探索のソース比較 |
| [2026-10-27-last-empty-optimization](2026-10-27-last-empty-optimization/README.md) | 2026-10-27 最終1マスと安定石表参照の最適化 |
| [2026-10-27-legacy-mpc](2026-10-27-legacy-mpc/README.md) | 旧mainのMPCと1コア終盤測定 |
| [2026-10-27-local-tt-best](2026-10-27-local-tt-best/README.md) | 局所TT候補手保存の前後比較 |
| [2026-10-27-ordering-reuse](2026-10-27-ordering-reuse/README.md) | 2026-10-27 評価と手順付けの合法手共有、14→13マスの評価状態更新 |
| [2026-10-27-post-leaf-source-review](2026-10-27-post-leaf-source-review/README.md) | 2026-10-27 最終1マスの改善後に残る実装差 |
| [2026-10-27-remaining-source-review](2026-10-27-remaining-source-review/README.md) | 2026-10-27 最適化後に残る終盤探索の実装差 |
| [2026-10-27-review-fixes](2026-10-27-review-fixes/README.md) | 2026-10-27 ソース・構造レビュー後の修正 |
| [2026-10-27-search-reuse](2026-10-27-search-reuse/README.md) | 探索中の計算再利用の試行 |
| [2026-10-27-six-empty](2026-10-27-six-empty/README.md) | 6空きの手順付けの試行 |
| [2026-10-27-tail-kernels](2026-10-27-tail-kernels/README.md) | 2026-10-27 残り5〜6マスの探索カーネル |
| [2026-10-27-three-engines](2026-10-27-three-engines/README.md) | 2026-10-27 現deft・Edax・Egaroucidの1コア終盤比較 |
| [2026-10-28-edax-gap](2026-10-28-edax-gap/README.md) | 2026-10-28（続き） Edaxとの差を縮める改善 |
| [2026-10-28-engine-gap](2026-10-28-engine-gap/README.md) | 2026-10-28 Edax・Egaroucidとの1コア終盤差の調査と改善 |
| [2026-10-28-multithread](2026-10-28-multithread/README.md) | 2026-10-28 マルチスレッド完全読みの計測とEdax・Egaroucidとの比較 |
| [2026-10-29-book](2026-10-29-book/README.md) | book実装の検証（2026-10-29） |
| [2026-10-29-edax](2026-10-29-edax/README.md) | Edax and Deft comparison |
| [2026-10-29-local-baseline](2026-10-29-local-baseline/README.md) | 2026-10-29 ローカルPCでの基準値（1・2・4・8スレッド、deft・Edax・Egaroucid） |
| [2026-10-29-midgame-edax](2026-10-29-midgame-edax/README.md) | Midgame fixed-depth comparison |
| [2026-10-29-mt-overhead](2026-10-29-mt-overhead/README.md) | 2026-10-29 マルチコア完全読みの遅い原因の特定と改善 |
| [2026-10-29-multithread-splits](2026-10-29-multithread-splits/README.md) | 2026-10-29 マルチスレッド完全読みの改善（YBWC分割の再試行） |
| [2026-10-29-refactor](2026-10-29-refactor/README.md) | リファクタリングの確認（2026-10-29） |
| [2026-10-30-pattern-train](2026-10-30-pattern-train/README.md) | パターン評価の学習器（v2）の初回学習 |
| [2026-10-31-mpc-calibration](2026-10-31-mpc-calibration/README.md) | 新しいモデルの MPC 校正 |
| [2026-10-31-pattern-lr](2026-10-31-pattern-lr/README.md) | パターン学習のエポック数と学習率 |
| [2026-10-31-pattern-lr0.3](2026-10-31-pattern-lr0.3/README.md) | パターン評価の学習（lr 0.3・8エポック） |
| [2026-10-31-time-match](2026-10-31-time-match/README.md) | 時間をそろえた自己対戦 |
| [2026-11-01-ffo-engines](2026-11-01-ffo-engines/README.md) | FFO 40〜49：Deft・Edax・Egaroucid（i3-14100） |
| [2026-11-01-mpc-formulas](2026-11-01-mpc-formulas/README.md) | MPC の式の比較 |
| [2026-11-01-mpc-models](2026-11-01-mpc-models/README.md) | MPC の統計モデルの再検討 |
| [2026-11-01-mpc-tails](2026-11-01-mpc-tails/README.md) | MPC の裾の重さの修正 |
| [2026-11-02-exact-mpc-formula](2026-11-02-exact-mpc-formula/README.md) | 完全読みの MPC 規則を式で表し、浅い探索の深さを選び直す |
| [2026-11-03-endgame-efficiency](2026-11-03-endgame-efficiency/README.md) | 完全読みの探索効率：Edax・Egaroucid との差の調査（2026-11-03） |
| [2026-11-03-endgame-large-empties](2026-11-03-endgame-large-empties/README.md) | 空きの多い完全読みが遅い原因の調査（2026-11-03） |
| [2026-11-03-engine-match](2026-11-03-engine-match/README.md) | Edax・Egaroucid との対局（2026-11-03〜04） |
| [2026-11-03-ffo-engines](2026-11-03-ffo-engines/README.md) | FFO 40〜59：Edax・Egaroucid との比較（2026-11-03） |
| [2026-11-03-midgame-speed](2026-11-03-midgame-speed/README.md) | 中盤探索の高速化（2026-11-03） |
| [2026-11-05-endgame-mpc-quality](2026-11-05-endgame-mpc-quality/README.md) | 選択的な終盤探索（98%）の悪手を減らす（2026-11-05） |
| [2026-11-06-ffo-pen](2026-11-06-ffo-pen/README.md) | FFO 40〜59：PEN と Edax・Egaroucid（2026-11-06） |
| [2026-11-06-pen](2026-11-06-pen/README.md) | パターン埋め込みネット（PEN）（2026-11-06） |
| [2026-11-06-pen-tuning](2026-11-06-pen-tuning/README.md) | 探索の値を PEN に合わせ直す（2026-11-06） |
| [2026-11-07-ffo-engines](2026-11-07-ffo-engines/README.md) | FFO 40〜59：1スレッドと20スレッドで Edax・Egaroucid と比較（2026-11-07） |
| [2026-11-07-mpc-1000](2026-11-07-mpc-1000/README.md) | MPC を1,000局面で校正したモデルの速さ（2026-11-07） |
| [2026-11-07-parallel](2026-11-07-parallel/README.md) | 多スレッドの完全読み：分割の下限をスレッド数と段階で変える（2026-11-07） |
