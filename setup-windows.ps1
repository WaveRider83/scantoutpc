# ScantoutPC - installation & build sur Windows 10/11
# Clic droit > "Executer avec PowerShell" ou : powershell -ExecutionPolicy Bypass -File setup-windows.ps1
$ErrorActionPreference = "Stop"
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
Write-Host "== ScantoutPC - setup Windows ==" -ForegroundColor Cyan
if (-not (Have winget)) { Write-Host "winget introuvable. Installe 'App Installer' depuis le Microsoft Store." -ForegroundColor Red; exit 1 }
if (-not (Have cargo)) {
  Write-Host "Installation de Rust..."
  winget install Rustlang.Rustup --accept-source-agreements --accept-package-agreements
  $env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
} else { Write-Host "Cargo: $(cargo --version) OK" }
Set-Location $PSScriptRoot
$proc = Get-Process -Name scantoutpc -ErrorAction SilentlyContinue
if ($proc) { Write-Host "Fermeture de ScantoutPC en cours..."; $proc | Stop-Process -Force }
cargo build --release

# copie les modeles OCR a cote de l'exe (l'app les cherche pres de l'executable)
$models = Join-Path $PSScriptRoot "models"
$dst = Join-Path $PSScriptRoot "target\release\models"
if (Test-Path $models) {
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item -Force (Join-Path $models "*") $dst
  Write-Host "Modeles OCR copies dans $dst"
} else { Write-Host "AVERTISSEMENT: dossier models absent" -ForegroundColor Yellow }
if ($LASTEXITCODE -ne 0) { throw "cargo build a echoue" }
Write-Host ""
Write-Host "== BUILD OK ==" -ForegroundColor Green
Write-Host "Executable : $PSScriptRoot\target\release\scantoutpc.exe"
