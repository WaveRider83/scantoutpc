# ScantoutPC

OCR live sur Windows 10/11 : zone fixe définie par l'utilisateur, survol → texte reconnu en direct.

## Fonctionnalités (cible)
- Zone fixe configurable, raccourci global
- Copie auto presse-papier + fenêtre flottante (copier/éditer)
- Langues : français, anglais, chinois (simp./trad.), coréen
- Traduction intégrée, 100 % hors-ligne

## Stack retenue (2026-11-18)
- **App** : Rust (windows-rs), Windows 10/11
- **OCR** : PP-OCR (PaddleOCR) via ONNX Runtime (`ort`), local — bon en zh/ko, aucune dépendance aux packs linguistiques Windows
- **Traduction** : Argos Translate, hors-ligne (fr/en/zh/ko) — tourne en sidecar Python embarqué (pas de binding Rust natif)
- Packaging : à définir (MSI/MSIX ou exe portable)

## Ouvert
- Périmètre MVP exact, UX détaillée, format de la fenêtre flottante, historique des captures
