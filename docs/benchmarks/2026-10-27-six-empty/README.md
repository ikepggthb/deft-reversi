# 6空きの手順付けの試行

`8ab1682e7a8d26815ce57cff6202702b20ceadf8` の単一スレッド完全読みで、6空きでも `ordered_endgame` を使う変更を試した。その他の探索処理は変更していない。FFO 40〜44では悪化したため本体には採用していない。

| 局面 | 現行中央値 | 試行中央値 | 試行の時間変化 |
|---|---:|---:|---:|
| FFO 40〜44 合計 | 3.319秒 | 3.413秒 | +2.8% |
| FFO 48 | 4.559秒 | 4.334秒 | -4.9% |

各5周、各局面で新プロセス・空のTT、実行順を周ごとに交互にした。すべてのスコアは前後で一致。AMD EPYC 9V74の仮想環境、CPU 0固定、Rust 1.99.1、native release、1スレッド、主TT 256 MiB。仮想環境のため時間変動がある。`deft-six-order.*` と `deft-six-48.*` に全測定・バイナリと入力のハッシュを記録した。

モデルはリポジトリ内の旧 `data/eval/eval.json` を変換した。この試行では現行エンジンの既定MPC設定を使った。後に `main` のソース内に別の終盤MPC係数表があることを確認したため、この結果は `main` の係数を使用した測定ではない。完全読みは最終段階を非選択的に行うが、準備探索と性能には係数が影響し得る。

```sh
python3 tools/convert_legacy_eval.py data/eval/eval.json /tmp/eval-legacy.json
python3 tools/convert_pattern_v2.py /tmp/eval-legacy.json /tmp/eval-main.dpr2
```

元JSONのSHA-256は `d606b0cd67d39aedf6738856b347f1fb5a97ad5fa24f6bfe71101cbff179e1de`、変換したDPR2は `3a07488cc364975eed03fe01c1806a6ff2737e99f5879f70f21dc023d64a6069`。変換ツールは参照元 `feature-new-engine` f4192a0 のものを使用した。60フェーズのパターン・合法手数・定数の全重みが、旧 `eval[move_count % 2][move_count / 2]` の値と一致することを確認した。

Edax 4.7 (`786421f`) と Egaroucid (`2a65c87`) もこの環境でビルドし、FFO 40〜44について1スレッド・レベル60でスコアが一致することを確認した。初回実行には他の処理との競合があり、TTサイズ・計時範囲も統一していないため、3者の性能比較としては扱わない。比較にはそれぞれの評価モデル、TT設定、準備探索、PV、計時範囲を明記し、同一CPUで交互に複数周測定する必要がある。
