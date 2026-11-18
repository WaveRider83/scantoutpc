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
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build a echoue" }
Write-Host ""
Write-Host "== BUILD OK ==" -ForegroundColor Green
Write-Host "Executable : $PSScriptRoot\target\release\scantoutpc.exe"
