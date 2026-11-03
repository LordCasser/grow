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
        for birth_case in [
            "empty",
            "narrow",
            "removed",
            "alias",
            "changed-model",
            "unavailable",
        ] {
            let temp = tempfile::tempdir().unwrap();
            for phase in [
                "suspend",
                "missing-birth",
                "invalid-birth",
                "missing-mcp",
                "plugin-fallback",
                "restore",
            ] {
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
                    for line in std::fs::read_to_string(temp.path().join("original-timeline"))
                        .unwrap()
                        .lines()
                    {
                        let record =
                            crate::session::history::decode_record(line.as_bytes()).unwrap();
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
                                match phase {
                                    "missing-birth" => spawn.capability_mode = None,
                                    "invalid-birth" => {
                                        spawn.capability_mode = Some("invalid-mode".into())
                                    }
                                    "missing-mcp" => spawn.birth_mcp_servers = None,
                                    _ => {}
                                }
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
                    if phase == "restore" {
                        patched =
                            std::fs::read_to_string(temp.path().join("original-timeline")).unwrap();
                    }
                    std::fs::write(timeline, patched).unwrap();
                    std::fs::copy(
                        temp.path().join("original-child-timeline"),
                        path_for_root(temp.path(), &cwd, &acp::SessionId::new("authority-child")),
                    )
                    .unwrap();
                }
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::subagent_restore_authority_tests::suspended_child_cold_restore_retains_birth_authority_end_to_end", "--nocapture"])
                .env(HOME, temp.path())
                .env("GROW_TEST_CHILD_MCP_BIRTH", birth_case).env("GROW_HOME", temp.path())
                .env("GROW_TEST_CHILD_AUTHORITY_PHASE", phase)
                .output().unwrap();
                assert!(
                    output.status.success(),
                    "{phase} (birth_case={birth_case}) failed:\n{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
                if phase == "suspend" {
                    let cwd = temp.path().join("workspace").canonicalize().unwrap();
                    let id = acp::SessionId::new(
                        std::fs::read_to_string(temp.path().join("root-id")).unwrap(),
                    );
                    let timeline = temp
                        .path()
                        .join("sessions")
                        .join(crate::util::grow_home::encode_cwd_dirname(
                            cwd.to_str().unwrap(),
                        ))
                        .join(id.0.as_ref())
                        .join("timeline.jsonl");
                    std::fs::copy(timeline, temp.path().join("original-timeline")).unwrap();
                    std::fs::copy(
                        path_for_root(temp.path(), &cwd, &acp::SessionId::new("authority-child")),
                        temp.path().join("original-child-timeline"),
                    )
                    .unwrap();
                }
            }
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let phase = std::env::var("GROW_TEST_CHILD_AUTHORITY_PHASE").unwrap();
        let restore = phase != "suspend";
        let birth_case = std::env::var("GROW_TEST_CHILD_MCP_BIRTH").unwrap();
        let gate_parent_mcp_initialize = phase == "restore" && birth_case == "narrow";
        let (parent_mcp_initialize_gate, parent_mcp_initialize_gate_rx) =
            tokio::sync::watch::channel(!gate_parent_mcp_initialize);
        let parent_mcp_initialize_started = Arc::new(tokio::sync::Notify::new());
        let expected_birth = if birth_case == "empty" {
            std::collections::BTreeSet::new()
        } else {
            std::collections::BTreeSet::from(["birth".to_owned()])
        };
        let available_birth = if matches!(
            birth_case.as_str(),
            "narrow" | "alias" | "changed-model" | "unavailable"
        ) {
            expected_birth.clone()
        } else {
            std::collections::BTreeSet::new()
        };
        let home = std::path::PathBuf::from(home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        if !restore {
            git2::Repository::init(&cwd).unwrap();
            for (name, mode) in [("birth-reader", "read-only"), ("selected-all", "all")] {
                let inheritance = if name == "selected-all" {
                    "all"
                } else if birth_case == "empty" {
                    "none"
                } else {
                    "\n  named: [birth]"
                };
                std::fs::write(cwd.join(format!(".grow/agents/{name}.md")), format!("---\nname: {name}\ndescription: Authority regression\ncapabilityMode: {mode}\nmcpInheritance: {inheritance}\ntools: [run_terminal_cmd, task, get_command_output, list_subagent, get_subagent_status, wait_subagent, kill_task]\nagentsMd: false\ndiscoverSkills: false\ninheritSkills: false\n---\nExecute the assigned task.\n")).unwrap();
            }
        }
        if phase == "restore" {
            // The active Agent was durably switched to selected-all before
            // suspension. Continuing it must not re-admit the old birth type.
            std::fs::remove_file(cwd.join(".grow/agents/birth-reader.md")).unwrap();
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
        let mcp_calls = Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::<
            String,
            usize,
        >::new()));
        let observed_calls = mcp_calls.clone();
        let initialize_gate = parent_mcp_initialize_gate_rx.clone();
        let initialize_started = parent_mcp_initialize_started.clone();
        let reject_birth_initialize = restore && birth_case == "unavailable";
        let app = app.route("/mcp/{name}", axum::routing::post(move |axum::extract::Path(name): axum::extract::Path<String>, axum::Json(message): axum::Json<serde_json::Value>| {
            use axum::response::IntoResponse as _;
            let calls = observed_calls.clone();
            let initialize_gate = initialize_gate.clone();
            let initialize_started = initialize_started.clone();
            async move {
                if message.get("id").is_none() { return axum::http::StatusCode::ACCEPTED.into_response(); }
                let result = match message["method"].as_str().unwrap() {
                    "initialize" => {
                        if reject_birth_initialize && name == "birth" {
                            return axum::Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": message["id"],
                                "error": {"code": -32603, "message": "controlled MCP initialize failure"}
                            })).into_response();
                        }
                        let mut gate = initialize_gate;
                        while !*gate.borrow_and_update() {
                            initialize_started.notify_one();
                            if gate.changed().await.is_err() {
                                break;
                            }
                        }
                        serde_json::json!({"protocolVersion":message["params"]["protocolVersion"], "capabilities":{"tools":{}}, "serverInfo":{"name":name,"version":"1"}})
                    },
                    "tools/list" => serde_json::json!({"tools":[{"name":"probe","description":"Birth membership probe","inputSchema":{"type":"object","properties":{}}, "annotations":{"readOnlyHint":true,"destructiveHint":false}}]}),
                    "tools/call" => {
                        *calls.lock().unwrap().entry(name.clone()).or_default() += 1;
                        serde_json::json!({"content":[{"type":"text","text":format!("current incarnation {name}")}], "isError":false})
                    }
                    _ => panic!("unexpected MCP request {message}"),
                };
                axum::Json(serde_json::json!({"jsonrpc":"2.0", "id":message["id"], "result":result})).into_response()
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
        let mcp_servers: Vec<_> = ["birth", "extra"]
            .into_iter()
            .filter(|name| !(restore && birth_case == "removed" && *name == "birth"))
            .map(|name| {
                acp::McpServer::Http(
                    acp::McpServerHttp::new(name, format!("http://{address}/mcp/{name}"))
                        .headers(vec![]),
                )
            })
            .collect();
        // Reuse the endpoint across processes: a changed transport route must
        // correctly fail checkpoint validation independently of this test.
        let catalog_key = if restore && birth_case == "alias" {
            "renamed"
        } else {
            "default"
        };
        let raw = format!(
            r#"
[models]
default = "test/{catalog_key}"
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://{address}/v1"
api_key = "test-only"
[provider.test.models.{catalog_key}]
model = "default"
context_window = 128000
[provider.test.models.second]
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
        let replaced_native_profile = if phase == "plugin-fallback" {
            let native = cwd.join(".grow/agents/selected-all.md");
            let original = std::fs::read(&native).unwrap();
            let plugin = home.join("replacement-plugin");
            std::fs::create_dir_all(plugin.join("agents")).unwrap();
            std::fs::write(plugin.join("plugin.json"), r#"{"name":"replacement"}"#).unwrap();
            std::fs::write(plugin.join("agents/selected-all.md"), &original).unwrap();
            std::fs::remove_file(&native).unwrap();
            cfg.plugins.cli_plugin_dirs = vec![plugin];
            Some((native, original))
        } else {
            None
        };
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
        if phase == "plugin-fallback" {
            agent.ensure_plugin_registry();
            let plugins = agent.plugin_registry_handle.snapshot().unwrap();
            let resolved = agent::discovery::by_name_in_cwd_with_plugins(
                "selected-all",
                &cwd,
                Some(plugins.as_ref()),
            )
            .expect("fixture must expose the unique same-name plugin fallback");
            assert_eq!(resolved.selector_identity(), "replacement:selected-all");
        }
        let child_id = acp::SessionId::new("authority-child");
        let root_id;
        if !restore {
            root_id = cold_recovery_stage(
                "authority.new",
                agent.new_session(
                    acp::NewSessionRequest::new(cwd.clone()).mcp_servers(mcp_servers.clone()),
                ),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("root-id"), root_id.0.as_ref()).unwrap();
            let root = agent.control_session_handle(&root_id).unwrap();
            wait_for_mcp_pool(&root, 2).await;
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
            let selected_model = if birth_case == "changed-model" {
                let catalog = Arc::new(agent.models_manager.published_catalog());
                let route = catalog
                    .resolve_session_route(&crate::agent::models::ModelId::new("test/second"), None)
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
                Some(selected)
            } else {
                None
            };
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
            if let Some(selected) = selected_model {
                cold_recovery_stage("authority.select_child_model", selected)
                    .await
                    .unwrap()
                    .unwrap();
            }
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
            assert_mcp_membership(&child, &root, &expected_birth, false).await;
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
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
            let birth_events = storage
                .read_timeline_events_sync(&crate::session::info::Info {
                    id: root_id.clone(),
                    cwd: cwd.to_str().unwrap().to_owned(),
                })
                .unwrap();
            let born = birth_events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) if spawn.subagent_id == child_id.0.as_ref() => Some(spawn),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                born.birth_mcp_servers.as_ref(),
                Some(&expected_birth),
                "check the actual durable birth grant before fault injection"
            );
            std::fs::write(home.join("operation-id"), operation).unwrap();
            drop(pending);
        } else {
            root_id = acp::SessionId::new(std::fs::read_to_string(home.join("root-id")).unwrap());
            let load_request = acp::LoadSessionRequest::new(root_id.clone(), cwd.clone())
                .mcp_servers(mcp_servers.clone());
            let loaded = if gate_parent_mcp_initialize {
                let (loaded, initialize_reached, remained_pending, command_executor_responded, snapshot_cancelled) =
                    cold_recovery_stage("authority.cold_load", async {
                        let load_future = agent.load_session(load_request);
                        tokio::pin!(load_future);
                        let initialize_wait = parent_mcp_initialize_started.notified();
                        tokio::pin!(initialize_wait);
                        let mut early_result = tokio::select! {
                            _ = &mut initialize_wait => None,
                            result = &mut load_future => Some(result),
                        };
                        let initialize_reached = early_result.is_none();

                        let (command_executor_responded, snapshot_cancelled) = if initialize_reached {
                            if let Some(root) = agent.control_session_handle(&root_id) {
                                let responsive = tokio::time::timeout(
                                    std::time::Duration::from_millis(100),
                                    root.snapshot_mcp_pool(),
                                )
                                .await
                                .is_ok_and(|pool| pool.is_some());
                                let cancellation = tokio_util::sync::CancellationToken::new();
                                let snapshot = root.snapshot_initialized_mcp_pool(&cancellation);
                                tokio::pin!(snapshot);
                                let pending = tokio::time::timeout(
                                    std::time::Duration::from_millis(100), &mut snapshot,
                                ).await.is_err();
                                cancellation.cancel();
                                let cancelled = tokio::time::timeout(
                                    std::time::Duration::from_millis(100), snapshot,
                                ).await.is_ok_and(|pool| pool.is_none());
                                (responsive, pending && cancelled)
                            } else {
                                (false, false)
                            }
                        } else {
                            (false, false)
                        };
                        if initialize_reached
                            && let Ok(result) = tokio::time::timeout(
                                std::time::Duration::from_millis(100),
                                &mut load_future,
                            )
                            .await
                        {
                            early_result = Some(result);
                        }
                        let remained_pending = early_result.is_none();

                        // Always release before assertions so a failing
                        // observation cannot strand the root initializer.
                        parent_mcp_initialize_gate.send_replace(true);
                        let loaded = match early_result {
                            Some(result) => result,
                            None => load_future.await,
                        };
                        (
                            loaded,
                            initialize_reached,
                            remained_pending,
                            command_executor_responded,
                            snapshot_cancelled,
                        )
                    })
                    .await;
                assert!(
                    initialize_reached,
                    "parent MCP initialize must begin while cold load is pending"
                );
                assert!(
                    remained_pending,
                    "cold load must remain pending while parent MCP initialize is gated"
                );
                assert!(
                    command_executor_responded,
                    "root command executor must answer a pool snapshot while MCP initialize is gated"
                );
                assert!(snapshot_cancelled, "cancelling one initialized snapshot must finish without releasing the parent handshake gate");
                loaded
            } else {
                cold_recovery_stage("authority.cold_load", agent.load_session(load_request)).await
            };
            if phase == "plugin-fallback" {
                let error = format!(
                    "{:?}",
                    loaded.expect_err("a plugin must not replace the original native Agent")
                );
                assert!(error.contains("selected-all"), "{error}");
                assert!(error.contains("different plugin"), "{error}");
                assert!(
                    agent.control_session_handle(&child_id).is_none(),
                    "the mismatched child must not be installed"
                );
                assert_eq!(
                    sequence.load(std::sync::atomic::Ordering::SeqCst),
                    0,
                    "identity rejection must precede model sampling"
                );
                let root = agent
                    .control_session_handle(&root_id)
                    .expect("prepared root owner must remain resident");
                let runtime = root.chat_state_handle.runtime_state().await.unwrap();
                assert_eq!(
                    runtime.phase,
                    chat_state::session_runtime::RuntimePhase::Restoring
                );
                assert!(!runtime.resume_committed);
                assert_eq!(
                    runtime.operation_id.as_deref(),
                    Some(
                        std::fs::read_to_string(home.join("operation-id"))
                            .unwrap()
                            .as_str()
                    )
                );
                assert!(
                    runtime.instance_id.is_some(),
                    "the original prepared instance must be retained"
                );
                let events = root.chat_state_handle.timeline_events().await.unwrap();
                assert!(
                    !events.iter().any(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                            _
                        ))
                    )),
                    "identity rejection must not terminalize the original child"
                );
                assert!(
                    !events.iter().any(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Runtime(
                            chat_state::session_runtime::SessionRuntimeEvent::ExecutionReleased { .. }
                        )
                    )),
                    "the failed tree must not be released"
                );
                let (native, original) = replaced_native_profile.unwrap();
                std::fs::write(native, original).unwrap();
                server.abort();
                return;
            }
            if phase != "restore" {
                let error = format!(
                    "{:?}",
                    loaded.expect_err("invalid birth fact must block the tree")
                );
                if phase == "missing-mcp" {
                    assert!(error.contains("missing birth MCP membership"), "{error}");
                } else {
                    assert!(error.contains("birth capability_mode"), "{error}");
                    assert!(
                        error.contains(if phase == "missing-birth" {
                            "missing"
                        } else {
                            "invalid"
                        }),
                        "{error}"
                    );
                }
                assert!(
                    agent.control_session_handle(&child_id).is_none(),
                    "no child was installed"
                );
                assert_eq!(
                    sequence.load(std::sync::atomic::Ordering::SeqCst),
                    0,
                    "no node sampled before all birth facts were validated"
                );
                assert!(
                    agent.control_session_handle(&root_id).is_none(),
                    "failed cold-load root must retire before retry"
                );
                let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
                let events = storage
                    .read_timeline_events_sync(&crate::session::info::Info {
                        id: root_id.clone(),
                        cwd: cwd.to_str().unwrap().to_owned(),
                    })
                    .unwrap();
                let timeline = chat_state::Timeline::from_events(events.clone()).unwrap();
                assert_eq!(
                    timeline.runtime_lifecycle().phase,
                    chat_state::session_runtime::RuntimePhase::Suspended
                );
                assert!(!timeline.runtime_lifecycle().resume_committed);
                assert!(
                    !events.iter().any(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                            _
                        ))
                    )),
                    "admission failure must not terminalize the original child"
                );
                if phase == "missing-mcp" {
                    // Repair only after the failed temporary writer is gone,
                    // then retry through the SAME manager, not a new process.
                    std::fs::write(
                        &path_for_root(&home, &cwd, &root_id),
                        std::fs::read(home.join("original-timeline")).unwrap(),
                    )
                    .unwrap();
                    cold_recovery_stage(
                        "authority.same_process_retry",
                        agent.load_session(
                            acp::LoadSessionRequest::new(root_id.clone(), cwd.clone())
                                .mcp_servers(mcp_servers.clone()),
                        ),
                    )
                    .await
                    .unwrap();
                    let resumed = agent.control_session_handle(&root_id).unwrap();
                    let runtime = resumed.chat_state_handle.runtime_state().await.unwrap();
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
                    let original_wait =
                        cold_recovery_stage("authority.retry_original_wait", permission_rx.recv())
                            .await
                            .unwrap();
                    assert_eq!(
                        original_wait.request.tool_call.tool_call_id.0.as_ref(),
                        "call-original-1"
                    );
                    assert_eq!(
                        sequence.load(std::sync::atomic::Ordering::SeqCst),
                        0,
                        "retry must resume the original call, not resample its response"
                    );
                    cold_recovery_stage(
                        "authority.retry_resuspend",
                        agent.suspend_session_tree(
                            &root_id,
                            tokio::time::Instant::now() + std::time::Duration::from_secs(9),
                        ),
                    )
                    .await
                    .unwrap();
                    drop(original_wait);
                }
                server.abort();
                return;
            }
            loaded.unwrap();
            let root = agent.control_session_handle(&root_id).unwrap();
            let child = agent
                .control_session_handle(&child_id)
                .expect("same child identity restored");
            let backend =
                tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
                    agent.subagent_event_tx.clone(),
                    root_id.0.to_string(),
                );
            let inspection = backend.inspect(child_id.0.as_ref()).await.unwrap().unwrap();
            assert_eq!(inspection.child_session_id, child_id.0.as_ref());
            let signals = child.signals_handle.snapshot().await.unwrap();
            assert!(matches!(
                inspection.snapshot.status,
                tools::implementations::grow_build::task::types::SubagentSnapshotStatus::Running {
                    turn_count, tool_call_count, ..
                } if turn_count == signals.turn_count && tool_call_count == signals.tool_call_count
            ));
            assert_eq!(sequence.load(std::sync::atomic::Ordering::SeqCst), 0);
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
            wait_for_mcp_pool(&root, mcp_servers.len()).await;
            assert_mcp_membership(&child, &root, &available_birth, birth_case == "unavailable")
                .await;
            let expected_model = if birth_case == "changed-model" {
                "test/second"
            } else if birth_case == "alias" {
                "test/renamed"
            } else {
                "test/default"
            };
            assert_eq!(
                child.model_route.snapshot().model_id.0.as_ref(),
                expected_model,
                "resume uses the checkpoint's selected route, not its birth route"
            );
            let model_events = child.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                crate::session::persistence::latest_model_selection(&model_events)
                    .unwrap()
                    .unwrap()
                    .0
                    .0
                    .as_ref(),
                expected_model,
                "alias rebind is a durable model observation"
            );
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
            assert_mcp_membership(
                &grandchild,
                &root,
                &available_birth,
                birth_case == "unavailable",
            )
            .await;
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
            assert_eq!(
                spawns
                    .iter()
                    .find(|spawn| spawn.subagent_id == child_id.0.as_ref())
                    .unwrap()
                    .birth_mcp_servers
                    .as_ref(),
                Some(&expected_birth)
            );
            assert_eq!(
                spawns
                    .iter()
                    .find(|spawn| spawn.subagent_id == "authority-grandchild")
                    .unwrap()
                    .birth_mcp_servers
                    .as_ref(),
                Some(&available_birth)
            );
            if birth_case == "unavailable" {
                assert_eq!(
                    mcp_calls
                        .lock()
                        .unwrap()
                        .get("birth")
                        .copied()
                        .unwrap_or_default(),
                    0,
                    "a rejected initialization must not dispatch a tool call"
                );
            }
            assert_eq!(
                mcp_calls
                    .lock()
                    .unwrap()
                    .get("extra")
                    .copied()
                    .unwrap_or_default(),
                0,
                "unborn MCP server must never be invoked"
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

async fn wait_for_mcp_pool(
    handle: &crate::session::SessionHandle,
    count: usize,
) -> crate::session::mcp_servers::SharedMcpPool {
    cold_recovery_stage("authority.mcp_pool", async {
        loop {
            if let Some(pool) = handle.snapshot_mcp_pool().await
                && pool.len() == count
            {
                return pool;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
}

async fn assert_mcp_membership(
    child: &crate::session::SessionHandle,
    root: &crate::session::SessionHandle,
    expected: &std::collections::BTreeSet<String>,
    birth_initialize_failed: bool,
) {
    let child_pool = child.snapshot_mcp_pool().await;
    let actual: std::collections::BTreeSet<_> = child_pool
        .as_ref()
        .map(|pool| pool.server_names().map(str::to_owned).collect())
        .unwrap_or_default();
    assert_eq!(
        &actual, expected,
        "live child pool must retain only surviving birth members"
    );
    let parent_pool = root.snapshot_mcp_pool().await.unwrap();
    for name in ["birth", "extra"] {
        if let Some(client) = parent_pool.get_client(name) {
            assert_eq!(
                child
                    .delegable_capability_ceiling
                    .as_ref()
                    .unwrap()
                    .permits_mcp_binding(name, client.client_id()),
                expected.contains(name),
                "fresh incarnation binding {name}"
            );
        }
        let called = cold_recovery_stage(
            "authority.mcp_call",
            child.call_mcp_tool(name.into(), None, "probe".into(), serde_json::json!({})),
        )
        .await;
        if name == "birth" && birth_initialize_failed {
            assert!(
                called.is_err(),
                "failed discovery must not answer a tool call"
            );
        } else if expected.contains(name) {
            called.unwrap();
        } else {
            assert!(
                called.unwrap_err().contains("not found"),
                "unborn/unavailable member must not dispatch"
            );
        }
    }
}

fn path_for_root(
    home: &std::path::Path,
    cwd: &std::path::Path,
    root: &acp::SessionId,
) -> std::path::PathBuf {
    home.join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(
            cwd.to_str().unwrap(),
        ))
        .join(root.0.as_ref())
        .join("timeline.jsonl")
}
