//! Exercise actual model/tool admission and terminalization with durable faults.
use super::support::*;
use super::*;
use chat_state::{TimelineEvent, TimelineEventKind, ToolEvent};
use serde_json::json;
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};

#[derive(Clone, Copy, Debug)]
enum Fault {
    Start,
    Result,
    Terminal,
}

struct FailingToolPersistence {
    fault: Fault,
    events: Arc<std::sync::Mutex<Vec<TimelineEvent>>>,
    failed: Arc<std::sync::atomic::AtomicBool>,
}
impl chat_state::TimelinePersistence for FailingToolPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let matches = match (&self.fault, &event.kind) {
            (Fault::Start, TimelineEventKind::Tool(ToolEvent::Started { .. })) => true,
            (Fault::Terminal, TimelineEventKind::Tool(ToolEvent::Completed { .. })) => true,
            (Fault::Result, TimelineEventKind::Messages(message)) => message
                .items
                .iter()
                .any(|item| matches!(item, sampling_types::ConversationItem::ToolResult(_))),
            _ => false,
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        if matches || self.failed.load(std::sync::atomic::Ordering::SeqCst) {
            self.failed.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                format!("injected-tool-{:?}-failure", self.fault),
            )));
        } else {
            self.events.lock().unwrap().push(event.clone());
            let _ = tx.send(Ok(()));
        }
        rx
    }
    fn flush(&mut self) {}
}

async fn run_fault(fault: Fault) {
    let server = MockInferenceServer::start().await.unwrap();
    server.enqueue_response("/v1/chat/completions", ScriptedResponse::sse(vec![
        SseEvent::data(json!({
            "id":"tool-response", "object":"chat.completion.chunk", "created":0,
            "model":"test-model", "choices":[{"index":0,"delta":{"tool_calls":[{
                "index":0,"id":"call_fault","type":"function",
                "function":{"name":"read_file","arguments":"{\"target_file\":\"missing-tool-fault-fixture\"}"}
            }]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        }).to_string()), SseEvent::data("[DONE]")
    ]));
    server.enqueue_response(
        "/v1/chat/completions",
        ScriptedResponse::sse(test_support::sse::chat_completion_script_exact(
            "must not sample again",
            "test-model",
        )),
    );
    let (gateway, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
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
    let (persistence, mut persisted) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move { while persisted.recv().await.is_some() {} });
    let mut actor = create_test_actor(0, 256_000, 85, gateway, persistence).await;
    // Two real tools keep the mock's foreground classifier on its main route.
    let tool_agent = test_agent_with_tools(vec![
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::read_file::ReadFileTool,
        >(),
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::LoadSkillTool,
        >(),
    ])
    .await;
    let mut definition = tool_agent.definition().clone();
    definition.completion_requirement = Some(agent::config::CompletionRequirement {
        tool: "never_called".into(),
        reminder: "retry would violate the durable boundary".into(),
        recovery: Some(agent::config::RecoveryPolicy {
            max_retries: 3,
            base_delay_ms: 0,
            max_delay_ms: 0,
        }),
    });
    *actor.agent.borrow_mut() = agent::Agent::new(
        definition,
        agent::PromptContext::default(),
        String::new(),
        None,
        tool_agent.tool_bridge().clone(),
    );
    actor
        .workspace_ops
        .bind_local_session(
            &actor.session_id_string(),
            actor.tool_context.cwd.as_path().to_path_buf(),
            actor.tool_context.hunk_tracker_handle.clone(),
            actor.agent.borrow().tool_bridge().toolset(),
            None,
        )
        .await
        .expect("bind tool execution runtime");
    let mut config = actor.chat_state_handle.get_sampling_config().await.unwrap();
    config.base_url = server.url();
    config.model = "test-model".into();
    config.api_backend = sampling_types::ApiBackend::ChatCompletions;
    let stored = Arc::new(std::sync::Mutex::new(Vec::new()));
    let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (chat_events, _chat_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn(
        vec![sampling_types::ConversationItem::system(
            "test system prompt",
        )],
        config,
        Box::new(FailingToolPersistence {
            fault,
            events: stored.clone(),
            failed: failed.clone(),
        }),
        chat_events,
        tokio_util::sync::CancellationToken::new(),
    );
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    let config = actor.reconstruct_full_config().await;
    let (sampler_events, mut sampler_rx) = tokio::sync::mpsc::unbounded_channel();
    let owner =
        sampler::SamplerActor::spawn_owned(config, sampler::RetryPolicy::default(), sampler_events);
    actor.sampler_handle = owner.handle();
    *actor.sampler_owner.borrow_mut() = Some(owner);
    let actor = Arc::new(actor);
    let drainer = actor.clone();
    let drain = tokio::task::spawn_local(async move {
        while let Some(event) = sampler_rx.recv().await {
            drainer.handle_sampling_event(event).await;
        }
    });
    let input_ids = admit_test_human_input(&actor, "tool-runtime-fault").await;
    install_test_foreground(&actor, "tool-runtime-fault").await;
    let error = tokio::time::timeout(
        Duration::from_secs(20),
        actor.handle_prompt(
            "tool-runtime-fault",
            input_ids,
            crate::session::PromptOrigin::User,
            Vec::new(),
            crate::session::TurnKind::User,
            vec![acp::ContentBlock::Text(acp::TextContent::new(
                "read the file",
            ))],
            tool_types::BehaviorId::Normal,
            None,
            None,
            false,
            None,
            None,
        ),
    )
    .await
    .expect("prompt must settle without recovery")
    .expect_err("runtime fault must fail");
    let diagnostic = serde_json::to_string(&error).unwrap();
    assert!(
        failed.load(std::sync::atomic::Ordering::SeqCst),
        "fault must reach real tool pipeline: {diagnostic}"
    );
    assert!(
        diagnostic.contains(&format!("injected-tool-{fault:?}-failure")),
        "original {fault:?} error lost: {diagnostic}"
    );
    assert!(
        diagnostic.contains("tool-execution"),
        "pipeline error must be classified before terminal settlement: {diagnostic}"
    );
    assert_eq!(
        server.requests().len(),
        1,
        "neither next Step nor completion recovery may sample"
    );
    if matches!(fault, Fault::Terminal) {
        let pending = actor.chat_state_handle.pending_timeline_commit().unwrap();
        assert!(
            matches!(
                pending.event.kind,
                TimelineEventKind::Tool(ToolEvent::Completed { details: None, .. })
            ),
            "terminal fault must follow actual dispatch, not an undispatched preflight result"
        );
    }
    let history = stored.lock().unwrap().clone();
    let restored =
        chat_state::Timeline::from_events(history.clone()).expect("durable prefix remains valid");
    assert_eq!(restored.events().len(), history.len());
    assert!(
        !history.iter().any(|event| matches!(
            &event.kind,
            TimelineEventKind::Tool(ToolEvent::Completed { .. })
                | TimelineEventKind::Turn(chat_state::TurnEvent::Ended { .. })
        )),
        "failed persistence cannot fabricate completion"
    );
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
    drain.abort();
}

#[test]
fn tool_runtime_failures_preserve_original_error_and_stop_recovery() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(tokio::task::LocalSet::new().run_until(async {
                    for fault in [Fault::Start, Fault::Result, Fault::Terminal] {
                        run_fault(fault).await;
                    }
                }));
        })
        .unwrap()
        .join()
        .unwrap();
}
