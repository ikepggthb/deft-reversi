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

- `deft-reversi.exe`（デスクトップ版）と `deft-reversi-cli.exe`
- `COPYING`
- `THIRD-PARTY-NOTICES.txt`（配布するバイナリのターゲットで作ったもの）

評価モデル（`deft-eval-v4.dpr2`）は Release `model-v4` のアセットとして別に配布します。
