use tauri::State;

use crate::AppState;
use crate::ai::chat_client::test_openrouter_key;
use crate::ai::models::curated_model_dtos;
use crate::dto::{ApiErrorDto, AppStatusDto, KeyTestResultDto, ModelOptionDto, SettingsDto};
use crate::secrets;
use crate::settings;

#[tauri::command]
pub fn get_app_status(state: State<'_, AppState>) -> Result<AppStatusDto, ApiErrorDto> {
    let has_api_key = secrets::has_api_key()
        .map_err(|err| ApiErrorDto::new("secret_store_error", Some(err.to_string())))?;
    let hotkey = state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .hotkey
        .clone();
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
