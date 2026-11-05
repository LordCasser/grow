//! Native tool -> Shell owner -> actor -> next provider request.
use super::*;
use acp_transport::AcpAgentHandler as _;
use sampling_types::ReasoningEffort;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

fn model_transition_count(events: &[chat_state::TimelineEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(&event.kind,
                chat_state::TimelineEventKind::Observation(observation)
                    if observation.scope == "model" && observation.name == "changed"
            )
        })
        .count()
}

#[test]
fn subagent_model_owner_requires_exact_incarnation_and_actor_admission() {
    const CASE: &str = "GROW_TEST_SUBAGENT_MODEL_OWNER_CASE";
    let Ok(case) = std::env::var(CASE) else {
        for case in [
            "queued",
            "actor-rejected",
            "receipt-closed",
            "receiver-closed",
            "caller-cancelled",
            "target-cancelled",
            "target-stopped",
            "replaced",
            "replaced-during-lock",
            "cancelled-during-lock",
        ] {
            let home = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::session_model_tests::subagent_model_owner_requires_exact_incarnation_and_actor_admission", "--nocapture"])
                .env(CASE, case).env("GROW_HOME", home.path())
                .output().unwrap();
            assert!(
                output.status.success(),
                "{case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        use tools::implementations::grow_build::set_subagent_model::SubagentModelControlRequest;
        let agent = Box::new(build_minimal_agent_for_tests());
        let (mut target, mut commands) = make_test_handle_with_receiver("test/default", None);
        target.info.id = acp::SessionId::new("exact-controlled-child");
        target
            .runtime_observation
            .write()
            .replace_queue_running(Vec::new(), Some(("controlled-turn".into(), 1)));
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(target.info.id.clone(), target.clone());
        let caller_cancellation = CancellationToken::new();
        let target_cancellation = CancellationToken::new();
        let (respond_to, response) = oneshot::channel();
        let mut response = Some(response);
        if case == "receiver-closed" {
            drop(response.take());
        }
        if case == "caller-cancelled" {
            caller_cancellation.cancel();
        }
        if case == "target-cancelled" {
            target_cancellation.cancel();
        }
        if case == "target-stopped" {
            agent
                .active_child_sessions
                .borrow_mut()
                .remove(&target.info.id);
        }
        let mut replacement_commands = None;
        if case == "replaced" {
            let (mut replacement, receiver) = make_test_handle_with_receiver("test/default", None);
            replacement.info.id = target.info.id.clone();
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(target.info.id.clone(), replacement);
            replacement_commands = Some(receiver);
        }
        let transaction = agent.model_reload_lock.lock().await;
        agent.set_subagent_model(
            SubagentModelControlRequest {
                source_session_id: "authorized-parent".into(),
                subagent_id: target.info.id.0.to_string(),
                model: Some("test/default".into()),
                effort: None,
                cancellation: caller_cancellation.clone(),
                respond_to,
            },
            target.clone(),
            target_cancellation.clone(),
        );
        tokio::task::yield_now().await;
        assert!(
            commands.try_recv().is_err(),
            "catalog lock must precede enqueue"
        );
        if case == "replaced-during-lock" {
            let (mut replacement, receiver) = make_test_handle_with_receiver("test/default", None);
            replacement.info.id = target.info.id.clone();
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(target.info.id.clone(), replacement);
            replacement_commands = Some(receiver);
        }
        if case == "cancelled-during-lock" {
            target_cancellation.cancel();
        }
        drop(transaction);
        if matches!(
            case.as_str(),
            "queued" | "actor-rejected" | "receipt-closed"
        ) {
            let command = cold_recovery_stage("model-owner.enqueued", commands.recv())
                .await
                .unwrap();
            let SessionCommand::AdmitToolSampling {
                selection,
                admitted,
                responds_to,
            } = command
            else {
                panic!("expected sampling admission");
            };
            let crate::session::SessionSamplingSelection::Model { route, .. } = selection else {
                panic!("expected model route");
            };
            let response = response.as_mut().unwrap();
            assert!(
                futures::poll!(&mut *response).is_pending(),
                "mailbox send is not admission"
            );
            // Waiting for actor admission must release the global catalog lock.
            assert!(agent.model_reload_lock.try_lock().is_ok());
            if case == "receipt-closed" {
                drop(admitted);
            } else if case == "actor-rejected" {
                admitted
                    .send(Err("target execution is closed".into()))
                    .unwrap();
            } else {
                admitted.send(Ok(())).unwrap();
            }
            let result = cold_recovery_stage("model-owner.receipt", response)
                .await
                .unwrap();
            assert_eq!(result.is_ok(), case == "queued");
            if case == "actor-rejected" {
                assert_eq!(result.as_ref().unwrap_err(), "target execution is closed");
            }
            if case == "receipt-closed" {
                assert!(result.as_ref().unwrap_err().contains("before confirming"));
            }
            responds_to
                .send(Ok(crate::session::DesiredStateOutcome::Applied(
                    route.model_id,
                )))
                .unwrap();
        } else if let Some(response) = response {
            let error = cold_recovery_stage("model-owner.rejected", response)
                .await
                .unwrap()
                .unwrap_err();
            if case.starts_with("replaced") {
                assert!(error.contains("incarnation"), "{error}");
            }
            assert!(
                commands.try_recv().is_err(),
                "invalid target never receives a command"
            );
        } else {
            tokio::task::yield_now().await;
            assert!(
                commands.try_recv().is_err(),
                "closed receiver never enqueues"
            );
        }
        if let Some(mut receiver) = replacement_commands {
            assert!(
                receiver.try_recv().is_err(),
                "an old grant cannot redirect to a replacement actor"
            );
        }
    });
}

#[test]
fn native_subagent_model_tool_controls_the_child_without_waiting_for_its_step() {
    const CASE: &str = "GROW_TEST_SUBAGENT_MODEL_CASE";
    let Ok(case) = std::env::var(CASE) else {
        for case in [
            "both",
            "model",
            "effort",
            "none",
            "null",
            "pending",
            "rebuild",
            "unknown-model",
            "unselectable",
            "hidden",
            "unsupported",
            "effort-unsupported",
            "malformed",
            "empty",
            "forged",
            "denied",
            "permission-denied",
            "hook-denied",
            "finished-during-lock",
            "mailbox-model-after-finish",
            "mailbox-effort-after-finish",
        ] {
            let home = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::session_model_tests::native_subagent_model_tool_controls_the_child_without_waiting_for_its_step", "--nocapture"])
                .env(CASE, case).env("GROW_HOME", home.path())
                .env("RUST_MIN_STACK", "16777216")
                .output().unwrap();
            assert!(
                output.status.success(),
                "{case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let home = ::config::grow_home();
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        git2::Repository::init(&cwd).unwrap();
        std::fs::write(cwd.join("fixture.txt"), "child tool boundary").unwrap();
        std::fs::write(cwd.join(".grow/agents/model-child.md"), "---\nname: model-child\ndescription: Sampling control fixture\ncapabilityMode: all\ntools: [read_file]\nagentsMd: false\ndiscoverSkills: false\ninheritSkills: false\n---\nComplete the task.\n").unwrap();
        let parent_calls = Arc::new(AtomicUsize::new(0));
        let child_calls = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(std::sync::Mutex::new(
            Vec::<(String, serde_json::Value)>::new(),
        ));
        let blocked = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let pending = case == "pending";
        let natural_finish = matches!(
            case.as_str(),
            "finished-during-lock" | "mailbox-model-after-finish" | "mailbox-effort-after-finish"
        );
        let selection = match case.as_str() {
            "model" => json!({"model":"test/selected"}),
            "effort" => json!({"effort":"none"}),
            "none" => json!({"model":"test/selected","effort":"none"}),
            "null" => json!({"model":"test/selected","effort":null}),
            "pending" => json!({"effort":"high"}),
            "unknown-model" => json!({"model":"test/unknown"}),
            "unselectable" => json!({"model":"test/private"}),
            "hidden" => json!({"model":"test/hidden"}),
            "unsupported" => json!({"model":"test/selected","effort":"minimal"}),
            "effort-unsupported" => json!({"effort":"high"}),
            "malformed" => json!({"effort":"HIGH"}),
            "empty" => json!({"model":null,"effort":null}),
            "forged" => {
                json!({"model":"test/selected","session_id":"another-session","clientIntent":{"revision":9}})
            }
            _ => json!({"model":"test/selected","effort":"high"}),
        };
        let mut arguments = json!({"subagent_id":"controlled-child"});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(selection.as_object().unwrap().clone());
        let provider = axum::Router::new().route("/v1/chat/completions", axum::routing::post({
            let parent_calls = parent_calls.clone();
            let child_calls = child_calls.clone();
            let requests = requests.clone();
            let blocked = blocked.clone();
            let release = release.clone();
            move |axum::Json(body): axum::Json<serde_json::Value>| {
                let requests = requests.clone();
                let foreground = body["tools"].as_array().is_some_and(|tools| !tools.is_empty());
                let parent = body["model"] == "parent-wire";
                let first_parent = foreground && parent && parent_calls.fetch_add(1, Ordering::SeqCst) == 0;
                let first_child = foreground && !parent && child_calls.fetch_add(1, Ordering::SeqCst) == 0;
                let arguments = arguments.clone();
                let blocked = blocked.clone();
                let release = release.clone();
                async move {
                    if foreground { requests.lock().unwrap().push(("chat_completions".into(), body)); }
                    if first_child { blocked.notify_one(); release.notified().await; }
                    let tool = if first_parent {
                        Some(("subagent-model-call", "set_subagent_model", arguments))
                    } else if first_child && !natural_finish {
                        Some(("child-boundary-call", "read_file", json!({"target_file":"fixture.txt"})))
                    } else { None };
                    let delta = if let Some((id, name, arguments)) = &tool {
                        json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
                    } else { json!({"role":"assistant","content":"completed"}) };
                    let part = json!({"id":"model-tool-response","object":"chat.completion.chunk","created":1,"model":"default","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
                    let done = json!({"id":"model-tool-response","object":"chat.completion.chunk","created":1,"model":"default","choices":[{"index":0,"delta":{},"finish_reason":if tool.is_some() {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":10,"total_tokens":20}});
                    ([("content-type","text/event-stream")], format!("data: {part}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
                }
            }
        })).route("/v1/responses", axum::routing::post({
            let requests = requests.clone();
            move |axum::Json(body): axum::Json<serde_json::Value>| {
                requests.lock().unwrap().push(("responses".into(), body));
                async move {
                    let body = test_support::sse::responses_api_script_exact("completed", "selected-wire")
                        .into_iter().map(|event| format!("data: {}\n\n", event.data)).collect::<String>();
                    ([("content-type","text/event-stream")], body)
                }
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
allowed_models = ["test/parent", "test/default", "test/selected", "test/hidden"]
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://{address}/v1"
api_key = "fixture-only"
[provider.test.models.parent]
model = "parent-wire"
context_window = 128000
reasoning_efforts = [{{value="low",default=true}}]
[provider.test.models.default]
model = "child-wire"
context_window = 128000
reasoning_efforts = [{{value="low",default=true}},{{value="none"}}]
[provider.test.models.selected]
model = "selected-wire"
api_backend = "responses"
context_window = 128000
reasoning_efforts = [{{value="max",default=true}},{{value="high"}},{{value="none"}}]
[provider.test.models.private]
context_window = 128000
[provider.test.models.hidden]
hidden = true
context_window = 128000
"#
        );
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        cfg.default_permission_mode = crate::util::config::PermissionMode::AlwaysApprove;
        if case == "permission-denied" {
            cfg.cli_agent_overrides.permission_rules.push(
                workspace::permission::types::PermissionRule {
                    action: workspace::permission::types::RuleAction::Deny,
                    tool: workspace::permission::types::ToolFilter::Any,
                    pattern: Some("set_subagent_model".into()),
                    pattern_mode: Default::default(),
                },
            );
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(rx);
        let agent = Rc::new(MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap());
        cold_recovery_stage("model-tool.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1)
            .meta(json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let mut profile = json!({"name":"model-control-fixture","description":"model tool fixture","tools":["set_subagent_model","read_file","search_replace","run_terminal_command"],"agentsMd":false,"discoverSkills":false,"injectDefaultTools":false});
        if case == "denied" {
            profile["disallowedTools"] = json!(["set_subagent_model"]);
        }
        if case == "hook-denied" {
            profile["hooks"] = json!({"pre_tool_use":[{"matcher":"set_subagent_model","hooks":[{"type":"command","command":"printf '%s' '{\"decision\":\"deny\",\"reason\":\"fixture denies sampling change\"}'"}]}]});
        }
        let session_id = cold_recovery_stage(
            "model-tool.new",
            agent.new_session(
                acp::NewSessionRequest::new(cwd.clone())
                    .meta(json!({"agentProfile":profile}).as_object().cloned()),
            ),
        )
        .await
        .unwrap()
        .session_id;
        let handle = agent.control_session_handle(&session_id).unwrap();
        let parent_initial = handle.model_route.snapshot();
        let parent_initial_transitions =
            model_transition_count(&handle.chat_state_handle.timeline_events().await.unwrap());
        if case == "rebuild" {
            let mut definition = agent::AgentDefinition::from_json(&profile).unwrap();
            definition.name = "rebuilt-model-control-fixture".into();
            let (responds_to, applied) = tokio::sync::oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::RebuildAgentForDefinition {
                    definition,
                    intent: None,
                    responds_to,
                })
                .unwrap();
            cold_recovery_stage("model-tool.rebuild", applied)
                .await
                .unwrap()
                .unwrap();
        }
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentOwner, SubagentRequest, SubagentRuntimeOverrides,
            SubagentSpawnRequest,
        };
        let child_id = acp::SessionId::new("controlled-child");
        // Hold the existing parent-usage acknowledgment only in the natural
        // completion race. This retains the live exact child during real
        // finalization, after its Turn has already returned.
        let (held_usage_tx, mut held_usage_rx) = mpsc::unbounded_channel();
        let proxy_forwarder = if natural_finish {
            let (proxy_tx, mut proxy_rx) = mpsc::unbounded_channel();
            agent
                .sessions
                .borrow_mut()
                .get_mut(&session_id)
                .unwrap()
                .cmd_tx = proxy_tx;
            let parent_commands = handle.cmd_tx.clone();
            Some(tokio::task::spawn_local(async move {
                while let Some(command) = proxy_rx.recv().await {
                    if matches!(&command, SessionCommand::RecordSubagentUsage { subagent_id, .. } if subagent_id == "controlled-child")
                    {
                        held_usage_tx.send(command).unwrap();
                    } else {
                        let _ = parent_commands.send(command);
                    }
                }
            }))
        } else {
            None
        };
        let (result_tx, child_result) = oneshot::channel();
        agent
            .subagent_event_tx
            .send(SubagentEvent::Spawn(SubagentSpawnRequest {
                request: Box::new(SubagentRequest {
                    id: child_id.0.to_string(),
                    origin_tool_call_id: None,
                    prompt: "Read fixture.txt and finish.".into(),
                    description: "controlled model fixture".into(),
                    subagent_type: "model-child".into(),
                    parent_session_id: session_id.0.to_string(),
                    parent_step_id: None,
                    parent_prompt_id: None,
                    resume_from: None,
                    cwd: None,
                    runtime_overrides: SubagentRuntimeOverrides {
                        model: Some("test/default".into()),
                        ..Default::default()
                    },
                    run_in_background: false,
                    surface_completion: false,
                    await_to_completion: true,
                    fork_context: false,
                    owner: SubagentOwner::Task,
                    goal_context: None,
                    cancel_token: CancellationToken::new(),
                }),
                result_tx,
            }))
            .unwrap();
        cold_recovery_stage("model-tool.child-provider-blocked", blocked.notified()).await;
        let child = agent
            .active_child_sessions
            .borrow()
            .get(&child_id)
            .unwrap()
            .clone();
        let child_initial = child.model_route.snapshot();
        let initial_transitions =
            model_transition_count(&child.chat_state_handle.timeline_events().await.unwrap());
        let birth = handle
            .chat_state_handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == child_id.0.as_ref() => {
                    Some(serde_json::to_value(spawn).unwrap())
                }
                _ => None,
            })
            .expect("real coordinator must persist the child birth");
        if natural_finish {
            use tools::implementations::grow_build::task::backend::{
                ChannelBackend, SubagentBackend,
            };
            agent
                .sessions
                .borrow_mut()
                .get_mut(&session_id)
                .unwrap()
                .cmd_tx = handle.cmd_tx.clone();
            let transaction = agent.model_reload_lock.lock().await;
            let backend = ChannelBackend::for_session(
                agent.subagent_event_tx.clone(),
                session_id.0.to_string(),
            );
            let control = backend.set_model(
                child_id.0.to_string(),
                Some("test/selected".into()),
                Some(ReasoningEffort::High),
                CancellationToken::new(),
            );
            tokio::pin!(control);
            let mut delayed_control = None;
            let mut delayed_admission = None;
            let mut delayed_application = None;
            if case == "finished-during-lock" {
                let waiting = super::super::session_model::observe_next_catalog_wait();
                assert!(futures::poll!(&mut control).is_pending());
                cold_recovery_stage("model-race.shell-awaiting-catalog", waiting)
                    .await
                    .unwrap();
                assert!(
                    futures::poll!(&mut control).is_pending(),
                    "the exact runner is waiting for the held catalog lock"
                );
            } else {
                // Freeze the production handler's command in a test mailbox
                // while the child is live. Deliver the exact prepared command
                // only after natural completion, exercising actor admission
                // independently of the earlier Shell lifecycle check.
                let (mailbox, mut commands) = mpsc::unbounded_channel();
                let mut delayed_target = child.clone();
                delayed_target.cmd_tx = mailbox;
                let (admitted, receipt) = oneshot::channel();
                let effort_only = case == "mailbox-effort-after-finish";
                delayed_application = Some(
                    crate::agent::handlers::model_switch::enqueue_tool(
                        &agent,
                        &transaction,
                        delayed_target,
                        (!effort_only).then(|| "test/selected".into()),
                        Some(if effort_only {
                            ReasoningEffort::None
                        } else {
                            ReasoningEffort::High
                        }),
                        admitted,
                    )
                    .unwrap(),
                );
                delayed_admission = Some(receipt);
                delayed_control = Some(commands.recv().await.unwrap());
            }
            release.notify_one();
            let usage = cold_recovery_stage("model-race.real-child-finished", held_usage_rx.recv())
                .await
                .unwrap();
            assert!(matches!(
                child.runtime_observation.read().lifecycle_status(),
                crate::session::prompt_queue::PromptStatus::Settling
                    | crate::session::prompt_queue::PromptStatus::Terminal { .. }
            ));
            assert!(
                agent
                    .active_child_sessions
                    .borrow()
                    .get(&child_id)
                    .unwrap()
                    .cmd_tx
                    .same_channel(&child.cmd_tx)
            );
            assert!(
                !child.cmd_tx.is_closed(),
                "finalization keeps the exact actor alive"
            );
            drop(transaction);
            let error = if let Some(command) = delayed_control {
                child.cmd_tx.send(command).unwrap();
                let error =
                    cold_recovery_stage("model-race.actor-rejected", delayed_admission.unwrap())
                        .await
                        .unwrap()
                        .unwrap_err();
                assert!(
                    crate::agent::handlers::model_switch::finish(
                        &agent,
                        delayed_application.unwrap()
                    )
                    .await
                    .is_err()
                );
                error
            } else {
                cold_recovery_stage("model-race.rejected", control)
                    .await
                    .unwrap_err()
            };
            assert!(error.contains("execution is no longer open"), "{error}");
            assert_eq!(
                model_transition_count(&child.chat_state_handle.timeline_events().await.unwrap()),
                initial_transitions,
                "a finished target cannot admit a new Sampling control"
            );
            assert_eq!(
                requests.lock().unwrap().len(),
                1,
                "natural completion has one unchanged provider attempt"
            );
            handle.cmd_tx.send(usage).unwrap();
            assert!(
                cold_recovery_stage("model-race.child-finalization", child_result)
                    .await
                    .unwrap()
                    .success
            );
            assert!(agent.close_session_explicit(&session_id).await.unwrap());
            proxy_forwarder.unwrap().abort();
            server.abort();
            return;
        }
        let mut user_change = None;
        if pending {
            let catalog_transaction = agent.model_reload_lock.lock().await;
            user_change = Some(
                crate::agent::handlers::model_switch::enqueue(
                    &agent,
                    &catalog_transaction,
                    child.clone(),
                    crate::agent::handlers::model_switch::ModelSwitchRequest::new(
                        child_id.clone(),
                        crate::agent::models::ModelId::new("test/selected"),
                    ),
                )
                .unwrap(),
            );
            drop(catalog_transaction);
            let (responds_to, barrier) = oneshot::channel();
            child
                .cmd_tx
                .send(SessionCommand::GetCurrentModel { responds_to })
                .unwrap();
            cold_recovery_stage("model-tool.user-admitted", barrier)
                .await
                .unwrap();
        }
        cold_recovery_stage(
            "model-tool.parent-turn",
            agent.prompt(acp::PromptRequest::new(
                session_id.clone(),
                vec![acp::ContentBlock::Text(acp::TextContent::new(
                    "Control the running child once.",
                ))],
            )),
        )
        .await
        .unwrap();
        assert_eq!(
            child_calls.load(Ordering::SeqCst),
            1,
            "queued never restarts the current child attempt"
        );
        let still_running = child.model_route.snapshot();
        assert_eq!(still_running.model_id, child_initial.model_id);
        assert_eq!(
            still_running.sampling_config.reasoning_effort,
            child_initial.sampling_config.reasoning_effort
        );
        let success = matches!(
            case.as_str(),
            "both" | "model" | "effort" | "none" | "null" | "pending" | "rebuild"
        );
        let parent_requests: Vec<_> = requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, body)| body["model"] == "parent-wire")
            .map(|(_, body)| body.clone())
            .collect();
        assert_eq!(
            parent_requests.len(),
            2,
            "one parent control exchange followed by an answer"
        );
        let parent_next = &parent_requests[1];
        let result = parent_next["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| {
                message["role"] == "tool" && message["tool_call_id"] == "subagent-model-call"
            })
            .and_then(|message| message["content"].as_str())
            .unwrap();
        assert_eq!(
            result.contains("queued"),
            success,
            "queued is evidence of target admission only: {result}"
        );
        if success {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(result).unwrap(),
                json!({"subagent_id":"controlled-child","status":"queued"})
            );
        }
        assert_eq!(
            parent_next
                .to_string()
                .matches("subagent-model-call")
                .count(),
            2,
            "parent call and receipt each occur once"
        );
        release.notify_one();
        assert!(
            cold_recovery_stage("model-tool.child-result", child_result)
                .await
                .unwrap()
                .success
        );
        if let Some(change) = user_change {
            let result = crate::agent::handlers::model_switch::finish(&agent, change)
                .await
                .unwrap();
            assert_eq!(
                result.meta.unwrap()["status"],
                "superseded",
                "effort patch retains the pending user model"
            );
        }
        let route = child.model_route.snapshot();
        let expected_model = if success && case != "effort" {
            "test/selected"
        } else {
            "test/default"
        };
        let expected_effort = if !success {
            ReasoningEffort::Low
        } else {
            match case.as_str() {
                "model" | "null" => ReasoningEffort::Max,
                "none" | "effort" => ReasoningEffort::None,
                _ => ReasoningEffort::High,
            }
        };
        assert_eq!(route.model_id.0.as_ref(), expected_model);
        assert_eq!(
            route.sampling_config.reasoning_effort,
            Some(expected_effort)
        );
        let parent_final = handle.model_route.snapshot();
        assert_eq!(parent_final.model_id, parent_initial.model_id);
        assert_eq!(
            parent_final.sampling_config.model,
            parent_initial.sampling_config.model
        );
        assert_eq!(
            parent_final.sampling_config.reasoning_effort,
            parent_initial.sampling_config.reasoning_effort
        );
        assert_eq!(
            agent.models_manager.current_model_id().0.as_ref(),
            "test/parent"
        );
        assert!(
            parent_requests
                .iter()
                .all(|body| body["model"] == "parent-wire" && body["reasoning_effort"] == "low")
        );
        let requests = requests.lock().unwrap().clone();
        let child_requests: Vec<_> = requests
            .iter()
            .filter(|(_, body)| body["model"] != "parent-wire")
            .collect();
        assert_eq!(
            child_requests.len(),
            2,
            "one original attempt, one next Step: {requests:?}"
        );
        assert_eq!(child_requests[0].0, "chat_completions");
        assert_eq!(child_requests[0].1["model"], "child-wire");
        assert_eq!(child_requests[0].1["reasoning_effort"], "low");
        let (endpoint, next) = child_requests[1];
        assert_eq!(next["model"], route.sampling_config.model);
        if success && case != "effort" {
            assert_eq!(endpoint, "responses");
            assert_eq!(next["reasoning"]["effort"], expected_effort.to_string());
        } else {
            assert_eq!(endpoint, "chat_completions");
            assert_eq!(next["reasoning_effort"], expected_effort.to_string());
        }
        assert_eq!(
            next.to_string().matches("child-boundary-call").count(),
            2,
            "child call and result survive the route switch exactly once"
        );
        assert!(
            !next.to_string().contains("subagent-model-call"),
            "parent control exchange does not enter child history"
        );
        let events = handle.chat_state_handle.timeline_events().await.unwrap();
        if case == "hook-denied" {
            let (occurrence_id, handlers) = events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        occurrence_id,
                        event: chat_state::HookEventType::PreToolUse,
                        cause: chat_state::HookCause::Tool { call_id },
                        handlers,
                        ..
                    }) if call_id == "subagent-model-call" => Some((occurrence_id, handlers)),
                    _ => None,
                })
                .expect("the actual model-tool call must trigger the native PreToolUse gate");
            assert_eq!(handlers.len(), 1, "the Agent hook must be installed");
            let handler = &handlers[0];
            assert_eq!(handler.provenance, chat_state::HookHandlerProvenance::Agent);
            assert_eq!(handler.kind, chat_state::HookHandlerKind::Command);
            assert_eq!(handler.action, chat_state::HookHandlerPlanAction::Execute);
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::RunFinished {
                        occurrence_id: finished_occurrence,
                        run_id,
                        outcome: chat_state::HookRunOutcome::Blocked,
                        control: chat_state::HookRunControl::Block { reason },
                        ..
                    }) if finished_occurrence == occurrence_id && run_id == &handler.run_id
                        && reason == "fixture denies sampling change"
                )),
                "the native command must execute and return the fixture deny decision"
            );
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id: completed_occurrence,
                        decision: chat_state::HookAggregateDecision::Tool {
                            decision: chat_state::HookGateDecision::Block { reason },
                        },
                    }) if completed_occurrence == occurrence_id
                        && reason == "fixture denies sampling change"
                )),
                "the tool gate must durably complete with that same deny decision"
            );
        }
        assert_eq!(events.iter().filter(|event| matches!(&event.kind, chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {call_id,..}) if call_id == "subagent-model-call")).count(), 1);
        assert_eq!(
            model_transition_count(&events),
            parent_initial_transitions,
            "parent sampling has no new durable selection"
        );
        let unchanged_birth = events
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == child_id.0.as_ref() => {
                    Some(serde_json::to_value(spawn).unwrap())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            unchanged_birth, birth,
            "runtime control cannot rewrite birth authority"
        );
        // A completed child retires its ChatState owner. Read the durable
        // Timeline through storage rather than a closed runtime handle.
        let child_events =
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(home.clone())
                .read_timeline_events_sync(&child.info)
                .unwrap();
        let durable = crate::session::persistence::latest_model_selection(&child_events)
            .unwrap()
            .unwrap();
        assert_eq!(durable.0.0.as_ref(), expected_model);
        assert_eq!(durable.1, Some(expected_effort));
        assert_eq!(
            model_transition_count(&child_events),
            initial_transitions + usize::from(success)
        );
        assert!(agent.close_session_explicit(&session_id).await.unwrap());
        let stored = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(home.clone())
            .read_timeline_events_sync(&child.info)
            .unwrap();
        let persisted = crate::session::persistence::latest_model_selection(&stored)
            .unwrap()
            .unwrap();
        assert_eq!(
            persisted, durable,
            "completed child retains the exact durable final selection"
        );
        server.abort();
    });
}
