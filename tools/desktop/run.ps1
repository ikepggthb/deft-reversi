# Builds and starts the desktop app.
#   tools/desktop/run.ps1           with the test home (local/desktop-home),
#                                   so your own settings and games stay untouched
#   tools/desktop/run.ps1 -UserHome with your usual settings and games
#   tools/desktop/run.ps1 -NoBuild  start the last build
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
