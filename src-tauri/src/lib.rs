mod app_info;
mod tray;
mod windows_mgr;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, RunEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

/// Tracks explicit Quit requests so closing the on-demand main window keeps Redline in the tray.
pub struct AppLifecycle {
    pub quitting: AtomicBool,
}

impl Default for AppLifecycle {
    fn default() -> Self {
        Self {
            quitting: AtomicBool::new(false),
        }
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(location) = info.location() {
            log::error!(
                "panic at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        } else {
            log::error!("panic at unknown location");
        }
        previous(info);
    }));
}

fn build_log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let mut targets = vec![Target::new(TargetKind::LogDir { file_name: None })];
    if cfg!(debug_assertions) {
        targets.push(Target::new(TargetKind::Stdout));
    }

    tauri_plugin_log::Builder::new()
        .targets(targets)
        .max_file_size(1_048_576)
        .rotation_strategy(RotationStrategy::KeepSome(2))
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    let app = tauri::Builder::default()
        // Single-instance MUST be registered first so secondary launches are intercepted immediately.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Err(err) = windows_mgr::open_or_focus_main(app) {
                log::error!("failed to focus main window on second instance: {err}");
            }
        }))
        .plugin(build_log_plugin())
        .manage(AppLifecycle::default())
        .setup(|app| {
            tray::setup(app)?;
            log::info!("{} started in tray", app_info::APP_NAME);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Redline application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !app_handle
                .state::<AppLifecycle>()
                .quitting
                .load(Ordering::SeqCst)
        {
            api.prevent_exit();
        }
    });
}
