use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::ai::provider::{
    AiError, ChatRequest, Completion, Provider, ReasoningMode, map_http_error,
};
use crate::ai::sse::SseParser;
use crate::dto::KeyTestResultDto;

pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// OpenAI-compatible streaming client configured by URL, headers, and extra body fields.
/// Configured for OpenRouter in Phase 1; covers OpenAI, Groq, Gemini, LM Studio and Ollama later.
#[derive(Clone)]
pub struct ChatClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    extra_headers: Vec<(String, String)>,
    extra_body: Map<String, Value>,
}

impl ChatClient {
    pub fn openrouter(api_key: String, deny_data_collection: bool) -> Result<Self, AiError> {
        let client = build_http_client()?;
        let mut extra_body = Map::new();
        if deny_data_collection {
            extra_body.insert("provider".to_string(), json!({ "data_collection": "deny" }));
        }

        Ok(Self {
            client,
            base_url: OPENROUTER_BASE_URL.to_string(),
            api_key,
            extra_headers: vec![("X-OpenRouter-Title".to_string(), "Redline".to_string())],
            extra_body,
        })
    }

    /// Builds the OpenAI-compatible streaming JSON payload.
    /// Deliberately omits `temperature` (unsupported by Haiku 5.5 and GPT-6 Luna).
    pub fn build_request_body(&self, req: &ChatRequest) -> Value {
        let mut body = Map::new();
        body.insert("model".to_string(), json!(req.model));
        body.insert(
            "messages".to_string(),
            json!([
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user }
            ]),
        );
        body.insert("stream".to_string(), json!(true));
        body.insert("max_tokens".to_string(), json!(req.max_tokens));

        match &req.reasoning {
            ReasoningMode::Omit => {}
            ReasoningMode::EffortNone => {
                body.insert("reasoning".to_string(), json!({ "effort": "none" }));
            }
            ReasoningMode::Disabled => {
                body.insert("reasoning".to_string(), json!({ "enabled": false }));
            }
            ReasoningMode::LowestEffort(effort) => {
                body.insert("reasoning".to_string(), json!({ "effort": effort }));
            }
        }

        for (k, v) in &self.extra_body {
            body.insert(k.clone(), v.clone());
        }

        Value::Object(body)
    }
}

fn build_http_client() -> Result<reqwest::Client, AiError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(DATA_IDLE_TIMEOUT)
        .build()
        .map_err(|err| AiError::TimeoutOrNetwork {
            message: Some(err.to_string()),
        })
}

impl Provider for ChatClient {
    async fn stream(
        &self,
        req: ChatRequest,
        mut on_delta: impl FnMut(&str) + Send,
    ) -> Result<Completion, AiError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = self.build_request_body(&req);

        let mut builder = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body);

        for (name, value) in &self.extra_headers {
            builder = builder.header(name, value);
        }

        let mut response = builder
            .send()
            .await
            .map_err(|err| AiError::TimeoutOrNetwork {
                message: Some(err.to_string()),
            })?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned);
            let error_body = response.text().await.unwrap_or_default();
            return Err(map_http_error(
                status.as_u16(),
                &error_body,
                retry_after.as_deref(),
            ));
        }

        let mut parser = SseParser::new(req.model);
        let mut last_data_at = Instant::now();

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|err| AiError::TimeoutOrNetwork {
                message: Some(err.to_string()),
            })?
        {
            let feed_status = parser.feed(&chunk, &mut on_delta)?;
            if feed_status.saw_data {
                last_data_at = Instant::now();
            } else if last_data_at.elapsed() > DATA_IDLE_TIMEOUT {
                return Err(AiError::TimeoutOrNetwork {
                    message: Some("Timed out waiting for model tokens".to_string()),
                });
            }

            if feed_status.done {
                break;
            }
        }

        let completion = parser.finish(&mut on_delta)?;
        if !completion.complete {
            return Err(AiError::CutOff);
        }
        if completion.text.trim().is_empty() {
            return Err(AiError::EmptyResult);
        }

        Ok(completion)
    }
}

#[derive(Debug, Deserialize)]
struct OpenRouterKeyEnvelope {
    data: OpenRouterKeyData,
}

#[derive(Debug, Deserialize)]
struct OpenRouterKeyData {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    limit_remaining: Option<f64>,
}

/// Verifies an OpenRouter API key via `GET /api/v1/key`.
/// Reports the key's label and `limit_remaining` (`None` = no limit on this key).
pub async fn test_openrouter_key(api_key: &str) -> Result<KeyTestResultDto, AiError> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err(AiError::NoKey);
    }

    let client = build_http_client()?;
    let url = format!("{OPENROUTER_BASE_URL}/key");
    let response = client
        .get(&url)
        .bearer_auth(trimmed)
        .header("X-OpenRouter-Title", "Redline")
        .send()
        .await
        .map_err(|err| AiError::TimeoutOrNetwork {
            message: Some(err.to_string()),
        })?;

    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
        let body = response.text().await.unwrap_or_default();
        return Err(map_http_error(
            status.as_u16(),
            &body,
            retry_after.as_deref(),
        ));
    }

    let parsed: OpenRouterKeyEnvelope =
        response
            .json()
            .await
            .map_err(|err| AiError::TimeoutOrNetwork {
                message: Some(err.to_string()),
            })?;

    Ok(KeyTestResultDto {
        label: parsed.data.label,
        limit_remaining: parsed.data.limit_remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::models::{CURATED_MODELS, reasoning_mode_for_model};
    use crate::ai::prompts::{Action, build_prompt, postprocess_reply};

    #[test]
    fn request_builder_sets_reasoning_privacy_and_omits_temperature() {
        let client = ChatClient::openrouter("test-key".to_string(), true).expect("client");

        for (model, expected_reasoning) in [
            (
                "anthropic/claude-haiku-5.5",
                Some(json!({ "effort": "none" })),
            ),
            ("openai/gpt-6-luna", Some(json!({ "effort": "none" }))),
            (
                "deepseek/deepseek-v4.1-flash",
                Some(json!({ "effort": "none" })),
            ),
            (
                "google/gemini-2.5-flash-lite",
                Some(json!({ "enabled": false })),
            ),
            ("qwen/qwen3.7-flash", Some(json!({ "enabled": false }))),
        ] {
            let req = ChatRequest {
                model: model.to_string(),
                system: "sys".to_string(),
                user: "usr".to_string(),
                max_tokens: 1024,
                reasoning: reasoning_mode_for_model(model, None),
            };
            let body = client.build_request_body(&req);
            assert!(body.get("temperature").is_none());
            assert_eq!(body.get("stream"), Some(&json!(true)));
            assert_eq!(body.get("max_tokens"), Some(&json!(1024)));
            assert_eq!(body.get("reasoning"), expected_reasoning.as_ref());
            assert_eq!(
                body.get("provider"),
                Some(&json!({ "data_collection": "deny" }))
            );
        }
    }

    #[test]
    fn request_builder_handles_omit_lowest_effort_and_privacy_off() {
        let client = ChatClient::openrouter("test-key".to_string(), false).expect("client");

        let omit_req = ChatRequest {
            model: "custom/model".to_string(),
            system: "sys".to_string(),
            user: "usr".to_string(),
            max_tokens: 2048,
            reasoning: ReasoningMode::Omit,
        };
        let omit_body = client.build_request_body(&omit_req);
        assert!(omit_body.get("reasoning").is_none());
        assert!(omit_body.get("provider").is_none());

        let mandatory_req = ChatRequest {
            model: "custom/reasoning".to_string(),
            system: "sys".to_string(),
            user: "usr".to_string(),
            max_tokens: 2048,
            reasoning: ReasoningMode::LowestEffort("minimal".to_string()),
        };
        let mandatory_body = client.build_request_body(&mandatory_req);
        assert_eq!(
            mandatory_body.get("reasoning"),
            Some(&json!({ "effort": "minimal" }))
        );
    }

    /// Opt-in live check (not run in CI):
    /// `OPENROUTER_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture`
    /// Confirms every curated model streams, finishes with `stop`, and emits `reasoning_tokens == 0`.
    #[test]
    #[ignore = "requires OPENROUTER_API_KEY and network access"]
    fn live_curated_models_check() {
        let api_key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY must be set");
        let client = ChatClient::openrouter(api_key, true).expect("openrouter client");
        let built = build_prompt(
            &Action::FixGrammar,
            "Teh quick brown fox jumpd over the dog.",
        );

        tauri::async_runtime::block_on(async {
            for model in CURATED_MODELS {
                let req = ChatRequest {
                    model: model.id.to_string(),
                    system: built.system.clone(),
                    user: built.user.clone(),
                    max_tokens: built.max_tokens,
                    reasoning: model.reasoning.to_mode(),
                };
                let mut chunks = 0usize;
                let completion = client
                    .stream(req, |_| {
                        chunks += 1;
                    })
                    .await
                    .unwrap_or_else(|e| panic!("model {} failed: {e:?}", model.id));

                let cleaned = postprocess_reply(&completion.text, &built);
                println!(
                    "[OK] {} -> finish={} chunks={} reasoning_tokens={:?} cost={:?} out={:?}",
                    model.id,
                    completion.finish_reason,
                    chunks,
                    completion.reasoning_tokens,
                    completion.cost_usd,
                    cleaned
                );

                assert!(completion.complete, "model {} incomplete", model.id);
                assert_eq!(
                    completion.finish_reason, "stop",
                    "model {} finish_reason != stop",
                    model.id
                );
                assert_eq!(
                    completion.reasoning_tokens.unwrap_or(0),
                    0,
                    "model {} used reasoning tokens ({:?}); reasoning-off config needs adjustment",
                    model.id,
                    completion.reasoning_tokens
                );
                assert!(chunks > 0, "model {} produced zero deltas", model.id);
                assert!(
                    !cleaned.trim().is_empty(),
                    "model {} returned empty text",
                    model.id
                );
            }
        });
    }
}
