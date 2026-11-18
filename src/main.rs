// ScantoutPC — OCR live pour Windows (fenêtre flottante + overlay de sélection)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use tray_icon::{TrayIconBuilder, Icon};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
mod ocr;

// zone en points logiques (egui) ; ppp = pixels_per_point de la fenêtre overlay
#[derive(Clone)]
struct PickedZone {
    monitor: String,
    x: i32, y: i32, w: i32, h: i32, // points logiques relatifs à l'écran
    ppp: f32,
}

fn monitor_key(m: &xcap::Monitor) -> String {
    m.name().unwrap_or_else(|_| "?".into())
}

fn capture_zone(sel: &PickedZone) -> Result<image::RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let mon = monitors.iter().find(|m| monitor_key(m) == sel.monitor)
        .ok_or_else(|| format!("écran '{}' introuvable", sel.monitor))?;
    let p = sel.ppp;
    let zx = (sel.x as f32 * p).round() as i32;
    let zy = (sel.y as f32 * p).round() as i32;
    let zw = ((sel.w as f32) * p).round().max(1.0) as i32;
    let zh = ((sel.h as f32) * p).round().max(1.0) as i32;
    let img = mon.capture_image().map_err(|e| e.to_string())?;
    let iw = img.width() as i32;
    let ih = img.height() as i32;
    let x = zx.clamp(0, iw.saturating_sub(1));
    let y = zy.clamp(0, ih.saturating_sub(1));
    let w = zw.min(iw - x);
    let h = zh.min(ih - y);
    if w <= 0 || h <= 0 { return Err("zone hors écran".into()); }
    let mut out = image::imageops::crop_imm(&img, x as u32, y as u32, w as u32, h as u32).to_image();
    for px in out.pixels_mut() { px.0[3] = 255; }
    Ok(out)
}

fn main() -> eframe::Result<()> {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}\n");
        let _ = std::fs::write(std::env::temp_dir().join("scantoutpc_panic.log"), &msg);
        let _ = std::fs::write(r"<local>\scantoutpc\scantoutpc_panic.log", &msg);
    }));
    // pointe la DLL ONNX Runtime embarquee (build avec load-dynamic)
    if std::env::var_os("ORT_DYLIB_PATH").is_none() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let candidate = if cfg!(windows) {
                    dir.join("models").join("onnxruntime.dll")
                } else {
                    dir.join("models").join("libonnxruntime.so")
                };
                if candidate.exists() {
                    unsafe { std::env::set_var("ORT_DYLIB_PATH", &candidate) };
                }
            }
        }
    }
    let cfg = load_config();
    let hotkey = cfg.hotkey;
    dbg_log(&format!("config: {:?}", config_path()));
    let opt_auto = CheckMenuItem::with_id("opt_auto", "Copie auto dans le presse-papier", true, cfg.auto_copy, None);
    let opt_keep = CheckMenuItem::with_id("opt_keep", "Conserver l'historique des captures", true, cfg.keep_history, None);
    let opt_clear = MenuItem::with_id("opt_clear", "Effacer l'historique", true, None);
    let opt_light = CheckMenuItem::with_id("opt_light", "Thème clair", true, cfg.light_mode, None);
    let opt_autostart = CheckMenuItem::with_id("opt_autostart", "Démarrer avec Windows", true, autostart_enabled(), None);
    let opt_start_mini = CheckMenuItem::with_id("opt_start_mini", "Réduire au lancement", true, cfg.start_mini, None);
    let opt_resize = CheckMenuItem::with_id("opt_resize", "Adapter la fenêtre à la capture", true, cfg.auto_resize, None);
    let lang_latin = CheckMenuItem::with_id("lang_latin", "Langue OCR : FR/EN (latin)", true, cfg.ocr_lang == "latin", None);
    let lang_zh = CheckMenuItem::with_id("lang_zh", "Langue OCR : 中文 (chinois)", true, cfg.ocr_lang == "zh", None);
    let lang_ko = CheckMenuItem::with_id("lang_ko", "Langue OCR : 한국어 (coréen)", true, cfg.ocr_lang == "ko", None);
    let options_sub = Submenu::with_id("options", "Options", true);
    let _ = options_sub.append_items(&[&opt_auto, &opt_keep, &opt_clear, &opt_light, &opt_autostart, &opt_start_mini, &opt_resize, &PredefinedMenuItem::separator(), &lang_latin, &lang_zh, &lang_ko]);
    let tray_opts = TrayOptions {
        auto_copy: opt_auto.clone(),
        keep_history: opt_keep.clone(),
        light: opt_light.clone(),
        start_mini: opt_start_mini.clone(),
        autostart: opt_autostart.clone(),
        resize: opt_resize.clone(),
        latin: lang_latin.clone(),
        zh: lang_zh.clone(),
        ko: lang_ko.clone(),
    };
    let tray_menu = Menu::new();
    let item_toggle = MenuItem::with_id("toggle", "Afficher / Masquer", true, None);
    let item_zone = MenuItem::with_id("zone", "Definir la zone...", true, None);
    let item_quit = MenuItem::with_id("quit", "Quitter", true, None);
    let _ = tray_menu.append_items(&[&item_toggle, &item_zone, &PredefinedMenuItem::separator(), &options_sub, &PredefinedMenuItem::separator(), &item_quit]);
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ScantoutPC")
        .with_icon(make_icon())
        .build()
        .expect("tray icon");

    let hotkey_manager = GlobalHotKeyManager::new().expect("hotkey manager");
    if let Err(e) = hotkey_manager.register(hotkey) {
        eprintln!("hotkey register failed: {e:?}; fallback Ctrl+Shift+O");
        let _ = hotkey_manager.register(default_hotkey());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ScantoutPC")
            .with_inner_size(if cfg.start_mini { [MINI_W, MINI_H] } else { [460.0, 360.0] })
            .with_min_inner_size([MINI_W, MINI_H])
            .with_decorations(true)
            .with_always_on_top()
            .with_resizable(true)
            .with_icon({ let (rgba, w, h) = icon_rgba(); egui::IconData { rgba, width: w, height: h } }),
        ..Default::default()
    };

    eframe::run_native(
        "ScantoutPC",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ScantoutApp {
                text: String::new(),
                status: "capture inactive".into(),
                zone: None,
                picking: false,
                drag_start: None,
                drag_now: None,
                drag_screen: None,
                main_visible: true,
                capture: None,
                overlay_active: false,
                overlay_shots: None,
                pending_capture: None,
                auto_capture_at: None,
                pick_at: None,
                keep_history: cfg.keep_history,
                auto_copy: cfg.auto_copy,
                light_mode: cfg.light_mode,
                start_mini: cfg.start_mini,
                start_mini_pending: cfg.start_mini,
                auto_resize: cfg.auto_resize,
                mini: false,
                mini_since: None,
                mini_collapsing: None,
                prev_rect: None,
                hist_tex: std::collections::HashMap::new(),
                tray_opts,
                ocr_lang: ocr_lang_from_key(&cfg.ocr_lang),
                ocr_engine: None,
                hotkey_label: hotkey_label(&hotkey),
                hotkey,
                hotkey_manager,
                recording: false,
            }))
        }),
    )
}

fn icon_rgba() -> (Vec<u8>, u32, u32) {
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let i = (y * 32 + x) * 4;
            let border = x < 2 || y < 2 || x >= 30 || y >= 30;
            let inner = (10..22).contains(&x) && (12..20).contains(&y);
            let (r, g, b, a) = if inner { (255, 255, 255, 255) } else if border { (96, 165, 250, 255) } else { (37, 99, 235, 255) };
            rgba[i] = r; rgba[i + 1] = g; rgba[i + 2] = b; rgba[i + 3] = a;
        }
    }
    (rgba, 32, 32)
}


// DEBUG TEMP : premier dossier de dump creable (a retirer apres le debug)
fn dump_dir() -> Option<std::path::PathBuf> {
    let mut cand: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("SCANTOUT_CAPTURES") { cand.push(std::path::PathBuf::from(p)); }
    cand.push(std::path::PathBuf::from(r"<local>\scantoutpc\captures"));
    cand.push(std::path::PathBuf::from(r"\\local-share\<local>\scantoutpc\captures"));
    cand.push(std::path::PathBuf::from(r"\\local-share\scantoutpc\captures"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() { cand.push(d.join("captures")); }
    }
    cand.into_iter().find(|d| d.exists() || std::fs::create_dir_all(d).is_ok())
}

fn history_append(dir: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("history.log")) {
        let _ = writeln!(f, "{}", line);
    }
}


const MINI_W: f32 = 230.0;
const MINI_H: f32 = 30.0;
const NORMAL_SIZE: egui::Vec2 = egui::vec2(460.0, 360.0);

impl ScantoutApp {
    fn enter_mini(&mut self, ctx: &egui::Context) {
        dbg_log(&format!("enter_mini (was mini={} main_visible={})", self.mini, self.main_visible));
        if !self.mini {
            self.prev_rect = ctx.input(|i| i.viewport().outer_rect);
        }
        self.mini = true;
        self.mini_collapsing = None;
        self.mini_since = Some(std::time::Instant::now());
        self.main_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, MINI_H)));
        if let Some(ms) = ctx.input(|i| i.viewport().monitor_size) {
            let taskbar = 46.0;
            let pos = egui::pos2((ms.x - MINI_W - 10.0).max(0.0), (ms.y - MINI_H - taskbar).max(0.0));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
    }
    fn exit_mini(&mut self, ctx: &egui::Context) {
        dbg_log(&format!("exit_mini (mini={} main_visible={})", self.mini, self.main_visible));
        self.mini = false;
        self.mini_collapsing = None;
        self.mini_since = None;
        self.main_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(NORMAL_SIZE));
        let mon = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        if let Some(r) = self.prev_rect {
            let x = r.min.x.clamp(0.0, (mon.x - NORMAL_SIZE.x).max(0.0));
            let y = r.min.y.clamp(0.0, (mon.y - NORMAL_SIZE.y).max(0.0));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                (mon.x - NORMAL_SIZE.x - 10.0).max(0.0),
                (mon.y - NORMAL_SIZE.y - 46.0).max(0.0),
            )));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

fn clear_history_files() -> usize {
    let mut n = 0;
    if let Some(d) = dump_dir() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                if let Some(name) = e.file_name().to_str().map(|x| x.to_string()) {
                    if name.starts_with("cap_") || name.starts_with("ocr_") || name == "history.log" {
                        if std::fs::remove_file(e.path()).is_ok() { n += 1; }
                    }
                }
            }
        }
    }
    n
}

fn scan_history() -> Vec<(u64, std::path::PathBuf, Option<std::path::PathBuf>)> {
    let mut v = Vec::new();
    if let Some(d) = dump_dir() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(rest) = name.strip_prefix("cap_") {
                    if let Some(ts) = rest.strip_suffix(".png").and_then(|x| x.parse::<u64>().ok()) {
                        let txt = d.join(format!("ocr_{ts}.txt"));
                        v.push((ts, e.path(), txt.exists().then_some(txt)));
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

fn fmt_ts_local(ts: u64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%d/%m %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}


fn dbg_log(msg: &str) {
    use std::io::Write;
    let line = format!("{} {}\n", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0), msg);
    for p in [std::path::PathBuf::from(r"<local>\scantoutpc\debug.log"), std::env::temp_dir().join("scantoutpc_debug.log")] {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

fn config_path() -> std::path::PathBuf {
    exe_dir().map(|d| d.join("config.ini")).unwrap_or_else(|| std::path::PathBuf::from("config.ini"))
}

fn legacy_config_path() -> Option<std::path::PathBuf> {
    std::env::var("APPDATA").ok().map(|d| std::path::PathBuf::from(d).join("ScantoutPC").join("config.txt"))
}

fn default_hotkey() -> HotKey {
    HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO)
}

fn autostart_enabled() -> bool {
    std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "ScantoutPC"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn set_autostart(enable: bool) -> bool {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let exe = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => return false,
    };
    let r = if enable {
        std::process::Command::new("reg").args(["add", key, "/v", "ScantoutPC", "/t", "REG_SZ", "/d", &exe, "/f"]).status()
    } else {
        std::process::Command::new("reg").args(["delete", key, "/v", "ScantoutPC", "/f"]).status()
    };
    r.map(|s| s.success()).unwrap_or(false)
}

struct AppConfig {
    hotkey: HotKey,
    start_mini: bool,
    auto_copy: bool,
    keep_history: bool,
    light_mode: bool,
    auto_resize: bool,
    ocr_lang: String,
}

fn ocr_lang_from_key(v: &str) -> ocr::OcrLang {
    match v {
        "zh" => ocr::OcrLang::Chinese,
        "ko" => ocr::OcrLang::Korean,
        _ => ocr::OcrLang::Latin,
    }
}

fn ocr_lang_key(l: &ocr::OcrLang) -> &'static str {
    match l {
        ocr::OcrLang::Chinese => "zh",
        ocr::OcrLang::Korean => "ko",
        _ => "latin",
    }
}

fn parse_bool(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn load_config() -> AppConfig {
    let mut cfg = AppConfig {
        hotkey: default_hotkey(),
        start_mini: false,
        auto_copy: true,
        keep_history: true,
        light_mode: false,
        auto_resize: false,
        ocr_lang: "latin".to_string(),
    };
    // config.ini a cote de l'exe ; migration douce depuis l'ancien %APPDATA% si absent
    let raw = std::fs::read_to_string(config_path()).ok().or_else(|| legacy_config_path().and_then(|p| std::fs::read_to_string(p).ok()));
    if let Some(raw) = raw {
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('[') || line.starts_with(';') || line.starts_with('#') { continue; }
            let Some((k, v)) = line.split_once('=') else { continue };
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            match k.as_str() {
                "hotkey" => { if let Ok(h) = v.parse::<HotKey>() { cfg.hotkey = h; } }
                "start_mini" => cfg.start_mini = parse_bool(v),
                "auto_copy" => cfg.auto_copy = parse_bool(v),
                "keep_history" => cfg.keep_history = parse_bool(v),
                "light_mode" => cfg.light_mode = parse_bool(v),
                "auto_resize" => cfg.auto_resize = parse_bool(v),
                "ocr_lang" => { if matches!(v, "latin" | "zh" | "ko") { cfg.ocr_lang = v.to_string(); } }
                _ => {}
            }
        }
    }
    cfg
}

fn save_config(cfg: &AppConfig) {
    let body = format!(
        "[scantoutpc]\nhotkey={}\nstart_mini={}\nauto_copy={}\nkeep_history={}\nlight_mode={}\nauto_resize={}\nocr_lang={}\n",
        cfg.hotkey,
        cfg.start_mini as u8,
        cfg.auto_copy as u8,
        cfg.keep_history as u8,
        cfg.light_mode as u8,
        cfg.auto_resize as u8,
        cfg.ocr_lang
    );
    let _ = std::fs::write(config_path(), body);
}

fn egui_mods(m: &egui::Modifiers) -> Modifiers {
    let mut mods = Modifiers::empty();
    if m.ctrl || m.command { mods |= Modifiers::CONTROL; }
    if m.shift { mods |= Modifiers::SHIFT; }
    if m.alt { mods |= Modifiers::ALT; }
    mods
}

fn egui_key_to_code(k: egui::Key) -> Option<Code> {
    use egui::Key as K;
    Some(match k {
        K::A => Code::KeyA, K::B => Code::KeyB, K::C => Code::KeyC, K::D => Code::KeyD,
        K::E => Code::KeyE, K::F => Code::KeyF, K::G => Code::KeyG, K::H => Code::KeyH,
        K::I => Code::KeyI, K::J => Code::KeyJ, K::K => Code::KeyK, K::L => Code::KeyL,
        K::M => Code::KeyM, K::N => Code::KeyN, K::O => Code::KeyO, K::P => Code::KeyP,
        K::Q => Code::KeyQ, K::R => Code::KeyR, K::S => Code::KeyS, K::T => Code::KeyT,
        K::U => Code::KeyU, K::V => Code::KeyV, K::W => Code::KeyW, K::X => Code::KeyX,
        K::Y => Code::KeyY, K::Z => Code::KeyZ,
        K::Num0 => Code::Digit0, K::Num1 => Code::Digit1, K::Num2 => Code::Digit2,
        K::Num3 => Code::Digit3, K::Num4 => Code::Digit4, K::Num5 => Code::Digit5,
        K::Num6 => Code::Digit6, K::Num7 => Code::Digit7, K::Num8 => Code::Digit8, K::Num9 => Code::Digit9,
        K::F1 => Code::F1, K::F2 => Code::F2, K::F3 => Code::F3, K::F4 => Code::F4,
        K::F5 => Code::F5, K::F6 => Code::F6, K::F7 => Code::F7, K::F8 => Code::F8,
        K::F9 => Code::F9, K::F10 => Code::F10, K::F11 => Code::F11, K::F12 => Code::F12,
        K::Space => Code::Space, K::Enter => Code::Enter, K::Tab => Code::Tab,
        K::Escape => Code::Escape, K::Backspace => Code::Backspace,
        K::Delete => Code::Delete, K::Insert => Code::Insert,
        K::ArrowUp => Code::ArrowUp, K::ArrowDown => Code::ArrowDown,
        K::ArrowLeft => Code::ArrowLeft, K::ArrowRight => Code::ArrowRight,
        K::Home => Code::Home, K::End => Code::End, K::PageUp => Code::PageUp, K::PageDown => Code::PageDown,
        K::Minus => Code::Minus, K::Equals => Code::Equal, K::Semicolon => Code::Semicolon,
        K::Comma => Code::Comma, K::Period => Code::Period, K::Slash => Code::Slash,
        K::Backslash => Code::Backslash, K::OpenBracket => Code::BracketLeft,
        K::CloseBracket => Code::BracketRight, K::Backtick => Code::Backquote, K::Quote => Code::Quote,
        _ => return None,
    })
}

fn hotkey_label(hk: &HotKey) -> String {
    let mut parts: Vec<String> = Vec::new();
    if hk.mods.contains(Modifiers::CONTROL) { parts.push("Ctrl".into()); }
    if hk.mods.contains(Modifiers::ALT) { parts.push("Alt".into()); }
    if hk.mods.contains(Modifiers::SHIFT) { parts.push("Maj".into()); }
    if hk.mods.contains(Modifiers::SUPER) || hk.mods.contains(Modifiers::META) { parts.push("Win".into()); }
    let dbg = format!("{:?}", hk.key);
    let key = dbg.strip_prefix("Key").map(String::from)
        .or_else(|| dbg.strip_prefix("Digit").map(String::from))
        .unwrap_or(dbg);
    parts.push(key);
    parts.join("+")
}


fn make_icon() -> Icon {
    let (rgba, w, h) = icon_rgba();
    Icon::from_rgba(rgba, w, h).expect("icon")
}

struct OverlayShot {
    name: String,
    logical_pos: [f32; 2],
    logical_size: [f32; 2],
    scale: f32,
    tex: egui::TextureHandle,
    raw: std::sync::Arc<Vec<u8>>,
    px_w: u32,
    px_h: u32,
}

struct TrayOptions {
    auto_copy: CheckMenuItem,
    keep_history: CheckMenuItem,
    light: CheckMenuItem,
    latin: CheckMenuItem,
    zh: CheckMenuItem,
    ko: CheckMenuItem,
    start_mini: CheckMenuItem,
    resize: CheckMenuItem,
    autostart: CheckMenuItem,
}

struct ScantoutApp {
    text: String,
    status: String,
    zone: Option<PickedZone>,
    picking: bool,
    drag_start: Option<egui::Pos2>,
    drag_now: Option<egui::Pos2>,
    drag_screen: Option<usize>,
    main_visible: bool,
    capture: Option<egui::TextureHandle>,
    overlay_active: bool,
    overlay_shots: Option<Vec<OverlayShot>>,
    pending_capture: Option<image::RgbaImage>,
    auto_capture_at: Option<std::time::Instant>,
    pick_at: Option<std::time::Instant>,
    keep_history: bool,
    auto_copy: bool,
    light_mode: bool,
    start_mini: bool,
    start_mini_pending: bool,
    auto_resize: bool,
    mini: bool,
    mini_since: Option<std::time::Instant>,
    mini_collapsing: Option<std::time::Instant>,
    prev_rect: Option<egui::Rect>,
    hist_tex: std::collections::HashMap<String, egui::TextureHandle>,
    tray_opts: TrayOptions,
    ocr_lang: ocr::OcrLang,
    ocr_engine: Option<ocr::OcrEngine>,
    hotkey_manager: GlobalHotKeyManager,
    hotkey: HotKey,
    hotkey_label: String,
    recording: bool,
}

impl ScantoutApp {
    fn do_capture(&mut self, ctx: &egui::Context) {
        dbg_log("do_capture start");
        match self.zone.clone() {
            Some(z) => match self.pending_capture.take().map(Ok).unwrap_or_else(|| capture_zone(&z)) {
                Ok(img) => {
                    dbg_log("capture_zone ok");
                    let dump = std::env::temp_dir().join("scantoutpc_capture.png");
                    let _ = img.save(&dump);
                    // DEBUG TEMP : dump de la capture pour analyse (a retirer)
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    if self.keep_history {
                        if let Some(d) = dump_dir() {
                            if img.save(d.join(format!("cap_{ts}.png"))).is_ok() {
                                history_append(&d, &format!("{ts} cap cap_{ts}.png {}x{}", img.width(), img.height()));
                                self.status = format!("historique: {}", d.display());
                            }
                        }
                    }
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    self.capture = Some(ctx.load_texture("capture", color, egui::TextureOptions::LINEAR));
                    if self.auto_resize {
                        let pp = ctx.pixels_per_point();
                        let w = (size[0] as f32 / pp + 28.0).clamp(400.0, 1100.0);
                        let h = (size[1] as f32 / pp + 240.0).clamp(340.0, 950.0);
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
                        dbg_log(&format!("auto-resize {w}x{h}"));
                    }
                    let dynimg = image::DynamicImage::ImageRgba8(img);
                    self.status = "ocr…".into();
                    let lang = self.ocr_lang;
                    let need_reload = self.ocr_engine.as_ref().map(|e| e.lang() != lang).unwrap_or(true);
                    if need_reload {
                        self.ocr_engine = None;
                        match ocr::OcrEngine::load(lang) {
                            Ok(e) => self.ocr_engine = Some(e),
                            Err(e) => { self.status = format!("modèles OCR: {e}"); }
                        }
                    }
                    if let Some(engine) = &self.ocr_engine {
                        match engine.recognize(&dynimg) {
                            Ok(text) => {
                                // DEBUG TEMP : texte OCR a cote des dumps
                                if self.keep_history {
                                    if let Some(d) = dump_dir() {
                                        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                                        let _ = std::fs::write(d.join(format!("ocr_{ts}.txt")), &text);
                                        let preview: String = text.chars().take(80).collect::<String>().replace(['\n', '\r'], " ");
                                        history_append(&d, &format!("{ts} ocr chars={} :: {}", text.len(), preview));
                                    }
                                }
                                let text = text.trim().to_string();
                                if text.is_empty() {
                                    self.status = "aucun texte détecté".into();
                                } else {
                                    self.text = text.clone();
                                    if self.auto_copy {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(text);
                                        }
                                        self.status = "texte copié ✓".into();
                                    } else {
                                        self.status = "texte reconnu".into();
                                    }
                                }
                            }
                            Err(e) => self.status = e,
                        }
                    }
                }
                Err(e) => self.status = format!("capture échouée: {e}"),
            },
            None => self.status = "définis d'abord une zone".into(),
        }
        if self.mini {
            self.enter_mini(ctx);
        } // fenetre normale visible : on ne la reduit pas apres la capture
    }

    fn save_cfg(&self) {
        save_config(&AppConfig {
            hotkey: self.hotkey,
            start_mini: self.start_mini,
            auto_copy: self.auto_copy,
            keep_history: self.keep_history,
            light_mode: self.light_mode,
            auto_resize: self.auto_resize,
            ocr_lang: ocr_lang_key(&self.ocr_lang).to_string(),
        });
    }

    fn toggle_main(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        dbg_log(&format!("tray toggle: mini={} main_visible={} minimized={}", self.mini, self.main_visible, minimized));
        if minimized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.main_visible = true;
            return;
        }
        if self.main_visible {
            self.main_visible = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else {
            self.main_visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            if !self.mini {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }
}

impl eframe::App for ScantoutApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(if self.light_mode { egui::Visuals::light() } else { egui::Visuals::dark() });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self.start_mini_pending {
            self.start_mini_pending = false;
            self.enter_mini(ctx);
        }
        if self.recording {
            let events = ctx.input(|i| i.events.clone());
            for ev in events {
                if let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = ev {
                    if key == egui::Key::Escape {
                        self.recording = false;
                        let _ = self.hotkey_manager.register(self.hotkey);
                        self.status = "raccourci inchang\u{e9}".into();
                        break;
                    }
                    match egui_key_to_code(key) {
                        Some(code) => {
                            let mods = egui_mods(&modifiers);
                            if mods.is_empty() {
                                self.status = "ajoute au moins Ctrl, Alt ou Maj".into();
                                continue;
                            }
                            let new_hk = HotKey::new(Some(mods), code);
                            let _ = self.hotkey_manager.unregister(self.hotkey);
                            match self.hotkey_manager.register(new_hk) {
                                Ok(()) => {
                                    self.hotkey = new_hk;
                                    self.hotkey_label = hotkey_label(&new_hk);
                                    self.save_cfg();
                                    self.status = format!("raccourci : {}", self.hotkey_label);
                                    dbg_log(&format!("hotkey set {}", self.hotkey_label));
                                }
                                Err(e) => {
                                    let _ = self.hotkey_manager.register(self.hotkey);
                                    self.status = format!("raccourci refuse ({e})");
                                }
                            }
                            self.recording = false;
                        }
                        None => { self.status = "touche non supportee".into(); }
                    }
                    break;
                }
            }
        }
        // Mini : disparition apres 3 s (animation de recollement)
        if self.mini && !self.picking && self.auto_capture_at.is_none() && self.mini_collapsing.is_none() {
            if let Some(t0) = self.mini_since {
                if t0.elapsed() >= std::time::Duration::from_secs(3) {
                    self.mini_collapsing = Some(std::time::Instant::now());
                    dbg_log("mini collapse start");
                    self.picking = false;
                    self.overlay_shots = None;
                    self.pending_capture = None;
                }
            }
        }
        if let Some(t0) = self.mini_collapsing {
            let p = (t0.elapsed().as_secs_f32() / 0.25).min(1.0);
            let h = (MINI_H * (1.0 - p)).max(1.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, h)));
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            if p >= 1.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, MINI_H)));
                self.mini_collapsing = None;
                self.mini_since = None;
                self.main_visible = false;
                dbg_log("mini hidden");
            }
        }
        if let Some(t) = self.auto_capture_at {
            if std::time::Instant::now() >= t {
                self.auto_capture_at = None;
                self.do_capture(ctx);
            }
        }
        if let Some(t) = self.pick_at {
            if std::time::Instant::now() >= t {
                self.pick_at = None;
                dbg_log("picking=true (differe)");
                self.picking = true;
            }
        }
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id().0.as_str() {
                "toggle" => self.toggle_main(ctx),
                "zone" => self.picking = true,
                "quit" => std::process::exit(0),
                "opt_auto" => {
                    self.auto_copy = !self.auto_copy;
                    self.tray_opts.auto_copy.set_checked(self.auto_copy);
                    self.save_cfg();
                }
                "opt_keep" => {
                    self.keep_history = !self.keep_history;
                    self.tray_opts.keep_history.set_checked(self.keep_history);
                    self.save_cfg();
                }
                "opt_autostart" => {
                    let want = !self.tray_opts.autostart.is_checked();
                    let ok = set_autostart(want);
                    self.tray_opts.autostart.set_checked(if ok { want } else { autostart_enabled() });
                }
                "opt_start_mini" => {
                    self.start_mini = !self.start_mini;
                    self.tray_opts.start_mini.set_checked(self.start_mini);
                    self.save_cfg();
                }
                "opt_resize" => {
                    self.auto_resize = !self.auto_resize;
                    self.tray_opts.resize.set_checked(self.auto_resize);
                    self.save_cfg();
                }
                "opt_light" => {
                    self.light_mode = !self.light_mode;
                    self.tray_opts.light.set_checked(self.light_mode);
                    self.save_cfg();
                }
                "opt_clear" => {
                    clear_history_files();
                    self.hist_tex.clear();
                    self.status = "historique effacé".into();
                }
                "lang_latin" | "lang_zh" | "lang_ko" => {
                    let ev = event.id().0.clone();
                    self.ocr_lang = match ev.as_str() {
                        "lang_zh" => ocr::OcrLang::Chinese,
                        "lang_ko" => ocr::OcrLang::Korean,
                        _ => ocr::OcrLang::Latin,
                    };
                    self.tray_opts.latin.set_checked(ev == "lang_latin");
                    self.tray_opts.zh.set_checked(ev == "lang_zh");
                    self.tray_opts.ko.set_checked(ev == "lang_ko");
                    self.save_cfg();
                }
                _ => {}
            }
        }
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::HotKeyState::Pressed {
                if self.recording { return; }
                dbg_log("hotkey pressed");
                // La selection de zone passe par des overlays egui enfants de la fenetre principale :
                // elle DOIT etre visible, sinon egui panique ("user callback was never called").
                // On restaure la fenetre puis on laisse une ou deux frames s'ecouler avant de
                // creer les overlays.
                let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
                let evis = ctx.input(|i| i.viewport().visible());
                dbg_log(&format!("hotkey state: mini={} main_visible={} minimized={:?} egui_visible={:?}", self.mini, self.main_visible, minimized, evis));
                let was_hidden = !self.main_visible || minimized;
                if self.mini {
                    if was_hidden {
                        self.enter_mini(ctx);
                        self.pick_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(250));
                        dbg_log("mini reshown + pick differe");
                    } else {
                        // deja en mini visible : capturer sans agrandir
                        self.mini_since = Some(std::time::Instant::now());
                        self.picking = true;
                        dbg_log("picking=true (mini, sans agrandir)");
                    }
                } else if was_hidden {
                    self.main_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.pick_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(250));
                    dbg_log("restore + pick differe");
                } else {
                    self.picking = true;
                    dbg_log("picking=true");
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ui.input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            dbg_log(&format!("close requested: mini={} main_visible={}", self.mini, self.main_visible));
            if self.mini {
                self.mini_since = None;
                self.mini_collapsing = None;
                self.main_visible = false;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                self.enter_mini(ui.ctx());
            }
        }
        if self.picking && self.overlay_shots.is_none() {
            self.pending_capture = None;
            dbg_log("spawn overlays start");
            let mut shots = Vec::new();
            if let Ok(monitors) = xcap::Monitor::all() {
                for (i, m) in monitors.iter().enumerate() {
                    if let Ok(img) = m.capture_image() {
                        let scale = m.scale_factor().unwrap_or(1.0);
                        let w = img.width() as usize;
                        let h = img.height() as usize;
                        let mut raw = img.into_raw();
                        for px in raw.chunks_exact_mut(4) { px[3] = 255; }
                        let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &raw);
                        let tex = ctx.load_texture(format!("overlay-mon-{i}"), color, egui::TextureOptions::LINEAR);
                        shots.push(OverlayShot {
                            name: m.name().unwrap_or_else(|_| format!("mon-{i}")),
                            logical_pos: [m.x().unwrap_or(0) as f32 / scale, m.y().unwrap_or(0) as f32 / scale],
                            logical_size: [w as f32 / scale, h as f32 / scale],
                            scale,
                            tex,
                            raw: std::sync::Arc::new(raw),
                            px_w: w as u32,
                            px_h: h as u32,
                        });
                    }
                }
            }
            if shots.is_empty() {
                self.picking = false;
                self.status = "capture d'écran impossible".into();
            } else {
                self.overlay_active = true;
            }
            dbg_log(&format!("spawn overlays done: {} ecran(s)", shots.len()));
            self.overlay_shots = Some(shots);
        }
        if !self.picking && self.overlay_active {
            self.overlay_active = false;
            self.overlay_shots = None;
            if let Ok(monitors) = xcap::Monitor::all() {
                for i in 0..monitors.len() {
                    ctx.send_viewport_cmd_to(egui::ViewportId::from_hash_of(("zone-overlay", i)), egui::ViewportCommand::Close);
                }
            }
        }
        if self.mini {
            ui.horizontal_centered(|ui| {
                ui.add_space(2.0);
                if ui.button(egui::RichText::new("● ScantoutPC — cliquer pour agrandir").small()).clicked() {
                    self.exit_mini(ui.ctx());
                }
            });
        } else {
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut self.text)
                .desired_width(f32::INFINITY)
                .desired_rows(10)
                .hint_text("Le texte capturé apparaîtra ici…"));
        });
        if let Some(tex) = &self.capture {
            let max_w = ui.available_width();
            let scale = (max_w / tex.size_vec2().x).min(120.0 / tex.size_vec2().y).min(1.0);
            ui.add(egui::Image::from_texture(tex).max_width(tex.size_vec2().x * scale));
        }
        let hist = scan_history();
        egui::CollapsingHeader::new(format!("Historique ({} capture{})", hist.len(), if hist.len() > 1 { "s" } else { "" }))
            .id_salt("historique")
            .default_open(false)
            .show(ui, |ui| {
                let hist_h = ui.available_height().max(140.0);
                egui::ScrollArea::vertical().max_height(hist_h).show(ui, |ui| {
                    for (ts, png, txt) in &hist {
                        egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let key = ts.to_string();
                            if !self.hist_tex.contains_key(&key) {
                                if let Ok(bytes) = std::fs::read(png) {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let t = img.thumbnail(120, 72).to_rgba8();
                                        let (w, h) = t.dimensions();
                                        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], t.as_raw());
                                        let tex = ui.ctx().load_texture(format!("hist-{ts}"), color, egui::TextureOptions::LINEAR);
                                        self.hist_tex.insert(key.clone(), tex);
                                    }
                                }
                            }
                            if let Some(tex) = self.hist_tex.get(&key) {
                                ui.add(egui::Image::from_texture(tex).fit_to_exact_size(egui::vec2(72.0, 46.0)));
                            }
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(fmt_ts_local(*ts)).small().weak());
                                let preview: String = txt.as_ref()
                                    .and_then(|p| std::fs::read_to_string(p).ok())
                                    .map(|t| t.split('\n').take(2).collect::<Vec<_>>().join(" "))
                                    .unwrap_or_default();
                                let trunc: String = preview.chars().take(90).collect();
                                ui.label(egui::RichText::new(trunc).small().color(ui.visuals().text_color()));
                                ui.horizontal(|ui| {
                                    if ui.button("Charger").clicked() {
                                        if let Some(p) = txt {
                                            self.text = std::fs::read_to_string(p).unwrap_or_default();
                                        }
                                    }
                                    if ui.button("Copier").clicked() {
                                        if let Some(p) = txt {
                                            if let Ok(t) = std::fs::read_to_string(p) {
                                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                                    let _ = cb.set_text(t);
                                                }
                                                self.status = "copié ✓".into();
                                            }
                                        }
                                    }
                                    if ui.button("✕").clicked() {
                                        let _ = std::fs::remove_file(png);
                                        if let Some(p) = txt { let _ = std::fs::remove_file(p); }
                                        self.hist_tex.remove(&key);
                                        if let Some(d) = dump_dir() {
                                            let hp = d.join("history.log");
                                            if let Ok(content) = std::fs::read_to_string(&hp) {
                                                let kept: Vec<&str> = content.lines().filter(|l| !l.starts_with(&format!("{ts} "))).collect();
                                                let out = if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" };
                                                let _ = std::fs::write(&hp, out);
                                            }
                                        }
                                    }
                                });
                            });
                        });
                        });
                        ui.add_space(4.0);
                    }
                });
            });
        ui.horizontal(|ui| {
            let label = if self.recording {
                egui::RichText::new("… appuie sur la combinaison (Echap = annuler)").strong()
            } else {
                egui::RichText::new(format!("⌨ {}", self.hotkey_label)).strong()
            };
            if ui.button(label).on_hover_text("Cliquer puis presser une nouvelle combinaison").clicked() {
                self.recording = !self.recording;
                if self.recording {
                    let _ = self.hotkey_manager.unregister(self.hotkey);
                    self.status = "en attente d'une combinaison…".into();
                } else {
                    let _ = self.hotkey_manager.register(self.hotkey);
                    self.status = "raccourci inchangé".into();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.status).small().color(egui::Color32::GRAY));
            });
        });

        }
        let parent_visible = self.main_visible && ctx.input(|i| i.viewport().visible()) != Some(false);
        if self.picking && !parent_visible {
            dbg_log(&format!("spawn blocked: main_visible={} egui_visible={:?}", self.main_visible, ctx.input(|i| i.viewport().visible())));
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.picking && parent_visible {
            let shots: Vec<&OverlayShot> = self.overlay_shots.as_ref().map(|v| v.iter().collect()).unwrap_or_default();
            for (idx, shot) in shots.into_iter().enumerate() {
                let builder = egui::ViewportBuilder::default()
                    .with_title("zone")
                    .with_inner_size(shot.logical_size)
                    .with_position(shot.logical_pos)
                    .with_decorations(false)
                    .with_always_on_top()
                    .with_active(true)
                    .with_taskbar(false)
                    .with_resizable(false);
                let tex = shot.tex.clone();
                let mon_name = shot.name.clone();
                let scale = shot.scale;
                let shot_raw = std::sync::Arc::clone(&shot.raw);
                let shot_px_w = shot.px_w;
                let shot_px_h = shot.px_h;
                let drag_start = &mut self.drag_start;
                let drag_now = &mut self.drag_now;
                let drag_screen = &mut self.drag_screen;
                let picking = &mut self.picking;
                let zone = &mut self.zone;
                let auto_capture_at = &mut self.auto_capture_at;
                let pending_capture = &mut self.pending_capture;
                let status = &mut self.status;
                let scale = scale;
                ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("zone-overlay", idx)), builder, |ui, _class| {
                    let screen = ui.max_rect();
                    ui.painter().image(tex.id(), screen, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                    ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(70));
                    ui.painter().text(
                        egui::Pos2::new(screen.center().x, 24.0),
                        egui::Align2::CENTER_TOP,
                        "Glissez pour définir la zone — Échap pour annuler",
                        egui::FontId::proportional(15.0),
                        egui::Color32::WHITE,
                    );
                    if *drag_screen == Some(idx) {
                        if let (Some(s), Some(n)) = (*drag_start, *drag_now) {
                            let rect = egui::Rect::from_two_pos(s, n);
                            ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(59, 130, 246, 60));
                            ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(59, 130, 246)), egui::StrokeKind::Inside);
                        }
                    }
                    let input = ui.input(|i| i.clone());
                    if input.key_pressed(egui::Key::Escape) {
                        dbg_log("zone released");
                        *picking = false;
                        *drag_start = None;
                        *drag_now = None;
                        *drag_screen = None;
                        return;
                    }
                    if input.pointer.button_pressed(egui::PointerButton::Primary) {
                        *drag_start = input.pointer.latest_pos();
                        *drag_now = *drag_start;
                        *drag_screen = Some(idx);
                    }
                    if *drag_screen == Some(idx) && input.pointer.button_down(egui::PointerButton::Primary) {
                        if let Some(p) = input.pointer.latest_pos() {
                            *drag_now = Some(p);
                        }
                    }
                    if *drag_screen == Some(idx) && input.pointer.button_released(egui::PointerButton::Primary) {
                        if let (Some(s), Some(e)) = (*drag_start, *drag_now) {
                            let (x0, x1) = (s.x.min(e.x).round() as i32, s.x.max(e.x).round() as i32);
                            let (y0, y1) = (s.y.min(e.y).round() as i32, s.y.max(e.y).round() as i32);
                            if x1 - x0 >= 20 && y1 - y0 >= 20 {
                                // decouper dans l'image figee affichee (pas de re-capture live)
                                if let (Some(raw), Some(rw), Some(rh)) = (Some(shot_raw.clone()), Some(shot_px_w), Some(shot_px_h)) {
                                    let sc = scale;
                                    let cx = (x0 as f32 * sc).round().max(0.0) as i64;
                                    let cy = (y0 as f32 * sc).round().max(0.0) as i64;
                                    let cw = ((x1 - x0) as f32 * sc).round().max(1.0) as i64;
                                    let ch = ((y1 - y0) as f32 * sc).round().max(1.0) as i64;
                                    let iw = rw as i64; let ih = rh as i64;
                                    let cx = cx.min(iw - 1); let cy = cy.min(ih - 1);
                                    let cw = cw.min(iw - cx); let ch = ch.min(ih - cy);
                                    if cw > 0 && ch > 0 {
                                        let mut out = Vec::with_capacity((cw * ch * 4) as usize);
                                        let stride = iw as usize * 4;
                                        let row_bytes = (cw * 4) as usize;
                                        for yy in 0..ch {
                                            let start = ((cy + yy) as usize) * stride + (cx as usize) * 4;
                                            out.extend_from_slice(&raw[start..start + row_bytes]);
                                        }
                                        if let Some(img) = image::RgbaImage::from_raw(cw as u32, ch as u32, out) {
                                            *pending_capture = Some(img);
                                        }
                                    }
                                }
                                *zone = Some(PickedZone { monitor: mon_name.clone(), x: x0, y: y0, w: x1 - x0, h: y1 - y0, ppp: scale });
                                *status = format!("zone : {}×{} sur {}", x1 - x0, y1 - y0, mon_name);
                                *auto_capture_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
                            }
                        }
                        dbg_log("zone released");
                        *picking = false;
                        *drag_start = None;
                        *drag_now = None;
                        *drag_screen = None;
                    }
                });
            }
        }
    }
}
