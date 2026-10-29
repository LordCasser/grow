use super::*;
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::rc::Rc;

use acp_transport::{AcpAgentHandler as _, protocol as acp};
use chat_state::{ChatStateActor, MessageCause, NullTimelinePersistence, Timeline};
use sampling_types::{ConversationItem, SamplingConfig, TokenUsage};
use shell::session::storage::StorageAdapter as _;

#[derive(Clone, Default)]
struct CapturedNotifications(Rc<RefCell<Vec<acp::SessionNotification>>>);

#[async_trait::async_trait(?Send)]
impl acp_transport::AcpClientHandler for CapturedNotifications {
    async fn request_permission(
        &self,
        request: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let option = request
            .options
            .first()
            .ok_or_else(acp::Error::method_not_found)?;
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                option.option_id.clone(),
            )),
        ))
    }

    async fn session_notification(
        &self,
        notification: acp::SessionNotification,
    ) -> acp::Result<()> {
        self.0.borrow_mut().push(notification);
        Ok(())
    }
}

fn sampling_config() -> SamplingConfig {
    SamplingConfig {
        base_url: "http://127.0.0.1".into(),
        model: "test-model".into(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(128_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    }
}

async fn append_initial_usage_fixture(timeline_path: &std::path::Path) -> chat_state::UsageLedger {
    let persisted = shell::session::testkit::read_timeline_fixture(timeline_path).unwrap();
    assert!(persisted.surface().is_empty());
    let original_len = persisted.events().len();
    let mut timeline = Timeline::from_events(persisted.events().to_vec()).unwrap();
    timeline
        .append(
            ConversationItem::system("restored usage fixture system head"),
            MessageCause::Seed,
        )
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::SessionTitle(
            chat_state::SessionTitleEvent {
                title: "Restored usage fixture".into(),
                source: chat_state::SessionTitleSource::User,
            },
        ))
        .unwrap();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        sampling_config(),
        Box::new(NullTimelinePersistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();

    handle
        .settle_model_attempt_usage(
            "initial-known-attempt".into(),
            0,
            "test/test-model".into(),
            Some(TokenUsage {
                prompt_tokens: 1_000,
                completion_tokens: 100,
                cached_prompt_tokens: 700,
                cache_read_known: true,
                cache_creation_prompt_tokens: 100,
                cache_write_known: true,
                ..Default::default()
            }),
            Some(20_000_000),
            Some(250),
        )
        .await
        .unwrap();
    handle
        .settle_model_attempt_usage(
            "initial-partial-cache-attempt".into(),
            0,
            "test/test-model".into(),
            Some(TokenUsage {
                prompt_tokens: 500,
                completion_tokens: 50,
                cache_read_known: false,
                cache_write_known: false,
                ..Default::default()
            }),
            None,
            Some(125),
        )
        .await
        .unwrap();
    handle
        .settle_model_attempt_usage(
            "initial-unknown-attempt".into(),
            0,
            "test/test-model".into(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

    let old_usage = handle.try_get_session_usage().await.unwrap();
    assert!(old_usage.incomplete);
    assert_eq!(old_usage.totals.input_tokens, 1_500);
    assert_eq!(old_usage.totals.cache_read_unknown_calls, 1);
    assert_eq!(old_usage.totals.cache_write_unknown_calls, 1);
    let events = handle.timeline_events().await.unwrap();
    shell::session::testkit::append_timeline_domain_fixture(timeline_path, &events[original_len..])
        .unwrap();
    old_usage
}

fn acp_message(notification: acp::SessionNotification) -> AcpClientMessage {
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    AcpClientMessage::SessionNotification(acp_transport::AcpArgs {
        request: notification,
        response_tx,
    })
}

fn usage_snapshot(
    notification: &acp::SessionNotification,
) -> shell::extensions::notification::PromptUsage {
    let acp::SessionUpdate::SessionInfoUpdate(update) = &notification.update else {
        panic!("expected SessionInfoUpdate");
    };
    serde_json::from_value(update.meta.as_ref().unwrap()["grow/sessionUsage"].clone()).unwrap()
}

fn status_line_text(usage: &shell::extensions::notification::PromptUsage) -> String {
    crate::views::agent_status::session_usage_status_line(
        Some(usage),
        &crate::theme::Theme::current(),
        false,
    )
    .spans
    .iter()
    .map(|span| span.content.as_ref())
    .collect()
}

async fn wait_for_usage_snapshot(
    capture: &CapturedNotifications,
    session_id: &str,
    minimum_input_tokens: u64,
) -> acp::SessionNotification {
    for _ in 0..200 {
        if let Some(notification) = capture.0.borrow().iter().rev().find(|notification| {
            notification.session_id.0.as_ref() == session_id
                && matches!(
                    &notification.update,
                    acp::SessionUpdate::SessionInfoUpdate(update)
                        if update.meta.as_ref().is_some_and(|meta| {
                            meta.get("grow/sessionUsage")
                                .and_then(|value| value.get("inputTokens"))
                                .and_then(serde_json::Value::as_u64)
                                .is_some_and(|tokens| tokens >= minimum_input_tokens)
                        })
                )
        }) {
            return notification.clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for session usage with input >= {minimum_input_tokens}");
}

async fn query_session_usage(
    connection: &acp_transport::ClientSideConnection,
    session_id: &str,
) -> shell::extensions::usage::SessionUsageResponse {
    let request = acp::ExtRequest::new(
        "grow/session/usage",
        serde_json::value::to_raw_value(&serde_json::json!({ "sessionId": session_id }))
            .unwrap()
            .into(),
    );
    let response = connection.ext_method(request).await.unwrap();
    serde_json::from_str(response.0.get()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial(GROW_HOME)]
async fn restored_usage_resume_snapshot_reaches_status_and_usage_without_stale_regression() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::restored_usage_resume_snapshot_reaches_status_and_usage_without_stale_regression"
    )) {
        return;
    }

    diagnostics::tls::install_ring_provider_once();
    let server = test_support::MockInferenceServer::start().await.unwrap();
    let _api_key = test_support::EnvGuard::set("GROW_API_KEY", "usage-resume-test-key");
    let response = serde_json::json!({
        "id": "chatcmpl-usage-resume",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{ "index": 0, "delta": { "content": "resumed" }, "finish_reason": null }]
    });
    let terminal = serde_json::json!({
        "id": "chatcmpl-usage-resume",
        "object": "chat.completion.chunk",
        "created": 1234567890,
        "model": "test-model",
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
        "usage": {
            "prompt_tokens": 900,
            "completion_tokens": 90,
            "total_tokens": 990,
            "prompt_tokens_details": { "cached_tokens": 720 }
        }
    });
    let mut foreground_expectation = server.expect_response(
        "resumed foreground usage",
        test_support::InferenceRequestMatcher::foreground(
            test_support::InferenceEndpoint::ChatCompletions,
        ),
        test_support::ScriptedResponse::sse(vec![
            test_support::SseEvent::data(response.to_string()),
            test_support::SseEvent::data(terminal.to_string()),
            test_support::SseEvent::data("[DONE]"),
        ]),
    );

    let grow_home = shell::util::grow_home::grow_home();
    let cwd = tempfile::tempdir().unwrap();
    let canonical_cwd = dunce::canonicalize(cwd.path()).unwrap();
    let spec = shell::session::testkit::synth::SessionSpec {
        turns: 0,
        acu_per_turn: 0,
        catalog_commands: 0,
        catalog_desc_len: 0,
        agent_chunks_per_turn: 0,
        agent_chunk_len: 0,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    };
    let (info, dir) =
        shell::session::testkit::synth::prepare_session(&grow_home, &canonical_cwd, &spec).await;
    shell::session::storage::JsonlStorageAdapter::with_root(grow_home.clone())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("test/test-model"),
            None,
            None,
        )
        .await
        .unwrap();
    let timeline_path = dir.join("timeline.jsonl");
    let old_usage = append_initial_usage_fixture(&timeline_path).await;
    let old_snapshot =
        shell::extensions::notification::session_usage_notification(&info.id, &old_usage);

    let capture = CapturedNotifications::default();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let loaded = shell::session::testkit::e2e::load_session_via_agent(
                capture.clone(),
                "pager-resumed-usage-test",
                info.id.clone(),
                PathBuf::from(&canonical_cwd),
                shell::session::testkit::e2e::mock_agent_config(&server.url()),
            )
            .await;

            let restored_snapshot =
                wait_for_usage_snapshot(&capture, info.id.0.as_ref(), 1_500).await;
            let restored_usage = usage_snapshot(&restored_snapshot);
            assert_eq!(restored_usage.totals.input_tokens, 1_500);
            assert_eq!(restored_usage.totals.output_tokens, 150);
            assert!(restored_usage.usage_is_incomplete);
            assert_eq!(restored_usage.totals.cache_read_unknown_calls, 1);
            assert_eq!(restored_usage.totals.cache_write_unknown_calls, 1);

            let usage_response = query_session_usage(&loaded.client_conn, info.id.0.as_ref()).await;
            assert_eq!(usage_response.segments.len(), 2);
            assert_eq!(usage_response.usage.totals.input_tokens, 1_500);
            assert!(usage_response.usage.usage_is_incomplete);

            let mut app = make_app_with_agent(info.id.0.as_ref());
            assert!(handle(acp_message(old_snapshot.clone()), &mut app));
            let old_status = status_line_text(
                app.agents[&AgentId(0)]
                    .session
                    .session_usage
                    .as_ref()
                    .unwrap(),
            );
            assert!(handle(acp_message(restored_snapshot.clone()), &mut app));
            let restored_status = status_line_text(
                app.agents[&AgentId(0)]
                    .session
                    .session_usage
                    .as_ref()
                    .unwrap(),
            );
            assert_eq!(restored_status, old_status);
            assert!(handle(acp_message(old_snapshot.clone()), &mut app));
            assert_eq!(
                status_line_text(
                    app.agents[&AgentId(0)]
                        .session
                        .session_usage
                        .as_ref()
                        .unwrap(),
                ),
                restored_status,
            );
            let same_total_usage =
                query_session_usage(&loaded.client_conn, info.id.0.as_ref()).await;
            assert_eq!(same_total_usage, usage_response);

            let prompt = acp::PromptRequest::new(
                info.id.clone(),
                vec![acp::ContentBlock::Text(acp::TextContent::new(
                    "record resumed usage",
                ))],
            );
            loaded.client_conn.prompt(prompt).await.unwrap();
            foreground_expectation.wait_satisfied().await;
            let updated_snapshot =
                wait_for_usage_snapshot(&capture, info.id.0.as_ref(), 2_400).await;
            let updated_usage = usage_snapshot(&updated_snapshot);
            assert_eq!(updated_usage.totals.input_tokens, 2_400);
            assert_eq!(updated_usage.totals.output_tokens, 240);
            assert_eq!(updated_usage.totals.cached_read_tokens, 1_420);
            assert!(updated_usage.usage_is_incomplete);

            let usage_response = query_session_usage(&loaded.client_conn, info.id.0.as_ref()).await;
            assert_eq!(usage_response.segments.len(), 2);
            assert_eq!(usage_response.segments[0].usage.totals.input_tokens, 1_500);
            assert_eq!(usage_response.segments[1].usage.totals.input_tokens, 900);

            let updated_message = acp_message(updated_snapshot);
            assert!(handle(updated_message, &mut app));
            let id = AgentId(0);
            let current = app.agents[&id].session.session_usage.as_ref().unwrap();
            assert_eq!(current.totals.input_tokens, 2_400);
            let status_before_stale = status_line_text(current);
            assert!(status_before_stale.contains("measured cache"));
            assert!(status_before_stale.starts_with('≥'));

            let report =
                crate::app::status_blocks::session_usage_report_block_text(&usage_response);
            assert!(report.contains("Session usage (lifetime):"));
            assert!(report.contains("Initial run"));
            assert!(report.contains("Resume #1"));
            assert!(report.contains("2,400"));
            assert!(report.contains("incomplete"));

            assert!(!handle(acp_message(old_snapshot), &mut app));
            let current = app.agents[&id].session.session_usage.as_ref().unwrap();
            assert_eq!(current.totals.input_tokens, 2_400);
            assert_eq!(status_line_text(current), status_before_stale);

            let after_stale_usage =
                query_session_usage(&loaded.client_conn, info.id.0.as_ref()).await;
            assert_eq!(after_stale_usage, usage_response);
            assert_eq!(
                crate::app::status_blocks::session_usage_report_block_text(&after_stale_usage),
                report,
            );

            drop(loaded);
        })
        .await;
}
