# Deft Reversi Web 設計メモ（現状）

このドキュメントは、`deft-reversi-web` の「いまの設計」と、各モジュールの責務・データ境界・主要な処理フローを共有するためのメモです。

## 目的と方針

- **UIの応答性を保つ**: 重いAI探索は Web Worker + WASM に隔離し、UIスレッドの停止を避ける。
- **責務を分離する**:
  - **UI層（JS/DOM/Canvas）**: 描画・入力・モーダルの制御。ゲームルール判断は持たない。
  - **アプリケーション層（JS）**: ゲーム進行（イベント直列化、AI手番、Undo/Redo、ヒント計算）を統括し、UIに渡す表示用データを組み立てる。
  - **ドメイン層（JS）**: 盤面ルール（合法手/反転/着手/終局/パス）を `BigInt` bitboard で実装し、不変オブジェクトとして扱う。
  - **インフラ層（JS）**: 設定の永続化（localStorage）や、AI呼び出し（Worker RPC）を抽象化する。
  - **WASM（Rust）**: 「与えられた局面の solve（最善手+評価など）」のみ。
- **キャンセル可能な非同期**: AI探索/ヒント計算は「結果が戻ってきても捨てられる」ようにトークンで世代管理する（Worker terminate は将来選択肢）。

## ビットボード表現

- 盤面は 64bit のビットボードで表す（LSB=マス0）。
- JS内部は `BigInt` を使用（`0n .. 2^64-1`）。
- WASM境界は `u64` を **`u32 low/high` に分割**して受け渡しする。
  - 変換は `infrastructure/ai-engine.js` が担当（UI/ドメインはWorker境界の型に依存しない）。

### マス番号の対応

- `pos: 0..63`
- `x = pos % 8`, `y = floor(pos / 8)`
- 棋譜は `a1..h8` 形式（`domain/types.js`）。

## 主要コンポーネントと責務

### `application/game-service.js`（オーケストレーター / 状態機械の核）

- ゲーム状態（`domain/Game`）と履歴（Undo/Redo）を保持し、UIへ `render(viewModel)` する。
- `_runExclusive()` によりイベント処理を直列化（レースの原因になりやすい並行更新を抑止）。
- ゲーム進行:
  - 人間クリック → 着手適用 → 必要ならパス処理 → AI手番起動
  - AI手番 → `AiEngine.solveTurn(...)` → 着手適用 → 次ターンへ
- ヒント:
  - `HintService` を使って逐次評価し、1手更新するごとにUIを再描画する。
- UIに渡すのはドメインそのものではなく、**表示用のViewModel**（BigInt bitboardなど）にまとめる。

### `domain/board.js` / `domain/game.js`（ドメイン）

- `Board`（値オブジェクト）: bitboard演算（合法手/反転/着手/パス/終局）を持つ。不変オブジェクトで扱う。
- `Game`（集約ルート）: `Board` と棋譜（`GameRecord`）と `lastMove` を束ね、`applyMove/applyPass` などの操作を提供する。

### `infrastructure/ai-engine.js`（AI呼び出し統合レイヤー）

- `postMessage` ベースのWorker RPC（`requestId`/timeout）と、bitboard変換（`BigInt` ↔ `u32 low/high`）を1箇所に集約する。
- アプリケーション層は `solveTurn(playerBits, opponentBits, level)` だけを呼び、Worker境界の表現に依存しない。

### `engine/engine.js`（Worker本体）

- WASMを初期化し、評価データ（`assets/deft_eval_2024-01-27.json.gz`）をロードして `AiSolver` を生成。
- 受信メッセージを `switch(type)` で処理し、結果を返す。
- 現状の公開RPCは **`solveTurn` のみ**。

### `wasm/src/lib.rs`（WASM公開API）

- `AiSolver` を公開し、solve結果を JSに渡す。
- JSが扱いやすいように `best_move_low/high` などへ変換して返す。

### 設定（Settings）

- 永続化される設定（例: AI有効、AIレベル、AI手番、定石選択）は `SettingsService` が管理する。
  - デフォルト値（仕様）は `application/settings-service.js` に置く。
  - 永続化（localStorage）は `infrastructure/settings-repository.js` が担当し、旧キー `gameSettings` から `deft-reversi-settings` へ移行する。
- ヒント設定（表示ON/OFF、深さなど）はセッション内のみ（現状は永続化しない）。

### UI（`ui/ui.js` / `ui/board.js` / `ui/status.js`）

- `UI.render(viewModel, ...)` が `BoardUI` と `StatusUI` を更新する。
- 入力は `EventDispatcher` を通じて `GameService` に通知される（UIはゲームルールを持たない）。

## UIに渡す ViewModel の形

`GameService` が生成する代表的なフィールド（概念）:

- `blackBits: bigint`
- `whiteBits: bigint`
- `legalMovesBits: bigint`（手番側の合法手）
- `nextTurn: "Black" | "White"`
- `eval: null | Array(64) of number|null`（ヒント用のマス別スコア）
- `lastMove: null | number`
- `currentHumanOpening: null | string`
- `humanOpeningNextPosition: null | number`

## 処理フロー（概要）

### 人間の着手

1. `boardClick` → `GameService` がクリック位置を受け取る
2. 必要なら強制パスを解決（演出: `drawPassMessage`）
3. 合法手なら着手を適用し、履歴を更新して描画
4. 状態に応じてヒント更新/AI手番へ遷移

### AIの着手

1. 強制パス連鎖を先に解決
2. `AiEngine.solveTurn(playerBits, opponentBits, aiLevel)` を呼び出す
3. 結果の最善手を `pos` に復元し、着手を適用して描画

### ヒント計算

- 目的: 「1手計算したらすぐ描画」し、深さを上げながら更新していく。
- 実装: `HintService` が反復深化で `solveTurn` を繰り返し呼び、更新ごとにコールバックで通知する。
- キャンセル: 世代トークンにより古い計算結果は破棄する。

## テスト

- `app/test/domain/*`: ドメイン（`Board`/`Game`）の不変性やルールを検証
- `app/test/application/*`: アプリケーションサービスの一部（例: 定石マッチング）を検証
- `app/test/utils.test.mjs`: `Board` の静的メソッドとしてルール不変条件を検証
