//! Session-administration extension handlers.
//!
//! Methods grouped here are operational/admin endpoints that mutate
//! persistent or shared agent state but are not part of the per-turn prompt
//! lifecycle:
//!
//! - `grow/session/rename`                  rename a local session
//! - `grow/session/delete`                  delete a local session
//! - `grow/session/update_mcp_servers`      mid-session MCP server swap
//! - `grow/session/fork`                    fork a session into a new one
//! - `grow/internal/reload_mcp_catalog`     config hot-reload, explicitly scoped
//! - `grow/internal/reload_skills`          skills file watcher fan-out
//! - `grow/internal/reload_models`          model list hot-reload from config.toml
//! - `grow/internal/reload_announcements`   local announcement hot-reload
//! - `grow/plugins/reload`                  rebuild shared plugin registry
//! - `grow/commands/list`                   list slash commands
//! - `grow/commands/execute`                execute a slash command out of band

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use acp_transport::AcpClientHandler as _;
use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::persistence::list_summaries;
use crate::session::storage::jsonl::JsonlStorageAdapter;
use crate::session::{ExtMethodResult, SessionCommand};

/// Wire request for the Shell-owned slash-command plane.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCommandRequest {
    pub session_id: String,
    pub command: String,
    pub description: String,
    pub invocation_id: String,
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/session/rename" => handle_session_rename(agent, args).await,
        "grow/session/delete" => handle_session_delete(agent, args).await,
        "grow/session/update_mcp_servers" => handle_update_mcp_servers(agent, args).await,
        "grow/session/fork" => handle_session_fork(agent, args).await,
        "grow/internal/reload_mcp_catalog" => handle_reload_mcp_catalog(agent, args).await,
        "grow/internal/reload_skills" => handle_reload_skills(agent),
        "grow/internal/reload_workflows" => handle_reload_workflows(agent),
        "grow/internal/reload_subagent_toggle" => handle_reload_subagent_toggle(agent),
        "grow/internal/reload_models" => handle_reload_models(agent).await,
        "grow/internal/reload_announcements" => handle_reload_announcements(agent, args),
        "grow/plugins/reload" => handle_plugins_reload(agent).await,
        "grow/commands/list" => handle_commands_list(agent, args).await,
        "grow/commands/execute" => handle_command_execute(agent, args).await,
        "grow/queue/prompt_status" => handle_prompt_status(agent, args).await,
        "grow/input/admission_status" => handle_input_status(agent, args).await,
        "grow/queue/control" => handle_queue_control(agent, args).await,
        "grow/plan/artifact" => handle_plan_artifact_query(agent, args).await,
        "grow/plan/review" => handle_plan_review(agent, args).await,
        "grow/plan/decision_status" => handle_plan_decision_status(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_plan_review(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: tools::implementations::grow_build::plan_control::types::PlanReviewRequest =
        parse_params(args)?;
    let handle = agent
        .control_session_handle_waiting_for_load(&acp::SessionId::new(request.session_id))
        .await
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session"))?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(crate::session::SessionCommand::ReviewRetainedPlan {
            submission_id: request.submission_id,
            decision_id: request.decision_id,
            respond_to: tx,
        })
        .map_err(|_| acp::Error::internal_error().data("Plan review owner unavailable"))?;
    match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(()))) => to_raw_response(&serde_json::json!({ "status": "accepted" })),
        Ok(Ok(Err(error))) => Err(acp::Error::invalid_params().data(error)),
        _ => to_raw_response(&serde_json::json!({ "status": "unconfirmed" })),
    }
}

async fn handle_plan_decision_status(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: tools::implementations::grow_build::plan_control::types::PlanDecisionQuery =
        parse_params(args)?;
    if request.decision_id.is_empty() || request.decision_id.len() > 128 {
        return Err(acp::Error::invalid_params().data("invalid decision identity"));
    }
    let handle = agent
        .control_session_handle_waiting_for_load(&acp::SessionId::new(request.session_id))
        .await
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session"))?;
    let chat_state = &handle.chat_state_handle;
    // The Timeline writer accepts an event only after durable ACK. If an ACK
    // is still pending, even a file that is visible on disk does not prove
    // this decision committed. Keep the original identity pending for a
    // later status query instead of queueing behind that write.
    if chat_state.pending_timeline_commit().is_some() {
        return to_raw_response(&serde_json::json!({ "status": "unconfirmed" }));
    }
    let events = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        chat_state.timeline_events(),
    )
    .await
    {
        Ok(Some(events)) => events,
        Ok(None) if chat_state.pending_timeline_commit().is_some() => {
            return to_raw_response(&serde_json::json!({ "status": "unconfirmed" }));
        }
        Ok(None) => {
            return Err(acp::Error::internal_error().data("Plan receipt history unavailable"));
        }
        Err(_) => return to_raw_response(&serde_json::json!({ "status": "unconfirmed" })),
    };
    for event in events.iter().rev() {
        let chat_state::TimelineEventKind::Control(control) = &event.kind else {
            continue;
        };
        let Some(value) = control.snapshot.get("behavior") else {
            continue;
        };
        let snapshot: crate::session::behavior::BehaviorSnapshot =
            serde_json::from_value(value.clone())
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        if let Some(receipt) = snapshot.retained_plan.as_ref().and_then(|candidate| {
            candidate
                .decisions
                .iter()
                .find(|receipt| receipt.decision_id == request.decision_id)
        }) {
            return to_raw_response(
                &serde_json::json!({ "status": "accepted", "receipt": receipt }),
            );
        }
    }
    // A query may overtake the submitted operation. Absence cannot prove rejection.
    to_raw_response(&serde_json::json!({ "status": "unconfirmed" }))
}

fn plan_preview_reference(
    snapshot: &crate::session::behavior::BehaviorSnapshot,
) -> Option<(u64, u64, String, bool)> {
    if snapshot.behavior() != tool_types::BehaviorId::Plan
        && let Some(completed) = &snapshot.completed_plan
    {
        return Some((
            completed.submission_id,
            completed.artifact_revision,
            completed.artifact_hash.clone(),
            false,
        ));
    }
    if let Some(candidate) = snapshot.retained_plan.as_ref().filter(|candidate| {
        matches!(
            candidate.status,
            crate::session::behavior::RetainedPlanStatus::Deferred
                | crate::session::behavior::RetainedPlanStatus::Reviewing
        )
    }) {
        return Some((
            candidate.submission_id,
            candidate.artifact_revision,
            candidate.artifact_hash.clone(),
            true,
        ));
    }
    Some((
        snapshot.plan_submission_id?,
        snapshot.plan_artifact_revision,
        snapshot.plan_artifact_hash.clone()?,
        false,
    ))
}

async fn handle_plan_artifact_query(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    use tools::implementations::grow_build::plan_control::{
        PlanArtifactQueryRequest, PlanArtifactQueryResponse,
    };

    let request: PlanArtifactQueryRequest = parse_params(args)?;
    if request.session_id.is_empty() || request.session_id.len() > 1024 {
        return Err(acp::Error::invalid_params().data("sessionId must contain 1 to 1024 bytes"));
    }
    let session_id = acp::SessionId::new(request.session_id.clone());
    let handle = agent
        .control_session_handle_waiting_for_load(&session_id)
        .await
        .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
    if handle.info.id != session_id {
        return Err(acp::Error::invalid_params().data("session identity changed"));
    }

    let snapshot = handle.behavior.lock().snapshot();
    let reference = plan_preview_reference(&snapshot).ok_or_else(|| {
        acp::Error::invalid_params().data("session has no current Plan candidate")
    })?;
    let (submission_id, revision, hash, retained) = reference;
    let completed = snapshot.completed_plan;
    let behavior = handle.behavior.clone();
    let session_directory = handle.session_directory.clone();
    let response = bounded_shared_read(move || {
        let content = crate::session::behavior::read_plan_artifact(&session_directory, &hash)?;
        let completion_report = completed
            .as_ref()
            .filter(|completed| {
                completed.submission_id == submission_id && completed.artifact_hash == hash
            })
            .map(|completed| {
                crate::session::behavior::read_plan_artifact(
                    &session_directory,
                    &completed.report_hash,
                )
            })
            .transpose()?;
        let current = behavior.lock().snapshot();
        if plan_preview_reference(&current)
            != Some((submission_id, revision, hash.clone(), retained))
            || current.completed_plan != completed
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Plan artifact changed during query",
            ));
        }
        Ok(PlanArtifactQueryResponse {
            session_id: request.session_id,
            submission_id,
            retained,
            artifact_revision: revision,
            artifact_hash: hash,
            plan_content: content,
            completion_report,
        })
    })
    .await
    .map_err(|error| match error {
        BoundedSharedReadError::TimedOut => {
            acp::Error::internal_error().data("Plan artifact query timed out")
        }
        BoundedSharedReadError::Io(error) => acp::Error::internal_error().data(error.to_string()),
        BoundedSharedReadError::Worker(error) => acp::Error::internal_error().data(error),
    })?;
    to_raw_response(&response)
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum InputAdmissionObservation {
    UnknownUnconfirmed,
    Submitted,
    Allowed {
        route: chat_state::InputRoute,
    },
    Blocked {
        reason: chat_state::InputBlockReason,
    },
    Consumed {
        route: chat_state::InputRoute,
    },
    Handled,
    Dismissed {
        reason: chat_state::InputDismissReason,
    },
    Unavailable {
        reason: String,
    },
    TimedOut,
    InvalidEvidence {
        reason: String,
    },
    Absent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InputAdmissionStatusResponse {
    session_id: String,
    prompt_id: String,
    source: &'static str,
    status: InputAdmissionObservation,
}

async fn handle_input_status(_agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct InputStatusRequest {
        session_id: String,
        prompt_id: String,
    }

    let request: InputStatusRequest = parse_params(args)?;
    if request.session_id.is_empty()
        || request.session_id.len() > 1024
        || request.prompt_id.is_empty()
        || request.prompt_id.len() > 1024
    {
        return Err(acp::Error::invalid_params()
            .data("sessionId and promptId must each contain 1 to 1024 bytes"));
    }
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    let storage_root = crate::util::grow_home::grow_home();
    let status = match bounded_shared_read({
        let session_id = session_id.clone();
        let prompt_id = request.prompt_id.clone();
        move || read_durable_input_status(session_id, prompt_id, storage_root)
    })
    .await
    {
        Ok(DurableInputObservation::Absent) => InputAdmissionObservation::Absent,
        Ok(DurableInputObservation::Found(status)) => status,
        Err(BoundedSharedReadError::TimedOut) => InputAdmissionObservation::TimedOut,
        Err(BoundedSharedReadError::Io(error))
            if error.kind() == std::io::ErrorKind::InvalidData =>
        {
            InputAdmissionObservation::InvalidEvidence {
                reason: error.to_string(),
            }
        }
        Err(BoundedSharedReadError::Io(error)) => InputAdmissionObservation::Unavailable {
            reason: error.to_string(),
        },
        Err(BoundedSharedReadError::Worker(reason)) => {
            InputAdmissionObservation::Unavailable { reason }
        }
    };
    to_raw_response(&InputAdmissionStatusResponse {
        session_id: request.session_id,
        prompt_id: request.prompt_id,
        source: "durable",
        status,
    })
}

enum DurableInputObservation {
    Absent,
    Found(InputAdmissionObservation),
}

fn read_durable_input_status(
    session_id: acp::SessionId,
    prompt_id: String,
    storage_root: std::path::PathBuf,
) -> std::io::Result<DurableInputObservation> {
    use chat_state::{InputEvent, InputState, TimelineEventKind};

    let storage = JsonlStorageAdapter::with_root(storage_root);
    let Some(opened) = storage.open_session_by_id_shared_read_bounded(session_id.0.as_ref())?
    else {
        return Ok(DurableInputObservation::Absent);
    };
    let validated = opened.bounded_validated_timeline(session_id.0.as_ref())?;
    let events = validated.timeline.events();
    let mut matched: Option<(String, chat_state::InputPayloadRef)> = None;
    for event in events {
        let TimelineEventKind::Input(InputEvent::Submitted {
            input_id,
            payload_ref,
            ..
        }) = &event.kind
        else {
            continue;
        };
        let linked_prompt_id =
            crate::session::input_inbox::read_prompt_id(opened.directory(), payload_ref).map_err(
                |error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("submitted input payload is unavailable or invalid: {error}"),
                    )
                },
            )?;
        if linked_prompt_id == prompt_id {
            if matched.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "multiple submitted inputs reference the same prompt identity",
                ));
            }
            matched = Some((input_id.clone(), payload_ref.clone()));
        }
    }
    let Some((input_id, payload_ref)) = matched else {
        // A durable snapshot without the prompt is not proof of absence: the
        // original RPC may still be in a mailbox or awaiting its first commit.
        return Ok(DurableInputObservation::Found(
            InputAdmissionObservation::UnknownUnconfirmed,
        ));
    };
    let payload = crate::session::input_inbox::read_payload(opened.directory(), &payload_ref)
        .map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("submitted input payload is unavailable or invalid: {error}"),
            )
        })?;
    let Some(lifecycle) = validated.timeline.input_state(&input_id) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "submitted input is missing from its validated lifecycle projection",
        ));
    };
    if !crate::session::input_inbox::payload_matches_intent(lifecycle.intent, &payload) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "submitted input intent does not match its payload artifact",
        ));
    }
    let handled = events.iter().any(|event| {
        matches!(
            &event.kind,
            TimelineEventKind::Input(InputEvent::Handled { input_ids, .. })
                if input_ids.iter().any(|id| id == &input_id)
        )
    });
    let status = if handled {
        InputAdmissionObservation::Handled
    } else {
        match lifecycle.state {
            InputState::Submitted => InputAdmissionObservation::Submitted,
            InputState::Allowed { route } => InputAdmissionObservation::Allowed { route },
            InputState::Blocked { reason } => InputAdmissionObservation::Blocked { reason },
            InputState::Consumed { route } => InputAdmissionObservation::Consumed { route },
            InputState::Dismissed { reason } => InputAdmissionObservation::Dismissed { reason },
        }
    };
    Ok(DurableInputObservation::Found(status))
}

async fn handle_queue_control(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: crate::session::prompt_queue::QueueControlRequest = parse_params(args)?;
    if request.id.is_empty() {
        return Err(acp::Error::invalid_params().data("queue id must not be empty"));
    }
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::invalid_request()
            .data(format!("unknown session id: {}", request.session_id)));
    };
    let (respond_to, response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::QueueControl {
            request,
            respond_to,
        })
        .map_err(|_| acp::Error::internal_error().data("session actor unavailable"))?;
    let result = response
        .await
        .map_err(|_| acp::Error::internal_error().data("session actor dropped queue control"))?;
    to_raw_response(&result)
}

async fn handle_prompt_status(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let result = tokio::time::timeout(
        RUNTIME_OBSERVATION_TIMEOUT,
        handle_prompt_status_inner(agent, args),
    )
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            let params: serde_json::Value = parse_params(args)?;
            let session_id = params
                .get("sessionId")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| acp::Error::invalid_params().data("sessionId is required"))?;
            prompt_status_response(
                &acp::SessionId::new(session_id),
                None,
                "observation",
                crate::session::prompt_queue::PromptStatus::TimedOut,
                None,
            )
        }
    }
}

async fn handle_prompt_status_inner(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PromptStatusRequest {
        session_id: String,
        #[serde(default)]
        prompt_id: Option<String>,
    }

    let request: PromptStatusRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    if request.prompt_id.is_none()
        && let Some((status, metadata, source)) =
            observe_subagent_lifecycle(agent, &session_id).await
    {
        let epoch = agent
            .control_session_handle(&session_id)
            .map(|handle| handle.runtime_observation.read().control_epoch.clone());
        return prompt_status_response(&session_id, epoch.as_deref(), source, status, metadata);
    }
    if let Some(handle) = agent.control_session_handle(&session_id) {
        let snapshot = handle.runtime_observation.read().clone();
        if snapshot.session_id != session_id || snapshot.control_epoch.is_empty() {
            return prompt_status_response(
                &session_id,
                None,
                "runtime",
                crate::session::prompt_queue::PromptStatus::InvalidEvidence {
                    reason: "runtime observation identity did not match the requested session"
                        .into(),
                },
                None,
            );
        }
        let status = request.prompt_id.as_deref().map_or_else(
            || snapshot.lifecycle_status(),
            |prompt_id| snapshot.prompt_status(prompt_id),
        );
        return prompt_status_response(
            &session_id,
            Some(&snapshot.control_epoch),
            "runtime",
            status,
            handle
                .chat_state_handle
                .pending_timeline_commit()
                .map(|pending| {
                    serde_json::json!({
                        "lastAcknowledgedSeq": pending.event.seq.get().checked_sub(1),
                        "pendingCommitSeq": pending.event.seq.get(),
                        "acknowledgementLost": pending.acknowledgement_lost,
                        "persistenceErrorKind": pending.error_kind.map(|kind| format!("{kind:?}")),
                        "commitObservation": "pending; disk visibility alone does not confirm sync",
                    })
                }),
        );
    }

    let session_id_for_read = session_id.clone();
    let prompt_id_for_read = request.prompt_id.clone();
    let durable = bounded_shared_read(move || {
        read_durable_prompt_status(session_id_for_read, prompt_id_for_read.as_deref())
    })
    .await;

    match durable {
        Err(BoundedSharedReadError::TimedOut) => prompt_status_response(
            &session_id,
            None,
            "durable",
            crate::session::prompt_queue::PromptStatus::TimedOut,
            None,
        ),
        Err(BoundedSharedReadError::Io(error))
            if error.kind() == std::io::ErrorKind::InvalidData =>
        {
            prompt_status_response(
                &session_id,
                None,
                "durable",
                crate::session::prompt_queue::PromptStatus::InvalidEvidence {
                    reason: error.to_string(),
                },
                None,
            )
        }
        Err(BoundedSharedReadError::Io(error)) => prompt_status_response(
            &session_id,
            None,
            "durable",
            crate::session::prompt_queue::PromptStatus::Unavailable {
                reason: error.to_string(),
            },
            None,
        ),
        Err(BoundedSharedReadError::Worker(error)) => prompt_status_response(
            &session_id,
            None,
            "durable",
            crate::session::prompt_queue::PromptStatus::Unavailable { reason: error },
            None,
        ),
        Ok(DurablePromptObservation::Absent) => prompt_status_response(
            &session_id,
            None,
            "durable",
            crate::session::prompt_queue::PromptStatus::Absent,
            None,
        ),
        Ok(DurablePromptObservation::Found { status, metadata }) => {
            prompt_status_response(&session_id, None, "durable", status, Some(metadata))
        }
    }
}

enum DurableSubagentLifecycle {
    NotSubagent,
    AwaitingParentEnd {
        parent_timeline_id: String,
        subagent_id: String,
    },
    Settling,
    Terminal {
        stop_reason: String,
        metadata: serde_json::Value,
    },
}

async fn observe_subagent_lifecycle(
    agent: &MvpAgent,
    child_session_id: &acp::SessionId,
) -> Option<(
    crate::session::prompt_queue::PromptStatus,
    Option<serde_json::Value>,
    &'static str,
)> {
    use crate::session::prompt_queue::PromptStatus;
    use tools::implementations::grow_build::task::types::SubagentObservationError;

    let is_live = agent.is_live_subagent_session(child_session_id);
    let child_id = child_session_id.0.to_string();
    let lifecycle =
        bounded_shared_read(move || read_durable_subagent_lifecycle(child_id, is_live)).await;
    let source = if is_live { "runtime" } else { "durable" };
    let lifecycle = match lifecycle {
        Ok(lifecycle) => lifecycle,
        Err(BoundedSharedReadError::TimedOut) => {
            return Some((PromptStatus::TimedOut, None, source));
        }
        Err(BoundedSharedReadError::Io(error))
            if error.kind() == std::io::ErrorKind::InvalidData =>
        {
            return Some((
                PromptStatus::InvalidEvidence {
                    reason: error.to_string(),
                },
                None,
                source,
            ));
        }
        Err(BoundedSharedReadError::Io(error)) => {
            return Some((
                PromptStatus::Unavailable {
                    reason: error.to_string(),
                },
                None,
                source,
            ));
        }
        Err(BoundedSharedReadError::Worker(error)) => {
            return Some((PromptStatus::Unavailable { reason: error }, None, source));
        }
    };
    match lifecycle {
        DurableSubagentLifecycle::NotSubagent => None,
        DurableSubagentLifecycle::Settling => Some((PromptStatus::Settling, None, "durable")),
        DurableSubagentLifecycle::Terminal {
            stop_reason,
            metadata,
        } => Some((
            PromptStatus::Terminal {
                stop_reason,
                agent_result: None,
            },
            Some(metadata),
            "durable",
        )),
        DurableSubagentLifecycle::AwaitingParentEnd {
            parent_timeline_id,
            subagent_id,
        } => {
            let backend = agent.authorized_subagent_observer();
            let inspection = match backend.inspect(&subagent_id).await {
                Ok(inspection) => inspection,
                Err(SubagentObservationError::TimedOut) => {
                    return Some((PromptStatus::TimedOut, None, "runtime"));
                }
                Err(SubagentObservationError::Unavailable) => {
                    return Some((
                        PromptStatus::Unavailable {
                            reason: "subagent coordinator observation is unavailable".into(),
                        },
                        None,
                        "runtime",
                    ));
                }
                Err(SubagentObservationError::InvalidEvidence) => {
                    return Some((
                        PromptStatus::InvalidEvidence {
                            reason: "subagent coordinator reported invalid lifecycle evidence"
                                .into(),
                        },
                        None,
                        "runtime",
                    ));
                }
                Err(SubagentObservationError::Unauthorized) => {
                    return Some((
                        PromptStatus::InvalidEvidence {
                            reason: "subagent coordinator rejected the durable parent identity"
                                .into(),
                        },
                        None,
                        "runtime",
                    ));
                }
            };
            let Some(inspection) = inspection else {
                return Some((PromptStatus::Settling, None, "durable"));
            };
            if inspection.parent_session_id != parent_timeline_id
                || inspection.child_session_id != child_session_id.0.as_ref()
                || inspection.snapshot.subagent_id != subagent_id
            {
                return Some((
                    PromptStatus::InvalidEvidence {
                        reason:
                            "coordinator inspection identity did not match the durable child seed"
                                .into(),
                    },
                    None,
                    "runtime",
                ));
            }
            let turn_start_ms = agent
                .control_session_handle(child_session_id)
                .and_then(|handle| {
                    handle
                        .runtime_observation
                        .read()
                        .running
                        .as_ref()
                        .map(|(_, started_at)| *started_at)
                })
                .unwrap_or(inspection.snapshot.started_at_epoch_ms);
            let metadata = match &inspection.snapshot.status {
                tools::implementations::grow_build::task::types::SubagentSnapshotStatus::FinalizationPending { outcome, error } => Some(serde_json::json!({
                    "executionOutcome": outcome, "commitState": "pending", "finalizationError": error,
                })),
                _ => None,
            };
            let status =
                prompt_status_from_subagent_inspection(inspection.snapshot.status, turn_start_ms);
            Some((status, metadata, "runtime"))
        }
    }
}

fn read_durable_subagent_lifecycle(
    child_session_id: String,
    require_subagent: bool,
) -> std::io::Result<DurableSubagentLifecycle> {
    use chat_state::{
        NotificationEvent, NotificationSource, SubagentEvent, SubagentOutcome, TimelineEventKind,
    };

    let storage = JsonlStorageAdapter::with_root(crate::util::grow_home::grow_home());
    let Some(child) = storage.open_session_by_id_shared_read_bounded(&child_session_id)? else {
        if require_subagent {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "live subagent session has no durable entity",
            ));
        }
        return Ok(DurableSubagentLifecycle::NotSubagent);
    };
    let summary = child.summary();
    if summary.info.id.0.as_ref() != child_session_id {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "child session identity did not match the requested id",
        ));
    }
    if !summary
        .session_kind
        .as_deref()
        .is_some_and(|kind| kind.starts_with("subagent"))
    {
        if require_subagent {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "live child session is not identified as a subagent",
            ));
        }
        return Ok(DurableSubagentLifecycle::NotSubagent);
    }
    let child_timeline = child
        .bounded_validated_timeline(&child_session_id)?
        .timeline;
    let seeds: Vec<_> = child_timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::SubagentSeed(seed) => Some(seed),
            _ => None,
        })
        .collect();
    if seeds.len() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "subagent Timeline must contain exactly one durable seed",
        ));
    }
    let seed = seeds[0];
    if summary.parent_session_id.as_deref() != Some(seed.parent_timeline_id.as_str()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "child session parent identity did not match its durable seed",
        ));
    }
    let Some(parent) = storage.open_session_by_id_shared_read_bounded(&seed.parent_timeline_id)?
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "subagent seed parent session is unavailable",
        ));
    };
    if parent.summary().info.id.0.as_ref() != seed.parent_timeline_id {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "parent Timeline identity did not match the child seed",
        ));
    }
    let parent_timeline = parent
        .bounded_validated_timeline(&seed.parent_timeline_id)?
        .timeline;
    let spawn_event = parent_timeline.events().iter().find(|event| {
        event.seq.get() == seed.parent_spawn_seq
            && matches!(
                &event.kind,
                TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                    if spawn.subagent_id == seed.subagent_id
                        && spawn.child_session_id == child_session_id
            )
    });
    let Some(spawn_event) = spawn_event else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "subagent seed does not resolve to its exact parent spawn",
        ));
    };
    let TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) = &spawn_event.kind else {
        unreachable!("spawn event was matched above")
    };
    child_timeline
        .validate_subagent_seed_link(&seed.parent_timeline_id, spawn_event.seq, spawn)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let terminal = parent_timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(terminal))
                if terminal.subagent_id == seed.subagent_id =>
            {
                Some(terminal)
            }
            _ => None,
        });
    let Some(terminal) = terminal else {
        return Ok(DurableSubagentLifecycle::AwaitingParentEnd {
            parent_timeline_id: seed.parent_timeline_id.clone(),
            subagent_id: seed.subagent_id.clone(),
        });
    };
    let result = match child_timeline.validate_subagent_result_link(
        &seed.parent_timeline_id,
        spawn_event.seq,
        spawn,
        terminal,
    ) {
        Ok(result) => result,
        Err(chat_state::TimelineError::MissingSubagentResult) => {
            return Ok(DurableSubagentLifecycle::Settling);
        }
        Err(error) => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    };

    let mut receipts = Vec::new();
    for event in parent_timeline.events() {
        if let TimelineEventKind::Notification(NotificationEvent::Received {
            id,
            owner_session_id,
            source: NotificationSource::SubagentCompleted { subagent_id, .. },
            ..
        }) = &event.kind
            && subagent_id == &seed.subagent_id
        {
            if owner_session_id != &seed.parent_timeline_id {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "subagent completion receipt has the wrong owner session",
                ));
            }
            receipts.push(id.as_str());
        }
    }
    let resolved: HashSet<&str> = parent_timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                notification_ids,
                ..
            })
            | TimelineEventKind::Notification(NotificationEvent::Dismissed {
                notification_ids,
                ..
            }) => Some(notification_ids.iter().map(String::as_str)),
            _ => None,
        })
        .flatten()
        .collect();
    if !subagent_completion_receipt_settled(
        spawn.surface_completion,
        result.outcome,
        &receipts,
        &resolved,
    ) {
        return Ok(DurableSubagentLifecycle::Settling);
    }

    let stop_reason = match result.outcome {
        SubagentOutcome::Completed => "completed",
        SubagentOutcome::Failed => "failed",
        SubagentOutcome::Cancelled => "cancelled",
    }
    .to_owned();
    let metadata = serde_json::json!({
        "cwd": summary.info.cwd,
        "title": summary.title,
        "messageCount": summary.num_messages,
        "timelineEventCount": child_timeline.events().len(),
    });
    Ok(DurableSubagentLifecycle::Terminal {
        stop_reason,
        metadata,
    })
}

fn prompt_status_from_subagent_inspection(
    status: tools::implementations::grow_build::task::types::SubagentSnapshotStatus,
    started_at_epoch_ms: u64,
) -> crate::session::prompt_queue::PromptStatus {
    use crate::session::prompt_queue::PromptStatus;
    use tools::implementations::grow_build::task::types::SubagentSnapshotStatus;

    match status {
        SubagentSnapshotStatus::Running { .. } | SubagentSnapshotStatus::Initializing => {
            PromptStatus::Running {
                turn_start_ms: started_at_epoch_ms,
            }
        }
        SubagentSnapshotStatus::Suspended { .. } => PromptStatus::Settling,
        SubagentSnapshotStatus::FinalizationPending { .. }
        | SubagentSnapshotStatus::Completed { .. }
        | SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
        | SubagentSnapshotStatus::Failed { .. }
        | SubagentSnapshotStatus::Cancelled { .. } => PromptStatus::Settling,
    }
}

fn subagent_completion_receipt_settled(
    surface_completion: bool,
    outcome: chat_state::SubagentOutcome,
    receipts: &[&str],
    resolved_receipts: &HashSet<&str>,
) -> bool {
    if receipts.is_empty() {
        return !surface_completion || outcome == chat_state::SubagentOutcome::Cancelled;
    }
    receipts
        .iter()
        .all(|receipt_id| resolved_receipts.contains(receipt_id))
}

const RUNTIME_OBSERVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn runtime_observation_workers() -> Arc<tokio::sync::Semaphore> {
    static WORKERS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(4)))
        .clone()
}

#[derive(Debug)]
pub(crate) enum BoundedSharedReadError {
    TimedOut,
    Io(std::io::Error),
    Worker(String),
}

/// Run identity-pinned, read-only JSONL work under a shared global budget.
/// The blocking worker owns its permit, so caller timeout never releases a
/// permit while the underlying file read remains active.
pub(crate) async fn bounded_shared_read<T, F>(work: F) -> Result<T, BoundedSharedReadError>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    bounded_shared_read_with_workers(runtime_observation_workers(), work).await
}

async fn bounded_shared_read_with_workers<T, F>(
    workers: Arc<tokio::sync::Semaphore>,
    work: F,
) -> Result<T, BoundedSharedReadError>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    tokio::time::timeout(RUNTIME_OBSERVATION_TIMEOUT, async move {
        let permit = workers
            .acquire_owned()
            .await
            .map_err(|error| BoundedSharedReadError::Worker(error.to_string()))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|error| BoundedSharedReadError::Worker(error.to_string()))?
        .map_err(BoundedSharedReadError::Io)
    })
    .await
    .map_err(|_| BoundedSharedReadError::TimedOut)?
}

enum DurablePromptObservation {
    Absent,
    Found {
        status: crate::session::prompt_queue::PromptStatus,
        metadata: serde_json::Value,
    },
}

fn read_durable_prompt_status(
    session_id: acp::SessionId,
    prompt_id: Option<&str>,
) -> std::io::Result<DurablePromptObservation> {
    use chat_state::{TimelineEventKind, TurnEvent};

    let storage = JsonlStorageAdapter::with_root(crate::util::grow_home::grow_home());
    let Some(opened) = storage.open_session_by_id_shared_read_bounded(session_id.0.as_ref())?
    else {
        return Ok(DurablePromptObservation::Absent);
    };
    let validated = opened.bounded_validated_timeline(session_id.0.as_ref())?;
    let mut active_turn: Option<(chat_state::TurnId, bool)> = None;
    let mut latest_terminal: Option<String> = None;
    let mut matching_terminal: Option<String> = None;
    for event in validated.timeline.events() {
        match &event.kind {
            TimelineEventKind::Turn(TurnEvent::Started { id, input_ids, .. }) => {
                active_turn = Some((
                    *id,
                    prompt_id.is_some_and(|id| input_ids.iter().any(|input| input == id)),
                ));
                latest_terminal = None;
                matching_terminal = None;
            }
            TimelineEventKind::Turn(TurnEvent::Ended { id, terminal, .. }) => {
                if active_turn
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == id)
                {
                    let matched = active_turn.as_ref().is_some_and(|(_, matched)| *matched);
                    latest_terminal = Some(terminal.stop_reason.clone());
                    if matched {
                        matching_terminal = Some(terminal.stop_reason.clone());
                    }
                    active_turn = None;
                }
            }
            _ => {}
        }
    }

    let status = if let Some(prompt_id) = prompt_id {
        if active_turn.as_ref().is_some_and(|(_, matched)| *matched) {
            crate::session::prompt_queue::PromptStatus::Unavailable {
                reason:
                    "durable Timeline shows no terminal for this prompt; runtime is not resident"
                        .into(),
            }
        } else if let Some(stop_reason) = matching_terminal {
            crate::session::prompt_queue::PromptStatus::Terminal {
                stop_reason,
                agent_result: None,
            }
        } else {
            let _ = prompt_id;
            crate::session::prompt_queue::PromptStatus::Unavailable {
                reason: "durable Timeline cannot prove this prompt's runtime status".into(),
            }
        }
    } else if active_turn.is_some() {
        crate::session::prompt_queue::PromptStatus::Unavailable {
            reason: "durable Timeline contains an open turn; runtime is not resident".into(),
        }
    } else if let Some(stop_reason) = latest_terminal {
        crate::session::prompt_queue::PromptStatus::Terminal {
            stop_reason,
            agent_result: None,
        }
    } else {
        crate::session::prompt_queue::PromptStatus::Unknown
    };
    let summary = opened.summary();
    let metadata = serde_json::json!({
        "cwd": summary.info.cwd,
        "title": summary.title,
        "messageCount": summary.num_messages,
        "timelineEventCount": validated.timeline.events().len(),
    });
    Ok(DurablePromptObservation::Found { status, metadata })
}

fn prompt_status_response(
    session_id: &acp::SessionId,
    control_epoch: Option<&str>,
    source: &str,
    status: crate::session::prompt_queue::PromptStatus,
    durable_metadata: Option<serde_json::Value>,
) -> ExtResult {
    let mut response = serde_json::to_value(status)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
    let object = response.as_object_mut().ok_or_else(|| {
        acp::Error::internal_error().data("prompt status response was not an object")
    })?;
    object.insert("sessionId".into(), serde_json::json!(session_id.0.as_ref()));
    object.insert(
        "controlEpoch".into(),
        control_epoch.map_or(serde_json::Value::Null, |epoch| serde_json::json!(epoch)),
    );
    object.insert("source".into(), serde_json::json!(source));
    if let Some(metadata) = durable_metadata {
        object.insert("durableMetadata".into(), metadata);
    }
    to_raw_response(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::info::Info;
    use crate::session::persistence::default_model_id;
    use crate::session::storage::StorageAdapter as _;
    use tempfile::TempDir;
    use tools::implementations::grow_build::task::types::SubagentSnapshotStatus;

    #[test]
    fn completed_plan_preview_resolves_until_the_result_is_cleared() {
        let mut behavior = crate::session::behavior::BehaviorCoordinator::new();
        behavior.select_behavior(tool_types::BehaviorId::Plan);
        behavior.record_plan_artifact("# approved");
        assert!(behavior.submit_initial_plan());
        assert!(behavior.approve_submitted_plan());
        let before = plan_preview_reference(&behavior.snapshot()).unwrap();
        assert!(behavior.complete_plan(blake3::hash(b"report").to_hex().to_string()));
        assert_eq!(plan_preview_reference(&behavior.snapshot()), Some(before));
        behavior.clear_completed_plan();
        assert!(plan_preview_reference(&behavior.snapshot()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn cwd_matches_resolves_aliases_without_matching_sibling_directories() {
        let root = TempDir::new().unwrap();
        let project = root.path().join("project");
        let alias = root.path().join("alias");
        let sibling = root.path().join("project-other");
        std::fs::create_dir_all(project.join("nested")).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        std::os::unix::fs::symlink(&project, &alias).unwrap();
        assert!(cwd_matches(&alias.join("nested"), &project));
        assert!(cwd_matches(&project.join("nested"), &alias));
        assert!(!cwd_matches(&sibling, &alias));
        assert!(!cwd_matches(&sibling, &project));
    }

    #[derive(Clone, Copy)]
    enum InputFixtureState {
        Submitted,
        Allowed,
        Blocked,
        Consumed,
        Handled,
        Dismissed,
    }

    async fn input_status_fixture(
        root: &std::path::Path,
        session_id: &str,
        prompt_id: &str,
        state: InputFixtureState,
        include_image: bool,
    ) -> (Info, std::path::PathBuf, chat_state::InputPayloadRef) {
        use agent_client_protocol::schema::v1::{ContentBlock, ImageContent, TextContent};
        use chat_state::{InputEvent, Timeline, TimelineEventKind};

        let info = Info {
            id: acp::SessionId::new(session_id),
            cwd: format!("/fixture/{session_id}"),
        };
        let writer = JsonlStorageAdapter::with_root(root.to_path_buf());
        writer
            .init_session(&info, default_model_id())
            .await
            .expect("fixture session");
        let directory = writer
            .open_session(&info)
            .expect("writer handle")
            .directory_handle();
        let mut prompt_blocks = vec![ContentBlock::Text(TextContent::new("durable prompt"))];
        if include_image {
            use base64::Engine as _;
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgba8(1, 1)
                .write_to(&mut png, image::ImageFormat::Png)
                .expect("valid fixture image");
            prompt_blocks.push(ContentBlock::Image(ImageContent::new(
                base64::engine::general_purpose::STANDARD.encode(png.into_inner()),
                "image/png",
            )));
        }
        let payload_ref = crate::session::input_inbox::write_payload(
            &directory,
            &crate::session::input_inbox::InputPayload::Prompt {
                prompt_id: prompt_id.into(),
                requested_route: crate::session::input_inbox::InputRequestRoute::Fifo,
                prompt_blocks,
                skill_invocations: Vec::new(),
                client_identifier: None,
                screen_mode: None,
                verbatim: false,
                json_schema: None,
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                queue: None,
            },
        )
        .expect("durable prompt payload");

        let mut timeline = Timeline::default();
        let input_id = format!("input-{session_id}");
        let event = timeline
            .record(TimelineEventKind::Input(InputEvent::Submitted {
                input_id: input_id.clone(),
                intent: chat_state::InputIntent::Prompt,
                payload_ref: payload_ref.clone(),
            }))
            .expect("valid input event");
        writer
            .append_timeline_event_durable(&info, &event)
            .await
            .expect("persist submission");

        if !matches!(state, InputFixtureState::Submitted) {
            let occurrence_id = format!("hook-{session_id}");
            let trigger = timeline
                .record(TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    occurrence_id: occurrence_id.clone(),
                    event: chat_state::HookEventType::UserPromptSubmit,
                    gate: chat_state::HookGateKind::Prompt,
                    cause: chat_state::HookCause::Input {
                        input_id: input_id.clone(),
                    },
                    config_generation: 1,
                    handlers: if matches!(state, InputFixtureState::Blocked) {
                        vec![chat_state::HookHandlerPlan {
                            index: 0,
                            run_id: "fixture-policy".into(),
                            name: "fixture policy".into(),
                            provenance: chat_state::HookHandlerProvenance::ProjectFile,
                            kind: chat_state::HookHandlerKind::Command,
                            failure_policy: chat_state::HookFailurePolicy::Allow,
                            action: chat_state::HookHandlerPlanAction::Execute,
                        }]
                    } else {
                        Vec::new()
                    },
                }))
                .expect("hook trigger");
            writer
                .append_timeline_event_durable(&info, &trigger)
                .await
                .expect("persist hook trigger");
            let hook_decision = if matches!(state, InputFixtureState::Blocked) {
                let started = timeline
                    .record(TimelineEventKind::Hook(chat_state::HookEvent::RunStarted {
                        occurrence_id: occurrence_id.clone(),
                        run_id: "fixture-policy".into(),
                        handler_index: 0,
                    }))
                    .expect("policy hook started");
                writer
                    .append_timeline_event_durable(&info, &started)
                    .await
                    .unwrap();
                let finished = timeline
                    .record(TimelineEventKind::Hook(
                        chat_state::HookEvent::RunFinished {
                            occurrence_id: occurrence_id.clone(),
                            run_id: "fixture-policy".into(),
                            handler_index: 0,
                            elapsed_ms: 1,
                            outcome: chat_state::HookRunOutcome::Blocked,
                            control: chat_state::HookRunControl::Block {
                                reason: "fixture denied".into(),
                            },
                        },
                    ))
                    .expect("policy hook blocked");
                writer
                    .append_timeline_event_durable(&info, &finished)
                    .await
                    .unwrap();
                chat_state::HookGateDecision::Block {
                    reason: "fixture denied".into(),
                }
            } else {
                chat_state::HookGateDecision::Allow
            };
            let completed = timeline
                .record(TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                    occurrence_id,
                    decision: chat_state::HookAggregateDecision::Prompt {
                        decision: hook_decision.clone(),
                    },
                }))
                .expect("hook completion");
            writer
                .append_timeline_event_durable(&info, &completed)
                .await
                .expect("persist hook completion");
            let (decision, route) = if matches!(state, InputFixtureState::Blocked) {
                (
                    chat_state::InputAdmissionDecision::Block {
                        reason: chat_state::InputBlockReason::Hook {
                            reason: "fixture denied".into(),
                        },
                    },
                    None,
                )
            } else {
                (
                    chat_state::InputAdmissionDecision::Allow,
                    Some(chat_state::InputRoute::Fifo),
                )
            };
            let admitted = timeline
                .record(TimelineEventKind::Input(InputEvent::AdmissionResolved {
                    input_id: input_id.clone(),
                    decision,
                    route,
                    supersedes: Vec::new(),
                }))
                .expect("input admission");
            writer
                .append_timeline_event_durable(&info, &admitted)
                .await
                .expect("persist input admission");

            match state {
                InputFixtureState::Submitted
                | InputFixtureState::Allowed
                | InputFixtureState::Blocked => {}
                InputFixtureState::Dismissed => {
                    let dismissed = timeline
                        .record(TimelineEventKind::Input(InputEvent::Dismissed {
                            input_ids: vec![input_id],
                            reason: chat_state::InputDismissReason::UserRemoved,
                        }))
                        .expect("input dismissal");
                    writer
                        .append_timeline_event_durable(&info, &dismissed)
                        .await
                        .expect("persist input dismissal");
                }
                InputFixtureState::Consumed | InputFixtureState::Handled => {
                    let turn = chat_state::TurnId(1);
                    let started = timeline
                        .record(TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                            prompt_id: "fixture-prompt".into(),
                            id: turn,
                            input_ids: vec![input_id.clone()],
                            identity: chat_state::TurnIdentity {
                                origin: "user".into(),
                                turn_kind: "user".into(),
                                goal_id: None,
                                goal_definition_revision: None,
                                stage_id: None,
                            },
                            model_id: "test-model".into(),
                            input_message_count: 0,
                            prompt_index: 0,
                            prompt_text: "durable prompt".into(),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            redirect_kind: None,
                        }))
                        .expect("turn start");
                    writer
                        .append_timeline_event_durable(&info, &started)
                        .await
                        .expect("persist turn start");
                    let terminal_input = match state {
                        InputFixtureState::Consumed => InputEvent::Consumed {
                            display_echo: None,
                            input_ids: vec![input_id],
                            turn,
                            item: sampling_types::ConversationItem::user("durable prompt"),
                            skill_loads: Vec::new(),
                        },
                        InputFixtureState::Handled => InputEvent::Handled {
                            input_ids: vec![input_id],
                            turn,
                        },
                        _ => unreachable!(),
                    };
                    let event = timeline
                        .record(TimelineEventKind::Input(terminal_input))
                        .expect("input terminal fact");
                    writer
                        .append_timeline_event_durable(&info, &event)
                        .await
                        .expect("persist input terminal fact");
                }
            }
        }

        (info, directory.display_path().to_path_buf(), payload_ref)
    }

    #[tokio::test]
    async fn durable_input_status_maps_isolated_timeline_lifecycles() {
        let root = TempDir::new().expect("isolated input root");
        assert!(matches!(
            read_durable_input_status(
                acp::SessionId::new("absent-session"),
                "prompt-absent".into(),
                root.path().to_path_buf(),
            )
            .unwrap(),
            DurableInputObservation::Absent
        ));

        let cases = [
            (InputFixtureState::Submitted, "submitted"),
            (InputFixtureState::Allowed, "allowed"),
            (InputFixtureState::Blocked, "blocked"),
            (InputFixtureState::Consumed, "consumed"),
            (InputFixtureState::Handled, "handled"),
            (InputFixtureState::Dismissed, "dismissed"),
        ];
        for (state, suffix) in cases {
            let session_id = format!("input-{suffix}");
            let prompt_id = format!("prompt-{suffix}");
            let (info, _, _) =
                input_status_fixture(root.path(), &session_id, &prompt_id, state, false).await;
            let observation =
                match read_durable_input_status(info.id, prompt_id, root.path().to_path_buf())
                    .expect("valid isolated timeline")
                {
                    DurableInputObservation::Found(status) => status,
                    DurableInputObservation::Absent => panic!("fixture session must be found"),
                };
            match state {
                InputFixtureState::Submitted => {
                    assert!(matches!(observation, InputAdmissionObservation::Submitted));
                }
                InputFixtureState::Allowed => assert!(matches!(
                    observation,
                    InputAdmissionObservation::Allowed {
                        route: chat_state::InputRoute::Fifo
                    }
                )),
                InputFixtureState::Blocked => assert!(matches!(
                    observation,
                    InputAdmissionObservation::Blocked {
                        reason: chat_state::InputBlockReason::Hook { reason }
                    } if reason == "fixture denied"
                )),
                InputFixtureState::Consumed => assert!(matches!(
                    observation,
                    InputAdmissionObservation::Consumed {
                        route: chat_state::InputRoute::Fifo
                    }
                )),
                InputFixtureState::Handled => {
                    assert!(matches!(observation, InputAdmissionObservation::Handled));
                }
                InputFixtureState::Dismissed => assert!(matches!(
                    observation,
                    InputAdmissionObservation::Dismissed {
                        reason: chat_state::InputDismissReason::UserRemoved
                    }
                )),
            }
        }

        let (info, _, _) = input_status_fixture(
            root.path(),
            "input-unknown",
            "prompt-submitted",
            InputFixtureState::Submitted,
            false,
        )
        .await;
        assert!(matches!(
            read_durable_input_status(
                info.id,
                "prompt-with-no-submission".into(),
                root.path().to_path_buf(),
            )
            .unwrap(),
            DurableInputObservation::Found(InputAdmissionObservation::UnknownUnconfirmed)
        ));
    }

    #[tokio::test]
    async fn durable_input_status_rejects_duplicate_prompt_and_missing_payload_artifacts() {
        use chat_state::{InputEvent, Timeline, TimelineEventKind};

        let root = TempDir::new().expect("isolated input root");
        let (info, _directory, first_ref) = input_status_fixture(
            root.path(),
            "input-duplicate-first",
            "same-prompt",
            InputFixtureState::Submitted,
            false,
        )
        .await;
        let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let opened = writer.open_session(&info).expect("writer handle");
        let second_ref = crate::session::input_inbox::write_payload(
            opened.directory(),
            &crate::session::input_inbox::InputPayload::Prompt {
                prompt_id: "same-prompt".into(),
                requested_route: crate::session::input_inbox::InputRequestRoute::Fifo,
                prompt_blocks: vec![agent_client_protocol::schema::v1::ContentBlock::Text(
                    agent_client_protocol::schema::v1::TextContent::new("second"),
                )],
                skill_invocations: Vec::new(),
                client_identifier: None,
                screen_mode: None,
                verbatim: false,
                json_schema: None,
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                queue: None,
            },
        )
        .expect("second prompt payload");
        let mut timeline = Timeline::from_events(vec![chat_state::TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: chat_state::EventSeq::new(0),
            at_ms: 1,
            kind: TimelineEventKind::Input(InputEvent::Submitted {
                input_id: "input-input-duplicate-first".into(),
                intent: chat_state::InputIntent::Prompt,
                payload_ref: first_ref,
            }),
        }])
        .expect("first submitted event");
        let second = timeline
            .record(TimelineEventKind::Input(InputEvent::Submitted {
                input_id: "input-duplicate-second".into(),
                intent: chat_state::InputIntent::Prompt,
                payload_ref: second_ref,
            }))
            .expect("second submitted event");
        writer
            .append_timeline_event_durable(&info, &second)
            .await
            .expect("persist second submission");
        assert!(matches!(
            read_durable_input_status(
                info.id.clone(),
                "same-prompt".into(),
                root.path().to_path_buf(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData
        ));

        // A missing prompt manifest and an image blob referenced by an
        // otherwise valid manifest both invalidate the durable evidence.
        let (missing_info, missing_dir, missing_ref) = input_status_fixture(
            root.path(),
            "input-missing-manifest",
            "prompt-missing-manifest",
            InputFixtureState::Submitted,
            false,
        )
        .await;
        std::fs::remove_file(
            std::path::Path::new(&missing_dir)
                .join("artifacts/inputs")
                .join(format!("{}.json", missing_ref.blake3)),
        )
        .expect("remove prompt manifest");
        assert!(matches!(
            read_durable_input_status(
                missing_info.id,
                "prompt-missing-manifest".into(),
                root.path().to_path_buf(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData
        ));

        let (image_info, image_dir, _) = input_status_fixture(
            root.path(),
            "input-missing-image",
            "prompt-missing-image",
            InputFixtureState::Submitted,
            true,
        )
        .await;
        let images =
            std::fs::read_dir(std::path::Path::new(&image_dir).join("artifacts/inputs/images"))
                .expect("image artifact directory");
        let image_path = images
            .map(|entry| entry.expect("image entry").path())
            .next()
            .expect("image artifact");
        std::fs::remove_file(image_path).expect("remove image artifact");
        assert!(matches!(
            read_durable_input_status(
                image_info.id,
                "prompt-missing-image".into(),
                root.path().to_path_buf(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData
        ));
    }

    #[test]
    fn coordinator_running_states_map_to_running_and_settlement_states_do_not_end_lifecycle() {
        assert!(matches!(
            prompt_status_from_subagent_inspection(SubagentSnapshotStatus::Initializing, 42,),
            crate::session::prompt_queue::PromptStatus::Running { turn_start_ms: 42 }
        ));
        assert!(matches!(
            prompt_status_from_subagent_inspection(
                SubagentSnapshotStatus::Running {
                    turn_count: 1,
                    tool_call_count: 0,
                    tokens_used: 0,
                    context_window_tokens: 0,
                    context_usage_pct: 0,
                    tools_used: Vec::new(),
                    error_count: 0,
                },
                84,
            ),
            crate::session::prompt_queue::PromptStatus::Running { turn_start_ms: 84 }
        ));
        assert!(matches!(
            prompt_status_from_subagent_inspection(
                SubagentSnapshotStatus::FinalizationPending {
                    outcome: "completed".into(),
                    error: None,
                },
                0,
            ),
            crate::session::prompt_queue::PromptStatus::Settling
        ));
        assert!(matches!(
            prompt_status_from_subagent_inspection(
                SubagentSnapshotStatus::Suspended {
                    operation_id: "operation-1".into(),
                    checkpoint_seq: 7,
                },
                0,
            ),
            crate::session::prompt_queue::PromptStatus::Settling
        ));
        assert!(matches!(
            prompt_status_from_subagent_inspection(
                SubagentSnapshotStatus::Completed {
                    output: "done".into(),
                    tool_calls: 0,
                    turns: 1,
                    tokens_used: 0,
                    worktree_path: None,
                },
                0,
            ),
            crate::session::prompt_queue::PromptStatus::Settling
        ));
    }

    #[test]
    fn completion_receipt_must_be_admitted_and_resolved_when_surfaceable() {
        let no_receipts = [];
        let resolved = HashSet::new();
        assert!(!subagent_completion_receipt_settled(
            true,
            chat_state::SubagentOutcome::Completed,
            &no_receipts,
            &resolved,
        ));
        assert!(subagent_completion_receipt_settled(
            false,
            chat_state::SubagentOutcome::Failed,
            &no_receipts,
            &resolved,
        ));
        assert!(subagent_completion_receipt_settled(
            true,
            chat_state::SubagentOutcome::Cancelled,
            &no_receipts,
            &resolved,
        ));

        let receipts = ["receipt-1"];
        assert!(!subagent_completion_receipt_settled(
            true,
            chat_state::SubagentOutcome::Completed,
            &receipts,
            &resolved,
        ));
        let resolved = HashSet::from(["receipt-1"]);
        assert!(subagent_completion_receipt_settled(
            true,
            chat_state::SubagentOutcome::Completed,
            &receipts,
            &resolved,
        ));
    }
}

fn handle_reload_announcements(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct ReloadAnnouncements {
        announcements: Vec<announcements::Announcement>,
    }

    let request: ReloadAnnouncements = parse_params(args)?;
    let count = request.announcements.len();
    agent.cfg.borrow_mut().announcements = request.announcements;
    agent.emit_announcements();
    ExtMethodResult::success(serde_json::json!({ "announcements": count }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// session/rename

/// Handles renaming a session.
async fn handle_session_rename(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RenameRequest {
        session_id: String,
        title: String,
        #[serde(default)]
        cwd: Option<String>,
    }

    let mut req: RenameRequest = parse_params(args)?;
    // Timeline title validation is repeated at this protocol boundary so the
    // caller receives a precise request error before any storage work begins.
    req.title = req.title.trim().to_string();
    if req.title.is_empty() {
        return Err(acp::Error::invalid_request().data("title must not be blank"));
    }
    if req.title.chars().count() > 160 {
        return Err(acp::Error::invalid_request().data("title must be at most 160 characters"));
    }

    let session_id = acp::SessionId::new(Arc::from(req.session_id.as_str()));

    // Keep lookup and the live/dormant write decision atomic with load/delete.
    let _lifecycle = agent.lock_session_lifecycle(&session_id).await;

    // Find the session info, scoping to cwd if provided
    let summaries = list_summaries(req.cwd.as_deref())
        .await
        .map_err(|e| acp::Error::internal_error().data(format!("failed to list sessions: {e}")))?;

    let summary = summaries
        .iter()
        .find(|s| s.info.id == session_id)
        .ok_or_else(|| {
            acp::Error::invalid_request().data(format!("session not found: {}", req.session_id))
        })?;

    let info = summary.info.clone();

    // An announced loader may be queued behind our guard. Waiting for it here
    // would wait on ourselves; residency is stable while this guard is held.
    let live_handle = agent.get_session_handle(&session_id);
    let event = if let Some(handle) = live_handle {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        handle
            .cmd_tx
            .send(crate::session::commands::SessionCommand::SetSessionTitle {
                title: req.title.clone(),
                respond_to,
            })
            .map_err(|_| acp::Error::internal_error().data("session actor unavailable"))?;
        response
            .await
            .map_err(|_| {
                acp::Error::internal_error().data("session actor dropped title acknowledgement")
            })?
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("failed to commit session/title event: {error}"))
            })?
    } else {
        let storage = JsonlStorageAdapter::default();

        storage
            .append_session_title_durable(&info, req.title.clone())
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("failed to commit session/title event: {error}"))
            })?
    };

    // Update session search index with new title
    crate::session::storage::search::notify_session_updated(&info.id.to_string(), &info.cwd);

    tracing::info!(session_id = %req.session_id, title = %req.title, "Session renamed");

    to_raw_response(&serde_json::json!({
        "success": true,
        "title": req.title,
        "eventSeq": event.seq.get(),
    }))
}

// session/delete

/// Delete a session from history.
async fn handle_session_delete(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteRequest {
        session_id: String,
        #[serde(default)]
        cwd: Option<String>,
    }

    let req: DeleteRequest = parse_params(args)?;

    let session_id = acp::SessionId::new(Arc::from(req.session_id.as_str()));

    // Tear down any live actor first (cancel turn/subagents/bg tasks,
    // process-scope kill, flush). Then wipe history so shutdown cannot
    // rewrite the session directory after delete.
    let _delete_lifecycle = agent
        .teardown_live_session_before_delete(&session_id)
        .await
        .map_err(|error| acp::Error::internal_error().data(error))?;

    // Local delete: disk + FTS eviction.
    // Mirrored by the `grow sessions delete <id>` CLI path.
    crate::session::persistence::delete_session_history(&req.session_id, req.cwd.as_deref())
        .await
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    tracing::info!(session_id = %req.session_id, "Session deleted");

    to_raw_response(&serde_json::json!({ "success": true }))
}

// session/update_mcp_servers

async fn handle_update_mcp_servers(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Params {
        session_id: acp::SessionId,
        mcp_servers: Vec<acp::McpServer>,
    }

    let params: Params = parse_params(args)?;
    ::mcp::servers::validate_mcp_server_names(&params.mcp_servers)
        .map_err(|error| acp::Error::invalid_params().data(error))?;
    let _lifecycle_guard = agent.lock_session_lifecycle(&params.session_id).await;

    let (handle, cwd) = {
        let sessions = agent.sessions.borrow();
        let h = sessions
            .get(&params.session_id)
            .cloned()
            .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
        let cwd = std::path::PathBuf::from(&h.info.cwd);
        (h, cwd)
    };

    let merged = crate::session::mcp_catalog::merge_mcp_servers(
        params.mcp_servers.clone(),
        &cwd,
        agent.plugin_registry_handle().snapshot().as_deref(),
    );

    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::UpdateMcpServers {
            mcp_servers: merged,
            respond_to: tx,
        })
        .map_err(|_| acp::Error::internal_error().data("session closed"))?;

    // Command admission and client-source replacement must not yield: a disk
    // reload during initialization must merge the client's latest intent.
    if let Some(h) = agent.sessions.borrow_mut().get_mut(&params.session_id) {
        h.initial_client_mcp_servers = params.mcp_servers;
    }

    // Wait for the session actor to finish MCP re-initialization.
    rx.await
        .map_err(|_| acp::Error::internal_error().data("session closed"))?
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    ExtMethodResult::success(serde_json::json!({ "ok": true }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// internal/reload_skills

/// Reload skills for ALL active sessions. Called by the skills file watcher
fn handle_reload_skills(agent: &MvpAgent) -> ExtResult {
    let reloaded = agent.reload_skills_all_sessions();
    ExtMethodResult::success(serde_json::json!({ "reloaded": reloaded }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

fn handle_reload_workflows(agent: &MvpAgent) -> ExtResult {
    let reloaded = agent.advertise_commands_all_sessions();
    ExtMethodResult::success(serde_json::json!({ "reloaded": reloaded }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// internal/reload_mcp_catalog

/// Reload the canonical MCP catalog. `project_root = None` targets every
/// active session; `Some(root)` targets sessions at or beneath that root.
pub(crate) async fn handle_reload_mcp_catalog(
    agent: &MvpAgent,
    args: &acp::ExtRequest,
) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        project_root: serde_json::Value,
    }

    let params: Params = parse_params(args)?;
    let project_root = match params.project_root {
        serde_json::Value::Null => None,
        serde_json::Value::String(root) => Some(std::path::PathBuf::from(root)),
        _ => {
            return Err(acp::Error::invalid_params().data("projectRoot must be a string or null"));
        }
    };

    // Capture both owner sessions and active children before any async send.
    // Child actors have their own cwd-scoped auxiliary policy, even though
    // their MCP clients and catalog remain owned by the parent.
    let matches_scope = |cwd: &std::path::Path| {
        project_root
            .as_deref()
            .is_none_or(|root| cwd_matches(cwd, root))
    };
    let mut targets: Vec<(
        acp::SessionId,
        std::path::PathBuf,
        bool,
        crate::session::SessionHandle,
    )> = agent
        .sessions
        .borrow()
        .iter()
        .map(|(sid, handle)| {
            (
                sid.clone(),
                std::path::PathBuf::from(&handle.info.cwd),
                false,
                handle.clone(),
            )
        })
        .filter(|(_, cwd, _, _)| matches_scope(cwd))
        .collect();
    targets.extend(
        agent
            .active_child_sessions
            .borrow()
            .iter()
            .map(|(sid, handle)| {
                (
                    sid.clone(),
                    std::path::PathBuf::from(&handle.info.cwd),
                    true,
                    handle.clone(),
                )
            })
            .filter(|(_, cwd, _, _)| matches_scope(cwd)),
    );

    // A child can briefly be present in both registries while its lifecycle
    // handoff completes. One config reload must enqueue at most one refresh.
    let mut seen = HashSet::new();
    targets.retain(|(session_id, _, _, _)| seen.insert(session_id.clone()));

    if targets.is_empty() {
        return ExtMethodResult::success(serde_json::json!({ "updated": 0 }))
            .to_ext_response()
            .map_err(|e| acp::Error::internal_error().data(e.to_string()));
    }

    let mut updated = 0u32;
    let plugin_registry = agent.plugin_registry_handle().snapshot();
    for (_session_id, cwd, is_child, handle) in &targets {
        let sent = if *is_child {
            let (respond_to, _response) = tokio::sync::oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::UpdateMcpServers {
                    mcp_servers: Vec::new(),
                    respond_to,
                })
                .is_ok()
        } else {
            crate::session::mcp_catalog::merge_and_send_mcp_update(
                &handle.cmd_tx,
                cwd,
                handle.initial_client_mcp_servers.clone(),
                plugin_registry.as_deref(),
            )
        };
        if sent {
            updated += 1;
        }
    }

    tracing::info!(
        updated,
        total = targets.len(),
        project_root = ?project_root,
        "reloaded MCP catalog for matching sessions"
    );
    ExtMethodResult::success(serde_json::json!({ "updated": updated }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

/// Returns `true` iff `session_cwd` equals `target_cwd` or sits
/// beneath it (so a `<repo>/` edit reloads `<repo>/subdir/` sessions
/// too).
///
/// Resolve directory aliases before component-aware comparison: filesystem
/// events can use a canonical path while SessionInfo retains a symlink cwd.
/// Keep the original path if a directory is no longer resolvable.
fn cwd_matches(session_cwd: &std::path::Path, target_cwd: &std::path::Path) -> bool {
    let session_cwd =
        dunce::canonicalize(session_cwd).unwrap_or_else(|_| session_cwd.to_path_buf());
    let target_cwd = dunce::canonicalize(target_cwd).unwrap_or_else(|_| target_cwd.to_path_buf());
    session_cwd.starts_with(target_cwd)
}

/// Publish the policy before refreshing catalogs. Existing spawn contexts
/// share this cell, so a stale preflight cannot authorize a later admission.
pub(crate) fn handle_reload_subagent_toggle(agent: &MvpAgent) -> ExtResult {
    let disk = crate::config::load_from_disk()
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    apply_subagent_toggle_config(agent, &disk)
}

pub(crate) fn apply_subagent_toggle_config(agent: &MvpAgent, disk: &toml::Value) -> ExtResult {
    let toggles = crate::config::SubagentsConfig::parse_toggle(disk)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let live_toggle = agent.cfg.borrow().subagent_toggle.clone();
    *live_toggle.write() = toggles;
    let mut queued = 0;
    for (_, handle) in agent.live_control_session_handles() {
        if handle
            .cmd_tx
            .send(SessionCommand::ReloadSubagentCatalog)
            .is_ok()
        {
            queued += 1;
        }
    }
    ExtMethodResult::success(serde_json::json!({ "queued": queued }))
        .to_ext_response()
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

// internal/reload_models

/// Re-resolve the agent model list from config.toml. Called by the config
/// hot-reload watcher when `[provider.*]` or `[models]` changes.
///
/// Re-reads config from disk, re-runs the same resolution logic as
/// `new_with_models()` for user TOML config entries, and swaps the model list
/// in-place. Prefetched (API) and default models are NOT re-fetched -- only
/// BYOK entries from config are updated.
async fn handle_reload_models(agent: &MvpAgent) -> ExtResult {
    let catalog_transaction = agent.model_reload_lock.lock().await;
    let disk_config = crate::config::load_effective_config()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    let toml_config = crate::agent::config::Config::new_from_toml_cfg(&disk_config)
        .map_err(|e| acp::Error::internal_error().data(e))?;

    // Build the complete candidate off to the side. Runtime-only fields
    // (remote settings, endpoints, CLI flags) survive, while no live reader is
    // changed until the catalog validates as one atomic snapshot.
    let mut merged_config = agent.cfg.borrow().clone();
    let overrides = crate::config::ModelOverrideConfig::resolve(
        merged_config.session_title_model_override.as_deref(),
        &disk_config,
        merged_config.remote_settings.as_ref(),
    );
    merged_config.models = toml_config.models;
    merged_config.config_models = toml_config.config_models;
    merged_config.auth_providers = toml_config.auth_providers;
    merged_config.config_warnings = toml_config.config_warnings;
    merged_config.session_title_model = overrides.session_title;
    merged_config.image_description_model = overrides.image_description;
    merged_config.prompt_suggest_model_pin = overrides.prompt_suggestion;
    crate::util::config::sync_campaign_fields(&mut merged_config);

    agent
        .models_manager
        .apply_config(merged_config.clone())
        .map_err(|error| acp::Error::invalid_request().data(error))?;
    *agent.cfg.borrow_mut() = merged_config.clone();
    let published_catalog = std::sync::Arc::new(agent.models_manager.published_catalog());
    let published_revision = published_catalog.revision;

    // Existing sessions adopt the same validated provider snapshot at their
    // actor mailbox boundary. A removed selection falls back to the newly
    // resolved default; explicit per-session reasoning effort remains intact.
    // Every command is enqueued while the publication transaction is held, so
    // later model producers are ordered after this generation. Acknowledgement
    // is deliberately outside the global lock: a busy foreground may defer its
    // own adoption, but must not freeze unrelated sessions.
    let reload_targets = agent.live_control_session_handles();
    let mut failed_sessions = Vec::<(
        acp::SessionId,
        tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
        String,
    )>::new();
    let mut convergence = Vec::new();
    for (session_id, handle) in reload_targets {
        let cmd_tx = handle.cmd_tx.clone();
        let (responds_to, response) = tokio::sync::oneshot::channel();
        if cmd_tx
            .send(
                crate::session::commands::SessionCommand::ReloadModelConfig {
                    catalog: published_catalog.clone(),
                    responds_to,
                },
            )
            .is_err()
        {
            failed_sessions.push((
                session_id.clone(),
                cmd_tx,
                format!(
                    "session {} rejected the catalog reload command",
                    session_id.0
                ),
            ));
            continue;
        }
        convergence.push((session_id, cmd_tx, response));
    }
    drop(catalog_transaction);

    for (session_id, actor, response) in convergence {
        match response.await.unwrap_or_else(|_| {
            Err(acp::Error::internal_error().data(format!(
                "session {} dropped the catalog reload acknowledgement",
                session_id.0
            )))
        }) {
            Ok(()) => {}
            Err(error) => failed_sessions.push((session_id, actor, format!("{error:?}"))),
        }
    }
    // A session that cannot durably adopt the published catalog is no longer
    // a healthy resident. Evict it instead of leaving a split-brain route in
    // memory. A newer publication supersedes this result and owns convergence,
    // so an older handler must never evict against that newer generation.
    let generation_is_current = agent.models_manager.catalog_revision() == published_revision;
    let evicted = if generation_is_current {
        let mut evicted = 0usize;
        for (session_id, actor, error) in &failed_sessions {
            tracing::error!(
                session_id = %session_id.0,
                catalog_revision = published_revision,
                %error,
                "evicting session that could not converge on the reloaded model catalog"
            );
            match agent
                .evict_catalog_diverged_session(session_id, actor, published_revision)
                .await
            {
                Ok(true) => evicted += 1,
                Ok(false) => {}
                Err(shutdown_error) => {
                    tracing::error!(
                        session_id = %session_id.0,
                        %shutdown_error,
                        "catalog-diverged session writer is still shutting down"
                    );
                }
            }
        }
        evicted
    } else if !failed_sessions.is_empty() {
        tracing::info!(
            catalog_revision = published_revision,
            failures = failed_sessions.len(),
            "ignoring convergence failures from a superseded model catalog"
        );
        0
    } else {
        0
    };
    let count = agent.models_manager.models().len();
    tracing::info!(count, evicted, "model list reloaded from config.toml");
    ExtMethodResult::success(serde_json::json!({ "models": count, "evictedSessions": evicted }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// plugins/reload

async fn handle_plugins_reload(agent: &MvpAgent) -> ExtResult {
    // Rebuild the shared registry so future/new sessions clone the latest.
    let session_cwd = agent
        .sessions
        .borrow()
        .values()
        .next()
        .map(|h| std::path::PathBuf::from(&h.info.cwd));
    let mut plugins = agent.cfg.borrow().plugins.clone();
    let disk_cfg = plugins.to_discovery_config();
    // Folder-trust gates repo-local project plugins (hooks/MCP). Resolve and
    // record the verdict for this cwd (honoring the real remote), then gate
    // plugins on it.
    let project_trusted = session_cwd.as_deref().is_some_and(|c| {
        let remote_settings = agent.cfg.borrow().remote_settings.clone();
        crate::agent::folder_trust::resolve_and_record(c, remote_settings.as_ref(), false)
    });
    // Explicit ACP `grow/plugins/reload`: force a full local-install re-copy.
    agent
        .plugin_registry_handle()
        .reload(session_cwd.as_deref(), &disk_cfg, project_trusted, true);

    // Eagerly fan out the new registry to every live session: each adopts a
    // cwd-correct snapshot (hooks + MCP + skills + client slash-command
    // catalog), the same refresh the originating session of a reload gets.
    agent.broadcast_plugin_registry_to_sessions(None);

    super::to_ext_response(Ok(serde_json::json!({"ok": true})))
}

// commands/list

async fn handle_command_execute(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: ExecuteCommandRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::invalid_request()
            .data(format!("unknown session id: {}", request.session_id)));
    };
    handle
        .execute_slash_command(crate::session::HostCommandInvocation {
            command: request.command,
            description: request.description,
            invocation_id: request.invocation_id,
        })
        .await?;
    // Acceptance may precede the Step boundary; the notice owns completion.
    to_raw_response(&serde_json::json!({ "status": "accepted" }))
}

async fn handle_commands_list(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: crate::session::slash_commands::ListCommandsRequest = parse_params(args)?;

    let availability = agent.command_availability();
    let skills_config = agent.cfg.borrow().skills.clone();

    // Chat product catalog never uses cwd plugin discovery / folder-trust.
    // Prefer kind=chat even when sessionId is set so the pull matches ACU.
    if req.kind.as_deref() == Some("chat") {
        let response = crate::session::slash_commands::list_commands(
            None,
            &skills_config,
            None,
            availability,
            false,
            Some("chat"),
        )
        .await?;
        return Ok(acp::ExtResponse::new(Arc::from(
            serde_json::value::to_raw_value(&response)?,
        )));
    }

    if let Some(session_id) = req.session_id.as_ref() {
        let Some(handle) = agent.session_handle_waiting_for_load(session_id).await else {
            return Err(
                acp::Error::invalid_request().data(format!("unknown session id: {}", session_id.0))
            );
        };
        let response = handle.list_available_commands().await;
        return Ok(acp::ExtResponse::new(Arc::from(
            serde_json::value::to_raw_value(&response)?,
        )));
    }

    // Pre-session command listing scans plugins, skills, and workflows. Keep
    // the entire composition in the shared bounded worker: moving only one of
    // those scans would still leave synchronous filesystem work on this async
    // request path.
    let cwd = req.cwd;
    let kind = req.kind;
    let remote_settings = agent.cfg.borrow().remote_settings.clone();
    let plugin_registry = agent.plugin_registry_handle().clone();
    let runtime = tokio::runtime::Handle::current();
    let response = super::skills::run_bounded_discovery(
        super::skills::DISCOVERY_SLOTS.clone(),
        super::skills::DISCOVERY_TIMEOUT,
        "commands/list discovery",
        move || {
            runtime.block_on(async move {
                // For a given cwd, compute the plugin registry the same way a
                // session does at spawn time. Trust must be recorded before
                // effective project config is read; that config gates project
                // plugin paths on the recorded verdict.
                let plugin_reg = if let Some(cwd_str) = &cwd {
                    let cwd_path = Path::new(cwd_str);
                    let project_trusted = crate::agent::folder_trust::resolve_and_record(
                        cwd_path,
                        remote_settings.as_ref(),
                        false,
                    );
                    let disk_cfg = crate::config::resolve_effective_plugins_config(cwd_path)
                        .to_discovery_config();
                    // Read-only discovery: do not refresh local plugin installs.
                    plugin_registry.build_for_cwd(cwd_path, &disk_cfg, &[], project_trusted)
                } else {
                    // No cwd: retain the boot snapshot; skill and workflow
                    // discovery below still runs inside this worker.
                    plugin_registry.snapshot()
                };

                crate::session::slash_commands::list_commands(
                    cwd.as_deref(),
                    &skills_config,
                    plugin_reg.as_deref(),
                    availability,
                    false,
                    kind.as_deref(),
                )
                .await
            })
        },
    )
    .await
    .map_err(|error| {
        acp::Error::internal_error().data(format!("commands/list discovery failed: {error}"))
    })??;
    Ok(acp::ExtResponse::new(Arc::from(
        serde_json::value::to_raw_value(&response)?,
    )))
}

// session/fork

async fn handle_session_fork(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    use crate::session::fork::{ForkSessionRequest, fork_session};

    let request: ForkSessionRequest = parse_params(args)?;

    let response = fork_session(request)
        .await
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    to_raw_response(&response)
}

#[cfg(test)]
mod observation_budget_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn timed_out_read_retains_worker_permit_and_queue_deadline() {
        let workers = Arc::new(tokio::sync::Semaphore::new(1));
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                if let Some(release) = self.0.take() {
                    let _ = release.send(());
                }
            }
        }
        let release = ReleaseOnDrop(Some(release));
        let (finished, finish) = tokio::sync::oneshot::channel();
        let limiter = workers.clone();
        let read = tokio::spawn(async move {
            bounded_shared_read_with_workers(limiter, move || {
                let _ = started.send(());
                released.recv().unwrap();
                let _ = finished.send(());
                Ok(())
            })
            .await
        });
        start.await.unwrap();
        tokio::time::advance(RUNTIME_OBSERVATION_TIMEOUT).await;
        assert!(matches!(
            read.await.unwrap(),
            Err(BoundedSharedReadError::TimedOut)
        ));
        assert_eq!(workers.available_permits(), 0);
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = attempts.clone();
        let queued_workers = workers.clone();
        let queued = tokio::spawn(async move {
            bounded_shared_read_with_workers(queued_workers, move || {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await
        });
        tokio::task::yield_now().await;
        // A live blocking worker prevents Tokio's automatic time advance.
        tokio::time::advance(RUNTIME_OBSERVATION_TIMEOUT).await;
        let queued = queued.await.unwrap();
        assert!(matches!(queued, Err(BoundedSharedReadError::TimedOut)));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(workers.available_permits(), 0);
        drop(release);
        finish.await.unwrap();
        tokio::time::resume();
        // Joining a second read proves the prior worker released its permit
        // after actual completion, not at either caller deadline.
        bounded_shared_read_with_workers(workers.clone(), || Ok(()))
            .await
            .unwrap();
        assert_eq!(workers.available_permits(), 1);
    }
}
