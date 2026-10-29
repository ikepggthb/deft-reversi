# deft-reversi-wasm

v2 の `deft_reversi_engine` をブラウザから利用するための独立したクレートです。
旧 `deft-reversi-web` は legacy API 用のため、そのまま残しています。

## ビルド

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack --locked
wasm-pack build deft-reversi-wasm --target web --release
```

生成物は `deft-reversi-wasm/pkg` に出力され、JavaScript と TypeScript の宣言を含みます。
評価モデルはバンドルしません。v3/v4 JSON・bincode または DPR2 のモデルを別途配信してください。

## 利用例（module Web Worker 内）

```js
import init, { WasmEngine, WasmPosition } from './pkg/deft_reversi_wasm.js';

await init();
const response = await fetch('/models/eval-v4.bin');
if (!response.ok) throw new Error(`Model download failed: ${response.status}`);
const engine = new WasmEngine(new Uint8Array(await response.arrayBuffer()), 32);
const position = new WasmPosition();
try {
  const result = engine.search(position, {
    goal: { kind: 'depth', value: 8 },
    timeLimitMs: 1000,
    nodeLimit: 1000000,
    allMoves: false,
  });
  postMessage(result);
} finally {
  position.free();
  engine.free();
}
```

- `WasmPosition`: `fromString` / `toString`（64マスと手番 X/O）、`cells()`（A1→H8、0=空/1=黒/2=白）、`sideToMove()`、`legalMoves()`、`play("F5" | "pass")`、`mustPass()`、`isTerminal()`。
- `play` は元の局面を変更せず、新しい局面を返します。生成したオブジェクトは利用後に `free()` してください。
- `WasmEngine(modelBytes, ttMiB)`: 1スレッド固定。置換表は 1〜256 MiB（補助表・モデルなどは別途メモリを使用）。`setModel(bytes)`、`clearTt()`、`search(position, request)` を公開します。
- `goal`: `{kind: "depth", value: 0..60}` / `{kind: "level", value: 1..60}` / `{kind: "endgame"}`。Depth と Endgame は完全選択率、Level はエンジン定義の選択率です。
- `timeLimitMs` / `nodeLimit`: 任意の u32。`allMoves`: 既定 false。探索は同期処理なので Web Worker 上で実行してください。実行中の同じ Worker へのメッセージでは停止できません。時間・ノード制限を使うか、Worker を terminate してください。
- 結果: `bestMove`、`score`、`depth`、`selectivity`、`solved`、`pv`、`moves`、`stopReason`、`nodes`、`elapsedMs`。任意値の未確定は `undefined`、手は `A1`〜`H8` / `pass`、スコアは手番側から見た石差です。`nodes` は精度を保つため十進文字列です。`stopReason` は `Completed` / `Requested` / `TimeLimit` / `NodeLimit` / `WorkerFailed`。
- 不正なモデル・局面・着手・要求は JavaScript 例外になります。探索は着手を実行しません。
- ブラウザの `performance.now()` に対応する `web-time` をエンジンの WASM ビルドで使用します。共有メモリ・COOP/COEP は不要です。ネイティブの `EngineWorker` は使用しません。

## 検証

```sh
cargo test --workspace --release
wasm-pack test deft-reversi-wasm --node
wasm-pack test deft-reversi-wasm --headless --chrome
```
