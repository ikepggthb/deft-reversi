# Contributing

## コミットメッセージ

### フォーマット（1行目）

```
type(scope): emoji title
```

- `type` / `scope` は小文字固定
- `emoji` は必須（1つだけ）
- `#Issue` 番号は付けない
- `title` は短く具体的に（日本語なら現在形/過去形どちらでも可）

### type（固定）

- `feat`：機能追加・変更
- `fix`：バグ修正
- `refactor`：リファクタリング
- `docs`：ドキュメント
- `test`：テスト追加・修正
- `chore`：雑務（ビルド/ツール/依存更新など）
- `style`：見た目・フォーマット（挙動変更なし）

### emoji（typeごとに固定）

- `feat`：✨
- `fix`：🐛
- `refactor`：♻️
- `docs`：📝
- `test`：✅
- `chore`：🔧
- `style`：💄

### scope（固定）

- `web`（deft-reversi-web）
- `engine`（deft-reversi-engine）
- `cli`（deft-reversi-cli）
- `learn`（deft-reversi-learn）
- `data`
- `tools`
- `workspace`（ルート設定・全体に関わる変更）

### 例

- `feat(web): ✨ 対局画面に解析パネルを追加`
- `fix(engine): 🐛 終盤探索でのオーバーフローを修正`
- `docs(workspace): 📝 ビルド手順を更新`
- `refactor(cli): ♻️ オプション解析を整理`
- `style(web): 💄 レイアウトの余白を調整`

