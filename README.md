# ScantoutPC

OCR live sur Windows 10/11 : zone fixe définie par l'utilisateur, survol → texte reconnu en direct.

## Fonctionnalités (cible)
- Zone fixe configurable, raccourci global
- Copie auto presse-papier + fenêtre flottante (copier/éditer)
- Langues : français, anglais, chinois (simp./trad.), coréen
- Traduction intégrée, 100 % hors-ligne

## Stack retenue (2026-11-18)
- **App** : Rust + egui (eframe). Pas de Node/npm, binaire unique. Tray via `tray-icon`, raccourci global Ctrl+Shift+O via `global-hotkey`, overlay de sélection en second viewport egui. Cibles Windows 10/11
- **OCR** : PP-OCR (PaddleOCR) via ONNX Runtime (`ort`), local — bon en zh/ko, aucune dépendance aux packs linguistiques Windows
- **Traduction** : Argos Translate, hors-ligne (fr/en/zh/ko) — tourne en sidecar Python embarqué (pas de binding Rust natif)
- Packaging : à définir (MSI/MSIX ou exe portable)

## Ouvert
- Périmètre MVP exact, UX détaillée, format de la fenêtre flottante, historique des captures

## Installation (portable)

1. Télécharger `ScantoutPC-0.2.1-portable.zip` depuis les [releases](https://github.com/WaveRider83/scantoutpc/releases) et dézipper où vous voulez (le dossier est portable : config, captures et modèles vivent à côté de `scantoutpc.exe`).
2. Lancer `scantoutpc.exe`. Raccourci par défaut : `Ctrl+Shift+O`.

### Packs de langue chinois / coréen

Le zip de base contient uniquement le modèle **latin** (FR/EN). Pour le chinois et le coréen :

- **Depuis l'app** : menu tray → Options → « Langue OCR : 中文 » ou « Langue OCR : 한국어 » — le pack est téléchargé automatiquement depuis la release GitHub et installé dans `models\` à côté de l'exe.
- **Manuellement** : télécharger `rec_chinese.onnx` + `rec_chinese.dict` (chinois) ou `rec_korean.onnx` + `rec_korean.dict` (coréen) depuis les [assets de la release](https://github.com/WaveRider83/scantoutpc/releases/tag/v0.2.1) et les copier dans le dossier `models\` à côté de `scantoutpc.exe`, puis relancer l'app.

Le français et l'anglais fonctionnent sans rien télécharger.

Vérifier l'intégrité du zip : `SHA256SUMS.txt` (fourni dans la release).
