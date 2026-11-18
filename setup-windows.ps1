# ScantoutPC — installation & build sur Windows 10
# Usage : clic droit > "Exécuter avec PowerShell"  (ou : powershell -ExecutionPolicy Bypass -File setup-windows.ps1)
$ErrorActionPreference = "Stop"

function Have($cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

Write-Host "== ScantoutPC — setup Windows ==" -ForegroundColor Cyan

# 1. winget requis pour l'install auto
if (-not (Have winget)) {
  Write-Host "winget introuvable. Installe 'App Installer' depuis le Microsoft Store puis relance." -ForegroundColor Red
  exit 1
}

# 2. Node.js LTS
if (Have node) { Write-Host "Node: $(node --version) OK" }
else {
  Write-Host "Installation de Node.js LTS..."
  winget install OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements
}

# 3. Rust (rustup)
if (Have cargo) { Write-Host "Cargo: $(cargo --version) OK" }
else {
  Write-Host "Installation de Rust..."
  winget install Rustlang.Rustup --accept-source-agreements --accept-package-agreements
}
# rafraîchir le PATH pour la session
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")

# 4. WebView2 (présent par défaut sur Win11 ; Win10 souvent absent)
$wv2 = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
if (Test-Path $wv2) { Write-Host "WebView2 OK" }
else {
  Write-Host "Installation de WebView2 Runtime..."
  winget install Microsoft.EdgeWebView2Runtime --accept-source-agreements --accept-package-agreements
}

# 5. Build frontend + app Rust
Set-Location $PSScriptRoot
Write-Host "npm install..."
npm install
if ($LASTEXITCODE -ne 0) { throw "npm install a échoué" }

Write-Host "Build Tauri (release)..."
npm run tauri build
if ($LASTEXITCODE -ne 0) { throw "tauri build a échoué" }

$exe = Get-ChildItem "src-tauri\target\release\*.exe" | Select-Object -First 1
Write-Host ""
Write-Host "== BUILD OK ==" -ForegroundColor Green
Write-Host "Exécutable : $($exe.FullName)"
$msi = Get-ChildItem "src-tauri\target\release\bundle" -Recurse -Include *.msi,*.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if ($msi) { Write-Host "Installateur : $($msi.FullName)" }
