# deft-reversi-book

リバーシの対局用book。`deft_reversi_engine` の公開APIを利用する独立クレート。

- `Book`：読取専用スナップショットと局面参照
- `BookEditor`：局面グラフの編集、生成、段階的強化、保存再開
- `BookPlayer`：最善手・seed付きランダム選択。bookに無ければ None を返し、探索は呼び出し側が行う

```rust,ignore
use deft_reversi_book::{Book, BookEditor, BookPlayer, ReadLimits, UsePolicy};
use deft_reversi_engine::{Engine, Position};
```

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book
cargo test --locked -p deft_reversi_book --features book-cli
```

`book-cli` は操作用バイナリとCtrl-C対応を有効にする。旧mainのMPC表を使う `--legacy-main-mpc` のため、エンジンの `tuning` featureも有効にする。`endgame-pattern-ordering` はエンジン側の同名featureを有効にする。

[API・CLI・保存形式](../docs/book-implementation.md) を参照。分離前の `.dbook` / `.dbook-work` と解析識別情報の互換性を維持する。
