use std::sync::atomic::Ordering;

use tauri::{
    App, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    AppState,
    app_info::{APP_NAME, TRAY_ID, TRAY_OPEN_ID, TRAY_QUIT_ID, TRAY_SETTINGS_ID},
    windows_mgr,
};

pub fn setup(app: &App) -> tauri::Result<()> {
    let open_item = MenuItem::with_id(app, TRAY_OPEN_ID, "Open Redline", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, TRAY_SETTINGS_ID, "Settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &settings_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(APP_NAME)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            TRAY_OPEN_ID | TRAY_SETTINGS_ID => {
                if let Err(err) = windows_mgr::open_or_focus_main(app) {
                    log::error!("failed to open main window from tray: {err}");
                }
            }
            TRAY_QUIT_ID => {
                app.state::<AppState>()
                    .quitting
                    .store(true, Ordering::SeqCst);
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
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
