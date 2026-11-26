use serde::{Deserialize, Serialize};

use crate::ai::provider::{AiError, ReasoningMode};
use crate::settings::Settings;

/// Current high-level app status returned to the UI.
/// Never contains the API key itself — only whether one is stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatusDto {
    pub has_api_key: bool,
    pub hotkey: String,
    pub hotkey_conflict: Option<String>,
    pub settings_recovered: bool,
}

/// Settings snapshot exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub hotkey: String,
    pub model: String,
    pub custom_model_id: Option<String>,
    pub deny_data_collection: bool,
    pub history_enabled: bool,
    pub auto_run_action: Option<String>,
    pub dialect: String,
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            hotkey: s.hotkey.clone(),
            model: s.model.clone(),
            custom_model_id: s.custom_model_id.clone(),
            deny_data_collection: s.deny_data_collection,
            history_enabled: s.history_enabled,
            auto_run_action: s.auto_run_action.clone(),
            dialect: s.dialect.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOptionDto {
    pub id: String,
    pub label: String,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub note: String,
    pub reasoning: ReasoningMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTestResultDto {
    pub label: Option<String>,
    /// `None` means no limit is set on this key (not account balance).
    pub limit_remaining: Option<f64>,
}

/// Structured error returned from Rust commands to the UI.
/// User-facing prose lives in `src/lib/messages.ts`; Rust supplies only the code and details.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDto {
    pub code: String,
    pub detail: Option<String>,
    pub retry_after_secs: Option<u64>,
}

impl ApiErrorDto {
    pub fn new(code: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            code: code.into(),
            detail,
            retry_after_secs: None,
        }
    }
}

impl From<AiError> for ApiErrorDto {
    fn from(err: AiError) -> Self {
        Self {
            code: err.code().to_string(),
            detail: err.provider_message().map(ToOwned::to_owned),
            retry_after_secs: err.retry_after_secs(),
        }
    }
}
