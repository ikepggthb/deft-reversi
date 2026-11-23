# Builds the desktop app and the CLI, optimised for x86-64-v3.
#   tools/desktop/build.ps1          desktop and CLI
#   tools/desktop/build.ps1 -Desktop desktop only
param([switch]$Desktop)
. (Join-Path $PSScriptRoot 'env.ps1')

$packages = @('-p', 'deft_reversi_desktop')
if (-not $Desktop) { $packages += @('-p', 'deft-reversi-cli') }
# A running app locks its exe, and the build would fail to replace it.
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
