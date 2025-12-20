# Deft Reversi Web 設計メモ（現状）

このドキュメントは、`deft-reversi-web` の「いまの設計」と、各モジュールの責務・データ境界・主要な処理フローを短く共有するためのメモです。

## 目的と方針

- **UIの応答性を保つ**: 重いAI探索は Web Worker + WASM に隔離し、UIスレッドの停止を避ける。
- **責務を分離する**:
  - **JS**: 盤面状態の保持、合法手/反転/着手などのルール適用、UI描画、操作の直列化（イベント→ゲーム処理）。
  - **WASM**: 「与えられた局面の solve（最善手+評価など）」のみ。
- **キャンセル可能な非同期**: AI探索/ヒント計算は「結果が戻ってきても捨てられる」ようにトークンで世代管理する（Worker terminate は将来選択肢）。

## ビットボード表現

- 盤面は 64bit のビットボードで表す（LSB=マス0）。
- JS内部は `BigInt` を使用（`0n .. 2^64-1`）。
- WASM境界は `u64` を **`u32 low/high` に分割**して受け渡しする。
  - JS: `bitsToParts(bits) -> { low, high }`
  - JS: `getBits(state, "black") -> BigInt` など

### マス番号の対応

- `pos: 0..63`
- `x = pos % 8`, `y = floor(pos / 8)`
- 表示（棋譜）は `a1..h8` 形式（`positionToStr()`）。

## 主要コンポーネントと責務

### `game.js`（オーケストレーター / 状態機械の核）

- 1局の状態（盤面・手番・合法手・ヒント評価など）を保持し、UIへ `render()` する。
- `runExclusive()` によりイベント処理を直列化（レースの原因になりやすい並行更新を抑止）。
- ゲーム進行:
  - `handleHumanMove()`：人間のクリック→着手→必要ならパス処理→AIターン起動
  - `startAiTurn()`：パス連鎖処理→solve→着手→次ターンへ
- ヒント:
  - `enableHint` がONのとき、状態変化後に `scheduleHintRefresh()` で再計算をスケジュール
  - `startHintJob()` が逐次評価し、**1手評価するたびに描画を更新**する

### `utils.js`（ルール純関数）

- ビット演算によるルール処理（JS内完結）:
  - `legalMoves(player, opponent)`
  - `flipBits(posMask, player, opponent)`
  - `applyMoveForTurn(player, opponent, pos)`
  - `isPass(player, opponent)` / `isEnd(player, opponent)`
- 状態↔ビットボードの変換:
  - `getBits(state, "black"|"white"|"legal_moves")`
  - `bitsToParts(BigInt) -> { low, high }`

### `engine.js`（Worker本体）

- WASMを初期化し、評価データ（`deft_eval_2024-01-27.json.gz`）をロードして `AiSolver` を生成。
- 受信メッセージを `switch(type)` で処理し、結果を返す。
- 現状の公開RPCは **`solveTurn` のみ**。

### `engine-client.js`（Worker RPCクライアント）

- `postMessage` ベースのRPCラッパ。
- `requestId` で Promise を紐づけ、タイムアウト（現状 `120s`）を管理。

### `src/lib.rs`（WASM公開API）

- `AiSolver` を公開し、solve結果を JSに渡す。
- JSが扱いやすいように `SolverResult` 相当の形へ変換して返す:
  - `best_move_low/high`, `eval`, `solver_type`, `searched_nodes_low/high`, `searched_leaf_nodes_low/high`

### `ui.js` / `board.js`（描画）

- `UI.render(status, ...)` が `board.update(status)` を呼び、盤面とボタンUIを更新する。
- `board.js` は `status.eval` が `null/undefined` のマスは **スコア未表示**にできる（ヒント逐次更新対応）。

## 状態データ（UIに渡す `state` の形）

`game.js` の `buildState()` が生成する（代表的なフィールド）:

- `black_bits_low/high: u32`
- `white_bits_low/high: u32`
- `legal_moves_bits_low/high: u32`（手番側の合法手）
- `next_turn: "Black" | "White"`
- `eval: null | (Array(64) of number|null)`（ヒント用のマス別スコア）
- `last_move: null | number`
- `flipping: string`（現状は互換のため保持）
- `human_opening_next_position/current_human_opening`（現状はUI互換のため保持）

## 処理フロー

### 人間の着手

1. `boardClick` → `handleHumanMove(pos)`
2. 強制パスがあれば `handleForcedPasses()`（演出: `handlePassAnimation()`）
3. `applyMove(pos)`:
   - `applyMoveForCurrentTurn()`（JSルールで盤面更新）
   - 直後に強制パスがあればパス適用
   - `restartHintIfEnabled()`（人間手番でのみヒント再計算）
4. `maybeRunAiTurn()`（AI手番なら `startAiTurn()`）

### AIの着手

1. `startAiTurn()`:
   - 強制パス連鎖を先に解決
   - まだAI手番なら `engine.solveTurn(...)`
2. `best_move_low/high` を 64bit マスクに復元し、LSB位置を `pos` に変換
3. `applyMove(pos)`（人間と同じくJSルールで更新）

### ヒント計算（逐次・低レベル→高レベル）

- 目的: **「1手計算したらすぐ描画」**し、深さを上げながら更新していく。
- 並び順: 合法手は「前回（浅いLv）の評価が高い順」に並べ替えてから次Lvを計算。

流れ:

1. 状態更新後 `scheduleHintRefresh(level)`（Microtaskで1回にまとめる）
2. `startHintJob(level)`:
   - `scores[64] = null` をセットして一度描画
   - `lv=1..level` を順に:
     - 合法手リストを `scores` 降順でソート
     - 各合法手について:
       - その手を適用した次局面を作り、`solveTurn(aiLevel=lv)` を呼ぶ
       - `scores[pos]` を更新して描画
       - `requestAnimationFrame` でUIに制御を返す

キャンセル:

- `hintToken`（世代）と `currentStateKey()`（局面キー）で「古い計算結果は捨てる」。

## 制約・注意点

- WASMの再ビルド（`pkg/`更新）はこのリポジトリ外の環境で必要になる場合があります（`wasm-pack` 前提）。
- Workerは単一スレッドなので、同時に大量 `solveTurn` を投げても並列化はされません（UI更新のため逐次化を採用）。

## テスト

- `test/utils.test.mjs`（`node --test`）で `utils.js` のルール純関数を検証する。

