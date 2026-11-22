# deft-reversi

## デスクトップ版の開発用スクリプト

`tools/desktop/` のPowerShellスクリプトを使う。ビルドのコマンドや環境変数を手で組み立てない。詳細は [tools/desktop/README.md](tools/desktop/README.md)。

```powershell
tools/desktop/build.ps1                # デスクトップ版とCLIをビルド（-Desktop でデスクトップ版のみ）
tools/desktop/check.ps1                # clippyとテスト（-Workspace で全クレート）
tools/desktop/run.ps1                  # ビルドしてテスト用ホームで起動（-NoBuild で省略）
tools/desktop/book-verify.ps1 <path>   # .dbook / .dbook-work をCLIで読み込んで検査
```

- ビルドは `x86-64-v3` 向けに最適化して `target-v3/` へ出力する。実行ファイルは `target-v3/release/deft-reversi.exe`（デスクトップ版）と `deft-reversi-cli.exe`。
- `run.ps1` は既定で `DEFT_REVERSI_HOME=local/desktop-home` を使う。普段の設定や棋譜に触れないため、動作確認ではこれを使い、`-UserHome` はユーザーに頼まれたときだけにする。
- アプリの起動中はexeがロックされるので、`build.ps1` は中止する。アプリを閉じてからビルドする。
- 実行ポリシーで拒否されたら `powershell -ExecutionPolicy Bypass -File tools/desktop/check.ps1` のように実行する。
- Git Bashからは `powershell -ExecutionPolicy Bypass -File tools/desktop/<script>.ps1` で呼ぶ。

## 注意

- この環境の `python` はMicrosoftストアの案内用スタブで、何も実行せずに終わる（エラーにならない）。スクリプトやファイル編集に使わない。
- `tools/desktop/*.ps1` はWindows PowerShell 5.1で動くよう英語のみで書く（BOMなしの日本語は文字化けする）。

## 研究ノート

`local/notes/` は、オセロAIエンジンの研究ノートを置く別リポジトリ（`deft-reversi-notes`）。評価関数・探索・枝刈り・Bookの精度や強さ、速度の実験と、その考察を記録する。新しい環境ではクローンしておく。

```powershell
git clone git@github.com:ikepggthb/deft-reversi-notes.git local/notes
```

`local/` は本体の `.gitignore` 対象なので、本体のコミットには含まれない。実験の結果と考察はここにMarkdownで置き、コミットはノート側のリポジトリで行う。
