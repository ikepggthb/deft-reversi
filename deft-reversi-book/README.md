# deft-reversi-book

[Book v2 ガイド（Markdown）](../docs/book-v2/index.md) / [HTML版](../docs/book-v2/index.html)：設計、継続育成、運用、移行、検証。

ソースレビューは[ソースの読み方](../docs/book-v2/source-review.md)から始める。ファイルの担当、評価の保存先、編集・生成・保存・対局の処理順、旧形式だけで使うコード、対応する試験をまとめている。

リバーシの対局用book。`deft_reversi_engine` の公開APIを利用する独立クレート。

- `Book`：読取専用スナップショットと局面参照
- `BookEditor`：局面グラフの編集、生成、段階的強化、保存再開
- `BookPlayer`：最善手・seed付きランダム選択。bookに無ければ None を返し、探索は呼び出し側が行う

```rust,ignore
use deft_reversi_book::{Book, BookEditor, BookPlayer, ReadLimits, UsePolicy};
use deft_reversi_engine::{Engine, Board};
```

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book
cargo test --locked -p deft_reversi_book --features book-cli
cargo doc --locked -p deft_reversi_book --all-features --no-deps --document-private-items
```

`book-cli` は操作用バイナリとCtrl-C対応を有効にする。旧mainのMPC表を使う `--legacy-main-mpc` のため、エンジンの `tuning` featureも有効にする。`endgame-pattern-ordering` はエンジン側の同名featureを有効にする。

[実装API](../docs/book-v2/architecture.md) と [保存形式・互換性](../docs/book-v2/compatibility.md) を参照。分離前の `.dbook` / `.dbook-work` を読込可能。モデル・校正・探索仕様の識別情報が異なる推定は、明示的な再解析を必要とする。

Book v2：[設計と実装順序](../docs/book-v2/design.md)、[レベルによる育成](../docs/book-v2/levels.md)。work v1〜v8と対局用v1〜v4を読込み、保存はwork v8・対局用v4。最良の未展開手と残り候補の上限を探索し、PV連続伸長と資源予算内の局面並列を行う。全候補生成モードは廃止。
[解析互換性と移行](../docs/book-v2/compatibility.md)、[損失制御](../docs/book-v2/loss-control.md)も参照。

[子からの更新](../docs/book-v2/propagation.md)、[末端再計算・品質診断](../docs/book-v2/diagnostics.md)。

[評価値と探索条件](../docs/book-v2/value-model.md)：要求・予定・実際に完了した探索、推定と最終石差の上下限を区別する。探索記録は共有し、子の値は自動で親へ反映する。過去の計算経路は保存しない。通常の対局参照には`BookEntry::moves()`を使い、詳しい記録は必要時に展開する。

保守性の見直しでは、現行の評価を`Evaluation`へ統合し、点数・探索条件・出所を直接読む形にした。旧分類・途中状態は形式の境界へ移した。[変更理由](../docs/book-v2/maintainability.md)と[Rust APIの移行](../docs/book-v2/compatibility.md)を参照。
