use serde::{Deserialize, Serialize};

use crate::ai::prompts::{Action, Tone};
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
    pub hotkey_paused: bool,
    pub settings_recovered: bool,
    pub history_recovered: bool,
    pub welcome_completed: bool,
    pub autostart_enabled: bool,
    pub app_version: String,
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
    pub recent_custom_instructions: Vec<String>,
    pub welcome_completed: bool,
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
            recent_custom_instructions: s.recent_custom_instructions.clone(),
            welcome_completed: s.welcome_completed,
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

/// Events emitted from Rust to the inline popup window (`popup:session`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum PopupSessionEvent {
    #[serde(rename_all = "camelCase")]
    Capturing { session_id: u64, source_app: String },
    #[serde(rename_all = "camelCase")]
    Ready {
        session_id: u64,
        source_app: String,
        text: String,
        recent_custom_instructions: Vec<String>,
        auto_run_action: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        session_id: u64,
        source_app: String,
        error_code: String,
    },
}

/// Action invocation payload sent from the UI to `rewrite_text`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequestDto {
    pub kind: String,
    pub tone: Option<String>,
    pub custom_instruction: Option<String>,
}

impl ActionRequestDto {
    pub fn into_action(self) -> Result<Action, ApiErrorDto> {
        match self.kind.as_str() {
            "improve" => Ok(Action::Improve),
            "fix_grammar" => Ok(Action::FixGrammar),
            "shorten" => Ok(Action::Shorten),
            "make_clearer" => Ok(Action::MakeClearer),
            "change_tone" => {
                let tone = match self.tone.as_deref() {
                    Some("professional") => Tone::Professional,
                    Some("friendly") => Tone::Friendly,
                    Some("casual") => Tone::Casual,
                    Some("confident") => Tone::Confident,
                    _ => {
                        return Err(ApiErrorDto::new(
                            "invalid_action",
                            Some("Invalid tone".to_string()),
                        ));
                    }
                };
                Ok(Action::ChangeTone(tone))
            }
            "custom" => {
                let instruction = self
                    .custom_instruction
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        ApiErrorDto::new(
                            "invalid_action",
                            Some("Custom instruction cannot be empty".to_string()),
                        )
                    })?;
                Ok(Action::Custom(instruction.to_string()))
            }
            _ => Err(ApiErrorDto::new("invalid_action", None)),
        }
    }
}

/// Streaming events sent over a Tauri `Channel` during `rewrite_text`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum RewriteStreamEvent {
    #[serde(rename_all = "camelCase")]
    Delta { request_id: u64, text: String },
    #[serde(rename_all = "camelCase")]
    Complete {
        request_id: u64,
        text: String,
        model_used: String,
        cost_usd: Option<f64>,
        elapsed_ms: u64,
        unchanged: bool,
    },
    #[serde(rename_all = "camelCase")]
    CutOff {
        request_id: u64,
        partial_text: String,
        detail: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Error { request_id: u64, error: ApiErrorDto },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptHistoryMetaDto {
    pub action: String,
    pub model: String,
    pub cost_usd: Option<f64>,
    pub original: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptResultDto {
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEditorDraft {
    pub text: String,
    pub source_app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionDto {
    pub label: String,
    pub replacement: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LintDto {
    pub id: String,
    /// UTF-16 start offset into the LF-normalized text.
    pub start: usize,
    /// UTF-16 end offset into the LF-normalized text.
    pub end: usize,
    pub flagged: String,
    /// `"spelling" | "grammar" | "style"` for distinct underline styles.
    pub category: String,
    pub kind: String,
    pub message: String,
    pub suggestions: Vec<SuggestionDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LintResponseDto {
    pub request_id: u64,
    pub english: bool,
    pub lints: Vec<LintDto>,
}
