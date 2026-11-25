<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/svg/deft-logo-horizontal-dark.svg">
    <img src="assets/logo/svg/deft-logo-horizontal-light.svg" alt="Deft Reversi" height="48">
  </picture>
</h1>

オセロの探索エンジン。現在の開発対象はネイティブ向けのRustライブラリです。

- [エンジンのAPI・設計・実行方法](deft-reversi-engine/README.md)
- [対局用bookの参照・生成・更新](deft-reversi-book/README.md)
- [リライト計画と移行範囲](https://github.com/ikepggthb/deft-reversi-notes/blob/main/design/rewrite-plan.md)
- [レビュー後の修正と性能確認](https://github.com/ikepggthb/deft-reversi-notes/blob/main/verification/2026-10-27-review-fixes/README.md)
- [終盤探索の比較と差分調査](https://github.com/ikepggthb/deft-reversi-notes/blob/main/endgame/2026-10-24-regression-investigation/README.md)
- [旧評価関数の説明](https://github.com/ikepggthb/deft-reversi-notes/blob/main/design/old-evaluation-function.md)

## ビルドと検証

```sh
cargo build --locked --release
cargo test --locked
RUSTFLAGS="-Ctarget-cpu=native" cargo test --locked --release -- --include-ignored
cargo clippy --locked --all-targets -- -D warnings
```

Rust 1.99.1を `rust-toolchain.toml` で固定し、依存関係はルートの `Cargo.lock` で固定しています。rustfmtとClippyも同じツールチェーンの構成要素として指定しています。

モデルは呼び出し元が指定します。形式は DPR2 の1ファイルで、重みと ProbCut の校正を持ちます（[エンジンの README](deft-reversi-engine/README.md#モデルとmpc)）。NNUEは対象外です。

```sh
cargo run --locked --release -p deft-reversi-cli -- solve POSITIONS.obf --model MODEL.dpr2 --exact --threads 1 --tt-mib 256
```

## バージョン

今回の開発系列は Deft Reversi 2.0 です。Engine は自身の `Cargo.toml` で `2.0.0-dev.1`、CLI・Book・学習ツール・デスクトップ版もそれぞれの `Cargo.toml` で `1.0.0-dev.1` を指定します。版はワークスペースで共有せず、クレートごとに管理します。`-dev.N` は正式リリース前の開発ビルドを表します。開発版の区切りで `N` を進め、個々のビルドはGitコミットでも識別します。

バージョンを変更したら `cargo check --workspace` でロックファイルを更新し、`cargo test --locked --workspace` で確認します。正式版をリリースするときにプレリリース接尾辞を外します。

```sh
cargo run --locked --release -p deft-reversi-cli -- --version
cargo run --locked --release -p deft-reversi-cli -- book --version
cargo run --locked --release -p deft_reversi_train -- --version
```

`--version` には製品番号に加え、ビルド元のGitコミットを表示します。`-dirty` は上記4クレートまたはワークスペースのビルド設定に未コミットの変更があることを示します。ソースアーカイブなど、コミットを取得できない場合は `git unknown` と表示します。対局中の `settings` では、ビルドターゲット・バイナリのSHA-256・モデルのSHA-256も確認できます。

`solve`、`review`、`self-play`、`perft`、`gtp` とBookの学習では、標準エラーにCLI・Engine・Bookライブラリのバージョン、探索仕様番号、Gitコミットを表示します。`solve --tsv` の標準出力は計測結果のままです。

モデルやBookのファイル形式、探索仕様の互換性はそれぞれの番号で管理します。公開済みの旧Web版は独立したバージョンを持ちます。

## 旧アプリケーション

`deft-reversi-train` はパターン評価の学習器です（[使い方](deft-reversi-train/README.md)）。`deft-reversi-cli` は新エンジンと定石を使う端末アプリです（[対局・solve・review・自己対戦・perft](deft-reversi-cli/README.md)）。`deft-reversi-web` は旧APIに依存する移植前のソースとして保存し、ワークスペースから除外しています。

公開済みの[Web版](https://az.recazbowl.net/deft-reversi-web/index.html)は、ここで保存している旧アプリケーションに対応します。新エンジンへの移行は未実施です。

計測ツールは [tools/bench](tools/bench/README.md) にあります（FFO の完全読みの比較、中盤の速さ、Edax・Egaroucid との対局）。比較用エンジン・モデル・学習データは、`.gitignore` で除外した `local/` に置きます。

## 測定記録

直近のFFO40–49・1コア比較は [Edax・Egaroucidとの測定記録](https://github.com/ikepggthb/deft-reversi-notes/blob/main/engine-comparison/2026-10-27-three-engines/README.md) を参照してください。過去の時間は、異なる実装・モデル・測定条件の結果を含むため、直接つなげて改善率を計算しません。

過去のレポート中の `deft_reversi_engine_v2` は移動前の名称です。測定時のハッシュ、コマンド、再現用ソースは当時の記録として保持しています。現在のディレクトリは `deft-reversi-engine`、クレート名は `deft_reversi_engine` です。

## ライセンス

[GNU General Public License v3.0](LICENSE)。
