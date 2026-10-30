# WASM手動SIMD・終盤処理の比較（2026-10-30）

FFO 40〜49を終局まで完全読みし、**10問の探索時間の合計**を比較した。正解照合と総ノード数を全回確認し、時間制限は設けていない。時刻はエンジンの `elapsedMs` の合計で、モデル読込と各局面前の置換表初期化は含まない。主TTは32 MiB、1スレッド。各局面の前に `clearTt` で表を初期化した。

## Chrome Web Workerでの最終コード

Chromium 154.0.8037.57 headless、AMD EPYC 9V74、CPU 0に固定。Rust 1.99.1のrelease、`+simd128`、wasm-bindgen 0.2.129、Binaryen 125の `wasm-opt -O` を使用。評価モデルは配布版DPR2で、gzipのSHA-256は `fbfee9c45cecb2b8bf792528163865aabeeede96ba596cba9cc2d235bc89415d`。

2回の測定で、それぞれ従来→改良→改良→従来の順に実行した。新しいWeb Workerとエンジンを各回作成し、ブラウザ上の同じモデルを使った。実行順と各問題の結果は [1回目](results/chrome-final.jsonl)・[2回目](results/chrome-final2.jsonl)に記録している。

| 実行の組 | 従来版：10問合計 | 改良版：10問合計 | 短縮率 |
|---|---:|---:|---:|
| 1回目・前半 | 37.180秒 | 33.854秒 | 8.94% |
| 1回目・後半 | 34.138秒 | 34.123秒 | 0.04% |
| 2回目・前半 | 35.230秒 | 33.616秒 | 4.58% |
| 2回目・後半 | 35.425秒 | 33.448秒 | 5.58% |

全回で各問の最善手・スコア・ノード数が一致し、10問合計は627,213,734ノードだった。実行時間には変動があり、端末ごとの性能や棋力向上を保証するものではない。2組目の差は非常に小さく、測定誤差と区別できない。

終盤処理のみの版と、それに合法手生成のSIMD化を加えた版も、同じ条件で終盤のみ→組み合わせ→組み合わせ→終盤のみの順に比較した。10問の合計はそれぞれ33.420秒・33.430秒・33.201秒・33.324秒で、今回の計測では合法手生成SIMD単独の明確な効果は確認できない。各問の結果とノード数は一致した（[生データ](results/chrome-components.jsonl)）。

WASMバイナリのSHA-256は、従来版 `b845950af80793de27c1f11ab680adcd0febeb855833830d1fb1f2de9c8ddba5`、最終改良版 `f90b242fdfac38a5b354a53a0fd624181f4ab39f88635dc43b37c3b1739dce83`。従来版のソースは `0b5519f16c97fef99eff87c3e8f1832c36f051af`。

## 試した実装

- 合法手生成をSIMD化し、左右の方向を2レーンで処理する案。最初のスカラー `reverse_bits` を使う案は深さ指定の探索で改善が見えなかったため、WASMの8ビットshuffleを使う反転に変更した。合法手は100,000通りの重複しないランダム盤面で従来実装と一致。
- 石を返す一般処理をSIMD化する案。方向の組み合わせ方や終盤だけ従来処理に切り替える案も試したが、一部の終盤局面では遅くなった。Node.jsでの完全読み10問合計も、[従来版34.574秒](results/ffo-baseline-1.jsonl)に対して[中盤に限る案34.660秒](results/ffo-midgame-1.jsonl)となったため採用しなかった。これは`wasm-opt`適用前の探索的な1回ずつの計測。
- 残り1マスの反転数を方向別のbitboardで求める案。約256万通りの盤面で一般の反転処理と一致。上位ビットの位置を`leading_zeros`で求める実装と、シフトを繰り返して下位ビットを埋める実装を比較し、前者を採用した。後者は[完全読み10問で34.373秒](results/ffo-last-fill-opt-1.jsonl)だった。比較案は同じ `wasm-opt` 条件だが、実行時間の変動を含み、単独の測定から原因を断定しない。
- 評価特徴の差分更新については、既存のRustコードがすでにベクトル化されていることを確認し、手書きSIMDには置き換えていない。

Node.js v24.19.0での探索的な完全読み10問の生データも [results](results/) に保存した。実装の寄与とばらつきを確認できるよう、合法手のみ・終盤のみ・組み合わせの結果を分けている。

## 再現

1. 配布用のモデルを `python3 deft-reversi-web/scripts/build-model.py` で生成する。
2. 従来コミットと改良版をそれぞれ同じRust設定で `cargo build --release --locked --target wasm32-unknown-unknown -p deft-reversi-wasm-engine` し、各WASMに `wasm-bindgen --target web` と `wasm-opt -O --enable-simd --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int` を適用する。別の `CARGO_TARGET_DIR` を使う。
3. 任意のHTTP配信ディレクトリに `baseline/` と `candidate/` を作り、それぞれに生成された `deft_reversi_wasm_engine.js` と `_bg.wasm` を配置する。同ディレクトリに評価モデルを `model.gz` としてコピーし、`index.html` を置いて `python3 -m http.server 8770` で配信する。
4. リポジトリのルートから、Playwrightを利用できる環境で `CHROMIUM_PATH=/path/to/chromium taskset -c 0 node tools/bench/wasm_ffo40_49_browser.cjs http://127.0.0.1:8770/` を実行する。全問を解けなかった場合や、各回の最善手・スコア・ノード数が違った場合はエラーにする。

Node.jsだけで確認する場合は、各バイナリを `wasm-bindgen --target nodejs` で生成し、`taskset -c 0 node tools/bench/wasm_ffo40_49.cjs /absolute/path/to/deft_reversi_wasm_engine.js` をそれぞれ実行する。どちらのスクリプトも各問題と10問合計をJSON Linesで出力する。
