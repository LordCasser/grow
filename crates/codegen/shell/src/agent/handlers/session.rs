//! Session meta-information handlers.
//!
//! Router pattern: single `handle()` dispatches by method name.
//! Business logic delegates to pure functions or MvpAgent methods.

use agent_client_protocol::{self as acp};
use serde::Deserialize;

use super::super::mvp_agent::MvpAgent;
use crate::session::{
    ContextInfo, ExtMethodResult, SessionCommand, SessionInfoData, SessionInfoResponse,
};

/// Router for the current session query and control methods.
pub async fn handle(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    match args.method.as_ref() {
        "grow/session/info" => handle_session_info(agent, args).await,
        "grow/session/set_agent" => handle_set_session_agent(agent, args).await,
        "grow/session/close" => handle_session_close(agent, args).await,
        "grow/session/list" => handle_session_list(agent, args).await,
        "grow/sessions/list" => handle_roster_list(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetSessionAgentRequest {
    session_id: String,
    agent_name: String,
}

async fn handle_set_session_agent(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    let request: SetSessionAgentRequest = serde_json::from_str(args.params.get())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let session_id = acp::SessionId::new(request.session_id);
    let handle = agent
        .sessions
        .borrow()
        .get(&session_id)
        .cloned()
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;

    let definition = agent::discovery::by_name_in_cwd_with_plugins(
        &request.agent_name,
        handle.tool_context.cwd.as_path(),
        agent.plugin_registry_handle.snapshot().as_deref(),
    )
    .ok_or_else(|| {
        acp::Error::invalid_params().data(format!("unknown agent: {}", request.agent_name))
    })?;
    let selected_name = definition.name.clone();
    let subagent_filter = definition.subagent_filter();
    let (responds_to, response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::RebuildAgentForDefinition {
            definition,
            responds_to,
        })
        .map_err(|_| acp::Error::internal_error().data("session actor closed"))?;
    response
        .await
        .map_err(|_| acp::Error::internal_error().data("session actor closed"))??;

    if let Some(handle) = agent.sessions.borrow_mut().get_mut(&session_id) {
        handle.agent_name = selected_name.clone();
        handle.subagent_filter = subagent_filter;
    }
    broadcast_agent_changed(agent, &session_id, &selected_name);
    ExtMethodResult::success(serde_json::json!({ "agentName": selected_name }))
        .to_ext_response()
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

fn broadcast_agent_changed(agent: &MvpAgent, session_id: &acp::SessionId, agent_name: &str) {
    let notification = crate::extensions::notification::SessionNotification {
        session_id: session_id.clone(),
        update: crate::extensions::notification::SessionUpdate::AgentChanged {
            agent_name: agent_name.to_owned(),
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

/// `grow/sessions/list` — the FleetView roster. Returns every
/// resident session plus recently-touched on-disk `Dormant` sessions. Clients
/// poll this while the dashboard is open and reconcile against the
/// `grow/sessions/changed` broadcast.
async fn handle_roster_list(
    agent: &MvpAgent,
    _args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    let sessions = agent.build_roster().await;
    ExtMethodResult::success(crate::agent::roster::RosterListResponse { sessions })
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionInfoRequest {
    session_id: Option<String>,
}

async fn handle_session_info(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    let req: SessionInfoRequest = serde_json::from_str(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;

    let session_id = req.session_id.or_else(|| {
        agent
            .sessions
            .borrow()
            .keys()
            .next()
            .map(|id| id.0.to_string())
    });

    let Some(session_id) = session_id else {
        return ExtMethodResult::success(serde_json::json!({}))
            .to_ext_response()
            .map_err(|e| acp::Error::internal_error().data(e.to_string()));
    };

    let sid = acp::SessionId::new(session_id.clone());
    let Some(session) = agent.sessions.borrow().get(&sid).cloned() else {
        return ExtMethodResult::success(serde_json::json!({}))
            .to_ext_response()
            .map_err(|e| acp::Error::internal_error().data(e.to_string()));
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = session
        .cmd_tx
        .send(SessionCommand::GetSessionInfo { responds_to: tx });
    let info = rx.await.ok();

    // Construct display data for `/session-info`.
    let mut data = info.unwrap_or_else(|| SessionInfoData {
        agent_name: None,
        model: None,
        model_display_name: None,
        resolved_model_id: None,
        model_fingerprint: None,
        show_model_fingerprint: false,
        api_backend: None,
        turns: 0,
        turn_index: 0,
        context: ContextInfo {
            auto_compact_threshold_percent:
                crate::util::config::DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT,
            ..ContextInfo::default()
        },
    });

    // Calculate the model's display name.
    let route = session.model_route.snapshot();
    data.model_display_name = agent
        .models_manager
        .models()
        .get(route.model_id.0.as_ref())
        .and_then(|entry| entry.info.name.clone());

    // Construct `SessionInfoResponse`.
    let response = SessionInfoResponse {
        session_id,
        cwd: session.info.cwd.clone(),
        data,
    };

    // Wrap `SessionInfoResponse` in `ExtMethodResult` and return it.
    ExtMethodResult::success(serde_json::to_value(&response).unwrap_or_default())
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

async fn handle_session_close(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct CloseRequest {
        session_id: String,
    }

    let req: CloseRequest = serde_json::from_str(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;

    let sid = acp::SessionId::new(req.session_id.clone());
    let existed = agent.sessions.borrow().contains_key(&sid);
    if existed {
        // Explicit terminal close: shut the actor down and finalize the cloud
        // replica (genuine session end). Distinct from a mere client disconnect,
        // which detaches but keeps the session resumable and never finalizes
        // (see `MvpAgent::handle_evict_sessions` / `close_session_explicit`).
        agent.request_session_shutdown(&sid);
        agent.close_session_explicit(&sid);
        tracing::info!(session_id = %req.session_id, "session closed via grow/session/close");
    } else {
        tracing::debug!(session_id = %req.session_id, "session/close: session not found (already closed)");
    }

    ExtMethodResult::success(serde_json::json!({ "success": true }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// ── Unified local session list ────────────────────────────────────────

async fn handle_session_list(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    use crate::session::unified_list;

    let req = unified_list::parse_list_req(args.params.get())
        .map_err(|e| acp::Error::invalid_params().data(format!("invalid params: {e}")))?;
    let result = unified_list::build_unified_list(req).await;

    ExtMethodResult::success(unified_list::ext_list_response(result))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}
