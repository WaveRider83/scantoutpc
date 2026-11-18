use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Clone, serde::Serialize)]
struct Zone {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

struct AppState {
    zone: Mutex<Option<Zone>>,
}

#[tauri::command]
fn set_zone(app: tauri::AppHandle, state: tauri::State<'_, AppState>, x: i32, y: i32, width: u32, height: u32) {
    let zone = Zone { x, y, width, height };
    *state.zone.lock().unwrap() = Some(zone.clone());
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }
    let _ = app.emit_to("main", "zone-set", zone);
}

#[tauri::command]
fn get_zone(state: tauri::State<'_, AppState>) -> Option<Zone> {
    state.zone.lock().unwrap().clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState { zone: Mutex::new(None) })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![set_zone, get_zone])
        .setup(|app| {
            // Tray
            let show = MenuItemBuilder::with_id("show", "Afficher / Masquer").build(app)?;
            let zone = MenuItemBuilder::with_id("zone", "Definir la zone...").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quitter").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &zone, &quit]).build()?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("ScantoutPC")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => toggle_main(app),
                    "zone" => show_overlay(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Raccourci global : Ctrl+Shift+O = afficher/masquer la fenetre
            app.global_shortcut().on_shortcut("ctrl+shift+o", move |app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    toggle_main(app);
                }
            })?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn toggle_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn show_overlay(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}
