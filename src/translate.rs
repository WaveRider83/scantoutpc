// ScantoutPC — traduction hors-ligne via sidecar Python (Argos Translate)
// Le sidecar (tools/translate_sidecar.py, copié à côté de l'exe) fait tout le travail ;
// ici on ne fait que le découvrir, le lancer et lire son stdout.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub const LANGS: &[(&str, &str)] = &[
    ("fr", "Français"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("zh", "中文"),
    ("ko", "한국어"),
    ("ja", "日本語"),
];

pub fn lang_name(code: &str) -> &'static str {
    LANGS.iter().find(|(c, _)| *c == code).map(|(_, n)| *n).unwrap_or("?")
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

fn sidecar_path() -> Option<PathBuf> {
    let p = exe_dir()?.join("translate_sidecar.py");
    if p.exists() { Some(p) } else {
        // dev : à côté du Cargo.toml
        let dev = std::env::current_dir().ok()?.join("tools").join("translate_sidecar.py");
        if dev.exists() { Some(dev) } else { None }
    }
}

/// Cherche un interpréteur Python utilisable (Windows : py launcher en priorité).
fn python_cmd() -> Option<Vec<String>> {
    let candidates: Vec<Vec<String>> = if cfg!(windows) {
        vec![vec!["py".into(), "-3".into()], vec!["python".into()], vec!["python3".into()]]
    } else {
        vec![vec!["python3".into()], vec!["python".into()]]
    };
    for cmd in candidates {
        let ok = Command::new(&cmd[0])
            .args(&cmd[1..])
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(cmd);
        }
    }
    None
}

fn run_sidecar(args: &[String], stdin_text: Option<&str>, timeout: Duration) -> Result<String, String> {
    let py = python_cmd().ok_or_else(|| "Python 3 introuvable (installez Python 3 puis Options → Installer le moteur)".to_string())?;
    let script = sidecar_path().ok_or_else(|| "translate_sidecar.py introuvable à côté de l'exe".to_string())?;
    let mut cmd = Command::new(&py[0]);
    cmd.args(&py[1..]).arg(&script).args(args)
        .stdin(if stdin_text.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("lancement python: {e}"))?;
    if let Some(text) = stdin_text {
        use std::io::Write;
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(text.as_bytes());
        }
    }
    // wait_with_output bloque ; on le borne via un thread + recv_timeout
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() {
                Ok(stdout)
            } else {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                Err(if err.is_empty() { format!("échec (code {:?})", out.status.code()) } else { err })
            }
        }
        Ok(Err(e)) => Err(format!("exécution python: {e}")),
        Err(_) => Err(format!("délai dépassé ({}s)", timeout.as_secs())),
    }
}

/// Traduit `text` de `from` vers `to` (codes Argos : fr, en, de…). Pivot anglais géré par le sidecar.
pub fn translate(text: &str, from: &str, to: &str) -> Result<String, String> {
    run_sidecar(&["translate".into(), from.into(), to.into()], Some(text), Duration::from_secs(90))
}

/// Télécharge/installe les modèles <lang><->en pour les langues données.
pub fn install_models(langs: &[String]) -> Result<String, String> {
    let mut args = vec!["install".to_string()];
    args.extend(langs.iter().cloned());
    run_sidecar(&args, None, Duration::from_secs(1800))
}

/// pip install --user argostranslate (moteur de traduction).
pub fn pip_install() -> Result<String, String> {
    let py = python_cmd().ok_or_else(|| "Python 3 introuvable".to_string())?;
    let mut cmd = Command::new(&py[0]);
    cmd.args(&py[1..])
        .args(["-m", "pip", "install", "--user", "--upgrade", "argostranslate"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = cmd.output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(Duration::from_secs(900)) {
        Ok(Ok(out)) if out.status.success() => Ok("moteur de traduction installé/à jour".into()),
        Ok(Ok(out)) => Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("échec pip").to_string()),
        Ok(Err(e)) => Err(format!("pip: {e}")),
        Err(_) => Err("pip : délai dépassé".into()),
    }
}
