# Rebuild rapide : tue ScantoutPC s'il tourne, recompile, relance.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$proc = Get-Process -Name scantoutpc -ErrorAction SilentlyContinue
if ($proc) { Write-Host "Fermeture de ScantoutPC..."; $proc | Stop-Process -Force; Start-Sleep -Seconds 1 }
cargo run --release
