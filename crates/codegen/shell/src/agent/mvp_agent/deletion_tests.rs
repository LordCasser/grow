//! Public deletion refuses before teardown, or reports an invalidated binding.
use super::*;
use crate::session::info::Info;
use crate::session::storage::{StorageAdapter, jsonl::JsonlStorageAdapter};

fn linked_spawn(root: &Info, child: &Info) -> chat_state::TimelineEventKind {
    chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(Box::new(
        chat_state::SubagentSpawnEvent {
            subagent_id: "delete-task".into(),
            child_session_id: child.id.0.to_string(),
            origin_tool_call_id: None,
            security_parent_session_id: root.id.0.to_string(),
            parent_step_id: None,
            subagent_type: "worker".into(),
            description: "delete race".into(),
            prompt: "run".into(),
            execution_recipe: serde_json::json!({}),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            birth_mcp_servers: Some(Default::default()),
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            goal_definition_revision: None,
            surface_completion: false,
            child_cwd: child.cwd.clone(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "model",
                "responses",
                "test",
            ),
            reasoning_effort: None,
        },
    )))
}

async fn drive_delete(agent: &MvpAgent, info: &Info) -> Result<acp::ExtResponse, acp::Error> {
    use acp_transport::AcpAgentHandler as _;
    agent
        .ext_method(acp::ExtRequest::new(
            "grow/session/delete",
            serde_json::value::to_raw_value(
                &serde_json::json!({"sessionId":info.id.0,"cwd":info.cwd}),
            )
            .unwrap()
            .into(),
        ))
        .await
}

#[test]
fn session_delete_runtime_failure_facts() {
    const CHILD: &str = "GROW_DELETE_RUNTIME_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::mvp_agent::tests::deletion_tests::session_delete_runtime_failure_facts",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("GROW_HOME", home.path())
            .env("RUST_MIN_STACK", "16777216")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    run_local_for_bridge_test(|| async {
        use crate::extensions::session_admin::SESSION_DELETE_RUNTIME_INVALIDATED_KEY as KEY;
        for late in [false, true] {
            let root = Info {
                id: acp::SessionId::new(if late {
                    "delete-late-root"
                } else {
                    "delete-early-root"
                }),
                cwd: "/delete-test".into(),
            };
            let child = Info {
                id: acp::SessionId::new(if late {
                    "delete-late-child"
                } else {
                    "delete-early-child"
                }),
                cwd: "/delete-test".into(),
            };
            let writer = JsonlStorageAdapter::default();
            writer
                .init_session(&root, crate::session::persistence::default_model_id())
                .await
                .unwrap();
            writer
                .init_session(&child, crate::session::persistence::default_model_id())
                .await
                .unwrap();
            let mut timeline = chat_state::Timeline::default();
            let spawn = timeline.record(linked_spawn(&root, &child)).unwrap();
            let directory = writer
                .open_session(&root)
                .unwrap()
                .directory()
                .display_path()
                .to_path_buf();
            let path = directory.join("timeline.jsonl");
            if !late {
                writer
                    .append_timeline_event_durable(&root, &spawn)
                    .await
                    .unwrap();
            }
            let agent = build_minimal_agent_for_tests();
            let (handle, _tx, mut commands) = make_live_session_handle(&root.id, None);
            agent.sessions.borrow_mut().insert(root.id.clone(), handle);
            if late {
                let bytes = JsonlStorageAdapter::encode_journal(timeline.events(), &[]).unwrap();
                let thread = std::thread::spawn(move || {
                    while let Some(command) = commands.blocking_recv() {
                        if matches!(command, TestSessionCommand::Shutdown) {
                            std::fs::write(path, bytes).unwrap();
                            break;
                        }
                    }
                    drop(writer);
                });
                agent.session_threads.borrow_mut().insert(
                    root.id.clone(),
                    crate::session::SessionThread::from_handle(thread),
                );
                let error = drive_delete(&agent, &root).await.unwrap_err();
                assert_eq!(
                    error
                        .data
                        .as_ref()
                        .and_then(|data| data.get(KEY))
                        .and_then(serde_json::Value::as_bool),
                    Some(true)
                );
                assert!(!agent.sessions.borrow().contains_key(&root.id));
                assert!(!agent.session_threads.borrow().contains_key(&root.id));
            } else {
                drop(writer);
                let error = drive_delete(&agent, &root).await.unwrap_err();
                assert!(
                    agent.sessions.borrow().contains_key(&root.id),
                    "preflight refusal must preserve the actor"
                );
                assert!(
                    commands.try_recv().is_err(),
                    "no cancellation or shutdown before a known refusal"
                );
                assert_eq!(
                    error
                        .data
                        .as_ref()
                        .and_then(|data| data.get(KEY))
                        .and_then(serde_json::Value::as_bool),
                    Some(false)
                );
            }
            assert!(directory.is_dir(), "refused deletion must preserve history");
        }
    });
}
