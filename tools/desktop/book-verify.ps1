# Book ファイルが開けるかを CLI で検証する。拡張子から .dbook（対局用）と .dbook-work（作業用）を判別する。
# 使い方:
#   tools/desktop/book-verify.ps1 C:\path\openings.dbook
#   tools/desktop/book-verify.ps1 C:\path\openings.dbook-work
param([Parameter(Mandatory)][string]$Path)
. (Join-Path $PSScriptRoot 'env.ps1')

$cli = Join-Path $Release 'deft-reversi-cli.exe'
if (-not (Test-Path $cli)) { throw "No CLI at $cli; run tools/desktop/build.ps1 first." }
$kind = if ($Path.EndsWith('.dbook-work')) { '--work' } else { '--book' }
Invoke-Native $cli @('book', 'verify', $kind, $Path)
