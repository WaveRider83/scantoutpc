# Rebuild rapide : tue ScantoutPC s'il tourne, recompile, relance.
# La sortie est journalisee dans build-latest.log (lisible depuis la VM Linux via I:)
$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot
$log = Join-Path $PSScriptRoot "build-latest.log"

"=== build $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ===" | Tee-Object -FilePath $log

$proc = Get-Process -Name scantoutpc -ErrorAction SilentlyContinue
if ($proc) { "Fermeture de ScantoutPC..." | Tee-Object -FilePath $log -Append; $proc | Stop-Process -Force; Start-Sleep -Seconds 1 }

cargo build --release --features debuglog 2>&1 | Tee-Object -FilePath $log -Append
if ($LASTEXITCODE -ne 0) {
    "BUILD ECHEC (code $LASTEXITCODE)" | Tee-Object -FilePath $log -Append
    exit $LASTEXITCODE
}

# copie les modeles OCR a cote de l'exe (l'app les cherche pres de l'executable)
$models = Join-Path $PSScriptRoot "models"
$dst = Join-Path $PSScriptRoot "target\release\models"
if (Test-Path $models) {
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item -Force (Join-Path $models "*") $dst
  "Modeles OCR copies dans $dst" | Tee-Object -FilePath $log -Append
} else { "AVERTISSEMENT: dossier models absent" | Tee-Object -FilePath $log -Append }

"BUILD OK" | Tee-Object -FilePath $log -Append
Start-Process (Join-Path $PSScriptRoot "target\release\scantoutpc.exe")
