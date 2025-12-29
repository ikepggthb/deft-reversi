# Plan

このプロジェクトにDDDを適用するなら、「DOM/Worker/WASM と独立した“ゲームルールの中核(ドメイン)”を作り、game.js はアプリケーション層として薄くする」のが一番効果が出ます。ビットボード自体は実装詳細なので、DDD適用と両立します（WASM側もそのまま維持）。

## Requirements
- ドメイン層は純粋（Canvas/DOM/Worker/localStorage に依存しない）
- WASMは現状の bitboard API を維持
- UIは現状の描画・操作感を維持しつつ分離

## Scope
- In: フォルダ分割、ドメインモデル導入、game.js の責務整理、境界（AI/永続化）をインターフェース化
- Out: WASMロジック刷新、UIデザイン変更、大規模な状態管理ライブラリ導入

## Files and entry points
- ドメイン（新規）: app/domain/**
- アプリケーション（整理）: game.js
- インフラ（現状を明確化）: app/engine/**, localStorage周り
- プレゼンテーション: app/ui/**
- 既存のビット演算: bitboard.js（最終的に app/domain/ 配下へ寄せる候補）

## Data model / API changes
- WASM境界のAPIは変更しない（*_bits_low/high の受け渡しは維持）
- JS内部は GameState（ドメイン）を中心にして、UIに渡す表示用DTOは別途組み立てる（必要なら今の state 形式も維持可能）

## Action items
[ ] ドメインのユビキタス言語を確定（例: Position, Turn, Move, Board, Game/Match）
[ ] Board（値オブジェクト）と Game（集約）を作る：内部表現は bitboard のままでOK
[ ] ドメインサービスとして legalMoves/applyMove/isEnd/isPass を Game から呼べる形に包む（今の bitboard.js をラップ）
[ ] アプリケーション層として GameService（例: start(), humanMove(pos), undo(), requestHint()）を作り、game.js のロジックを移す
[ ] インフラとして AiGateway（engine-client呼び出し）と SettingsRepository（localStorage）をインターフェース化
[ ] UIは「入力→コマンド呼び出し」「状態変化→再描画」だけにして、ルール判断を持たせない
[ ] 既存テスト（utils.test.mjs）をドメインテストに寄せて追加する

## Testing and validation
- node --test（既存 + ドメインの新規テスト）
- 人間手番/AI手番/パス連鎖/終局の手動確認（ブラウザ）

## Risks and edge cases
- 既存の state（UI向けDTO）と新しいドメイン状態の二重管理になりやすい → 生成場所をアプリ層に一本化
- 「ヒント逐次更新」「AIキャンセル」の世代管理（token）はアプリ層に残すのが安全

## Open questions
- 「ドメインの内部表現」を bitboard のままにしますか？（DDD的には推奨：配列化は価値が薄い割に変換バグが増えます）
- まずは最小のDDD（フォルダ分割 + Game 集約導入）から始めますか？それとも一気に GameService/AiGateway/SettingsRepository まで切りますか？
