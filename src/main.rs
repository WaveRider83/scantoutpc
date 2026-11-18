// ScantoutPC — OCR live pour Windows (fenêtre flottante + overlay de sélection)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use tray_icon::{TrayIconBuilder, Icon};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};

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
            .with_inner_size([420.0, 300.0])
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(true),
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
                auto_capture: false,
                _hotkey_manager: hotkey_manager,
            }))
        }),
    )
}

fn make_icon() -> Icon {
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
    Icon::from_rgba(rgba, 32, 32).expect("icon")
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
    auto_capture: bool,
    _hotkey_manager: GlobalHotKeyManager,
}

impl ScantoutApp {
    fn do_capture(&mut self, ctx: &egui::Context) {
        match self.zone.clone() {
            Some(z) => match capture_zone(&z) {
                Ok(img) => {
                    let dump = std::env::temp_dir().join("scantoutpc_capture.png");
                    let _ = img.save(&dump);
                    let size = [img.width() as usize, img.height() as usize];
                    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    self.capture = Some(ctx.load_texture("capture", color, egui::TextureOptions::LINEAR));
                    self.status = format!("capturé {}×{}", img.width(), img.height());
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
        if std::mem::take(&mut self.auto_capture) {
            self.do_capture(&ctx);
        }
        if self.picking && !self.overlay_active {
            let mut v = egui::Visuals::dark();
            v.window_fill = egui::Color32::TRANSPARENT;
            v.panel_fill = egui::Color32::TRANSPARENT;
            v.extreme_bg_color = egui::Color32::TRANSPARENT;
            v.faint_bg_color = egui::Color32::TRANSPARENT;
            ctx.set_visuals(v);
            self.overlay_active = true;
        }
        if !self.picking && self.overlay_active {
            ctx.set_visuals(egui::Visuals::dark());
            self.overlay_active = false;
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
            let monitors = xcap::Monitor::all().unwrap_or_default();
            for (idx, m) in monitors.iter().enumerate() {
                let scale = m.scale_factor().unwrap_or(1.0);
                let lw = m.width().unwrap_or(0) as f32 / scale;
                let lh = m.height().unwrap_or(0) as f32 / scale;
                let lx = m.x().unwrap_or(0) as f32 / scale;
                let ly = m.y().unwrap_or(0) as f32 / scale;
                let mon_name = m.name().unwrap_or_else(|_| format!("mon-{idx}"));
                let builder = egui::ViewportBuilder::default()
                    .with_title("zone")
                    .with_inner_size([lw, lh])
                    .with_position([lx, ly])
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_always_on_top()
                    .with_active(true)
                    .with_taskbar(false)
                    .with_resizable(false);
                let drag_start = &mut self.drag_start;
                let drag_now = &mut self.drag_now;
                let drag_screen = &mut self.drag_screen;
                let picking = &mut self.picking;
                let zone = &mut self.zone;
                let auto_capture = &mut self.auto_capture;
                let status = &mut self.status;
                let scale = scale;
                ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(("zone-overlay", idx)), builder, |ui, _class| {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Transparent(true));
                    let screen = ui.max_rect();
                    ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(110));
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
                                *auto_capture = true;
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
