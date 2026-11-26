use crate::ai::provider::ReasoningMode;
use crate::dto::ModelOptionDto;

pub struct CuratedModel {
    pub id: &'static str,
    pub label: &'static str,
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub note: &'static str,
    pub reasoning: ReasoningModeConst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningModeConst {
    EffortNone,
    Disabled,
}

impl ReasoningModeConst {
    pub fn to_mode(self) -> ReasoningMode {
        match self {
            Self::EffortNone => ReasoningMode::EffortNone,
            Self::Disabled => ReasoningMode::Disabled,
        }
    }
}

/// Curated OpenRouter model list — single constant so swapping a model is a one-line change.
pub const CURATED_MODELS: &[CuratedModel] = &[
    CuratedModel {
        id: "anthropic/claude-haiku-5.5",
        label: "Claude Haiku 5.5",
        input_cost_per_million: 0.10,
        output_cost_per_million: 0.50,
        note: "Default — fast, strong at minimal edits",
        reasoning: ReasoningModeConst::EffortNone,
    },
    CuratedModel {
        id: "openai/gpt-6-luna",
        label: "GPT-6 Luna",
        input_cost_per_million: 0.10,
        output_cost_per_million: 0.50,
        note: "Fast GPT-6 tier (pinned)",
        reasoning: ReasoningModeConst::EffortNone,
    },
    CuratedModel {
        id: "google/gemini-2.5-flash-lite",
        label: "Gemini 2.5 Flash-Lite",
        input_cost_per_million: 0.10,
        output_cost_per_million: 0.40,
        note: "Budget Gemini tier",
        reasoning: ReasoningModeConst::Disabled,
    },
    CuratedModel {
        id: "deepseek/deepseek-v4.1-flash",
        label: "DeepSeek V4.1 Flash",
        input_cost_per_million: 0.14,
        output_cost_per_million: 0.42,
        note: "Fast DeepSeek Flash tier",
        reasoning: ReasoningModeConst::EffortNone,
    },
    CuratedModel {
        id: "qwen/qwen3.7-flash",
        label: "Qwen 3.7 Flash",
        input_cost_per_million: 0.03,
        output_cost_per_million: 0.13,
        note: "Cheapest overall",
        reasoning: ReasoningModeConst::Disabled,
    },
];

pub fn curated_model_dtos() -> Vec<ModelOptionDto> {
    CURATED_MODELS
        .iter()
        .map(|m| ModelOptionDto {
            id: m.id.to_string(),
            label: m.label.to_string(),
            input_cost_per_million: m.input_cost_per_million,
            output_cost_per_million: m.output_cost_per_million,
            note: m.note.to_string(),
            reasoning: m.reasoning.to_mode(),
        })
        .collect()
}

pub fn reasoning_mode_for_model(
    model_id: &str,
    custom_reasoning: Option<&ReasoningMode>,
) -> ReasoningMode {
    if let Some(curated) = CURATED_MODELS.iter().find(|m| m.id == model_id) {
        return curated.reasoning.to_mode();
    }
    custom_reasoning.cloned().unwrap_or(ReasoningMode::Omit)
}

const EFFORT_RANK: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

/// Derives `ReasoningMode` from a model's OpenRouter catalog `reasoning` object (rules 0–3 in §3).
#[allow(dead_code)]
pub fn derive_reasoning_mode_from_catalog(reasoning: Option<&serde_json::Value>) -> ReasoningMode {
    let Some(obj) = reasoning.and_then(serde_json::Value::as_object) else {
        return ReasoningMode::Omit;
    };

    let efforts: Vec<&str> = obj
        .get("supported_efforts")
        .and_then(serde_json::Value::as_array)
        .map(|arr| arr.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();

    if efforts.contains(&"none") {
        return ReasoningMode::EffortNone;
    }

    let mandatory = obj
        .get("mandatory")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !mandatory {
        return ReasoningMode::Disabled;
    }

    for candidate in EFFORT_RANK {
        if efforts.contains(candidate) {
            return ReasoningMode::LowestEffort((*candidate).to_string());
        }
    }

    let fallback = efforts.first().copied().unwrap_or("low");
    ReasoningMode::LowestEffort(fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn curated_models_return_expected_reasoning_modes() {
        assert_eq!(
            reasoning_mode_for_model("anthropic/claude-haiku-5.5", None),
            ReasoningMode::EffortNone
        );
        assert_eq!(
            reasoning_mode_for_model("openai/gpt-6-luna", None),
            ReasoningMode::EffortNone
        );
        assert_eq!(
            reasoning_mode_for_model("deepseek/deepseek-v4.1-flash", None),
            ReasoningMode::EffortNone
        );
        assert_eq!(
            reasoning_mode_for_model("google/gemini-2.5-flash-lite", None),
            ReasoningMode::Disabled
        );
        assert_eq!(
            reasoning_mode_for_model("qwen/qwen3.7-flash", None),
            ReasoningMode::Disabled
        );
    }

    #[test]
    fn derives_reasoning_mode_from_catalog_rules() {
        assert_eq!(
            derive_reasoning_mode_from_catalog(None),
            ReasoningMode::Omit
        );
        assert_eq!(
            derive_reasoning_mode_from_catalog(Some(&json!(null))),
            ReasoningMode::Omit
        );
        assert_eq!(
            derive_reasoning_mode_from_catalog(Some(&json!({
                "mandatory": false,
                "supported_efforts": ["none", "low", "medium", "high"]
            }))),
            ReasoningMode::EffortNone
        );
        assert_eq!(
            derive_reasoning_mode_from_catalog(Some(&json!({
                "mandatory": false,
                "supported_efforts": ["low", "medium", "high"]
            }))),
            ReasoningMode::Disabled
        );
        assert_eq!(
            derive_reasoning_mode_from_catalog(Some(&json!({
                "mandatory": true,
                "supported_efforts": ["high", "medium", "minimal"]
            }))),
            ReasoningMode::LowestEffort("minimal".to_string())
        );
    }
}
