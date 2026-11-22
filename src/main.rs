// ScantoutPC — OCR live pour Windows (fenêtre flottante + overlay de sélection)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use tray_icon::{TrayIconBuilder, Icon};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
mod barcode;
mod gdi;
mod importer;
mod ocr;
mod translate;

enum TradEvent {
    Translated(String, Result<String, String>), // (langue cible, résultat)
    Finished(Result<String, String>),           // install modèles / pip
}

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
    // capture uniquement la zone demandée (évite de copier tout l'écran en mémoire)
    let mw = mon.width().map_err(|e| e.to_string())? as i32;
    let mh = mon.height().map_err(|e| e.to_string())? as i32;
    let x = zx.clamp(0, mw.saturating_sub(1));
    let y = zy.clamp(0, mh.saturating_sub(1));
    let w = zw.min(mw - x);
    let h = zh.min(mh - y);
    if w <= 0 || h <= 0 { return Err("zone hors écran".into()); }
    let t0 = std::time::Instant::now();
    let ox = mon.x().map_err(|e| e.to_string())?;
    let oy = mon.y().map_err(|e| e.to_string())?;
    let mut out = match gdi::gdi_capture(ox + x, oy + y, w, h) {
        Ok(img) => { dbg_log(&format!("capture_zone: path=gdi zone=({},{} {}x{}) ok en {:?}", x, y, w, h, t0.elapsed())); img }
        Err(e) => { dbg_log(&format!("capture_zone: path=gdi zone=({},{} {}x{}) ECHEC en {:?}: {e}", x, y, w, h, t0.elapsed())); return Err(e); }
    };
    for px in out.pixels_mut() { px.0[3] = 255; }
    Ok(out)
}

fn main() -> eframe::Result<()> {
    #[cfg(feature = "debuglog")]
    std::panic::set_hook(Box::new(|info| {
        dbg_log(&format!("PANIC: {info}"));
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
    let opt_barcode = CheckMenuItem::with_id("opt_barcode", "Décoder les codes-barres (QR, DataMatrix…)", true, cfg.decode_barcodes, None);
    let opt_top = CheckMenuItem::with_id("opt_top", "Toujours au premier plan", true, cfg.always_on_top, None);
    let opt_layout = CheckMenuItem::with_id("opt_layout", "Conserver la mise en page", true, cfg.layout_keep, None);
    let opt_trad = CheckMenuItem::with_id("opt_trad", "Traduction (Argos, hors-ligne)", true, cfg.trad_enabled, None);
    let mut trad_lang_items: Vec<(String, CheckMenuItem)> = Vec::new();
    let trad_langs_sub = Submenu::with_id("trad_langs", "Langues proposées", true);
    for (code, name) in translate::LANGS {
        let item = CheckMenuItem::with_id(format!("trad_lang_{code}"), *name, true, cfg.trad_langs.iter().any(|l| l == code), None);
        let _ = trad_langs_sub.append(&item);
        trad_lang_items.push((code.to_string(), item));
    }
    let trad_dl = MenuItem::with_id("trad_dl", "Télécharger les modèles de traduction…", true, None);
    let trad_pip = MenuItem::with_id("trad_pip", "Installer/MAJ le moteur (pip)…", true, None);
    let lang_latin = CheckMenuItem::with_id("lang_latin", "Langue OCR : FR/EN (latin)", true, cfg.ocr_lang == "latin", None);
    let lang_zh = CheckMenuItem::with_id("lang_zh", "Langue OCR : 中文 (chinois)", true, cfg.ocr_lang == "zh", None);
    let lang_ko = CheckMenuItem::with_id("lang_ko", "Langue OCR : 한국어 (coréen)", true, cfg.ocr_lang == "ko", None);
    let options_sub = Submenu::with_id("options", "Options", true);
    let _ = options_sub.append_items(&[&opt_auto, &opt_keep, &opt_clear, &opt_light, &opt_autostart, &opt_start_mini, &opt_resize, &opt_barcode, &opt_top, &opt_layout, &PredefinedMenuItem::separator(), &opt_trad, &trad_langs_sub, &trad_dl, &trad_pip, &PredefinedMenuItem::separator(), &lang_latin, &lang_zh, &lang_ko]);
    let tray_opts = TrayOptions {
        auto_copy: opt_auto.clone(),
        keep_history: opt_keep.clone(),
        light: opt_light.clone(),
        start_mini: opt_start_mini.clone(),
        autostart: opt_autostart.clone(),
        resize: opt_resize.clone(),
        barcode: opt_barcode.clone(),
        top: opt_top.clone(),
        layout: opt_layout.clone(),
        trad: opt_trad.clone(),
        trad_langs: trad_lang_items.clone(),
        latin: lang_latin.clone(),
        zh: lang_zh.clone(),
        ko: lang_ko.clone(),
    };
    let tray_menu = Menu::new();
    let item_toggle = MenuItem::with_id("toggle", "Afficher / Masquer", true, None);
    let item_zone = MenuItem::with_id("zone", "Definir la zone...", true, None);
    let item_about = MenuItem::with_id("about", "À propos…", true, None);
    let item_quit = MenuItem::with_id("quit", "Quitter", true, None);
    let _ = tray_menu.append_items(&[&item_toggle, &item_zone, &PredefinedMenuItem::separator(), &options_sub, &PredefinedMenuItem::separator(), &item_about, &item_quit]);
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

    // Restauration exacte de la position/taille mémorisées ; le filtre n'écarte que les
    // valeurs aberrantes (fenêtre garée hors écran à -30000). L'utilisateur veut retrouver
    // sa fenêtre exactement où il l'a laissée.
    let saved_pos = if !cfg.start_mini {
        cfg.win_x.zip(cfg.win_y).filter(|(x, y)| x.abs() < 10000 && y.abs() < 10000)
    } else { None };
    let options = eframe::NativeOptions {
        viewport: {
            let vb = egui::ViewportBuilder::default()
            .with_title("ScantoutPC")
            .with_inner_size(if cfg.start_mini { [MINI_W, MINI_H] } else { cfg.win_w.zip(cfg.win_h).map(|(w, h)| [w.max(200.0), h.max(150.0)]).unwrap_or([460.0, 360.0]) });
            let vb = if let Some((x, y)) = saved_pos { vb.with_position([x as f32, y as f32]) } else { vb };
            vb
            .with_min_inner_size([MINI_W, MINI_H])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top() // affiné juste après via WindowLevel si désactivé
            .with_resizable(true)
            .with_icon({ let (rgba, w, h) = icon_rgba(); egui::IconData { rgba, width: w, height: h } })
        },
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
                decode_barcodes: cfg.decode_barcodes,
                always_on_top: cfg.always_on_top,
                layout_keep: cfg.layout_keep,
                top_init: true,
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
                pack_rx: None,
                pack_pending_lang: None,
                show_about: false,
                win_pos: cfg.win_x.zip(cfg.win_y).map(|(x, y)| [x, y]),
                win_size: cfg.win_w.zip(cfg.win_h).map(|(w, h)| [w, h]),
                last_pos_save: None,
                mini_flash: false,
                mini_icon: None,
                about_tex: None,
                update_rx: None,
                update_info: None,
                update_dl_rx: None,
                update_dl_busy: false,
                trad_enabled: cfg.trad_enabled,
                trad_langs: cfg.trad_langs.clone(),
                trad_source: cfg.trad_source.clone(),
                trad_rx: None,
                trad_busy: false,
                trad_result: None,
                title_icon: None,
                show_welcome: cfg.show_welcome,
                welcome_dont_show: false,
                dep_missing: check_dependencies(),
                capture_hidden_pos: None,
                prev_picking: false,
            }))
        }),
    )
}

fn star_points(cx: f32, cy: f32, r: f32) -> Vec<egui::Pos2> {
    (0..10).map(|i| {
        let rad = if i % 2 == 0 { r } else { r * 0.382 };
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        egui::pos2(cx + rad * a.cos(), cy + rad * a.sin())
    }).collect()
}

fn arc_half(cx: f32, cy: f32, r: f32, bottom: bool) -> Vec<egui::Pos2> {
    let steps = 24;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let a = std::f32::consts::PI * (i as f32 / steps as f32);
        let y = if bottom { cy + r * a.sin() } else { cy - r * a.sin() };
        pts.push(egui::pos2(cx + r * a.cos(), y));
    }
    pts
}

fn paint_flag(p: &egui::Painter, rect: egui::Rect, code: &str) {
    use egui::{Color32, Rect, pos2};
    let (l, t, w, h) = (rect.left(), rect.top(), rect.width(), rect.height());
    let rgb = |v: u32| Color32::from_rgb(((v >> 16) & 255) as u8, ((v >> 8) & 255) as u8, (v & 255) as u8);
    let vstripes = |cols: &[u32]| {
        let n = cols.len() as f32;
        for (i, c) in cols.iter().enumerate() {
            let x0 = l + w * i as f32 / n;
            p.rect_filled(Rect::from_min_max(pos2(x0, t), pos2(l + w * (i as f32 + 1.0) / n, t + h)), 0.0, rgb(*c));
        }
    };
    let hstripes = |cols: &[(u32, f32)]| {
        let total: f32 = cols.iter().map(|c| c.1).sum();
        let mut y = t;
        for (c, frac) in cols {
            let hh = h * frac / total;
            p.rect_filled(Rect::from_min_max(pos2(l, y), pos2(l + w, y + hh)), 0.0, rgb(*c));
            y += hh;
        }
    };
    match code {
        "fr" => vstripes(&[0x0055A4, 0xFFFFFF, 0xEF4135]),
        "it" => vstripes(&[0x009246, 0xFFFFFF, 0xCE2B37]),
        "de" => hstripes(&[(0x000000, 1.0), (0xDD0000, 1.0), (0xFFCE00, 1.0)]),
        "es" => hstripes(&[(0xAA151B, 1.0), (0xF1BF00, 2.0), (0xAA151B, 1.0)]),
        "pt" => {
            p.rect_filled(Rect::from_min_max(pos2(l, t), pos2(l + w * 0.42, t + h)), 0.0, rgb(0x046A38));
            p.rect_filled(Rect::from_min_max(pos2(l + w * 0.42, t), pos2(l + w, t + h)), 0.0, rgb(0xDA291C));
            p.circle_filled(pos2(l + w * 0.42, t + h * 0.5), h * 0.22, rgb(0xFFE900));
        }
        "en" => {
            for i in 0..7 {
                let y0 = t + h * i as f32 / 7.0;
                p.rect_filled(Rect::from_min_max(pos2(l, y0), pos2(l + w, y0 + h / 7.0)), 0.0, if i % 2 == 0 { rgb(0xB31942) } else { rgb(0xFFFFFF) });
            }
            p.rect_filled(Rect::from_min_max(pos2(l, t), pos2(l + w * 0.45, t + h * 4.0 / 7.0)), 0.0, rgb(0x0A3161));
            for row in 0..3 {
                for col in 0..4 {
                    p.circle_filled(pos2(l + w * (0.07 + col as f32 * 0.11), t + h * (0.10 + row as f32 * 0.17)), h * 0.035, rgb(0xFFFFFF));
                }
            }
        }
        "zh" => {
            p.rect_filled(rect, 0.0, rgb(0xDE2910));
            let (cx, cy, r) = (l + w * 0.22, t + h * 0.32, h * 0.22);
            let pts = star_points(cx, cy, r);
            let center = egui::pos2(cx, cy);
            // étoile concave : éventail de triangles depuis le centre (dans le noyau)
            for i in 0..10 {
                let tri = vec![center, pts[i], pts[(i + 1) % 10]];
                p.add(egui::Shape::convex_polygon(tri, rgb(0xFFDE00), egui::Stroke::NONE));
            }
        }
        "ko" => {
            p.rect_filled(rect, 0.0, rgb(0xFFFFFF));
            let c = egui::pos2(l + w * 0.5, t + h * 0.5);
            let r = h * 0.32;
            p.circle_filled(c, r, rgb(0xCD2E3A));
            p.add(egui::Shape::convex_polygon(arc_half(c.x, c.y, r, true), rgb(0x0047A0), egui::Stroke::NONE));
        }
        "ja" => {
            p.rect_filled(rect, 0.0, rgb(0xFFFFFF));
            p.circle_filled(egui::pos2(l + w * 0.5, t + h * 0.5), h * 0.30, rgb(0xBC002D));
        }
        _ => { p.rect_filled(rect, 0.0, rgb(0x333333)); }
    }
    p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, Color32::from_black_alpha(120)), egui::StrokeKind::Inside);
}

fn flag_button(ui: &mut egui::Ui, code: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(38.0, 25.0), egui::Sense::click());
    paint_flag(ui.painter(), rect, code);
    if resp.hovered() {
        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 200, 80)), egui::StrokeKind::Outside);
    }
    resp.on_hover_text(format!("Traduire vers {}", translate::lang_name(code)))
}

fn icon_rgba() -> (Vec<u8>, u32, u32) {
    // icône générée (NitroFusion2) — assets/icon_256.png embarquée
    let img = image::load_from_memory(include_bytes!("../assets/icon_256.png"))
        .expect("icône embarquée illisible")
        .to_rgba8();
    let (w, h) = img.dimensions();
    (img.into_raw(), w, h)
}


fn dump_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.join("captures");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn history_append(dir: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("history.log")) {
        let _ = writeln!(f, "{}", line);
    }
}


/// Zone de travail (hors barre des tâches) du moniteur le plus proche de (x, y), en pixels écran.
#[cfg(windows)]
fn work_area_near(x: f32, y: f32) -> Option<(f32, f32, f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let pt = POINT { x: x as i32, y: y as i32 };
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if mon.is_null() {
            return None;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let r = mi.rcWork;
        Some((r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32))
    }
}
#[cfg(not(windows))]
fn work_area_near(_x: f32, _y: f32) -> Option<(f32, f32, f32, f32)> {
    None
}

const MINI_W: f32 = 230.0;
const MINI_H: f32 = 150.0;
const NORMAL_SIZE: egui::Vec2 = egui::vec2(460.0, 360.0);

impl ScantoutApp {
    fn enter_mini(&mut self, ctx: &egui::Context) {
        dbg_log(&format!("enter_mini (was mini={} main_visible={})", self.mini, self.main_visible));
        if !self.mini {
            self.prev_rect = ctx.input(|i| i.viewport().outer_rect);
        }
        self.mini = true;
        self.mini_flash = false;
        self.mini_collapsing = None;
        self.mini_since = Some(std::time::Instant::now());
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
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
        self.mini_flash = false;
        self.mini_collapsing = None;
        self.mini_since = None;
        self.main_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        let restored = self.win_size.map(|s| egui::vec2(s[0].max(200.0), s[1].max(150.0))).unwrap_or(NORMAL_SIZE);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(restored));
        let mon = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1920.0, 1080.0));
        if let Some(r) = self.prev_rect {
            let x = r.min.x.clamp(0.0, (mon.x - NORMAL_SIZE.x).max(0.0));
            let y = r.min.y.clamp(0.0, (mon.y - NORMAL_SIZE.y).max(0.0));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        } else if let Some(p) = self.win_pos {
            let x = (p[0] as f32).clamp(0.0, (mon.x - NORMAL_SIZE.x).max(0.0));
            let y = (p[1] as f32).clamp(0.0, (mon.y - NORMAL_SIZE.y).max(0.0));
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
                        let exact = d.join(format!("ocr_{ts}.txt"));
                        let txt = if exact.exists() {
                            Some(exact)
                        } else {
                            std::fs::read_dir(&d).ok().and_then(|rd| {
                                rd.flatten()
                                    .filter_map(|f| f.file_name().to_string_lossy().strip_prefix("ocr_").and_then(|r| r.strip_suffix(".txt")).and_then(|n| n.parse::<u64>().ok()).map(|t| (t, f.path())))
                                    .filter(|(t, _)| t.abs_diff(ts) <= 5)
                                    .min_by_key(|(t, _)| t.abs_diff(ts))
                                    .map(|(_, p)| p)
                            })
                        };
                        v.push((ts, e.path(), txt));
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


#[cfg(feature = "debuglog")]
fn dbg_log(msg: &str) {
    use std::io::Write;
    if let Some(dir) = exe_dir() {
        let line = format!("{} {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("debug.log"))
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
}

#[cfg(not(feature = "debuglog"))]
fn dbg_log(_msg: &str) {}

fn exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

// Packs de langue OCR téléchargeables à la demande (zh/ko trop lourds pour le zip de base).
// Source : assets de la release GitHub ; surchargeable via SCANTOUT_PACK_BASE_URL
// (ex. file:///... pour un dossier local ou un partage réseau).
fn pack_base_url() -> String {
    std::env::var("SCANTOUT_PACK_BASE_URL")
        .unwrap_or_else(|_| format!("https://github.com/WaveRider83/scantoutpc/releases/download/v{}", env!("CARGO_PKG_VERSION")))
        .trim_end_matches('/')
        .to_string()
}

fn parse_ver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    Some((a, b, c))
}

struct ReleaseInfo {
    tag: String,
    is_newer: bool,
}

fn http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(20))
        .build()
}

fn latest_tag_from_api() -> Result<String, String> {
    let resp = http_agent()
        .get("https://api.github.com/repos/WaveRider83/scantoutpc/releases/latest")
        .set("User-Agent", "scantoutpc")
        .call()
        .map_err(|e| e.to_string())?;
    let body = resp.into_string().map_err(|e| e.to_string())?;
    body.split("\"tag_name\"")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .map(|t| t.to_string())
        .ok_or_else(|| "tag_name introuvable (rate-limit ou réponse inattendue)".to_string())
}

// Fallback sans API (pas de quota) : suit la redirection de /releases/latest et lit le tag dans l'URL finale.
fn latest_tag_from_web() -> Result<String, String> {
    let resp = http_agent()
        .get("https://github.com/WaveRider83/scantoutpc/releases/latest")
        .set("User-Agent", "scantoutpc")
        .call()
        .map_err(|e| e.to_string())?;
    let url = resp.get_url().to_string();
    url.rsplit("/tag/")
        .next()
        .filter(|t| t.starts_with('v'))
        .map(|t| t.to_string())
        .ok_or_else(|| format!("tag introuvable dans {url}"))
}

fn check_latest_release() -> Result<ReleaseInfo, String> {
    let tag = latest_tag_from_api().or_else(|e1| {
        dbg_log(&format!("check MAJ via API échoué ({e1}), fallback page web"));
        latest_tag_from_web()
    })?;
    let is_newer = match (parse_ver(&tag), parse_ver(env!("CARGO_PKG_VERSION"))) {
        (Some(new), Some(cur)) => new > cur,
        _ => false,
    };
    Ok(ReleaseInfo { tag, is_newer })
}

/// Télécharge le zip de la release `tag` dans le dossier temporaire, retourne le chemin du zip.
fn download_update(tag: &str) -> Result<std::path::PathBuf, String> {
    let ver = tag.trim_start_matches('v');
    let url = format!("https://github.com/WaveRider83/scantoutpc/releases/download/{tag}/ScantoutPC-{ver}-portable.zip");
    let dest = std::env::temp_dir().join(format!("ScantoutPC-{ver}-portable.zip"));
    let resp = http_agent().get(&url).call().map_err(|e| format!("téléchargement : {e}"))?;
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&dest).map_err(|e| format!("création {} : {e}", dest.display()))?;
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("écriture : {e}"))?;
    Ok(dest)
}

/// Extrait le zip via PowerShell et lance un updater qui attend la fermeture de l'app,
/// copie les fichiers par-dessus, puis relance scantoutpc.exe.
fn spawn_updater(zip: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let install_dir = exe.parent().ok_or("dossier de l'exe introuvable")?.to_path_buf();
    let extract = std::env::temp_dir().join("scantoutpc-update");
    let zip_s = zip.to_string_lossy().replace('\'', "''");
    let extract_s = extract.to_string_lossy().replace('\'', "''");
    let install_s = install_dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop';          Remove-Item -Recurse -Force '{extract_s}' -ErrorAction SilentlyContinue;          Expand-Archive -LiteralPath '{zip_s}' -DestinationPath '{extract_s}' -Force;          $src = Join-Path '{extract_s}' 'ScantoutPC';          Wait-Process -Id {pid} -ErrorAction SilentlyContinue;          Copy-Item -Path (Join-Path $src '*') -Destination '{install_s}' -Recurse -Force;          Start-Process (Join-Path '{install_s}' 'scantoutpc.exe');          Remove-Item -Recurse -Force '{extract_s}' -ErrorAction SilentlyContinue;          Remove-Item -Force '{zip_s}' -ErrorAction SilentlyContinue",
        pid = std::process::id()
    );
    let script_path = std::env::temp_dir().join("scantoutpc-update.ps1");
    std::fs::write(&script_path, script).map_err(|e| format!("écriture updater : {e}"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
            .arg(&script_path)
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| format!("lancement de l'updater : {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = script_path;
        return Err("auto-update uniquement sur Windows".into());
    }
    Ok(())
}

fn pack_files(lang: &str) -> &'static [&'static str] {
    match lang {
        "zh" => &["rec_chinese.onnx", "rec_chinese.dict"],
        "ko" => &["rec_korean.onnx", "rec_korean.dict"],
        _ => &[],
    }
}

fn pack_installed(lang: &str) -> bool {
    let dir = ocr::models_dir();
    pack_files(lang).iter().all(|f| dir.join(f).exists())
}

/// Vérifie les dépendances indispensables au premier lancement.
/// Retourne la liste des éléments manquants (vide = tout est OK).
fn check_dependencies() -> Vec<String> {
    let mut missing = Vec::new();
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| std::path::PathBuf::from("."));
    let dll = if cfg!(windows) { "onnxruntime.dll" } else { "libonnxruntime.so" };
    if !exe_dir.join(dll).exists() && !exe_dir.join("models").join(dll).exists() {
        missing.push(format!("{dll} (à côté de l'exécutable)"));
    }
    let mdir = ocr::models_dir();
    for f in ["det.onnx", "rec_latin.onnx", "rec_latin.dict"] {
        if !mdir.join(f).exists() {
            missing.push(format!("models/{f}"));
        }
    }
    missing
}

fn download_pack(lang: &str) -> Result<String, String> {
    let base = pack_base_url();
    let dir = ocr::models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("models: {e}"))?;
    for f in pack_files(lang) {
        let url = format!("{base}/{f}");
        let tmp = dir.join(format!("{f}.part"));
        let dst = dir.join(f);
        if let Some(p) = url.strip_prefix("file:///") {
            std::fs::copy(p, &tmp).map_err(|e| format!("copie {f}: {e}"))?;
        } else {
            let resp = http_agent().get(&url).call().map_err(|e| format!("téléchargement {f}: {e}"))?;
            let mut reader = resp.into_reader();
            let mut out = std::fs::File::create(&tmp).map_err(|e| format!("{f}: {e}"))?;
            std::io::copy(&mut reader, &mut out).map_err(|e| format!("écriture {f}: {e}"))?;
        }
        std::fs::rename(&tmp, &dst).map_err(|e| format!("finalisation {f}: {e}"))?;
    }
    Ok(format!("pack {} installé", lang))
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
    decode_barcodes: bool,
    always_on_top: bool,
    layout_keep: bool,
    win_x: Option<i32>,
    win_y: Option<i32>,
    win_w: Option<f32>,
    win_h: Option<f32>,
    trad_enabled: bool,
    trad_langs: Vec<String>,
    trad_source: String,
    show_welcome: bool,
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
        decode_barcodes: true,
        always_on_top: true,
        layout_keep: false,
        win_x: None,
        win_y: None,
        win_w: None,
        win_h: None,
        trad_enabled: false,
        trad_langs: vec!["en".into(), "fr".into()],
        trad_source: "fr".into(),
        show_welcome: true,
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
                "decode_barcodes" => cfg.decode_barcodes = parse_bool(v),
                "always_on_top" => cfg.always_on_top = parse_bool(v),
                "layout_keep" => cfg.layout_keep = parse_bool(v),
                "win_x" => cfg.win_x = v.parse::<i32>().ok(),
                "win_y" => cfg.win_y = v.parse::<i32>().ok(),
                "win_w" => cfg.win_w = v.parse::<f32>().ok(),
                "win_h" => cfg.win_h = v.parse::<f32>().ok(),
                "ocr_lang" => { if matches!(v, "latin" | "zh" | "ko") { cfg.ocr_lang = v.to_string(); } }
                "trad_enabled" => cfg.trad_enabled = parse_bool(v),
                "trad_langs" => { cfg.trad_langs = v.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect(); }
                "trad_source" => { if translate::LANGS.iter().any(|(c, _)| *c == v.to_lowercase()) { cfg.trad_source = v.to_lowercase(); } }
                "show_welcome" => cfg.show_welcome = parse_bool(v),
                _ => {}
            }
        }
    }
    cfg
}

fn save_config(cfg: &AppConfig) {
    let body = format!(
        "[scantoutpc]\nhotkey={}\nstart_mini={}\nauto_copy={}\nkeep_history={}\nlight_mode={}\nauto_resize={}\nocr_lang={}\ndecode_barcodes={}\nalways_on_top={}\nwin_x={}\nwin_y={}\nwin_w={}\nwin_h={}\ntrad_enabled={}\ntrad_langs={}\ntrad_source={}\nshow_welcome={}\nlayout_keep={}\n",
        cfg.hotkey,
        cfg.start_mini as u8,
        cfg.auto_copy as u8,
        cfg.keep_history as u8,
        cfg.light_mode as u8,
        cfg.auto_resize as u8,
        cfg.ocr_lang,
        cfg.decode_barcodes as u8,
        cfg.always_on_top as u8,
        cfg.win_x.map(|v| v.to_string()).unwrap_or_default(),
        cfg.win_y.map(|v| v.to_string()).unwrap_or_default(),
        cfg.win_w.map(|v| format!("{v:.0}")).unwrap_or_default(),
        cfg.win_h.map(|v| format!("{v:.0}")).unwrap_or_default(),
        cfg.trad_enabled as u8,
        cfg.trad_langs.join(","),
        cfg.trad_source,
        cfg.show_welcome as u8,
        cfg.layout_keep as u8,
    );
    if let Err(e) = std::fs::write(config_path(), &body) {
        dbg_log(&format!("save_config FAILED: {e} (path: {})", config_path().display()));
    }
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
    if hk.mods.contains(Modifiers::META) { parts.push("Win".into()); }
    let dbg = format!("{:?}", hk.key);
    let key = dbg.strip_prefix("Key").map(String::from)
        .or_else(|| dbg.strip_prefix("Digit").map(String::from))
        .unwrap_or(dbg);
    parts.push(key);
    parts.join("+")
}


fn make_icon() -> Icon {
    // tray : icône 32px dédiée (évite le downscale grossier du 256 par Windows)
    let img = image::load_from_memory(include_bytes!("../assets/icon_32.png"))
        .expect("icon_32 embarquée illisible")
        .to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("icon")
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
    barcode: CheckMenuItem,
    top: CheckMenuItem,
    layout: CheckMenuItem,
    trad: CheckMenuItem,
    trad_langs: Vec<(String, CheckMenuItem)>,
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
    decode_barcodes: bool,
    always_on_top: bool,
    layout_keep: bool,
    top_init: bool,
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
    pack_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    pack_pending_lang: Option<String>,
    show_about: bool,
    win_pos: Option<[i32; 2]>,
    win_size: Option<[f32; 2]>,
    last_pos_save: Option<std::time::Instant>,
    mini_flash: bool,
    mini_icon: Option<egui::TextureHandle>,
    about_tex: Option<egui::TextureHandle>,
    update_rx: Option<std::sync::mpsc::Receiver<Result<ReleaseInfo, String>>>,
    update_info: Option<Result<ReleaseInfo, String>>,
    update_dl_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    update_dl_busy: bool,
    trad_enabled: bool,
    trad_langs: Vec<String>,
    trad_source: String,
    trad_rx: Option<std::sync::mpsc::Receiver<TradEvent>>,
    trad_busy: bool,
    trad_result: Option<(String, String)>,
    title_icon: Option<egui::TextureHandle>,
    show_welcome: bool,
    welcome_dont_show: bool,
    dep_missing: Vec<String>,
    capture_hidden_pos: Option<egui::Pos2>,
    prev_picking: bool,
}

impl ScantoutApp {
    fn set_ocr_lang(&mut self, key: &str) {
        self.ocr_lang = match key {
            "zh" => ocr::OcrLang::Chinese,
            "ko" => ocr::OcrLang::Korean,
            _ => ocr::OcrLang::Latin,
        };
        self.tray_opts.latin.set_checked(key == "latin");
        self.tray_opts.zh.set_checked(key == "zh");
        self.tray_opts.ko.set_checked(key == "ko");
        self.save_cfg();
    }

    /// Pipeline commun capture / import fichier : historique, image, OCR, presse-papier.
    /// Renvoie le texte composé (pour l'assemblage multi-pages).
    fn process_image(&mut self, ctx: &egui::Context, img: image::RgbaImage, ts: u64, show_texture: bool) -> String {
        if self.keep_history {
            if let Some(d) = dump_dir() {
                if img.save(d.join(format!("cap_{ts}.png"))).is_ok() {
                    history_append(&d, &format!("{ts} cap cap_{ts}.png {}x{}", img.width(), img.height()));
                    self.status = format!("historique: {}", d.display());
                }
            }
        }
        let size = [img.width() as usize, img.height() as usize];
        if show_texture {
            let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            self.capture = Some(ctx.load_texture("capture", color, egui::TextureOptions::LINEAR));
        }
        let reduced = self.mini || !self.main_visible || ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        if reduced && show_texture {
            self.enter_mini(ctx);
            self.mini_flash = true;
        }
        if self.auto_resize && show_texture {
            let pp = ctx.pixels_per_point();
            let w = (size[0] as f32 / pp + 28.0).clamp(400.0, 1100.0);
            let h = (size[1] as f32 / pp + 240.0).clamp(340.0, 950.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
            dbg_log(&format!("auto-resize {w}x{h}"));
        }
        let dynimg = image::DynamicImage::ImageRgba8(img);
        let barcode_hit = if self.decode_barcodes {
            barcode::decode(&dynimg)
        } else {
            None
        };
        if let Some((fmt, code)) = &barcode_hit {
            dbg_log(&format!("barcode detecte: {fmt} :: {}", &code.chars().take(80).collect::<String>()));
        }
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
        let mut result = String::new();
        if let Some(engine) = &self.ocr_engine {
            match engine.recognize(&dynimg, self.layout_keep) {
                Ok(text) => {
                    let text = text.trim().to_string();
                    let composed = match &barcode_hit {
                        Some((fmt, code)) => {
                            let line = format!("[{}] {}", fmt, code);
                            if text.is_empty() { line } else { format!("{}\n{}", line, text) }
                        }
                        None => text,
                    };
                    if self.keep_history {
                        if let Some(d) = dump_dir() {
                            let _ = std::fs::write(d.join(format!("ocr_{ts}.txt")), &composed);
                            let preview: String = composed.chars().take(80).collect::<String>().replace(['\n', '\r'], " ");
                            history_append(&d, &format!("{ts} ocr chars={} :: {}", composed.chars().count(), preview));
                            if let Some((fmt, code)) = &barcode_hit {
                                history_append(&d, &format!("{ts} barcode {fmt} :: {}", code.chars().take(120).collect::<String>()));
                            }
                        }
                    }
                    if composed.is_empty() {
                        self.status = "aucun texte détecté".into();
                    } else {
                        self.text = composed.clone();
                        self.trad_result = None;
                        if self.auto_copy {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(composed.clone());
                            }
                            self.status = match &barcode_hit {
                                Some((fmt, _)) => format!("{} détecté, copié ✓", fmt),
                                None => "texte copié ✓".into(),
                            };
                        } else {
                            self.status = match &barcode_hit {
                                Some((fmt, _)) => format!("{} détecté", fmt),
                                None => "texte reconnu".into(),
                            };
                        }
                        result = composed;
                    }
                }
                Err(e) => {
                    if let Some((fmt, code)) = &barcode_hit {
                        let composed = format!("[{}] {}", fmt, code);
                        self.text = composed.clone();
                        self.trad_result = None;
                        if self.auto_copy {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(composed.clone());
                            }
                        }
                        if self.keep_history {
                            if let Some(d) = dump_dir() {
                                let _ = std::fs::write(d.join(format!("ocr_{ts}.txt")), &composed);
                                history_append(&d, &format!("{ts} barcode {fmt} :: {}", code.chars().take(120).collect::<String>()));
                            }
                        }
                        self.status = format!("{} détecté (OCR a échoué)", fmt);
                        result = composed;
                    } else {
                        self.status = e;
                    }
                }
            }
        }
        result
    }

    fn do_capture(&mut self, ctx: &egui::Context) {
        dbg_log("do_capture start");
        match self.zone.clone() {
            Some(z) => match self.pending_capture.take().map(Ok).unwrap_or_else(|| capture_zone(&z)) {
                Ok(img) => {
                    dbg_log("capture_zone ok");
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    self.process_image(ctx, img, ts, true);
                }
                Err(e) => self.status = format!("capture échouée: {e}"),
            },
            None => self.status = "définis d'abord une zone".into(),
        }
        if self.mini {
            self.enter_mini(ctx);
        } // fenetre normale visible : on ne la reduit pas apres la capture
    }

    /// Démarre une sélection de zone (même logique que le raccourci clavier).
    fn trigger_capture(&mut self, ctx: &egui::Context) {
        let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
        let evis = ctx.input(|i| i.viewport().visible());
        dbg_log(&format!("trigger capture: mini={} main_visible={} minimized={:?} egui_visible={:?}", self.mini, self.main_visible, minimized, evis));
        let was_hidden = !self.main_visible || minimized;
        if self.mini {
            if was_hidden {
                self.enter_mini(ctx);
            } else {
                self.mini_since = Some(std::time::Instant::now());
            }
            self.begin_pick(ctx);
        } else if was_hidden {
            self.main_visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.begin_pick(ctx);
        } else {
            self.begin_pick(ctx);
        }
    }

    fn import_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images et PDF", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff", "ico", "pdf"])
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff", "ico"])
            .add_filter("PDF", &["pdf"])
            .pick_file() else { return };
        dbg_log(&format!("import fichier: {}", path.display()));
        match crate::importer::load_pages(&path) {
            Ok(pages) => {
                let base = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let total = pages.len();
                let mut texts = Vec::with_capacity(total);
                for (i, img) in pages.into_iter().enumerate() {
                    // timestamp distinct par page : chaque page devient une entrée d'historique
                    let t = self.process_image(ctx, img, base + i as u64, i == 0);
                    if !t.is_empty() {
                        if total > 1 {
                            texts.push(format!("--- page {} ---\n{}", i + 1, t));
                        } else {
                            texts.push(t);
                        }
                    }
                }
                if total > 1 {
                    let combined = texts.join("\n\n");
                    if !combined.is_empty() {
                        self.text = combined.clone();
                        if self.auto_copy {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(combined);
                            }
                        }
                    }
                    self.status = format!("{} page(s) importée(s)", total);
                }
            }
            Err(e) => self.status = format!("import échoué: {e}"),
        }
    }

    fn save_cfg(&self) {
        save_config(&AppConfig {
            hotkey: self.hotkey,
            start_mini: self.start_mini,
            auto_copy: self.auto_copy,
            keep_history: self.keep_history,
            light_mode: self.light_mode,
            auto_resize: self.auto_resize,
            decode_barcodes: self.decode_barcodes,
            always_on_top: self.always_on_top,
            layout_keep: self.layout_keep,
            ocr_lang: ocr_lang_key(&self.ocr_lang).to_string(),
            win_x: self.win_pos.map(|p| p[0]),
            win_y: self.win_pos.map(|p| p[1]),
            win_w: self.win_size.map(|s| s[0]),
            win_h: self.win_size.map(|s| s[1]),
            trad_enabled: self.trad_enabled,
            trad_langs: self.trad_langs.clone(),
            trad_source: self.trad_source.clone(),
            show_welcome: self.show_welcome,
        });
    }

    fn cancel_pick(&mut self) {
        self.picking = false;
        self.pick_at = None;
        self.overlay_shots = None;
        self.pending_capture = None;
        self.drag_start = None;
        self.drag_now = None;
        self.drag_screen = None;
    }

    fn can_hide(&self) -> bool {
        !self.picking && self.overlay_shots.is_none()
    }

    /// Garantit que la fenêtre est visible sur un écran : si sa position courante est hors
    /// limites (ou inconnue), on la ramène dans les bornes du moniteur.
    fn ensure_on_screen(&self, ctx: &egui::Context) {
        // Zone de travail du moniteur le plus proche (exclut la barre des tâches)
        let r = ctx.input(|i| i.viewport().outer_rect);
        let wa = work_area_near(
            r.map(|r| r.center().x).unwrap_or(0.0),
            r.map(|r| r.center().y).unwrap_or(0.0),
        );
        let mon = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(egui::vec2(1920.0, 1080.0));
        let (wa_x, wa_y, wa_w, wa_h) = wa.unwrap_or((0.0, 0.0, mon.x, mon.y));
        let r = match r {
            Some(r) => r,
            None => {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    wa_x + wa_w - MINI_W - 10.0,
                    wa_y + wa_h - MINI_H - 10.0,
                )));
                return;
            }
        };
        let w = r.width().min(wa_w);
        let h = r.height().min(wa_h);
        // Fenêtre totalement hors de la zone de travail (marge 64px anti faux-positifs DPI) ?
        // -> placement par défaut en bas à droite. Sinon, on ne touche pas à la position de l'utilisateur.
        let margin = 64.0;
        let disjoint = r.max.x <= wa_x + margin
            || r.min.x >= wa_x + wa_w - margin
            || r.max.y <= wa_y + margin
            || r.min.y >= wa_y + wa_h - margin;
        if !disjoint {
            return;
        }
        let (x, y) = (wa_x + wa_w - w - 10.0, wa_y + wa_h - h - 10.0);
        dbg_log(&format!("ensure_on_screen: ({:.0},{:.0}) -> ({},{}) (zone travail {:.0},{:.0} {}x{})", r.min.x, r.min.y, x, y, wa_x, wa_y, wa_w, wa_h));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
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
            if !self.can_hide() {
                dbg_log("tray toggle: masquage refuse (capture en cours)");
                return;
            }
            self.main_visible = false;
            self.show_about = false;
            self.save_cfg();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else {
            self.main_visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            if !self.mini {
                if let Some(p) = self.win_pos.filter(|p| p[0].abs() < 10000 && p[1].abs() < 10000) {
                    dbg_log(&format!("restore pos {},{}", p[0], p[1]));
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(p[0] as f32, p[1] as f32)));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.ensure_on_screen(ctx);
        }
    }

    /// Masque la fenêtre principale en la parquant hors écrans, puis lance la sélection de zone.
    /// (La fenêtre doit rester "visible" pour egui — les overlays sont ses enfants —
    /// donc on la déplace hors du champ de vision plutôt que de la cacher.)
    fn begin_pick(&mut self, ctx: &egui::Context) {
        // annule toute réduction mini en cours et garantit la fenêtre visible
        self.mini_collapsing = None;
        self.mini_since = Some(std::time::Instant::now());
        self.main_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(if self.mini { egui::vec2(MINI_W, MINI_H) } else { egui::vec2(self.win_size.map(|s| s[0]).unwrap_or(460.0), self.win_size.map(|s| s[1]).unwrap_or(360.0)) }));
        if self.capture_hidden_pos.is_none() {
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                self.capture_hidden_pos = Some(r.min);
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(-30000.0, -30000.0)));
        self.pick_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(250));
        dbg_log("begin_pick: window parked off-screen");
    }

    fn end_pick_restore(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.capture_hidden_pos.take() {
            // Ramène la position mémorisée dans la zone de travail (clamp déterministe,
            // sans lire le rect courant qui est périmé juste après le parking).
            let target = match work_area_near(p.x, p.y) {
                Some((wa_x, wa_y, wa_w, wa_h)) => {
                    let keep = 120.0f32; // garantir au moins 120px visibles
                    egui::pos2(
                        p.x.clamp(wa_x, (wa_x + wa_w - keep).max(wa_x)),
                        p.y.clamp(wa_y, (wa_y + wa_h - keep).max(wa_y)),
                    )
                }
                None => p,
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
        }
    }

    fn handle_menu_event(&mut self, id: &str, ctx: &egui::Context) {
            match id {
                "toggle" => self.toggle_main(ctx),
                "zone" => self.begin_pick(ctx),
                "quit" => { self.save_cfg(); std::process::exit(0); }
                "about" => {
                    self.exit_mini(ctx);
                    self.show_about = true;
                    if self.update_info.is_none() && self.update_rx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || { let _ = tx.send(check_latest_release()); });
                        self.update_rx = Some(rx);
                    }
                }
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
                "opt_barcode" => {
                    self.decode_barcodes = !self.decode_barcodes;
                    self.tray_opts.barcode.set_checked(self.decode_barcodes);
                    self.save_cfg();
                }
                "opt_trad" => {
                    self.trad_enabled = !self.trad_enabled;
                    self.tray_opts.trad.set_checked(self.trad_enabled);
                    self.status = if self.trad_enabled { "traduction activée (hors-ligne)".into() } else { "traduction désactivée".into() };
                    self.save_cfg();
                }
                "trad_dl" => {
                    if self.trad_rx.is_none() {
                        let langs = self.trad_langs.clone();
                        if langs.is_empty() {
                            self.status = "coche d'abord des langues (Options → Langues proposées)".into();
                        } else {
                            let (tx, rx) = std::sync::mpsc::channel();
                            std::thread::spawn(move || { let _ = tx.send(TradEvent::Finished(translate::install_models(&langs))); });
                            self.trad_rx = Some(rx);
                            self.trad_busy = true;
                            self.status = "téléchargement des modèles de traduction…".into();
                        }
                    }
                }
                "trad_pip" => {
                    if self.trad_rx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || { let _ = tx.send(TradEvent::Finished(translate::pip_install())); });
                        self.trad_rx = Some(rx);
                        self.trad_busy = true;
                        self.status = "pip install argostranslate…".into();
                    }
                }
                "opt_top" => {
                    self.always_on_top = !self.always_on_top;
                    self.tray_opts.top.set_checked(self.always_on_top);
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal }));
                    self.save_cfg();
                }
                "opt_layout" => {
                    self.layout_keep = !self.layout_keep;
                    self.tray_opts.layout.set_checked(self.layout_keep);
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
                ev if ev.starts_with("trad_lang_") => {
                    let code = ev.trim_start_matches("trad_lang_").to_string();
                    if let Some(item) = self.tray_opts.trad_langs.iter().find(|(c, _)| *c == code).map(|(_, i)| i.clone()) {
                        if self.trad_langs.contains(&code) {
                            self.trad_langs.retain(|c| c != &code);
                            item.set_checked(false);
                        } else {
                            self.trad_langs.push(code.clone());
                            item.set_checked(true);
                        }
                        self.save_cfg();
                    }
                }
                "lang_latin" | "lang_zh" | "lang_ko" => {
                    let key = match id { "lang_zh" => "zh", "lang_ko" => "ko", _ => "latin" };
                    if key != "latin" && !pack_installed(key) {
                        if self.pack_rx.is_none() {
                            let (tx, rx) = std::sync::mpsc::channel();
                            let k = key.to_string();
                            std::thread::spawn(move || { let _ = tx.send(download_pack(&k)); });
                            self.pack_rx = Some(rx);
                            self.pack_pending_lang = Some(key.to_string());
                            self.status = format!("téléchargement du pack {}…", key);
                        }
                    } else {
                        self.set_ocr_lang(key);
                    }
                }
                _ => {}
            }
    }

    fn request_close(&mut self, ctx: &egui::Context) {
        self.cancel_pick();
        if self.mini {
            self.mini_since = None;
            self.mini_collapsing = None;
            self.main_visible = false;
            self.show_about = false;
            self.save_cfg();
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else {
            self.enter_mini(ctx);
        }
    }

    fn menu_contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if ui.button("Définir la zone…").clicked() { self.handle_menu_event("zone", ctx); ui.close(); }
        ui.separator();
        if ui.checkbox(&mut self.auto_copy, "Copie auto dans le presse-papier").changed() { self.tray_opts.auto_copy.set_checked(self.auto_copy); self.save_cfg(); }
        if ui.checkbox(&mut self.keep_history, "Conserver l'historique des captures").changed() { self.tray_opts.keep_history.set_checked(self.keep_history); self.save_cfg(); }
        if ui.checkbox(&mut self.light_mode, "Thème clair").changed() { self.tray_opts.light.set_checked(self.light_mode); self.save_cfg(); }
        if ui.checkbox(&mut self.always_on_top, "Toujours au premier plan").changed() { self.tray_opts.top.set_checked(self.always_on_top); ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })); self.save_cfg(); }
        if ui.checkbox(&mut self.decode_barcodes, "Décoder les codes-barres").changed() { self.tray_opts.barcode.set_checked(self.decode_barcodes); self.save_cfg(); }
        if ui.checkbox(&mut self.layout_keep, "Conserver la mise en page").changed() { self.tray_opts.layout.set_checked(self.layout_keep); self.save_cfg(); }
        if ui.checkbox(&mut self.auto_resize, "Adapter la fenêtre à la capture").changed() { self.tray_opts.resize.set_checked(self.auto_resize); self.save_cfg(); }
        if ui.checkbox(&mut self.start_mini, "Réduire au lancement").changed() { self.tray_opts.start_mini.set_checked(self.start_mini); self.save_cfg(); }
        let mut autostart = self.tray_opts.autostart.is_checked();
        if ui.checkbox(&mut autostart, "Démarrer avec Windows").changed() { self.handle_menu_event("opt_autostart", ctx); }
        ui.separator();
        ui.menu_button("Langue OCR", |ui| {
            for (key, label) in [("latin", "FR / EN (latin)"), ("zh", "中文 (chinois)"), ("ko", "한국어 (coréen)")] {
                let selected = ocr_lang_key(&self.ocr_lang) == key;
                if ui.selectable_label(selected, label).clicked() {
                    let id = match key { "zh" => "lang_zh", "ko" => "lang_ko", _ => "lang_latin" };
                    self.handle_menu_event(id, ctx);
                    ui.close();
                }
            }
        });
        ui.menu_button("Traduction (hors-ligne)", |ui| {
            if ui.checkbox(&mut self.trad_enabled, "Traduction activée").changed() { self.tray_opts.trad.set_checked(self.trad_enabled); self.save_cfg(); }
            if ui.button("Télécharger les modèles…").clicked() { self.handle_menu_event("trad_dl", ctx); ui.close(); }
            if ui.button("Installer / MAJ le moteur (pip)…").clicked() { self.handle_menu_event("trad_pip", ctx); ui.close(); }
        });
        ui.separator();
        if ui.button("Effacer l'historique").clicked() { self.handle_menu_event("opt_clear", ctx); ui.close(); }
        ui.separator();
        if ui.button("À propos…").clicked() { self.handle_menu_event("about", ctx); ui.close(); }
        if ui.button("Quitter").clicked() { self.handle_menu_event("quit", ctx); }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = ui.visuals().dark_mode;
        let bar_bg = if dark { egui::Color32::from_rgb(25, 26, 32) } else { egui::Color32::from_rgb(238, 238, 242) };
        let line = if dark { egui::Color32::from_rgb(56, 57, 68) } else { egui::Color32::from_rgb(210, 210, 218) };
        let fg = ui.visuals().text_color();
        let muted = ui.visuals().weak_text_color();
        let bar_h = 40.0;
        let full_w = ui.available_width();
        let (rect, bar) = ui.allocate_exact_size(egui::vec2(full_w, bar_h), egui::Sense::click_and_drag());
        {
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, bar_bg);
            painter.line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(2.0, egui::Color32::from_rgb(245, 130, 55)));
        }
        if bar.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bar.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        // icône (chargée paresseusement)
        if self.title_icon.is_none() {
            if let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon_32.png")) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                self.title_icon = Some(ui.ctx().load_texture("title-icon", color, egui::TextureOptions::LINEAR));
            }
        }
        // zone gauche : icône + nom de l'app (= ouvrant le menu)
        if self.title_icon.is_some() {
            let r = egui::Rect::from_min_size(egui::pos2(rect.left() + 12.0, rect.center().y - 10.0), egui::vec2(20.0, 20.0));
            ui.painter().image(self.title_icon.as_ref().unwrap().id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        let left_w = (full_w - 3.0 * 46.0 - 190.0).clamp(120.0, 320.0);
        let mut left = ui.new_child(egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(egui::pos2(rect.left() + 40.0, rect.top()), egui::pos2(rect.left() + 40.0 + left_w, rect.bottom())))
            .layout(egui::Layout::left_to_right(egui::Align::Center)));
        left.menu_button(egui::RichText::new("ScantoutPC  ▾").strong().size(13.5).color(fg), |ui| self.menu_contents(ui, ctx));
        // zone droite : À propos + version (adaptative), puis boutons fenêtre
        let btn = 46.0;
        let show_about = full_w >= 460.0;
        let show_version = full_w >= 380.0;
        let mut right_w = 0.0;
        if show_about { right_w += 96.0; }
        if show_version { right_w += 52.0; }
        if right_w > 0.0 {
            let mut right = ui.new_child(egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(egui::pos2(rect.right() - 2.0 * btn - right_w, rect.top()), egui::pos2(rect.right() - 2.0 * btn, rect.bottom())))
                .layout(egui::Layout::right_to_left(egui::Align::Center)));
            if show_version {
                right.label(egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).size(10.0).color(muted));
                if show_about { right.add_space(8.0); }
            }
            if show_about {
                if right.add(egui::Button::new(egui::RichText::new("À propos").size(11.0).color(fg)).fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::new(1.0, line)).corner_radius(4.0)).clicked() {
                    self.handle_menu_event("about", ctx);
                }
            }
        }
        // max / close (positionnés depuis le bord droit) — pas de bouton réduire : la fenêtre
        // réduite ne se restaurait pas via la barre des tâches ; la croix fait office de réduction (mini).
        let close_r = egui::Rect::from_min_size(egui::pos2(rect.right() - btn, rect.top()), egui::vec2(btn, bar_h));
        let max_r = close_r.translate(egui::vec2(-btn, 0.0));
        let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
        let max_resp = ui.interact(max_r, ui.scope_id().with("tb-max"), egui::Sense::click());
        if max_resp.hovered() { ui.painter().rect_filled(max_r, 0.0, ui.visuals().widgets.hovered.bg_fill); }
        {
            let c = max_r.center();
            let s = egui::Stroke::new(1.4, fg);
            if maximized {
                ui.painter().rect_stroke(egui::Rect::from_min_max(egui::pos2(c.x - 3.0, c.y - 6.0), egui::pos2(c.x + 6.0, c.y + 3.0)), 0.0, s, egui::StrokeKind::Outside);
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(c.x - 6.0, c.y - 3.0), egui::pos2(c.x + 3.0, c.y + 6.0)), 0.0, bar_bg);
                ui.painter().rect_stroke(egui::Rect::from_min_max(egui::pos2(c.x - 6.0, c.y - 3.0), egui::pos2(c.x + 3.0, c.y + 6.0)), 0.0, s, egui::StrokeKind::Outside);
            } else {
                ui.painter().rect_stroke(egui::Rect::from_min_max(egui::pos2(c.x - 5.0, c.y - 5.0), egui::pos2(c.x + 5.0, c.y + 5.0)), 0.0, s, egui::StrokeKind::Outside);
            }
        }
        if max_resp.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)); }
        let close_resp = ui.interact(close_r, ui.scope_id().with("tb-close"), egui::Sense::click());
        let close_hover = close_resp.hovered();
        if close_hover { ui.painter().rect_filled(close_r, 0.0, egui::Color32::from_rgb(232, 17, 35)); }
        {
            let c = close_r.center();
            let col = if close_hover { egui::Color32::WHITE } else { fg };
            let s = egui::Stroke::new(1.6, col);
            let d = 5.0;
            ui.painter().line_segment([egui::pos2(c.x - d, c.y - d), egui::pos2(c.x + d, c.y + d)], s);
            ui.painter().line_segment([egui::pos2(c.x - d, c.y + d), egui::pos2(c.x + d, c.y - d)], s);
        }
        if close_resp.clicked() { self.request_close(ctx); }
    }

    fn resize_handles(ui: &mut egui::Ui, ctx: &egui::Context) {
        use egui::ResizeDirection as Dir;
        // contour fin de la fenêtre
        let screen = ui.ctx().content_rect();
        let dark = ui.visuals().dark_mode;
        ui.painter().rect_stroke(screen.shrink(0.5), 0.0, egui::Stroke::new(1.0, if dark { egui::Color32::from_rgb(70, 71, 82) } else { egui::Color32::from_rgb(200, 200, 208) }), egui::StrokeKind::Inside);
        if ctx.input(|i| i.viewport().maximized).unwrap_or(false) { return; }
        let s = screen;
        let t = 5.0_f32;
        let c = 14.0_f32;
        let zones: [(egui::Rect, Dir, egui::CursorIcon); 8] = [
            (egui::Rect::from_min_max(s.left_top(), egui::pos2(s.right(), s.top() + t)), Dir::North, egui::CursorIcon::ResizeVertical),
            (egui::Rect::from_min_max(egui::pos2(s.left(), s.bottom() - t), s.right_bottom()), Dir::South, egui::CursorIcon::ResizeVertical),
            (egui::Rect::from_min_max(egui::pos2(s.right() - t, s.top()), s.right_bottom()), Dir::East, egui::CursorIcon::ResizeHorizontal),
            (egui::Rect::from_min_max(egui::pos2(s.left(), s.top()), egui::pos2(s.left() + t, s.bottom())), Dir::West, egui::CursorIcon::ResizeHorizontal),
            (egui::Rect::from_min_size(s.left_top(), egui::vec2(c, c)), Dir::NorthWest, egui::CursorIcon::ResizeNorthWest),
            (egui::Rect::from_min_size(egui::pos2(s.right() - c, s.top()), egui::vec2(c, c)), Dir::NorthEast, egui::CursorIcon::ResizeNorthEast),
            (egui::Rect::from_min_size(egui::pos2(s.left(), s.bottom() - c), egui::vec2(c, c)), Dir::SouthWest, egui::CursorIcon::ResizeSouthWest),
            (egui::Rect::from_min_size(egui::pos2(s.right() - c, s.bottom() - c), egui::vec2(c, c)), Dir::SouthEast, egui::CursorIcon::ResizeSouthEast),
        ];
        for (i, (r, dir, cur)) in zones.iter().enumerate() {
            let resp = ui.interact(*r, ui.scope_id().with(("win-resize", i)), egui::Sense::drag());
            if resp.hovered() { ctx.set_cursor_icon(*cur); }
            if resp.drag_started() { ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(*dir)); }
        }
        }
}

impl eframe::App for ScantoutApp {
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.window_fill().to_normalized_gamma_f32()
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.mini && self.main_visible && self.mini_collapsing.is_none() && self.capture_hidden_pos.is_none() && !self.picking {
            if let Some(r) = ctx.input(|i| i.viewport().outer_rect) {
                if r.width() < 100.0 || r.height() < 80.0 {
                    // fenêtre en cours de réduction : ne pas mémoriser
                } else {
                let p = [r.min.x.round() as i32, r.min.y.round() as i32];
                let s = [r.width(), r.height()];
                if self.win_size != Some(s) {
                    self.win_size = Some(s);
                    self.last_pos_save = None; // force un save rapide aussi pour la taille
                }
                if self.win_pos != Some(p) {
                    self.win_pos = Some(p);
                    let due = self.last_pos_save.map(|t: std::time::Instant| t.elapsed() > std::time::Duration::from_secs(1)).unwrap_or(true);
                    if due {
                        self.last_pos_save = Some(std::time::Instant::now());
                        self.save_cfg();
                    }
                }
                }
            }
        }
        // packs de langue téléchargés en tâche de fond
        if let Some(rx) = &self.update_rx {
            match rx.try_recv() {
                Ok(res) => { self.update_info = Some(res); self.update_rx = None; }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.update_rx = None; }
            }
        }
        if let Some(rx) = &self.trad_rx {
            match rx.try_recv() {
                Ok(TradEvent::Translated(target, res)) => {
                    self.trad_busy = false;
                    match res {
                        Ok(t) => { self.trad_result = Some((target, t)); self.status = "traduction terminée".into(); }
                        Err(e) => { self.status = format!("traduction : {e}"); }
                    }
                    self.trad_rx = None;
                }
                Ok(TradEvent::Finished(res)) => {
                    self.trad_busy = false;
                    self.status = match res {
                        Ok(msg) => msg,
                        Err(e) => format!("échec : {e}"),
                    };
                    self.trad_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.trad_rx = None; self.trad_busy = false; }
            }
        }
        if let Some(rx) = &self.update_dl_rx {
            match rx.try_recv() {
                Ok(Ok(zip)) => {
                    match spawn_updater(&zip) {
                        Ok(()) => {
                            self.status = "mise à jour en cours : fermeture puis relance…".into();
                            std::process::exit(0);
                        }
                        Err(e) => { self.status = format!("updater : {e}"); self.update_dl_busy = false; self.update_dl_rx = None; }
                    }
                }
                Ok(Err(e)) => { self.status = format!("téléchargement : {e}"); self.update_dl_busy = false; self.update_dl_rx = None; }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.update_dl_busy = false; self.update_dl_rx = None; }
            }
        }
        if let Some(rx) = &self.pack_rx {
            match rx.try_recv() {
                Ok(Ok(msg)) => {
                    if let Some(k) = self.pack_pending_lang.take() { self.set_ocr_lang(&k); }
                    self.status = msg;
                    self.pack_rx = None;
                }
                Ok(Err(e)) => {
                    self.status = format!("pack échoué : {e}");
                    self.pack_rx = None;
                    self.pack_pending_lang = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.pack_rx = None; }
            }
        }
        let mut visuals = if self.light_mode { egui::Visuals::light() } else { egui::Visuals::dark() };
        if self.mini {
            visuals.window_fill = egui::Color32::TRANSPARENT;
        }
        ctx.set_visuals(visuals);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if self.top_init {
            self.top_init = false;
            if !self.always_on_top {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            }
        }
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
                    if !self.can_hide() {
                        self.mini_since = Some(std::time::Instant::now()); // report tant que la capture est active
                    } else {
                        self.mini_collapsing = Some(std::time::Instant::now());
                    dbg_log("mini collapse start");
                    self.picking = false;
                    self.overlay_shots = None;
                    self.pending_capture = None;
                    }
                }
            }
        }
        if self.mini_collapsing.is_some() && self.picking {
            self.mini_collapsing = None;
            self.mini_since = Some(std::time::Instant::now());
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, MINI_H)));
        }
        if let Some(t0) = self.mini_collapsing {
            let p = (t0.elapsed().as_secs_f32() / 0.25).min(1.0);
            let h = (MINI_H * (1.0 - p)).max(1.0);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, h)));
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            if p >= 1.0 && !self.can_hide() {
                // un pick a démarré entre-temps : abandonner la réduction
                self.mini_collapsing = None;
                self.mini_since = Some(std::time::Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, MINI_H)));
            } else if p >= 1.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(MINI_W, MINI_H)));
                self.mini_collapsing = None;
                self.mini_since = None;
                self.main_visible = false;
                self.show_about = false;
                self.save_cfg();
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
        if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if let tray_icon::TrayIconEvent::DoubleClick { button: tray_icon::MouseButton::Left, .. } = event {
                if self.mini {
                    self.exit_mini(ctx);
                } else {
                    self.main_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.ensure_on_screen(ctx);
                }
            }
        }
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id().0.clone();
            self.handle_menu_event(id.as_str(), ctx);
        }
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::HotKeyState::Pressed {
                if self.recording { return; }
                if self.picking { dbg_log("hotkey ignore: picking en cours"); return; }
                dbg_log("hotkey pressed");
                self.trigger_capture(ctx);
            }
        }
    }


    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.prev_picking && !self.picking {
            self.end_pick_restore(&ctx);
        }
        if self.capture_hidden_pos.is_some() && self.pick_at.is_none() && !self.picking {
            self.end_pick_restore(&ctx); // filet de sécurité : ne jamais laisser la fenêtre hors écran
        }
        self.prev_picking = self.picking;
        if ui.input(|i| i.viewport().close_requested()) {
            dbg_log(&format!("close requested: mini={} main_visible={}", self.mini, self.main_visible));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_close(&ctx.clone());
        }
        if !self.mini {
            self.title_bar(ui, &ctx);
            Self::resize_handles(ui, &ctx);
        }
        if self.show_about {
            if self.about_tex.is_none() {
                let img = image::load_from_memory(include_bytes!("../assets/about_art.png")).expect("about_art.png").to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                self.about_tex = Some(ui.ctx().load_texture("about-art", color, egui::TextureOptions::LINEAR));
            }
            let mut open = true;
            let mut about_rect: Option<egui::Rect> = None;
            egui::Window::new("À propos de ScantoutPC")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(600.0, 400.0))
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::new().inner_margin(egui::Margin::same(18)).fill(egui::Color32::from_rgb(12, 12, 14)))
                .show(ui.ctx(), |ui| {
                    about_rect = Some(ui.max_rect().expand2(egui::vec2(18.0, 40.0)));
                    if let Some(tex) = &self.about_tex {
                        let rect = ui.max_rect();
                        ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_black_alpha(90));
                    }
                    let fg = egui::Color32::from_rgb(235, 235, 235);
                    ui.heading(egui::RichText::new("ScantoutPC").color(fg).size(30.0));
                    ui.label(egui::RichText::new(format!("Version active : v{}", env!("CARGO_PKG_VERSION"))).color(fg));
                    match &self.update_info {
                        Some(Ok(info)) => {
                            ui.label(egui::RichText::new(format!("Dernière version : v{}", info.tag.trim_start_matches('v'))).color(fg));
                        }
                        Some(Err(_)) => {
                            ui.label(egui::RichText::new("Dernière version : indisponible (hors ligne ?)").weak());
                        }
                        None => {
                            ui.label(egui::RichText::new("Dernière version : vérification…").weak());
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Capture et OCR de zones d'écran, 100 % local.").color(fg));
                    ui.label(egui::RichText::new("© 2026 Cédric Burnotte").color(fg));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Licence : MIT (voir LICENSE).").color(fg));
                    ui.add(egui::Label::new(egui::RichText::new("OCR : PP-OCR via oar-ocr / ONNX Runtime\nCapture : GDI / Windows.Graphics.Capture").color(fg)).wrap());
                    ui.hyperlink_to(egui::RichText::new("Code source : github.com/WaveRider83/scantoutpc").color(egui::Color32::from_rgb(255, 160, 80)), "https://github.com/WaveRider83/scantoutpc");
                    ui.add_space(4.0);
                    match &self.update_info {
                        None => {}
                        Some(Ok(info)) if !info.is_newer => { ui.label(egui::RichText::new("✓ Vous utilisez la dernière version.").color(egui::Color32::from_rgb(110, 200, 120))); }
                        Some(Ok(info)) => {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("⬆ Mise à jour disponible : {}", info.tag)).strong().color(egui::Color32::from_rgb(240, 180, 80)));
                                if self.update_dl_busy {
                                    ui.add(egui::Spinner::new().size(16.0));
                                    ui.label(egui::RichText::new("téléchargement…").weak());
                                } else if ui.button("Installer maintenant").clicked() {
                                    let tag = info.tag.clone();
                                    let tag_thread = tag.clone();
                                    let (tx, rx) = std::sync::mpsc::channel();
                                    std::thread::spawn(move || { let _ = tx.send(download_update(&tag_thread)); });
                                    self.update_dl_rx = Some(rx);
                                    self.update_dl_busy = true;
                                    self.status = format!("téléchargement de {tag}…");
                                }
                                ui.hyperlink_to("releases", "https://github.com/WaveRider83/scantoutpc/releases/latest");
                            });
                        }
                        Some(Err(_)) => {}
                    }
                });
            // clic en dehors de la fenêtre À propos -> fermer
            let outside_click = ui.ctx().input(|i| {
                i.pointer.primary_pressed() && i.pointer.interact_pos().map(|p| {
                    about_rect.map(|r| !r.contains(p)).unwrap_or(false)
                }).unwrap_or(false)
            });
            if outside_click {
                open = false;
            }
            if !open {
                self.show_about = false;
            }
        }
        if self.show_welcome {
            let mut dismissed = false;
            egui::Window::new("Bienvenue dans ScantoutPC")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size(egui::vec2(460.0, 300.0))
                .show(ui.ctx(), |ui| {
                    ui.heading("Capture et OCR hors-ligne");
                    ui.add_space(6.0);
                    ui.label("1. Appuie sur Ctrl+Shift+O (modifiable) ou choisis « Définir la zone » dans le menu ☰ / l'icône zone.");
                    ui.label("2. Glisse pour sélectionner une zone de l'écran : le texte est reconnu et copié dans le presse-papier.");
                    ui.label("3. Retrouve tes captures dans l'Historique, en bas de la fenêtre.");
                    ui.add_space(4.0);
                    ui.label("Tout est local : aucune image n'est envoyée sur Internet.");
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.welcome_dont_show, "Ne plus afficher ce message");
                    ui.add_space(4.0);
                    if ui.button("Compris !").clicked() { dismissed = true; }
                });
            if dismissed {
                self.show_welcome = false;
                if self.welcome_dont_show { self.save_cfg(); }
            }
        }
        if !self.dep_missing.is_empty() {
            egui::Window::new("Dépendances manquantes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size(egui::vec2(480.0, 240.0))
                .show(ui.ctx(), |ui| {
                    ui.label("ScantoutPC ne peut pas fonctionner correctement : il manque");
                    ui.add_space(2.0);
                    for m in &self.dep_missing {
                        ui.label(egui::RichText::new(format!("• {m}")).color(egui::Color32::from_rgb(240, 160, 80)));
                    }
                    ui.add_space(6.0);
                    ui.label("La version portable officielle contient tout le nécessaire :");
                    ui.hyperlink_to(egui::RichText::new("Télécharger la dernière version").color(egui::Color32::from_rgb(255, 160, 80)), "https://github.com/WaveRider83/scantoutpc/releases/latest");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Vérifier à nouveau").clicked() {
                            self.dep_missing = check_dependencies();
                            if self.dep_missing.is_empty() { self.status = "dépendances OK ✓".into(); }
                        }
                        if ui.button("Ignorer").clicked() { self.dep_missing.clear(); }
                    });
                });
        }
        if self.picking && self.overlay_shots.is_none() {
            self.pending_capture = None;
            dbg_log("spawn overlays start");
            let mut shots = Vec::new();
            if let Ok(monitors) = xcap::Monitor::all() {
                for (i, m) in monitors.iter().enumerate() {
                    let t0 = std::time::Instant::now();
                    let mw = m.width().unwrap_or(0) as i32;
                    let mh = m.height().unwrap_or(0) as i32;
                    let img = gdi::gdi_capture(m.x().unwrap_or(0), m.y().unwrap_or(0), mw, mh).ok();
                    dbg_log(&format!("overlay moniteur[{}]: path=gdi ok={} en {:?}", i, img.is_some(), t0.elapsed()));
                    if let Some(img) = img {
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
            if self.mini_icon.is_none() {
                let (rgba, w, h) = icon_rgba();
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                self.mini_icon = Some(ui.ctx().load_texture("mini-icon", img, egui::TextureOptions::LINEAR));
            }
            let mut clicked = false;
            let full = ui.max_rect();
            ui.painter().rect_filled(full, 0.0, egui::Color32::from_rgb(24, 26, 33));
            ui.painter().rect_stroke(full, 0.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(72, 76, 90)), egui::StrokeKind::Inside);
            egui::Frame::new()
                .inner_margin(6.0)
                .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if self.mini_flash {
                        if let Some(tex) = &self.capture {
                            let img = egui::Image::from_texture(tex)
                                .max_width(MINI_W - 18.0)
                                .max_height(MINI_H - 14.0)
                                .sense(egui::Sense::click());
                            if ui.add(img).on_hover_text("Cliquer pour agrandir").clicked() {
                                clicked = true;
                            }
                        }
                    } else if let Some(icon) = &self.mini_icon {
                        let img = egui::Image::from_texture(icon)
                            .fit_to_exact_size(egui::vec2(56.0, 56.0))
                            .sense(egui::Sense::click());
                        if ui.add(img).on_hover_text("ScantoutPC — cliquer pour ouvrir").clicked() {
                            clicked = true;
                        }
                        ui.label(egui::RichText::new("ScantoutPC").small().color(egui::Color32::from_rgb(200, 204, 214)));
                    }
                });
            });
            if clicked {
                self.exit_mini(ui.ctx());
            }
        } else {
        let mut trad_request: Option<String> = None;
        let trad_source_before = self.trad_source.clone();
        ui.horizontal(|ui| {
            if self.trad_enabled {
                ui.vertical(|ui| {
                    egui::ComboBox::from_id_salt("trad_src")
                        .width(52.0)
                        .selected_text(self.trad_source.to_uppercase())
                        .show_ui(ui, |ui| {
                            for (code, name) in translate::LANGS {
                                ui.selectable_value(&mut self.trad_source, code.to_string(), format!("{code} — {name}"));
                            }
                        });
                    ui.add_space(2.0);
                    let langs = self.trad_langs.clone();
                    for code in &langs {
                        if flag_button(ui, code).clicked() {
                            trad_request = Some(code.clone());
                        }
                        ui.add_space(2.0);
                    }
                    if self.trad_busy {
                        ui.add(egui::Spinner::new().size(18.0));
                    }
                });
            }
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let mut edit = egui::TextEdit::multiline(&mut self.text)
                        .desired_width(ui.available_width())
                        .desired_rows(10)
                        .hint_text("Le texte capturé apparaîtra ici…");
                    if self.layout_keep {
                        edit = edit.font(egui::TextStyle::Monospace);
                    }
                    ui.add(edit);
                });
            });
        });
        if self.trad_source != trad_source_before {
            self.save_cfg();
        }
        if let Some(target) = trad_request {
            let text = self.text.trim().to_string();
            if text.is_empty() {
                self.status = "rien à traduire".into();
            } else if target == self.trad_source {
                self.status = "langue source = langue cible".into();
            } else if self.trad_rx.is_none() {
                let (tx, rx) = std::sync::mpsc::channel();
                let from = self.trad_source.clone();
                std::thread::spawn(move || {
                    let res = translate::translate(&text, &from, &target);
                    let _ = tx.send(TradEvent::Translated(target, res));
                });
                self.trad_rx = Some(rx);
                self.trad_busy = true;
                self.status = "traduction…".into();
            }
        }
        if let Some((target, translated)) = self.trad_result.clone() {
            ui.add_space(4.0);
            let mut close = false;
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 90, 110)))
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("→ {}", translate::lang_name(&target))).strong());
                        if ui.small_button("Copier").clicked() {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(translated.clone());
                            }
                            self.status = "traduction copiée ✓".into();
                        }
                        if ui.small_button("✕").clicked() {
                            close = true;
                        }
                    });
                    ui.add(egui::Label::new(egui::RichText::new(translated)).selectable(true));
                });
            if close {
                self.trad_result = None;
            }
        }
        if let Some(tex) = &self.capture {
            let max_w = ui.available_width();
            let scale = (max_w / tex.size_vec2().x).min(120.0 / tex.size_vec2().y).min(1.0);
            ui.add(egui::Image::from_texture(tex).max_width(tex.size_vec2().x * scale));
        }
        let hist = scan_history();
        egui::CollapsingHeader::new(egui::RichText::new(format!("🕘  Historique — {} capture{}", hist.len(), if hist.len() > 1 { "s" } else { "" })).strong())
            .id_salt("historique")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Captures précédentes").small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !hist.is_empty() && ui.small_button("🗑 Tout effacer").clicked() {
                            clear_history_files();
                            self.hist_tex.clear();
                            self.status = "historique effacé".into();
                        }
                    });
                });
                ui.add_space(2.0);
                let hist_h = ui.available_height().max(140.0);
                egui::ScrollArea::vertical().max_height(hist_h).show(ui, |ui| {
                    if hist.is_empty() {
                        ui.add_space(12.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("Aucune capture pour l'instant").weak());
                            ui.label(egui::RichText::new("Définis une zone puis capture : la vignette apparaîtra ici.").small().weak());
                        });
                        ui.add_space(12.0);
                    }
                    for (ts, png, txt) in &hist {
                        let card_fill = if ui.visuals().dark_mode { egui::Color32::from_rgb(34, 36, 46) } else { egui::Color32::from_rgb(244, 244, 248) };
                        egui::Frame::new().fill(card_fill).corner_radius(8.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let key = ts.to_string();
                            if !self.hist_tex.contains_key(&key) {
                                if let Ok(bytes) = std::fs::read(png) {
                                    if let Ok(img) = image::load_from_memory(&bytes) {
                                        let t = img.thumbnail(168, 108).to_rgba8();
                                        let (w, h) = t.dimensions();
                                        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], t.as_raw());
                                        let tex = ui.ctx().load_texture(format!("hist-{ts}"), color, egui::TextureOptions::LINEAR);
                                        self.hist_tex.insert(key.clone(), tex);
                                    }
                                }
                            }
                            if let Some(tex) = self.hist_tex.get(&key) {
                                egui::Frame::new().corner_radius(6.0).stroke(egui::Stroke::new(1.0, egui::Color32::from_black_alpha(60))).show(ui, |ui| {
                                    ui.add(egui::Image::from_texture(tex).fit_to_exact_size(egui::vec2(84.0, 54.0)));
                                });
                            }
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(fmt_ts_local(*ts)).small().strong());
                                let preview: String = txt.as_ref()
                                    .and_then(|p| std::fs::read_to_string(p).ok())
                                    .map(|t| t.split('\n').take(2).collect::<Vec<_>>().join(" "))
                                    .unwrap_or_default();
                                let trunc: String = preview.chars().take(90).collect();
                                ui.label(egui::RichText::new(if trunc.is_empty() { "(pas de texte)".to_string() } else { trunc }).small().weak());
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    if ui.small_button("Charger").clicked() {
                                        let img_ok = std::fs::read(png).ok()
                                            .and_then(|b| image::load_from_memory(&b).ok())
                                            .map(|img| {
                                                let rgba = img.to_rgba8();
                                                let size = [rgba.width() as usize, rgba.height() as usize];
                                                let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                                                self.capture = Some(ui.ctx().load_texture("capture", color, egui::TextureOptions::LINEAR));
                                            })
                                            .is_some();
                                        self.text = txt.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
                                        self.trad_result = None;
                                        self.status = if img_ok { "capture chargée depuis l'historique".into() } else { "image illisible (fichier déplacé ou supprimé ?)".into() };
                                    }
                                    if ui.small_button("Copier").clicked() {
                                        if let Some(p) = txt {
                                            if let Ok(t) = std::fs::read_to_string(p) {
                                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                                    let _ = cb.set_text(t);
                                                }
                                                self.status = "copié ✓".into();
                                            }
                                        }
                                    }
                                    if ui.small_button("🗑").on_hover_text("Supprimer cette capture").clicked() {
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
                        ui.add_space(6.0);
                    }
                });
            });
        ui.horizontal(|ui| {
            let label = if self.recording {
                egui::RichText::new("… appuie sur la combinaison (Echap = annuler)").strong()
            } else {
                egui::RichText::new(format!("⌨ {}", self.hotkey_label)).strong()
            };
            if ui.button("Capture").on_hover_text("Capturer une zone (comme le raccourci)").clicked() {
                if !self.recording && !self.picking {
                    self.trigger_capture(ui.ctx());
                }
            }
            if ui.button("Charger…").on_hover_text("Charger une image ou un PDF et l'analyser comme une capture").clicked() {
                self.import_file(ui.ctx());
            }
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
