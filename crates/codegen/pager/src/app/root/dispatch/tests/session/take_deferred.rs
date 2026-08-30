use crate::acp::model_state::{EffortTokenError, ModelState};
use crate::app::root::dispatch::session::lifecycle::{
    DeferredSwitchOutcome, apply_deferred_switch_outcome, take_deferred_model_switch,
};
use crate::scrollback::block::RenderBlock;
use shell::sampling::types::ReasoningEffort;
use std::sync::Arc;

fn model_with_support(
    id: &str,
    supports: bool,
) -> (
    shell::agent::models::ModelId,
    shell::agent::models::ModelInfo,
) {
    let id = shell::agent::models::ModelId::new(Arc::from(id));
    let meta = if supports {
        Some(serde_json::json!({
            "reasoningEffort": "medium",
            "reasoningEfforts": [
                { "id": "deep", "value": "xhigh", "label": "Deep" },
                { "id": "high", "value": "high", "label": "High" },
            ],
        }))
    } else {
        Some(serde_json::json!({ "reasoningEffort": "medium" }))
    };
    let info = shell::agent::models::ModelInfo::new(id.clone(), id.0.to_string())
        .meta(meta.and_then(|v| v.as_object().cloned()));
    (id, info)
}

fn models_with_current(supports: bool) -> ModelState {
    let (id, info) = model_with_support("grow-build", supports);
    let mut models = ModelState::default();
    models.available.insert(id.clone(), info);
    models.current = Some(id);
    models.reasoning_effort = Some(ReasoningEffort::Medium);
    models
}

#[test]
fn effort_only_resolves_canonical_token() {
    let models = models_with_current(true);
    let out = take_deferred_model_switch(None, &models, Some("high"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((models.current.clone().unwrap(), Some(ReasoningEffort::High))),
            effort_error: None,
        }
    );
}

#[test]
fn effort_only_resolves_remapped_menu_id() {
    let models = models_with_current(true);
    let out = take_deferred_model_switch(None, &models, Some("deep"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((
                models.current.clone().unwrap(),
                Some(ReasoningEffort::Xhigh)
            )),
            effort_error: None,
        }
    );
}

#[test]
fn effort_only_unsupported_canonical_token_is_unsupported() {
    // Gate-first: a canonical token on a model that doesn't support reasoning
    // effort surfaces Unsupported (matching `/effort` and headless) rather than
    // silently applying an effort the server would drop.
    let models = models_with_current(false);
    assert_eq!(
        take_deferred_model_switch(None, &models, Some("high")),
        DeferredSwitchOutcome {
            switch: None,
            effort_error: Some(EffortTokenError::Unsupported),
        }
    );
}

#[test]
fn effort_only_unsupported_unknown_token_is_unsupported() {
    let models = models_with_current(false);
    assert_eq!(
        take_deferred_model_switch(None, &models, Some("bogus")),
        DeferredSwitchOutcome {
            switch: None,
            effort_error: Some(EffortTokenError::Unsupported),
        }
    );
}

#[test]
fn effort_only_skips_when_already_equal() {
    let mut models = models_with_current(true);
    models.reasoning_effort = Some(ReasoningEffort::High);
    assert_eq!(
        take_deferred_model_switch(None, &models, Some("high")),
        DeferredSwitchOutcome {
            switch: None,
            effort_error: None,
        }
    );
}

#[test]
fn effort_only_errors_on_unknown_token() {
    let models = models_with_current(true);
    assert_eq!(
        take_deferred_model_switch(None, &models, Some("bogus")),
        DeferredSwitchOutcome {
            switch: None,
            effort_error: Some(EffortTokenError::UnknownToken {
                token: "bogus".into(),
                offered: vec!["deep".into(), "high".into()],
            }),
        }
    );
}

#[test]
fn stashed_model_switch_prefers_explicit_stash() {
    let models = models_with_current(true);
    let other = shell::agent::models::ModelId::new(Arc::from("other-model"));
    let out = take_deferred_model_switch(
        Some((other.clone(), Some(ReasoningEffort::Low))),
        &models,
        Some("high"),
    );
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((other, Some(ReasoningEffort::Low))),
            effort_error: None,
        }
    );
}

#[test]
fn stashed_model_re_resolves_remap_when_effort_missing() {
    let models = models_with_current(true);
    let current = models.current.clone().unwrap();
    let out = take_deferred_model_switch(Some((current.clone(), None)), &models, Some("deep"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((current, Some(ReasoningEffort::Xhigh))),
            effort_error: None,
        }
    );
}

#[test]
fn stashed_model_keeps_model_when_token_unresolvable() {
    let models = models_with_current(true);
    let current = models.current.clone().unwrap();
    let out = take_deferred_model_switch(Some((current.clone(), None)), &models, Some("bogus"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((current, None)),
            effort_error: Some(EffortTokenError::UnknownToken {
                token: "bogus".into(),
                offered: vec!["deep".into(), "high".into()],
            }),
        }
    );
}

#[test]
fn stashed_model_keeps_model_when_unsupported() {
    // -m targets a non-reasoning model plus an effort token: keep the model
    // switch, drop the effort, and surface Unsupported.
    let mut models = models_with_current(true);
    let (plain, plain_info) = model_with_support("plain-model", false);
    models.available.insert(plain.clone(), plain_info);
    let out = take_deferred_model_switch(Some((plain.clone(), None)), &models, Some("high"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: Some((plain, None)),
            effort_error: Some(EffortTokenError::Unsupported),
        }
    );
}

#[test]
fn effort_only_rejects_max_when_model_does_not_offer_it() {
    let models = models_with_current(true);
    let out = take_deferred_model_switch(None, &models, Some("max"));
    assert_eq!(
        out,
        DeferredSwitchOutcome {
            switch: None,
            effort_error: Some(EffortTokenError::UnknownToken {
                token: "max".into(),
                offered: vec!["deep".into(), "high".into()],
            }),
        }
    );
}

#[test]
fn effort_only_errors_without_active_model() {
    let models = ModelState::default();
    assert_eq!(
        take_deferred_model_switch(None, &models, Some("high")),
        DeferredSwitchOutcome {
            switch: None,
            effort_error: Some(EffortTokenError::NoActiveModel),
        }
    );
}

#[test]
fn deferred_effort_error_uses_one_typed_control_notice() {
    let mut app = super::super::test_app_with_agent();
    let agent = app
        .agents
        .get_mut(&crate::app::session::AgentId(0))
        .unwrap();
    let initial_scrollback = agent.scrollback.len();

    assert_eq!(
        apply_deferred_switch_outcome(
            agent,
            DeferredSwitchOutcome {
                switch: None,
                effort_error: Some(EffortTokenError::Unsupported),
            },
        ),
        None
    );
    assert_eq!(agent.scrollback.len(), initial_scrollback + 1);
    assert!(
        agent.toast.is_none(),
        "the same failure must not also become a toast"
    );
    let entry = agent.scrollback.entries_mut().last().unwrap();
    let RenderBlock::Notice(notice) = &entry.block else {
        panic!("effort failure must render as a typed Notice");
    };
    assert_eq!(notice.tone, crate::scrollback::blocks::NoticeTone::Error);
    assert_eq!(
        notice.category,
        crate::scrollback::blocks::NoticeCategory::Control
    );
}
