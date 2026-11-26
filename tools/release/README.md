# リリースの添付物

配布するバイナリには、Deft Reversi のライセンス全文（ルートの [COPYING](../../COPYING)）と、リンクしている依存クレートのライセンス一覧（`THIRD-PARTY-NOTICES.txt`）を同梱します。依存の多くは MIT・BSD・Apache-2.0 などで、バイナリを配布するときにも著作権表示とライセンス文を添える必要があります。

## 依存クレートのライセンス一覧：[third_party_notices.py](third_party_notices.py)

`Cargo.lock` から、配布するパッケージ（既定はデスクトップ版と CLI）が通常の依存としてリンクするクレートを集め、各クレートのソースにあるライセンスファイルの本文を1つのファイルにまとめます。依存が変わるたびに作り直すので、リポジトリには置きません。

```sh
python3 tools/release/third_party_notices.py                                   # Windows 向け → target/THIRD-PARTY-NOTICES.txt
python3 tools/release/third_party_notices.py --target x86_64-unknown-linux-gnu --out OUT.txt
python3 tools/release/third_party_notices.py --package deft-reversi-cli          # CLI だけを配布するとき
```

- dev・build の依存と proc-macro（`clap_derive` や `serde_derive` と、それらだけが使う `syn` など）は、バイナリに入らないので載せません。

- 依存のソースが手元にないと読めないので、先に一度ビルドするか `cargo fetch --locked` を実行しておきます。
- ライセンスファイルを同梱していないクレートは、MIT を選べるなら Cargo の `authors` から著作権表示を作って MIT の本文を載せ、Apache-2.0 なら標準の本文を載せます。どちらも選べないクレートがあると止まるので、そのときは手で確かめます。

## 同梱するもの

- 実行ファイル（デスクトップ版は `deft-reversi.exe`、CLI は `deft-reversi-cli(.exe)`）
- `COPYING`
- `THIRD-PARTY-NOTICES.txt`（配布するバイナリのターゲットとパッケージで作ったもの）

評価モデル（`deft-eval-v4.dpr2`）は Release `model-v4` のアセットとして別に配布します。

## CLI のリリース：[release-cli.yml](../../.github/workflows/release-cli.yml)

CLI は GitHub Actions でビルドします。Windows 版はこのワークフローで作ります。

- **`main` への PR：** Windows（`x86_64-pc-windows-msvc`）と Linux（`x86_64-unknown-linux-gnu`、Ubuntu 24.04）でビルドし、`perft 8` で動作を確かめてから、アーカイブを Actions の成果物に置きます。
- **`cli-v<版>` のタグ：** 同じアーカイブと `SHA256SUMS` を、下書きのプレリリースに添付します。タグと `deft-reversi-cli` の版が違うと止まります。
- **ビルド設定：** エンジンは AVX2・BMI2 の実装をコンパイル時に選ぶので、`-Ctarget-cpu=x86-64-v3` でビルドします。
- **アーカイブ：** `deft-reversi-cli-<版>-<ターゲット>.zip`（Windows）または `.tar.gz`（Linux）。中身は実行ファイル、CLI の README、`COPYING`、`THIRD-PARTY-NOTICES.txt` です。

手順:

1. `develop/v2` から `release/cli-v<版>` を作り、各クレートの版を上げる。
2. `main` への PR を作り、CI と Release CLI のビルドが通ったらマージする。
3. `main` のマージコミットに `cli-v<版>` のタグを打ってプッシュする。
4. 下書きの Release で添付物を確かめ、リリースノートを書いてから公開する。
5. `main` を `develop/v2` に取り込む。
