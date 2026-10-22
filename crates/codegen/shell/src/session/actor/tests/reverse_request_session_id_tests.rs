//! Regression guard: every blocking reverse-request
//! (permission / `ask_user_question` / plan-approval) must carry a
//! non-empty `sessionId`, otherwise Tier-2 routing silently drops it
//! (`server.rs`). The invariant holds today; these tests pin it.
use tools::implementations::grow_build::ask_user_question::{
    AskUserQuestionExtRequest, AskUserQuestionMode,
};
use tools::implementations::grow_build::plan_control::PlanApprovalExtRequest;

#[derive(Clone, Copy)]
enum FailQuestionPersistenceAt {
    Opened,
    Resolved,
}

struct QuestionTestPersistence {
    fail_at: Option<FailQuestionPersistenceAt>,
    committed: std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
}

impl chat_state::TimelinePersistence for QuestionTestPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let should_fail = matches!(
            (&self.fail_at, &event.kind),
            (
                Some(FailQuestionPersistenceAt::Opened),
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Opened { .. }
                ),
            ) | (
                Some(FailQuestionPersistenceAt::Resolved),
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Resolved { .. },
                ),
            )
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        let result = if should_fail {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "injected question interaction persistence failure",
            ))
        } else {
            self.committed
                .lock()
                .expect("question persistence log must remain available")
                .push(event.clone());
            Ok(())
        };
        let _ = tx.send(result);
        rx
    }

    fn flush(&mut self) {}
}

async fn question_worker_actor(
    fail_at: Option<FailQuestionPersistenceAt>,
) -> (
    std::sync::Arc<super::super::SessionActor>,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
) {
    use super::support::create_test_actor;

    let (gateway_tx, gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    let committed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sampling_config = actor
        .chat_state_handle
        .get_sampling_config()
        .await
        .expect("test actor must have a sampling config");
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn(
        Vec::new(),
        sampling_config,
        Box::new(QuestionTestPersistence {
            fail_at,
            committed: committed.clone(),
        }),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    (std::sync::Arc::new(actor), gateway_rx, committed)
}

fn submit_plan_candidate(actor: &super::super::SessionActor, plan: &str) -> u64 {
    let mut behavior = actor.behavior.lock();
    assert!(behavior.select_behavior(tool_types::BehaviorId::Plan));
    behavior.record_plan_artifact(plan);
    assert!(behavior.submit_initial_plan());
    behavior
        .snapshot()
        .plan_submission_id
        .expect("submitted Plan has an identity")
}

async fn receive_question_ext_method(
    gateway_rx: &mut tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> acp_transport::AcpArgs<agent_client_protocol::schema::v1::ExtRequest> {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match gateway_rx.recv().await.expect("gateway remains open") {
                acp_transport::AcpClientMessage::SessionNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                acp_transport::AcpClientMessage::ExtMethod(args) => return args,
                _ => {}
            }
        }
    })
    .await
    .expect("question worker must send the reverse request")
}

fn interaction_events(events: &[chat_state::TimelineEvent]) -> Vec<chat_state::InteractionEvent> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Interaction(interaction) => Some(interaction.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn ask_user_question_request_carries_session_id() {
    let req = AskUserQuestionExtRequest {
        session_id: "sess-abc".to_string(),
        tool_call_id: "call-1".to_string(),
        questions: vec![],
        mode: AskUserQuestionMode::Default,
    };
    assert!(!req.session_id.is_empty());
    // Wire format is camelCase (`sessionId`); Tier-2 routing reads it.
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["sessionId"], "sess-abc");
    assert!(!json["sessionId"].as_str().unwrap().is_empty());
}

#[test]
fn plan_approval_request_carries_session_id() {
    let req = PlanApprovalExtRequest {
        session_id: "sess-xyz".to_string(),
        tool_call_id: "call-2".to_string(),
        plan_content: "plan".to_string(),
        submission_id: 1,
        artifact_revision: 2,
        artifact_hash: "a".repeat(64),
    };
    assert!(!req.session_id.is_empty());
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["sessionId"], "sess-xyz");
    assert!(!json["sessionId"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn plan_approval_reverse_request_requires_durable_open_and_resolution() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (failure, expected_reverse_calls) in [
                (FailQuestionPersistenceAt::Opened, 0),
                (FailQuestionPersistenceAt::Resolved, 1),
            ] {
                let (actor, mut gateway_rx, committed) =
                    question_worker_actor(Some(failure)).await;
                let plan = "# Approval boundary plan\n\n1. Verify the durable wait.\n";
                let submission_id = {
                    let mut behavior = actor.behavior.lock();
                    assert!(behavior.select_behavior(tool_types::BehaviorId::Plan));
                    behavior.record_plan_artifact(plan);
                    assert!(behavior.submit_initial_plan());
                    behavior
                        .snapshot()
                        .plan_submission_id
                        .expect("submitted Plan has an identity")
                };

                let reverse_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let observed_calls = reverse_calls.clone();
                let gateway_task = tokio::task::spawn_local(async move {
                    while let Some(message) = gateway_rx.recv().await {
                        match message {
                            acp_transport::AcpClientMessage::SessionNotification(args) => {
                                let _ = args.response_tx.send(Ok(()));
                            }
                            acp_transport::AcpClientMessage::ExtMethod(args) => {
                                observed_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let response =
                                    tools::implementations::grow_build::plan_control::PlanApprovalExtResponse {
                                        outcome: tools::implementations::grow_build::plan_control::PlanApprovalOutcome::Approved,
                                        feedback: None,
                                        submission_id,
                                        decision_id: "decision-1".into(),
                                    };
                                let raw = serde_json::value::to_raw_value(&response)
                                    .expect("valid Plan approval response serializes");
                                let _ = args.response_tx.send(Ok(
                                    acp_transport::protocol::ExtResponse::new(raw.into()),
                                ));
                            }
                            _ => {}
                        }
                    }
                });

                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    actor.request_plan_approval(
                        &agent_client_protocol::schema::v1::ToolCallId::new(
                            std::sync::Arc::<str>::from("plan-approval-boundary"),
                        ),
                        plan.into(),
                    ),
                )
                .await
                .expect("Plan request boundary resolves promptly");
                let error = result.as_ref().expect_err("the injected persistence failure is returned");
                let expected_boundary = match failure {
                    FailQuestionPersistenceAt::Opened => "plan approval interaction could not be opened",
                    FailQuestionPersistenceAt::Resolved => "plan approval interaction could not be durably resolved",
                };
                assert!(serde_json::to_string(error).unwrap().contains(expected_boundary), "the intended durable interaction boundary must fail: {error:?}");
                assert_eq!(
                    reverse_calls.load(std::sync::atomic::Ordering::Relaxed),
                    expected_reverse_calls,
                    "Plan reverse request must follow its durable Opened ACK"
                );
                assert!(
                    actor.behavior.lock().last_plan_handoff().is_none(),
                    "request boundary does not accept or commit a Plan handoff"
                );

                let interactions = interaction_events(
                    &committed
                        .lock()
                        .expect("committed Timeline log remains available"),
                );
                match failure {
                    FailQuestionPersistenceAt::Opened => {
                        assert!(interactions.is_empty(), "failed Opened is not committed");
                    }
                    FailQuestionPersistenceAt::Resolved => {
                        assert!(matches!(
                            interactions.as_slice(),
                            [chat_state::InteractionEvent::Opened {
                                tool_call_id,
                                attempt_id,
                                kind: chat_state::InteractionKind::PlanApproval,
                            }] if tool_call_id == "plan-approval-boundary"
                                && !attempt_id.is_empty()
                        ), "only the exact durable PlanApproval Opened is committed");
                    }
                }

                gateway_task.abort();
                actor.background_service_shutdown.cancel();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn plan_approval_transport_and_malformed_responses_close_the_exact_attempt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for malformed in [false, true] {
                let (actor, mut gateway_rx, committed) = question_worker_actor(None).await;
                let plan = "# Plan response fault\n\n1. Preserve the request boundary.\n";
                submit_plan_candidate(&actor, plan);
                let request_actor = actor.clone();
                let request =
                    tokio::task::spawn_local(async move {
                        request_actor
                            .request_plan_approval(
                                &agent_client_protocol::schema::v1::ToolCallId::new(
                                    std::sync::Arc::<str>::from("plan-response-fault"),
                                ),
                                plan.into(),
                            )
                            .await
                    });
                let ext_args = receive_question_ext_method(&mut gateway_rx).await;
                if malformed {
                    let raw = serde_json::value::to_raw_value(&serde_json::json!({
                        "outcome": "unrecognized"
                    }))
                    .expect("malformed response fixture serializes");
                    let _ = ext_args
                        .response_tx
                        .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
                } else {
                    drop(ext_args.response_tx);
                }

                let error = request
                    .await
                    .expect("Plan request task completes")
                    .expect_err("transport and malformed responses must fail closed");
                if malformed {
                    assert!(
                        serde_json::to_string(&error)
                            .unwrap()
                            .contains("invalid grow/plan_approval response")
                    );
                }
                let committed = committed
                    .lock()
                    .expect("test persistence log remains available")
                    .clone();
                assert!(
                    matches!(
                        interaction_events(&committed).as_slice(),
                        [
                            chat_state::InteractionEvent::Opened {
                                tool_call_id,
                                attempt_id,
                                kind: chat_state::InteractionKind::PlanApproval,
                            },
                            chat_state::InteractionEvent::Resolved {
                                tool_call_id: resolved_call_id,
                                attempt_id: resolved_attempt,
                                kind: chat_state::InteractionKind::PlanApproval,
                                resolution: chat_state::InteractionResolution::Failed,
                            }
                        ] if tool_call_id == "plan-response-fault"
                            && resolved_call_id == tool_call_id
                            && attempt_id == resolved_attempt
                            && !attempt_id.is_empty()
                    ),
                    "failed response must close the same typed PlanApproval attempt"
                );
                actor.background_service_shutdown.cancel();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn suspended_plan_approval_rebinds_with_a_fresh_attempt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway_rx, committed) = question_worker_actor(None).await;
            let plan = "# Plan resumed\n\n1. Re-open approval after reconnect.\n";
            submit_plan_candidate(&actor, plan);
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            let request_actor = actor.clone();
            let parked_request = tokio::task::spawn_local(async move {
                request_actor
                    .request_plan_approval(
                        &agent_client_protocol::schema::v1::ToolCallId::new(
                            std::sync::Arc::<str>::from("plan-rebind"),
                        ),
                        plan.into(),
                    )
                    .await
            });
            let ext_args = receive_question_ext_method(&mut gateway_rx).await;
            let before_rebind = interaction_events(
                &actor.chat_state_handle.timeline_events().await.unwrap(),
            );
            let old_attempt = match before_rebind.as_slice() {
                [chat_state::InteractionEvent::Opened { attempt_id, .. }] => attempt_id.clone(),
                other => panic!("expected one open Plan approval attempt, got {other:?}"),
            };
            runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
            drop(ext_args.response_tx);
            assert!(parked_request
                .await
                .expect("suspended request task completes")
                .is_err());
            assert!(matches!(
                interaction_events(&actor.chat_state_handle.timeline_events().await.unwrap())
                    .as_slice(),
                [
                    chat_state::InteractionEvent::Opened { attempt_id, .. },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: resolved_attempt,
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    }
                ] if attempt_id == resolved_attempt && attempt_id == &old_attempt
            ), "closing the old transport resolves that exact attempt as suspended");

            let (fresh_gateway_tx, mut fresh_gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (fresh_persistence_tx, _fresh_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel();
            let mut rebound = super::support::create_test_actor(
                0,
                256_000,
                85,
                fresh_gateway_tx,
                fresh_persistence_tx,
            )
            .await;
            rebound.session_info = actor.session_info.clone();
            rebound.chat_state_handle = actor.chat_state_handle.clone();
            rebound.events = crate::session::events::EventTracker::new(
                rebound.chat_state_handle.clone(),
            );
            rebound.behavior = actor.behavior.clone();
            rebound.pending_interactions = actor.pending_interactions.clone();
            let rebound = std::sync::Arc::new(rebound);
            let rebound_actor = rebound.clone();
            let resumed_request = tokio::task::spawn_local(async move {
                rebound_actor
                    .request_plan_approval(
                        &agent_client_protocol::schema::v1::ToolCallId::new(
                            std::sync::Arc::<str>::from("plan-rebind"),
                        ),
                        "# Plan resumed\n\n1. Re-open approval after reconnect.\n".into(),
                    )
                    .await
            });
            let ext_args = receive_question_ext_method(&mut fresh_gateway_rx).await;
            let response = tools::implementations::grow_build::plan_control::PlanApprovalExtResponse {
                outcome: tools::implementations::grow_build::plan_control::PlanApprovalOutcome::Approved,
                feedback: None,
                submission_id: actor.behavior.lock().snapshot().plan_submission_id.unwrap(),
                decision_id: "decision-after-rebind".into(),
            };
            let raw = serde_json::value::to_raw_value(&response)
                .expect("valid rebind response serializes");
            let _ = ext_args
                .response_tx
                .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
            assert!(resumed_request
                .await
                .expect("rebound request task completes")
                .is_ok());

            let events = interaction_events(
                &actor.chat_state_handle.timeline_events().await.unwrap(),
            );
            assert!(matches!(
                events.as_slice(),
                [
                    chat_state::InteractionEvent::Opened {
                        attempt_id: first,
                        ..
                    },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: resolved_first,
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    },
                    chat_state::InteractionEvent::Opened {
                        attempt_id: second,
                        ..
                    },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: resolved_second,
                        resolution: chat_state::InteractionResolution::Answered,
                        ..
                    }
                ] if first == resolved_first
                    && second == resolved_second
                    && first == &old_attempt
                    && second != first
            ), "rebind creates and resolves a fresh approval attempt");
            assert_eq!(
                interaction_events(
                    &committed
                        .lock()
                        .expect("test persistence log remains available")
                        .clone()
                )
                .len(),
                4
            );
            actor.background_service_shutdown.cancel();
            rebound.background_service_shutdown.cancel();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_question_does_not_stop_worker() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::{
        UserQuestionRequest, UserQuestionResponse,
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _prx) = tokio::sync::mpsc::unbounded_channel();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            tokio::task::spawn_local(async move {
                while let Some(message) = gateway_rx.recv().await {
                    match message {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let raw = serde_json::value::to_raw_value(&serde_json::json!({
                                "outcome": "cancelled"
                            }))
                            .unwrap();
                            let _ = args
                                .response_tx
                                .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let (first_tx, first_rx) = tokio::sync::oneshot::channel();
            drop(first_rx);
            tx.send(UserQuestionRequest {
                tool_call_id: "cancelled".into(),
                questions: vec![],
                deadline: None,
                result_tx: first_tx,
            })
            .unwrap();
            let (next_tx, next_rx) = tokio::sync::oneshot::channel();
            tx.send(UserQuestionRequest {
                tool_call_id: "next".into(),
                questions: vec![],
                deadline: None,
                result_tx: next_tx,
            })
            .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(&actor, rx, runtime_suspend);
            let response = tokio::time::timeout(std::time::Duration::from_secs(2), next_rx)
                .await
                .unwrap()
                .expect("worker must serve the next request")
                .unwrap();
            assert!(matches!(response, UserQuestionResponse::Cancelled));
            actor.background_service_shutdown.cancel();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn question_worker_resolves_user_outcomes_with_opened_durable_before_acp() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::{
        UserQuestionRequest, UserQuestionResponse,
    };

    tokio::task::LocalSet::new()
        .run_until(async {
            for (call_id, response_value, expected_resolution) in [
                (
                    "answered-question",
                    Some(serde_json::json!({"outcome":"accepted", "answers":{}})),
                    chat_state::InteractionResolution::Answered,
                ),
                (
                    "cancelled-question",
                    Some(serde_json::json!({"outcome":"cancelled"})),
                    chat_state::InteractionResolution::Cancelled,
                ),
                (
                    "malformed-question",
                    Some(serde_json::json!({"outcome":"unknown"})),
                    chat_state::InteractionResolution::Failed,
                ),
                (
                    "transport-question",
                    None,
                    chat_state::InteractionResolution::Failed,
                ),
            ] {
                let (actor, mut gateway_rx, _) = question_worker_actor(None).await;
                let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
                let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                request_tx
                    .send(UserQuestionRequest {
                        tool_call_id: call_id.into(),
                        questions: Vec::new(),
                        deadline: None,
                        result_tx,
                    })
                    .unwrap();
                let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
                super::super::spawn::start_user_question_worker(
                    &actor,
                    request_rx,
                    runtime_suspend,
                );

                let ext_args = receive_question_ext_method(&mut gateway_rx).await;
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let opened = interaction_events(&events);
                assert!(matches!(
                    opened.as_slice(),
                    [chat_state::InteractionEvent::Opened {
                        tool_call_id,
                        kind: chat_state::InteractionKind::Question,
                        ..
                    }] if tool_call_id == call_id
                ), "durable Opened must be queryable before ACP emits the question");

                if let Some(value) = response_value {
                    let raw = serde_json::value::to_raw_value(&value).unwrap();
                    let _ = ext_args
                        .response_tx
                        .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
                } else {
                    drop(ext_args.response_tx);
                }

                let result = tokio::time::timeout(std::time::Duration::from_secs(2), result_rx)
                    .await
                    .expect("question worker must resolve the request")
                    .expect("question worker must return a result");
                match (call_id, result) {
                    ("answered-question", Ok(UserQuestionResponse::Accepted { .. })) => {}
                    ("cancelled-question", Ok(UserQuestionResponse::Cancelled)) => {}
                    (
                        "malformed-question" | "transport-question",
                        Err(tools::implementations::grow_build::ask_user_question::UserQuestionError::MalformedResponse(_)
                            | tools::implementations::grow_build::ask_user_question::UserQuestionError::TransportError(_)),
                    ) => {}
                    (id, other) => panic!("unexpected result for {id}: {other:?}"),
                }

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let interactions = interaction_events(&events);
                assert!(matches!(
                    interactions.as_slice(),
                    [
                        chat_state::InteractionEvent::Opened { tool_call_id, attempt_id, .. },
                        chat_state::InteractionEvent::Resolved {
                            tool_call_id: resolved_id,
                            attempt_id: resolved_attempt,
                            resolution,
                            ..
                        }
                    ] if tool_call_id == call_id
                        && resolved_id == call_id
                        && attempt_id == resolved_attempt
                        && *resolution == expected_resolution
                ), "question attempt must have exactly its typed durable resolution");
                actor.background_service_shutdown.cancel();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn question_deadline_requires_closed_receiver_and_expired_shared_deadline() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::UserQuestionRequest;

    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway_rx, _) = question_worker_actor(None).await;
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(80);
            request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "timed-out-question".into(),
                    questions: Vec::new(),
                    deadline: Some(deadline),
                    result_tx,
                })
                .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(&actor, request_rx, runtime_suspend);
            let ext_args = receive_question_ext_method(&mut gateway_rx).await;
            tokio::time::sleep_until(deadline).await;
            drop(result_rx);
            drop(ext_args.response_tx);

            let events = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let events = actor.chat_state_handle.timeline_events().await.unwrap();
                    if interaction_events(&events).iter().any(|event| {
                        matches!(
                            event,
                            chat_state::InteractionEvent::Resolved {
                                resolution: chat_state::InteractionResolution::TimedOut,
                                ..
                            }
                        )
                    }) {
                        break events;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("closed result receiver after shared deadline must resolve as timeout");
            assert!(matches!(
                interaction_events(&events).as_slice(),
                [
                    chat_state::InteractionEvent::Opened { .. },
                    chat_state::InteractionEvent::Resolved {
                        resolution: chat_state::InteractionResolution::TimedOut,
                        ..
                    }
                ]
            ));
            actor.background_service_shutdown.cancel();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn question_worker_resolves_suspend_when_foreground_receiver_closes_before_shutdown() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::UserQuestionRequest;

    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway_rx, _) = question_worker_actor(None).await;
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "suspended-question-receiver-drop".into(),
                    questions: Vec::new(),
                    deadline: None,
                    result_tx,
                })
                .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(
                &actor,
                request_rx,
                runtime_suspend.clone(),
            );
            let ext_args = receive_question_ext_method(&mut gateway_rx).await;
            let opened =
                interaction_events(&actor.chat_state_handle.timeline_events().await.unwrap());
            assert!(matches!(
                opened.as_slice(),
                [chat_state::InteractionEvent::Opened { .. }]
            ));
            assert!(
                actor
                    .pending_interactions
                    .lock()
                    .unwrap()
                    .contains_key("suspended-question-receiver-drop")
            );

            // The foreground owner is aborted before suspend_node cancels
            // background services. The worker must classify this as a
            // suspension from runtime_suspend alone.
            runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
            drop(result_rx);
            assert!(!actor.background_service_shutdown.is_cancelled());

            let events = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let events = actor.chat_state_handle.timeline_events().await.unwrap();
                    if interaction_events(&events).iter().any(|event| {
                        matches!(
                            event,
                            chat_state::InteractionEvent::Resolved {
                                resolution: chat_state::InteractionResolution::Suspended,
                                ..
                            }
                        )
                    }) {
                        break events;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("closed foreground receiver during suspend must resolve as Suspended");
            assert!(matches!(
                interaction_events(&events).as_slice(),
                [
                    chat_state::InteractionEvent::Opened { attempt_id, .. },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: resolved_attempt,
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    }
                ] if attempt_id == resolved_attempt
            ));
            assert!(actor.pending_interactions.lock().unwrap().is_empty());
            actor.background_service_shutdown.cancel();
            drop(ext_args.response_tx);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn question_worker_never_reports_answer_when_open_or_resolution_persistence_fails() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::{
        UserQuestionError, UserQuestionRequest,
    };

    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway_rx, opened_committed) =
                question_worker_actor(Some(FailQuestionPersistenceAt::Opened)).await;
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            let mut result_rx = result_rx;
            request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "open-failure-question".into(),
                    questions: Vec::new(),
                    deadline: None,
                    result_tx,
                })
                .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(
                &actor,
                request_rx,
                runtime_suspend,
            );
            let (result, saw_acp) = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                async {
                    let mut saw_acp = false;
                    loop {
                        tokio::select! {
                            result = &mut result_rx => break (result.expect("worker result sender remains open"), saw_acp),
                            message = gateway_rx.recv() => match message.expect("gateway remains open") {
                                acp_transport::AcpClientMessage::SessionNotification(args) => {
                                    let _ = args.response_tx.send(Ok(()));
                                }
                                acp_transport::AcpClientMessage::ExtMethod(args) => {
                                    saw_acp = true;
                                    drop(args.response_tx);
                                }
                                _ => {}
                            }
                        }
                    }
                },
            )
            .await
            .expect("failed Opened commit must return promptly");
            assert!(matches!(result, Err(UserQuestionError::TransportError(_))));
            assert!(!saw_acp, "ACP request must follow durable Opened");
            assert!(interaction_events(
                &opened_committed
                    .lock()
                    .expect("test persistence log remains available")
                    .clone()
            )
            .is_empty());
            actor.background_service_shutdown.cancel();

            let (actor, mut gateway_rx, resolved_committed) =
                question_worker_actor(Some(FailQuestionPersistenceAt::Resolved)).await;
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "resolve-failure-question".into(),
                    questions: Vec::new(),
                    deadline: None,
                    result_tx,
                })
                .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(
                &actor,
                request_rx,
                runtime_suspend,
            );
            let ext_args = receive_question_ext_method(&mut gateway_rx).await;
            let raw = serde_json::value::to_raw_value(&serde_json::json!({
                "outcome": "accepted",
                "answers": {}
            }))
            .unwrap();
            let _ = ext_args
                .response_tx
                .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), result_rx)
                .await
                .expect("failed Resolved commit must return promptly")
                .expect("worker must return a result");
            assert!(matches!(result, Err(UserQuestionError::TransportError(_))));
            let committed = resolved_committed
                .lock()
                .expect("test persistence log remains available")
                .clone();
            let interactions = interaction_events(&committed);
            assert!(matches!(
                interactions.as_slice(),
                [chat_state::InteractionEvent::Opened { .. }]
            ), "a failed durable Resolved event cannot return the already-read answer");
            actor.background_service_shutdown.cancel();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn suspended_question_attempt_is_closed_and_fresh_attempt_can_reuse_call_id() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::{
        UserQuestionRequest, UserQuestionResponse,
    };

    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, mut gateway_rx, _) = question_worker_actor(None).await;
            let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (result_tx, result_rx) = tokio::sync::oneshot::channel();
            request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "reused-after-suspend".into(),
                    questions: Vec::new(),
                    deadline: None,
                    result_tx,
                })
                .unwrap();
            let runtime_suspend = actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(
                &actor,
                request_rx,
                runtime_suspend.clone(),
            );
            let ext_args = receive_question_ext_method(&mut gateway_rx).await;
            let opened =
                interaction_events(&actor.chat_state_handle.timeline_events().await.unwrap());
            let first_attempt = match opened.as_slice() {
                [chat_state::InteractionEvent::Opened { attempt_id, .. }] => attempt_id.clone(),
                other => panic!("expected one durable Opened before suspend, got {other:?}"),
            };

            runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
            actor.background_service_shutdown.cancel();
            drop(ext_args.response_tx);
            assert!(matches!(
                tokio::time::timeout(std::time::Duration::from_secs(2), result_rx)
                    .await
                    .expect("suspend must resolve the old request")
                    .expect("worker must return its suspended result"),
                Ok(UserQuestionResponse::Cancelled)
            ));
            let suspended =
                interaction_events(&actor.chat_state_handle.timeline_events().await.unwrap());
            assert!(matches!(
                suspended.as_slice(),
                [
                    chat_state::InteractionEvent::Opened { attempt_id, .. },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: resolved_attempt,
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    }
                ] if attempt_id == resolved_attempt && attempt_id == &first_attempt
            ));

            let (fresh_gateway_tx, mut fresh_gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (fresh_persistence_tx, _fresh_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel();
            let mut fresh_actor = super::support::create_test_actor(
                0,
                256_000,
                85,
                fresh_gateway_tx,
                fresh_persistence_tx,
            )
            .await;
            fresh_actor.session_info = actor.session_info.clone();
            fresh_actor.chat_state_handle = actor.chat_state_handle.clone();
            fresh_actor.pending_interactions = actor.pending_interactions.clone();
            let fresh_actor = std::sync::Arc::new(fresh_actor);
            let (fresh_request_tx, fresh_request_rx) = tokio::sync::mpsc::unbounded_channel();
            let (fresh_result_tx, fresh_result_rx) = tokio::sync::oneshot::channel();
            fresh_request_tx
                .send(UserQuestionRequest {
                    tool_call_id: "reused-after-suspend".into(),
                    questions: Vec::new(),
                    deadline: None,
                    result_tx: fresh_result_tx,
                })
                .unwrap();
            let fresh_runtime_suspend = fresh_actor.state.lock().await.runtime_suspend.clone();
            super::super::spawn::start_user_question_worker(
                &fresh_actor,
                fresh_request_rx,
                fresh_runtime_suspend,
            );
            let fresh_ext = receive_question_ext_method(&mut fresh_gateway_rx).await;
            let after_fresh_open = interaction_events(
                &fresh_actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap(),
            );
            let fresh_attempt = match after_fresh_open.as_slice() {
                [
                    chat_state::InteractionEvent::Opened { attempt_id, .. },
                    chat_state::InteractionEvent::Resolved {
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    },
                    chat_state::InteractionEvent::Opened {
                        attempt_id: new_attempt,
                        ..
                    },
                ] if new_attempt != attempt_id => new_attempt.clone(),
                other => panic!(
                    "fresh worker must open a distinct attempt for the same call id: {other:?}"
                ),
            };
            assert_ne!(fresh_attempt, first_attempt);
            let raw = serde_json::value::to_raw_value(&serde_json::json!({
                "outcome": "accepted",
                "answers": {}
            }))
            .unwrap();
            let _ = fresh_ext
                .response_tx
                .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
            assert!(matches!(
                tokio::time::timeout(std::time::Duration::from_secs(2), fresh_result_rx)
                    .await
                    .expect("fresh attempt must resolve")
                    .expect("fresh worker must return its result"),
                Ok(UserQuestionResponse::Accepted { .. })
            ));
            let events =
                interaction_events(&actor.chat_state_handle.timeline_events().await.unwrap());
            assert!(matches!(
                events.as_slice(),
                [
                    chat_state::InteractionEvent::Opened { attempt_id: old, .. },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: old_resolved,
                        resolution: chat_state::InteractionResolution::Suspended,
                        ..
                    },
                    chat_state::InteractionEvent::Opened { attempt_id: new, .. },
                    chat_state::InteractionEvent::Resolved {
                        attempt_id: new_resolved,
                        resolution: chat_state::InteractionResolution::Answered,
                        ..
                    }
                ] if old == old_resolved
                    && new == new_resolved
                    && old == &first_attempt
                    && new != old
            ));
            fresh_actor.background_service_shutdown.cancel();
        })
        .await;
}
