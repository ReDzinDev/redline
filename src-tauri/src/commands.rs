use std::sync::atomic::Ordering;
use std::time::Instant;

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_opener::OpenerExt;

use crate::AppState;
use crate::ai::chat_client::{ChatClient, test_openrouter_key, validate_custom_model};
use crate::ai::models::{curated_model_dtos, reasoning_mode_for_model};
use crate::ai::prompts::{Action, build_prompt, postprocess_reply_lf, restore_line_endings};
use crate::ai::provider::{AiError, ChatRequest, Provider};
use crate::app_info::{
    EDITOR_MAX_INPUT_CHARS, OPENROUTER_CREDITS_URL, OPENROUTER_KEYS_URL, POPUP_MAX_INPUT_CHARS,
    REPO_URL, WINDOW_MAIN, WINDOW_POPUP,
};
use crate::capture::PasteOutcome;
use crate::dto::{
    AcceptHistoryMetaDto, AcceptResultDto, ActionRequestDto, ApiErrorDto, AppStatusDto,
    KeyTestResultDto, LintResponseDto, ModelOptionDto, PendingEditorDraft, RewriteStreamEvent,
    SettingsDto,
};
use crate::history::{HistoryEntry, NewHistoryEntry};
use crate::platform::clipboard;
use crate::{secrets, settings, windows_mgr};

#[tauri::command]
pub fn get_app_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppStatusDto, ApiErrorDto> {
    let has_api_key = secrets::has_api_key()
        .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?;
    let (hotkey, welcome_completed) = {
        let guard = state
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (guard.hotkey.clone(), guard.welcome_completed)
    };
    let hotkey_conflict = state
        .hotkey_conflict
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let history_recovered = state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recovered_from_corrupt;

    Ok(AppStatusDto {
        has_api_key,
        hotkey,
        hotkey_conflict,
        hotkey_paused: state.hotkey_paused.load(Ordering::SeqCst),
        settings_recovered: state.settings_recovered,
        history_recovered,
        welcome_completed,
        autostart_enabled,
        app_version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsDto {
    let guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    SettingsDto::from(&*guard)
}

#[tauri::command]
pub fn get_curated_models() -> Vec<ModelOptionDto> {
    curated_model_dtos()
}

#[tauri::command]
pub fn save_api_key(key: String) -> Result<bool, ApiErrorDto> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(ApiErrorDto::new("ai_no_key", None));
    }
    secrets::set_api_key(trimmed)
        .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?;
    log::info!("saved OpenRouter API key to Credential Manager");
    Ok(true)
}

#[tauri::command]
pub fn delete_api_key() -> Result<bool, ApiErrorDto> {
    secrets::delete_api_key()
        .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?;
    log::info!("removed OpenRouter API key from Credential Manager");
    Ok(false)
}

#[tauri::command]
pub async fn test_api_key(key: Option<String>) -> Result<KeyTestResultDto, ApiErrorDto> {
    let candidate = match key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(provided) => provided.to_string(),
        None => secrets::get_api_key()
            .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?
            .ok_or_else(|| ApiErrorDto::new("ai_no_key", None))?,
    };

    test_openrouter_key(&candidate).await.map_err(Into::into)
}

#[tauri::command]
pub fn save_model(state: State<'_, AppState>, model: String) -> Result<SettingsDto, ApiErrorDto> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(ApiErrorDto::new("invalid_model", None));
    }

    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.model = trimmed.to_string();
    guard.custom_model_id = None;
    guard.custom_model_reasoning = None;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    log::info!("updated active model to {}", guard.model);
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub async fn save_custom_model(
    state: State<'_, AppState>,
    custom_model_id: Option<String>,
) -> Result<SettingsDto, ApiErrorDto> {
    let trimmed = custom_model_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let derived_reasoning = match trimmed.as_deref() {
        Some(model_id) => {
            let api_key = secrets::get_api_key().ok().flatten();
            let mode = validate_custom_model(model_id, api_key.as_deref()).await?;
            Some(mode)
        }
        None => None,
    };

    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.custom_model_id = trimmed;
    guard.custom_model_reasoning = derived_reasoning;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    log::info!("updated custom_model_id={:?}", guard.custom_model_id);
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn save_privacy(
    state: State<'_, AppState>,
    deny_data_collection: bool,
) -> Result<SettingsDto, ApiErrorDto> {
    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.deny_data_collection = deny_data_collection;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    log::info!(
        "updated deny_data_collection={}",
        guard.deny_data_collection
    );
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn save_dialect(
    state: State<'_, AppState>,
    dialect: String,
) -> Result<SettingsDto, ApiErrorDto> {
    let normalized = match dialect.trim().to_ascii_lowercase().as_str() {
        "british" => "british",
        "canadian" => "canadian",
        "australian" => "australian",
        "indian" => "indian",
        _ => "american",
    };

    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.dialect = normalized.to_string();
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    state.grammar.set_dialect(normalized);
    log::info!("updated grammar dialect={normalized}");
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn save_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkey: String,
) -> Result<SettingsDto, ApiErrorDto> {
    let next = hotkey.trim();
    if next.is_empty() {
        return Err(ApiErrorDto::new(
            "hotkey_conflict",
            Some("Shortcut cannot be empty".to_string()),
        ));
    }

    let previous = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .hotkey
        .clone();

    let paused = state.hotkey_paused.load(Ordering::SeqCst);
    if !paused {
        let _ = app.global_shortcut().unregister(previous.as_str());
        if let Err(err) = app.global_shortcut().register(next) {
            // Re-register previous hotkey if the new one is unavailable.
            let _ = app.global_shortcut().register(previous.as_str());
            log::warn!("hotkey '{next}' could not be registered: {err}");
            return Err(ApiErrorDto::new("hotkey_conflict", Some(next.to_string())));
        }
    }

    *state
        .hotkey_conflict
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.hotkey = next.to_string();
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    log::info!("updated global hotkey to '{}'", guard.hotkey);
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn save_history_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsDto, ApiErrorDto> {
    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.history_enabled = enabled;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn save_auto_run_action(
    state: State<'_, AppState>,
    action: Option<String>,
) -> Result<SettingsDto, ApiErrorDto> {
    let normalized = match action.as_deref() {
        Some("improve") => Some("improve".to_string()),
        Some("fix_grammar") => Some("fix_grammar".to_string()),
        Some("shorten") => Some("shorten".to_string()),
        Some("make_clearer") => Some("make_clearer".to_string()),
        _ => None,
    };

    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.auto_run_action = normalized;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, ApiErrorDto> {
    let manager = app.autolaunch();
    let res = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    res.map_err(|err| ApiErrorDto::new("autostart_error", Some(err.to_string())))?;
    Ok(manager.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
pub fn complete_welcome(state: State<'_, AppState>) -> Result<SettingsDto, ApiErrorDto> {
    let mut guard = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.welcome_completed = true;
    settings::save_to_path(&state.settings_path, &guard)
        .map_err(|err| ApiErrorDto::new("settings_save_failed", Some(err.to_string())))?;
    Ok(SettingsDto::from(&*guard))
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> Vec<HistoryEntry> {
    state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entries()
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), ApiErrorDto> {
    state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear()
        .map_err(|err| ApiErrorDto::new("history_error", Some(err.to_string())))
}

#[tauri::command]
pub fn open_external_url(
    app: AppHandle,
    window: tauri::WebviewWindow,
    target: String,
) -> Result<(), ApiErrorDto> {
    if window.label() != WINDOW_MAIN {
        return Err(ApiErrorDto::new("forbidden_window", None));
    }
    let url = match target.as_str() {
        "repo" => REPO_URL,
        "openrouter_keys" => OPENROUTER_KEYS_URL,
        "openrouter_credits" => OPENROUTER_CREDITS_URL,
        _ => return Err(ApiErrorDto::new("invalid_url_target", None)),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| ApiErrorDto::new("opener_error", Some(err.to_string())))
}

#[tauri::command]
pub fn open_log_folder(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), ApiErrorDto> {
    if window.label() != WINDOW_MAIN {
        return Err(ApiErrorDto::new("forbidden_window", None));
    }
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|err| ApiErrorDto::new("opener_error", Some(err.to_string())))?;
    let _ = std::fs::create_dir_all(&log_dir);
    app.opener()
        .open_path(log_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|err| ApiErrorDto::new("opener_error", Some(err.to_string())))
}

#[tauri::command]
pub async fn lint_text(
    app: AppHandle,
    text: String,
    request_id: u64,
) -> Result<LintResponseDto, ApiErrorDto> {
    let lint_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        lint_app.state::<AppState>().grammar.lint(&text, request_id)
    })
    .await
    .map_err(|err| ApiErrorDto::new("lint_failed", Some(err.to_string())))
}

#[tauri::command]
pub async fn rewrite_text(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    request_id: u64,
    text: String,
    action: ActionRequestDto,
    on_event: Channel<RewriteStreamEvent>,
) -> Result<(), ApiErrorDto> {
    let window_id = window.label().to_string();
    let max_chars = if window_id == WINDOW_POPUP {
        POPUP_MAX_INPUT_CHARS
    } else {
        EDITOR_MAX_INPUT_CHARS
    };
    if text.chars().count() > max_chars {
        return Err(ApiErrorDto::new("selection_too_long", None));
    }

    let parsed_action = action.into_action()?;

    // Cancel any existing in-flight stream for this window before starting a new one.
    crate::cancel_inflight(&state, &window_id);

    let api_key = secrets::get_api_key()
        .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?
        .ok_or_else(|| ApiErrorDto::new("ai_no_key", None))?;

    let (model_id, reasoning, deny_data_collection) = {
        let mut guard = state
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Action::Custom(ref instruction) = parsed_action {
            guard.push_recent_custom_instruction(instruction);
            if let Err(err) = settings::save_to_path(&state.settings_path, &guard) {
                log::warn!("failed saving recent custom instruction: {err}");
            }
        }
        let model = guard.active_model_id().to_string();
        let reasoning = reasoning_mode_for_model(&model, guard.custom_model_reasoning.as_ref());
        (model, reasoning, guard.deny_data_collection)
    };

    let client = ChatClient::openrouter(api_key, deny_data_collection)?;
    let built = build_prompt(&parsed_action, &text);
    let req = ChatRequest {
        model: model_id.clone(),
        system: built.system.clone(),
        user: built.user.clone(),
        max_tokens: built.max_tokens,
        reasoning,
    };

    let stream_key = window_id.clone();
    let app_for_cleanup = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let mut accumulated = String::new();

        let result = client
            .stream(req, |delta| {
                accumulated.push_str(delta);
                let _ = on_event.send(RewriteStreamEvent::Delta {
                    request_id,
                    text: delta.to_string(),
                });
            })
            .await;

        let elapsed_ms = started.elapsed().as_millis() as u64;

        match result {
            Ok(completion) => {
                let cleaned_lf = postprocess_reply_lf(&completion.text, &built);
                let unchanged = cleaned_lf == built.normalized_input;
                log::info!(
                    "rewrite complete: window={} model={} finish={} cost_usd={:?} reasoning_tokens={:?} out_chars={} elapsed_ms={} unchanged={}",
                    stream_key,
                    completion.model_used,
                    completion.finish_reason,
                    completion.cost_usd,
                    completion.reasoning_tokens,
                    cleaned_lf.chars().count(),
                    elapsed_ms,
                    unchanged
                );
                let _ = on_event.send(RewriteStreamEvent::Complete {
                    request_id,
                    text: cleaned_lf,
                    model_used: completion.model_used,
                    cost_usd: completion.cost_usd,
                    elapsed_ms,
                    unchanged,
                });
            }
            Err(AiError::CutOff) => {
                log::warn!(
                    "rewrite cut off: window={} model={} partial_chars={} elapsed_ms={}",
                    stream_key,
                    model_id,
                    accumulated.chars().count(),
                    elapsed_ms
                );
                let _ = on_event.send(RewriteStreamEvent::CutOff {
                    request_id,
                    partial_text: accumulated,
                    detail: None,
                });
            }
            Err(AiError::MidStream { message }) if !accumulated.is_empty() => {
                log::warn!(
                    "rewrite mid-stream cut off: window={} model={} partial_chars={} elapsed_ms={}",
                    stream_key,
                    model_id,
                    accumulated.chars().count(),
                    elapsed_ms
                );
                let _ = on_event.send(RewriteStreamEvent::CutOff {
                    request_id,
                    partial_text: accumulated,
                    detail: message,
                });
            }
            Err(AiError::TimeoutOrNetwork { message }) if !accumulated.is_empty() => {
                log::warn!(
                    "rewrite network drop mid-stream: window={} model={} partial_chars={} elapsed_ms={}",
                    stream_key,
                    model_id,
                    accumulated.chars().count(),
                    elapsed_ms
                );
                let _ = on_event.send(RewriteStreamEvent::CutOff {
                    request_id,
                    partial_text: accumulated,
                    detail: message,
                });
            }
            Err(err) => {
                log::warn!(
                    "rewrite error: window={} code={} model={} elapsed_ms={}",
                    stream_key,
                    err.code(),
                    model_id,
                    elapsed_ms
                );
                let _ = on_event.send(RewriteStreamEvent::Error {
                    request_id,
                    error: err.into(),
                });
            }
        }

        // Remove our handle once finished so map doesn't retain completed JoinHandles.
        let app_state = app_for_cleanup.state::<AppState>();
        let mut inflight = app_state
            .inflight_rewrites
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inflight.remove(&stream_key);
    });

    state
        .inflight_rewrites
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(window_id, handle);

    Ok(())
}

#[tauri::command]
pub fn cancel_rewrite(window: tauri::WebviewWindow, state: State<'_, AppState>) {
    crate::cancel_inflight(&state, window.label());
}

#[tauri::command]
pub fn resize_popup(app: AppHandle, expanded: bool) -> Result<(), ApiErrorDto> {
    windows_mgr::resize_popup(&app, expanded)
        .map_err(|err| ApiErrorDto::new("window_error", Some(err.to_string())))
}

#[tauri::command]
pub async fn accept_popup_result(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    history_meta: Option<AcceptHistoryMetaDto>,
) -> Result<AcceptResultDto, ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup_and_wait(&app).await;

    let active = state
        .active_capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();

    let source_app = active.as_ref().map(|c| c.target.process_name.clone());

    let outcome = if let Some(capture) = active {
        let final_text = restore_line_endings(&text, capture.original_line_ending);
        let target = capture.target;
        let paste_app = app.clone();

        let res = tauri::async_runtime::spawn_blocking(move || {
            paste_app
                .state::<AppState>()
                .worker
                .paste_result(target, final_text)
        })
        .await
        .map_err(|err| ApiErrorDto::new("paste_failed", Some(err.to_string())))?
        .map_err(|err| ApiErrorDto::new(err.code(), Some(err.to_string())))?;

        match res {
            PasteOutcome::Pasted { rendered } => {
                log::info!(
                    "popup accepted and pasted: app={:?} rendered={}",
                    source_app,
                    rendered
                );
                "pasted"
            }
            PasteOutcome::CopiedFallback => {
                log::info!(
                    "popup accepted via clipboard fallback: app={:?}",
                    source_app
                );
                windows_mgr::reshow_popup_for_notice(&app);
                "copied_fallback"
            }
        }
    } else {
        clipboard::write_plain_unicode_text(&text)
            .map_err(|err| ApiErrorDto::new("clipboard_busy", Some(err.to_string())))?;
        windows_mgr::reshow_popup_for_notice(&app);
        "copied_fallback"
    };

    // Save history entry if history is enabled (covers both AI rewrites and Harper-only pastes).
    if let Some(meta) = history_meta {
        let enabled = state
            .settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .history_enabled;
        if let Err(err) = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(
                enabled,
                NewHistoryEntry {
                    action: meta.action,
                    source_app,
                    model: meta.model,
                    cost_usd: meta.cost_usd,
                    original: meta.original,
                    result: text,
                },
            )
        {
            log::warn!("failed saving history entry: {err}");
        }
    }

    Ok(AcceptResultDto {
        outcome: outcome.to_string(),
    })
}

#[tauri::command]
pub fn record_history_entry(
    state: State<'_, AppState>,
    entry: NewHistoryEntry,
) -> Result<(), ApiErrorDto> {
    let enabled = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .history_enabled;
    state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .record(enabled, entry)
        .map_err(|err| ApiErrorDto::new("history_error", Some(err.to_string())))
}

#[tauri::command]
pub async fn dismiss_popup(
    app: AppHandle,
    state: State<'_, AppState>,
    restore_focus: bool,
) -> Result<(), ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup_and_wait(&app).await;

    let active = state
        .active_capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();

    if restore_focus && let Some(capture) = active {
        let focus_app = app.clone();
        let target = capture.target;
        let _ = tauri::async_runtime::spawn_blocking(move || {
            focus_app.state::<AppState>().worker.restore_focus(target)
        })
        .await;
    }

    Ok(())
}

#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), ApiErrorDto> {
    clipboard::write_plain_unicode_text(&text)
        .map_err(|err| ApiErrorDto::new("clipboard_busy", Some(err.to_string())))
}

#[tauri::command]
pub async fn open_in_editor(
    app: AppHandle,
    state: State<'_, AppState>,
    text: Option<String>,
) -> Result<(), ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup_and_wait(&app).await;

    let source_app = state
        .active_capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .map(|c| c.target.process_name);

    let draft = text.map(|t| PendingEditorDraft {
        text: t,
        source_app,
    });
    *state
        .pending_editor_draft
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = draft.clone();

    windows_mgr::open_or_focus_main(&app)
        .map_err(|err| ApiErrorDto::new("window_error", Some(err.to_string())))?;
    let _ = app.emit("main:navigate", "write");

    if let Some(payload) = draft {
        let _ = app.emit("editor:load-draft", payload);
    }

    Ok(())
}

#[tauri::command]
pub async fn open_settings(app: AppHandle, state: State<'_, AppState>) -> Result<(), ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup_and_wait(&app).await;
    windows_mgr::open_or_focus_main(&app)
        .map_err(|err| ApiErrorDto::new("window_error", Some(err.to_string())))?;
    let _ = app.emit("main:navigate", "settings");
    Ok(())
}

#[tauri::command]
pub fn take_editor_draft(state: State<'_, AppState>) -> Option<PendingEditorDraft> {
    state
        .pending_editor_draft
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}
