# Shared settings for the scripts in tools/desktop; dot-source it.
# Optimised builds for x86-64-v3 go to target-v3, apart from plain cargo builds.
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$env:RUSTFLAGS = '-Ctarget-cpu=x86-64-v3'
$env:CARGO_TARGET_DIR = Join-Path $Root 'target-v3'
$Release = Join-Path $env:CARGO_TARGET_DIR 'release'
# Settings and games of test runs; local/ is never committed.
$TestHome = Join-Path $Root 'local\desktop-home'

function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    # cargo reports progress on stderr, which Windows PowerShell would turn
    # into errors under 'Stop'; the exit code says whether it failed.
    $ErrorActionPreference = 'Continue'
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe exited with $LASTEXITCODE" }
}
