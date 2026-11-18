//! Weiterlaufen im Hintergrund: Wird das Fenster geschlossen, während noch ein Spiel läuft,
//! versteckt sich der Launcher nur (Tray-Symbol zum Zurückholen). So bleiben Discord Rich
//! Presence und die Überwachung der Instanz aktiv. Sind alle Spiele beendet, beendet sich der
//! Launcher von selbst – er sollte ja eigentlich schon geschlossen sein.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent};

const TRAY_ID: &str = "lion-background";
const CHECK_INTERVAL: Duration = Duration::from_secs(3);

/// Fenster wurde geschlossen, der Launcher läuft nur noch wegen eines Spiels weiter.
static HIDDEN_WHILE_PLAYING: AtomicBool = AtomicBool::new(false);

/// Für `Builder::on_window_event`
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != "main" || crate::core::minecraft::get_running_profile_ids().is_empty() {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    HIDDEN_WHILE_PLAYING.store(true, Ordering::SeqCst);
    if let Err(e) = show_tray(window.app_handle()) {
        tracing::warn!("Tray icon could not be created: {}", e);
    }
}

/// Startet die Überwachung, die den versteckten Launcher nach dem letzten Spiel beendet.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("background-exit".into())
        .spawn(move || loop {
            std::thread::sleep(CHECK_INTERVAL);
            if HIDDEN_WHILE_PLAYING.load(Ordering::SeqCst)
                && crate::core::minecraft::get_running_profile_ids().is_empty()
            {
                tracing::info!("Last game closed while the launcher was hidden – exiting");
                app.exit(0);
                break;
            }
        })
        .ok();
}

fn show_main_window(app: &AppHandle) {
    HIDDEN_WHILE_PLAYING.store(false, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    // Das Symbol gibt es nur, solange der Launcher versteckt ist
    let _ = app.remove_tray_by_id(TRAY_ID);
}

fn show_tray(app: &AppHandle) -> tauri::Result<()> {
    if app.tray_by_id(TRAY_ID).is_some() {
        return Ok(());
    }
    let open = MenuItem::with_id(app, "open", "Lion Launcher öffnen", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Launcher beenden", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Lion Launcher – Spiel läuft")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../../icons/icon.png")) {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}
