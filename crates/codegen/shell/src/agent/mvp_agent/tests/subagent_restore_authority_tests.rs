//! Real coordinator/actor/permission/persistence lifecycle, across OS processes.
use super::*;
use acp_transport::AcpAgentHandler as _;
use tool_types::SubagentCapabilityMode as Mode;
use tools::implementations::grow_build::task::types::{
    SubagentEvent, SubagentOwner, SubagentRequest, SubagentSpawnRequest,
};

fn delegate(
    agent: &MvpAgent,
    parent: &acp::SessionId,
    id: &str,
    profile: &str,
    mode: Option<Mode>,
) -> tokio::sync::oneshot::Receiver<tools::implementations::grow_build::task::types::SubagentResult>
{
    let (result_tx, result) = tokio::sync::oneshot::channel();
    agent
        .subagent_event_tx
        .send(SubagentEvent::Spawn(SubagentSpawnRequest {
            request: Box::new(SubagentRequest {
                id: id.into(),
                origin_tool_call_id: None,
                prompt: "Run the delegated authority regression task.".into(),
                description: "authority regression".into(),
                subagent_type: profile.into(),
                parent_session_id: parent.0.to_string(),
                parent_step_id: None,
                parent_prompt_id: None,
                resume_from: None,
                cwd: None,
                runtime_overrides:
                    tools::implementations::grow_build::task::types::SubagentRuntimeOverrides {
                        capability_mode: mode,
                        ..Default::default()
                    },
                run_in_background: true,
                surface_completion: false,
                await_to_completion: false,
                fork_context: false,
                owner: SubagentOwner::Task,
                goal_context: None,
                cancel_token: tokio_util::sync::CancellationToken::new(),
            }),
            result_tx,
        }))
        .unwrap();
    result
}

/// The mock controls only the provider and human replies. All session, child,
/// tool execution, permission, durable checkpoint and recovery code is real.
#[test]
fn suspended_child_cold_restore_retains_birth_authority_end_to_end() {
    const HOME: &str = "GROW_TEST_CHILD_AUTHORITY_HOME";
    let Ok(home) = std::env::var(HOME) else {
        let temp = tempfile::tempdir().unwrap();
        for phase in ["suspend", "missing-birth", "invalid-birth", "restore"] {
            if phase != "suspend" {
                // Fault injection happens only after the old process retired
                // all writers. Keep every other durable identity unchanged.
                let cwd = temp.path().join("workspace").canonicalize().unwrap();
                let root = std::fs::read_to_string(temp.path().join("root-id")).unwrap();
                let timeline = temp
                    .path()
                    .join("sessions")
                    .join(crate::util::grow_home::encode_cwd_dirname(
                        cwd.to_str().unwrap(),
                    ))
                    .join(root)
                    .join("timeline.jsonl");
                let mut patched = String::new();
                let mut count = 0;
                for line in std::fs::read_to_string(&timeline).unwrap().lines() {
                    let record = crate::session::history::decode_record(line.as_bytes()).unwrap();
                    if let crate::session::history::JournalFact::Domain {
                        mut event,
                        response_anchor,
                    } = record.fact
                    {
                        if let chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn),
                        ) = &mut event.kind
                        {
                            assert_eq!(spawn.subagent_id, "authority-child");
                            spawn.capability_mode = match phase {
                                "missing-birth" => None,
                                "invalid-birth" => Some("invalid-mode".into()),
                                _ => Some("read-only".into()),
                            };
                            count += 1;
                            let raw = serde_json::value::to_raw_value(&event).unwrap();
                            patched.push_str(
                                &String::from_utf8(
                                    crate::session::history::encode_domain(
                                        record.seq,
                                        &raw,
                                        response_anchor,
                                    )
                                    .unwrap(),
                                )
                                .unwrap(),
                            );
                            continue;
                        }
                    }
                    patched.push_str(line);
                    patched.push('\n');
                }
                assert_eq!(count, 1);
                std::fs::write(timeline, patched).unwrap();
            }
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::subagent_restore_authority_tests::suspended_child_cold_restore_retains_birth_authority_end_to_end", "--nocapture"])
                .env(HOME, temp.path()).env("GROW_HOME", temp.path())
                .env("GROW_TEST_CHILD_AUTHORITY_PHASE", phase)
                .output().unwrap();
            assert!(
                output.status.success(),
                "{phase} failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let phase = std::env::var("GROW_TEST_CHILD_AUTHORITY_PHASE").unwrap();
        let restore = phase != "suspend";
        let home = std::path::PathBuf::from(home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        if !restore {
            git2::Repository::init(&cwd).unwrap();
            for (name, mode) in [("birth-reader", "read-only"), ("selected-all", "all")] {
                std::fs::write(cwd.join(format!(".grow/agents/{name}.md")), format!("---\nname: {name}\ndescription: Authority regression\ncapabilityMode: {mode}\ntools: [run_terminal_cmd, task, get_task_output, kill_task]\nagentsMd: false\ndiscoverSkills: false\ninheritSkills: false\n---\nExecute the assigned task.\n")).unwrap();
            }
        }
        let sequence = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = sequence.clone();
        let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let seen = seen.clone();
            async move {
                // Ancillary title/metadata requests are not child tool Steps.
                let tool_step = body["tools"].as_array().is_some_and(|tools| tools.len() >= 2);
                if !tool_step {
                    return ([("content-type", "text/event-stream")], "data: {\"id\":\"meta\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"default\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"test\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_owned());
                }
                let index = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // Keep every child live at a real permission boundary.
                let marker = if restore { format!("restored-{index}") } else { format!("original-{index}") };
                let arguments = serde_json::json!({"command": format!("sh -c 'echo authorized >> {marker}'"), "description": marker, "background": false}).to_string();
                let delta = serde_json::json!({"id": format!("response-{index}"), "object":"chat.completion.chunk", "created":1, "model":"default", "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("call-{marker}"),"type":"function","function":{"name":"run_terminal_command","arguments":arguments}}]},"finish_reason":null}]});
                let done = serde_json::json!({"id":format!("response-{index}"),"object":"chat.completion.chunk","created":1,"model":"default","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}});
                ([("content-type", "text/event-stream")], format!("data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
            }
        }));
        let bind_address = if restore {
            std::fs::read_to_string(home.join("provider-address")).unwrap()
        } else {
            "127.0.0.1:0".to_owned()
        };
        let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
        let address = listener.local_addr().unwrap();
        if !restore {
            std::fs::write(home.join("provider-address"), address.to_string()).unwrap();
        }
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Reuse the endpoint across processes: a changed transport route must
        // correctly fail checkpoint validation independently of this test.
        let raw = format!(
            r#"
[models]
default = "test/default"
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://{address}/v1"
api_key = "test-only"
[provider.test.models.default]
context_window = 128000
[subagents]
permission_mode = "ask"
max_depth = 3
"#
        );
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        cfg.subagent_permission_mode = workspace::permission::types::RequestPermissionMode::Ask;
        let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
        let (permission_tx, mut permission_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::task::spawn_local(async move {
            while let Some(message) = gateway_rx.recv().await {
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
        let agent = MvpAgent::new(GatewaySender::new(gateway_tx), &cfg).unwrap();
        cold_recovery_stage("authority.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let child_id = acp::SessionId::new("authority-child");
        let root_id;
        if !restore {
            root_id = cold_recovery_stage(
                "authority.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("root-id"), root_id.0.as_ref()).unwrap();
            let _child_result =
                delegate(&agent, &root_id, child_id.0.as_ref(), "birth-reader", None);
            let first = cold_recovery_stage("authority.first_permission", permission_rx.recv())
                .await
                .unwrap();
            assert_eq!(first.request.session_id, child_id);
            let child = agent.control_session_handle(&child_id).unwrap();
            assert_eq!(
                child
                    .delegable_capability_ceiling
                    .as_ref()
                    .unwrap()
                    .constrain_mode(Mode::All),
                Mode::ReadOnly
            );
            let definition = agent::discovery::by_name_in_cwd("selected-all", &cwd).unwrap();
            let (responds_to, switched) = tokio::sync::oneshot::channel();
            child
                .cmd_tx
                .send(crate::session::SessionCommand::RebuildAgentForDefinition {
                    definition,
                    intent: None,
                    responds_to,
                })
                .unwrap();
            first
                .response_tx
                .send(Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        acp::PermissionOptionId::new("allow-once"),
                    )),
                )))
                .unwrap();
            cold_recovery_stage("authority.select_all", switched)
                .await
                .unwrap()
                .unwrap();
            let pending = cold_recovery_stage("authority.second_permission", permission_rx.recv())
                .await
                .unwrap();
            assert_eq!(pending.request.session_id, child_id);
            assert!(cwd.join("original-0").is_file(), "approved tool ran");
            assert!(!cwd.join("original-1").exists(), "pending tool never ran");
            assert_eq!(child.agent_profile.name(), "selected-all");
            assert_eq!(
                child
                    .delegable_capability_ceiling
                    .as_ref()
                    .unwrap()
                    .constrain_mode(Mode::All),
                Mode::ReadOnly
            );
            let (operation, nodes) = cold_recovery_stage(
                "authority.suspend",
                agent.suspend_session_tree(
                    &root_id,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(9),
                ),
            )
            .await
            .unwrap();
            assert_eq!(nodes.len(), 2);
            assert!(
                nodes
                    .iter()
                    .any(|node| node.session_id == child_id.0.as_ref())
            );
            assert!(child.cmd_tx.is_closed(), "old child actor retired");
            assert!(
                agent.control_session_handle(&root_id).is_none(),
                "old root retired"
            );
            std::fs::write(home.join("operation-id"), operation).unwrap();
            drop(pending);
        } else {
            root_id = acp::SessionId::new(std::fs::read_to_string(home.join("root-id")).unwrap());
            let loaded = cold_recovery_stage(
                "authority.cold_load",
                agent.load_session(acp::LoadSessionRequest::new(root_id.clone(), cwd.clone())),
            )
            .await;
            if phase != "restore" {
                let error = format!(
                    "{:?}",
                    loaded.expect_err("invalid birth fact must block the tree")
                );
                assert!(error.contains("birth capability_mode"), "{error}");
                assert!(
                    error.contains(if phase == "missing-birth" {
                        "missing"
                    } else {
                        "invalid"
                    }),
                    "{error}"
                );
                assert!(
                    agent.control_session_handle(&child_id).is_none(),
                    "no child was installed"
                );
                assert_eq!(
                    sequence.load(std::sync::atomic::Ordering::SeqCst),
                    0,
                    "no node sampled before all birth facts were validated"
                );
                if let Some(root) = agent.control_session_handle(&root_id) {
                    let runtime = root.chat_state_handle.runtime_state().await.unwrap();
                    assert_eq!(
                        runtime.phase,
                        chat_state::session_runtime::RuntimePhase::Suspended
                    );
                    assert!(!runtime.resume_committed);
                    let events = root.chat_state_handle.timeline_events().await.unwrap();
                    assert!(
                        !events.iter().any(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Subagent(
                                chat_state::SubagentEvent::Ended(_)
                            )
                        )),
                        "admission failure must not terminalize the original child"
                    );
                }
                server.abort();
                return;
            }
            loaded.unwrap();
            let root = agent.control_session_handle(&root_id).unwrap();
            let child = agent
                .control_session_handle(&child_id)
                .expect("same child identity restored");
            for handle in [&root, &child] {
                let runtime = handle.chat_state_handle.runtime_state().await.unwrap();
                assert_eq!(
                    runtime.phase,
                    chat_state::session_runtime::RuntimePhase::Running
                );
                assert_eq!(
                    runtime.operation_id.as_deref(),
                    Some(
                        std::fs::read_to_string(home.join("operation-id"))
                            .unwrap()
                            .as_str()
                    )
                );
                assert!(runtime.resume_committed);
            }
            assert_eq!(child.agent_profile.name(), "selected-all");
            assert_eq!(
                child
                    .delegable_capability_ceiling
                    .as_ref()
                    .unwrap()
                    .constrain_mode(Mode::All),
                Mode::ReadOnly,
                "restored profile cannot expand birth RWX"
            );
            let pending =
                cold_recovery_stage("authority.restored_permission", permission_rx.recv())
                    .await
                    .unwrap();
            assert_eq!(pending.request.session_id, child_id);
            assert_eq!(
                pending.request.tool_call.tool_call_id.0.as_ref(),
                "call-original-1",
                "restore rebinds the original undispatched call"
            );
            assert!(
                !cwd.join("original-1").exists(),
                "restored call still waits for a new decision"
            );
            assert!(
                !cwd.join("restored-0").exists(),
                "old permit was not reused"
            );
            pending
                .response_tx
                .send(Ok(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        acp::PermissionOptionId::new("allow-once"),
                    )),
                )))
                .unwrap();
            let child_wait = cold_recovery_stage("authority.next_permission", permission_rx.recv())
                .await
                .unwrap();
            assert!(
                cwd.join("original-1").is_file(),
                "fresh approval executes the restored original call"
            );
            assert_eq!(
                std::fs::read_to_string(cwd.join("original-0")).unwrap(),
                "authorized\n",
                "completed tool is never replayed"
            );
            assert_eq!(
                std::fs::read_to_string(cwd.join("original-1")).unwrap(),
                "authorized\n",
                "restored call executes exactly once"
            );
            assert_eq!(
                child_wait.request.tool_call.tool_call_id.0.as_ref(),
                "call-restored-0"
            );
            assert!(
                !cwd.join("restored-0").exists(),
                "new call requires its own approval"
            );
            // Real descendant admission requests All; its effective birth fact
            // must still be constrained by the restored immediate parent.
            let _grandchild_result = delegate(
                &agent,
                &child_id,
                "authority-grandchild",
                "selected-all",
                Some(Mode::All),
            );
            let grandchild_wait =
                cold_recovery_stage("authority.grandchild_permission", permission_rx.recv())
                    .await
                    .unwrap();
            let grandchild = agent
                .control_session_handle(&acp::SessionId::new("authority-grandchild"))
                .unwrap();
            assert_eq!(
                grandchild
                    .delegable_capability_ceiling
                    .as_ref()
                    .unwrap()
                    .constrain_mode(Mode::All),
                Mode::ReadOnly
            );
            assert_eq!(
                grandchild_wait.request.session_id.0.as_ref(),
                "authority-grandchild"
            );
            let events = root.chat_state_handle.timeline_events().await.unwrap();
            let spawns = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) => Some(spawn),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                spawns
                    .iter()
                    .filter(|spawn| spawn.subagent_id == child_id.0.as_ref())
                    .count(),
                1,
                "restore never spawns a replacement child"
            );
            assert!(
                spawns
                    .iter()
                    .all(|spawn| spawn.capability_mode.as_deref() == Some("read-only"))
            );
            cold_recovery_stage(
                "authority.resuspend",
                agent.suspend_session_tree(
                    &root_id,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(9),
                ),
            )
            .await
            .unwrap();
            drop((child_wait, grandchild_wait));
        }
        server.abort();
    });
}
