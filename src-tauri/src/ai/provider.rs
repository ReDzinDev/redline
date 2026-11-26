use std::future::Future;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Controls how reasoning is disabled (or minimized when mandatory) in OpenRouter requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "effort", rename_all = "snake_case")]
pub enum ReasoningMode {
    /// Model has no `reasoning` catalog entry — omit the field completely.
    Omit,
    /// `supported_efforts` includes `"none"` — send `{ "effort": "none" }`.
    EffortNone,
    /// Reasoning is optional and not mandatory — send `{ "enabled": false }`.
    Disabled,
    /// Reasoning is mandatory — send `{ "effort": "<lowest>" }`.
    LowestEffort(String),
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub user: String,
    pub max_tokens: u32,
    pub reasoning: ReasoningMode,
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    pub model_used: String,
    pub cost_usd: Option<f64>,
    pub finish_reason: String,
    /// True only when the SSE stream ended with `[DONE]` AND `finish_reason == "stop"`.
    pub complete: bool,
    pub reasoning_tokens: Option<u64>,
}

/// Single streaming provider trait; returns an explicitly `Send` future so it can be spawned
/// on the Tauri async runtime without `dyn` or boxing.
pub trait Provider {
    fn stream(
        &self,
        req: ChatRequest,
        on_delta: impl FnMut(&str) + Send,
    ) -> impl Future<Output = Result<Completion, AiError>> + Send;
}

#[derive(Debug, Error)]
pub enum AiError {
    #[error("no OpenRouter API key configured")]
    NoKey,
    #[error("bad request (400)")]
    BadRequest { message: Option<String> },
    #[error("unauthorized (401)")]
    Unauthorized { message: Option<String> },
    #[error("out of credits or key limit reached (402)")]
    PaymentRequired { message: Option<String> },
    #[error("request blocked (403)")]
    Forbidden { message: Option<String> },
    #[error("no matching provider or unavailable (404/503)")]
    NoProvider { message: Option<String> },
    #[error("rate limited (429)")]
    RateLimited {
        retry_after_secs: Option<u64>,
        message: Option<String>,
    },
    #[error("model unavailable (502)")]
    BadGateway { message: Option<String> },
    #[error("network error or request timed out")]
    TimeoutOrNetwork { message: Option<String> },
    #[error("stream interrupted by provider error")]
    MidStream { message: Option<String> },
    #[error("result was cut off before completion")]
    CutOff,
    #[error("model returned an empty result")]
    EmptyResult,
    #[error("unexpected provider error (HTTP {status})")]
    Unexpected {
        status: u16,
        message: Option<String>,
    },
}

impl AiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoKey => "ai_no_key",
            Self::BadRequest { .. } => "ai_bad_request",
            Self::Unauthorized { .. } => "ai_unauthorized",
            Self::PaymentRequired { .. } => "ai_payment_required",
            Self::Forbidden { .. } => "ai_forbidden",
            Self::NoProvider { .. } => "ai_no_provider",
            Self::RateLimited { .. } => "ai_rate_limited",
            Self::BadGateway { .. } => "ai_bad_gateway",
            Self::TimeoutOrNetwork { .. } => "ai_network",
            Self::MidStream { .. } => "ai_mid_stream",
            Self::CutOff => "ai_cut_off",
            Self::EmptyResult => "ai_empty",
            Self::Unexpected { .. } => "ai_unexpected",
        }
    }

    pub fn provider_message(&self) -> Option<&str> {
        match self {
            Self::BadRequest { message }
            | Self::Unauthorized { message }
            | Self::PaymentRequired { message }
            | Self::Forbidden { message }
            | Self::NoProvider { message }
            | Self::RateLimited { message, .. }
            | Self::BadGateway { message }
            | Self::TimeoutOrNetwork { message }
            | Self::MidStream { message }
            | Self::Unexpected { message, .. } => message.as_deref(),
            Self::NoKey | Self::CutOff | Self::EmptyResult => None,
        }
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            Self::RateLimited {
                retry_after_secs, ..
            } => *retry_after_secs,
            _ => None,
        }
    }
}

/// Extracts OpenRouter's human-readable error message from a JSON response body, if present.
pub fn extract_provider_message(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    let msg = parsed
        .get("error")
        .and_then(|e| e.get("message").and_then(serde_json::Value::as_str))
        .or_else(|| parsed.get("message").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(msg.to_string())
}

/// Maps an HTTP status code, response body, and optional `Retry-After` header into a typed `AiError`.
pub fn map_http_error(status: u16, body: &str, retry_after_header: Option<&str>) -> AiError {
    let message = extract_provider_message(body);
    match status {
        400 => AiError::BadRequest { message },
        401 => AiError::Unauthorized { message },
        402 => AiError::PaymentRequired { message },
        403 => AiError::Forbidden { message },
        404 | 503 => AiError::NoProvider { message },
        408 => AiError::TimeoutOrNetwork { message },
        429 => {
            let retry_after_secs = retry_after_header
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|&secs| secs > 0);
            AiError::RateLimited {
                retry_after_secs,
                message,
            }
        }
        502 => AiError::BadGateway { message },
        _ => AiError::Unexpected { status, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_standard_openrouter_error_statuses() {
        let err = map_http_error(400, r#"{"error":{"message":"Invalid model ID"}}"#, None);
        assert_eq!(err.code(), "ai_bad_request");
        assert_eq!(err.provider_message(), Some("Invalid model ID"));

        let err = map_http_error(401, r#"{"error":{"message":"No auth credentials"}}"#, None);
        assert_eq!(err.code(), "ai_unauthorized");

        let err = map_http_error(402, r#"{"error":{"message":"Insufficient credits"}}"#, None);
        assert_eq!(err.code(), "ai_payment_required");
        assert_eq!(err.provider_message(), Some("Insufficient credits"));

        let err = map_http_error(403, r#"{"error":{"message":"Moderation blocked"}}"#, None);
        assert_eq!(err.code(), "ai_forbidden");

        let err = map_http_error(404, r#"{"error":{"message":"No endpoints found"}}"#, None);
        assert_eq!(err.code(), "ai_no_provider");

        let err = map_http_error(503, "", None);
        assert_eq!(err.code(), "ai_no_provider");

        let err = map_http_error(408, "", None);
        assert_eq!(err.code(), "ai_network");

        let err = map_http_error(502, "", None);
        assert_eq!(err.code(), "ai_bad_gateway");
    }

    #[test]
    fn maps_429_with_and_without_retry_after() {
        let with_header = map_http_error(
            429,
            r#"{"error":{"message":"Too many requests"}}"#,
            Some("12"),
        );
        assert_eq!(with_header.code(), "ai_rate_limited");
        assert_eq!(with_header.retry_after_secs(), Some(12));
        assert_eq!(with_header.provider_message(), Some("Too many requests"));

        let without_header = map_http_error(429, "{}", None);
        assert_eq!(without_header.code(), "ai_rate_limited");
        assert_eq!(without_header.retry_after_secs(), None);

        let non_numeric = map_http_error(429, "{}", Some("Wed, 21 Oct 2026 07:28:00 GMT"));
        assert_eq!(non_numeric.retry_after_secs(), None);
    }
}
