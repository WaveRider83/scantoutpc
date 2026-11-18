// ScantoutPC — OCR live pour Windows (fenêtre flottante + overlay de sélection)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui;
use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager};
use tray_icon::{TrayIconBuilder, Icon};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};

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
                main_visible: true,
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
    zone: Option<[i32; 4]>,
    picking: bool,
    drag_start: Option<egui::Pos2>,
    drag_now: Option<egui::Pos2>,
    main_visible: bool,
    _hotkey_manager: GlobalHotKeyManager,
}

impl ScantoutApp {
    fn toggle_main(&mut self, ctx: &egui::Context) {
        self.main_visible = !self.main_visible;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.main_visible));
    }
}

impl eframe::App for ScantoutApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            match event.id().0.as_str() {
                "toggle" => self.toggle_main(ctx),
                "zone" => self.picking = true,
                "quit" => std::process::exit(0),
                _ => {}
            }
        }
        if let Ok(_event) = GlobalHotKeyEvent::receiver().try_recv() {
            self.toggle_main(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let title = ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("● ScantoutPC").color(egui::Color32::from_rgb(96, 165, 250)).strong());
        }).response;
        let title_rect = title.rect.expand2(egui::vec2(8.0, 4.0));
        let drag = ui.interact(title_rect, ui.id().with("drag-title"), egui::Sense::drag());
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.status).small().color(egui::Color32::GRAY));
            });
        });

        if self.picking {
            let builder = egui::ViewportBuilder::default()
                .with_title("zone")
                .with_fullscreen(true)
                .with_decorations(false)
                .with_transparent(true)
                .with_always_on_top()
                .with_taskbar(false);
            let drag_start = &mut self.drag_start;
            let drag_now = &mut self.drag_now;
            let picking = &mut self.picking;
            let zone = &mut self.zone;
            let status = &mut self.status;
            ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("zone-overlay"), builder, |ui, _class| {
                let screen = ui.max_rect();
                let painter = ui.painter();
                painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));
                painter.text(
                    egui::Pos2::new(screen.center().x, 24.0),
                    egui::Align2::CENTER_TOP,
                    "Glissez pour définir la zone — Échap pour annuler",
                    egui::FontId::proportional(15.0),
                    egui::Color32::WHITE,
                );
                if let (Some(s), Some(n)) = (*drag_start, *drag_now) {
                    let rect = egui::Rect::from_two_pos(s, n);
                    painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(59, 130, 246, 40));
                    painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(59, 130, 246)), egui::StrokeKind::Inside);
                }
                let input = ui.input(|i| i.clone());
                if input.key_pressed(egui::Key::Escape) {
                    *picking = false;
                    *drag_start = None;
                    *drag_now = None;
                    return;
                }
                if input.pointer.button_pressed(egui::PointerButton::Primary) {
                    *drag_start = input.pointer.latest_pos();
                    *drag_now = *drag_start;
                }
                if input.pointer.button_down(egui::PointerButton::Primary) {
                    if let Some(p) = input.pointer.latest_pos() {
                        *drag_now = Some(p);
                    }
                }
                if input.pointer.button_released(egui::PointerButton::Primary) {
                    if let (Some(s), Some(e)) = (*drag_start, *drag_now) {
                        let (x0, x1) = (s.x.min(e.x).round() as i32, s.x.max(e.x).round() as i32);
                        let (y0, y1) = (s.y.min(e.y).round() as i32, s.y.max(e.y).round() as i32);
                        if x1 - x0 >= 20 && y1 - y0 >= 20 {
                            *zone = Some([x0, y0, x1 - x0, y1 - y0]);
                            *status = format!("zone : {}×{} @ {},{}", x1 - x0, y1 - y0, x0, y0);
                        }
                    }
                    *picking = false;
                    *drag_start = None;
                    *drag_now = None;
                }
                ui.ctx().request_repaint();
            });
        }
    }
}
