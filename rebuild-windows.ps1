# Rebuild rapide : tue ScantoutPC s'il tourne, recompile, relance.
# La sortie est journalisee dans build-latest.log (lisible depuis la VM Linux via I:)
$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot
$log = Join-Path $PSScriptRoot "build-latest.log"

"=== build $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ===" | ForEach-Object { $_; Set-Content -Path $log -Value $_ -Encoding utf8 }

$proc = Get-Process -Name scantoutpc -ErrorAction SilentlyContinue
if ($proc) { "Fermeture de ScantoutPC..." | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }; $proc | Stop-Process -Force; Start-Sleep -Seconds 1 }

cargo build --release --features debuglog 2>&1 | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
if ($LASTEXITCODE -ne 0) {
    "BUILD ECHEC (code $LASTEXITCODE)" | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
    exit $LASTEXITCODE
}

# copie les modeles OCR a cote de l'exe (l'app les cherche pres de l'executable)
$models = Join-Path $PSScriptRoot "models"
$dst = Join-Path $PSScriptRoot "target\release\models"
if (Test-Path $models) {
  New-Item -ItemType Directory -Force $dst | Out-Null
  Copy-Item -Force (Join-Path $models "*") $dst
  "Modeles OCR copies dans $dst" | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
} else { "AVERTISSEMENT: dossier models absent" | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 } }

"BUILD OK" | ForEach-Object { $_; Add-Content -Path $log -Value $_ -Encoding utf8 }
Start-Process (Join-Path $PSScriptRoot "target\release\scantoutpc.exe")
