# デスクトップ版アプリと CLI を x86-64-v3 向けに最適化してビルドする。
# 使い方:
#   tools/desktop/build.ps1          デスクトップ版と CLI をビルドする
#   tools/desktop/build.ps1 -Desktop デスクトップ版のみビルドする
param([switch]$Desktop)
. (Join-Path $PSScriptRoot 'env.ps1')

$packages = @('-p', 'deft_reversi_desktop')
if (-not $Desktop) { $packages += @('-p', 'deft-reversi-cli') }
# アプリが起動中だと実行ファイルがロックされ、ビルドで上書きできないため事前に停止する。
if (Get-Process deft-reversi -ErrorAction SilentlyContinue) {
    throw 'deft-reversi is running; close it before building.'
}
Push-Location $Root
try {
    Invoke-Native cargo (@('build', '--release') + $packages)
} finally {
    Pop-Location
}
Write-Host "Built into $Release"
