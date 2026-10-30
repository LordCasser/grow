use super::*;
use chat_state::{TimelineEvent, TimelineEventKind};
use std::sync::{Arc, Mutex};

struct HandoffPersistence {
    fail_handoff: bool,
    committed: Arc<Mutex<Vec<TimelineEvent>>>,
}

impl chat_state::TimelinePersistence for HandoffPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let handoff = matches!(&event.kind, TimelineEventKind::Messages(message)
            if message.items.iter().any(|item| item.text_content().contains(HANDOFF_MARKER)));
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.fail_handoff && handoff {
            let _ = tx.send(Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "injected-route-handoff-failure",
            )));
        } else {
            self.committed.lock().unwrap().push(event.clone());
            let _ = tx.send(Ok(()));
        }
        rx
    }

    fn flush(&mut self) {}
}

async fn select_responses(
    actor: &Arc<SessionActor>,
    server: &MockInferenceServer,
) -> tokio::sync::oneshot::Receiver<
    Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
> {
    let mut selected = actor.reconstruct_full_config().await;
    selected.model = "selected-model".into();
    selected.base_url = server.url();
    selected.api_backend = sampling_types::ApiBackend::Responses;
    let model_id = crate::agent::models::ModelId::new("test/selected-model");
    let catalog = SessionActor::published_catalog_for_test(
        model_id.clone(),
        selected,
        None,
        std::time::Duration::from_secs(60),
        0,
        85,
    );
    let route = catalog.resolve_session_route(&model_id, None).unwrap();
    let (ack, applied) = tokio::sync::oneshot::channel();
    actor
        .admit_session_model_selection(route, Some(catalog), None, ack)
        .await;
    applied
}

#[derive(Clone, Copy)]
enum Boundary {
    FailedAppend,
    Cancel,
    ManualCompaction,
    FirstStep,
}

#[test]
fn handoff_append_failure_stops_before_next_step_or_provider() {
    run_boundary(Boundary::FailedAppend);
}

#[test]
fn cancelled_route_switch_has_no_handoff_or_extra_request() {
    run_boundary(Boundary::Cancel);
}

#[test]
fn manual_compaction_takes_over_without_route_handoff() {
    run_boundary(Boundary::ManualCompaction);
}

#[test]
fn initial_route_selection_uses_the_real_user_input_without_handoff() {
    run_boundary(Boundary::FirstStep);
}

fn run_boundary(boundary: Boundary) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
            let server = MockInferenceServer::start().await.unwrap();
            let mut first = server.expect_response_blocked(
                "original route tool response",
                InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                messages_tool_call("boundary-call", "old_reasoning"),
            );
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "done",
                    "selected-model",
                )),
            );
            let (gateway_tx, mut gateway_rx) =
                mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            tokio::task::spawn_local(async move {
                while let Some(message) = gateway_rx.recv().await {
                    match message {
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
            let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
            tokio::task::spawn_local(async move { while persistence_rx.recv().await.is_some() {} });
            let (mut actor, planning_rx) = create_test_actor_with_planning_receiver(
                0,
                100_000,
                85,
                gateway_tx,
                persistence_tx,
            )
            .await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig::for_tool::<TodoWriteTool>(),
                ToolConfig::for_tool::<ReadFileTool>(),
            ])
            .await;
            let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            config.base_url = server.url();
            config.api_backend = sampling_types::ApiBackend::Messages;
            let committed = Arc::new(Mutex::new(Vec::new()));
            let (chat_tx, _chat_rx) = mpsc::unbounded_channel();
            actor.chat_state_handle = chat_state::ChatStateActor::spawn(
                vec![ConversationItem::system("test system prompt")],
                config,
                Box::new(HandoffPersistence {
                    fail_handoff: matches!(boundary, Boundary::FailedAppend),
                    committed: committed.clone(),
                }),
                chat_tx,
                tokio_util::sync::CancellationToken::new(),
            );
            actor.events =
                crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
            let config = actor.reconstruct_full_config().await;
            actor
                .model_route
                .replace(actor.model_route.snapshot().model_id, config.clone());
            let (sampler_tx, mut sampler_rx) = mpsc::unbounded_channel();
            actor.sampler_handle =
                sampler::SamplerActor::spawn(config, sampler::RetryPolicy::default(), sampler_tx);
            let actor = Arc::new(actor);
            start_test_planning_driver(&actor, planning_rx);
            let drainer = actor.clone();
            tokio::task::spawn_local(async move {
                while let Some(event) = sampler_rx.recv().await {
                    drainer.handle_sampling_event(event).await;
                }
            });

            let result = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                if matches!(boundary, Boundary::FirstStep) {
                    // Queue under the future turn owner, before its initial boundary.
                    actor.state.lock().await.foreground =
                        ForegroundState::RegularTurn(running_task_stub("boundary"));
                    let applied = select_responses(&actor, &server).await;
                    let result = run_user_turn(&actor, "boundary").await;
                    applied.await.unwrap().unwrap();
                    return result;
                }
                let turn = tokio::task::spawn_local({
                    let actor = actor.clone();
                    async move { run_user_turn(&actor, "boundary").await }
                });
                first.wait_blocked().await;
                let applied = select_responses(&actor, &server).await;
                match boundary {
                    Boundary::Cancel => actor
                        .cancel_running_task(false, false, false, Some("ctrl_c".into()))
                        .await
                        .unwrap(),
                    Boundary::ManualCompaction => {
                        actor.state.lock().await.pending_manual_compact = Some(None)
                    }
                    _ => {}
                }
                first.release();
                let result = turn.await.unwrap();
                if !matches!(boundary, Boundary::Cancel) {
                    applied.await.unwrap().unwrap();
                }
                result
            })
            .await
            .expect("boundary must settle without retrying or deadlocking");

            if matches!(boundary, Boundary::FailedAppend) {
                let error =
                    serde_json::to_string(&result.expect_err("handoff failure is fatal")).unwrap();
                assert!(error.contains("injected-route-handoff-failure"), "{error}");
                assert!(
                    actor.chat_state_handle.pending_timeline_commit().is_some(),
                    "failure reached durable append"
                );
            } else if !matches!(boundary, Boundary::Cancel) {
                result.unwrap();
            }
            assert_eq!(
                server.request_bodies().len(),
                1,
                "no successor request on a closed/failed boundary"
            );
            let history = committed.lock().unwrap().clone();
            assert!(!history.iter().any(
                |event| matches!(&event.kind, TimelineEventKind::Messages(message)
                if message.items.iter().any(|item| item.text_content().contains(HANDOFF_MARKER)))
            ));
            assert_eq!(
                history
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        TimelineEventKind::Step(chat_state::StepEvent::Started { .. })
                    ))
                    .count(),
                1
            );
            chat_state::Timeline::from_events(history)
                .expect("committed prefix remains replayable");
            if matches!(boundary, Boundary::FirstStep) {
                let request = server.request_bodies()[0].to_string();
                assert!(request.contains("write a long answer"));
                assert!(!request.contains(HANDOFF_MARKER));
            }
        }));
    });
}
