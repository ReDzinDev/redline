mod ai;
mod app_info;
mod capture;
mod commands;
mod dto;
mod grammar;
mod history;
mod platform;
mod secrets;
mod settings;
mod tray;
mod windows_mgr;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::ai::prompts::{LineEnding, normalize_line_endings};
use crate::app_info::WINDOW_POPUP;
use crate::capture::PlatformWorker;
use crate::dto::{PendingEditorDraft, PopupSessionEvent};
use crate::grammar::GrammarEngine;
use crate::history::HistoryStore;
use crate::platform::focus::TargetWindow;
use crate::settings::Settings;

const BLUR_DISMISS_GRACE: Duration = Duration::from_millis(350);

/// Holds the active inline capture target for the currently open popup session.
pub struct ActiveCapture {
    pub target: TargetWindow,
    pub original_line_ending: LineEnding,
}

/// Shared application state managed by Tauri.
pub struct AppState {
    pub quitting: AtomicBool,
    pub hotkey_paused: AtomicBool,
    pub popup_visible: AtomicBool,
    pub popup_capturing: AtomicBool,
    pub popup_activated_at: Mutex<Option<Instant>>,
    pub session_counter: AtomicU64,
    pub active_capture: Mutex<Option<ActiveCapture>>,
    pub pending_editor_draft: Mutex<Option<PendingEditorDraft>>,
    pub inflight_rewrites: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub settings_recovered: bool,
    pub hotkey_conflict: Mutex<Option<String>>,
    pub history: Mutex<HistoryStore>,
    pub worker: PlatformWorker,
    pub grammar: GrammarEngine,
}

impl AppState {
    pub fn should_dismiss_on_blur(&self) -> bool {
        if !self.popup_visible.load(Ordering::SeqCst) || self.popup_capturing.load(Ordering::SeqCst)
        {
            return false;
        }
        let activated = *self
            .popup_activated_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match activated {
            Some(at) => at.elapsed() >= BLUR_DISMISS_GRACE,
            None => true,
        }
    }
}

pub fn cancel_inflight(state: &AppState, window_id: &str) {
    if let Some(handle) = state
        .inflight_rewrites
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(window_id)
    {
        handle.abort();
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

fn on_hotkey_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.hotkey_paused.load(Ordering::SeqCst) {
        return;
    }

    // Toggle closed if the popup is already visible (check both state and OS window visibility).
    if state.popup_visible.load(Ordering::SeqCst) || windows_mgr::is_popup_hwnd_visible(app) {
        cancel_inflight(&state, WINDOW_POPUP);
        windows_mgr::hide_popup(app);
        let active = state
            .active_capture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(capture) = active {
            let focus_app = app.clone();
            let target = capture.target;
            tauri::async_runtime::spawn_blocking(move || {
                focus_app.state::<AppState>().worker.restore_focus(target);
            });
        }
        return;
    }

    if state.worker.is_busy() {
        log::debug!("ignoring hotkey while platform worker is busy");
        return;
    }

    let started = Instant::now();
    let Some(target) = platform::focus::capture_foreground_window() else {
        log::debug!("hotkey pressed with no active foreground window");
        return;
    };

    let session_id = state.session_counter.fetch_add(1, Ordering::SeqCst) + 1;
    let source_app = target.process_name.clone();

    if let Err(err) = windows_mgr::show_popup_for_capture(app) {
        log::error!("failed showing popup for capture: {err}");
        return;
    }

    log::info!(
        "popup shown (capturing): session={} app={} hotkey_to_popup_ms={}",
        session_id,
        source_app,
        started.elapsed().as_millis()
    );

    let _ = app.emit_to(
        WINDOW_POPUP,
        "popup:session",
        PopupSessionEvent::Capturing {
            session_id,
            source_app: source_app.clone(),
        },
    );

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let capture_app = app_handle.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            capture_app
                .state::<AppState>()
                .worker
                .capture_selection(target)
        })
        .await;

        let state = app_handle.state::<AppState>();
        if !state.popup_visible.load(Ordering::SeqCst)
            || state.session_counter.load(Ordering::SeqCst) != session_id
        {
            return;
        }

        match result {
            Ok(Ok(captured)) => {
                let (normalized_text, original_line_ending) =
                    normalize_line_endings(&captured.text);
                let (recent_custom_instructions, auto_run_action) = {
                    let guard = state
                        .settings
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    (
                        guard.recent_custom_instructions.clone(),
                        guard.auto_run_action.clone(),
                    )
                };

                log::info!(
                    "capture ready: session={} method={:?} app={} chars={} elapsed_ms={}",
                    session_id,
                    captured.method,
                    captured.target.process_name,
                    normalized_text.chars().count(),
                    started.elapsed().as_millis()
                );

                *state
                    .active_capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(ActiveCapture {
                    target: captured.target,
                    original_line_ending,
                });

                windows_mgr::activate_popup_after_capture(&app_handle);

                let _ = app_handle.emit_to(
                    WINDOW_POPUP,
                    "popup:session",
                    PopupSessionEvent::Ready {
                        session_id,
                        source_app,
                        text: normalized_text,
                        recent_custom_instructions,
                        auto_run_action,
                    },
                );
            }
            Ok(Err(err)) => {
                log::info!(
                    "capture ended with error: session={} code={} app={} elapsed_ms={}",
                    session_id,
                    err.code(),
                    source_app,
                    started.elapsed().as_millis()
                );
                *state
                    .active_capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                windows_mgr::activate_popup_after_capture(&app_handle);

                let _ = app_handle.emit_to(
                    WINDOW_POPUP,
                    "popup:session",
                    PopupSessionEvent::Error {
                        session_id,
                        source_app,
                        error_code: err.code().to_string(),
                    },
                );
            }
            Err(err) => {
                log::error!("capture task join error: {err}");
                windows_mgr::hide_popup(&app_handle);
            }
        }
    });
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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        on_hotkey_pressed(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::get_settings,
            commands::get_curated_models,
            commands::save_api_key,
            commands::delete_api_key,
            commands::test_api_key,
            commands::save_model,
            commands::save_custom_model,
            commands::save_privacy,
            commands::save_dialect,
            commands::save_hotkey,
            commands::save_history_enabled,
            commands::save_auto_run_action,
            commands::set_autostart_enabled,
            commands::complete_welcome,
            commands::get_history,
            commands::clear_history,
            commands::record_history_entry,
            commands::open_external_url,
            commands::open_log_folder,
            commands::lint_text,
            commands::rewrite_text,
            commands::cancel_rewrite,
            commands::resize_popup,
            commands::accept_popup_result,
            commands::dismiss_popup,
            commands::copy_to_clipboard,
            commands::open_in_editor,
            commands::open_settings,
            commands::take_editor_draft,
        ])
        .setup(|app| {
            if let Err(err) = secrets::init_store() {
                log::error!("failed to initialize credential store: {err}");
            }

            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let settings_file = settings::settings_path(&config_dir);
            let loaded = settings::load_from_path(&settings_file);
            let history_store = HistoryStore::load_from_path(history::history_path(&data_dir));

            let hotkey = loaded.settings.hotkey.clone();
            let first_launch = !loaded.settings.welcome_completed;

            let worker = PlatformWorker::spawn();
            let grammar = GrammarEngine::new(&loaded.settings.dialect);
            let mut conflict: Option<String> = None;

            if let Err(err) = app.global_shortcut().register(hotkey.as_str()) {
                log::warn!("failed to register global shortcut '{hotkey}': {err}");
                conflict = Some(hotkey.clone());
            } else {
                log::info!("registered global shortcut '{hotkey}'");
            }

            let open_main_on_start = first_launch || conflict.is_some();
            let initial_nav = if conflict.is_some() {
                "settings"
            } else {
                "write"
            };

            app.manage(AppState {
                quitting: AtomicBool::new(false),
                hotkey_paused: AtomicBool::new(false),
                popup_visible: AtomicBool::new(false),
                popup_capturing: AtomicBool::new(false),
                popup_activated_at: Mutex::new(None),
                session_counter: AtomicU64::new(0),
                active_capture: Mutex::new(None),
                pending_editor_draft: Mutex::new(None),
                inflight_rewrites: Mutex::new(HashMap::new()),
                settings: Mutex::new(loaded.settings),
                settings_path: settings_file,
                settings_recovered: loaded.recovered_from_corrupt,
                hotkey_conflict: Mutex::new(conflict),
                history: Mutex::new(history_store),
                worker,
                grammar,
            });

            windows_mgr::precreate_popup(app.handle())?;
            tray::setup(app)?;

            if open_main_on_start {
                let _ = windows_mgr::open_or_focus_main(app.handle());
                let _ = app.emit("main:navigate", initial_nav);
            }

            log::info!("{} started", app_info::APP_NAME);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Redline application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event
            && !app_handle
                .state::<AppState>()
                .quitting
                .load(Ordering::SeqCst)
        {
            api.prevent_exit();
        }
    });
}
