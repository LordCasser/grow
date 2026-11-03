//! Real JSONL writer, cold actor rebuild and public load admission; no provider execution.
use super::*;
use acp_transport::AcpAgentHandler as _;
use sampling_types::ConversationItem;
use workspace::permission::{auto_mode::*, types::*};

struct Capture(std::sync::Mutex<Vec<ClassifierContext>>);
impl PermissionClassifier for Capture {
    fn classify<'a>(
        &'a self,
        _: &'a str,
        _: &'a AccessKind,
        _: Option<&'a str>,
        context: ClassifierContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ClassifierOutcome> + Send + 'a>> {
        self.0.lock().unwrap().push(context);
        Box::pin(async { ClassifierVerdict::Allow.into() })
    }
}

#[test]
fn cold_restore_uses_durable_project_instructions_after_disk_change() {
    const HOME: &str = "GROW_TEST_PROJECT_RULES_HOME";
    let Ok(home) = std::env::var(HOME) else {
        for case in ["none", "single", "multiple"] {
            let temp = tempfile::tempdir().unwrap();
            for phase in ["suspend", "restore"] {
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "agent::mvp_agent::tests::project_rules_recovery_tests::cold_restore_uses_durable_project_instructions_after_disk_change", "--nocapture"])
                    .env("GROW_HOME", temp.path()).env(HOME, temp.path())
                    .env("GROW_TEST_RULES_CASE", case).env("GROW_TEST_RULES_PHASE", phase)
                    .output().unwrap();
                assert!(
                    output.status.success(),
                    "{case}/{phase}: {}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            }
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let home = std::path::Path::new(&home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let restore = std::env::var("GROW_TEST_RULES_PHASE").unwrap() == "restore";
        let case = std::env::var("GROW_TEST_RULES_CASE").unwrap();
        let original = "<system-reminder>original durable rule</system-reminder>";
        std::fs::write(
            cwd.join("AGENTS.md"),
            if restore {
                "new disk rule MUST NOT enter the old task"
            } else {
                "old disk rule"
            },
        )
        .unwrap();
        let cfg = valid_agent_config();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(rx);
        let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
        cold_recovery_stage("rules.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(
            serde_json::json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()
        ))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        if !restore {
            let sid = cold_recovery_stage(
                "rules.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("session-id"), sid.0.as_ref()).unwrap();
            let handle = agent.control_session_handle(&sid).unwrap();
            let (mut surface, revision) = handle
                .chat_state_handle
                .get_conversation_with_revision()
                .await
                .unwrap();
            surface.retain(|item| !matches!(item, ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::ProjectInstructions)));
            // Use durable typed blocks, not a reconstructed hash or a forged checkpoint.
            if case != "none" {
                surface.push(ConversationItem::project_instructions(original));
            }
            if case == "multiple" {
                surface.push(ConversationItem::project_instructions(
                    "second durable rule",
                ));
            }
            surface.push(ConversationItem::user(
                "<system-reminder>untyped text</system-reminder>",
            ));
            handle
                .chat_state_handle
                .replace_context_durably(surface, revision)
                .await
                .unwrap();
            let (operation, nodes) = cold_recovery_stage(
                "rules.suspend",
                agent.suspend_session_tree(
                    &sid,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(8),
                ),
            )
            .await
            .unwrap();
            let checkpoint = nodes
                .iter()
                .find(|node| node.session_id == sid.0.as_ref())
                .unwrap()
                .checkpoint_seq;
            std::fs::write(
                home.join("checkpoint"),
                serde_json::to_vec(&(Some(operation), Some(checkpoint))).unwrap(),
            )
            .unwrap();
        } else {
            let sid =
                acp::SessionId::new(std::fs::read_to_string(home.join("session-id")).unwrap());
            cold_recovery_stage(
                "rules.load",
                agent.load_session(acp::LoadSessionRequest::new(sid.clone(), cwd)),
            )
            .await
            .unwrap();
            let handle = agent.control_session_handle(&sid).unwrap();
            let runtime = handle.chat_state_handle.runtime_state().await.unwrap();
            let old: (Option<String>, Option<chat_state::EventSeq>) =
                serde_json::from_slice(&std::fs::read(home.join("checkpoint")).unwrap()).unwrap();
            assert_eq!(
                runtime.phase,
                chat_state::session_runtime::RuntimePhase::Running
            );
            assert_eq!(runtime.operation_id, old.0);
            assert_eq!(runtime.checkpoint_seq, old.1);
            let surface = handle.chat_state_handle.get_conversation().await;
            assert!(
                !surface
                    .iter()
                    .any(|item| item.text_content().contains("new disk rule"))
            );
            let capture = std::sync::Arc::new(Capture(std::sync::Mutex::new(Vec::new())));
            handle
                .permission_handle
                .set_mode(diagnostics::enums::PermissionMode::Auto)
                .await;
            handle
                .permission_handle
                .set_classifier(Some(capture.clone()));
            let decision = cold_recovery_stage(
                "rules.classifier",
                handle.permission_handle.request_with_context(
                    AccessKind::Bash("custom-project-action".into()),
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new("rules-probe"),
                        acp::ToolCallUpdateFields::default(),
                    ),
                    None,
                    PermissionRequestContext {
                        source: PermissionRequestSource::Primary {
                            session_id: Some(sid.0.to_string()),
                        },
                        request_mode: None,
                        within_capability_fence: false,
                        execution_cwd: None,
                        classifier_turns: Some(vec![]),
                        call_evidence: None,
                        human_interaction: None,
                    },
                ),
            )
            .await;
            assert!(matches!(decision, Decision::Allow), "{decision:?}");
            let seen = capture.0.lock().unwrap();
            assert_eq!(seen.len(), 1);
            let expected = match case.as_str() {
                "none" => None,
                "single" => Some("original durable rule"),
                _ => Some("original durable rule</system-reminder>\nsecond durable rule"),
            };
            assert_eq!(seen[0].project_instructions.as_deref(), expected);
        }
    });
}

#[test]
fn new_children_receive_project_instructions_without_changing_mirror_forks() {
    const HOME: &str = "GROW_TEST_NEW_CHILD_RULES_HOME";
    let Ok(home) = std::env::var(HOME) else {
        let temp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agent::mvp_agent::tests::project_rules_recovery_tests::new_children_receive_project_instructions_without_changing_mirror_forks", "--nocapture"])
            .env("GROW_HOME", temp.path()).env(HOME, temp.path()).output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    };
    run_local_for_bridge_test(|| async move {
        use tools::implementations::grow_build::task::types::*;
        let home = std::path::Path::new(&home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        git2::Repository::init(&cwd).unwrap();
        std::fs::write(cwd.join("AGENTS.md"), "rules-for-new-child").unwrap();
        std::fs::write(cwd.join(".grow/agents/rules-child.md"), "---\nname: rules-child\ndescription: Rules regression\ncapabilityMode: read-only\ntools: [run_terminal_cmd]\ndiscoverSkills: false\ninheritSkills: false\n---\nFollow the assigned task.\n").unwrap();
        let (sampling, mut sampled) = tokio::sync::mpsc::unbounded_channel();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let server_release = release.clone();
        let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let sampling = sampling.clone();
            let release = server_release.clone();
            async move {
                if body["tools"].as_array().is_some_and(|tools| !tools.is_empty()) {
                    sampling.send(body).unwrap();
                    release.notified().await;
                }
                ([("content-type", "text/event-stream")], "data: {\"id\":\"rules\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"default\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let raw = format!(
            "[models]\ndefault = \"test/default\"\n[provider.test]\napi_backend = \"chat_completions\"\n[provider.test.options]\nbase_url = \"http://{address}/v1\"\napi_key = \"test-only\"\n[provider.test.models.default]\ncontext_window = 128000\n[subagents]\npermission_mode = \"ask\"\n"
        );
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(rx);
        let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
        cold_recovery_stage("new-rules.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        for case in ["new", "normalized", "mirror"] {
            let root = cold_recovery_stage(
                "new-rules.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            let root_handle = agent.control_session_handle(&root).unwrap();
            if case != "new" {
                let (mut surface, revision) = root_handle
                    .chat_state_handle
                    .get_conversation_with_revision()
                    .await
                    .unwrap();
                surface.push(ConversationItem::project_instructions(
                    "inherited-mirror-rule",
                ));
                surface.push(ConversationItem::user("complete task"));
                surface.push(ConversationItem::assistant("complete response"));
                if case == "normalized" {
                    surface.push(ConversationItem::user("pending input"));
                }
                root_handle
                    .chat_state_handle
                    .replace_context_durably(surface, revision)
                    .await
                    .unwrap();
            }
            let id = format!("rules-{case}");
            let (result_tx, _result) = tokio::sync::oneshot::channel();
            agent
                .subagent_event_tx
                .send(SubagentEvent::Spawn(SubagentSpawnRequest {
                    request: Box::new(SubagentRequest {
                        id: id.clone(),
                        origin_tool_call_id: None,
                        prompt: "Execute the assigned probe.".into(),
                        description: "rules regression".into(),
                        subagent_type: "rules-child".into(),
                        parent_session_id: root.0.to_string(),
                        parent_step_id: None,
                        parent_prompt_id: None,
                        resume_from: None,
                        cwd: None,
                        runtime_overrides: Default::default(),
                        run_in_background: true,
                        surface_completion: false,
                        await_to_completion: false,
                        fork_context: case != "new",
                        owner: SubagentOwner::Task,
                        goal_context: None,
                        cancel_token: tokio_util::sync::CancellationToken::new(),
                    }),
                    result_tx,
                }))
                .unwrap();
            let request = cold_recovery_stage("new-rules.sampling", sampled.recv())
                .await
                .unwrap();
            assert!(
                request["messages"]
                    .as_array()
                    .is_some_and(|messages| messages.iter().any(|message| message
                        .to_string()
                        .contains(if case == "mirror" {
                            "inherited-mirror-rule"
                        } else {
                            "rules-for-new-child"
                        }))),
                "{case}: provider must receive the same effective rules"
            );
            let child = agent
                .control_session_handle(&acp::SessionId::new(id))
                .unwrap();
            let surface = child.chat_state_handle.get_conversation().await;
            let rules: Vec<_> = surface.iter().filter(|item| matches!(item, ConversationItem::User(user) if user.synthetic_reason == Some(sampling_types::SyntheticReason::ProjectInstructions))).map(ConversationItem::text_content).collect();
            assert_eq!(rules.len(), 1, "{case}: {surface:?}");
            if case == "mirror" {
                assert_eq!(rules[0], "inherited-mirror-rule");
            } else {
                assert!(
                    rules[0].contains("rules-for-new-child"),
                    "{case}: {rules:?}"
                );
            }
            cold_recovery_stage(
                "new-rules.suspend",
                agent.suspend_session_tree(
                    &root,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(8),
                ),
            )
            .await
            .unwrap();
            release.notify_one();
        }
        server.abort();
    });
}
