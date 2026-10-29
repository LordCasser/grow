//! Durable ACK fault injection around compaction replacement and Control
//! context reprojection.

use super::support::*;
use super::*;
use serde_json::json;
use std::sync::{Arc, Mutex};
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};

const END_TURN: &str = "end_turn";

fn prompt(text: impl Into<String>, index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(index);
    item
}

fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

fn messages_turn(blocks: &[(&str, &str)], stop_reason: &str) -> ScriptedResponse {
    let mut events = vec![
        json!({
            "type": "message_start",
            "message": {
                "id": "ack-fault-summary",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "test-model",
                "usage": { "input_tokens": 10, "output_tokens": 0,
                    "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0 }
            }
        })
        .to_string(),
    ];
    for (index, (text, _)) in blocks.iter().enumerate() {
        events.push(
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "text", "text": "" }
            })
            .to_string(),
        );
        events.push(
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": { "type": "text_delta", "text": text }
            })
            .to_string(),
        );
        events.push(json!({ "type": "content_block_stop", "index": index }).to_string());
    }
    events.push(
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason },
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        })
        .to_string(),
    );
    events.push(json!({ "type": "message_stop" }).to_string());
    ScriptedResponse::sse(events.into_iter().map(SseEvent::data).collect())
}

#[derive(Default)]
struct CompactionFaultLog {
    events: Vec<chat_state::TimelineEvent>,
    reprojection_attempts: Vec<chat_state::TimelineEvent>,
    replacement_seen: bool,
}

struct FailControlReprojectionAfterReplacement {
    log: Arc<Mutex<CompactionFaultLog>>,
    fault_injected: bool,
}

impl chat_state::TimelinePersistence for FailControlReprojectionAfterReplacement {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let mut log = self.log.lock().unwrap();
        if let Some(previous) = log.events.iter().find(|previous| previous.seq == event.seq) {
            assert_eq!(
                serde_json::to_value(previous).unwrap(),
                serde_json::to_value(event).unwrap(),
                "a durable retry must reuse its exact Timeline event"
            );
        } else {
            log.events.push(event.clone());
        }
        if matches!(
            &event.kind,
            chat_state::TimelineEventKind::Messages(message)
                if message.cause == chat_state::MessageCause::Compaction
        ) {
            log.replacement_seen = true;
        }
        let is_reprojection = log.replacement_seen
            && matches!(
                &event.kind,
                chat_state::TimelineEventKind::Control(control)
                    if control.model_contexts.iter().any(|context| {
                        context.activation == chat_state::ControlContextActivation::Reprojection
                    })
            );
        if is_reprojection {
            log.reprojection_attempts.push(event.clone());
            if !self.fault_injected {
                // Retain the exact event, then lose its ACK. The actor must
                // retry the same seq/payload before the permanent failure.
                self.fault_injected = true;
                drop(reply);
            } else {
                let _ = reply.send(Err(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "simulated permanent failure confirming Control reprojection",
                )));
            }
        } else {
            let _ = reply.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

#[test]
fn compaction_reprojection_and_repair_failure_leaves_only_the_committed_timeline_prefix() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let summary = format!(
                "Summary of replaced history only: {}",
                "verified prior work and decisions. ".repeat(35)
            );
            server.enqueue_response(
                "/v1/messages",
                messages_turn(&[(&summary, END_TURN)], END_TURN),
            );
            let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (session_persistence_tx, mut session_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel();
            tokio::task::spawn_local(async move {
                while let Some(message) = session_persistence_rx.recv().await {
                    if let crate::session::persistence::PersistenceMsg::SidebandDurablyAndAck {
                        respond_to,
                        ..
                    } = message
                    {
                        let _ = respond_to.send(Ok(()));
                    }
                }
            });
            let (mut actor, planning_rx) = create_test_actor_with_planning_receiver(
                0,
                100_000,
                85,
                gateway_tx,
                session_persistence_tx,
            )
            .await;
            let (goal_usage_tx, mut goal_usage_rx) = tokio::sync::mpsc::unbounded_channel();
            actor.goal_usage_window =
                goal_support::GoalUsageWindow::new(goal_usage_tx.clone(), None);
            let mut sampling = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling route");
            sampling.base_url = server.url();
            sampling.api_backend = sampling_types::ApiBackend::Messages;

            let log = Arc::new(Mutex::new(CompactionFaultLog::default()));
            let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
            let cancellation = tokio_util::sync::CancellationToken::new();
            let handle = chat_state::ChatStateActor::spawn(
                vec![ConversationItem::system("test system prompt")],
                sampling,
                Box::new(FailControlReprojectionAfterReplacement {
                    log: log.clone(),
                    fault_injected: false,
                }),
                chat_event_tx,
                cancellation,
            );
            actor.chat_state_handle = handle.clone();
            actor.events = crate::session::events::EventTracker::new(handle.clone());

            let sampler_config = sampler::SamplerConfig {
                api_key: Some("test-key".into()),
                base_url: server.url(),
                model: "test-model".into(),
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
                tokio::sync::mpsc::unbounded_channel::<sampler::SamplingEvent>();
            actor.sampler_handle = sampler::SamplerActor::spawn(
                sampler_config,
                sampler::RetryPolicy::default(),
                sampler_event_tx,
            );
            let actor = Arc::new(actor);
            start_test_planning_driver(&actor, planning_rx);
            let usage_actor = actor.clone();
            tokio::task::spawn_local(async move {
                let _mailbox_owner = goal_usage_tx;
                while let Some(crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
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
            let event_actor = actor.clone();
            tokio::task::spawn_local(async move {
                while let Some(event) = sampler_event_rx.recv().await {
                    event_actor.handle_sampling_event(event).await;
                }
            });

            actor
                .chat_state_handle
                .push_user_message(prompt("old task", 0));
            actor
                .persist_control_snapshot_with_contexts_durably(
                    actor.behavior.lock().snapshot(),
                    None,
                    Some("test-agent"),
                    vec![(
                        chat_state::ControlContextLayer::AgentRole,
                        chat_state::ControlContextActivation::Transition,
                        ConversationItem::system_reminder("current agent authority"),
                    )],
                )
                .await
                .expect("seed active Control context");
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("x".repeat(24_000)));
            actor
                .chat_state_handle
                .push_user_message(prompt("newer task", 1));
            actor
                .chat_state_handle
                .push_assistant_response(ConversationItem::assistant("y".repeat(88_000)));
            let _ = actor.chat_state_handle.get_conversation_len().await;
            actor.compaction.pre_prune.set(false);

            let error = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                actor.run_compact_only(compaction::AutoCompactTriggerInfo {
                    tokens_used: 90_000,
                    context_window: 100_000,
                    percentage: 90,
                    source: "control_reprojection_ack_fault",
                    max_output_tokens: None,
                }),
            )
            .await
            .expect("the injected post-commit failure must settle")
            .expect_err("repair failure must close this compaction turn");
            assert!(
                error.to_string().contains("Control context")
                    || error.to_string().contains("compaction terminal"),
                "the failure should preserve the post-commit Control/terminal boundary: {error}"
            );
            assert_eq!(
                server.messages_request_count(),
                1,
                "the failed post-commit control repair does not dispatch another provider request"
            );

            let log_state = log.lock().unwrap();
            assert_eq!(
                log_state.reprojection_attempts.len(),
                2,
                "the lost reprojection ACK is retried once before the injected permanent failure"
            );
            assert_eq!(
                serde_json::to_value(&log_state.reprojection_attempts[0]).unwrap(),
                serde_json::to_value(&log_state.reprojection_attempts[1]).unwrap(),
                "reprojection retry must preserve the exact event"
            );
            let retained = log_state
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(message)
                            if message.cause == chat_state::MessageCause::Compaction
                    )
                })
                .count();
            assert_eq!(
                retained, 1,
                "replacement is physically present exactly once"
            );
            let attempts_before = log_state.events.len();
            let events = log_state.events.clone();
            drop(log_state);

            let rejected = actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "compaction_ack_fault_test".into(),
                        name: "must_not_follow_failed_control_repair".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await;
            assert!(
                rejected.is_err(),
                "the poisoned writer must reject follow-up work"
            );
            assert_eq!(
                log.lock().unwrap().events.len(),
                attempts_before,
                "no new durable event follows the failed Control repair"
            );

            let mut replayed = chat_state::Timeline::from_events(events)
                .expect("physically retained replacement and reprojection replay");
            assert!(
                replayed
                    .settle_open_compaction("process_interrupted")
                    .unwrap()
                    .is_some()
            );
            let surface = replayed.surface();
            assert!(
                surface
                    .iter()
                    .any(|item| item.text_content().contains("Summary of replaced history"))
            );
            assert!(
                surface
                    .iter()
                    .any(|item| item.text_content() == "current agent authority")
            );
            assert_eq!(
                replayed
                    .events()
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Compaction(
                            chat_state::CompactionEvent::Completed { .. }
                        )
                    ))
                    .count(),
                1,
                "cold recovery closes the committed replacement exactly once"
            );
        }));
    });
}
