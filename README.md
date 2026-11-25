<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/svg/deft-logo-horizontal-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="assets/logo/svg/deft-logo-horizontal-light.svg" />
    <img alt="Deft Reversi" src="assets/logo/svg/deft-logo-horizontal-light.svg" width="380" />
  </picture>

  <p><strong>Rust 製リバーシ探索エンジン ＆ デスクトップアプリ</strong></p>

  <p>
    <img alt="Rust 1.99.1" src="https://img.shields.io/badge/Rust-1.99.1-000000?style=flat-square&logo=rust" />
    <img alt="Edition 2024" src="https://img.shields.io/badge/Edition-2024-1f2937?style=flat-square" />
    <img alt="License GPL-3.0-or-later" src="https://img.shields.io/badge/License-GPL--3.0--or--later-2563eb?style=flat-square" />
  </p>

  <p>
    <a href="#構成">構成</a> ・
    <a href="#主な機能">主な機能</a> ・
    <a href="#ビルドと実行">ビルドと実行</a> ・
    <a href="#cli--学習ツール">CLI / 学習ツール</a> ・
    <a href="#ドキュメント">ドキュメント</a>
  </p>
</div>

---

Deft Reversi は、リバーシの探索エンジンと関連ツール一式を収めたリポジトリです。探索エンジン本体をはじめ、Book ライブラリ、GPUI を使ったデスクトップアプリ、CLI、評価関数の学習ツールを同じワークスペースに置いています。

## 構成

```text
deft-reversi
├── deft-reversi-engine   # 盤面処理、評価関数、並列探索
├── deft-reversi-book     # Book データの作成・拡張・対局時の参照（.dbook / .dbook-work）
├── deft-reversi-desktop  # GPUI 製のデスクトップアプリ（deft-reversi）
├── deft-reversi-cli      # 対局・解析・Book 管理・GTP / FlipSight 連携用の CLI
├── deft-reversi-train    # 評価関数の学習・比較・ProbCut 校正を行う学習ツール
└── tools/                # ベンチマーク（Python）と Windows 向け開発スクリプト（PowerShell）
```

> [!NOTE]
> `deft-reversi-web` は旧 API 向けのまま残しているため、ワークスペースのビルド対象からは外しています。

<details>
<summary>クレートとパッケージ名の対応</summary>

| クレート | パッケージ名 | 役割 |
| --- | --- | --- |
| [deft-reversi-engine](deft-reversi-engine/) | `deft_reversi_engine` | ビットボード、パターン評価、YBWC 並列探索、終盤完全読み |
| [deft-reversi-book](deft-reversi-book/) | `deft_reversi_book` | Book の保存・自動生成・対局時の候補選択 |
| [deft-reversi-desktop](deft-reversi-desktop/) | `deft_reversi_desktop` | 対局・棋譜編集・解析・Book 作成を行えるデスクトップアプリ（`deft-reversi`） |
| [deft-reversi-cli](deft-reversi-cli/) | `deft-reversi-cli` | ターミナル対局、問題集の解答チェック、棋譜検討、自動対局プロトコル |
| [deft-reversi-train](deft-reversi-train/) | `deft_reversi_train` | `.rd` データセット変換、評価モデルの学習、MPC 校正 |

</details>

## 主な機能

- **探索エンジン**: `u64` ビットボードとロックフリー置換表を使い、YBWC による並列探索や ProbCut の枝刈り、空き 1〜13 マス専用の終盤ソルバを組み合わせています。
- **評価関数**: 11 パターン×4 回転の線形評価に、小型ニューラルネットの量子化出力を足し合わせて石差を見積もります。
- **Book 管理**: 生成や編集の途中経過を `.dbook-work` に貯めながら育て、対局で使うときは読み取り専用の `.dbook` へ書き出して読み込みます。
- **デスクトップアプリ**: 対局、棋譜の編集と解析、Book の閲覧・生成を GPUI の画面上でまとめて操作できます。

## ビルドと実行

### 動作環境

- Rust `1.99.1`（[rust-toolchain.toml](rust-toolchain.toml) で固定。`rustfmt` と `clippy` を含む）
- Windows 向けの開発スクリプトを使う場合は PowerShell

### ビルドとテスト

```bash
cargo check --workspace --all-targets
cargo test --workspace
```

Windows で `x86-64-v3` 向けのリリースビルド（`target-v3/release/`）や起動確認をするときは、[tools/desktop/](tools/desktop/README.md) のスクリプトを使えます。

```powershell
tools/desktop/build.ps1       # デスクトップ版と CLI をビルド（-Desktop でデスクトップ版のみ）
tools/desktop/check.ps1       # clippy とテストを実行（-Workspace で全クレート）
tools/desktop/run.ps1         # ビルド後、テスト用ホーム（local/desktop-home）で起動
```

### 評価モデル（`.dpr2`）について

対局や解析で探索を実行するときは、`.dpr2` 形式の評価モデルファイルを用意してください。

| アプリ／ツール | モデルファイルの探し方 |
| --- | --- |
| `deft-reversi`（Desktop） | 環境変数 `DEFT_REVERSI_MODEL` → 実行ファイル横の `resources/deft-eval-v4.dpr2` → `data/eval/deft-eval-v4.dpr2` の順 |
| `deft-reversi-cli`（CLI） | `-m` / `--model` オプション、または環境変数 `DEFT_MODEL` で指定 |

## CLI / 学習ツール

<details open>
<summary><strong>deft-reversi-cli</strong> のサブコマンド</summary>

```bash
cargo run -p deft-reversi-cli --release -- <サブコマンド>
```

| コマンド | できること |
| --- | --- |
| `play` | ターミナル上でエンジンと対局する |
| `book` | `.dbook` / `.dbook-work` の生成、再開、書き出し、検査を行う |
| `solve` | OBF 形式の問題集を解いて結果を表示する |
| `review` | 棋譜を読み込み、実際の手と最善手を比べる |
| `self-play` | エンジン同士で自己対戦し、棋譜を書き出す |
| `perft` | 初期局面から指定手数までの合法手展開数を数える |
| `gtp` | 自動対局向けの GTP（Go Text Protocol）モードで動く |
| `flipsight` | FlipSight ボット連携用の JSON Lines モードで動く |

</details>

<details>
<summary><strong>deft-reversi-train</strong> のサブコマンド</summary>

```bash
cargo run -p deft_reversi_train --release -- <サブコマンド>
```

| コマンド | できること |
| --- | --- |
| `convert` | 棋譜から学習用データセット（`.rd`）を作る |
| `train` | 全フェーズの評価モデルを学習して書き出す |
| `export` | 学習途中のチェックポイントからモデルを書き出す |
| `eval` | 検証データを使ってモデル同士の誤差を比べる |
| `mpc` | モデルに合わせて ProbCut の係数を校正する |
| `pack` | モデルと ProbCut 校正結果を 1 ファイルにまとめる |

</details>

## ドキュメント

| 内容 | 参照先 |
| --- | --- |
| 技術解説（第1章〜第9章） | [docs/README.md](docs/README.md) |
| 探索エンジン API | [deft-reversi-engine/docs/api.md](deft-reversi-engine/docs/api.md) |
| Book の仕組みと運用 | [deft-reversi-book/docs/index.md](deft-reversi-book/docs/index.md) |
| 評価関数の学習手順 | [deft-reversi-train/docs/pattern-training.md](deft-reversi-train/docs/pattern-training.md) |
| ベンチマークの使い方 | [tools/bench/README.md](tools/bench/README.md) |
| デスクトップ開発スクリプト | [tools/desktop/README.md](tools/desktop/README.md) |

## 謝辞と出典

### 移植したコード

探索エンジンの一部は、次のリバーシエンジンのコードを Rust に移植したものです。移植した箇所は、各ソースファイルの先頭に「出典」として書いています。

| 出典 | 移植したもの | Deft のファイル（`deft-reversi-engine/src/`） |
| --- | --- | --- |
| [Edax](https://github.com/abulmo/edax-reversi)（Richard Delorme・奥原俊彦、GPL-3.0） | 合法手生成と着手の AVX2 版（奥原氏の手法と acepck 氏の最適化）、最終手で返る石数の計算（BMI2）、辺の確定石の表と確定石による枝刈り、空き 1〜4 マスのソルバ、高速な終盤の着手順序 | `board/moves.rs`、`board/flip.rs`、`board/last.rs`、`board/edge_stability.rs`、`board/stability.rs`、`search/endgame/last_four.rs`、`search/endgame/ordered.rs` |
| [Egaroucid](https://github.com/Nyanyan/Egaroucid)（山名琢翔、GPL-3.0-or-later） | 着手順序の深さ・窓・重み、スレッド専有の終盤置換表、孤立した空きマスを先に読む並べ替え | `search/ordering.rs`、`search/endgame/local_tt.rs`、`search/endgame/shallow.rs` |

このほか、ProbCut を Null-Window のノードだけで試す（Edax）、静的評価が境界から遠いときは ProbCut の浅い探索を省く（Egaroucid）といった設計も参考にしています。ビットボードの手法は、奥原俊彦さんの解説（[bitboard tricks](http://www.amy.hi-ho.ne.jp/okuhara/bitboard.htm)）に多くを負っています。

### 評価関数の学習データ

評価モデル（`deft-eval-v4.dpr2`）は、山名琢翔さんが公開している [Egaroucid 学習データ](https://www.egaroucid.nyanyan.dev/ja/technology/train-data/) で学習しました。

- Egaroucid 7.8.0 レベル11 と Edax 4.5.5 レベル11 の対戦棋譜（v0002）
- Egaroucid 7.4.0 / 7.5.1 レベル17 によるデータ（石数 4〜13 の局面）

これだけの量と質のデータを自由に使える形で公開していただいたおかげで、評価関数を作れました。心から感謝します。学習データは再配布が禁止されているため、このリポジトリにもリリースにも含めていません。使うときは配布ページから取得してください。

> 山名琢翔: Egaroucid 学習データ, https://www.egaroucid.nyanyan.dev/ja/technology/train-data/

### 定石名

デスクトップ版の定石名リスト（[openings.txt](deft-reversi-desktop/assets/openings.txt)）は、うえのんさんの「[オセロ定石一覧(250種以上)](https://evaccaneer.livedoor.blog/archives/11101657.html)」（ブログ「レコちゃんも頑張ってるのに」）を使わせていただいています。ページには著作権を主張しないと書かれています。

### 局面と開局

- FFO の終盤問題（`ffo*.obf`、`fforum-*.obf`）は、フランスの連盟（Fédération Française d'Othello）の会誌 FFORUM に掲載された、プログラム向けの問題です。Edax にも同じものが同梱されています。
- 対局のベンチマークに使う XOT の開局リストは、Matthias Berg さんの [XOT](https://berg.earthlingz.de/xot/) から取得します（リポジトリには含めていません。[tools/bench](tools/bench/README.md) を参照）。

## 商標

「オセロ」「Othello」は株式会社メガハウスの登録商標です。Deft Reversi は株式会社メガハウスおよび Othello Co. とは関係ありません。

## ライセンス

[GPL-3.0-or-later](LICENSE)（全文は [COPYING](COPYING)）。依存クレートのライセンスは、リリースに同梱する `THIRD-PARTY-NOTICES.txt` にまとめています（[tools/release](tools/release/README.md)）。
