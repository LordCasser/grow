//! Reproduction + contract tests for the subagent bash-permission chain.
//!
//! Bug under investigation (Task C): "subagent requests bash/shell
//! permission, the user approves in the pager, but the tool still does not
//! execute."
//!
//! These tests drive the **shell side** of that chain on the real session
//! seam: a `SessionActor` configured as a subagent
//! (`startup_hints.is_subagent = true`) runs one `ToolInput::Bash` tool call
//! through `execute_tool_calls` against a **real** permission manager whose
//! `AcpPrompter` talks to a fake gateway that answers `request_permission`
//! with `Selected("allow-once")`.
//!
//! The tests cover the **inherited-handle** spawn shape, the only production
//! subagent shape: the mvp-agent coordinator always passes
//! `parent.permission_handle` (see `subagent_coordinator.rs`), so the ACP
//! `request_permission` must carry the **parent** session id (that is what
//! the pager routes on), the `PermissionEvent` must still record the
//! subagent session id, and after approval the bash tool must actually
//! execute.
//!
//! A second test pins the reject path: `reject-once` must not execute the
//! command and must surface as `ToolLoop::PermissionReject`.

use std::sync::Arc;

use acp_transport::{AcpAgentGatewaySender, AcpClientMessage};
use agent_client_protocol as acp;
use paths::AbsPathBuf;
use tools::implementations::grow_build::bash::BashTool;
use tools::registry::types::ToolConfig;
use workspace::permission::{ClientType, PermissionEvent, spawn_permission_manager};

use super::support::{create_test_actor_ex, test_agent_with_tools};
use super::{PersistenceMsg, ReplayBuffer, SessionActor, SessionEvent, ToolLoop};

/// Subagent session id used by every test in this file.
const SUBAGENT_SID: &str = "subagent-bash-perm-repro";
/// Parent session id used by the inherited-handle test.
const PARENT_SID: &str = "parent-bash-perm-repro";
/// Bash command that is harmless to really execute in tests, yet must be
/// prompted for: `sh -c …` is an opaque shell (bash_request_floor), so the
/// manager cannot auto-allow it via the safe-command list.
const BASH_ARGS: &str = r#"{"command":"sh -c 'echo repro-ok'","description":"repro bash permission test","is_background":false}"#;
const TOOL_CALL_ID: &str = "call_bash_repro";

/// Everything the fake gateway observed, for assertions.
#[derive(Debug, Default)]
struct GatewayLog {
    /// Every `request_permission` the prompter sent (with its session id).
    permission_requests: Vec<acp::RequestPermissionRequest>,
    /// Every `ToolCallUpdate` status the shell pushed via `send_update`.
    tool_update_statuses: Vec<(String, Option<acp::ToolCallStatus>)>,
}

/// Answer every client-bound gateway message. `request_permission` gets the
/// requested option id (`allow-once` / `reject-once`); notifications are
/// ACKed; anything else is dropped (a dropped `response_tx` is the same
/// no-answer behavior as `dummy_gateway()` in the auto-mode tests, and the
/// bash path never blocks on those).
fn spawn_gateway_responder(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AcpClientMessage>,
    reply_option: Option<&'static str>,
    log: Arc<std::sync::Mutex<GatewayLog>>,
) {
    tokio::task::spawn_local(async move {
        let mut pending_permissions = Vec::new();
        while let Some(msg) = rx.recv().await {
            match msg {
                AcpClientMessage::RequestPermission(args) => {
                    log.lock()
                        .expect("gateway log lock")
                        .permission_requests
                        .push(args.request.clone());
                    if let Some(reply_option) = reply_option {
                        let resp = acp::RequestPermissionResponse::new(
                            acp::RequestPermissionOutcome::Selected(
                                acp::SelectedPermissionOutcome::new(acp::PermissionOptionId::new(
                                    Arc::from(reply_option),
                                )),
                            ),
                        );
                        let _ = args.response_tx.send(Ok(resp));
                    } else {
                        // Keep the response sender alive to model a request or
                        // response lost beyond the gateway boundary.
                        pending_permissions.push(args);
                    }
                }
                AcpClientMessage::SessionNotification(args) => {
                    if let acp::SessionUpdate::ToolCallUpdate(update) = &args.request.update {
                        log.lock()
                            .expect("gateway log lock")
                            .tool_update_statuses
                            .push((update.tool_call_id.0.to_string(), update.fields.status));
                    }
                    let _ = args.response_tx.send(Ok(()));
                }
                AcpClientMessage::ExtNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                _ => {
                    // No other message type is awaited by the bash path;
                    // dropping the response sender is harmless.
                }
            }
        }
    });
}

/// Shared fixture: subagent-shaped actor + real permission manager + fake
/// gateway responder + a tool bridge that knows the Bash tool.
///
/// Returns `(actor, event_rx, gateway log, permission events)`. `event_rx`
/// is the actor's `SessionEvent` queue: `execute_tool_calls` runs the actor
/// directly (not via `run_session`), so `send_update` notifications pile up
/// there and must be drained through the `ReplayBuffer` by the caller (see
/// [`drain_notifications_to_gateway`]) before gateway-side assertions.
async fn make_subagent_fixture(
    reply_option: Option<&'static str>,
    prompt_timeout: std::time::Duration,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    Arc<std::sync::Mutex<GatewayLog>>,
    tokio::sync::mpsc::UnboundedReceiver<PermissionEvent>,
) {
    let (gateway_tx, gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    let log = Arc::new(std::sync::Mutex::new(GatewayLog::default()));
    spawn_gateway_responder(gateway_rx, reply_option, Arc::clone(&log));

    let (mut actor, event_rx) =
        create_test_actor_ex(0, 256_000, 85, gateway_tx.clone(), persistence_tx).await;
    actor.startup_hints.is_subagent = true;
    actor.session_info.id = acp::SessionId::new(SUBAGENT_SID);

    // Real permission manager (ask mode) wired to the same gateway the actor
    // uses. The manager is the shared one the subagent INHERITS from its
    // parent: `manager_session_id` is the PARENT session id (the id the
    // pager matches as Root and routes the request on), while the subagent
    // session id is attributed via `subagent_session_id` in the
    // PermissionEvent. This is the only production spawn shape — the
    // coordinator always passes `parent.permission_handle`.
    let cwd = AbsPathBuf::new(std::path::PathBuf::from(actor.session_info.cwd.clone()))
        .unwrap_or_else(|_| AbsPathBuf::new(std::path::PathBuf::from("/tmp")).unwrap());
    let (handle, permission_events) = spawn_permission_manager(
        acp::SessionId::new(PARENT_SID),
        AcpAgentGatewaySender::new(gateway_tx),
        cwd,
        ClientType::GrowPager,
        prompt_timeout,
        None,
        vec![],
        vec![],
        false,
        None,
        false,
    );
    actor.permissions = handle;

    // Tool bridge must know the Bash tool (client name `run_terminal_cmd`).
    // Background bash is disabled: `BashParams::enabled_background` defaults
    // to true, and `finalize_builder` then requires `get_task_output` +
    // `kill_task` (BackgroundTaskAction/KillTaskAction) to be registered.
    // This test only drives a foreground command, so disabling background is
    // the minimal registration that satisfies the requirement — same shape as
    // `tools`' own `bash_definition_hides_is_background_when_disabled`
    // test and the production fallback in `agent/src/builder.rs`. The
    // permission gate is unaffected: the opaque-shell bash floor is decided
    // by the command shape alone (`sh -c …`), not by `enabled_background`.
    let bash_config = ToolConfig::for_tool::<BashTool>().with_param("enabled_background", false);
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![bash_config]).await;
    actor
        .workspace_ops
        .bind_local_session(
            &actor.session_id_string(),
            actor.tool_context.cwd.as_path().to_path_buf(),
            actor.tool_context.hunk_tracker_handle.clone(),
            actor.agent.borrow().tool_bridge().toolset(),
            None,
        )
        .expect("bind_local_session must succeed");

    (actor, event_rx, log, permission_events)
}

/// Deliver the actor's queued notifications to the gateway, mirroring the
/// session loop (`run_loop.rs`): `send_update` enqueues `SessionEvent` on
/// `event_tx`, and the loop feeds them through the `ReplayBuffer` into
/// `emit_buffered`. Tests that drive the actor directly must do the same or
/// ToolCallUpdates never reach the gateway. Same pattern as the
/// `updates.rs` / `turn_completion_emit_tests.rs` precedents.
async fn drain_notifications_to_gateway(
    actor: &SessionActor,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    let mut replay_buffer = ReplayBuffer::new(actor.buffering_settings.clone());
    while let Ok(event) = event_rx.try_recv() {
        match event {
            SessionEvent::Notification(notification) => {
                if let Some((primary, secondary)) = replay_buffer.consume_chunk(notification) {
                    actor.emit_buffered(primary).await;
                    if let Some(extra) = secondary {
                        actor.emit_buffered(extra).await;
                    }
                }
            }
            other => panic!("expected only Notification events, got {other:?}"),
        }
    }
}

/// The gateway responder runs as a separate task on the same LocalSet, so
/// gateway traffic appears in `log` only after the test future yields.
/// Wait (bounded) until `pred` observes the expected traffic, so the
/// assertions don't race the responder's next poll.
async fn wait_for_gateway_log(
    log: &Arc<std::sync::Mutex<GatewayLog>>,
    pred: impl Fn(&GatewayLog) -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if pred(&log.lock().expect("gateway log lock")) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// One `ToolInput::Bash` tool call, as the model would emit it.
fn bash_call() -> crate::sampling::types::ToolCallResponse {
    crate::sampling::types::ToolCallResponse {
        id: TOOL_CALL_ID.to_string(),
        kind: "function".to_string(),
        function: crate::sampling::types::ToolCallFunction::new("run_terminal_cmd", BASH_ARGS),
    }
}

fn drain_permission_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<PermissionEvent>,
) -> Vec<PermissionEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

/// Suspicious point 1 (inherited-handle shape, the mvp-agent coordinator
/// path): the ACP request must carry the PARENT session id (the id the pager
/// matches as Root), the PermissionEvent must still record the subagent id,
/// and an allow-once approval must execute the bash tool.
#[tokio::test(flavor = "current_thread")]
async fn subagent_inherited_handle_bash_approved_after_prompt_executes() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, mut event_rx, log, mut permission_events) =
                make_subagent_fixture(Some("allow-once"), std::time::Duration::from_secs(60)).await;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                actor.execute_tool_calls(vec![bash_call()]),
            )
            .await
            .expect("execute_tool_calls must not hang")
            .expect("execute_tool_calls must not error");

            assert!(
                matches!(result, ToolLoop::Continue),
                "an approved bash call must continue the turn, got {result:?}"
            );

            // Deliver queued SessionEvents (ToolCall updates, …) to the
            // gateway exactly as the session loop would.
            drain_notifications_to_gateway(&actor, &mut event_rx).await;

            {
                let log = log.lock().expect("gateway log lock");
                assert_eq!(log.permission_requests.len(), 1);
                assert_eq!(
                    log.permission_requests[0].session_id.0.as_ref(),
                    PARENT_SID,
                    "the inherited-handle request must carry the PARENT session id \
                     (the id the pager matches as Root)"
                );
            }
            assert!(
                wait_for_gateway_log(&log, |l| l
                    .tool_update_statuses
                    .iter()
                    .any(|(id, status)| id == TOOL_CALL_ID
                        && *status == Some(acp::ToolCallStatus::Completed)))
                .await,
                "a Completed ToolCallUpdate must be emitted for the approved bash call; got {:?}",
                log.lock().expect("gateway log lock").tool_update_statuses,
            );
            let conv = actor.chat_state_handle.get_conversation().await;
            assert!(
                conv.iter().any(|c| c.text_content().contains("repro-ok")),
                "the bash output must land in the conversation as a tool result"
            );

            let events = drain_permission_events(&mut permission_events);
            let event = events
                .iter()
                .find(|e| e.tool_id == TOOL_CALL_ID)
                .expect("a PermissionEvent for the bash call must exist");
            assert_eq!(event.decision, "allow");
            assert_eq!(event.prompt_outcome.as_deref(), Some("allow_once"));
            assert_eq!(
                event.subagent_session_id.as_deref(),
                Some(SUBAGENT_SID),
                "diagnostics must attribute the request to the subagent even though \
                 the ACP request carries the parent id"
            );
        })
        .await;
}

/// Reject path regression: `reject-once` must NOT execute the command and
/// must surface as `ToolLoop::PermissionReject`.
#[tokio::test(flavor = "current_thread")]
async fn subagent_bash_rejected_after_prompt_does_not_execute() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, mut event_rx, log, mut permission_events) =
                make_subagent_fixture(Some("reject-once"), std::time::Duration::from_secs(60))
                    .await;

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                actor.execute_tool_calls(vec![bash_call()]),
            )
            .await
            .expect("execute_tool_calls must not hang")
            .expect("execute_tool_calls must not error");

            assert!(
                matches!(result, ToolLoop::PermissionReject { .. }),
                "a rejected bash call must surface as PermissionReject, got {result:?}"
            );

            // Deliver the queued SessionEvents (the rejection's Failed
            // ToolCallUpdate) to the gateway so the negative assertions
            // below are checked against real gateway traffic.
            drain_notifications_to_gateway(&actor, &mut event_rx).await;

            {
                let log = log.lock().expect("gateway log lock");
                assert_eq!(log.permission_requests.len(), 1);
            }
            assert!(
                wait_for_gateway_log(&log, |l| l
                    .tool_update_statuses
                    .iter()
                    .any(|(id, status)| id == TOOL_CALL_ID
                        && *status == Some(acp::ToolCallStatus::Failed)))
                .await,
                "the rejection must emit a Failed ToolCallUpdate; got {:?}",
                log.lock().expect("gateway log lock").tool_update_statuses,
            );
            // The Failed-wait above already synchronized the responder;
            // now assert the full delivered traffic contains no Completed.
            {
                let log = log.lock().expect("gateway log lock");
                assert!(
                    !log.tool_update_statuses
                        .iter()
                        .any(|(id, status)| id == TOOL_CALL_ID
                            && *status == Some(acp::ToolCallStatus::Completed)),
                    "a rejected bash call must never complete; got {:?}",
                    log.tool_update_statuses,
                );
            }

            let conv = actor.chat_state_handle.get_conversation().await;
            assert!(
                !conv.iter().any(|c| c.text_content().contains("repro-ok")),
                "a rejected bash call must not produce output"
            );

            let events = drain_permission_events(&mut permission_events);
            let event = events
                .iter()
                .find(|e| e.tool_id == TOOL_CALL_ID)
                .expect("a PermissionEvent for the bash call must exist");
            assert_eq!(event.decision, "reject");
            assert_eq!(event.prompt_outcome.as_deref(), Some("reject_once"));
            assert_eq!(
                event.subagent_session_id.as_deref(),
                Some(SUBAGENT_SID),
                "the PermissionEvent must attribute the request to the subagent"
            );
        })
        .await;
}

/// Lost permission responses must cancel the tool and clear the pending
/// interaction instead of leaving the shared subagent session blocked.
#[tokio::test(start_paused = true, flavor = "current_thread")]
async fn subagent_bash_permission_timeout_does_not_execute() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let timeout = std::time::Duration::from_secs(5);
            let (actor, mut event_rx, log, mut permission_events) =
                make_subagent_fixture(None, timeout).await;

            let result = actor
                .execute_tool_calls(vec![bash_call()])
                .await
                .expect("execute_tool_calls must not error");
            assert!(
                matches!(result, ToolLoop::PermissionTimedOut { .. }),
                "a lost permission response must time out, got {result:?}"
            );
            assert!(
                actor
                    .pending_interactions
                    .lock()
                    .expect("pending interaction lock")
                    .is_empty(),
                "the timeout must drop PendingInteractionGuard"
            );

            drain_notifications_to_gateway(&actor, &mut event_rx).await;
            assert!(
                wait_for_gateway_log(&log, |l| l
                    .tool_update_statuses
                    .iter()
                    .any(|(id, status)| id == TOOL_CALL_ID
                        && *status == Some(acp::ToolCallStatus::Failed)))
                .await,
                "the timeout must emit a Failed ToolCallUpdate"
            );
            let conv = actor.chat_state_handle.get_conversation().await;
            assert!(
                !conv.iter().any(|c| c.text_content().contains("repro-ok")),
                "a timed-out bash call must not produce command output"
            );

            let events = drain_permission_events(&mut permission_events);
            let event = events
                .iter()
                .find(|event| event.tool_id == TOOL_CALL_ID)
                .expect("a PermissionEvent for the timeout must exist");
            assert_eq!(event.decision, "timed_out");
            assert_eq!(event.prompt_outcome.as_deref(), Some("timed_out"));
            assert_eq!(event.decision_reason.as_deref(), Some("permission_timeout"));
            assert_eq!(event.wait_ms, Some(timeout.as_millis() as u64));
        })
        .await;
}
