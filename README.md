# Deft Reversi

オセロの探索エンジン。現在の開発対象はネイティブ向けのRustライブラリです。

- [エンジンのAPI・設計・実行方法](deft-reversi-engine/README.md)
- [リライト計画と移行範囲](docs/rewrite-plan.md)
- [レビュー後の修正と性能確認](docs/benchmarks/2026-10-27-review-fixes/README.md)
- [終盤探索の比較と差分調査](docs/benchmarks/2026-10-24-regression-investigation/README.md)
- [旧評価関数の説明](docs/old-evaluation-function.md)

## ビルドと検証

```sh
cargo build --locked --release
cargo test --locked
RUSTFLAGS="-Ctarget-cpu=native" cargo test --locked --release -- --include-ignored
cargo clippy --locked --all-targets -- -D warnings
```

Rust 1.99.1を `rust-toolchain.toml` で固定し、依存関係はルートの `Cargo.lock` で固定しています。rustfmtとClippyも同じツールチェーンの構成要素として指定しています。

モデルは呼び出し元が指定します。元のv3/v4パターンモデル（JSON・bincode）をMPC設定とともに直接読み込めます。NNUEは対象外です。

```sh
cargo run --locked --release --example solve -- MODEL.bin POSITIONS.obf exact 1 256
```

## 旧アプリケーション

`deft-reversi-cli`、`deft-reversi-web`、`deft-reversi-learn` は移植前のソースとして保存し、ワークスペースから除外しています。旧エンジンは削除済みです。CLI・Webは旧APIに依存するため、新エンジンへの移植前にはビルドできません。学習処理も今回の検証対象には含めていません。

公開済みの[Web版](https://az.recazbowl.net/deft-reversi-web/index.html)は、ここで保存している旧アプリケーションに対応します。新エンジンへの移行は未実施です。

現在の比較ツールは [tools/bench](tools/bench/README.md) にあります。旧版とのAPI比較では `--old-engine` で旧クレートの場所を指定し、通常API同士の比較には `level:60` を指定します。Edax・Egaroucidとの比較も、評価データとバイナリの場所を引数で指定できます。

## 測定記録

直近のFFO40–49・1コア比較は [Edax・Egaroucidとの測定記録](docs/benchmarks/2026-10-27-three-engines/README.md) を参照してください。過去の時間は、異なる実装・モデル・測定条件の結果を含むため、直接つなげて改善率を計算しません。

過去のレポート中の `deft_reversi_engine_v2` は移動前の名称です。測定時のハッシュ、コマンド、再現用ソースは当時の記録として保持しています。現在のディレクトリは `deft-reversi-engine`、クレート名は `deft_reversi_engine` です。

## ライセンス

[GNU General Public License v3.0](LICENSE)。
