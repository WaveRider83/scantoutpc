// ScantoutPC — OCR live pour Windows (fenêtre flottante + overlay de sélection)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use tray_icon::{TrayIconBuilder, Icon};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
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
    let tray_menu = Menu::new();
    let item_toggle = MenuItem::with_id("toggle", "Afficher / Masquer", true, None);
    let item_zone = MenuItem::with_id("zone", "Definir la zone...", true, None);
    let item_quit = MenuItem::with_id("quit", "Quitter", true, None);
    let _ = tray_menu.append_items(&[&item_toggle, &item_zone, &item_quit]);
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("ScantoutPC")
        .with_icon(make_icon())
        .build()
        .expect("tray icon");

    let hotkey_manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    hotkey_manager.register(hotkey).expect("register Ctrl+Shift+O");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ScantoutPC")
            .with_inner_size([460.0, 360.0])
            .with_min_inner_size([360.0, 280.0])
            .with_decorations(false)
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
                auto_capture_at: None,
                ocr_lang: ocr::OcrLang::Latin,
                ocr_engine: None,
                _hotkey_manager: hotkey_manager,
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


// DEBUG TEMP : dossiers candidats pour les dumps (a retirer)
fn capture_dump_dir() -> Vec<std::path::PathBuf> {
    let mut cand: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("SCANTOUT_CAPTURES") { cand.push(std::path::PathBuf::from(p)); }
    cand.push(std::path::PathBuf::from(r"\\local-share\<local>\scantoutpc\captures"));
    cand.push(std::path::PathBuf::from(r"\\local-share\scantoutpc\captures"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() { cand.push(d.join("captures")); }
    }
    cand.into_iter().filter(|d| d.exists() || std::fs::create_dir_all(d).is_ok()).collect()
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
    auto_capture_at: Option<std::time::Instant>,
    ocr_lang: ocr::OcrLang,
    ocr_engine: Option<ocr::OcrEngine>,
    _hotkey_manager: GlobalHotKeyManager,
}

impl ScantoutApp {
    fn do_capture(&mut self, ctx: &egui::Context) {
        match self.zone.clone() {
            Some(z) => match capture_zone(&z) {
                Ok(img) => {
                    let dump = std::env::temp_dir().join("scantoutpc_capture.png");
                    let _ = img.save(&dump);
                    // DEBUG TEMP : dump de la capture pour analyse (a retirer)
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let mut cand: Vec<std::path::PathBuf> = Vec::new();
                    if let Ok(p) = std::env::var("SCANTOUT_CAPTURES") { cand.push(std::path::PathBuf::from(p)); }
                    cand.push(std::path::PathBuf::from(r"\\local-share\<local>\scantoutpc\captures"));
                    cand.push(std::path::PathBuf::from(r"\\local-share\scantoutpc\captures"));
                    if let Ok(exe) = std::env::current_exe() {
                        if let Some(d) = exe.parent() { cand.push(d.join("captures")); }
                    }
                    let mut dumped = false;
                    for d in cand {
                        if std::fs::create_dir_all(&d).is_ok() {
                            if img.save(d.join(format!("cap_{ts}.png"))).is_ok() {
                                self.status = format!("{} (dump: {})", self.status, d.display());
                                dumped = true;
                                break;
                            }
                        }
                    }
                    if !dumped { self.status = format!("{} (dump: echec)", self.status); }
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    self.capture = Some(ctx.load_texture("capture", color, egui::TextureOptions::LINEAR));
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
                                for d in capture_dump_dir() {
                                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                                    let _ = std::fs::write(d.join(format!("ocr_{ts}.txt")), &text);
                                }
                                let text = text.trim().to_string();
                                if text.is_empty() {
                                    self.status = "aucun texte détecté".into();
                                } else {
                                    self.text = text.clone();
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        let _ = cb.set_text(text);
                                    }
                                    self.status = "texte copié ✓".into();
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
    }

    fn toggle_main(&mut self, ctx: &egui::Context) {
        self.main_visible = !self.main_visible;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.main_visible));
    }
}

impl eframe::App for ScantoutApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id().0.as_str() {
                "toggle" => self.toggle_main(ctx),
                "zone" => self.picking = true,
                "quit" => std::process::exit(0),
                _ => {}
            }
        }
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::HotKeyState::Pressed {
                self.picking = true;
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(t) = self.auto_capture_at {
            if std::time::Instant::now() >= t {
                self.auto_capture_at = None;
                self.do_capture(&ctx);
            }
        }
        if self.picking && self.overlay_shots.is_none() {
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
        let title = ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("● ScantoutPC").color(egui::Color32::from_rgb(96, 165, 250)).strong());
        }).response;
        let title_rect = title.rect.expand2(egui::vec2(8.0, 4.0));
        let drag = ui.interact(title_rect, ui.scope_id().with("drag-title"), egui::Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut self.text)
                .desired_width(f32::INFINITY)
                .desired_rows(8)
                .hint_text("Le texte capturé apparaîtra ici…"));
        });
        if let Some(tex) = &self.capture {
            let max_w = ui.available_width();
            let scale = (max_w / tex.size_vec2().x).min(120.0 / tex.size_vec2().y).min(1.0);
            ui.add(egui::Image::from_texture(tex).max_width(tex.size_vec2().x * scale));
        }
        ui.horizontal(|ui| {
            ui.label("Langue OCR :");
            egui::ComboBox::from_id_salt("ocr_lang")
                .selected_text(self.ocr_lang.label())
                .show_ui(ui, |ui| {
                    for lang in ocr::OcrLang::ALL {
                        ui.selectable_value(&mut self.ocr_lang, lang, lang.label());
                    }
                });
        });
        ui.horizontal(|ui| {
            if ui.button("Copier").clicked() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(self.text.clone());
                    self.status = "copié ✓".into();
                }
            }
            if ui.button("Traduire").clicked() {
                self.status = "traduction pas encore branchée".into();
            }
            if ui.button("Zone…").clicked() {
                self.picking = true;
            }
            if ui.button("Capturer").clicked() {
                let ctx2 = ui.ctx().clone();
                self.do_capture(&ctx2);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.status).small().color(egui::Color32::GRAY));
            });
        });

        if self.picking {
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
                let drag_start = &mut self.drag_start;
                let drag_now = &mut self.drag_now;
                let drag_screen = &mut self.drag_screen;
                let picking = &mut self.picking;
                let zone = &mut self.zone;
                let auto_capture_at = &mut self.auto_capture_at;
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
                                *zone = Some(PickedZone { monitor: mon_name.clone(), x: x0, y: y0, w: x1 - x0, h: y1 - y0, ppp: scale });
                                *status = format!("zone : {}×{} sur {}", x1 - x0, y1 - y0, mon_name);
                                *auto_capture_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
                            }
                        }
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
