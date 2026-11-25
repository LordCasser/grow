use super::*;
use chat_state::{CompactionEvent, MessageCause, TimelineEventKind};
use test_support::{InferenceEndpoint, InferenceRequestMatcher};

const WINDOW: u64 = 100_000;

fn run_session_test<F: Future<Output = ()>>(test: impl FnOnce() -> F + Send + 'static) {
    run_with_session_stack(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(test()));
    });
}

#[test]
fn portable_overflow_pre_prunes_before_dispatch_and_cold_replays_the_recent_result() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        server.enqueue_response(
            "/v1/messages",
            messages_turn(&[("done", END_TURN)], END_TURN),
        );
        let (actor, _) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        seed_tool_result_rounds(&actor, 2).await;
        let state = &actor.chat_state_handle;
        let portable = sampling_types::project_portable_history(&state.get_conversation().await);
        assert!(chat_state::estimate_conversation_tokens(&portable) > WINDOW);
        state.record_provider_context_anchor(2_000);
        assert_eq!(state.get_projected_tokens().await, 2_000);
        let suppression = &actor.compaction.auto_compact_suppressed;
        let previous = suppression.swap(
            crate::session::compaction_config::SUPPRESS_TURN,
            std::sync::atomic::Ordering::Relaxed,
        );
        assert!(actor.check_auto_compact_needed(None).await.is_none());
        suppression.store(previous, std::sync::atomic::Ordering::Relaxed);

        run_user_turn(&actor, "portable-prune").await.unwrap();
        let events = state.timeline_events().await.unwrap();
        let pruned = events
            .iter()
            .filter_map(|event| event.messages())
            .filter(|message| message.cause == MessageCause::ToolResultPrune)
            .count();
        assert_eq!(pruned, 1);
        assert_eq!(server.messages_request_count(), 1, "pruning alone fits");
        let (chat_events, _events) = mpsc::unbounded_channel();
        let cold = chat_state::ChatStateActor::spawn_from_timeline(
            events,
            state.get_sampling_config().await.unwrap(),
            Box::new(chat_state::NullTimelinePersistence),
            chat_events,
            Default::default(),
        )
        .await
        .unwrap();
        assert!(cold.get_projected_tokens().await < WINDOW);
        let request = cold
            .build_request("resumed-prune", vec![], None, None, None)
            .await
            .unwrap();
        assert_eq!(
            cold.get_projected_tokens().await,
            chat_state::estimate_request_input_tokens(&request)
        );
        assert!(cold.get_projected_tokens().await < WINDOW);
        let results = tool_result_texts(&request.items);
        assert_eq!(results.len(), 2);
        assert!(results[0].len() <= 20_000);
        assert_eq!(
            results[1],
            big_tool_text(),
            "recent tool evidence survives cold replay"
        );
    });
}

#[test]
fn portable_overflow_without_a_range_allows_the_measured_request_once() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        server.enqueue_response(
            "/v1/messages",
            messages_turn(&[("done", END_TURN)], END_TURN),
        );
        let (actor, mut notifications) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        actor.compaction.pre_prune.set(false);
        let state = &actor.chat_state_handle;
        state
            .push_user_message_durably(ConversationItem::system_reminder("x".repeat(420_000)))
            .await
            .unwrap();
        state.record_provider_context_anchor(2_000);
        let turn = run_user_turn(&actor, "portable-no-range").await.unwrap();

        assert_eq!(turn.stop_reason, acp::StopReason::EndTurn);
        assert_eq!(server.messages_request_count(), 1);
        assert_one_preventive_skip(&mut notifications);
    });
}

#[test]
fn portable_overflow_with_an_indivisible_oversized_exchange_sends_the_fitting_request() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        server.enqueue_response(
            "/v1/messages",
            messages_turn(&[("done", END_TURN)], END_TURN),
        );
        let (actor, mut notifications) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        actor.compaction.pre_prune.set(false);
        // The old 105K result is eligible as a complete exchange. No whole
        // source containing it fits the summary window, and the 20K tail stays.
        append_tool_exchange(&actor, "indivisible", 0, 420_000).await;
        append_tool_exchange(&actor, "retained", 1, 80_000).await;
        let state = &actor.chat_state_handle;
        state.record_provider_context_anchor(2_000);
        let turn = run_user_turn(&actor, "portable-no-fitting-source")
            .await
            .unwrap();

        assert_eq!(turn.stop_reason, acp::StopReason::EndTurn);
        assert_eq!(server.messages_request_count(), 1);
        assert_one_preventive_skip(&mut notifications);
        let events = state.timeline_events().await.unwrap();
        let failures: Vec<_> = events
            .iter()
            .filter_map(|event| match &event.kind {
                TimelineEventKind::Compaction(CompactionEvent::Failed { error, .. }) => Some(error),
                _ => None,
            })
            .collect();
        assert!(
            matches!(failures.as_slice(), [why] if why.contains("no complete source range fits")),
            "the structural selection reached the summary budget check: {failures:?}"
        );
        let results = tool_result_texts(&state.get_conversation().await);
        assert_eq!(
            results.iter().map(String::len).collect::<Vec<_>>(),
            [420_000, 80_000]
        );
    });
}

fn assert_one_preventive_skip(
    notifications: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let mut outcomes = Vec::new();
    while let Ok(message) = notifications.try_recv() {
        if let Some((kind, params)) = session_update(&message)
            && kind.starts_with("auto_compact_")
        {
            outcomes.push((kind, params["update"]["reason"].clone()));
        }
    }
    assert_eq!(outcomes.len(), 2, "{outcomes:?}");
    assert_eq!(
        outcomes[0],
        (
            "auto_compact_started".into(),
            json!("Preparing retained history for session resume"),
        )
    );
    assert_eq!(
        outcomes[1],
        ("auto_compact_cancelled".into(), json!("no_eligible_range"))
    );
}

#[test]
fn text_only_image_projection_rechecks_retained_portable_text() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let (actor, mut notifications) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        super::super::read_file_image_description_tests::run_image_result(&actor).await;
        actor
            .record_unsupported_model_image_input(
                actor.current_model_image_input_key().await.unwrap(),
            )
            .await
            .unwrap();
        let state = &actor.chat_state_handle;
        state
            .push_user_message_durably(ConversationItem::system_reminder("x".repeat(420_000)))
            .await
            .unwrap();
        state.record_provider_context_anchor(2_000);
        let trigger = actor.check_auto_compact_needed(None).await.unwrap();
        actor.run_compact_only(trigger).await.unwrap();

        let events = state.timeline_events().await.unwrap();
        let image = events
            .iter()
            .position(|event| matches!(event.kind, TimelineEventKind::ImageProjection(_)))
            .expect("the image was durably projected for the text-only model");
        let compact = events
            .iter()
            .position(|event| {
                matches!(
                    event.kind,
                    TimelineEventKind::Compaction(CompactionEvent::Started { .. })
                )
            })
            .expect("portable text still reaches the threshold after removing the image");
        assert!(image < compact);
        assert_one_preventive_skip(&mut notifications);
        assert_eq!(server.messages_request_count(), 0);
    });
}

#[test]
fn portable_history_promotes_its_running_summary_as_tool_evidence_grows() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let summary = "The earlier search completed and its findings were verified. ".repeat(20);
        let mut provider = server.expect_response_blocked(
            "one summary for both thresholds",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn(&[(&summary, END_TURN)], END_TURN),
        );
        let (actor, mut notifications) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        let state = &actor.chat_state_handle;
        actor.compaction.threshold_percent.set(80);
        actor.compaction.pre_prune.set(false);
        seed_tool_result_rounds(&actor, 1).await;
        append_tool_exchange(&actor, "recent", 1, 88_000).await;
        state.record_provider_context_anchor(2_000);
        actor.state.lock().await.foreground =
            ForegroundState::RegularTurn(running_task_stub("portable-promotion"));
        assert!(
            actor.check_auto_compact_needed(None).await.is_none(),
            "72K has not reached 80%"
        );
        actor.background_compaction_boundary().await.unwrap();
        assert!(
            actor.compaction.background.borrow().is_some(),
            "72K has crossed 70%"
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), provider.wait_blocked())
            .await
            .unwrap();

        append_tool_exchange(&actor, "latest", 2, 40_000).await;
        assert!(state.get_projected_tokens().await < 20_000);
        let hard = actor
            .check_auto_compact_needed(None)
            .await
            .expect("82K crosses the hard threshold");
        let promoted = tokio::task::spawn_local({
            let actor = actor.clone();
            async move { actor.run_compact_only(hard).await }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_session_update(&mut notifications, "auto_compact_started"),
        )
        .await
        .unwrap();
        provider.release();
        tokio::time::timeout(std::time::Duration::from_secs(10), promoted)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let events = state.timeline_events().await.unwrap();
        let lifecycle: Vec<_> = events
            .iter()
            .filter_map(|event| match &event.kind {
                TimelineEventKind::Compaction(compaction) => Some(compaction),
                _ => None,
            })
            .collect();
        let [
            CompactionEvent::Started {
                id: start,
                mode: chat_state::CompactionMode::Background,
                ..
            },
            CompactionEvent::Promoted { id: promotion },
            CompactionEvent::Summary { id: summary, .. },
            CompactionEvent::Completed { id: end, .. },
        ] = lifecycle.as_slice()
        else {
            panic!("expected one promoted generation, got {lifecycle:?}");
        };
        assert_eq!([start, promotion, summary], [end; 3]);
        assert_eq!(server.messages_request_count(), 1);
        assert!(actor.compaction.background.borrow().is_none());
        let surface = state.get_conversation().await;
        let latest = surface.iter().find_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == "latest" => {
                Some(&result.content)
            }
            _ => None,
        });
        assert_eq!(latest.map(|text| text.len()), Some(40_000));
    });
}

async fn append_tool_exchange(
    actor: &SessionActor,
    id: &str,
    prompt_index: usize,
    result_bytes: usize,
) {
    let state = &actor.chat_state_handle;
    state.push_user_message(prompt(id, prompt_index));
    append_tool_response(state, id, result_bytes).await;
}

async fn append_tool_response(state: &chat_state::ChatStateHandle, id: &str, result_bytes: usize) {
    state.push_assistant_response(ConversationItem::assistant_tool_calls(vec![
        sampling_types::ToolCall {
            id: id.into(),
            name: "grep".into(),
            arguments: "{}".into(),
        },
    ]));
    state
        .push_tool_result_durably(ConversationItem::tool_result(id, "y".repeat(result_bytes)))
        .await
        .unwrap();
}

async fn install_foreground_tools(actor: &SessionActor) {
    use tools::implementations::grow_build::{read_file::ReadFileTool, todo::TodoWriteTool};
    use tools::registry::types::ToolConfig;

    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        ToolConfig::for_tool::<TodoWriteTool>(),
        ToolConfig::for_tool::<ReadFileTool>(),
    ])
    .await;
}

#[test]
fn preventive_compaction_does_not_resummarize_its_own_summary_across_tool_steps() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let summary = "The earlier search verified the recorded result. ".repeat(500);
        let auxiliary = server.expect_response(
            "summarize the first tool response",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn(&[(&summary, END_TURN)], END_TURN),
        );
        let tool = server.expect_response(
            "continue with a tool",
            InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
            ScriptedResponse::sse([
                json!({"type":"message_start","message":{"id":"tool-msg","type":"message","role":"assistant","content":[],"model":"test","usage":{"input_tokens":2_000,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"step-read","name":"read_file","input":{}}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":r#"{"target_file":"Cargo.toml","limit":1}"#}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":2_000,"output_tokens":5}}),
                json!({"type":"message_stop"}),
            ].into_iter().map(|event| SseEvent::data(event.to_string())).collect()),
        );
        let foreground = server.expect_response(
            "continue with the recent tool response",
            InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
            messages_turn(&[("done", END_TURN)], END_TURN),
        );
        let (actor, _) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        install_foreground_tools(&actor).await;
        actor.compaction.pre_prune.set(false);
        let state = &actor.chat_state_handle;
        // Both responses belong to the same turn. Replacing the first leaves
        // a summary-sized source directly before the indivisible 105K result.
        append_tool_exchange(&actor, "older", 0, 40_000).await;
        let before = state
            .materialize_timeline(actor.session_id_string())
            .await
            .unwrap();
        let old_exchange = &before.surface_ids[before.surface_ids.len() - 2..];
        append_tool_response(state, "retained", 420_000).await;
        state.record_provider_context_anchor(2_000);

        let turn = run_user_turn(&actor, "portable-summary-before-dispatch")
            .await
            .unwrap();
        assert_eq!(turn.stop_reason, acp::StopReason::EndTurn);
        auxiliary.assert_satisfied();
        tool.assert_satisfied();
        foreground.assert_satisfied();
        assert_eq!(server.messages_request_count(), 3);
        let requests = server.request_bodies();
        let blocks = requests[2]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .collect::<Vec<_>>();
        for call in ["retained", "step-read"] {
            assert_eq!(
                blocks
                    .iter()
                    .filter(|block| block["type"] == "tool_result" && block["tool_use_id"] == call)
                    .count(),
                1,
                "the next Step must receive the result for {call}"
            );
        }
        let events = state.timeline_events().await.unwrap();
        let replacements = events
            .iter()
            .filter_map(|event| event.messages())
            .filter(|message| message.cause == MessageCause::Compaction)
            .collect::<Vec<_>>();
        assert!(
            matches!(replacements.as_slice(), [replacement] if matches!(&replacement.surface,
            chat_state::SurfaceOp::Replace { shadowed, .. } if shadowed == old_exchange)),
            "only the first complete exchange may be replaced"
        );

        let committed = state
            .materialize_timeline(actor.session_id_string())
            .await
            .unwrap();
        let portable = sampling_types::project_portable_history(&committed.surface);
        assert!(chat_state::estimate_conversation_tokens(&portable) > WINDOW);
        let next = chat_state::compaction_utils::plan_compaction_range(
            &committed.surface,
            &committed.surface_ids,
            &committed.unconsumed_skill_load_ids,
            16_000,
            5_000,
        )
        .expect("the previous summary is still eligible before the retained result");
        assert_eq!(next.start_index, next.end_index);
        assert!(
            committed.surface[next.start_index]
                .text_content()
                .contains(summary.trim_end())
        );
    });
}

#[test]
fn pruning_that_leaves_portable_history_above_threshold_still_runs_the_summary() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let summary =
            "The earlier tool exchange and its verified conclusions were retained. ".repeat(20);
        server.enqueue_response(
            "/v1/messages",
            messages_turn(&[(&summary, END_TURN)], END_TURN),
        );
        server.enqueue_response(
            "/v1/messages",
            messages_turn(&[("done", END_TURN)], END_TURN),
        );
        let (actor, _) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        seed_tool_result_rounds(&actor, 1).await;
        let state = &actor.chat_state_handle;
        state.push_assistant_response(ConversationItem::assistant("s".repeat(240_000)));
        state.push_user_message(prompt("recent work", 1));
        state.push_assistant_response(ConversationItem::assistant("r".repeat(128_000)));
        state.record_provider_context_anchor(2_000);

        run_user_turn(&actor, "portable-prune-needs-summary")
            .await
            .unwrap();
        let events = state.timeline_events().await.unwrap();
        let prune = events
            .iter()
            .position(|event| {
                event
                    .messages()
                    .is_some_and(|message| message.cause == MessageCause::ToolResultPrune)
            })
            .expect("old 50K result was pruned");
        let summarized = events
            .iter()
            .position(|e| {
                matches!(
                    &e.kind,
                    TimelineEventKind::Compaction(CompactionEvent::Summary { .. })
                )
            })
            .expect("92K of assistant text alone still exceeds the 85K threshold");
        assert!(prune < summarized);
        assert_eq!(
            server.messages_request_count(),
            2,
            "one summary and one ordinary request"
        );
    });
}

#[test]
fn preventive_summary_rejection_preserves_primary_admission_and_failed_terminal() {
    run_session_test(|| async {
        for (anchor, cause, provider_message, admits) in [
            (
                2_000,
                "portable_replay",
                "unsupported summary parameter",
                true,
            ),
            (
                85_000,
                "pre_sampling",
                "unsupported summary parameter",
                true,
            ),
            (
                100_000,
                "pre_sampling",
                "unsupported summary parameter",
                false,
            ),
            (
                2_000,
                "portable_replay",
                "maximum context length exceeded",
                true,
            ),
            (
                100_000,
                "pre_sampling",
                "maximum context length exceeded",
                false,
            ),
        ] {
            let server = MockInferenceServer::start().await.unwrap();
            let failed = server.expect_response(
                "reject only the summary",
                InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                ScriptedResponse::json(
                    400,
                    json!({"error":{"type":"invalid_request_error","message":provider_message}}),
                ),
            );
            let primary = admits.then(|| {
                server.expect_response(
                    "the fitting ordinary request",
                    InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
                    messages_turn(&[("done", END_TURN)], END_TURN),
                )
            });
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
            install_foreground_tools(&actor).await;
            actor.compaction.pre_prune.set(false);
            seed_closed_compaction_range(&actor, 396_000).await;
            let state = &actor.chat_state_handle;
            state.record_provider_context_anchor(anchor);
            assert_eq!(
                actor.check_auto_compact_needed(None).await.unwrap().source,
                cause
            );
            let result = run_user_turn(&actor, "preventive-failure").await;
            if let Some(primary) = primary {
                assert_eq!(result.unwrap().stop_reason, acp::StopReason::EndTurn);
                primary.assert_satisfied();
            } else {
                result.expect_err("actual overflow still requires a successful recovery");
            }
            failed.assert_satisfied();
            assert_eq!(server.messages_request_count(), 1 + usize::from(admits));
            let timeline = state.timeline_events().await.unwrap();
            assert!(timeline.iter().any(|e| matches!(&e.kind, TimelineEventKind::Compaction(CompactionEvent::Failed { error, .. }) if error.contains(provider_message))));
            assert!(
                !timeline
                    .iter()
                    .filter_map(|event| event.messages())
                    .any(|message| message.cause == MessageCause::Compaction)
            );
            let mut outcomes = Vec::new();
            while let Ok(message) = notifications.try_recv() {
                if let Some((kind, _)) = session_update(&message)
                    && kind.starts_with("auto_compact_")
                {
                    outcomes.push(kind);
                }
            }
            assert_eq!(outcomes, ["auto_compact_started", "auto_compact_failed"]);
        }
    });
}

#[test]
fn promoted_preventive_summary_rechecks_the_live_output_reservation_on_failure() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let mut provider = server.expect_response_blocked("promoted summary", InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            ScriptedResponse::json(400, json!({"error":{"type":"invalid_request_error","message":"unsupported summary parameter"}})));
        let (actor, mut notifications) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        actor.compaction.threshold_percent.set(80);
        actor.compaction.pre_prune.set(false);
        seed_closed_compaction_range(&actor, 68_000).await;
        let state = &actor.chat_state_handle;
        state.record_provider_context_anchor(72_000);
        actor.state.lock().await.foreground =
            ForegroundState::RegularTurn(running_task_stub("reservation"));
        actor.background_compaction_boundary().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), provider.wait_blocked())
            .await
            .unwrap();
        state.record_provider_context_anchor(85_000);
        let trigger = actor.check_auto_compact_needed(Some(5_000)).await.unwrap();
        assert!(trigger.is_preventive());
        let promoted = tokio::task::spawn_local({
            let actor = actor.clone();
            async move { actor.run_compact_only(trigger).await }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_session_update(&mut notifications, "auto_compact_started"),
        )
        .await
        .unwrap();
        state.record_provider_context_anchor(96_000);
        provider.release();
        let error = promoted
            .await
            .unwrap()
            .expect_err("96K input plus 5K output no longer fits");
        assert!(format!("{error:?}").contains("96000 input + 5000 output"));
        assert!(state.timeline_events().await.unwrap().iter().any(|e| matches!(&e.kind, TimelineEventKind::Compaction(CompactionEvent::Failed { error, .. }) if error.contains("unsupported summary parameter"))));
        assert_eq!(server.messages_request_count(), 1);
    });
}

#[test]
fn background_preventive_account_rejection_stops_the_foreground_boundary() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        let account = server.expect_response(
            "account rejects the background summary",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            ScriptedResponse::json(402, json!({"error":{"message":"denied"}})),
        );
        let (actor, _) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        actor.compaction.threshold_percent.set(80);
        actor.compaction.pre_prune.set(false);
        seed_closed_compaction_range(&actor, 68_000).await;
        actor
            .chat_state_handle
            .record_provider_context_anchor(72_000);
        actor.state.lock().await.foreground =
            ForegroundState::RegularTurn(running_task_stub("billing"));
        actor.background_compaction_boundary().await.unwrap();
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Err(error) = actor.background_compaction_boundary().await {
                    break error;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(format!("{error:?}").contains("402"));
        account.assert_satisfied();
        assert_eq!(server.messages_request_count(), 1);
    });
}

#[test]
fn preventive_sideband_cancellation_settles_usage_already_reported_by_the_stream() {
    run_session_test(|| async {
        let server = MockInferenceServer::start().await.unwrap();
        server.set_chunk_delay(Some(std::time::Duration::from_millis(20)));
        let mut provider = server.expect_response_blocked(
            "summary reports usage before being cancelled",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn_with_usage(&[("An unfinished summary", END_TURN)], END_TURN, 17_000),
        );
        let (actor, _) =
            actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
        actor.compaction.pre_prune.set(false);
        seed_closed_compaction_range(&actor, 68_000).await;
        let state = &actor.chat_state_handle;
        state.record_provider_context_anchor(85_000);
        let trigger = actor.check_auto_compact_needed(None).await.unwrap();
        let summary = tokio::task::spawn_local({
            let actor = actor.clone();
            async move { actor.run_compact_only(trigger).await }
        });
        let timeout = std::time::Duration::from_secs(5);
        tokio::time::timeout(timeout, provider.wait_blocked())
            .await
            .unwrap();
        actor.sideband_cancel.cancel();
        tokio::time::timeout(timeout, summary)
            .await
            .unwrap()
            .unwrap()
            .expect_err("a closed Sideband cannot authorize a primary request");
        provider.release();
        let timeline = state.timeline_events().await.unwrap();
        let charges: Vec<_> = timeline
            .iter()
            .filter_map(|event| match &event.kind {
                TimelineEventKind::Observation(obs) if obs.name == "aux_attempt_settled" => {
                    obs.data.as_ref()
                }
                _ => None,
            })
            .collect();
        assert!(
            matches!(charges.as_slice(), [charge] if charge["usage"]["prompt_tokens"] == 17_000),
            "{charges:?}"
        );
        assert_eq!(server.messages_request_count(), 1);
    });
}

#[test]
fn promoted_preventive_retries_a_summary_only_prefix_only_for_required_recovery() {
    run_session_test(|| async {
        for (anchor, required) in [(85_000, false), (105_000, true)] {
            let server = MockInferenceServer::start().await.unwrap();
            let mut rejected = server.expect_response_blocked("provider rejects full frozen input", InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                ScriptedResponse::json(400, json!({"error":{"type":"invalid_request_error","message":"maximum context length exceeded"}})));
            let compacted =
                "The recorded old search results and their verified implications. ".repeat(24);
            let reduced = required.then(|| {
                server.expect_response(
                    "retry required recovery on the smaller prefix",
                    InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
                    messages_turn(&[(&compacted, END_TURN)], END_TURN),
                )
            });
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, WINDOW).await;
            actor.compaction.threshold_percent.set(80);
            actor.compaction.pre_prune.set(false);
            let state = &actor.chat_state_handle;
            state.push_user_message(prompt("task", 0));
            state.push_user_message(ConversationItem::user_meta("s".repeat(24_000)));
            append_tool_response(state, "older", 48_000).await;
            append_tool_response(state, "tail", 80_000).await;
            state.record_provider_context_anchor(72_000);
            actor.state.lock().await.foreground =
                ForegroundState::RegularTurn(running_task_stub("required-retry"));
            actor.background_compaction_boundary().await.unwrap();
            let timeout = std::time::Duration::from_secs(5);
            tokio::time::timeout(timeout, rejected.wait_blocked())
                .await
                .unwrap();
            state.record_provider_context_anchor(anchor);
            let trigger = actor.check_auto_compact_needed(None).await.unwrap();
            assert_eq!(trigger.is_preventive(), !required);
            let promoted = tokio::task::spawn_local({
                let actor = actor.clone();
                async move { actor.run_compact_only(trigger).await }
            });
            tokio::time::timeout(
                timeout,
                wait_for_session_update(&mut notifications, "auto_compact_started"),
            )
            .await
            .unwrap();
            rejected.release();
            tokio::time::timeout(timeout, promoted)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            if let Some(reduced) = reduced {
                reduced.assert_satisfied();
            }
            assert_eq!(
                server.messages_request_count(),
                if required { 2 } else { 1 }
            );
            let events = state.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter_map(|event| event.messages())
                    .filter(|message| message.cause == MessageCause::Compaction)
                    .count(),
                usize::from(required)
            );
        }
    });
}
