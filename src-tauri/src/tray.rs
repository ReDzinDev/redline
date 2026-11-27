use std::sync::atomic::Ordering;

use tauri::{
    App, Emitter, Manager,
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::{
    AppState,
    app_info::{
        APP_NAME, TRAY_ID, TRAY_OPEN_ID, TRAY_PAUSE_ID, TRAY_QUIT_ID, TRAY_SETTINGS_ID,
        WINDOW_MAIN, WINDOW_POPUP, sanitize_log_value,
    },
    windows_mgr,
};

pub fn setup(app: &App) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, TRAY_OPEN_ID, "Open Redline", true, None::<&str>)?;
    let pause_item = CheckMenuItem::with_id(
        app,
        TRAY_PAUSE_ID,
        "Pause hotkey",
        true,
        false,
        None::<&str>,
    )?;
    let settings_item = MenuItem::with_id(app, TRAY_SETTINGS_ID, "Settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &pause_item, &settings_item, &quit_item])?;

    let pause_for_handler = pause_item.clone();

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(APP_NAME)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            TRAY_OPEN_ID => {
                if let Err(err) = windows_mgr::open_or_focus_main(app) {
                    log::error!("failed to open main window from tray: {err}");
                }
                let _ = app.emit_to(WINDOW_MAIN, "main:navigate", "write");
            }
            TRAY_SETTINGS_ID => {
                if let Err(err) = windows_mgr::open_or_focus_main(app) {
                    log::error!("failed to open main window on settings: {err}");
                }
                let _ = app.emit_to(WINDOW_MAIN, "main:navigate", "settings");
            }
            TRAY_PAUSE_ID => {
                let state = app.state::<AppState>();
                let currently_paused = state.hotkey_paused.load(Ordering::SeqCst);
                let next_paused = !currently_paused;
                let hotkey = state
                    .settings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .hotkey
                    .clone();
                let safe_hotkey = sanitize_log_value(&hotkey, 64);

                if next_paused {
                    let _ = app.global_shortcut().unregister(hotkey.as_str());
                    state.hotkey_paused.store(true, Ordering::SeqCst);
                    let _ = pause_for_handler.set_checked(true);
                    log::info!("paused global hotkey '{safe_hotkey}'");
                } else {
                    match app.global_shortcut().register(hotkey.as_str()) {
                        Ok(()) => {
                            state.hotkey_paused.store(false, Ordering::SeqCst);
                            let _ = pause_for_handler.set_checked(false);
                            *state
                                .hotkey_conflict
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                            log::info!("resumed global hotkey '{safe_hotkey}'");
                        }
                        Err(err) => {
                            log::warn!("failed to resume global hotkey '{safe_hotkey}': {err}");
                            let _ = pause_for_handler.set_checked(true);
                            *state
                                .hotkey_conflict
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                                Some(hotkey.clone());
                            let _ = windows_mgr::open_or_focus_main(app);
                            let _ = app.emit_to(WINDOW_MAIN, "main:navigate", "settings");
                        }
                    }
                }
            }
            TRAY_QUIT_ID => {
                let state = app.state::<AppState>();
                state.quitting.store(true, Ordering::SeqCst);
                crate::cancel_inflight(&state, WINDOW_POPUP);
                crate::cancel_inflight(&state, WINDOW_MAIN);
                // Drain any in-flight capture/paste job so ClipboardSnapshot restores cleanly.
                state.worker.shutdown();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Err(err) = windows_mgr::open_or_focus_main(app) {
                    log::error!("failed to open main window from tray click: {err}");
                }
                let _ = app.emit_to(WINDOW_MAIN, "main:navigate", "write");
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
