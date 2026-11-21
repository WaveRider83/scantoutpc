# ScantoutPC

<p align="center"><img src="assets/src/logo_nf2_cropped.png" width="160" alt="ScantoutPC"></p>

OCR live sur Windows 10/11 : zone fixe définie par l'utilisateur, survol → texte reconnu en direct.

## Fonctionnalités (cible)
- Zone fixe configurable, raccourci global
- Copie auto presse-papier + fenêtre flottante (copier/éditer)
- Langues : français, anglais, chinois (simp./trad.), coréen
- Traduction intégrée (Argos Translate), 100 % hors-ligne, désactivée par défaut

## Stack retenue (2026-11-18)
- **App** : Rust + egui (eframe). Pas de Node/npm, binaire unique. Tray via `tray-icon`, raccourci global Ctrl+Shift+O via `global-hotkey`, overlay de sélection en second viewport egui. Cibles Windows 10/11
- **Mise à jour** : l'écran À propos affiche la version active et la dernière version publiée ; bouton « Installer maintenant » = téléchargement du zip + mise à jour automatique (updater PowerShell qui attend la fermeture puis remplace les fichiers et relance l'app).
- **OCR** : PP-OCR (PaddleOCR) via ONNX Runtime (`ort`), local — bon en zh/ko, aucune dépendance aux packs linguistiques Windows
- **Traduction** : Argos Translate, hors-ligne — sidecar Python (`translate_sidecar.py` + `argostranslate` via pip), modèles téléchargeables par langue (pivot anglais). Lancer `pip install argostranslate` puis « Télécharger les modèles » dans les Options
- Packaging : à définir (MSI/MSIX ou exe portable)

## Ouvert
- Périmètre MVP exact, UX détaillée, format de la fenêtre flottante, historique des captures

## Installation (portable)

1. Télécharger `ScantoutPC-0.2.3-portable.zip` depuis les [releases](https://github.com/WaveRider83/scantoutpc/releases) et dézipper où vous voulez (le dossier est portable : config, captures et modèles vivent à côté de `scantoutpc.exe`).
2. Lancer `scantoutpc.exe`. Raccourci par défaut : `Ctrl+Shift+O`.

### Packs de langue chinois / coréen

Le zip de base contient uniquement le modèle **latin** (FR/EN). Pour le chinois et le coréen :

- **Depuis l'app** : menu tray → Options → « Langue OCR : 中文 » ou « Langue OCR : 한국어 » — le pack est téléchargé automatiquement depuis la release GitHub et installé dans `models\` à côté de l'exe.
- **Manuellement** : télécharger `rec_chinese.onnx` + `rec_chinese.dict` (chinois) ou `rec_korean.onnx` + `rec_korean.dict` (coréen) depuis les [assets de la release](https://github.com/WaveRider83/scantoutpc/releases/tag/v0.2.3) et les copier dans le dossier `models\` à côté de `scantoutpc.exe`, puis relancer l'app.

Le français et l'anglais fonctionnent sans rien télécharger.

### Traduction intégrée (Argos Translate, hors-ligne)

La traduction utilise [Argos Translate](https://www.argosopentech.com/) via un sidecar Python (`translate_sidecar.py`, fourni dans le zip). Elle est désactivée par défaut.

1. Installer Python 3 (https://www.python.org/) si absent.
2. Menu tray → Options → « Installer/MAJ le moteur (pip) » (équivalent : `py -m pip install --user argostranslate`).
3. Options → cocher « Traduction (Argos, hors-ligne) », puis choisir les langues proposées dans « Langues proposées ».
4. Options → « Télécharger les modèles de traduction… » (modèles `.argosmodel` depuis le dépôt Argos, ~70–150 Mo par langue, pivot anglais).
5. Dans la fenêtre principale : une colonne de drapeaux apparaît à gauche du texte capturé. Le sélecteur en haut choisit la langue source (par défaut `fr`) ; cliquer un drapeau traduit le texte vers cette langue (résultat affiché sous le texte, bouton Copier).

Vérifier l'intégrité du zip : `SHA256SUMS.txt` (fourni dans la release).
