# Deft Reversi

オセロの探索エンジン。現在の開発対象はネイティブ向けのRustライブラリです。

- [エンジンのAPI・設計・実行方法](deft-reversi-engine/README.md)
- [リライト計画と移行範囲](docs/rewrite-plan.md)
- [終盤探索の比較と差分調査](docs/benchmarks/2026-10-24-regression-investigation/README.md)
- [旧評価関数の説明](docs/old-evaluation-function.md)

## ビルドと検証

```sh
cargo build --release
cargo test
RUSTFLAGS="-Ctarget-cpu=native" cargo test --release -- --include-ignored
cargo clippy --all-targets -- -D warnings
```

モデルは呼び出し元が指定します。元のv3/v4パターンモデル（JSON・bincode）をMPC設定とともに直接読み込めます。NNUEは対象外です。

```sh
cargo run --release --example solve -- MODEL.bin POSITIONS.obf exact 1 256
```

## 旧アプリケーション

`deft-reversi-cli`、`deft-reversi-web`、`deft-reversi-learn` は移植前のソースとして保存し、ワークスペースから除外しています。旧エンジンは削除済みです。CLI・Webは旧APIに依存するため、新エンジンへの移植前にはビルドできません。学習処理も今回の検証対象には含めていません。

公開済みの[Web版](https://az.recazbowl.net/deft-reversi-web/index.html)は、ここで保存している旧アプリケーションに対応します。新エンジンへの移行は未実施です。

`tools/engine-compare` は任意の比較ツールで、隣接する `../deft-reversi-feature` の元実装を必要とします。通常のビルドには不要です。通常API同士の比較には `level:60` を指定します。

## 測定記録

同一モデル・単一スレッド・通常APIの直近FFO40–49比較では、元実装12.535239秒、新実装12.880500秒でした。残る性能差と設計差は上記の調査レポートに記録しています。

過去のレポート中の `deft_reversi_engine_v2` は移動前の名称です。測定時のハッシュ、コマンド、再現用ソースは当時の記録として保持しています。現在のディレクトリは `deft-reversi-engine`、クレート名は `deft_reversi_engine` です。

## ライセンス

[GNU General Public License v3.0](LICENSE)。
