# デスクトップ版アプリをビルドして起動する。
# 使い方:
#   tools/desktop/run.ps1           テスト用ホーム（local/desktop-home）で起動し、普段の設定や棋譜を変更しない
#   tools/desktop/run.ps1 -UserHome ユーザー自身の通常の設定と棋譜を使って起動する
#   tools/desktop/run.ps1 -NoBuild  ビルドを省略し、前回のビルドを起動する
param([switch]$UserHome, [switch]$NoBuild)
. (Join-Path $PSScriptRoot 'env.ps1')

if (-not $NoBuild) { & (Join-Path $PSScriptRoot 'build.ps1') -Desktop }
if ($UserHome) {
    Remove-Item Env:DEFT_REVERSI_HOME -ErrorAction SilentlyContinue
} else {
    New-Item -ItemType Directory -Force $TestHome | Out-Null
    $env:DEFT_REVERSI_HOME = $TestHome
    Write-Host "Home: $TestHome"
}
Start-Process (Join-Path $Release 'deft-reversi.exe') -WorkingDirectory $Root
