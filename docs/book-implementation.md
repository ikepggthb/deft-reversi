# Deft Reversi Book

現行仕様は[Book v2ガイド](book-v2/index.md)と[HTML版](book-v2/index.html)にまとめた。bookは独立したdeft_reversi_bookクレートで実装し、既存のEngineを利用する。

理解に必要な分類を減らした変更は[保守性の見直し](book-v2/maintainability.md)に記録した。実装のレビューは[ソースの読み方](book-v2/source-review.md)を入口にする。型・関数と対応する試験まで案内している。

- BookEditorは局面、登録した進行、直接評価・手動値、候補の集合探索結果、生成の仕事一覧を持つ元データ。
- Bookは読取専用スナップショット。BookPlayerが品質と登録を確認して着手を返し、使えない場合の探索は呼出し元が行う。
- 生成器は最良の未展開手と他候補の上限を探索する方式へ一本化。全候補の個別評価モードは廃止した。
- PV（探索が返す最良の進行）に沿った連続生成と、総スレッド・TT（探索の置換表）・ノード予算内の独立局面並列を実装。
- 保存はwork v8・対局用v4。work v1〜v8・対局用v1〜v4を読込可能。ファイル形式とモデル等の識別値の互換性は別々に検査する。

```sh
cargo build --locked --release -p deft_reversi_book --features book-cli --bin book
target/release/book build --model MODEL --work trial.dbook-work --book trial.dbook \
  --levels 19 --max-plies 31 --loss 4 --cumulative-loss 4 --leaf-loss 4 \
  --threads 18 --jobs 0 --line-plies 8 --tt-mib 1024 --memory-mib 2048 \
  --schedule frontier --seconds 60
target/release/book verify --work trial.dbook-work
target/release/book resume --model MODEL --work trial.dbook-work --seconds 60 --threads 18
```

モデル固有の校正を持つDPR2を使う。数値はユーザー指定に対応する操作例で、棋力の最適値を保証するものではない。

[APIと構造](book-v2/architecture.md)、[高速育成の設計](book-v2/design.md)、[損失制御](book-v2/loss-control.md)、[優先処理と並列](book-v2/scheduling.md)、[運用](book-v2/operations.md)、[移行](book-v2/compatibility.md)、[検証・実測](book-v2/validation.md)を参照。

初版の測定記録は[2026-10-29の検証](benchmarks/2026-10-29-book/README.md)に保存している。
