# Runs clippy and the tests.
#   tools/desktop/check.ps1             desktop crate
#   tools/desktop/check.ps1 -Workspace  every crate (book, CLI, engine, ...)
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
