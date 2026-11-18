# Build sur Windows 10

1. Dézipper le projet (ex. `C:\scantoutpc`)
2. Clic droit sur `setup-windows.ps1` → **Exécuter avec PowerShell**
   (si bloqué : `powershell -ExecutionPolicy Bypass -File setup-windows.ps1`)
3. Le script installe Node LTS, Rust, WebView2 si absents, puis build l'app.
4. L'exe final est dans `src-tauri\target\release\scantoutpc.exe` (+ installateur dans `bundle\`).

Pour du dev : `npm run tauri dev`
