mod ai;
mod app_info;
mod capture;
mod commands;
mod dto;
mod platform;
mod secrets;
mod settings;
mod tray;
mod windows_mgr;

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use crate::ai::chat_client::ChatClient;
use crate::ai::models::reasoning_mode_for_model;
use crate::ai::prompts::{Action, build_prompt, postprocess_reply};
use crate::ai::provider::{ChatRequest, Provider};
use crate::capture::{PasteOutcome, PlatformWorker};
use crate::settings::Settings;

/// Shared application state managed by Tauri.
pub struct AppState {
    pub quitting: AtomicBool,
    pub bare_loop_busy: AtomicBool,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub settings_recovered: bool,
    pub hotkey_conflict: Mutex<Option<String>>,
    pub worker: PlatformWorker,
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

struct BusyReset {
    app: AppHandle,
}

impl Drop for BusyReset {
    fn drop(&mut self) {
        self.app
            .state::<AppState>()
            .bare_loop_busy
            .store(false, Ordering::SeqCst);
    }
}

/// Milestone 1 bare loop (no popup yet):
/// Hotkey -> capture selection -> OpenRouter "Fix grammar" -> paste back with clipboard restore.
fn on_hotkey_pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.worker.is_busy()
        || state
            .bare_loop_busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        log::debug!("ignoring hotkey while capture or rewrite is in flight");
        return;
    }

    let Some(target) = platform::focus::capture_foreground_window() else {
        log::debug!("hotkey pressed with no active foreground window");
        state.bare_loop_busy.store(false, Ordering::SeqCst);
        return;
    };

    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let _busy_guard = BusyReset {
            app: app_handle.clone(),
        };
        run_bare_loop(app_handle, target).await;
    });
}

async fn run_bare_loop(app: AppHandle, target: platform::focus::TargetWindow) {
    let started = Instant::now();
    let target_proc = target.process_name.clone();

    let capture_app = app.clone();
    let captured = match tauri::async_runtime::spawn_blocking(move || {
        capture_app
            .state::<AppState>()
            .worker
            .capture_selection(target)
    })
    .await
    {
        Ok(Ok(sel)) => sel,
        Ok(Err(err)) => {
            log::info!(
                "capture did not complete: code={} app={} elapsed_ms={}",
                err.code(),
                target_proc,
                started.elapsed().as_millis()
            );
            return;
        }
        Err(err) => {
            log::error!("capture task join error: {err}");
            return;
        }
    };

    log::info!(
        "captured selection: method={:?} app={} chars={} elapsed_ms={}",
        captured.method,
        captured.target.process_name,
        captured.text.chars().count(),
        started.elapsed().as_millis()
    );

    let api_key = match secrets::get_api_key() {
        Ok(Some(key)) => key,
        Ok(None) => {
            log::info!("no OpenRouter API key configured; opening Settings");
            let _ = windows_mgr::open_or_focus_main(&app);
            return;
        }
        Err(err) => {
            log::error!("credential store read failed: {err}");
            let _ = windows_mgr::open_or_focus_main(&app);
            return;
        }
    };

    let (model_id, reasoning, deny_data_collection) = {
        let state = app.state::<AppState>();
        let guard = state
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let model = guard.active_model_id().to_string();
        let reasoning = reasoning_mode_for_model(&model, guard.custom_model_reasoning.as_ref());
        (model, reasoning, guard.deny_data_collection)
    };

    let client = match ChatClient::openrouter(api_key, deny_data_collection) {
        Ok(c) => c,
        Err(err) => {
            log::error!("failed to build OpenRouter client: code={}", err.code());
            return;
        }
    };

    let built = build_prompt(&Action::FixGrammar, &captured.text);
    let req = ChatRequest {
        model: model_id.clone(),
        system: built.system.clone(),
        user: built.user.clone(),
        max_tokens: built.max_tokens,
        reasoning,
    };

    let ai_started = Instant::now();
    let completion = match client.stream(req, |_| {}).await {
        Ok(done) => done,
        Err(err) => {
            log::warn!(
                "AI rewrite failed: code={} model={} elapsed_ms={}",
                err.code(),
                model_id,
                ai_started.elapsed().as_millis()
            );
            return;
        }
    };

    let cleaned = postprocess_reply(&completion.text, &built);
    log::info!(
        "AI rewrite complete: model={} finish={} cost_usd={:?} reasoning_tokens={:?} out_chars={} ai_ms={}",
        completion.model_used,
        completion.finish_reason,
        completion.cost_usd,
        completion.reasoning_tokens,
        cleaned.chars().count(),
        ai_started.elapsed().as_millis()
    );

    if cleaned == captured.text {
        log::info!(
            "rewrite unchanged; restoring focus without pasting (total_ms={})",
            started.elapsed().as_millis()
        );
        let target_for_focus = captured.target;
        let focus_app = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            focus_app
                .state::<AppState>()
                .worker
                .restore_focus(target_for_focus)
        })
        .await;
        return;
    }

    let paste_app = app.clone();
    let target_for_paste = captured.target;
    match tauri::async_runtime::spawn_blocking(move || {
        paste_app
            .state::<AppState>()
            .worker
            .paste_result(target_for_paste, cleaned)
    })
    .await
    {
        Ok(Ok(PasteOutcome::Pasted { rendered })) => {
            log::info!(
                "paste complete: rendered={} app={} total_ms={}",
                rendered,
                target_proc,
                started.elapsed().as_millis()
            );
        }
        Ok(Ok(PasteOutcome::CopiedFallback)) => {
            log::warn!(
                "focus could not be restored to {}; result copied to clipboard",
                target_proc
            );
        }
        Ok(Err(err)) => {
            log::warn!("paste failed: code={} app={}", err.code(), target_proc);
        }
        Err(err) => {
            log::error!("paste task join error: {err}");
        }
    }
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
            commands::save_privacy,
        ])
        .setup(|app| {
            if let Err(err) = secrets::init_store() {
                log::error!("failed to initialize credential store: {err}");
            }

            let config_dir = app.path().app_config_dir()?;
            let path = settings::settings_path(&config_dir);
            let loaded = settings::load_from_path(&path);
            let hotkey = loaded.settings.hotkey.clone();

            let worker = PlatformWorker::spawn();
            let mut conflict: Option<String> = None;

            if let Err(err) = app.global_shortcut().register(hotkey.as_str()) {
                log::warn!("failed to register global shortcut '{hotkey}': {err}");
                conflict = Some(hotkey.clone());
            } else {
                log::info!("registered global shortcut '{hotkey}'");
            }

            let open_settings_on_start = conflict.is_some();

            app.manage(AppState {
                quitting: AtomicBool::new(false),
                bare_loop_busy: AtomicBool::new(false),
                settings: Mutex::new(loaded.settings),
                settings_path: path,
                settings_recovered: loaded.recovered_from_corrupt,
                hotkey_conflict: Mutex::new(conflict),
                worker,
            });

            tray::setup(app)?;

            if open_settings_on_start {
                let _ = windows_mgr::open_or_focus_main(app.handle());
            }

            log::info!("{} started in tray", app_info::APP_NAME);
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
