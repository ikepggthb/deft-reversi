# Deft Reversi Web v2

`feature/web-ui` のUIを v2 エンジンに接続したブラウザ版です。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked
wasm-pack build deft-reversi-wasm --target web --release --out-dir ../deft-reversi-web/app/pkg
cd deft-reversi-web/app
python3 -m http.server 8000
```

http://localhost:8000/ を開いてください。評価モデルは初回に読み込まれ、Web Worker内で探索します。gzip解凍には `DecompressionStream` を利用します。

```sh
cd deft-reversi-web/app
node --test
```
