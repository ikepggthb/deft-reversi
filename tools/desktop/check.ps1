# cargo clippy とテストを実行する。
# 使い方:
#   tools/desktop/check.ps1             デスクトップ版のクレートのみ検査する
#   tools/desktop/check.ps1 -Workspace  ワークスペース内の全クレート（book、CLI、エンジンなど）を検査する
param([switch]$Workspace)
. (Join-Path $PSScriptRoot 'env.ps1')

$scope = if ($Workspace) { @('--workspace') } else { @('-p', 'deft_reversi_desktop') }
Push-Location $Root
try {
    Invoke-Native cargo (@('clippy', '--release', '--all-targets') + $scope)
    Invoke-Native cargo (@('test', '--release') + $scope)
} finally {
    Pop-Location
}
Write-Host 'clippy and tests passed'
