use super::*;
use acp_transport::AcpAgentHandler as _;

#[test]
#[cfg(unix)]
fn cold_restore_uses_execution_dependencies_and_preserves_effective_role() {
    const HOME: &str = "GROW_TEST_DEPENDENCY_ADMISSION_HOME";
    let Ok(home) = std::env::var(HOME) else {
        let temp = tempfile::tempdir().unwrap();
        for phase in [
            "suspend",
            "different-directory",
            "wrong-route",
            "missing-agent",
            "restore",
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::dependency_admission_tests::cold_restore_uses_execution_dependencies_and_preserves_effective_role", "--nocapture"])
                .env("GROW_HOME", temp.path()).env(HOME, temp.path()).env("GROW_TEST_DEPENDENCY_PHASE", phase)
                .output().unwrap();
            assert!(
                output.status.success(),
                "{phase}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let home = std::path::Path::new(&home);
        let phase = std::env::var("GROW_TEST_DEPENDENCY_PHASE").unwrap();
        let fresh = phase == "suspend";
        let cwd = home.join("workspace");
        std::fs::create_dir_all(cwd.join(".grow/agents")).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let profile = cwd.join(".grow/agents/recoverable.md");
        let (description, color, turns, role) = if fresh {
            ("original description", "red", 12, "ORIGINAL_EFFECTIVE_ROLE")
        } else {
            ("changed presentation", "blue", 8, "NEW_FILE_ROLE")
        };
        let hooks = if fresh {
            ""
        } else {
            "hooks:\n  PreToolUse:\n    - matcher: never_matches\n      hooks:\n        - type: command\n          command: exit 0\n"
        };
        std::fs::write(&profile, format!("---\nname: recoverable\ndescription: {description}\ncolor: {color}\nmaxTurns: {turns}\n{hooks}---\n{role}\n")).unwrap();
        std::fs::write(
            cwd.join("AGENTS.md"),
            if fresh {
                "ORIGINAL_PROJECT_RULE"
            } else {
                "UPDATED_DISK_RULE"
            },
        )
        .unwrap();
        if phase == "missing-agent" {
            std::fs::remove_file(&profile).unwrap();
        }
        let requests = Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let seen = requests.clone();
        let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
            let seen = seen.clone();
            async move {
                let mut requests = seen.lock().unwrap();
                let id = requests.len();
                requests.push(body);
                ([("content-type", "text/event-stream")], format!("data: {{\"id\":\"dependency-{id}\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"actual-route\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"done\"}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"))
            }
        }));
        let bind = if fresh {
            "127.0.0.1:0".into()
        } else {
            std::fs::read_to_string(home.join("provider-address")).unwrap()
        };
        let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
        let address = listener.local_addr().unwrap();
        if fresh {
            std::fs::write(home.join("provider-address"), address.to_string()).unwrap();
        }
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let catalog_key = if phase == "restore" {
            "renamed"
        } else {
            "original"
        };
        let endpoint = if phase == "wrong-route" {
            "http://127.0.0.1:1/v1".to_owned()
        } else {
            format!("http://{address}/v1")
        };
        let mut raw = format!(
            "[models]\ndefault = \"test/{catalog_key}\"\n[provider.test]\napi_backend = \"chat_completions\"\n[provider.test.options]\nbase_url = \"{endpoint}\"\napi_key = \"test-only\"\n[provider.test.models.{catalog_key}]\nmodel = \"actual-route\"\ncontext_window = 128000\n"
        );
        if phase == "restore" {
            // The old key still exists but now names another wire model. Only
            // the renamed alias retains the checkpoint's actual destination.
            raw.push_str("[provider.test.models.original]\nmodel = \"different-model\"\ncontext_window = 128000\n");
        }
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        if fresh {
            cfg.agent_profile_path = Some(profile.clone());
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(rx);
        let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
        cold_recovery_stage("dependencies.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(
            serde_json::json!({"startupHints":{"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()
        ))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        if fresh {
            let id = cold_recovery_stage(
                "dependencies.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("session-id"), id.0.as_ref()).unwrap();
            cold_recovery_stage(
                "dependencies.prompt",
                agent.prompt(acp::PromptRequest::new(
                    id.clone(),
                    vec![acp::ContentBlock::from("--initial-wire--")],
                )),
            )
            .await
            .unwrap();
            assert_wire(&requests, "--initial-wire--", "ORIGINAL_EFFECTIVE_ROLE");
            let (operation, nodes) = cold_recovery_stage(
                "dependencies.suspend",
                agent.suspend_session_tree(
                    &id,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(8),
                ),
            )
            .await
            .unwrap();
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
            let events = storage
                .read_timeline_events_sync(&crate::session::info::Info {
                    id,
                    cwd: cwd.to_str().unwrap().into(),
                })
                .unwrap();
            let timeline = chat_state::Timeline::from_events(events).unwrap();
            let source = &timeline
                .runtime_lifecycle()
                .checkpoint
                .as_ref()
                .unwrap()
                .runtime_source_hash;
            std::fs::write(
                home.join("original-checkpoint"),
                serde_json::to_vec(&(operation, nodes[0].checkpoint_seq, source)).unwrap(),
            )
            .unwrap();
        } else {
            let id = acp::SessionId::new(std::fs::read_to_string(home.join("session-id")).unwrap());
            let target = if phase == "different-directory" {
                let other = home.join("other-workspace");
                std::fs::create_dir_all(&other).unwrap();
                other
            } else if phase == "restore" {
                let alias = home.join("workspace-alias");
                std::os::unix::fs::symlink(&cwd, &alias).unwrap();
                alias
            } else {
                cwd.clone()
            };
            let loaded = cold_recovery_stage(
                "dependencies.load",
                agent.load_session(acp::LoadSessionRequest::new(id.clone(), target)),
            )
            .await;
            if phase != "restore" {
                let error = format!("{:?}", loaded.unwrap_err());
                assert!(
                    error.contains(match phase.as_str() {
                        "different-directory" => "workspace differs",
                        "wrong-route" => "provider route changed",
                        _ => "Agent `recoverable` is unavailable",
                    }),
                    "{error}"
                );
                assert!(agent.control_session_handle(&id).is_none());
                assert!(
                    requests.lock().unwrap().is_empty(),
                    "a failed dependency cannot sample"
                );
                server.abort();
                return;
            }
            loaded.unwrap();
            let handle = agent.control_session_handle(&id).unwrap();
            let runtime = handle.chat_state_handle.runtime_state().await.unwrap();
            let (operation, checkpoint, source): (String, chat_state::EventSeq, String) =
                serde_json::from_slice(&std::fs::read(home.join("original-checkpoint")).unwrap())
                    .unwrap();
            assert_eq!(
                runtime.phase,
                chat_state::session_runtime::RuntimePhase::Running
            );
            assert_eq!(runtime.operation_id.as_deref(), Some(operation.as_str()));
            assert_eq!(runtime.checkpoint_seq, Some(checkpoint));
            assert_eq!(
                runtime.checkpoint.as_ref().unwrap().runtime_source_hash,
                source
            );
            assert_eq!(
                handle.info.cwd,
                cwd.to_str().unwrap(),
                "storage identity retains its original namespace"
            );
            assert_eq!(
                handle.max_turns,
                Some(8),
                "current tighter turn policy applies"
            );
            assert_eq!(
                handle.model_route.snapshot().model_id.0.as_ref(),
                "test/renamed"
            );
            let surface = handle.chat_state_handle.get_conversation().await;
            assert!(
                surface
                    .iter()
                    .any(|item| item.text_content().contains("ORIGINAL_EFFECTIVE_ROLE"))
            );
            assert!(
                !surface
                    .iter()
                    .any(|item| item.text_content().contains("NEW_FILE_ROLE"))
            );
            cold_recovery_stage(
                "dependencies.continue",
                agent.prompt(acp::PromptRequest::new(
                    id.clone(),
                    vec![acp::ContentBlock::from("--after-resume-wire--")],
                )),
            )
            .await
            .unwrap();
            assert_wire(
                &requests,
                "--after-resume-wire--",
                "ORIGINAL_EFFECTIVE_ROLE",
            );
            let definition = agent::discovery::by_name_in_cwd("recoverable", &cwd).unwrap();
            let (responds_to, selected) = tokio::sync::oneshot::channel();
            handle
                .cmd_tx
                .send(crate::session::SessionCommand::RebuildAgentForDefinition {
                    definition,
                    intent: None,
                    responds_to,
                })
                .unwrap();
            cold_recovery_stage("dependencies.explicit_role_control", selected)
                .await
                .unwrap()
                .unwrap();
            cold_recovery_stage(
                "dependencies.selected_prompt",
                agent.prompt(acp::PromptRequest::new(
                    id.clone(),
                    vec![acp::ContentBlock::from("--selected-role-wire--")],
                )),
            )
            .await
            .unwrap();
            assert_wire(&requests, "--selected-role-wire--", "NEW_FILE_ROLE");
            let mut duplicates = 0;
            for namespace in std::fs::read_dir(home.join("sessions")).unwrap() {
                if namespace.unwrap().path().join(id.0.as_ref()).is_dir() {
                    duplicates += 1;
                }
            }
            assert_eq!(
                duplicates, 1,
                "alias restore must not create another storage namespace"
            );
        }
        server.abort();
    });
}

fn assert_wire(requests: &std::sync::Mutex<Vec<serde_json::Value>>, marker: &str, role: &str) {
    let requests = requests.lock().unwrap();
    let request = requests
        .iter()
        .rev()
        .find(|body| {
            body["tools"]
                .as_array()
                .is_some_and(|tools| tools.len() >= 2)
                && body["messages"].to_string().contains(marker)
        })
        .expect("main model request must be captured");
    let messages = request["messages"].to_string();
    assert!(messages.contains(role), "expected effective role {role}");
    assert!(messages.contains("ORIGINAL_PROJECT_RULE"));
    assert!(!messages.contains("UPDATED_DISK_RULE"));
    if role == "ORIGINAL_EFFECTIVE_ROLE" {
        assert!(!messages.contains("NEW_FILE_ROLE"));
    }
}
