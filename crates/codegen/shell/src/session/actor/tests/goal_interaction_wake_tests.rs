//! Missing-wake regression for the idle human-interaction dependency path
//! (behavior-goal delta in `openspec/changes/repair-goal-turn-continuation`).
//!
//! Discriminating chain (C1): a cross-workspace Peer coordination inquiry
//! enters through the real `SessionCommand::RunCoordinationInquiry` arm, and
//! `request_coordination_approval` registers a Permission guard in this
//! session's pending-interaction registry while the foreground is idle under
//! a durable whole-work wait keyed to that inquiry id. Resolving the inquiry
//! (cancellation here; a user decision or `APPROVAL_TIMEOUT` takes the same
//! guard-drop path) must re-enter idle arbitration through the run loop's
//! registry subscription, without any additional user input.
//!
//! Scope limits recorded in the change: this is a module-level regression on
//! an idle state the Timeline already supports. It is NOT a reproduction of
//! the user-reported stall, and it does not prove the model can discover the
//! inquiry id on its own — the wait uses a known id as a runtime commitment.
use super::support::*;
use super::*;
use serde_json::json;
use std::sync::Arc;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};
use tokio::sync::mpsc;

const END_TURN: &str = "end_turn";

/// Run a test body on a dedicated thread with a large stack (same shape as
/// the heavy session tests).
fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

fn text_block(text: &str) -> serde_json::Value {
    json!({ "type": "text", "text": text })
}

/// Assemble a Messages-API SSE turn (same wire shape as the sibling suites).
fn messages_turn(blocks: &[serde_json::Value], stop_reason: &str) -> ScriptedResponse {
    let mut events: Vec<String> = vec![
        json!({
            "type": "message_start",
            "message": {
                "id": "msg_test", "type": "message", "role": "assistant",
                "content": [], "model": "test-model", "stop_reason": null,
                "usage": {
                    "input_tokens": 10, "output_tokens": 0,
                    "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0
                }
            }
        })
        .to_string(),
    ];
    for (i, block) in blocks.iter().enumerate() {
        let start_block = json!({ "type": "text", "text": "" });
        events.push(
            json!({ "type": "content_block_start", "index": i, "content_block": start_block })
                .to_string(),
        );
        events.push(
            json!({
                "type": "content_block_delta",
                "index": i,
                "delta": { "type": "text_delta", "text": block["text"] }
            })
            .to_string(),
        );
        events.push(json!({ "type": "content_block_stop", "index": i }).to_string());
    }
    events.push(
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason },
            "usage": { "output_tokens": 5, "input_tokens": 10 }
        })
        .to_string(),
    );
    events.push(json!({ "type": "message_stop" }).to_string());
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

async fn activate_test_goal(actor: &SessionActor, goal_id: &str, objective: &str) {
    actor
        .goal_tracker
        .lock()
        .create_goal(
            goal_id.into(),
            objective.into(),
            None,
            "2026-10-19T00:00:00Z".into(),
        )
        .expect("test Goal must be created");
    let goal = actor
        .goal_tracker
        .lock()
        .snapshot()
        .cloned()
        .expect("created Goal must have an identity");
    actor
        .persist_behavior_transition_durably(
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            Some(goal.clone()),
        )
        .await
        .expect("Goal activation must be durable before its turn");
    actor
        .behavior
        .lock()
        .select_behavior(tool_types::BehaviorId::Goal);
    actor.sync_goal_usage_window();
}

/// Seed a durable whole-work Permission wait keyed to `inquiry_id` through
/// the same Timeline mechanism production uses. The mailbox loop is already
/// running; the test establishes this state before admitting the inquiry.
async fn seed_whole_work_permission_wait(actor: &SessionActor, inquiry_id: &str) {
    let mut plan = tool_types::LocalPlan::new(actor.session_id_string(), "work:idle-inquiry");
    plan.waits.push(tool_types::PlanWait {
        id: "idle-inquiry-wait".into(),
        step_ids: Vec::new(),
        dependencies: vec![tool_types::WaitDependency {
            kind: tool_types::DependencyKind::Permission,
            id: inquiry_id.into(),
        }],
        condition: tool_types::WaitCondition::All,
    });
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan),
            },
        ))
        .await
        .expect("the idle wait is owned by Timeline");
}

/// Everything the gateway stub observed.
#[derive(Default)]
struct GatewayLog {
    permission_requests: std::sync::Mutex<Vec<acp::RequestPermissionRequest>>,
    /// Reverse requests whose response sender is held open (no answer), so
    /// the approval guard stays pending exactly like a waiting user.
    held: std::sync::Mutex<usize>,
}

/// Ack notifications; log and hold every `request_permission` without
/// answering. A held reverse request is the real "user has not decided"
/// state; resolution happens through inquiry cancellation in the tests.
fn spawn_gateway_stub(
    mut rx: mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    log: Arc<GatewayLog>,
) {
    tokio::task::spawn_local(async move {
        let mut held = Vec::new();
        while let Some(message) = rx.recv().await {
            match message {
                acp_transport::AcpClientMessage::RequestPermission(args) => {
                    log.permission_requests
                        .lock()
                        .unwrap()
                        .push(args.request.clone());
                    // Holding the response sender keeps the request open.
                    held.push(args);
                    *log.held.lock().unwrap() = held.len();
                }
                acp_transport::AcpClientMessage::SessionNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                acp_transport::AcpClientMessage::ExtNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                _ => {}
            }
        }
    });
}

/// Goal-capable actor with a real sampler pointed at `server`, a held-open
/// gateway stub, and the real `run_session` mailbox (same wiring as the
/// lifecycle mailbox tests).
async fn goal_actor_with_mailbox(
    server: &MockInferenceServer,
) -> (
    Arc<SessionActor>,
    mpsc::UnboundedSender<SessionCommand>,
    Arc<GatewayLog>,
) {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let log = Arc::new(GatewayLog::default());
    spawn_gateway_stub(gateway_rx, log.clone());
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                let _ = respond_to.send(Ok(()));
            }
        }
    });
    let (mut actor, events) = create_test_actor_ex_with_projection_error(
        0,
        256_000,
        85,
        gateway_tx,
        persistence_tx,
        None,
    )
    .await;
    actor.goal_enabled = true;
    // Goal continuation requires the runtime availability flag, which the
    // Goal tools (create/get/update + todo_write) establish.
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::CreateGoalTool,
        >(),
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::GetGoalTool,
        >(),
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::UpdateGoalTool,
        >(),
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::todo::TodoWriteTool,
        >(),
    ])
    .await;
    // The cross-workspace coordination approval arm requires an online UI.
    actor
        .notifications
        .gateway_enabled
        .store(true, std::sync::atomic::Ordering::Release);
    if let Some(mut config) = actor.chat_state_handle.get_sampling_config().await {
        config.base_url = server.url();
        config.api_backend = sampling_types::ApiBackend::Messages;
        actor.chat_state_handle.replace_sampling_route(config);
    }
    let sampler_config = sampler::SamplerConfig {
        api_key: Some("test-key".to_string()),
        base_url: server.url(),
        model: "test-model".to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: sampling_types::ApiBackend::Messages,
        auth_scheme: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: 100_000,
        force_http1: false,
        max_retries: Some(0),
        stream_tool_calls: false,
        idle_timeout_secs: Some(60),
        reasoning_effort: None,
        origin_client: None,
        bearer_resolver: None,
        compactions_remaining: None,
        compaction_at_tokens: None,
        doom_loop_recovery: None,
    };
    let (sampler_event_tx, mut sampler_event_rx) =
        mpsc::unbounded_channel::<sampler::SamplingEvent>();
    actor.sampler_handle = sampler::SamplerActor::spawn(
        sampler_config,
        sampler::RetryPolicy {
            output_delivery: sampler::OutputDelivery::Irreversible,
            ..Default::default()
        },
        sampler_event_tx,
    );
    let (goal_usage_tx, mut goal_usage_rx) = mpsc::unbounded_channel();
    actor.goal_usage_window = goal_support::GoalUsageWindow::new(goal_usage_tx.clone(), None);
    let actor = Arc::new(actor);
    assert!(
        actor.refresh_goal_runtime_availability().await,
        "Goal runtime must be available through its registered tools"
    );
    let usage_actor = actor.clone();
    tokio::task::spawn_local(async move {
        let _mailbox_owner = goal_usage_tx;
        while let Some(SessionCommand::SettleGoalUsageAttempt {
            attempt_id,
            respond_to,
        }) = goal_usage_rx.recv().await
        {
            let _ = respond_to.send(
                usage_actor
                    .settle_claimed_goal_usage_attempt(&attempt_id)
                    .await,
            );
        }
    });
    let drainer = actor.clone();
    tokio::task::spawn_local(async move {
        while let Some(event) = sampler_event_rx.recv().await {
            drainer.handle_sampling_event(event).await;
        }
    });
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (chat_owner, chat_rx) = mpsc::unbounded_channel();
    let session = actor.clone();
    tokio::task::spawn_local(async move {
        let _chat_owner = chat_owner;
        let root = session.session_dir.clone();
        crate::session::actor::run_loop::run_session(
            session,
            command_rx,
            chat_rx,
            events,
            None,
            Arc::new(parking_lot::Mutex::new(CodebaseIndexManager::default())),
            root,
            Default::default(),
        )
        .await;
    });
    let (responds_to, ready) = tokio::sync::oneshot::channel();
    commands
        .send(SessionCommand::GetActiveAgent { responds_to })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), ready)
        .await
        .expect("run_session mailbox startup must complete")
        .unwrap();
    (actor, commands, log)
}

fn pending_permission(actor: &SessionActor, id: &str) -> bool {
    actor
        .pending_interactions
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(id)
        == Some(&crate::session::pending_interaction::PendingKind::Permission)
}

/// An inbound Peer inquiry whose source cwd cannot canonicalize to this
/// session's cwd, so `handle_coordination_inquiry` takes the cross-workspace
/// approval arm (`request_coordination_approval`).
fn cross_cwd_peer_inquiry(
    actor: &SessionActor,
    inquiry_id: &str,
    question: &str,
) -> crate::coordination::InboundInquiry {
    use crate::coordination::*;
    let (progress, _) = tokio::sync::watch::channel(InquiryPhase::Queued);
    let (respond_to, _response) = tokio::sync::oneshot::channel();
    InboundInquiry {
        authority: InquiryAuthority::Peer,
        direction: InquiryDirection::Peer,
        inquiry_id: inquiry_id.into(),
        source_peer_id: "external-primary-peer".into(),
        source_session_id: "external-primary-session".into(),
        source_cwd: "/nonexistent-external-workspace".into(),
        delegated_subagent_task_name: None,
        target_session_id: actor.session_info.id.to_string(),
        question: question.into(),
        cancellation: InquiryCancellation::new(),
        progress,
        respond_to,
    }
}

async fn goal_continuation_started(actor: &SessionActor, goal_id: &str) -> bool {
    actor
        .chat_state_handle
        .timeline_events()
        .await
        .unwrap_or_default()
        .iter()
        .any(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                    identity, ..
                }) if identity.goal_id.as_deref() == Some(goal_id)
            )
        })
}

/// C1 discriminating regression (old code is red): while idle under a durable
/// whole-work Permission wait, resolving the cross-workspace inquiry's
/// approval guard through the real coordination entry re-arms Goal
/// continuation with no additional user input.
#[test]
fn resolving_idle_inquiry_permission_wakes_goal_continuation() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            // The freed continuation samples once and ends its turn.
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("continuation ran")], END_TURN),
            );
            let (actor, commands, log) = goal_actor_with_mailbox(&server).await;
            activate_test_goal(&actor, "idle-inquiry-goal", "continue after the inquiry").await;
            seed_whole_work_permission_wait(&actor, "inquiry-c1").await;

            // Before the guard exists the Permission dependency observes as
            // missing, which correctly counts as ready — the parked state
            // only exists once the inquiry registers its guard.
            assert!(actor.goal_loop_active());
            assert!(actor.state.lock().await.foreground.is_idle());
            assert!(!actor.planning_waits_for_event().await);

            // Real production entry: the run_session command arm.
            let inquiry = cross_cwd_peer_inquiry(&actor, "inquiry-c1", "may I proceed?");
            let cancellation = inquiry.cancellation.clone();
            commands
                .send(SessionCommand::RunCoordinationInquiry { inquiry })
                .unwrap();

            // The approval guard must be registered and keep the Goal parked.
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !pending_permission(&actor, "inquiry-c1") {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the cross-workspace approval must reach the registry");
            // Now the whole-work wait has a matching active owner: the Goal
            // must be parked by it.
            assert!(actor.planning_waits_for_event().await);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            assert_eq!(
                server.messages_request_count(),
                0,
                "a pending human dependency must not admit a Goal sample"
            );
            assert!(!goal_continuation_started(&actor, "idle-inquiry-goal").await);

            // Resolution through the same production path: the source side
            // cancels the inquiry; the guard drops inside
            // `request_coordination_approval`'s biased cancellation arm.
            cancellation
                .cancel(crate::coordination::InquiryCancellationReason::Explicit)
                .then_some(())
                .expect("inquiry cancellation must be admitted");

            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while !goal_continuation_started(&actor, "idle-inquiry-goal").await {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("registry resolution must wake idle Goal continuation without extra input");
            assert!(!pending_permission(&actor, "inquiry-c1"));
            assert!(!log.permission_requests.lock().unwrap().is_empty());
        }));
    });
}

/// Control: a Goal that stopped before resolution receives the same registry
/// change and is not restarted by it.
#[test]
fn registry_wake_does_not_restart_stopped_goal() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, commands, _log) = goal_actor_with_mailbox(&server).await;
            activate_test_goal(&actor, "stopped-goal", "done before the inquiry").await;
            seed_whole_work_permission_wait(&actor, "inquiry-stopped").await;
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .complete("stopped for the control test"),
                "active Goal accepts completion"
            );

            let inquiry = cross_cwd_peer_inquiry(&actor, "inquiry-stopped", "may I proceed?");
            let cancellation = inquiry.cancellation.clone();
            commands
                .send(SessionCommand::RunCoordinationInquiry { inquiry })
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !pending_permission(&actor, "inquiry-stopped") {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("the approval guard must register before resolution");
            cancellation
                .cancel(crate::coordination::InquiryCancellationReason::Explicit)
                .then_some(())
                .expect("inquiry cancellation must be admitted");

            // Give a misfiring wake ample time to restart the Goal.
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            assert_eq!(
                server.messages_request_count(),
                0,
                "a completed Goal must not be restarted by a registry change"
            );
            assert!(!goal_continuation_started(&actor, "stopped-goal").await);
        }));
    });
}

/// Control: an ordinary same-session ask keeps the foreground busy, so the
/// idle C1 premise is unreachable through that path — a same-session guard
/// cannot be used to manufacture an idle wait.
#[test]
fn same_session_ask_guard_never_yields_idle_foreground() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            tokio::task::spawn_local(async move {
                while let Some(message) = persistence_rx.recv().await {
                    if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                        let _ = respond_to.send(Ok(()));
                    }
                }
            });
            let (actor, _) = create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
            // A live same-session ask runs inside a foreground RegularTurn.
            begin_test_active_causal_turn(&actor).await;
            let (ask_gateway, _ask_gateway_rx) = mpsc::unbounded_channel();
            let _ask = crate::session::pending_interaction::PendingInteractionGuard::new(
                actor.pending_interactions.clone(),
                acp_transport::AcpAgentGatewaySender::new(ask_gateway),
                actor.session_info.id.clone(),
                "same-session-ask".into(),
                crate::session::pending_interaction::PendingKind::Question,
            );
            let state = actor.state.lock().await;
            assert!(
                !state.foreground.is_idle(),
                "an unanswered same-session ask owns the foreground; idle cannot form"
            );
        }));
    });
}
