use acp_transport::protocol as acp;
use sampling_types::{ReasoningEffort, ReasoningEffortOption};
use serde::Serialize;

use crate::agent::models::{ModelId, ModelInfo};
use crate::session::unified_list::SessionKind;

pub const MODEL_CONFIG_ID: &str = "model";
pub const REASONING_EFFORT_CONFIG_ID: &str = "reasoning_effort";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowSessionDetail {
    pub session_id: String,
    pub kind: String,
    pub cwd: String,
    pub current_model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl GrowSessionDetail {
    pub fn build(
        session_id: String,
        cwd: String,
        current_model_id: String,
        title: Option<String>,
    ) -> Self {
        Self {
            session_id,
            kind: SessionKind::Build.as_str().to_string(),
            cwd,
            current_model_id,
            title,
        }
    }
}

pub(crate) fn build_session_config_options(
    available_models: &[ModelInfo],
    current_model_id: &ModelId,
    effort_options: &[ReasoningEffortOption],
    current_effort: Option<ReasoningEffort>,
) -> Vec<acp::SessionConfigOption> {
    let models = available_models
        .iter()
        .map(|model| {
            let label = if model.name.is_empty() {
                model.model_id.0.to_string()
            } else {
                model.name.clone()
            };
            acp::SessionConfigSelectOption::new(model.model_id.0.clone(), label)
                .description(model.description.clone())
                .meta(model.meta.clone())
        })
        .collect::<Vec<_>>();
    let mut options = vec![
        acp::SessionConfigOption::select(
            MODEL_CONFIG_ID,
            "Model",
            current_model_id.0.clone(),
            models,
        )
        .category(acp::SessionConfigOptionCategory::Model),
    ];

    if let Some(current_effort) = current_effort {
        let efforts = effort_options
            .iter()
            .map(|effort| {
                acp::SessionConfigSelectOption::new(effort.id.clone(), effort.label.clone())
                    .description(effort.description.clone())
            })
            .collect::<Vec<_>>();
        if !efforts.is_empty() {
            options.push(
                acp::SessionConfigOption::select(
                    REASONING_EFFORT_CONFIG_ID,
                    "Reasoning effort",
                    current_effort.as_str(),
                    efforts,
                )
                .category(acp::SessionConfigOptionCategory::ThoughtLevel),
            );
        }
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &'static str, name: &str) -> ModelInfo {
        ModelInfo::new(ModelId::new(id), name.to_string())
    }

    fn efforts(values: &[ReasoningEffort]) -> Vec<ReasoningEffortOption> {
        values
            .iter()
            .copied()
            .map(|value| {
                serde_json::from_value(serde_json::json!(value.as_str()))
                    .expect("canonical effort option")
            })
            .collect()
    }

    #[test]
    fn builds_stable_model_and_reasoning_selectors() {
        let models = [model("grow-build", "Grow"), model("grow-4.5", "Grow 4.5")];
        let current = ModelId::from("grow-build");
        let menu = efforts(&[ReasoningEffort::Low, ReasoningEffort::High]);
        let options =
            build_session_config_options(&models, &current, &menu, Some(ReasoningEffort::High));

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].id.0.as_ref(), MODEL_CONFIG_ID);
        assert_eq!(options[1].id.0.as_ref(), REASONING_EFFORT_CONFIG_ID);
        let acp::SessionConfigKind::Select(model_select) = &options[0].kind else {
            panic!("model config must be a select")
        };
        assert_eq!(model_select.current_value.0.as_ref(), "grow-build");
        let acp::SessionConfigKind::Select(effort_select) = &options[1].kind else {
            panic!("effort config must be a select")
        };
        assert_eq!(effort_select.current_value.0.as_ref(), "high");
    }

    #[test]
    fn omits_reasoning_selector_without_a_current_effort() {
        let models = [model("grow-build", "Grow")];
        let current = ModelId::from("grow-build");
        let menu = efforts(&[ReasoningEffort::Low, ReasoningEffort::High]);
        let options = build_session_config_options(&models, &current, &menu, None);
        assert_eq!(options.len(), 1);
    }

    #[test]
    fn model_label_falls_back_to_id() {
        let models = [model("grow-build", "")];
        let current = ModelId::from("grow-build");
        let options = build_session_config_options(&models, &current, &[], None);
        let value = serde_json::to_value(&options[0]).expect("serialize");
        assert_eq!(value["options"][0]["name"], "grow-build");
    }

    #[test]
    fn grow_session_detail_serializes_camel_case() {
        let detail = GrowSessionDetail::build(
            "sess-1".to_string(),
            "/Users/me/grow".to_string(),
            "grow-build".to_string(),
            None,
        );
        let value = serde_json::to_value(&detail).expect("serialize");
        assert_eq!(value["sessionId"], "sess-1");
        assert_eq!(value["kind"], "build");
        assert_eq!(value["cwd"], "/Users/me/grow");
        assert_eq!(value["currentModelId"], "grow-build");
        assert!(value.get("title").is_none());
    }
}
