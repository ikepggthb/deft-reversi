# Deft Reversi Web v2

`feature/web-ui` のUIを v2 エンジンに接続したブラウザ版です。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked
wasm-pack build deft-reversi-wasm-engine --target web --release --out-dir ../deft-reversi-web/app/pkg
python3 deft-reversi-web/scripts/build-model.py
cd deft-reversi-web/app
python3 -m http.server 8000
```

http://localhost:8000/ を開いてください。評価モデルは初回に読み込まれ、Web Worker内で探索します。gzip解凍には `DecompressionStream` を利用します。

```sh
cd deft-reversi-web/app
node --test
```

GitHub Actions は `feature/web-v2` の更新時に `gh-pages` へビルドを公開します。

## FFOベンチマーク

`benchmark/` でFFO 1〜79を測定できます。完全読み、深さ・レベル指定、局面ごとの時間上限、CSV保存に対応しています。正解データを収録する1〜49では完全読み完了時に照合します。各局面の前に置換表を初期化し、その時間を探索時間から除きます。
