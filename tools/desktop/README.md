# デスクトップ版の開発用スクリプト

PowerShellで、リポジトリのどこからでも実行できる。ビルドは`x86-64-v3`向けに最適化し、`target-v3`へ出力する（通常の`cargo build`の`target`とは別）。

|スクリプト|内容|
|---|---|
|`build.ps1`|デスクトップ版とCLIをビルド。`-Desktop`でデスクトップ版のみ|
|`check.ps1`|clippyとテスト。既定はデスクトップ版、`-Workspace`で全クレート|
|`run.ps1`|ビルドして起動。既定はテスト用ホーム`local/desktop-home`を使い、普段の設定・棋譜には触れない。`-UserHome`で普段の設定、`-NoBuild`でビルドを省略|
|`book-verify.ps1`|BookをCLIで読み込んで検査。拡張子で対局用（`.dbook`）と作業ファイル（`.dbook-work`）を判別|

```powershell
tools/desktop/check.ps1
tools/desktop/run.ps1
tools/desktop/book-verify.ps1 $HOME\Documents\openings.dbook
```

Windowsの既定ではスクリプトの実行が禁止されている。`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`で許可するか、`powershell -ExecutionPolicy Bypass -File tools/desktop/check.ps1`のように実行する。

`env.ps1`は共通設定で、ほかのスクリプトから読み込む。起動中のアプリはexeをロックするので、`build.ps1`はアプリが動いていると中止する。
