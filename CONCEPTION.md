# ScantoutPC — Conception (v0.1, 2026-11-18)

## Vision
OCR live ultra-simple, intégré à Windows : définir une zone fixe à l'écran, le texte y est reconnu en direct, copié automatiquement, affiché dans une fenêtre flottante.

## Périmètre MVP
1. **Zone fixe** définie par l'utilisateur (overlay de sélection, redimensionnable, position mémorisée).
2. **Capture live** : rafraîchissement périodique de la zone (intervalle réglable, ~1–2 s), OCR automatique quand l'image change (diff de pixels).
3. **Sortie** :
   - texte copié automatiquement dans le presse-papier dès qu'une capture aboutit (option on/off) ;
   - fenêtre flottante sans bordure avec le texte reconnu (éditable, bouton copier).
4. **Traduction** : bouton « Traduire » dans la fenêtre flottante (fr/en/zh/ko, hors-ligne via Argos).
5. **Langues OCR** : français, anglais, chinois (simp./trad.), coréen — modèles PP-OCR ONNX embarqués.
6. **Système** : tray icon, raccourci global (défaut Win+Shift+O) pour activer/couper, démarrage avec Windows (option).

Hors MVP : historique persistant, multi-zones, cloud, reconnaissance manuscrite.

## Architecture
- **ScantoutPC.exe** (Rust) : UI (icône tray, overlay de sélection, fenêtre flottante), capture écran (windows-rs / GDI ou Windows.Graphics.Capture), orchestration.
- **Moteur OCR** : PP-OCR det+rec en ONNX via `ort` (ONNX Runtime), modèles embarqués dans l'installeur.
- **Traduction** : sidecar Python embarqué (argostranslate), communication JSON via stdin/stdout ; à défaut, traduction masquée si sidecar absent.
- Config utilisateur : `%APPDATA%/ScantoutPC/config.toml` (zone, raccourci, langues, intervalles).

## Points ouverts
- Packaging (MSI/MSIX vs exe portable) et embarquement du sidecar Python (PyInstaller ?)
- Intervalle live & seuil de diff à calibrer
- UI : egui (portable, simple) vs WinUI natif — egui favorisé pour simplicité Rust
