use super::*;
use acp_transport::AcpAgentHandler as _;
use tools::implementations::grow_build::task::types::{
    SubagentEvent, SubagentOwner, SubagentRequest, SubagentSpawnRequest,
};

fn launch(
    agent: &MvpAgent,
    root: &acp::SessionId,
    id: &str,
    resume: bool,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::sync::oneshot::Receiver<tools::implementations::grow_build::task::types::SubagentResult>
{
    let (result_tx, result) = tokio::sync::oneshot::channel();
    agent
        .subagent_event_tx
        .send(SubagentEvent::Spawn(SubagentSpawnRequest {
            request: Box::new(SubagentRequest {
                id: id.into(),
                origin_tool_call_id: None,
                prompt: if resume {
                    "DERIVED_TASK_PROMPT"
                } else {
                    "SOURCE_TASK_PROMPT"
                }
                .into(),
                description: "resume transcript fixture".into(),
                subagent_type: "resume-reader".into(),
                parent_session_id: root.0.to_string(),
                parent_step_id: None,
                parent_prompt_id: None,
                resume_from: resume.then(|| "source-child".into()),
                cwd: None,
                runtime_overrides: Default::default(),
                run_in_background: true,
                surface_completion: false,
                await_to_completion: false,
                fork_context: false,
                owner: SubagentOwner::Task,
                goal_context: None,
                cancel_token: cancel,
            }),
            result_tx,
        }))
        .unwrap();
    result
}

fn transcript(updates: Vec<acp::SessionUpdate>) -> Vec<String> {
    updates
        .into_iter()
        .filter_map(|update| {
            let (kind, chunk) = match update {
                acp::SessionUpdate::UserMessageChunk(chunk) => ("user", chunk),
                acp::SessionUpdate::AgentMessageChunk(chunk) => ("assistant", chunk),
                acp::SessionUpdate::AgentThoughtChunk(chunk) => ("thought", chunk),
                acp::SessionUpdate::ToolCall(call) => {
                    return Some(format!("tool:{}", call.tool_call_id.0));
                }
                acp::SessionUpdate::ToolCallUpdate(update) => {
                    return Some(format!(
                        "tool-update:{}",
                        serde_json::to_string(&update).unwrap()
                    ));
                }
                _ => return None,
            };
            let acp::ContentBlock::Text(text) = chunk.content else {
                return None;
            };
            Some(format!("{kind}:{}", text.text))
        })
        .collect()
}

#[test]
fn task_resume_inherits_completed_and_cancelled_transcripts_before_live_and_cold_view() {
    const HOME: &str = "GROW_TEST_RESUME_TRANSCRIPT_HOME";
    let Ok(home) = std::env::var(HOME) else {
        for outcome in ["completed", "cancelled"] {
            let home = tempfile::tempdir().unwrap();
            for phase in ["create", "cold"] {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "agent::mvp_agent::tests::task_resume_presentation_tests::task_resume_inherits_completed_and_cancelled_transcripts_before_live_and_cold_view", "--nocapture"])
                    .env(HOME, home.path()).env("GROW_HOME", home.path())
                    .env("GROW_TEST_RESUME_TRANSCRIPT_OUTCOME", outcome)
                    .env("GROW_TEST_RESUME_TRANSCRIPT_PHASE", phase).output().unwrap();
                assert!(
                    output.status.success(),
                    "{outcome}/{phase}: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            }
        }
        return;
    };
    let home = std::path::PathBuf::from(home);
    if std::env::var("GROW_TEST_RESUME_TRANSCRIPT_PHASE").unwrap() == "cold" {
        let updates = crate::session::storage::load_updates_for_replay_at("derived-child", &home)
            .unwrap()
            .unwrap();
        let mut streamed = Vec::new();
        assert_eq!(
            crate::session::storage::stream_replay_updates_at("derived-child", &home, |update| {
                streamed.push(update)
            })
            .unwrap(),
            crate::session::storage::ReplayEmission::Emitted,
        );
        let rows = transcript(updates);
        assert_eq!(rows, transcript(streamed));
        let source: Vec<String> =
            serde_json::from_slice(&std::fs::read(home.join("source-display")).unwrap()).unwrap();
        assert!(
            rows.starts_with(&source),
            "{rows:?} must inherit {source:?}"
        );
        for original in &source {
            assert_eq!(
                rows.iter().filter(|row| *row == original).count(),
                source.iter().filter(|row| *row == original).count(),
                "inherited row duplicated: {original}"
            );
        }
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("SOURCE_TASK_PROMPT"))
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("DERIVED_TASK_PROMPT"))
                .count(),
            1
        );
        assert!(
            rows.iter()
                .position(|row| row.contains("DERIVED_TASK_PROMPT"))
                .unwrap()
                >= source.len()
        );
        return;
    }
    run_local_for_bridge_test(|| async move {
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        git2::Repository::init(&cwd).unwrap();
        std::fs::write(cwd.join(".grow/agents/resume-reader.md"), "---\nname: resume-reader\ndescription: Resume transcript fixture\ncapabilityMode: all\ntools: [run_terminal_cmd]\nagentsMd: false\ndiscoverSkills: false\ninheritSkills: false\n---\nFollow the task.\n").unwrap();
        let source_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let source_calls_seen = source_calls.clone();
        let provider = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let source_calls = source_calls_seen.clone();
            async move {
                let is_task = body["tools"].as_array().is_some_and(|tools| !tools.is_empty());
                let derived = body["messages"].to_string().contains("DERIVED_TASK_PROMPT");
                let first = is_task && (derived || source_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0);
                let delta = if first {
                    let id = if derived { "derived-tool" } else { "source-tool" };
                    serde_json::json!({"role":"assistant", "reasoning_content": if derived { "DERIVED_THOUGHT" } else { "SOURCE_THOUGHT" }, "content": if derived { "DERIVED_VISIBLE_INTRO" } else { "SOURCE_VISIBLE_INTRO" }, "tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":"run_terminal_command","arguments":"{\"command\":\"sh -c 'echo SOURCE_TOOL_RESULT'\",\"description\":\"fixture shell\",\"background\":false}"}}]})
                } else { serde_json::json!({"role":"assistant","content": if is_task { "SOURCE_FINAL" } else { "metadata" }}) };
                let part = serde_json::json!({"id":"resume-response","object":"chat.completion.chunk","created":1,"model":"default","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
                let done = serde_json::json!({"id":"resume-response","object":"chat.completion.chunk","created":1,"model":"default","choices":[{"index":0,"delta":{},"finish_reason":if first { "tool_calls" } else { "stop" }}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}});
                ([("content-type","text/event-stream")], format!("data: {part}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap();
        });
        let raw = format!(
            "[models]\ndefault = \"test/default\"\n[provider.test]\napi_backend = \"chat_completions\"\n[provider.test.options]\nbase_url = \"http://{address}/v1\"\napi_key = \"fixture-only\"\n[provider.test.models.default]\ncontext_window = 128000\n[subagents]\npermission_mode = \"ask\"\n"
        );
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        cfg.subagent_permission_mode = workspace::permission::types::RequestPermissionMode::Ask;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (permission_tx, mut permission_rx) = tokio::sync::mpsc::unbounded_channel();
        let eager_history = std::rc::Rc::new(std::cell::RefCell::new(None));
        let eager_seen = eager_history.clone();
        let read_home = home.clone();
        tokio::task::spawn_local(async move {
            while let Some(message) = rx.recv().await {
                match message {
                    acp_transport::AcpClientMessage::RequestPermission(args) => {
                        permission_tx.send(args).unwrap();
                    }
                    acp_transport::AcpClientMessage::SessionNotification(args) => {
                        let _ = args.response_tx.send(Ok(()));
                    }
                    acp_transport::AcpClientMessage::ExtNotification(args) => {
                        if let Ok(notification) =
                            serde_json::from_str::<
                                crate::extensions::notification::SessionNotification,
                            >(args.request.params.get())
                            && matches!(&notification.update, crate::extensions::notification::SessionUpdate::SubagentSpawned { child_session_id, .. } if child_session_id == "derived-child")
                        {
                            // Pager eagerly reads inherited history at the
                            // live Spawned boundary, before child output.
                            *eager_seen.borrow_mut() = Some(transcript(
                                crate::session::storage::load_updates_for_replay_at(
                                    "derived-child",
                                    &read_home,
                                )
                                .unwrap()
                                .unwrap(),
                            ));
                        }
                        let _ = args.response_tx.send(Ok(()));
                    }
                    _ => {}
                }
            }
        });
        let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
        cold_recovery_stage("transcript.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let root = cold_recovery_stage(
            "transcript.new",
            agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
        )
        .await
        .unwrap()
        .session_id;
        let cancel = tokio_util::sync::CancellationToken::new();
        let source_result = launch(&agent, &root, "source-child", false, cancel.clone());
        let source_wait = cold_recovery_stage("transcript.source_permission", permission_rx.recv())
            .await
            .unwrap();
        assert_eq!(source_wait.request.session_id.0.as_ref(), "source-child");
        let cancelled =
            std::env::var("GROW_TEST_RESUME_TRANSCRIPT_OUTCOME").unwrap() == "cancelled";
        let mut held_wait = None;
        if cancelled {
            let (respond_to, acknowledged) = tokio::sync::oneshot::channel();
            agent.subagent_event_tx.send(SubagentEvent::Cancel(
                tools::implementations::grow_build::task::types::SubagentCancelRequest {
                    parent_session_id: Some(root.0.to_string()),
                    target: tools::implementations::grow_build::task::types::SubagentCancelTarget::SubagentId("source-child".into()),
                    respond_to,
                }
            )).unwrap();
            assert!(matches!(
                cold_recovery_stage("transcript.cancel_source", acknowledged)
                    .await
                    .unwrap(),
                tools::implementations::grow_build::task::types::SubagentCancelOutcome::Cancelled
            ));
            held_wait = Some(source_wait);
        } else {
            source_wait
                .response_tx
                .send(Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        acp::PermissionOptionId::new("allow-once"),
                    )),
                )))
                .unwrap();
        }
        let result = cold_recovery_stage("transcript.source_terminal", source_result)
            .await
            .unwrap();
        drop(held_wait);
        assert_eq!(result.cancelled, cancelled, "{result:?}");
        assert_eq!(result.success, !cancelled, "{result:?}");
        let source = transcript(
            crate::session::storage::load_updates_for_replay_at("source-child", &home)
                .unwrap()
                .unwrap(),
        );
        assert!(
            source.iter().any(|row| row.contains("SOURCE_TASK_PROMPT")),
            "{source:?}"
        );
        assert!(
            source.iter().any(|row| row.contains("SOURCE_THOUGHT")),
            "{source:?}"
        );
        assert!(
            source.iter().any(|row| row == "tool:source-tool"),
            "{source:?}"
        );
        if !cancelled {
            assert!(
                source.iter().any(
                    |row| row.starts_with("tool-update:") && row.contains("SOURCE_TOOL_RESULT")
                ),
                "{source:?}"
            );
            assert!(
                source
                    .iter()
                    .any(|row| row.starts_with("tool-update:") && row.contains("completed")),
                "{source:?}"
            );
        }
        let source_path = crate::session::persistence::session_dir(&crate::session::info::Info {
            id: acp::SessionId::new("source-child"),
            cwd: cwd.to_str().unwrap().into(),
        })
        .join("timeline.jsonl");
        let source_bytes = std::fs::read(&source_path).unwrap();
        let _derived = launch(
            &agent,
            &root,
            "derived-child",
            true,
            tokio_util::sync::CancellationToken::new(),
        );
        let derived_wait =
            cold_recovery_stage("transcript.derived_permission", permission_rx.recv())
                .await
                .unwrap();
        assert_eq!(derived_wait.request.session_id.0.as_ref(), "derived-child");
        assert!(
            eager_history
                .borrow()
                .as_ref()
                .expect("live Spawned must be delivered")
                .starts_with(&source),
            "inherited transcript must already be durable at live Spawned"
        );
        assert_eq!(
            std::fs::read(&source_path).unwrap(),
            source_bytes,
            "resume cannot modify the source journal"
        );
        std::fs::write(
            home.join("source-display"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        cold_recovery_stage(
            "transcript.suspend",
            agent.suspend_session_tree(
                &root,
                tokio::time::Instant::now() + std::time::Duration::from_secs(8),
            ),
        )
        .await
        .unwrap();
        drop(derived_wait);
        server.abort();
    });
}
