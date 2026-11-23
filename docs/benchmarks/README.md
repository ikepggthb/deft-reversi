# 測定の記録

測定ごとに `<日付>-<名前>/` のフォルダを作り、README（何を、どのコマンドで、どの機械・コミット・モデルで測ったか、結果と結論）と結果のデータを置く。計測スクリプトは写さず [tools/bench](../../tools/bench/README.md) のものを使う。

対局ごと・着手ごとの生ログ（`logs/`、対局の進行を書いた `run.log`、展開した生CSVやそのZIP）は git で管理しない。README の数字のもとになる集計（`games.tsv`・`summary.json` など）だけを置き、生ログは手元の `local/bench-runs/` に置く（`logs/` は `.gitignore` で除外している）。

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
| [2026-11-08-midgame-mpc](2026-11-08-midgame-mpc/README.md) | 空き19以上のMPC選択確率：予備測定・600局、6候補の総当たり、77〜95%の実測倍率による再比較 |
| [2026-11-09-midgame-mpc-79-87](2026-11-09-midgame-mpc-79-87/README.md) | 中盤MPC 79〜87%、深さ＝空き数で終盤99→100%、2,000局 |
| [2026-11-09-endgame-mpc](2026-11-09-endgame-mpc/README.md) | 終盤MPCの確率と中盤83%の深さ：空き14〜24の着手精度と時間 |
| [2026-11-09-level-endgame](2026-11-09-level-endgame/README.md) | レベル表の終盤への切り替え：空き数−レベルの規則（1〜10倍）と旧表の対局、レベル12・16・20付近 |
| [2026-11-15-level-table](2026-11-15-level-table/README.md) | レベル表1〜60の決定：局面ごとの損失と石の値段、旧表・Edax・Egaroucid との対局、L1〜10 の形、L21〜60 の規則 |
| [2026-11-16-ffo-latest](2026-11-16-ffo-latest/README.md) | FFO40〜59：Deft・Edax 4.5.6・Egaroucid Console 7.8.2、i3-14100で1スレッド・4物理コア比較 |
| [2026-11-16-ffo-console-4core](2026-11-16-ffo-console-4core/README.md) | CLI経由のFFO40〜59：4物理コア、各1回、Edax 4.5.6・Egaroucid Console 7.8.2と比較 |
| [2026-11-16-ffo-console-8thread](2026-11-16-ffo-console-8thread/README.md) | CLI経由のFFO40〜59：8スレッド（4物理コア＋SMT）、各1回、4スレッドとの比較 |
| [2026-11-16-ffo33-51-console-8thread](2026-11-16-ffo33-51-console-8thread/README.md) | CLI経由のFFO33〜51：8スレッド（4物理コア＋SMT）、各1回、全57解が正解と一致 |
| [2026-11-16-edax456-audit](2026-11-16-edax456-audit/README.md) | Edax 4.5.6の設定確認、公式配布版・GCC/Clang native再ビルドの比較、TT容量の確認 |
| [2026-11-20-game-clock](2026-11-20-game-clock/README.md) | 持ち時間制対局（FlipSight / GTP）の時間管理最適化と644局の直接対局評価 |
| [2026-11-20-game-clock-v2](2026-11-20-game-clock-v2/README.md) | 60秒対局を見据えた `reuse_pv` 導入と不安定度減衰の256局独立評価 |
| [2026-11-20-time-control-forecast](2026-11-20-time-control-forecast/README.md) | 持ち時間制：`Progress::next` による次の反復の予測と `a21b8c9` との評価（9条件・2,628局、時間切れ0、捨てた時間 5.0%→0.5%） |
| [2026-11-20-time-allocation-loss-table](2026-11-20-time-allocation-loss-table/README.md) | 持ち時間の配分：20〜48空き各64局面の損失の表と最適配分の計算（交差検証で今の配分との差は見えず） |
| [2026-11-20-time-stopping-oracle](2026-11-20-time-stopping-oracle/README.md) | 持ち時間制：反復の時間が既知の場合の打ち切り規則の比較（264局×10局面、価値÷時間の規則と今の方式の差は見えず、最善手変化による延長は有効） |
| [2026-11-20-time-prediction-features](2026-11-20-time-prediction-features/README.md) | 持ち時間制：時間管理の各予測に効く特徴量の分析（probe 2,639局面と対局ログ。中盤→中盤の今の式は定数より外れる、予約は実際の3〜10倍、価値は最善手の変化以外ほぼ効かない） |
| [2026-11-23-time-control-stage3](2026-11-23-time-control-stage3/README.md) | 持ち時間制：段階3-1 のデータ生成（ログ付き自己対局 v4 1,000局・延長モード 1,200手番、時間切れ0。v3 と v4 で反復の倍率とノード速度に差は見えず、延長の ENTRY は約7割が打ち切り） |
