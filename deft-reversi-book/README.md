# deft-reversi-book

[Book v2 ガイド](docs/index.md)：設計、継続育成、運用、移行、検証。

ソースレビューは[ソースの読み方](docs/source-review.md)から始める。ファイルの担当、評価の保存先、編集・生成・保存・対局の処理順、旧形式だけで使うコード、対応する試験をまとめている。

リバーシの対局用book。`deft_reversi_engine` の公開APIを利用する独立クレート。

- `Book`：読取専用スナップショットと局面参照
- `BookEditor`：局面グラフの編集、生成、段階的強化、保存再開
- `BookPlayer`：最善手・seed付きランダム選択。bookに無ければ None を返し、探索は呼び出し側が行う

```rust,ignore
use deft_reversi_book::{Book, BookEditor, BookPlayer, ReadLimits, UsePolicy};
use deft_reversi_engine::{Engine, Board};
```

```sh
cargo build --locked --release -p deft-reversi-cli
cargo test --locked -p deft_reversi_book
cargo doc --locked -p deft_reversi_book --all-features --no-deps --document-private-items
```

CLIがBook操作とCtrl-Cを扱う。`endgame-pattern-ordering` はエンジン側の同名featureを有効にする。

[実装API](docs/architecture.md) と [保存形式・互換性](docs/compatibility.md) を参照。分離前の `.dbook` / `.dbook-work` を読込可能。モデル・校正・探索仕様の識別情報が異なる推定は、明示的な再解析を必要とする。

Book v2：[設計と実装順序](docs/design.md)、[レベルによる育成](docs/levels.md)。work v1〜v9と対局用v1〜v5を読込み、集合全体の上限を含む場合にwork v9・対局用v5を使用。frontier方式は主線を優先し、損失予算を満たさない探索集合の上限と、候補手の点評価を保持する。全候補生成モードは廃止。
[解析互換性と移行](docs/compatibility.md)、[損失制御](docs/loss-control.md)も参照。

[子からの更新](docs/propagation.md)、[末端再計算・品質診断](docs/diagnostics.md)。

[評価値と探索条件](docs/value-model.md)：要求・予定・実際に完了した探索、推定と最終石差の上下限を区別する。探索記録は共有し、子の値は自動で親へ反映する。過去の計算経路は保存しない。通常の対局参照には`BookEntry::moves()`を使い、詳しい記録は必要時に展開する。

保守性の見直しでは、現行の評価を`Evaluation`へ統合し、点数・探索条件・出所を直接読む形にした。旧分類・途中状態は形式の境界へ移した。[変更理由](docs/maintainability.md)と[Rust APIの移行](docs/compatibility.md)を参照。

Bookの生成と検査は `deft-reversi-cli book --help` を参照。

## 実装の要点と操作例

- PV（探索が返す最良の進行）に沿った連続生成と、総スレッド・TT（探索の置換表）・ノード予算内の独立局面並列を実装している。
- モデル固有の校正を持つDPR2を使う。

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book
target/release/book build --model MODEL --work trial.dbook-work --book trial.dbook \
  --levels 19 --max-plies 31 --loss 4 --cumulative-loss 4 --leaf-loss 4 \
  --threads 18 --jobs 0 --line-plies 8 --tt-mib 1024 --memory-mib 2048 \
  --schedule frontier --seconds 60
target/release/book verify --work trial.dbook-work
target/release/book resume --model MODEL --work trial.dbook-work --seconds 60 --threads 18
```

数値はユーザー指定に対応する操作例で、棋力の最適値を保証するものではない。

優先処理と並列、運用、検証・実測は [優先処理と並列](docs/scheduling.md)、[運用](docs/operations.md)、[検証・実測](docs/validation.md) を参照。初版の測定記録は [2026-10-29の検証](https://github.com/ikepggthb/deft-reversi-notes/blob/main/verification/2026-10-29-book/README.md) に保存している。
