//! Session meta-information handlers.
//!
//! Router pattern: single `handle()` dispatches by method name.
//! Business logic delegates to pure functions or MvpAgent methods.

use acp_transport::protocol as acp;
use serde::Deserialize;

use super::super::mvp_agent::MvpAgent;
use crate::session::{
    ContextInfo, ControlIntent, ExtMethodResult, SessionCommand, SessionInfoData,
    SessionInfoResponse,
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
    intent: Option<ControlIntent>,
}

async fn handle_set_session_agent(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> Result<acp::ExtResponse, acp::Error> {
    let request: SetSessionAgentRequest = serde_json::from_str(args.params.get())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let session_id = acp::SessionId::new(request.session_id);
    let handle = agent
        .control_session_handle_waiting_for_load(&session_id)
        .await
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;

    let workflow_pinned = handle.workflow_run_id.is_some();
    let mut definition = if let Some(run_id) = handle.workflow_run_id.as_deref() {
        let tracker = handle.workflow_tracker.lock();
        let route = &tracker
            .get(run_id)
            .ok_or_else(|| {
                acp::Error::invalid_params()
                    .data(format!("Workflow Run '{run_id}' is no longer registered"))
            })?
            .runtime_route;
        route
            .agent_definition(&request.agent_name)
            .map_err(|error| acp::Error::invalid_params().data(error))?
    } else {
        let plugins = handle.plugin_registry.read().clone();
        agent::discovery::by_name_in_cwd_with_plugins(
            &request.agent_name,
            handle.tool_context.cwd.as_path(),
            plugins.as_deref(),
        )
        .ok_or_else(|| {
            acp::Error::invalid_params().data(format!("unknown agent: {}", request.agent_name))
        })?
    };
    let is_child = agent
        .active_child_sessions
        .borrow()
        .contains_key(&session_id);
    // Agent selection changes the authored profile, never the session
    // operator's capability clamp. Child clamps remain layered so the new
    // profile also stays below its parent-owned authority boundary.
    if !workflow_pinned && is_child {
        agent
            .cfg
            .borrow()
            .cli_agent_overrides
            .apply_to_subagent_definition(&mut definition);
    } else if !workflow_pinned {
        agent
            .cfg
            .borrow()
            .cli_agent_overrides
            .apply_to_definition(&mut definition);
    }
    let selected_name = definition.selector_identity();
    let (responds_to, response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::RebuildAgentForDefinition {
            definition,
            intent: request.intent,
            responds_to,
        })
        .map_err(|_| acp::Error::internal_error().data("session actor closed"))?;
    let outcome = response
        .await
        .map_err(|_| acp::Error::internal_error().data("session actor closed"))??;
    let response = match outcome {
        crate::session::DesiredStateOutcome::Applied(()) => {
            serde_json::json!({ "status": "applied", "agentName": selected_name })
        }
        crate::session::DesiredStateOutcome::InFlight => {
            serde_json::json!({ "status": "in_flight" })
        }
        crate::session::DesiredStateOutcome::Superseded => {
            serde_json::json!({ "status": "superseded" })
        }
    };
    ExtMethodResult::success(response)
        .to_ext_response()
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
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
        return session_info_failure(
            "unavailable",
            "selection",
            None,
            Some("no resident session is available; provide sessionId"),
        );
    };

    let sid = acp::SessionId::new(session_id.clone());
    let Some(session) = agent.control_session_handle(&sid) else {
        let session_id_for_read = session_id.clone();
        let storage_root = crate::util::grow_home::grow_home();
        let durable = crate::extensions::session_admin::bounded_shared_read(move || {
            read_durable_session_info(session_id_for_read, storage_root)
        })
        .await;
        return match durable {
            Ok(Some((mut response, metadata))) => {
                let models = agent.models_manager.models();
                response.data.model_display_name = response
                    .data
                    .model
                    .as_deref()
                    .and_then(|model_id| models.get(model_id))
                    .and_then(|entry| entry.info.name.clone());
                session_info_found(response, "durable", metadata)
            }
            Ok(None) => session_info_failure("absent", "durable", Some(&session_id), None),
            Err(crate::extensions::session_admin::BoundedSharedReadError::TimedOut) => {
                session_info_failure("timed_out", "durable", Some(&session_id), None)
            }
            Err(crate::extensions::session_admin::BoundedSharedReadError::Io(error))
                if error.kind() == std::io::ErrorKind::InvalidData =>
            {
                session_info_failure(
                    "invalid_evidence",
                    "durable",
                    Some(&session_id),
                    Some(&error.to_string()),
                )
            }
            Err(crate::extensions::session_admin::BoundedSharedReadError::Io(error)) => {
                session_info_failure(
                    "unavailable",
                    "durable",
                    Some(&session_id),
                    Some(&error.to_string()),
                )
            }
            Err(crate::extensions::session_admin::BoundedSharedReadError::Worker(error)) => {
                session_info_failure("unavailable", "durable", Some(&session_id), Some(&error))
            }
        };
    };

    let (tx, rx) = tokio::sync::oneshot::channel();
    if session
        .cmd_tx
        .send(SessionCommand::GetSessionInfo { responds_to: tx })
        .is_err()
    {
        return session_info_failure(
            "unavailable",
            "runtime",
            Some(&session_id),
            Some("session actor is unavailable"),
        );
    }
    let info = match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
        Ok(Ok(info)) => Some(info),
        Ok(Err(_)) => {
            return session_info_failure(
                "unavailable",
                "runtime",
                Some(&session_id),
                Some("session actor dropped the info response"),
            );
        }
        Err(_) => {
            return session_info_failure("timed_out", "runtime", Some(&session_id), None);
        }
    };

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
    session_info_found(response, "runtime", serde_json::Value::Null)
}

fn session_info_found(
    response: SessionInfoResponse,
    source: &str,
    metadata: serde_json::Value,
) -> Result<acp::ExtResponse, acp::Error> {
    let mut value = serde_json::to_value(&response)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| {
        acp::Error::internal_error().data("session info response was not an object")
    })?;
    object.insert("status".into(), serde_json::json!("found"));
    object.insert("source".into(), serde_json::json!(source));
    if !metadata.is_null() {
        object.insert("metadata".into(), metadata);
    }
    ExtMethodResult::success(value)
        .to_ext_response()
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

fn session_info_failure(
    status: &str,
    source: &str,
    session_id: Option<&str>,
    reason: Option<&str>,
) -> Result<acp::ExtResponse, acp::Error> {
    let mut value = serde_json::json!({
        "status": status,
        "source": source,
        "sessionId": session_id,
    });
    if let Some(reason) = reason {
        value["reason"] = serde_json::json!(reason);
    }
    ExtMethodResult::success(value)
        .to_ext_response()
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

fn read_durable_session_info(
    session_id: String,
    storage_root: std::path::PathBuf,
) -> std::io::Result<Option<(SessionInfoResponse, serde_json::Value)>> {
    use chat_state::{TimelineEventKind, TurnEvent};

    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(storage_root);
    let Some(opened) = storage.open_session_by_id_shared_read_bounded(&session_id)? else {
        return Ok(None);
    };
    let validated = opened.bounded_validated_timeline(&session_id)?;
    let summary = opened.summary();
    let turns = validated
        .timeline
        .events()
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                TimelineEventKind::Turn(TurnEvent::Started { .. })
            )
        })
        .count() as u64;
    let data = SessionInfoData {
        agent_name: None,
        model: Some(summary.current_model_id.to_string()),
        model_display_name: None,
        resolved_model_id: None,
        model_fingerprint: None,
        show_model_fingerprint: false,
        api_backend: None,
        turns,
        turn_index: turns.saturating_sub(1),
        context: ContextInfo {
            auto_compact_threshold_percent:
                crate::util::config::DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT,
            ..ContextInfo::default()
        },
    };
    let response = SessionInfoResponse {
        session_id,
        cwd: summary.info.cwd.clone(),
        data,
    };
    let metadata = serde_json::json!({
        "title": summary.title,
        "createdAt": summary.created_at,
        "updatedAt": summary.updated_at,
        "messageCount": summary.num_messages,
        "timelineEventCount": validated.timeline.events().len(),
    });
    Ok(Some((response, metadata)))
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
    // The existence check belongs inside the per-session lifecycle
    // transaction. Otherwise a racing load can appear after this handler has
    // already reported a successful no-op close.
    let existed = agent
        .close_session_explicit(&sid)
        .await
        .map_err(|error| acp::Error::internal_error().data(error))?;
    if existed {
        // Explicit terminal close: shut the actor down and finalize the cloud
        // replica (genuine session end). Distinct from a mere client disconnect,
        // which detaches but keeps the session resumable and never finalizes
        // (see `MvpAgent::handle_evict_sessions` / `close_session_explicit`).
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

#[cfg(test)]
mod session_info_tests {
    use super::*;
    use crate::session::info::Info;
    use crate::session::persistence::default_model_id;
    use crate::session::storage::StorageAdapter as _;
    use tempfile::TempDir;

    fn result(response: acp::ExtResponse) -> serde_json::Value {
        serde_json::from_str(response.0.get()).expect("valid extension response")
    }

    #[test]
    fn session_info_success_preserves_existing_fields_and_adds_source() {
        let response = SessionInfoResponse {
            session_id: "session-1".into(),
            cwd: "/tmp/project".into(),
            data: SessionInfoData {
                agent_name: None,
                model: Some("provider/model".into()),
                model_display_name: None,
                resolved_model_id: None,
                model_fingerprint: None,
                show_model_fingerprint: false,
                api_backend: None,
                turns: 3,
                turn_index: 2,
                context: ContextInfo::default(),
            },
        };

        let response =
            session_info_found(response, "durable", serde_json::json!({"title": "saved"}))
                .expect("session info response");
        let value = result(response)["result"].clone();
        assert_eq!(value["status"], "found");
        assert_eq!(value["source"], "durable");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["cwd"], "/tmp/project");
        assert_eq!(value["model"], "provider/model");
        assert_eq!(value["turns"], 3);
        assert_eq!(value["metadata"]["title"], "saved");
    }

    #[test]
    fn session_info_failure_is_typed_instead_of_an_empty_object() {
        let response = session_info_failure("absent", "durable", Some("missing"), None)
            .expect("typed absence response");
        let value = result(response)["result"].clone();
        assert_eq!(value["status"], "absent");
        assert_eq!(value["source"], "durable");
        assert_eq!(value["sessionId"], "missing");
        assert!(!value.as_object().expect("typed object").is_empty());
    }

    #[tokio::test]
    async fn durable_session_info_reads_isolated_root_without_mutation_or_writer_conflict() {
        let root = TempDir::new().expect("isolated session root");
        let info = Info {
            id: acp::SessionId::new("dormant-info-fixture"),
            cwd: "/fixture/project".into(),
        };
        let writer = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
            root.path().to_path_buf(),
        );
        let summary = writer
            .init_session(&info, default_model_id())
            .await
            .expect("fixture session");
        let directory = writer
            .open_session(&info)
            .expect("writer session handle")
            .directory_handle();
        let summary_before = std::fs::read(directory.display_path().join("summary.json"))
            .expect("summary bytes before read");
        let timeline_before = std::fs::read(directory.display_path().join("timeline.jsonl"))
            .expect("timeline bytes before read");

        // The original adapter has initialized the session as its writer.
        // The dormant observer must read concurrently without trying to claim
        // a competing writer capability.
        let (response, metadata) =
            read_durable_session_info(info.id.0.to_string(), root.path().to_path_buf())
                .expect("durable read")
                .expect("session found");
        let model_id = summary.current_model_id.to_string();

        assert_eq!(response.session_id, info.id.0.as_ref());
        assert_eq!(response.cwd, info.cwd);
        assert_eq!(response.data.model.as_deref(), Some(model_id.as_str()));
        assert_eq!(response.data.turns, 0);
        assert_eq!(metadata["timelineEventCount"], 0);
        assert_eq!(
            std::fs::read(directory.display_path().join("summary.json")).unwrap(),
            summary_before
        );
        assert_eq!(
            std::fs::read(directory.display_path().join("timeline.jsonl")).unwrap(),
            timeline_before
        );
        assert!(
            writer.open_session(&info).is_ok(),
            "writer handle remains usable"
        );
    }
}
