use std::time::Instant;

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;
use crate::ai::chat_client::{ChatClient, test_openrouter_key};
use crate::ai::models::{curated_model_dtos, reasoning_mode_for_model};
use crate::ai::prompts::{Action, build_prompt, postprocess_reply_lf, restore_line_endings};
use crate::ai::provider::{AiError, ChatRequest, Provider};
use crate::app_info::{EDITOR_MAX_INPUT_CHARS, POPUP_MAX_INPUT_CHARS, WINDOW_POPUP};
use crate::capture::PasteOutcome;
use crate::dto::{
    AcceptResultDto, ActionRequestDto, ApiErrorDto, AppStatusDto, KeyTestResultDto,
    LintResponseDto, ModelOptionDto, PendingEditorDraft, RewriteStreamEvent, SettingsDto,
};
use crate::platform::clipboard;
use crate::{secrets, settings, windows_mgr};

#[tauri::command]
pub fn get_app_status(state: State<'_, AppState>) -> Result<AppStatusDto, ApiErrorDto> {
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

    Ok(AppStatusDto {
        has_api_key,
        hotkey,
        hotkey_conflict,
        settings_recovered: state.settings_recovered,
        welcome_completed,
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
) -> Result<AcceptResultDto, ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup(&app);

    let active = state
        .active_capture
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();

    let Some(capture) = active else {
        clipboard::write_plain_unicode_text(&text)
            .map_err(|err| ApiErrorDto::new("clipboard_busy", Some(err.to_string())))?;
        return Ok(AcceptResultDto {
            outcome: "copied_fallback".to_string(),
        });
    };

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

    let outcome = match res {
        PasteOutcome::Pasted { .. } => "pasted",
        PasteOutcome::CopiedFallback => {
            // Re-show the popup briefly so the user sees the "Copied: paste it with Ctrl+V." notice.
            if let Some(popup) = app.get_webview_window(WINDOW_POPUP) {
                windows_mgr::set_popup_low_memory(&popup, false);
                let _ = popup.show();
                let _ = popup.set_focus();
            }
            "copied_fallback"
        }
    };

    Ok(AcceptResultDto {
        outcome: outcome.to_string(),
    })
}

#[tauri::command]
pub async fn dismiss_popup(
    app: AppHandle,
    state: State<'_, AppState>,
    restore_focus: bool,
) -> Result<(), ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup(&app);

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
    windows_mgr::hide_popup(&app);

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

    if let Some(payload) = draft {
        let _ = app.emit("editor:load-draft", payload);
    }

    Ok(())
}

#[tauri::command]
pub async fn open_settings(app: AppHandle, state: State<'_, AppState>) -> Result<(), ApiErrorDto> {
    crate::cancel_inflight(&state, WINDOW_POPUP);
    windows_mgr::hide_popup(&app);
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
