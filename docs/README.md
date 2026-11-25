# Deft Reversi 技術解説

Deft Reversi の仕組みと設計意図を、リバーシ AI に初めて触れる方にも追えるように章立ててまとめた技術ドキュメントです。

## 目次

| 章 | 内容 | 扱うテーマ |
| --- | --- | --- |
| [第1章](01-architecture.md) | 全体構成とワークスペース設計 | クレート構成、フィーチャフラグ、モデルの受け渡し形式 |
| [第2章](02-bitboard.md) | ビット演算：合法手生成・石反転・確定石 | `u64` ビットボード、Kogge-Stone 法、石反転、確定石の伝播 |
| [第3章](03-evaluation.md) | 評価関数：パターン評価と補助ニューラルネット | 11 パターン×4 回転、60 段階フェーズ、差分更新、量子化推論 |
| [第4章](04-search.md) | 探索アルゴリズム：反復深化・PVS・置換表・終盤ソルバ | 探索窓、ロックフリー置換表、ETC、空き 13 マス以下専用ソルバ |
| [第5章](05-parallel.md) | 並列探索：YBWC とデッドロック回避 | 長男先行探索、兄弟分配、祖先ヘルパー、偽共有対策 |
| [第6章](06-mpc.md) | MPC（Multi-ProbCut）と枝刈り・校正 | 選択率（83%〜99%）、一次回帰モデル、中盤・終盤の自動校正 |
| [第7章](07-book.md) | Book データの構造と生成 | `.dbook-work` と `.dbook`、8 通り対称正規化、バックアップと拡張 |
| [第8章](08-training.md) | 評価関数の学習パイプライン | `.rd` データセット、AdaGrad / Adam、中心化と `shrink` |
| [第9章](09-desktop-and-cli.md) | デスクトップアプリと CLI の設計 | GPUI 状態管理、非同期エンジン制御、アトミック保存、持ち時間制御 |

## クレート別の個別ドキュメント

- 探索エンジン API: [deft-reversi-engine/docs/api.md](../deft-reversi-engine/docs/api.md)
- Book の詳細仕様: [deft-reversi-book/docs/index.md](../deft-reversi-book/docs/index.md)
- 評価関数の学習手順: [deft-reversi-train/docs/pattern-training.md](../deft-reversi-train/docs/pattern-training.md)
