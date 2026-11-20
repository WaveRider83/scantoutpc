# Construit la distribution portable ScantoutPC (zip de base, latin only) + packs zh/ko.
# Usage : .\make-dist.ps1 [-Version 0.2.0]
param([string]$Version = "0.2.0")
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$dist = Join-Path $PSScriptRoot "dist"
$stage = Join-Path $dist "ScantoutPC"
Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
New-Item -ItemType Directory -Force (Join-Path $stage "models") | Out-Null

Write-Host "Build release..."; cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Copy-Item "target\release\scantoutpc.exe" $stage
# ORT DLL (CPU) — les .so Linux ne sont pas embarqués
Copy-Item "models\onnxruntime.dll" $stage
Copy-Item "models\onnxruntime_providers_shared.dll" $stage
# Modèles latin uniquement (zh/ko = packs téléchargeables)
Copy-Item "models\det.onnx","models\rec_latin.onnx","models\rec_latin.dict" (Join-Path $stage "models")
Copy-Item "assets\icon.ico" $stage -ErrorAction SilentlyContinue
Copy-Item "README.md" $stage

# Packs langue (à déposer sur le partage / hébergement packs\)
$packs = Join-Path $dist "packs"
New-Item -ItemType Directory -Force $packs | Out-Null
Copy-Item "models\rec_chinese.onnx","models\rec_chinese.dict" $packs
Copy-Item "models\rec_korean.onnx","models\rec_korean.dict" $packs

$zip = Join-Path $dist "ScantoutPC-$Version-portable.zip"
Compress-Archive -Path $stage -DestinationPath $zip -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
"$hash  ScantoutPC-$Version-portable.zip" | Out-File -Encoding ascii (Join-Path $dist "SHA256SUMS.txt")
Write-Host "OK : $zip"
Write-Host "SHA256: $hash"
Write-Host "Packs zh/ko dans dist\packs (a uploader sur la release GitHub : gh release upload v<ver> dist\packs\*)"
