//! Real sampler/Turn regressions for the passive message delivery boundary.
use super::*;
use std::sync::Arc;

#[path = "message_delivery_joint_tests.rs"]
mod joint_tests;

#[path = "message_delivery_cancellation_tests.rs"]
mod cancellation_tests;

#[path = "message_delivery_frame_tests.rs"]
mod frame_tests;

#[path = "message_delivery_lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "message_delivery_ack_tests.rs"]
mod ack_tests;

async fn inject(actor: &SessionActor, id: &str) {
    actor
        .receive_notification(
            chat_state::NotificationSource::InquiryAnswered {
                source_session_id: "consulting-child".into(),
                inquiry_id: id.into(),
            },
            chat_state::NotificationSourceVersion::Opaque {
                value: format!("answer-{id}"),
            },
            format!("Full consultation question and answer: {id}"),
        )
        .await
        .unwrap();
}

async fn communication_agent(actor: &SessionActor) {
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::todo::TodoWriteTool,
        >(),
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::read_file::ReadFileTool,
        >(),
    ])
    .await;
}

fn live_consultation(
    actor: &SessionActor,
    inquiry_id: &str,
    question: &str,
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
            source_session_id: "live-child-session".into(),
            source_cwd: actor.session_info.cwd.clone(),
            delegated_subagent_task_name: Some("investigation".into()),
            target_session_id: actor.session_info.id.to_string(),
            question: question.into(),
            cancellation: InquiryCancellation::new(),
            progress,
            respond_to,
        },
        response,
    )
}

#[test]
fn passive_context_does_not_cancel_blocked_sampling_and_continues_same_turn() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut first = server.expect_response_blocked(
                "ordinary final",
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(&[text_block("Original completed response.")], END_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("Consultation incorporated.")], END_TURN),
            );
            let (actor, _) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            communication_agent(&actor).await;
            let (result, ()) = tokio::join!(run_user_turn(&actor, "passive-stream"), async {
                tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked())
                    .await
                    .unwrap();
                inject(&actor, "during-stream").await;
                assert!(actor.pending_interjections.is_empty());
                assert!(!*actor.parent_message_interrupt.borrow());
                assert_eq!(actor.sampler_handle.active_count().await, 1);
                assert!(!first.is_satisfied());
                first.release();
            });
            result.unwrap();
            assert_eq!(server.requests().len(), 2);
            assert!(
                !server.request_bodies()[0]
                    .to_string()
                    .contains("during-stream")
            );
            assert!(
                server.request_bodies()[1]
                    .to_string()
                    .contains("during-stream")
            );
            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                timeline
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                    ))
                    .count(),
                1
            );
            assert!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .is_empty()
            );
        }));
    });
}

struct FailNotificationConsumption;
impl chat_state::TimelinePersistence for FailNotificationConsumption {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let result = if matches!(
            event.kind,
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed { .. }
            )
        ) {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "injected passive consumption failure",
            ))
        } else {
            Ok(())
        };
        let _ = sender.send(result);
        receiver
    }
    fn flush(&mut self) {}
}

async fn failing_actor(server: &MockInferenceServer) -> Arc<SessionActor> {
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
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
    let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    let history = actor.chat_state_handle.timeline_events().await.unwrap();
    let (event_tx, _) = mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        history,
        config,
        Box::new(FailNotificationConsumption),
        event_tx,
        Default::default(),
    )
    .await
    .unwrap();
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
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
    communication_agent(&actor).await;
    actor
}

#[test]
fn passive_consumption_failure_blocks_first_successor_and_final_candidate_requests() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            for stage in ["first", "successor", "final"] {
                let server = MockInferenceServer::start().await.unwrap();
                let actor = failing_actor(&server).await;
                if stage == "first" {
                    inject(&actor, stage).await;
                    server.enqueue_response("/v1/messages", messages_turn(&[text_block("must not execute")], END_TURN));
                    let error = run_user_turn(&actor, "passive-first-fault").await.unwrap_err();
                    assert!(crate::session::commands::is_fatal_turn_boundary_error(&error), "{error:?}");
                    assert!(format!("{error:?}").contains("notification consumption"), "{error:?}");
                    assert_eq!(server.requests().len(), 0);
                } else {
                    let response = if stage == "successor" {
                        messages_turn(&[tool_use_block("passive-tool", "todo_write", r#"{"todos":[{"id":"done","content":"completed tool batch","status":"completed"}]}"#)], "tool_use")
                    } else { messages_turn(&[text_block("finished first request")], END_TURN) };
                    let mut first = server.expect_response_blocked(stage, test_support::InferenceRequestMatcher::foreground(test_support::InferenceEndpoint::Messages), response);
                    server.enqueue_response("/v1/messages", messages_turn(&[text_block("illegal dependent request")], END_TURN));
                    let (result, ()) = tokio::join!(run_user_turn(&actor, "passive-boundary-fault"), async {
                        tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked()).await.unwrap();
                        inject(&actor, stage).await;
                        first.release();
                    });
                    let error = result.unwrap_err();
                    assert!(crate::session::commands::is_fatal_turn_boundary_error(&error), "{stage}: {error:?}");
                    assert!(format!("{error:?}").contains("notification consumption"), "{stage}: {error:?}");
                    assert_eq!(server.requests().len(), 1, "{stage}");
                }
            }
        }));
    });
}

#[test]
fn queued_consultation_reaches_same_turn_successor_without_preempting_foreground() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::coordination::{InquiryEvent, InquiryStatus};
            let server = MockInferenceServer::start().await.unwrap();
            let mut foreground = server.expect_response_blocked(
                "foreground while answering consultation",
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(&[text_block("The original work is complete.")], END_TURN),
            );
            let consultation = server.expect_response(
                "actual InfoRequest response",
                test_support::InferenceRequestMatcher::auxiliary(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(&[text_block("Preserve the completed investigation.")], END_TURN),
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[text_block("The consultation is now part of my context.")], END_TURN),
            );
            let (actor, _) = actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            communication_agent(&actor).await;
            let inquiry_id = uuid::Uuid::now_v7().to_string();
            let make_inquiry = || live_consultation(&actor, &inquiry_id, "Should I preserve the completed investigation?");
            let (result, receipt_id) = tokio::join!(run_user_turn(&actor, "live-consultation"), async {
                tokio::time::timeout(std::time::Duration::from_secs(5), foreground.wait_blocked()).await.unwrap();
                let before = serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap();
                let (inquiry, response) = make_inquiry();
                actor.enqueue_coordination_inquiry(inquiry).await;
                let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), response).await.unwrap().unwrap();
                assert_eq!(outcome.status, InquiryStatus::Answered, "{outcome:?}");
                assert_eq!(outcome.answer.as_deref(), Some("Preserve the completed investigation."));
                assert!(consultation.is_satisfied());
                assert!(!foreground.is_satisfied());
                assert_eq!(actor.sampler_handle.active_count().await, 1);
                assert!(actor.pending_interjections.is_empty());
                assert!(!*actor.parent_message_interrupt.borrow());
                assert_eq!(serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(), before);
                let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
                assert_eq!(pending.len(), 1);
                assert!(matches!(&pending[0].source, chat_state::NotificationSource::InquiryAnswered { source_session_id, inquiry_id: id }
                    if source_session_id == "live-child-session" && id == &inquiry_id));
                let receipt_id = pending[0].id.clone();
                assert_eq!(server.requests().len(), 2, "only the in-flight foreground and InfoRequest have run");
                foreground.release();
                receipt_id
            });
            result.unwrap();
            let bodies = server.request_bodies();
            assert_eq!(bodies.len(), 3, "one original, one consultation, one same-Turn successor");
            assert!(!bodies[0].to_string().contains("Should I preserve"));
            assert!(bodies[1].to_string().contains("Should I preserve"));
            let successor = bodies[2].to_string();
            assert!(successor.contains("Should I preserve the completed investigation?"));
            assert!(successor.contains("Preserve the completed investigation."));
            assert!(successor.contains("live-child-session"));
            assert!(successor.contains("not a foreground decision or human permission"));
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. }))).count(), 1);
            assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Sideband(spawn) if spawn.purpose == chat_state::SidebandPurpose::InfoRequest)).count(), 1);
            assert_eq!(events.iter().filter_map(|event| InquiryEvent::from_timeline(event).unwrap()).filter(|event| matches!(event,
                InquiryEvent::Incoming { inquiry_id: id, audit } if id == &inquiry_id && audit.answer_context.is_some())).count(), 1);
            assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. })
                    if notification_ids.contains(&receipt_id))).count(), 1);
            let context = actor.chat_state_handle.get_conversation().await;
            let consultation_inputs = context.iter().filter_map(|item| match item {
                sampling_types::ConversationItem::User(message) if item.text_content().contains("Consultation record:") => Some(message), _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(consultation_inputs.len(), 1);
            assert!(consultation_inputs.iter().all(|message| message.permission_evidence.is_none()));
            assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            let (retry, response) = make_inquiry();
            actor.enqueue_coordination_inquiry(retry).await;
            assert_eq!(response.await.unwrap().status, InquiryStatus::Answered);
            assert_eq!(server.requests().len(), 3, "outcome retry does not rerun consultation or foreground");
            assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
        }));
    });
}

async fn answer_live_consultation(
    actor: &Arc<SessionActor>,
    server: &MockInferenceServer,
    inquiry_id: &str,
    question: &str,
    answer: &str,
) -> crate::coordination::InquiryOutcome {
    let expected = server.expect_response(
        inquiry_id,
        test_support::InferenceRequestMatcher::auxiliary(test_support::InferenceEndpoint::Messages),
        messages_turn(&[text_block(answer)], END_TURN),
    );
    let (inquiry, response) = live_consultation(actor, inquiry_id, question);
    actor.enqueue_coordination_inquiry(inquiry).await;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(15), response)
        .await
        .unwrap()
        .unwrap();
    assert!(expected.is_satisfied());
    outcome
}

#[test]
fn consultation_payload_limit_is_admitted_before_canonical_success() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::coordination::{InquiryEvent, InquiryStatus};
            let server = MockInferenceServer::start().await.unwrap();
            let records = Arc::new(std::sync::Mutex::new(crate::session::storage::SidebandLedgers::new()));
            let captured = records.clone();
            let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            tokio::task::spawn_local(async move {
                while let Some(message) = persistence_rx.recv().await {
                    if let PersistenceMsg::SidebandDurablyAndAck { event, respond_to, .. } = message {
                        captured.lock().unwrap().entry(event.sideband_id.clone()).or_default().push(event);
                        let _ = respond_to.send(Ok(()));
                    }
                }
            });
            let (actor, events) = create_test_actor_ex_with_projection_error(0, 256_000, 85, gateway_tx, persistence_tx, None).await;
            let (actor, _gateway, _) = configure_actor_with_sampler(&server, sampling_types::ApiBackend::Messages,
                sampler::OutputDelivery::Irreversible, None, actor, gateway_rx, events).await;
            communication_agent(&actor).await;
            let question = "保留调查吗？ full rendered question overhead";
            let probe = answer_live_consultation(&actor, &server, "size-probe", question, "界").await;
            assert_eq!(probe.status, InquiryStatus::Answered, "{probe:?}");
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let audit = events.iter().find_map(|event| match InquiryEvent::from_timeline(event).unwrap() {
                Some(InquiryEvent::Incoming { inquiry_id, audit }) if inquiry_id == "size-probe" && audit.answer_context.is_some() => Some(audit),
                _ => None,
            }).unwrap();
            let mut oversized_prefix = events.iter().take_while(|event| !matches!(&event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received { .. }))).cloned().collect::<Vec<_>>();
            let probe_ref = audit.answer_context.as_ref().unwrap().result_ref.clone();
            let overhead = audit.answer_context_body().unwrap().len() - "界".len();
            let limit = chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES as usize;
            let budget = limit - overhead;
            let mut exact_answer = "界".repeat(budget / 3);
            exact_answer.push_str(&"x".repeat(budget % 3));
            let exact = answer_live_consultation(&actor, &server, "exact-size", question, &exact_answer).await;
            assert_eq!(exact.status, InquiryStatus::Answered, "{exact:?}");
            let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
            assert_eq!(pending.len(), 2);
            assert_eq!(pending.last().unwrap().payload_ref.bytes, limit as u64, "actual full UTF-8 body exactly fits");
            let mut oversized_answer = exact_answer;
            oversized_answer.push('x');
            let oversized = answer_live_consultation(&actor, &server, "one-byte-over", question, &oversized_answer).await;
            assert_eq!(oversized.status, InquiryStatus::Failed);
            assert!(oversized.answer.is_none());
            assert!(oversized.error.unwrap().to_string().contains("notification payload limit"));
            assert_eq!(actor.chat_state_handle.pending_notifications().await.unwrap().len(), 2);
            actor.reconcile_coordination_answers().await.unwrap();
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let failure = events.iter().find_map(|event| match InquiryEvent::from_timeline(event).unwrap() {
                Some(InquiryEvent::Incoming { inquiry_id, audit }) if inquiry_id == "one-byte-over" && audit.outcome.is_some() => Some(audit),
                _ => None,
            }).unwrap();
            assert!(failure.answer_context.is_none());
            let ledgers = records.lock().unwrap().clone();
            assert_eq!(ledgers.len(), 3);
            assert!(ledgers.values().all(|ledger| matches!(&ledger.last().unwrap().kind,
                chat_state::SidebandEventKind::End(end) if end.outcome == chat_state::SidebandOutcome::Completed)));
            assert!(ledgers.values().any(|ledger| ledger.iter().any(|event| matches!(&event.kind,
                chat_state::SidebandEventKind::Result(result) if result.raw_output == oversized_answer))));
            crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(),
                &chat_state::Timeline::from_events(events).unwrap(), &ledgers).unwrap();

            // Even without any Received fact, imported canonical success must
            // be deliverable. Keep Result/question binding valid to isolate cap.
            let impossible_answer = "x".repeat(limit);
            for event in &mut oversized_prefix {
                if let Some(InquiryEvent::Incoming { inquiry_id, mut audit }) = InquiryEvent::from_timeline(event).unwrap()
                    && audit.answer_context.is_some()
                {
                    audit.outcome.as_mut().unwrap().answer = Some(impossible_answer.clone());
                    event.kind = InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                }
            }
            let mut oversized_ledgers = ledgers.clone();
            oversized_ledgers.retain(|id, _| id == &probe_ref.timeline_id);
            for event in oversized_ledgers.values_mut().flatten() {
                if let chat_state::SidebandEventKind::Result(result) = &mut event.kind {
                    result.raw_output = impossible_answer.clone();
                }
            }
            let error = crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(),
                &chat_state::Timeline::from_events(oversized_prefix).unwrap(), &oversized_ledgers).unwrap_err();
            assert!(error.to_string().contains("notification payload limit"), "{error}");

            // The original 1 MiB answer case must fail the consultation without
            // leaving an impossible obligation that blocks the next real Turn.
            let next_server = MockInferenceServer::start().await.unwrap();
            let (next_actor, _) = actor_with_sampler(&next_server, sampling_types::ApiBackend::Messages).await;
            communication_agent(&next_actor).await;
            let mut raw_mebibyte = "界".repeat(limit / 3);
            raw_mebibyte.push_str(&"x".repeat(limit % 3));
            assert_eq!(raw_mebibyte.len(), limit);
            let failed = answer_live_consultation(&next_actor, &next_server, "raw-mebibyte", question, &raw_mebibyte).await;
            assert_eq!(failed.status, InquiryStatus::Failed);
            assert!(next_actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            next_actor.reconcile_coordination_answers().await.unwrap();
            next_server.enqueue_response("/v1/messages", messages_turn(&[text_block("Future foreground is still usable.")], END_TURN));
            run_user_turn(&next_actor, "after-oversized-consultation").await.unwrap();
            assert_eq!(next_server.requests().len(), 2, "one completed Sideband and one healthy foreground request");
        }));
    });
}
