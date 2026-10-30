# 通常の探索APIでのFFO比較

旧版の低水準APIとの比較では通常の探索制御を評価できていなかったため、再測定した。

- 旧版: ../deft-reversi-feature f4192a0。Solver::from_fileでeval-v4.bin内の評価器とMPC設定を読み込み、solve(board, 60)を呼ぶ。準備探索・選択性の段階移行を含み、最終結果は完全読み。
- 新版: 同じ重みを変換したモデル、SearchMode::Exact、1スレッド。元の探索制御は未移植のため、同一アルゴリズムの比較ではない。
- CPU 1固定、native release、メインTT 256 MiB、5周交互順。局面ごとに旧版Solverとスレッドを再作成、新版の表を消去。モデル読込・初期化を除く。

| FFO | 旧版 秒 | 新版 秒 | 新版所要時間/旧版 | スコア |
|---|---:|---:|---:|---:|
| 40 | 0.099313 | 0.280604 | 2.825倍 | 38 |
| 41 | 0.227297 | 0.663324 | 2.918倍 | 0 |
| 42 | 0.298547 | 0.673229 | 2.255倍 | 6 |
| 43 | 0.915432 | 2.724666 | 2.976倍 | -12 |
| 44 | 0.257541 | 0.940741 | 3.653倍 | -14 |

合計時間の中央値: 旧版 1.801152秒、新版 5.284167秒。新版は2.934倍の時間を要する。全局面・全周のスコア一致。

以前の「旧版比1.57倍高速」は低水準APIとの比較結果であり、通常利用時の性能の結論として撤回する。

再現コマンド:
```sh
taskset -c 1 tools/engine-compare/target/release/deft-engine-compare ../deft-reversi-feature/data/eval/eval-v4.bin /tmp/deft-pattern.dpr2 deft_reversi_engine_v2/tests/fixtures/ffo40-44.obf level:60 5 1
```
