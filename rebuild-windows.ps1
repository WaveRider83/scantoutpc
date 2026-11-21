# Rebuild rapide : tue ScantoutPC s'il tourne, recompile, relance.
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$proc = Get-Process -Name scantoutpc -ErrorAction SilentlyContinue
if ($proc) { Write-Host "Fermeture de ScantoutPC..."; $proc | Stop-Process -Force; Start-Sleep -Seconds 1 }
cargo build --release --features debuglog

# copie les modeles OCR a cote de l'exe (l'app les cherche pres de l'executable)
$models = Join-Path $PSScriptRoot "models"
$dst = Join-Path $PSScriptRoot "target\release\models"
if (Test-Path $models) {
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item -Force (Join-Path $models "*") $dst
  Write-Host "Modeles OCR copies dans $dst"
} else { Write-Host "AVERTISSEMENT: dossier models absent" -ForegroundColor Yellow }
Start-Process (Join-Path $PSScriptRoot "target\release\scantoutpc.exe")
