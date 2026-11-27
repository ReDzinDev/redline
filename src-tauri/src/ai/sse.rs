use serde::Deserialize;

use crate::ai::prompts::sanitize_output_text;
use crate::ai::provider::{AiError, Completion};
use crate::app_info::sanitize_log_value;

const MAX_SSE_LINE_BYTES: usize = 256 * 1024;
const MAX_COMPLETION_BYTES: usize = 512 * 1024;
const MAX_MODEL_ID_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, Default)]
pub struct SseFeedStatus {
    /// True if at least one meaningful `data:` event (content, finish_reason, usage, or `[DONE]`)
    /// was processed in this chunk (excludes `:` keep-alives and empty `data: {}` frames).
    pub saw_data: bool,
    /// True once `data: [DONE]` has been observed.
    pub done: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
    #[serde(default)]
    error: Option<StreamError>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    #[serde(default)]
    cost: Option<f64>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct StreamError {
    #[serde(default)]
    message: Option<String>,
}

/// Incremental SSE parser that buffers raw bytes so multi-byte UTF-8 characters split across
/// network chunks decode losslessly while enforcing strict per-line and total completion limits.
pub struct SseParser {
    buf: Vec<u8>,
    text: String,
    model_used: String,
    cost_usd: Option<f64>,
    finish_reason: Option<String>,
    reasoning_tokens: Option<u64>,
    saw_done: bool,
}

impl SseParser {
    pub fn new(requested_model: impl Into<String>) -> Self {
        let model = sanitize_log_value(&requested_model.into(), MAX_MODEL_ID_CHARS);
        Self {
            buf: Vec::new(),
            text: String::new(),
            model_used: model,
            cost_usd: None,
            finish_reason: None,
            reasoning_tokens: None,
            saw_done: false,
        }
    }

    pub fn feed(
        &mut self,
        chunk: &[u8],
        on_delta: &mut impl FnMut(&str),
    ) -> Result<SseFeedStatus, AiError> {
        let mut status = SseFeedStatus::default();
        let mut slice = chunk;

        while !slice.is_empty() {
            if let Some(pos) = slice.iter().position(|&b| b == b'\n') {
                let seg = &slice[..pos];
                if self.buf.len().saturating_add(seg.len()) > MAX_SSE_LINE_BYTES {
                    return Err(AiError::MidStream {
                        message: Some("Provider SSE line exceeded size limit".to_string()),
                    });
                }
                self.buf.extend_from_slice(seg);
                slice = &slice[pos + 1..];

                if self.buf.last() == Some(&b'\r') {
                    self.buf.pop();
                }
                let line_bytes = std::mem::take(&mut self.buf);
                let line = String::from_utf8_lossy(&line_bytes);
                self.process_line(&line, &mut status, on_delta)?;
                if status.done {
                    break;
                }
            } else {
                if self.buf.len().saturating_add(slice.len()) > MAX_SSE_LINE_BYTES {
                    return Err(AiError::MidStream {
                        message: Some("Provider SSE line exceeded size limit".to_string()),
                    });
                }
                self.buf.extend_from_slice(slice);
                break;
            }
        }

        Ok(status)
    }

    pub fn finish(mut self, on_delta: &mut impl FnMut(&str)) -> Result<Completion, AiError> {
        if !self.buf.is_empty() && !self.saw_done {
            let mut tail = std::mem::take(&mut self.buf);
            if tail.last() == Some(&b'\r') {
                tail.pop();
            }
            let line = String::from_utf8_lossy(&tail);
            let mut status = SseFeedStatus::default();
            self.process_line(&line, &mut status, on_delta)?;
        }

        let finish_reason = self.finish_reason.unwrap_or_default();
        let complete = self.saw_done && finish_reason == "stop";

        Ok(Completion {
            text: self.text,
            model_used: self.model_used,
            cost_usd: self.cost_usd,
            finish_reason,
            complete,
            reasoning_tokens: self.reasoning_tokens,
        })
    }

    fn process_line(
        &mut self,
        line: &str,
        status: &mut SseFeedStatus,
        on_delta: &mut impl FnMut(&str),
    ) -> Result<(), AiError> {
        if line.is_empty() || line.starts_with(':') {
            return Ok(());
        }

        let Some(rest) = line.strip_prefix("data:") else {
            return Ok(());
        };
        let payload = rest.strip_prefix(' ').unwrap_or(rest).trim();
        if payload.is_empty() {
            return Ok(());
        }

        if payload == "[DONE]" {
            self.saw_done = true;
            status.saw_data = true;
            status.done = true;
            return Ok(());
        }

        let parsed: StreamChunk = serde_json::from_str(payload).map_err(|err| {
            log::warn!(
                "malformed SSE JSON chunk: kind={:?} line={} col={}",
                err.classify(),
                err.line(),
                err.column()
            );
            AiError::MidStream {
                message: Some("Malformed response from provider".to_string()),
            }
        })?;

        if let Some(err) = parsed.error {
            status.saw_data = true;
            return Err(AiError::MidStream {
                message: err.message.map(|m| sanitize_log_value(&m, 256)),
            });
        }

        if let Some(model) = parsed.model {
            let clean_model = sanitize_log_value(&model, MAX_MODEL_ID_CHARS);
            if !clean_model.is_empty() {
                self.model_used = clean_model;
            }
        }

        if let Some(usage) = parsed.usage {
            if let Some(cost) = usage.cost {
                self.cost_usd = Some(cost);
                status.saw_data = true;
            }
            if let Some(details) = usage.completion_tokens_details
                && let Some(rt) = details.reasoning_tokens
            {
                self.reasoning_tokens = Some(rt);
                status.saw_data = true;
            }
        }

        if let Some(choice) = parsed.choices.into_iter().next() {
            if let Some(reason) = choice.finish_reason
                && !reason.is_empty()
            {
                self.finish_reason = Some(sanitize_log_value(&reason, 64));
                status.saw_data = true;
            }
            if let Some(delta) = choice.delta
                && let Some(content) = delta.content
                && !content.is_empty()
            {
                let sanitized = sanitize_output_text(&content);
                if !sanitized.is_empty() {
                    if self.text.len().saturating_add(sanitized.len()) > MAX_COMPLETION_BYTES {
                        return Err(AiError::MidStream {
                            message: Some("Provider response exceeded size limit".to_string()),
                        });
                    }
                    status.saw_data = true;
                    self.text.push_str(&sanitized);
                    on_delta(&sanitized);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_utf8_split_across_chunks_crlf_and_keepalives() {
        let mut parser = SseParser::new("anthropic/claude-haiku-5.5");
        let full = concat!(
            ": OPENROUTER PROCESSING\r\n",
            "\r\n",
            "data: {\"model\":\"anthropic/claude-haiku-5.5\",\"choices\":[{\"delta\":{\"content\":null,\"reasoning\":\"ignored\"}}]}\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Olá 🚀\"}}]}\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}]}\r\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"cost\":0.00002,\"completion_tokens_details\":{\"reasoning_tokens\":0}}}\r\n",
            "data: [DONE]\r\n"
        );
        let bytes = full.as_bytes();
        let mut streamed = String::new();

        // Feed 3 bytes at a time to guarantee multi-byte UTF-8 characters split mid-codepoint.
        for slice in bytes.chunks(3) {
            parser
                .feed(slice, &mut |delta| streamed.push_str(delta))
                .expect("feed ok");
        }

        let completion = parser
            .finish(&mut |delta| streamed.push_str(delta))
            .expect("finish ok");
        assert!(completion.complete);
        assert_eq!(completion.finish_reason, "stop");
        assert_eq!(completion.text, "Olá 🚀!");
        assert_eq!(streamed, "Olá 🚀!");
        assert_eq!(completion.cost_usd, Some(0.00002));
        assert_eq!(completion.reasoning_tokens, Some(0));
    }

    #[test]
    fn keep_alive_and_empty_data_frames_do_not_mark_saw_data() {
        let mut parser = SseParser::new("test");
        let status = parser
            .feed(
                b": OPENROUTER PROCESSING\n\ndata: {}\ndata: {\"choices\":[{\"delta\":{}}]}\n",
                &mut |_| {},
            )
            .expect("feed ok");
        assert!(!status.saw_data);
        assert!(!status.done);
    }

    #[test]
    fn mid_stream_error_returns_err() {
        let mut parser = SseParser::new("test");
        let chunk = b"data: {\"error\":{\"code\":502,\"message\":\"Provider disconnected\"}}\n";
        let err = parser
            .feed(chunk, &mut |_| {})
            .expect_err("expected mid-stream error");
        assert_eq!(err.code(), "ai_mid_stream");
        assert_eq!(err.provider_message(), Some("Provider disconnected"));
    }

    #[test]
    fn stream_ending_without_done_or_length_stop_is_incomplete() {
        let mut parser = SseParser::new("test");
        parser
            .feed(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Half\"},\"finish_reason\":\"stop\"}]}\n",
                &mut |_| {},
            )
            .expect("feed ok");
        let no_done = parser.finish(&mut |_| {}).expect("finish ok");
        assert!(!no_done.complete);

        let mut parser2 = SseParser::new("test");
        parser2
            .feed(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"Cut\"},\"finish_reason\":\"length\"}]}\ndata: [DONE]\n",
                &mut |_| {},
            )
            .expect("feed ok");
        let length_cut = parser2.finish(&mut |_| {}).expect("finish ok");
        assert!(!length_cut.complete);
        assert_eq!(length_cut.finish_reason, "length");
    }

    #[test]
    fn rejects_oversized_sse_line_and_oversized_completion() {
        let mut parser = SseParser::new("test");
        let huge_line = vec![b'a'; MAX_SSE_LINE_BYTES + 1];
        let err = parser
            .feed(&huge_line, &mut |_| {})
            .expect_err("oversized line should fail");
        assert_eq!(err.code(), "ai_mid_stream");

        let mut parser2 = SseParser::new("test");
        let piece = "x".repeat(60_000);
        let chunk = format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{piece}\"}}}}]}}\n");
        let mut hit_err = false;
        for _ in 0..12 {
            if parser2.feed(chunk.as_bytes(), &mut |_| {}).is_err() {
                hit_err = true;
                break;
            }
        }
        assert!(hit_err, "oversized total completion should fail");
    }

    #[test]
    fn sanitizes_remote_model_id_and_delta_controls() {
        let mut parser = SseParser::new("test");
        let payload = concat!(
            "data: {\"model\":\"evil/model\\r\\nINJECTED\\u001b[31m\",\"choices\":[{\"delta\":{\"content\":\"Hi\\u0000\\u202eWorld\"},\"finish_reason\":\"stop\"}]}\n",
            "data: [DONE]\n"
        );
        let mut streamed = String::new();
        parser
            .feed(payload.as_bytes(), &mut |d| streamed.push_str(d))
            .expect("feed ok");
        let completion = parser.finish(&mut |_| {}).expect("finish ok");
        assert!(!completion.model_used.contains('\r'));
        assert!(!completion.model_used.contains('\n'));
        assert!(!completion.model_used.contains('\x1b'));
        assert_eq!(completion.text, "HiWorld");
        assert_eq!(streamed, "HiWorld");
    }
}
