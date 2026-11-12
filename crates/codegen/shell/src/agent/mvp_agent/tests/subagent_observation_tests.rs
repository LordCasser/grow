use super::*;
use crate::session::storage::StorageAdapter as _;
use chat_state::{SubagentOutcome, TimelineEventKind};
use tools::implementations::grow_build::task::types::SubagentEvent as RuntimeEvent;

async fn init_history_session(
    storage: &crate::session::storage::jsonl::JsonlStorageAdapter,
    id: &str,
    parent: Option<&str>,
    facts: Vec<TimelineEventKind>,
) -> (crate::session::info::Info, Vec<chat_state::TimelineEvent>) {
    let info = crate::session::info::Info {
        id: acp::SessionId::new(id),
        cwd: "/workspace".into(),
    };
    let mut summary = crate::session::persistence::Summary::new(
        &info,
        crate::session::persistence::default_model_id(),
    )
    .unwrap();
    if let Some(parent) = parent {
        summary.parent_session_id = Some(parent.into());
        summary.session_kind = Some("subagent".into());
    }
    let (_, events) = storage
        .init_session_with_summary(
            &info,
            summary,
            Vec::new(),
            Default::default(),
            Default::default(),
            facts,
        )
        .await
        .unwrap();
    (info, events)
}

fn ext_request(session_id: &str, subagent_id: &str, block: bool) -> acp::ExtRequest {
    acp::ExtRequest::new(
        "grow/subagent/get",
        serde_json::value::to_raw_value(&serde_json::json!({
            "sessionId": session_id,
            "subagentId": subagent_id,
            "block": block,
            "timeoutMs": 30_000,
        }))
        .unwrap()
        .into(),
    )
}

async fn get_subagent(
    agent: &MvpAgent,
    session_id: &str,
    subagent_id: &str,
) -> Result<acp::ExtResponse, acp::Error> {
    let request = ext_request(session_id, subagent_id, false);
    crate::extensions::task::handle_subagent(agent, &request).await
}

fn running_inspection(
    session_id: &str,
    subagent_id: &str,
) -> tools::implementations::grow_build::task::types::SubagentInspection {
    use tools::implementations::grow_build::task::types::{
        SubagentInspection, SubagentSnapshot, SubagentSnapshotStatus,
    };
    SubagentInspection {
        snapshot: SubagentSnapshot {
            subagent_id: subagent_id.to_owned(),
            description: "observed child".into(),
            subagent_type: "review".into(),
            status: SubagentSnapshotStatus::Running {
                turn_count: 1,
                tool_call_count: 2,
                tokens_used: 31,
                context_window_tokens: 128_000,
                context_usage_pct: 1,
                tools_used: vec!["read_file".into()],
                error_count: 0,
            },
            started_at_epoch_ms: 1,
            duration_ms: 4,
        },
        parent_session_id: session_id.to_owned(),
        child_session_id: "observed-child-session".into(),
        fork_parent_prompt_id: Some("prompt-1".into()),
        resumed_from: Some("source-child".into()),
    }
}

fn inspect_request(
    event: RuntimeEvent,
) -> tools::implementations::grow_build::task::types::SubagentInspectRequest {
    let RuntimeEvent::Inspect(request) = event else {
        panic!("expected only the initial Inspect event")
    };
    request
}

#[tokio::test(start_paused = true, flavor = "current_thread")]
async fn nonblocking_subagent_get_uses_one_inspect_within_five_seconds() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let mut events = agent.subagent_event_rx.borrow_mut().take().unwrap();
            let request = ext_request("observation-parent", "observed-child", false);
            let response = tokio::task::spawn_local(async move {
                crate::extensions::task::handle_subagent(&agent, &request).await
            });

            let inspect = inspect_request(events.recv().await.unwrap());
            tokio::time::advance(std::time::Duration::from_secs(4)).await;
            inspect
                .respond_to
                .send(Ok(Some(running_inspection(
                    "observation-parent",
                    "observed-child",
                ))))
                .unwrap();

            let response = response.await.unwrap().unwrap();
            let body: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
            assert_eq!(body["result"]["snapshot"]["status"], "running");
            assert!(
                events.try_recv().is_err(),
                "nonblocking get must not query again"
            );
        })
        .await;
}

#[tokio::test(start_paused = true, flavor = "current_thread")]
async fn unanswered_inspect_times_out_at_the_observation_deadline() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let mut events = agent.subagent_event_rx.borrow_mut().take().unwrap();
            let request = ext_request("observation-timeout-parent", "silent-child", false);
            let response = tokio::task::spawn_local(async move {
                crate::extensions::task::handle_subagent(&agent, &request).await
            });

            let inspect = inspect_request(events.recv().await.unwrap());
            tokio::time::advance(std::time::Duration::from_secs(5)).await;
            let response = response.await.unwrap().unwrap();
            let body: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
            assert!(body["error"].as_str().unwrap().contains("timed out"));
            drop(inspect);
            assert!(events.try_recv().is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn public_subagent_get_reads_only_the_authorized_canonical_cold_result() {
    use crate::session::info::Info;
    use crate::session::persistence::{Summary, default_model_id};
    // grow_home() is cached per process; EnvGuard cannot isolate this fixture
    // after another test has initialized it.
    const CHILD_HOME: &str = "GROW_COLD_SUBAGENT_OBSERVATION_TEST_HOME";
    if std::env::var_os(CHILD_HOME).is_none() {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::mvp_agent::tests::subagent_observation_tests::public_subagent_get_reads_only_the_authorized_canonical_cold_result",
                "--nocapture",
            ])
            .env(CHILD_HOME, home.path())
            .env("GROW_HOME", home.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
    let root_id = "cold-observation-root";
    let child_id = "cold-observation-child";
    let other_root_id = "cold-observation-other-root";
    let broken_root_id = "cold-observation-broken-root";

    async fn add_cancelled_history(
        storage: &crate::session::storage::jsonl::JsonlStorageAdapter,
        root_id: &str,
        child_id: &str,
        corrupt_result_link: bool,
    ) {
        let spawn = chat_state::SubagentSpawnEvent {
            subagent_id: "cold-child".into(),
            child_session_id: child_id.into(),
            origin_tool_call_id: None,
            security_parent_session_id: root_id.into(),
            parent_step_id: None,
            subagent_type: "review".into(),
            description: "cold completed child".into(),
            prompt: "complete the review".into(),
            execution_recipe: serde_json::json!({}),
            context_source: chat_state::SubagentContextSource::Forked,
            source_ref: None,
            context_normalized: false,
            resumed_from: Some("source-child".into()),
            parent_prompt_id: Some("prompt-1".into()),
            goal_definition_revision: None,
            capability_mode: None,
            birth_mcp_servers: Some(Default::default()),
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            surface_completion: true,
            child_cwd: "/workspace".into(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "model",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
        };
        let root_info = crate::session::info::Info {
            id: acp::SessionId::new(root_id),
            cwd: "/workspace".into(),
        };
        let root_summary = crate::session::persistence::Summary::new(
            &root_info,
            crate::session::persistence::default_model_id(),
        )
        .unwrap();
        let (_, root_events) = storage
            .init_session_with_summary(
                &root_info,
                root_summary,
                Vec::new(),
                Default::default(),
                Default::default(),
                vec![TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Spawned(Box::new(spawn.clone())),
                )],
            )
            .await
            .unwrap();
        let spawn_seq = root_events.last().unwrap().seq.get();

        let child_info = crate::session::info::Info {
            id: acp::SessionId::new(child_id),
            cwd: "/workspace".into(),
        };
        let mut child_summary = crate::session::persistence::Summary::new(
            &child_info,
            crate::session::persistence::default_model_id(),
        )
        .unwrap();
        child_summary.parent_session_id = Some(root_id.into());
        child_summary.session_kind = Some("subagent".into());
        let (_, child_events) = storage
            .init_session_with_summary(
                &child_info,
                child_summary,
                Vec::new(),
                Default::default(),
                Default::default(),
                vec![TimelineEventKind::SubagentSeed(
                    chat_state::SubagentSeedEvent {
                        parent_timeline_id: root_id.into(),
                        parent_spawn_seq: spawn_seq,
                        subagent_id: spawn.subagent_id.clone(),
                        security_parent_session_id: root_id.into(),
                        context_source: spawn.context_source,
                        source_ref: spawn.source_ref.clone(),
                        normalized: spawn.context_normalized,
                    },
                )],
            )
            .await
            .unwrap();
        let mut child_timeline = chat_state::Timeline::from_events(child_events).unwrap();
        let result = child_timeline
            .record(TimelineEventKind::SubagentResult(
                chat_state::SubagentResultEvent {
                    subagent_id: spawn.subagent_id.clone(),
                    outcome: SubagentOutcome::Cancelled,
                    duration_ms: 17,
                    tool_calls: 2,
                    turns: 1,
                    tokens_used: 31,
                    error: Some("stopped".into()),
                    output_ref: None,
                    planning_summary: None,
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&child_info, &result)
            .await
            .unwrap();

        let mut root_timeline = chat_state::Timeline::from_events(root_events).unwrap();
        let result_seq = if corrupt_result_link {
            result.seq.get() + 1
        } else {
            result.seq.get()
        };
        let ended = root_timeline
            .record(TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: spawn.subagent_id,
                    child_session_id: child_id.into(),
                    outcome: SubagentOutcome::Cancelled,
                    duration_ms: 17,
                    tool_calls: 2,
                    turns: 1,
                    tokens_used: 31,
                    error: Some("stopped".into()),
                    result_ref: Some(chat_state::TimelineRangeRef {
                        timeline_id: child_id.into(),
                        first_seq: result_seq,
                        last_seq: result_seq,
                    }),
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&root_info, &ended)
            .await
            .unwrap();
    }

    add_cancelled_history(&storage, root_id, child_id, false).await;
    add_cancelled_history(
        &storage,
        broken_root_id,
        "cold-observation-broken-child",
        true,
    )
    .await;
    init_history_session(&storage, other_root_id, None, Vec::new()).await;

    let agent = build_minimal_agent_for_tests();
    drop(agent.subagent_event_rx.borrow_mut().take());

    let mut resources = tools::types::resources::Resources::new();
    resources.insert(tools::implementations::grow_build::task::backend::SubagentBackendResource(
        Arc::new(crate::agent::subagent::observation::backend(
            agent.subagent_event_tx.clone(), root_id.to_owned(), None,
        )),
    ));
    let resources = resources.into_shared();
    let listed = tool_runtime::Tool::run(
        &tools::implementations::grow_build::ListSubagentTool,
        tools::types::tool_metadata::test_ctx(resources.clone()),
        tool_types::ListSubagentInput { scope: tool_types::SubagentListScope::All },
    ).await.unwrap();
    assert_eq!(listed.subagents.len(), 1);
    assert_eq!(listed.subagents[0].subagent_id, "cold-child");
    assert_eq!(listed.subagents[0].status, tool_types::SubagentStatus::Cancelled);
    assert_eq!(listed.subagents[0].direct_parent_session_id, root_id);
    let model_output = tools::types::output::ToolOutput::SubagentList(listed).to_prompt_format();
    let structured: serde_json::Value = serde_json::from_str(&model_output).unwrap();
    assert!(structured["subagents"][0].get("tool_call_count").is_none());
    assert!(structured["subagents"][0].get("output").is_none());

    let details = tool_runtime::Tool::run(
        &tools::implementations::grow_build::GetSubagentStatusTool,
        tools::types::tool_metadata::test_ctx(resources.clone()),
        tool_types::GetSubagentStatusInput { subagent_ids: vec!["cold-child".into(), "unknown".into()] },
    ).await.unwrap();
    assert_eq!(details.subagents[0].status, tool_types::SubagentStatus::Cancelled);
    assert_eq!(details.subagents[0].statistics.tool_call_count, Some(2));
    assert_eq!(details.subagents[0].statistics.turn_count, Some(1));
    assert_eq!(details.subagents[1].status, tool_types::SubagentStatus::Unauthorized);
    assert!(details.subagents[1].description.is_none());
    let output = tools::types::output::ToolOutput::SubagentStatus(details);
    assert!(tools::reminders::task_completion::consumed_completion_ids(&output).is_empty());

    let waited = tool_runtime::Tool::run(
        &tools::implementations::grow_build::WaitSubagentTool,
        tools::types::tool_metadata::test_ctx(resources),
        tool_types::WaitSubagentInput { subagent_ids: vec!["cold-child".into()], timeout_ms: Some(1000) },
    ).await.unwrap();
    assert_eq!(waited.reason, tool_types::SubagentWaitReason::AnyFinished);
    assert_eq!(waited.subagents[0].status, tool_types::SubagentStatus::Cancelled);
    assert_eq!(tools::reminders::task_completion::consumed_completion_ids(&tools::types::output::ToolOutput::SubagentWait(waited)), ["cold-child"]);

    let response = get_subagent(&agent, root_id, "cold-child").await.unwrap();
    let body: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
    assert!(body["error"].is_null());
    assert_eq!(body["result"]["snapshot"]["status"], "cancelled");
    assert_eq!(body["result"]["snapshot"]["parentSessionId"], root_id);
    assert_eq!(body["result"]["snapshot"]["childSessionId"], child_id);
    assert_eq!(body["result"]["snapshot"]["forkParentPromptId"], "prompt-1");
    assert_eq!(body["result"]["snapshot"]["resumedFrom"], "source-child");

    let response = get_subagent(&agent, other_root_id, "cold-child")
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
    assert!(body["error"].as_str().unwrap().contains("authorized scope"));

    let response = get_subagent(&agent, broken_root_id, "cold-child")
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
    assert!(body["error"].as_str().unwrap().contains("invalid"));
}
