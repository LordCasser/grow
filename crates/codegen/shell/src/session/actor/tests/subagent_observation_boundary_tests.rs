use super::*;
use std::io;
use std::sync::Arc;
use tools::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};

struct TerminalGatedChild {
    started: tokio::sync::mpsc::UnboundedSender<String>,
    terminal_release: tokio::sync::Notify,
    result_release: tokio::sync::Notify,
    terminal: tokio::sync::mpsc::UnboundedSender<String>,
}

#[async_trait::async_trait]
impl SubagentBackend for TerminalGatedChild {
    async fn spawn(
        &self,
        request: tools::implementations::grow_build::task::types::SubagentRequest,
    ) -> Result<
        tools::implementations::grow_build::task::types::SubagentResult,
        tool_runtime::ToolError,
    > {
        self.started.send(request.id.clone()).unwrap();
        self.terminal_release.notified().await;
        self.terminal.send(request.id.clone()).unwrap();
        self.result_release.notified().await;
        Ok(
            tools::implementations::grow_build::task::types::SubagentResult {
                success: true,
                output: Arc::from("terminal child result"),
                subagent_id: request.id.clone(),
                child_session_id: request.id,
                ..Default::default()
            },
        )
    }

    async fn query(
        &self,
        _id: &str,
        _block: bool,
        _timeout_ms: Option<u64>,
    ) -> Result<
        Option<tools::implementations::grow_build::task::types::SubagentSnapshot>,
        tools::implementations::grow_build::task::types::SubagentObservationError,
    > {
        Ok(None)
    }

    async fn cancel(
        &self,
        _id: &str,
    ) -> Result<
        tools::implementations::grow_build::task::types::SubagentCancelOutcome,
        tools::implementations::grow_build::task::types::SubagentObservationError,
    > {
        Ok(tools::implementations::grow_build::task::types::SubagentCancelOutcome::NotFound)
    }

    async fn validate_type(
        &self,
        _subagent_type: &str,
        _parent_session_id: &str,
    ) -> tools::implementations::grow_build::task::types::SubagentValidateTypeOutcome {
        tools::implementations::grow_build::task::types::SubagentValidateTypeOutcome::Ok
    }
}

struct HoldConsumedReceiptAck {
    staged: Option<
        tokio::sync::mpsc::UnboundedSender<(
            chat_state::TimelineEvent,
            tokio::sync::oneshot::Sender<io::Result<()>>,
        )>,
    >,
}

async fn actor_with_held_consumed_receipt_ack(
    server: &test_support::MockInferenceServer,
    stage: tokio::sync::mpsc::UnboundedSender<(
        chat_state::TimelineEvent,
        tokio::sync::oneshot::Sender<io::Result<()>>,
    )>,
) -> Arc<SessionActor> {
    let (gateway_tx, gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
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
    let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let (chat_events, _) = tokio::sync::mpsc::unbounded_channel();
    let chat_state = chat_state::ChatStateActor::spawn_from_timeline(
        timeline,
        config,
        Box::new(HoldConsumedReceiptAck {
            staged: Some(stage),
        }),
        chat_events,
        Default::default(),
    )
    .await
    .unwrap();
    actor.chat_state_handle = chat_state.clone();
    actor.events = crate::session::events::EventTracker::new(chat_state);
    let (actor, _, _) = configure_actor_with_sampler(
        server,
        sampling_types::ApiBackend::Messages,
        sampler::OutputDelivery::Irreversible,
        None,
        actor,
        gateway_rx,
        events,
    )
    .await;
    actor
}

impl chat_state::TimelinePersistence for HoldConsumedReceiptAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<io::Result<()>> {
        let (ack, receive) = tokio::sync::oneshot::channel();
        let consumed = matches!(
            &event.kind,
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed { .. }
            )
        );
        if consumed && let Some(staged) = self.staged.take() {
            let _ = staged.send((event.clone(), ack));
        } else {
            let _ = ack.send(Ok(()));
        }
        receive
    }

    fn flush(&mut self) {}
}

fn live_consultation(
    actor: &SessionActor,
    inquiry_id: &str,
    source_session_id: &str,
) -> (
    crate::coordination::InboundInquiry,
    tokio::sync::oneshot::Receiver<crate::coordination::InquiryOutcome>,
) {
    use crate::coordination::*;
    let (progress, _) = tokio::sync::watch::channel(InquiryPhase::Queued);
    let (respond_to, response) = tokio::sync::oneshot::channel();
    (
        InboundInquiry {
            authority: InquiryAuthority::Delegation,
            direction: InquiryDirection::ChildToParent,
            inquiry_id: inquiry_id.into(),
            source_peer_id: "live-child-peer".into(),
            source_session_id: source_session_id.into(),
            source_cwd: actor.session_info.cwd.clone(),
            delegated_subagent_task_name: Some("investigation".into()),
            target_session_id: actor.session_id_string(),
            question: format!("What should survive child terminal order {inquiry_id}?"),
            cancellation: InquiryCancellation::new(),
            progress,
            respond_to,
        },
        response,
    )
}

async fn install_task_planning(actor: &Arc<SessionActor>, prompt_id: &str) {
    let mut plan = tool_types::LocalPlan::new(actor.session_id_string(), "work:task-test");
    plan.steps.push(tool_types::PlanStep {
        id: "task-step".into(),
        content: "Run the observed child task".into(),
        priority: tool_types::PlanPriority::Normal,
        status: tool_types::PlanStepStatus::InProgress,
        requirements: None,
        removed: false,
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
        .unwrap();
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let (planning_tx, mut planning_rx) = tokio::sync::mpsc::unbounded_channel();
    bridge
        .update_resource(
            tools::implementations::grow_build::todo::runtime::PlanningRuntimeHandle(planning_tx),
        )
        .await;
    bridge
        .update_resource(
            tools::implementations::grow_build::task::types::CurrentPromptIdResource(
                prompt_id.into(),
            ),
        )
        .await;
    let planning_actor = Arc::downgrade(actor);
    tokio::task::spawn_local(async move {
        while let Some(command) = planning_rx.recv().await {
            let Some(actor) = planning_actor.upgrade() else {
                break;
            };
            actor.handle_planning_command(command).await;
        }
    });
}

async fn actual_session_wait_order(intervention_precedes_terminal: bool) {
    use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};
    use tools::implementations::grow_build::task::TaskTool;
    use tools::implementations::grow_build::task::types::{
        MaxSubagentDepth, SessionIdResource, SubagentDepthCounter,
    };
    use tools::implementations::grow_build::{
        GetSubagentStatusTool, KillTaskTool, ListSubagentTool, WaitSubagentTool,
    };
    use tools::registry::types::ToolConfig;

    let server = MockInferenceServer::start().await.unwrap();
    let call_id = if intervention_precedes_terminal {
        "wait-intervention-before-terminal"
    } else {
        "wait-terminal-before-intervention"
    };
    let first = server.expect_response(
        "real TaskTool child wait",
        InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
        messages_turn(
            &[tool_use_block(
                call_id,
                "spawn_subagent",
                r#"{"parent_step_id":"task-step","prompt":"inspect target","description":"inspect target","subagent_type":"explore","background":false,"capability_mode":"read-only"}"#,
            )],
            "tool_use",
        ),
    );
    let successor = server.expect_response(
        "turn resumes after receipt acknowledgement",
        InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
        messages_turn(
            &[serde_json::json!({"type":"text","text":"The exact child result and receipt are settled."})],
            "end_turn",
        ),
    );
    let consultation = server.expect_response(
        "live consultation during task wait",
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
        messages_turn(
            &[serde_json::json!({"type":"text","text":"Keep the exact child identity and wait for its terminal result."})],
            "end_turn",
        ),
    );
    let (ack_tx, mut ack_rx) = tokio::sync::mpsc::unbounded_channel();
    let actor = actor_with_held_consumed_receipt_ack(&server, ack_tx).await;
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        ToolConfig::for_tool::<TaskTool>()
            .with_name("spawn_subagent")
            .with_param_rename("run_in_background", "background"),
        ToolConfig::for_tool::<ListSubagentTool>(),
        ToolConfig::for_tool::<GetSubagentStatusTool>(),
        ToolConfig::for_tool::<WaitSubagentTool>(),
        ToolConfig::for_tool::<KillTaskTool>(),
    ])
    .await;
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let (terminal_tx, mut terminal_rx) = tokio::sync::mpsc::unbounded_channel();
    let child = Arc::new(TerminalGatedChild {
        started: started_tx,
        terminal_release: tokio::sync::Notify::new(),
        result_release: tokio::sync::Notify::new(),
        terminal: terminal_tx,
    });
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge
        .update_resource(
            tools::implementations::grow_build::task::backend::SubagentBackendResource(
                child.clone(),
            ),
        )
        .await;
    bridge.update_resource(SubagentDepthCounter(0)).await;
    bridge.update_resource(MaxSubagentDepth(1)).await;
    bridge
        .update_resource(SessionIdResource(actor.session_id_string()))
        .await;

    let prompt_id = format!("wait-order-{call_id}");
    install_task_planning(&actor, &prompt_id).await;
    let turn = tokio::task::spawn_local({
        let actor = actor.clone();
        let prompt_id = prompt_id.clone();
        async move { run_user_turn(&actor, &prompt_id).await }
    });
    let child_id = tokio::time::timeout(std::time::Duration::from_secs(10), started_rx.recv())
        .await
        .expect("real TaskTool reaches backend child")
        .unwrap();
    assert!(first.is_satisfied());
    let inject_intervention = || {
        let actor = actor.clone();
        let child_id = child_id.clone();
        async move {
            let (inquiry, response) = live_consultation(&actor, call_id, &child_id);
            actor.enqueue_coordination_inquiry(inquiry).await;
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), response)
                .await
                .expect("live coordination inquiry is answered")
                .expect("coordination owner remains live");
            assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered);
            assert!(consultation.is_satisfied());
            actor
                .chat_state_handle
                .pending_notifications()
                .await
                .unwrap()
                .into_iter()
                .find(|pending| matches!(&pending.source,
                    chat_state::NotificationSource::InquiryAnswered { inquiry_id, .. } if inquiry_id == call_id))
                .expect("intervention has one pending durable receipt")
                .id
        }
    };

    if intervention_precedes_terminal {
        let receipt = inject_intervention().await;
        child.terminal_release.notify_one();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(10), terminal_rx.recv())
                .await
                .expect("child reaches terminal result owner")
                .as_deref(),
            Some(child_id.as_str())
        );
        child.result_release.notify_one();
        settle_session_wait(
            actor, turn, successor, ack_rx, receipt, call_id, &child_id, &server,
        )
        .await;
    } else {
        child.terminal_release.notify_one();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(10), terminal_rx.recv())
                .await
                .expect("child reaches terminal result owner")
                .as_deref(),
            Some(child_id.as_str())
        );
        let receipt = inject_intervention().await;
        child.result_release.notify_one();
        settle_session_wait(
            actor, turn, successor, ack_rx, receipt, call_id, &child_id, &server,
        )
        .await;
    }
}

async fn settle_session_wait(
    actor: Arc<SessionActor>,
    turn: tokio::task::JoinHandle<Result<PromptTurnOk, acp::Error>>,
    successor: test_support::InferenceExpectation,
    mut ack_rx: tokio::sync::mpsc::UnboundedReceiver<(
        chat_state::TimelineEvent,
        tokio::sync::oneshot::Sender<io::Result<()>>,
    )>,
    receipt: String,
    call_id: &str,
    child_id: &str,
    server: &test_support::MockInferenceServer,
) {
    let (consumed, ack) = tokio::time::timeout(std::time::Duration::from_secs(10), ack_rx.recv())
        .await
        .expect("session reaches durable receipt-consumption ACK")
        .expect("persistence owner stays live");
    assert!(matches!(&consumed.kind,
        chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. })
            if notification_ids == &[receipt.clone()]));
    assert!(
        !successor.is_satisfied(),
        "sampling cannot pass the held durable receipt ACK"
    );
    assert!(
        !turn.is_finished(),
        "the real SessionActor turn owns the pending receipt"
    );
    ack.send(Ok(()))
        .expect("test releases the exact durable receipt ACK");
    turn.await
        .expect("SessionActor turn task joins")
        .expect("turn completes after receipt acknowledgement");
    assert!(successor.is_satisfied());
    let provider_request = server.request_bodies().last().unwrap().to_string();
    assert!(
        provider_request.contains(child_id),
        "child identity survives the real wait result"
    );
    assert!(provider_request.contains("terminal child result"));
    assert!(
        provider_request
            .contains("Keep the exact child identity and wait for its terminal result.")
    );

    let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
    let child_result = timeline
        .iter()
        .position(|event| matches!(&event.kind,
            chat_state::TimelineEventKind::Messages(message) if message.items.iter().any(|item|
                matches!(item, sampling_types::ConversationItem::ToolResult(result) if result.tool_call_id == call_id))))
        .expect("actual child wait result is in the parent Timeline");
    let consumed_receipt = timeline
        .iter()
        .position(|event| matches!(&event.kind,
            chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. })
                if notification_ids.contains(&receipt)))
        .expect("intervention receipt is durably consumed exactly once");
    assert!(child_result < consumed_receipt);
    assert_eq!(timeline.iter().filter(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. })
            if notification_ids.contains(&receipt))).count(), 1);
}

#[test]
fn actual_session_wait_settles_intervention_on_both_sides_of_child_terminal() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(tokio::task::LocalSet::new().run_until(async {
                actual_session_wait_order(true).await;
                actual_session_wait_order(false).await;
            }));
        })
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

struct HoldBootstrapTimelineAck {
    staged:
        Option<tokio::sync::mpsc::UnboundedSender<tokio::sync::oneshot::Sender<io::Result<()>>>>,
}

impl chat_state::TimelinePersistence for HoldBootstrapTimelineAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        _event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<io::Result<()>> {
        let (ack, receive) = tokio::sync::oneshot::channel();
        if let Some(staged) = self.staged.take() {
            let _ = staged.send(ack);
        } else {
            let _ = ack.send(Ok(()));
        }
        receive
    }

    fn flush(&mut self) {}
}

#[tokio::test(start_paused = true)]
async fn expired_source_timeline_queries_drain_after_chat_state_ack_is_released() {
    let (ack_tx, mut ack_rx) = tokio::sync::mpsc::unbounded_channel();
    let (chat_events, _) = tokio::sync::mpsc::unbounded_channel();
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let source_id = format!("observation-source-{}", uuid::Uuid::now_v7());
    let source = chat_state::ChatStateActor::spawn(
        vec![sampling_types::ConversationItem::system(
            "test source system",
        )],
        sampling_types::SamplingConfig {
            base_url: "http://localhost".into(),
            model: "test".into(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(4096).unwrap(),
            reasoning_effort: None,
            stream_tool_calls: None,
        },
        Box::new(HoldBootstrapTimelineAck {
            staged: Some(ack_tx),
        }),
        chat_events,
        cancellation_token.clone(),
    );
    let held_ack = tokio::time::timeout(std::time::Duration::from_secs(1), ack_rx.recv())
        .await
        .expect("ChatState bootstrap reaches its durable ACK")
        .expect("persistence owner stays live");

    let (coordinator_tx, coordinator_rx) = tokio::sync::mpsc::unbounded_channel();
    drop(coordinator_rx);
    let completed_history_reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let history_source = source.clone();
    let history_read_count = completed_history_reads.clone();
    let backend = ChannelBackend::for_session(coordinator_tx, source_id).with_history_query(
        move |_id| {
            let source = history_source.clone();
            let completed_history_reads = history_read_count.clone();
            async move {
                source
                    .timeline_events()
                    .await
                    .ok_or(tools::implementations::grow_build::task::types::SubagentObservationError::Unavailable)?;
                completed_history_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(None)
            }
        },
    );
    let blocked = (0..32)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .query(&format!("missing-history-child-{index}"), false, None)
                    .await
            })
        })
        .collect::<Vec<_>>();
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
    for query in blocked {
        assert!(matches!(
            query.await.expect("timed out query task joins"),
            Err(
                tools::implementations::grow_build::task::types::SubagentObservationError::TimedOut
            )
        ));
    }

    held_ack
        .send(Ok(()))
        .expect("test releases the real ChatState bootstrap ACK");
    let after_release = (0..33)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .query(&format!("missing-after-release-{index}"), false, None)
                    .await
            })
        })
        .collect::<Vec<_>>();
    for query in after_release {
        assert!(matches!(
            query.await.expect("released query task joins"),
            Ok(None)
        ));
    }
    assert_eq!(
        completed_history_reads.load(std::sync::atomic::Ordering::SeqCst),
        33,
        "post-release calls traverse the same backend history source after the expired mailbox queries drain"
    );
    let events = source
        .timeline_events()
        .await
        .expect("the ChatState mailbox drains the timed-out reads and accepts a later read");
    assert!(
        !events.is_empty(),
        "the real Timeline bootstrap was committed"
    );
    cancellation_token.cancel();
}

#[tokio::test(flavor = "current_thread")]
async fn kill_task_session_dispatch_persists_backend_cancel_receipts() {
    use test_support::MockInferenceServer;
    use tools::implementations::grow_build::task::types::{
        SubagentCancelOutcome, SubagentCancelTarget, SubagentEvent, SubagentResult,
        SubagentValidateTypeOutcome,
    };
    use tools::implementations::grow_build::{
        GetSubagentStatusTool, KillTaskTool, ListSubagentTool, TaskTool, WaitSubagentTool,
    };
    use tools::registry::types::ToolConfig;

    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let actor = actor_with_sampler_named(&server, "cancel-receipt-parent").await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig::for_tool::<TaskTool>()
                    .with_name("spawn_subagent")
                    .with_param_rename("run_in_background", "background"),
                ToolConfig::for_tool::<KillTaskTool>(),
                ToolConfig::for_tool::<ListSubagentTool>(),
                ToolConfig::for_tool::<GetSubagentStatusTool>(),
                ToolConfig::for_tool::<WaitSubagentTool>(),
            ])
            .await;

            // TaskTool creates the child through the session-bound backend;
            // the event owner holds its real spawn reply until cancellation.
            let (coordinator_tx, mut coordinator_rx) = tokio::sync::mpsc::unbounded_channel();
            let backend = ChannelBackend::for_session(coordinator_tx, actor.session_id_string());
            let bridge = actor.agent.borrow().tool_bridge().clone();
            bridge
                .update_resource(
                    tools::implementations::grow_build::task::backend::SubagentBackendResource(
                        Arc::new(backend),
                    ),
                )
                .await;
            bridge
                .update_resource(
                    tools::implementations::grow_build::task::types::SubagentDepthCounter(0),
                )
                .await;
            bridge
                .update_resource(
                    tools::implementations::grow_build::task::types::MaxSubagentDepth(1),
                )
                .await;
            bridge
                .update_resource(
                    tools::implementations::grow_build::task::types::SessionIdResource(
                        actor.session_id_string(),
                    ),
                )
                .await;

            let (spawned_tx, mut spawned_rx) = tokio::sync::mpsc::unbounded_channel();
            let coordinator = tokio::task::spawn_local(async move {
                let mut owned_child_cancelled = false;
                let mut child_result = None;
                while let Some(event) = coordinator_rx.recv().await {
                    match event {
                        SubagentEvent::OpenSpawnAdmission { parent_session_id } => {
                            assert_eq!(parent_session_id, "cancel-receipt-parent");
                        }
                        SubagentEvent::ValidateType(request) => {
                            assert_eq!(request.parent_session_id, "cancel-receipt-parent");
                            let _ = request.respond_to.send(SubagentValidateTypeOutcome::Ok);
                        }
                        SubagentEvent::Spawn(request) => {
                            assert_eq!(request.request.id, "owned-child");
                            let _ = spawned_tx.send(request.request.id.clone());
                            child_result = Some(request.result_tx);
                        }
                        SubagentEvent::Cancel(request) => {
                            assert_eq!(
                                request.parent_session_id.as_deref(),
                                Some("cancel-receipt-parent")
                            );
                            let SubagentCancelTarget::SubagentId(id) = &request.target else {
                                panic!("kill_task must target a specific child ID");
                            };
                            let outcome = match id.as_str() {
                                "owned-child" if !owned_child_cancelled => {
                                    owned_child_cancelled = true;
                                    child_result
                                        .take()
                                        .expect("the child was actually spawned before cancel")
                                        .send(SubagentResult {
                                            success: false,
                                            cancelled: true,
                                            subagent_id: id.clone(),
                                            child_session_id: id.clone(),
                                            error: Some("cancelled".into()),
                                            ..Default::default()
                                        })
                                        .expect("cancelled child result is accepted");
                                    let _ =
                                        request.respond_to.send(SubagentCancelOutcome::Cancelled);
                                    continue;
                                }
                                "owned-child" => SubagentCancelOutcome::AlreadyFinished {
                                    status: "cancelled".into(),
                                },
                                "foreign-child" => SubagentCancelOutcome::Unauthorized,
                                other => panic!("unexpected child ID {other}"),
                            };
                            let foreign = id == "foreign-child";
                            let _ = request.respond_to.send(outcome);
                            if foreign {
                                break;
                            }
                        }
                        _ => panic!("unexpected event sent to cancellation fixture"),
                    }
                }
                owned_child_cancelled
            });

            crate::session::actor::tests::support::begin_test_active_causal_turn_with_origin(
                &actor,
                "cancel-receipt-turn",
                crate::session::PromptOrigin::User,
                crate::session::TurnKind::User,
            )
            .await;
            let call =
                |id: &str, name: &str, input: &str| crate::sampling::types::ToolCallResponse {
                    id: id.into(),
                    kind: "function".into(),
                    function: crate::sampling::types::ToolCallFunction::new(name, input),
                };
            install_task_planning(&actor, "cancel-receipt-turn").await;
            let spawn_input = serde_json::json!({
                "parent_step_id": "task-step",
                "prompt": "wait for cancellation",
                "description": "cancel receipt test",
                "subagent_type": "explore",
                "task_id": "owned-child",
                "background": true,
                "capability_mode": "read-only",
            })
            .to_string();
            actor
                .execute_tool_calls(vec![call(
                    "spawn-owned-child",
                    "spawn_subagent",
                    &spawn_input,
                )])
                .await
                .expect("real SessionActor TaskTool dispatch succeeds");
            assert_eq!(spawned_rx.recv().await.as_deref(), Some("owned-child"));
            for (call_id, task_id) in [
                ("cancel-owned-child", "owned-child"),
                ("cancel-owned-child-again", "owned-child"),
                ("cancel-foreign-child", "foreign-child"),
            ] {
                let input = serde_json::json!({"task_id": task_id}).to_string();
                actor
                    .execute_tool_calls(vec![call(call_id, "kill_task", &input)])
                    .await
                    .expect("real SessionActor kill_task dispatch is a tool result");
            }
            assert!(coordinator.await.expect("backend event owner joins"));

            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            let result_for = |call_id: &str| {
                timeline.iter().find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Messages(message) => {
                        message.items.iter().find_map(|item| match item {
                            sampling_types::ConversationItem::ToolResult(result)
                                if result.tool_call_id == call_id =>
                            {
                                Some(result.content.to_string())
                            }
                            _ => None,
                        })
                    }
                    _ => None,
                })
            };
            let cancelled = result_for("cancel-owned-child")
                .expect("first cancellation result is durably recorded");
            assert!(
                cancelled.contains("killed"),
                "receipt must report actual cancellation: {cancelled}"
            );
            assert!(
                result_for("cancel-owned-child-again")
                    .expect("idempotent result is durably recorded")
                    .contains("already_exited")
            );
            let denied = result_for("cancel-foreign-child")
                .expect("authorization failure is durably recorded as a tool result");
            assert!(denied.to_lowercase().contains("not authorized"));
            assert!(!denied.contains("already_exited") && !denied.contains("\"killed\""));

            let conversation = actor.chat_state_handle.get_conversation().await;
            let foreign_error = conversation.iter().find_map(|item| match item {
                sampling_types::ConversationItem::ToolResult(result)
                    if result.tool_call_id == "cancel-foreign-child" =>
                {
                    Some(result.content.to_string())
                }
                _ => None,
            });
            assert!(
                foreign_error
                    .expect("foreign rejection is durably recorded as a tool result")
                    .to_lowercase()
                    .contains("not authorized")
            );
        })
        .await;
}
