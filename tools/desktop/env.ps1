# tools/desktop/ 配下の各スクリプトがドットソースして使う共通設定。
# 通常の cargo ビルドとは分け、x86-64-v3 向けに最適化したビルドを target-v3/ へ出力する。
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$env:RUSTFLAGS = '-Ctarget-cpu=x86-64-v3'
$env:CARGO_TARGET_DIR = Join-Path $Root 'target-v3'
$Release = Join-Path $env:CARGO_TARGET_DIR 'release'
# テスト実行時に使うホームディレクトリ。普段の設定や棋譜と分けるために使い、local/ は Git 管理外とする。
$TestHome = Join-Path $Root 'local\desktop-home'

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    # cargo は進捗を stderr に出力するため、$ErrorActionPreference = 'Stop' のままだと
    # Windows PowerShell がエラー扱いにしてしまう。終了コードで成否を判定するため一時的に Continue にする。
    $ErrorActionPreference = 'Continue'
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe exited with $LASTEXITCODE" }
}
