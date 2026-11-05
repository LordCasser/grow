use super::*;
use acp_transport::AcpAgentHandler as _;
use tools::implementations::grow_build::task::types::{
    SubagentEvent, SubagentOwner, SubagentRequest, SubagentRuntimeOverrides, SubagentSpawnRequest,
};

fn launch(
    agent: &MvpAgent,
    root: &acp::SessionId,
    id: &str,
    resume: bool,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::sync::oneshot::Receiver<tools::implementations::grow_build::task::types::SubagentResult>
{
    launch_with_selection(agent, root, id, resume, Default::default(), cancel)
}

fn launch_with_selection(
    agent: &MvpAgent,
    root: &acp::SessionId,
    id: &str,
    resume: bool,
    runtime_overrides: SubagentRuntimeOverrides,
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
                runtime_overrides,
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

#[test]
fn task_resume_selection_reaches_provider_and_preserves_canonical_source() {
    const HOME: &str = "GROW_TEST_RESUME_SELECTION_HOME";
    let Ok(home) = std::env::var(HOME) else {
        for source_case in ["completed", "cancelled", "unset", "changed"] {
            let home = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::task_resume_presentation_tests::task_resume_selection_reaches_provider_and_preserves_canonical_source", "--nocapture"])
                .env(HOME, home.path())
                .env("GROW_HOME", home.path())
                .env("GROW_TEST_RESUME_SELECTION_SOURCE", source_case)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{source_case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let home = std::path::PathBuf::from(home);
        let source_case = std::env::var("GROW_TEST_RESUME_SELECTION_SOURCE").unwrap();
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        git2::Repository::init(&cwd).unwrap();
        std::fs::write(
            cwd.join(".grow/agents/resume-reader.md"),
            "---\nname: resume-reader\ndescription: Resume selection fixture\ncapabilityMode: all\neffort: high\ntools: [run_terminal_cmd]\nagentsMd: false\ndiscoverSkills: false\ninheritSkills: false\n---\nRESUME_AGENT_ROLE: follow the assigned task.\n",
        )
        .unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let seen = requests.clone();
        let source_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let pause_source = matches!(source_case.as_str(), "cancelled" | "changed");
        let provider = axum::Router::new().route("/{version}/chat/completions", axum::routing::post(move |axum::extract::Path(version): axum::extract::Path<String>, axum::Json(body): axum::Json<serde_json::Value>| {
            let seen = seen.clone();
            let source_calls = source_calls.clone();
            async move {
                let expected_version = match body["model"].as_str().unwrap() {
                    "alternate-wire" | "limited-wire" | "tiny-wire" => "v2",
                    _ => "v1",
                };
                assert_eq!(version, expected_version, "provider endpoint disagrees with model route: {body}");
                seen.lock().unwrap().push(body.clone());
                let messages = body["messages"].to_string();
                let source = messages.contains("SOURCE_TASK_PROMPT") && !messages.contains("DERIVED_TASK_PROMPT");
                let first = source && source_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
                let tool = first && pause_source;
                let delta = if tool {
                    serde_json::json!({"role":"assistant","tool_calls":[{"index":0,"id":"source-pause","type":"function","function":{"name":"run_terminal_command","arguments":"{\"command\":\"sh -c 'echo SOURCE_TOOL_RESULT'\",\"description\":\"source boundary\",\"background\":false}"}}]})
                } else {
                    serde_json::json!({"role":"assistant","content": if source { "SOURCE_FINAL" } else { "DERIVED_FINAL" }})
                };
                let part = serde_json::json!({"id":"selection-response","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
                let done = serde_json::json!({"id":"selection-response","object":"chat.completion.chunk","created":1,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":if tool { "tool_calls" } else { "stop" }}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}});
                ([("content-type","text/event-stream")], format!("data: {part}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap();
        });
        let raw = format!(
            r#"
[models]
default = "test/parent"
allowed_models = ["test/parent", "test/source", "test/switched", "test/hidden", "replacement/*"]
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://{address}/v1"
api_key = "fixture-only"
[provider.test.models.parent]
model = "parent-wire"
context_window = 128000
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.test.models.source]
model = "source-wire"
context_window = 128000
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.test.models.switched]
model = "switched-wire"
context_window = 128000
[provider.test.models.hidden]
hidden = true
context_window = 128000
[provider.test.models.blocked]
context_window = 128000
[provider.replacement]
api_backend = "chat_completions"
[provider.replacement.options]
base_url = "http://{address}/v2"
api_key = "fixture-only"
[provider.replacement.models.alternate]
model = "alternate-wire"
context_window = 128000
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.replacement.models.limited]
model = "limited-wire"
context_window = 128000
reasoning_efforts = ["none"]
[provider.replacement.models.tiny]
model = "tiny-wire"
context_window = 1
reasoning_efforts = ["none", "high"]
[subagents]
permission_mode = "ask"
"#
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
                        let _ = args.response_tx.send(Ok(()));
                    }
                    _ => {}
                }
            }
        });
        let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
        cold_recovery_stage("selection.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let root = cold_recovery_stage(
            "selection.new",
            agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
        )
        .await
        .unwrap()
        .session_id;
        let source_result = launch_with_selection(
            &agent,
            &root,
            "source-child",
            false,
            SubagentRuntimeOverrides {
                model: Some("test/source".into()),
                reasoning_effort: Some((source_case != "unset").then(|| "high".into())),
                ..Default::default()
            },
            tokio_util::sync::CancellationToken::new(),
        );
        let mut held_permission = None;
        if pause_source {
            let permission =
                cold_recovery_stage("selection.source_permission", permission_rx.recv())
                    .await
                    .unwrap();
            let before = requests.lock().unwrap().len();
            let active_resume = cold_recovery_stage(
                "selection.live_source",
                launch_with_selection(
                    &agent,
                    &root,
                    "live-source-attempt",
                    true,
                    SubagentRuntimeOverrides {
                        model: Some("replacement/alternate".into()),
                        reasoning_effort: Some(Some("none".into())),
                        ..Default::default()
                    },
                    tokio_util::sync::CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            assert!(!active_resume.success, "{active_resume:?}");
            assert!(
                active_resume
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("still running or settling")
            );
            assert_eq!(requests.lock().unwrap().len(), before);
            if source_case == "cancelled" {
                let (respond_to, cancelled) = tokio::sync::oneshot::channel();
                agent.subagent_event_tx.send(SubagentEvent::Cancel(tools::implementations::grow_build::task::types::SubagentCancelRequest {
                    parent_session_id: Some(root.0.to_string()),
                    target: tools::implementations::grow_build::task::types::SubagentCancelTarget::SubagentId("source-child".into()),
                    respond_to,
                })).unwrap();
                assert!(matches!(cold_recovery_stage("selection.cancel", cancelled).await.unwrap(), tools::implementations::grow_build::task::types::SubagentCancelOutcome::Cancelled));
                held_permission = Some(permission);
            } else {
                let child = agent
                    .active_child_sessions
                    .borrow()
                    .get(&acp::SessionId::new("source-child"))
                    .unwrap()
                    .clone();
                let catalog = Arc::new(agent.models_manager.published_catalog());
                let route = catalog
                    .resolve_session_route(
                        &crate::agent::models::ModelId::new("test/switched"),
                        None,
                    )
                    .unwrap();
                let (responds_to, selected) = tokio::sync::oneshot::channel();
                child
                    .cmd_tx
                    .send(crate::session::SessionCommand::SetSessionModel {
                        route,
                        catalog: Some(catalog),
                        intent: None,
                        responds_to,
                    })
                    .unwrap();
                permission
                    .response_tx
                    .send(Ok(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(acp::PermissionOptionId::new(
                                "allow-once",
                            )),
                        ),
                    )))
                    .unwrap();
                cold_recovery_stage("selection.source_switch", selected)
                    .await
                    .unwrap()
                    .unwrap();
            }
        }
        let source_result = cold_recovery_stage("selection.source_terminal", source_result)
            .await
            .unwrap();
        drop(held_permission);
        assert_eq!(
            source_result.cancelled,
            source_case == "cancelled",
            "{source_result:?}"
        );
        assert_eq!(
            source_result.success,
            source_case != "cancelled",
            "{source_result:?}"
        );
        let source_model = if source_case == "changed" {
            "test/switched"
        } else {
            "test/source"
        };
        let source_effort =
            (!matches!(source_case.as_str(), "unset" | "changed")).then_some("high");
        let foreign_root = cold_recovery_stage(
            "selection.foreign_parent",
            agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
        )
        .await
        .unwrap()
        .session_id;
        let before_foreign = requests.lock().unwrap().len();
        let foreign = cold_recovery_stage(
            "selection.foreign_source",
            launch_with_selection(
                &agent,
                &foreign_root,
                "foreign-source-attempt",
                true,
                SubagentRuntimeOverrides {
                    model: Some("replacement/alternate".into()),
                    reasoning_effort: Some(Some("none".into())),
                    ..Default::default()
                },
                tokio_util::sync::CancellationToken::new(),
            ),
        )
        .await
        .unwrap();
        assert!(!foreign.success, "{foreign:?}");
        assert!(
            foreign
                .error
                .as_deref()
                .unwrap()
                .contains("source is unavailable to the current parent")
        );
        assert_eq!(requests.lock().unwrap().len(), before_foreign);
        let source_path = crate::session::persistence::session_dir(&crate::session::info::Info {
            id: acp::SessionId::new("source-child"),
            cwd: cwd.to_str().unwrap().into(),
        })
        .join("timeline.jsonl");
        let source_bytes = std::fs::read(&source_path).unwrap();
        let root_handle = agent.control_session_handle(&root).unwrap();
        let source_facts: Vec<_> = root_handle.chat_state_handle.timeline_events().await.unwrap().into_iter().filter(|event| matches!(&event.kind,
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) if spawn.subagent_id == "source-child") || matches!(&event.kind,
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(ended)) if ended.subagent_id == "source-child")).collect();
        assert_eq!(source_facts.len(), 2);
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
        let source_events = storage
            .read_timeline_events_sync(&crate::session::info::Info {
                id: acp::SessionId::new("source-child"),
                cwd: cwd.to_str().unwrap().into(),
            })
            .unwrap();
        let final_source = crate::session::persistence::latest_model_selection(&source_events)
            .unwrap()
            .unwrap();
        assert_eq!(final_source.0.0.as_ref(), source_model);
        assert_eq!(
            final_source.1.map(|effort| effort.to_string()).as_deref(),
            source_effort
        );
        let source_tools = requests
            .lock()
            .unwrap()
            .iter()
            .find(|body| body["messages"].to_string().contains("SOURCE_TASK_PROMPT"))
            .unwrap()["tools"]
            .clone();
        for (id, model, effort, expected_error) in [
            ("omitted", None, None, None),
            ("model", Some("replacement/alternate"), None, None),
            (
                "effort",
                None,
                Some("none"),
                (source_case == "changed").then_some("is not offered by model"),
            ),
            ("both", Some("replacement/alternate"), Some("none"), None),
            (
                "invalid",
                None,
                Some("bogus"),
                Some("Invalid subagent reasoning effort"),
            ),
            (
                "unsupported",
                Some("replacement/limited"),
                Some("high"),
                Some("is not offered by model"),
            ),
            (
                "retained",
                Some("replacement/limited"),
                None,
                source_effort.map(|_| "is not offered by model"),
            ),
            (
                "unknown",
                Some("unknown/model"),
                None,
                Some("Unknown Task.model ID"),
            ),
            (
                "hidden",
                Some("test/hidden"),
                None,
                Some("Unknown Task.model ID"),
            ),
            (
                "blocked",
                Some("test/blocked"),
                None,
                Some("Unknown Task.model ID"),
            ),
            (
                "small",
                Some("replacement/tiny"),
                None,
                Some("source transcript"),
            ),
        ] {
            let before = requests.lock().unwrap().len();
            let result = cold_recovery_stage(
                "selection.derived",
                launch_with_selection(
                    &agent,
                    &root,
                    id,
                    true,
                    SubagentRuntimeOverrides {
                        model: model.map(str::to_owned),
                        reasoning_effort: effort.map(|effort| Some(effort.into())),
                        ..Default::default()
                    },
                    tokio_util::sync::CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            let bodies = requests.lock().unwrap()[before..].to_vec();
            if let Some(expected_error) = expected_error {
                assert!(
                    !result.success
                        && result
                            .error
                            .as_deref()
                            .is_some_and(|error| error.contains(expected_error)),
                    "{id}: {result:?}"
                );
                assert!(
                    bodies.is_empty(),
                    "{id} issued provider requests: {bodies:?}"
                );
            } else {
                assert!(result.success, "{id}: {result:?}");
                let selected_id = model.unwrap_or(source_model);
                let selected_effort = effort.or(source_effort);
                let entry = agent.models_manager.models()[selected_id].clone();
                let expected_transport = sampling_types::model_image_input_key_from_parts(
                    &entry.info.model,
                    &entry.info.api_backend,
                    &entry.info.base_url,
                    &entry.info.query_params,
                );
                assert!(!bodies.is_empty(), "{id}: no provider request");
                for body in bodies {
                    assert_eq!(body["model"], entry.info.model, "{id}: {body}");
                    assert_eq!(
                        body["reasoning_effort"].as_str(),
                        selected_effort,
                        "{id}: {body}"
                    );
                    assert_eq!(
                        body["tools"], source_tools,
                        "model selection changed Agent tools"
                    );
                    assert!(body["messages"].to_string().contains("RESUME_AGENT_ROLE"));
                }
                let events = storage
                    .read_timeline_events_sync(&crate::session::info::Info {
                        id: acp::SessionId::new(id),
                        cwd: cwd.to_str().unwrap().into(),
                    })
                    .unwrap();
                let selection = crate::session::persistence::latest_model_selection(&events)
                    .unwrap()
                    .unwrap();
                assert_eq!(selection.0.0.as_ref(), selected_id);
                assert_eq!(
                    selection.1.map(|effort| effort.to_string()).as_deref(),
                    selected_effort
                );
                assert_eq!(selection.3, expected_transport);
                let parent_events = root_handle
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap();
                let spawn = parent_events
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn),
                        ) if spawn.subagent_id == id => Some(spawn),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(spawn.effective_model_id, selected_id);
                assert_eq!(spawn.reasoning_effort, selection.1);
                assert_eq!(spawn.model_transport_key, selection.3);
                assert_eq!(spawn.subagent_type, "resume-reader");
                assert_eq!(spawn.capability_mode.as_deref(), Some("all"));
                assert_eq!(spawn.birth_mcp_servers, Some(Default::default()));
                assert_eq!(spawn.resumed_from.as_deref(), Some("source-child"));
            }
            assert_eq!(
                std::fs::read(&source_path).unwrap(),
                source_bytes,
                "{id} changed source Timeline"
            );
        }
        // A catalog key may be repointed between epochs. Only an explicit model
        // releases the transport pin, even if it repeats the same catalog ID.
        let mut repointed = agent.models_manager.models()[source_model].clone();
        repointed.info.model = "repointed-wire".into();
        agent
            .models_manager
            .insert_test_entry(source_model, repointed);
        for (id, model, effort, success, error) in [
            ("repointed-omitted", None, None, false, "model transport"),
            (
                "repointed-effort",
                None,
                Some("none"),
                false,
                if source_case == "changed" {
                    "is not offered by model"
                } else {
                    "model transport"
                },
            ),
            ("repointed-explicit", Some(source_model), None, true, ""),
            (
                "repointed-alternate",
                Some("replacement/alternate"),
                Some("none"),
                true,
                "",
            ),
        ] {
            let before = requests.lock().unwrap().len();
            let result = cold_recovery_stage(
                "selection.repointed",
                launch_with_selection(
                    &agent,
                    &root,
                    id,
                    true,
                    SubagentRuntimeOverrides {
                        model: model.map(str::to_owned),
                        reasoning_effort: effort.map(|effort| Some(effort.into())),
                        ..Default::default()
                    },
                    tokio_util::sync::CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            assert_eq!(result.success, success, "{id}: {result:?}");
            let bodies = requests.lock().unwrap()[before..].to_vec();
            if success {
                assert!(!bodies.is_empty());
                assert_eq!(
                    bodies[0]["model"],
                    if model == Some(source_model) {
                        "repointed-wire"
                    } else {
                        "alternate-wire"
                    }
                );
            } else {
                assert!(
                    result.error.as_deref().unwrap().contains(error),
                    "{id}: {result:?}"
                );
                assert!(bodies.is_empty(), "{id}: {bodies:?}");
            }
        }
        let mut unavailable = cfg;
        unavailable.models.disabled_models = Some(vec![source_model.into()]);
        agent.models_manager.apply_config(unavailable).unwrap();
        for (id, model, success) in [
            ("unavailable-omitted", None, false),
            ("unavailable-override", Some("replacement/alternate"), true),
        ] {
            let before = requests.lock().unwrap().len();
            let result = cold_recovery_stage(
                "selection.unavailable",
                launch_with_selection(
                    &agent,
                    &root,
                    id,
                    true,
                    SubagentRuntimeOverrides {
                        model: model.map(str::to_owned),
                        ..Default::default()
                    },
                    tokio_util::sync::CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            assert_eq!(result.success, success, "{id}: {result:?}");
            if !success {
                assert!(
                    result
                        .error
                        .as_deref()
                        .unwrap()
                        .contains("not present in the model catalogue"),
                    "{result:?}"
                );
                assert_eq!(requests.lock().unwrap().len(), before);
            }
        }
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
        let after_facts: Vec<_> = root_handle.chat_state_handle.timeline_events().await.unwrap().into_iter().filter(|event| matches!(&event.kind,
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) if spawn.subagent_id == "source-child") || matches!(&event.kind,
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(ended)) if ended.subagent_id == "source-child")).collect();
        assert_eq!(
            serde_json::to_value(&after_facts).unwrap(),
            serde_json::to_value(&source_facts).unwrap(),
            "derived selections changed source lifecycle facts"
        );
        cold_recovery_stage(
            "selection.suspend",
            agent.suspend_session_tree(
                &root,
                tokio::time::Instant::now() + std::time::Duration::from_secs(8),
            ),
        )
        .await
        .unwrap();
        server.abort();
    });
}
