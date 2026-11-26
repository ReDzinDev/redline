use serde::Deserialize;

use crate::ai::provider::{AiError, Completion};

#[derive(Debug, Clone, Copy, Default)]
pub struct SseFeedStatus {
    /// True if at least one `data:` event was processed in this chunk (excludes `:` keep-alives).
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
/// network chunks decode losslessly.
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
        Self {
            buf: Vec::new(),
            text: String::new(),
            model_used: requested_model.into(),
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
        self.buf.extend_from_slice(chunk);
        let mut status = SseFeedStatus::default();

        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line_bytes: Vec<u8> = self.buf.drain(..=pos).collect();
            // Drop trailing '\n' and optional '\r'.
            line_bytes.pop();
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }

            let line = String::from_utf8_lossy(&line_bytes);
            self.process_line(&line, &mut status, on_delta)?;
            if status.done {
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

        status.saw_data = true;

        if payload == "[DONE]" {
            self.saw_done = true;
            status.done = true;
            return Ok(());
        }

        let parsed: StreamChunk = serde_json::from_str(payload).map_err(|err| {
            log::warn!("malformed SSE JSON chunk: {err}");
            AiError::MidStream {
                message: Some("Malformed response from provider".to_string()),
            }
        })?;

        if let Some(err) = parsed.error {
            return Err(AiError::MidStream {
                message: err.message,
            });
        }

        if let Some(model) = parsed.model
            && !model.is_empty()
        {
            self.model_used = model;
        }

        if let Some(usage) = parsed.usage {
            if let Some(cost) = usage.cost {
                self.cost_usd = Some(cost);
            }
            if let Some(details) = usage.completion_tokens_details
                && let Some(rt) = details.reasoning_tokens
            {
                self.reasoning_tokens = Some(rt);
            }
        }

        if let Some(choice) = parsed.choices.into_iter().next() {
            if let Some(reason) = choice.finish_reason
                && !reason.is_empty()
            {
                self.finish_reason = Some(reason);
            }
            if let Some(delta) = choice.delta
                && let Some(content) = delta.content
                && !content.is_empty()
            {
                self.text.push_str(&content);
                on_delta(&content);
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
    fn keep_alive_alone_does_not_mark_saw_data() {
        let mut parser = SseParser::new("test");
        let status = parser
            .feed(b": OPENROUTER PROCESSING\n\n", &mut |_| {})
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
}
