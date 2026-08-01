//! Applies a model switch to a session — the ungated path. `set_session_model`
//! enforces the `allowed_models` gate before delegating here; internal callers
//! (`new_session`, `load_session`) call `apply` directly.
use crate::agent::config;
use crate::agent::models::resolve_catalog_key;
use crate::agent::mvp_agent::MvpAgent;
use crate::session::SessionCommand;
use agent_client_protocol::{self as acp};
use sampling_types::parse_reasoning_effort_meta;
use tokio::sync::oneshot;
/// Apply a model switch to a session (no gate — `set_session_model` gates first).
pub(crate) async fn apply(
    agent: &MvpAgent,
    args: acp::SetSessionModelRequest,
) -> Result<acp::SetSessionModelResponse, acp::Error> {
    tracing::info!("Received set session model request {args:?}");
    ::diagnostics::unified_log::info(
        "model changed",
        Some(args.session_id.0.as_ref()),
        Some(serde_json::json!({"model": args.model_id.0.as_ref()})),
    );
    tracing::debug!("session_session_model::mvp_agent: {:?}", &args);
    let effort_override = parse_reasoning_effort_meta(args.meta.as_ref());
    let acp::SetSessionModelRequest {
        session_id,
        model_id,
        ..
    } = args;
    let handle = agent
        .session_handle_waiting_for_load(&session_id)
        .await
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
    let model = agent.resolve_model_id(&model_id)?;
    let model_id = resolve_catalog_key(&agent.models_manager.models(), &model_id)
        .expect("resolve_model_id accepted a model missing from the catalog");
    let use_concise = false;
    let previous_model_id = handle.model_id.0.clone();
    let mut model_sampling =
        agent.prepare_sampling_config_for_model(&model, handle.origin_client.clone());
    if let Some(eff) = effort_override {
        if agent
            .models_manager
            .model_supports_reasoning_effort(model_id.0.as_ref())
        {
            tracing::info!(
                session_id = %session_id.0,
                effort = %eff,
                "set_session_model: applying reasoning_effort override from meta"
            );
            model_sampling.reasoning_effort = Some(eff);
        } else {
            tracing::warn!(
                session_id = %session_id.0,
                model_id = %model_id.0,
                effort = %eff,
                "set_session_model: ignoring reasoning_effort override — model does not support it"
            );
        }
    }
    let applied_effort = model_sampling.reasoning_effort;
    let model_unchanged = previous_model_id == model_id.0;
    let new_threshold = {
        let cfg = agent.cfg.borrow();
        let models = agent.models_manager.models();
        let model = config::find_model_by_id(&models, model_sampling.model.as_str());
        crate::util::config::resolve_auto_compact_threshold_percent(
            &cfg,
            model_sampling.model.as_str(),
            model.map(|e| &e.info),
        )
    };
    let (tx, rx) = oneshot::channel();
    let _ = handle.cmd_tx.send(SessionCommand::SetSessionModel {
        model_id: model_id.clone(),
        sampling_config: model_sampling,
        use_concise,
        apply_prompt_override: false,
        skip_prompt_rewrite: model_unchanged,
        auto_compact_threshold_percent: new_threshold,
        responds_to: tx,
    });
    let updated_model = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("failed to set session model"))?;
    if let Some(handle) = agent.sessions.borrow_mut().get_mut(&session_id) {
        handle.model_id = model_id.clone();
        handle.reasoning_effort = applied_effort;
    }
    broadcast_model_changed(
        agent,
        &session_id,
        model_id.0.as_ref(),
        applied_effort.map(|eff| eff.to_string()),
    );
    ::diagnostics::session_ctx::log_event(::diagnostics::events::ModelSwitched {
        session_id: session_id.0.to_string(),
        previous_model_id: previous_model_id.to_string(),
        new_model_id: model_id.0.to_string(),
        success: true,
        error_code: None,
        required_agent_type: None,
        current_agent_type: None,
    });
    // The catalog manager owns the global default used by future sessions.
    // A switch—whether user initiated or performed while restoring a session—
    // updates only this session's handle and persisted state.
    Ok(acp::SetSessionModelResponse::new().meta(
        serde_json::json!({
            "model": updated_model,
        })
        .as_object()
        .cloned(),
    ))
}
/// Broadcast a `ModelChanged` to every client subscribed to this session so
/// followers mirror the new model. The originating client ignores its own echo
/// (gated by `model_switch_pending`). Broadcast-only — no eventId, not persisted.
fn broadcast_model_changed(
    agent: &MvpAgent,
    session_id: &acp::SessionId,
    model_id: &str,
    reasoning_effort: Option<String>,
) {
    let notification = crate::extensions::notification::SessionNotification {
        session_id: session_id.clone(),
        update: crate::extensions::notification::SessionUpdate::ModelChanged {
            model_id: model_id.to_owned(),
            reasoning_effort,
        },
        meta: None,
    };
    if let Ok(params) = serde_json::value::to_raw_value(&notification) {
        agent
            .gateway
            .forward_fire_and_forget(acp::ExtNotification::new(
                "grow/session_notification",
                params.into(),
            ));
    }
}
