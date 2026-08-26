#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
use super::handle_request::{
    canonical_total_tokens, record_subagent_usage, usage_is_incomplete,
};
use crate::test_support::lsp_runtime::{
    DummyLspDispatch, ctx_with_toggle, test_gateway_with_receiver,
};
use tools::implementations::grow_build::task::coordinator::{
    ChildCompletion, CompletionDisposition,
};

#[test]
fn resume_authority_follows_immediate_security_parent() {
    assert!(resume_security_parent_allows("root", "root", "child-a"));
    assert!(resume_security_parent_allows("root", "child-a", "child-a"));
    assert!(!resume_security_parent_allows(
        "root", "child-b", "child-a"
    ));
}

#[test]
fn normalized_child_seeds_its_system_head_before_timeline_creation() {
    let mut conversation = vec![
        ConversationItem::system("parent head"),
        ConversationItem::user("<background_context>summary</background_context>"),
    ];
    let mut prefix_len = Some(2);

    seed_child_system_head(
        &InitialContextSource::Forked,
        false,
        &mut conversation,
        &mut prefix_len,
        "child head",
    )
    .unwrap();

    assert!(matches!(
        &conversation[0],
        ConversationItem::System(system) if system.content.as_ref() == "child head"
    ));
    assert_eq!(prefix_len, Some(2));
}

#[test]
fn new_child_system_head_is_part_of_the_preserved_prefix() {
    let mut conversation = Vec::new();
    let mut prefix_len = None;

    seed_child_system_head(
        &InitialContextSource::New,
        false,
        &mut conversation,
        &mut prefix_len,
        "child head",
    )
    .unwrap();

    assert!(matches!(conversation.first(), Some(ConversationItem::System(_))));
    assert_eq!(prefix_len, Some(1));
}

#[test]
fn inherited_child_context_requires_and_preserves_its_system_head() {
    for (source, verbatim) in [
        (InitialContextSource::Resumed, false),
        (InitialContextSource::Forked, true),
    ] {
        let mut conversation = vec![ConversationItem::system("inherited head")];
        let mut prefix_len = Some(1);
        seed_child_system_head(
            &source,
            verbatim,
            &mut conversation,
            &mut prefix_len,
            "fresh child head",
        )
        .unwrap();
        assert!(matches!(
            conversation.first(),
            Some(ConversationItem::System(system))
                if system.content.as_ref() == "inherited head"
        ));

        let mut missing = vec![ConversationItem::user("legacy headless context")];
        assert!(
            seed_child_system_head(
                &source,
                verbatim,
                &mut missing,
                &mut prefix_len,
                "fresh child head",
            )
            .is_err()
        );
    }
}
#[test]
fn canonical_total_tokens_does_not_double_count_reasoning() {
    let totals = chat_state::UsageTotals {
        input_tokens: 100,
        output_tokens: 40,
        reasoning_tokens: 25,
        ..Default::default()
    };
    assert_eq!(canonical_total_tokens(&totals), 140);
}
#[test]
fn cancellation_makes_an_otherwise_complete_usage_snapshot_incomplete() {
    assert!(usage_is_incomplete(false, true, 0, false));
    assert!(usage_is_incomplete(false, true, 10, false));
    assert!(!usage_is_incomplete(false, false, 0, false));
    assert!(usage_is_incomplete(true, false, 0, false));
}
#[tokio::test]
async fn usage_ack_precedes_terminal_presentation() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (parent_cmd_tx, mut parent_cmd_rx) = mpsc::unbounded_channel();
    ctx.parent_cmd_tx = Some(parent_cmd_tx);
    let by_model = vec![(
            "test-model".to_string(),
            chat_state::UsageTotals {
                input_tokens: 10,
                output_tokens: 4,
                ..Default::default()
            },
        )];
    let mut fold = Box::pin(
        record_subagent_usage(
            ctx.parent_cmd_tx.as_ref(),
            "subagent-1".into(),
            Some(by_model),
            Some("parent-prompt".to_string()),
            false,
        ),
    );
    let command = tokio::select! {
            command = parent_cmd_rx.recv() => command.expect("usage command"),
            result = &mut fold => panic!("usage fold returned before parent command: {result}"),
        };
    let SessionCommand::RecordSubagentUsage { respond_to, .. } = command else {
        panic!("expected RecordSubagentUsage");
    };
    assert!(
            tokio::time::timeout(std::time::Duration::ZERO, &mut fold)
                .await
                .is_err(),
            "child return must wait for usage acknowledgement"
        );
    assert!(parent_cmd_rx.try_recv().is_err());
    respond_to.send(()).expect("usage ack");
    assert!(fold.await);
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    let mut request = auto_wake_test_request("usage-order");
    request.run_in_background = false;
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.spawned_notification_emitted = true;
    completion_data.mark_terminal_committed();
    present_child_completion(
        ChildCompletion {
            request,
            result: SubagentResult {
                success: true,
                subagent_id: "usage-order".to_string(),
                child_session_id: "usage-order".to_string(),
                ..Default::default()
            },
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: true,
                backgrounded: false,
                waiter_delivered: false,
                explicitly_killed: false,
                should_surface: false,
            },
        },
        &gateway,
    );
    assert!(matches!(
            parent_cmd_rx.try_recv(),
            Ok(SessionCommand::GrowSessionNotification {
                notification: SessionNotification {
                    update: SessionUpdate::SubagentFinished { .. },
                    ..
                }
            })
        ));
}
/// Invariant: resolving a subagent applies the parent session's
/// `--tools`/`--disallowed-tools` — driven through
/// `resolve_agent_definition` so the spawn path can't skip them.
#[tokio::test]
async fn subagent_inherits_session_cli_overrides() {
    use agent::config::AgentDefinition;
    let mut probe = AgentDefinition::general_purpose();
    probe.name = "session-override-probe".into();
    probe.disallowed_tools = vec!["write".into()];
    let mut config = crate::agent::config::Config::default();
    config.cli_agents = vec![probe];
    config.cli_agent_overrides = crate::agent::config::CliAgentOverrides {
        tools: Some(vec!["read_file".into(), "grep".into()]),
        disallowed_tools: Some(vec!["search_docs".into(), "write".into()]),
        ..Default::default()
    };
    let mut ctx = ctx_with_toggle(std::collections::HashMap::new());
    ctx.agent_config = Some(config);
    let def = resolve_agent_definition("session-override-probe", &ctx)
        .expect("cli agent resolves");
    assert_eq!(
            def.session_tools_allowlist.as_deref(),
            Some(&["read_file".into(), "grep".into()][..])
        );
    assert_eq!(
            def.session_tools_denylist.as_deref(),
            Some(&["search_docs".into(), "write".into()][..])
        );
    assert_eq!(def.disallowed_tools, vec!["write"]);
}
/// Persisted⇒stamped chokepoint for the subagent emitter: the
/// `SessionCommand` persist hop and the live broadcast must carry the
/// SAME `eventId`, minted before the fork (divergent or missing ids
/// degrade cursor reconnects to full replays or re-applied lines).
#[tokio::test]
async fn emit_subagent_notification_stamps_one_event_id_on_both_paths() {
    use crate::test_support::lsp_runtime::test_gateway_with_receiver;
    let (gateway, mut gateway_rx) = test_gateway_with_receiver();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    emit_subagent_notification(
        &gateway,
        "parent-sess",
        SessionUpdate::SubagentFinished {
            subagent_id: "sa-1".into(),
            child_session_id: "child-1".into(),
            status: "completed".into(),
            error: None,
            tool_calls: 0,
            turns: 0,
            duration_ms: 5,
            tokens_used: 0,
            output: None,
        },
        Some(&cmd_tx),
    );
    let persisted_id = match cmd_rx.try_recv().expect("persist hop must fire") {
        SessionCommand::GrowSessionNotification { notification } => {
            notification
                .meta
                .as_ref()
                .and_then(|m| m.get("eventId"))
                .and_then(|v| v.as_str())
                .expect("persisted subagent lines must carry an eventId")
                .to_string()
        }
        _ => panic!("expected GrowSessionNotification"),
    };
    assert!(persisted_id.starts_with("parent-sess-"));
    let broadcast_id = match gateway_rx.try_recv().expect("broadcast must fire") {
        acp_transport::AcpClientMessage::ExtNotification(args) => {
            let params: serde_json::Value = serde_json::from_str(
                    args.request.params.get(),
                )
                .unwrap();
            params["_meta"]["eventId"].as_str().unwrap().to_string()
        }
        _ => panic!("expected ExtNotification"),
    };
    assert_eq!(persisted_id, broadcast_id);
}
#[test]
fn subagent_max_turns_definition_wins_else_inherits_parent() {
    assert_eq!(super::resolve_subagent_max_turns(Some(2), Some(5)), Some(2));
    assert_eq!(super::resolve_subagent_max_turns(None, Some(5)), Some(5));
}
#[test]
fn resume_worktree_action_covers_three_outcomes() {
    use super::{ResumeWorktreeAction, resume_worktree_action};
    assert_eq!(
            resume_worktree_action(true, Some("refs/grow/subagents/x")),
            ResumeWorktreeAction::Rehydrate
        );
    assert_eq!(
            resume_worktree_action(false, Some("refs/grow/subagents/x")),
            ResumeWorktreeAction::Rehydrate
        );
    assert_eq!(
            resume_worktree_action(true, None),
            ResumeWorktreeAction::Reuse
        );
    assert_eq!(
            resume_worktree_action(false, None),
            ResumeWorktreeAction::Missing
        );
}
#[test]
fn subagent_inherits_parent_lsp_via_context() {
    let parent: std::sync::Arc<dyn tools::implementations::lsp::LspBackend> = Arc::new(
        DummyLspDispatch,
    );
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.lsp = Some(parent.clone());
    assert!(ctx.lsp.is_some());
    assert_eq!(
            Arc::as_ptr(&parent),
            Arc::as_ptr(ctx.lsp.as_ref().unwrap()),
            "child should inherit parent LSP via context"
        );
}
#[test]
fn no_parent_lsp_means_child_gets_none() {
    let ctx = ctx_with_toggle(HashMap::new());
    assert!(ctx.lsp.is_none());
}
fn auto_wake_test_request(id: &str) -> SubagentRequest {
    SubagentRequest {
        id: id.into(),
        prompt: String::new(),
        description: "explore".into(),
        subagent_type: "general-purpose".into(),
        parent_session_id: "parent".into(),
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: true,
        surface_completion: true,
        await_to_completion: false,
        fork_context: false,
        owner: SubagentOwner::Task,
        goal_context: None,
        cancel_token: CancellationToken::new(),
    }
}
async fn admit_test_completion_receipt(
    request: &SubagentRequest,
    result: &SubagentResult,
    completion_data: &mut ShellCompletionData,
    cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
    expected_id: &str,
) -> String {
    let body = {
        let admission = admit_completion_receipt_before_result(request, result, completion_data);
        tokio::pin!(admission);
        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("completion producer command"),
            () = &mut admission => panic!("terminal result outran receipt admission"),
        };
        let SessionCommand::ReceiveNotification {
            source,
            body,
            respond_to: Some(respond_to),
            ..
        } = command
        else {
            panic!("expected acknowledged completion receipt");
        };
        assert!(matches!(
            source,
            chat_state::NotificationSource::SubagentCompleted { subagent_id, .. }
                if subagent_id == expected_id
        ));
        respond_to.send(Ok("receipt-1".into())).unwrap();
        admission.await;
        body
    };
    assert!(completion_data.completion_receipt_admitted);
    body
}

#[tokio::test]
async fn background_subagent_completion_emits_one_acknowledged_durable_receipt() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(cmd_tx);
    let request = auto_wake_test_request("sa-1");
    let result = SubagentResult {
        success: true,
        subagent_id: "sa-1".into(),
        child_session_id: "sa-1".into(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.mark_terminal_committed();
    let body = admit_test_completion_receipt(
        &request,
        &result,
        &mut completion_data,
        &mut cmd_rx,
        "sa-1",
    )
    .await;
    assert!(body.contains("sa-1"));
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    present_child_completion(
        ChildCompletion {
            request,
            result,
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: false,
                backgrounded: true,
                waiter_delivered: false,
                explicitly_killed: false,
                should_surface: true,
            },
        },
        &gateway,
    );
    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(SessionCommand::GrowSessionNotification { .. })
    ));
    assert!(cmd_rx.try_recv().is_err());
}

#[tokio::test]
async fn loop_completion_uses_the_same_acknowledged_durable_receipt() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(cmd_tx);
    let mut request = auto_wake_test_request("loop-child");
    request.description = "loop: check deploy (every 5 minutes)".into();
    request.runtime_overrides.loop_task_id = Some("loop-1".into());
    let result = SubagentResult {
        success: true,
        output: Arc::from("deploy is healthy"),
        subagent_id: "loop-child".into(),
        child_session_id: "loop-child".into(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.mark_terminal_committed();
    let body = admit_test_completion_receipt(
        &request,
        &result,
        &mut completion_data,
        &mut cmd_rx,
        "loop-child",
    )
    .await;
    assert!(body.contains("loop: check deploy"));
    assert!(body.contains("loop-child"));
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    present_child_completion(
        ChildCompletion {
            request,
            result,
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: false,
                backgrounded: true,
                waiter_delivered: false,
                explicitly_killed: false,
                should_surface: true,
            },
        },
        &gateway,
    );
    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(SessionCommand::GrowSessionNotification { .. })
    ));
    assert!(cmd_rx.try_recv().is_err());
}

#[tokio::test]
async fn goal_steering_race_still_emits_the_single_acknowledged_receipt() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(cmd_tx);
    ctx.goal_loop_active
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let request = auto_wake_test_request("sa-race");
    let result = SubagentResult {
        success: true,
        subagent_id: "sa-race".into(),
        child_session_id: "sa-race".into(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.mark_terminal_committed();
    admit_test_completion_receipt(
        &request,
        &result,
        &mut completion_data,
        &mut cmd_rx,
        "sa-race",
    )
    .await;
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    present_child_completion(
        ChildCompletion {
            request,
            result,
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: false,
                backgrounded: true,
                waiter_delivered: true,
                explicitly_killed: false,
                should_surface: false,
            },
        },
        &gateway,
    );
    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(SessionCommand::GrowSessionNotification { .. })
    ));
    assert!(cmd_rx.try_recv().is_err());
}
#[tokio::test]
async fn goal_waiter_cannot_outrun_receipt_admission_or_emit_it_twice() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(cmd_tx);
    let mut request = auto_wake_test_request("goal-child");
    request.owner = SubagentOwner::goal("goal-1", 1);
    let result = SubagentResult {
        success: true,
        subagent_id: request.id.clone(),
        child_session_id: request.id.clone(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.mark_terminal_committed();

    {
        let admission =
            admit_completion_receipt_before_result(&request, &result, &mut completion_data);
        tokio::pin!(admission);
        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("receipt command"),
            () = &mut admission => panic!("completion returned before receipt admission"),
        };
        let SessionCommand::ReceiveNotification { respond_to: Some(respond_to), .. } = command else {
            panic!("expected acknowledged completion receipt");
        };
        respond_to.send(Ok("receipt-1".into())).unwrap();
        admission.await;
    }
    assert!(completion_data.completion_receipt_admitted);

    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    present_child_completion(
        ChildCompletion {
            request,
            result,
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: false,
                backgrounded: true,
                waiter_delivered: true,
                explicitly_killed: false,
                should_surface: false,
            },
        },
        &gateway,
    );
    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(SessionCommand::GrowSessionNotification { .. })
    ));
    assert!(cmd_rx.try_recv().is_err());
}

#[tokio::test]
async fn completion_receipt_retries_a_dropped_ack_with_the_same_identity() {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    let delivery = tokio::spawn(async move {
        admit_subagent_completion_receipt(
            &cmd_tx,
            "retry-child",
            &SubagentOwner::Goal {
                goal_id: "goal-1".into(),
                definition_revision: 1,
            },
            "done".into(),
        )
        .await
    });

    let SessionCommand::ReceiveNotification {
        source: first_source,
        source_version: first_version,
        respond_to: Some(first_ack),
        ..
    } = cmd_rx.recv().await.expect("first receipt attempt")
    else {
        panic!("expected acknowledged receipt");
    };
    drop(first_ack);

    let SessionCommand::ReceiveNotification {
        source: retry_source,
        source_version: retry_version,
        respond_to: Some(retry_ack),
        ..
    } = cmd_rx.recv().await.expect("retried receipt")
    else {
        panic!("expected acknowledged receipt retry");
    };
    assert!(matches!(
        &first_source,
        chat_state::NotificationSource::SubagentCompleted {
            owner: chat_state::NotificationOwner::Goal { goal_id, .. },
            ..
        } if goal_id == "goal-1"
    ));
    assert_eq!(retry_source, first_source);
    assert_eq!(retry_version, first_version);
    retry_ack.send(Ok("receipt-1".into())).unwrap();
    assert!(delivery.await.unwrap());
}

#[tokio::test]
async fn completion_receipt_exhaustion_releases_the_terminal_result() {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    let delivery = tokio::spawn(async move {
        admit_subagent_completion_receipt(
            &cmd_tx,
            "failed-child",
            &SubagentOwner::Task,
            "done".into(),
        )
        .await
    });

    for _ in 0..3 {
        let SessionCommand::ReceiveNotification {
            respond_to: Some(respond_to),
            ..
        } = cmd_rx.recv().await.expect("bounded receipt attempt")
        else {
            panic!("expected acknowledged receipt");
        };
        respond_to.send(Err("disk unavailable".into())).unwrap();
    }

    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(1), delivery)
            .await
            .expect("terminal result must be released after bounded retries")
            .unwrap()
    );
    assert!(cmd_rx.try_recv().is_err());
}
#[test]
fn initializing_snapshot_is_running() {
    let snap = SubagentSnapshot {
        subagent_id: "s".to_string(),
        description: "d".to_string(),
        subagent_type: "t".to_string(),
        status: SubagentSnapshotStatus::Initializing,
        started_at_epoch_ms: 0,
        duration_ms: 0,
    };
    assert!(snap.is_running());
}
#[test]
fn persist_gate_only_persists_successful_nonempty_outputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = crate::session::storage::ContainedDirectory::open(
        dir.path(),
        std::path::Path::new(""),
        "subagent output test session",
        false,
    )
    .expect("open test session capability");
    let ok = SubagentResult {
        success: true,
        output: std::sync::Arc::from("text"),
        ..Default::default()
    };
    let output_ref = persist_subagent_output(&session, &ok)
        .expect("artifact write")
        .expect("successful non-empty output has an artifact");
    assert!(
        output_ref
            .timeline_ref
            .starts_with("artifact:subagent-output:blake3:")
    );
    assert_eq!(
        load_subagent_output_ref_from_directory(&session, &output_ref.timeline_ref).as_deref(),
        Ok("text")
    );
    let empty = SubagentResult {
        success: true,
        ..Default::default()
    };
    assert_eq!(persist_subagent_output(&session, &empty), Ok(None));
    let failed = SubagentResult {
        success: false,
        output: std::sync::Arc::from("partial"),
        ..Default::default()
    };
    assert_eq!(persist_subagent_output(&session, &failed), Ok(None));
}
#[test]
fn subagent_output_roundtrips_through_immutable_artifact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = crate::session::storage::ContainedDirectory::open(
        dir.path(),
        std::path::Path::new(""),
        "subagent output test session",
        false,
    )
    .expect("open test session capability");
    let output = "line one\nline two with unicode ✓";
    let output_ref = write_subagent_output(&session, output).expect("artifact write");
    assert_eq!(
        write_subagent_output(&session, output).expect("idempotent artifact write"),
        output_ref
    );
    assert_eq!(
        load_subagent_output_ref_from_directory(&session, &output_ref.timeline_ref).as_deref(),
        Ok(output)
    );
    assert!(load_subagent_output_ref_from_directory(
        &session,
        "artifact:subagent-output:blake3:0000000000000000000000000000000000000000000000000000000000000000"
    )
    .is_err());
}

fn recovery_spawn(subagent_id: &str, child_session_id: &str) -> chat_state::SubagentSpawnEvent {
    chat_state::SubagentSpawnEvent {
        subagent_id: subagent_id.into(),
        child_session_id: child_session_id.into(),
        security_parent_session_id: "parent-session".into(),
        subagent_type: "review".into(),
        description: "recover child".into(),
        prompt: "finish".into(),
        context_source: chat_state::SubagentContextSource::Forked,
        source_ref: None,
        context_normalized: false,
        resumed_from: None,
        parent_prompt_id: None,
        goal_definition_revision: None,
        capability_mode: None,
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
    }
}

fn write_recovery_child(
    dir: &Path,
    parent_timeline_id: &str,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
) {
    let info = crate::session::info::Info {
        id: acp::SessionId::new(spawn.child_session_id.clone()),
        cwd: spawn.child_cwd.clone(),
    };
    let mut summary = crate::session::persistence::Summary::new(
        &info,
        crate::session::persistence::default_model_id(),
    )
    .unwrap();
    summary.parent_session_id = Some(parent_timeline_id.into());
    summary.session_kind = Some("subagent".into());
    let mut child = chat_state::Timeline::default();
    child
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent_timeline_id.into(),
                parent_spawn_seq: spawn_seq.get(),
                subagent_id: spawn.subagent_id.clone(),
                security_parent_session_id: spawn.security_parent_session_id.clone(),
                context_source: spawn.context_source,
                source_ref: spawn.source_ref.clone(),
                normalized: spawn.context_normalized,
            },
        ))
        .unwrap();
    crate::session::storage::write_jsonl_atomic(
        &dir.join(crate::session::storage::TIMELINE_FILE),
        child.events(),
    )
    .unwrap();
    crate::session::storage::write_bytes_atomic(
        &dir.join(crate::session::storage::SUMMARY_FILE),
        &crate::session::storage::serialize_summary(&summary).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn completed_recovery_publishes_artifact_before_exact_child_result() {
    let child_dir = tempfile::tempdir().unwrap();
    let spawn = recovery_spawn("sa-recovery", "child-recovery");
    let spawn_seq = chat_state::EventSeq::new(7);
    write_recovery_child(child_dir.path(), "parent-recovery", spawn_seq, &spawn);
    let inspection = SubagentInspection {
        snapshot: SubagentSnapshot {
            subagent_id: spawn.subagent_id.clone(),
            description: spawn.description.clone(),
            subagent_type: spawn.subagent_type.clone(),
            status: SubagentSnapshotStatus::Completed {
                output: "canonical recovered output".into(),
                tool_calls: 4,
                turns: 3,
                tokens_used: 912,
                worktree_path: None,
            },
            started_at_epoch_ms: 1,
            duration_ms: 88,
        },
        parent_session_id: "parent-recovery".into(),
        child_session_id: spawn.child_session_id.clone(),
        fork_parent_prompt_id: None,
        resumed_from: None,
    };
    let fallback = result_from_inspection(&spawn, Some(&inspection), 999);
    let (result_ref, result, output) = ensure_recovered_child_result_in_dir(
        "parent-recovery",
        spawn_seq,
        &spawn,
        fallback,
        child_dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(output.as_deref(), Some("canonical recovered output"));
    assert_eq!(result.duration_ms, 88);
    assert_eq!(result.tool_calls, 4);
    assert_eq!(result.turns, 3);
    assert_eq!(result.tokens_used, 912);
    let output_ref = result.output_ref.as_deref().expect("artifact reference");
    let child_directory = crate::session::storage::ContainedDirectory::open(
        child_dir.path(),
        std::path::Path::new(""),
        "recovered child test session",
        false,
    )
    .unwrap();
    assert_eq!(
        load_subagent_output_ref_from_directory(&child_directory, output_ref).unwrap(),
        "canonical recovered output"
    );

    let terminal = chat_state::SubagentTerminalEvent {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        outcome: result.outcome,
        duration_ms: result.duration_ms,
        tool_calls: result.tool_calls,
        turns: result.turns,
        tokens_used: result.tokens_used,
        error: result.error.clone(),
        result_ref: Some(result_ref.clone()),
        snapshot_ref: None,
    };
    let SessionUpdate::SubagentFinished { output, .. } = finish_from_durable_facts_in_directory(
        "parent-recovery",
        spawn_seq,
        &spawn,
        &terminal,
        &child_directory,
    )
    .unwrap() else {
        panic!("expected finished projection");
    };
    assert_eq!(output.as_deref(), Some("canonical recovered output"));

    let different = RecoveredInspectionResult {
        event: chat_state::SubagentResultEvent {
            subagent_id: spawn.subagent_id.clone(),
            outcome: chat_state::SubagentOutcome::Failed,
            duration_ms: 1,
            tool_calls: 0,
            turns: 0,
            tokens_used: 0,
            error: Some("must not replace existing result".into()),
            output_ref: None,
        },
        output: None,
    };
    let (reused_ref, reused_result, reused_output) = ensure_recovered_child_result_in_dir(
        "parent-recovery",
        spawn_seq,
        &spawn,
        different,
        child_dir.path(),
    )
    .await
    .unwrap();
    assert_eq!(reused_ref, result_ref);
    assert_eq!(reused_result, result);
    assert_eq!(reused_output.as_deref(), Some("canonical recovered output"));

    let hash = output_ref.rsplit(':').next().unwrap();
    std::fs::write(
        child_dir
            .path()
            .join("artifacts/subagent-output")
            .join(format!("{hash}.json")),
        "corrupt",
    )
    .unwrap();
    assert!(finish_from_durable_facts_in_directory(
        "parent-recovery",
        spawn_seq,
        &spawn,
        &terminal,
        &child_directory,
    )
    .is_err());
}

#[tokio::test]
async fn invalid_child_seed_does_not_publish_a_result() {
    let child_dir = tempfile::tempdir().unwrap();
    let spawn = recovery_spawn("sa-invalid", "child-invalid");
    let spawn_seq = chat_state::EventSeq::new(3);
    write_recovery_child(child_dir.path(), "wrong-parent", spawn_seq, &spawn);
    let timeline_path = child_dir.path().join(crate::session::storage::TIMELINE_FILE);
    let before = std::fs::read(&timeline_path).unwrap();
    let error = ensure_recovered_child_result_in_dir(
        "parent",
        spawn_seq,
        &spawn,
        RecoveredInspectionResult {
            event: chat_state::SubagentResultEvent {
                subagent_id: spawn.subagent_id.clone(),
                outcome: chat_state::SubagentOutcome::Cancelled,
                duration_ms: 1,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: Some("restart".into()),
                output_ref: None,
            },
            output: None,
        },
        child_dir.path(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, ChildResultRecoveryError::Invalid(_)));
    assert_eq!(std::fs::read(&timeline_path).unwrap(), before);
}

async fn recovery_parent(
    session_dir: &Path,
    parent_session_id: &str,
    spawn: &chat_state::SubagentSpawnEvent,
) -> (
    chat_state::ChatStateHandle,
    chat_state::MockPersistenceReceiver,
) {
    let mut timeline = chat_state::Timeline::default();
    timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(spawn.clone()),
        ))
        .unwrap();
    crate::session::storage::write_jsonl_atomic(
        &session_dir.join(crate::session::storage::TIMELINE_FILE),
        timeline.events(),
    )
    .unwrap();
    let (persistence, receiver) = chat_state::MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = chat_state::ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_sampling_config("model"),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    (handle, receiver)
}

#[tokio::test]
async fn recovery_repairs_receipt_after_durable_parent_terminal() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-receipt-{}", uuid::Uuid::now_v7());
    let mut spawn = recovery_spawn(
        "sa-receipt",
        &format!("child-receipt-{}", uuid::Uuid::now_v7()),
    );
    spawn.goal_id = Some("goal-receipt".into());
    spawn.goal_definition_revision = Some(1);
    let (parent, _persistence) = recovery_parent(parent_dir.path(), &parent_id, &spawn).await;
    let terminal = chat_state::SubagentTerminalEvent {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        outcome: chat_state::SubagentOutcome::Failed,
        duration_ms: 42,
        tool_calls: 3,
        turns: 2,
        tokens_used: 100,
        error: Some("child failed".into()),
        result_ref: None,
        snapshot_ref: None,
    };
    parent
        .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(terminal),
        ))
        .await
        .unwrap();

    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let (backend_tx, _backend_rx) = mpsc::unbounded_channel();
    let backend =
        tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
            backend_tx,
            parent_id.clone(),
        );
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    let task = tokio::spawn({
        let parent = parent.clone();
        let parent_id = parent_id.clone();
        async move {
            reconcile_orphaned_subagents_with_backend(
                &crate::session::storage::SubagentProjectionState::default(),
                false,
                &backend,
                &parent_id,
                &parent,
                None,
                &gateway,
                Some(&cmd_tx),
            )
            .await;
        }
    });

    let SessionCommand::ReceiveNotification {
        source,
        source_version,
        body,
        respond_to: Some(respond_to),
        ..
    } = cmd_rx.recv().await.expect("recovery receipt command")
    else {
        panic!("expected acknowledged recovery receipt");
    };
    assert!(matches!(
        &source,
        chat_state::NotificationSource::SubagentCompleted {
            subagent_id,
            owner: chat_state::NotificationOwner::Goal { goal_id, .. },
        } if subagent_id == "sa-receipt" && goal_id == "goal-receipt"
    ));
    let receipt = parent
        .receive_notification_durably(
            parent_id.clone(),
            source.clone(),
            source_version.clone(),
            chat_state::NotificationPayloadRef {
                blake3: "a".repeat(64),
                bytes: body.len() as u64,
            },
        )
        .await
        .unwrap();
    let receipt_id = match receipt.kind {
        chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received { id, .. },
        ) => id,
        other => panic!("expected notification receipt, got {other:?}"),
    };
    respond_to.send(Ok(receipt_id.clone())).unwrap();
    task.await.unwrap();
    assert_eq!(
        parent
            .received_notification_id(source, source_version)
            .await
            .flatten()
            .as_deref(),
        Some(receipt_id.as_str())
    );
}

#[tokio::test]
async fn recovery_preserves_internal_child_no_surface_decision() {
    let mut spawn = recovery_spawn("internal-child", "internal-session");
    spawn.surface_completion = false;
    let terminal = chat_state::SubagentTerminalEvent {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        outcome: chat_state::SubagentOutcome::Completed,
        duration_ms: 1,
        tool_calls: 0,
        turns: 1,
        tokens_used: 1,
        error: None,
        result_ref: None,
        snapshot_ref: None,
    };
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();

    repair_subagent_completion_receipt(
        "parent",
        &chat_state::ChatStateHandle::noop(),
        Some(&cmd_tx),
        chat_state::EventSeq::new(1),
        &spawn,
        &terminal,
    )
    .await;

    assert!(cmd_rx.try_recv().is_err());
}

#[tokio::test]
async fn backend_running_inspection_keeps_parent_spawn_open() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-running-{}", uuid::Uuid::now_v7());
    let spawn = recovery_spawn(
        "sa-running",
        &format!("child-running-{}", uuid::Uuid::now_v7()),
    );
    let (parent, mut persistence) = recovery_parent(parent_dir.path(), &parent_id, &spawn).await;
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx,
        parent_id.clone(),
    );
    let inspection_spawn = spawn.clone();
    let inspection_parent = parent_id.clone();
    let responder = tokio::spawn(async move {
        let Some(tools::implementations::grow_build::task::types::SubagentEvent::Inspect(request)) =
            backend_rx.recv().await
        else {
            panic!("expected backend inspection");
        };
        request
            .respond_to
            .send(Some(SubagentInspection {
                snapshot: SubagentSnapshot {
                    subagent_id: inspection_spawn.subagent_id.clone(),
                    description: inspection_spawn.description.clone(),
                    subagent_type: inspection_spawn.subagent_type.clone(),
                    status: SubagentSnapshotStatus::Running {
                        turn_count: 1,
                        tool_call_count: 2,
                        tokens_used: 300,
                        context_window_tokens: 10_000,
                        context_usage_pct: 3,
                        tools_used: vec!["read_file".into()],
                        error_count: 0,
                    },
                    started_at_epoch_ms: 1,
                    duration_ms: 20,
                },
                parent_session_id: inspection_parent,
                child_session_id: inspection_spawn.child_session_id.clone(),
                fork_parent_prompt_id: None,
                resumed_from: None,
            }))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(chat_state::TimelineEvent {
                kind: chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(_)
                ),
                ..
            })
        )),
        "a live backend child must remain reconnectable"
    );
}

#[tokio::test]
async fn foreign_backend_inspection_cannot_close_or_fill_a_parent_spawn() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-bound-{}", uuid::Uuid::now_v7());
    let spawn = recovery_spawn(
        "sa-collision",
        &format!("child-bound-{}", uuid::Uuid::now_v7()),
    );
    let (parent, mut persistence) = recovery_parent(parent_dir.path(), &parent_id, &spawn).await;
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx,
        parent_id.clone(),
    );
    let inspection_spawn = spawn.clone();
    let requested_parent = parent_id.clone();
    let responder = tokio::spawn(async move {
        let Some(tools::implementations::grow_build::task::types::SubagentEvent::Inspect(request)) =
            backend_rx.recv().await
        else {
            panic!("expected backend inspection");
        };
        assert_eq!(
            request.parent_session_id.as_deref(),
            Some(requested_parent.as_str())
        );
        request
            .respond_to
            .send(Some(SubagentInspection {
                snapshot: SubagentSnapshot {
                    subagent_id: inspection_spawn.subagent_id.clone(),
                    description: inspection_spawn.description.clone(),
                    subagent_type: inspection_spawn.subagent_type.clone(),
                    status: SubagentSnapshotStatus::Completed {
                        output: "foreign secret".into(),
                        tool_calls: 9,
                        turns: 3,
                        tokens_used: 777,
                        worktree_path: None,
                    },
                    started_at_epoch_ms: 1,
                    duration_ms: 20,
                },
                parent_session_id: "different-parent".into(),
                child_session_id: "different-child".into(),
                fork_parent_prompt_id: None,
                resumed_from: None,
            }))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(chat_state::TimelineEvent {
                kind: chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(_)
                ),
                ..
            })
        )),
        "a foreign inspection must leave the local spawn open"
    );
}

#[tokio::test]
async fn unavailable_completed_output_keeps_parent_spawn_open() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-unavailable-{}", uuid::Uuid::now_v7());
    let spawn = recovery_spawn(
        "sa-unavailable",
        &format!("child-unavailable-{}", uuid::Uuid::now_v7()),
    );
    let (parent, mut persistence) = recovery_parent(parent_dir.path(), &parent_id, &spawn).await;
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx,
        parent_id.clone(),
    );
    let inspection_spawn = spawn.clone();
    let inspection_parent = parent_id.clone();
    let responder = tokio::spawn(async move {
        let Some(tools::implementations::grow_build::task::types::SubagentEvent::Inspect(request)) =
            backend_rx.recv().await
        else {
            panic!("expected backend inspection");
        };
        request
            .respond_to
            .send(Some(SubagentInspection {
                snapshot: SubagentSnapshot {
                    subagent_id: inspection_spawn.subagent_id.clone(),
                    description: inspection_spawn.description.clone(),
                    subagent_type: inspection_spawn.subagent_type.clone(),
                    status: SubagentSnapshotStatus::CompletedOutputUnavailable {
                        error: "artifact hash mismatch".into(),
                        tool_calls: 4,
                        turns: 2,
                        tokens_used: 500,
                        worktree_path: None,
                    },
                    started_at_epoch_ms: 1,
                    duration_ms: 20,
                },
                parent_session_id: inspection_parent,
                child_session_id: inspection_spawn.child_session_id.clone(),
                fork_parent_prompt_id: None,
                resumed_from: None,
            }))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(chat_state::TimelineEvent {
                kind: chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(_)
                ),
                ..
            })
        )),
        "unverifiable completed output must not be laundered into a parent terminal"
    );
}

#[tokio::test]
async fn missing_unpublished_child_closes_without_forging_result_ref() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-missing-{}", uuid::Uuid::now_v7());
    let spawn = recovery_spawn(
        "sa-missing",
        &format!("child-missing-{}", uuid::Uuid::now_v7()),
    );
    let (parent, mut persistence) = recovery_parent(parent_dir.path(), &parent_id, &spawn).await;
    let (backend_tx, mut backend_rx) = mpsc::unbounded_channel();
    let backend = tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        backend_tx,
        parent_id.clone(),
    );
    let responder = tokio::spawn(async move {
        let Some(tools::implementations::grow_build::task::types::SubagentEvent::Inspect(request)) =
            backend_rx.recv().await
        else {
            panic!("expected backend inspection");
        };
        request.respond_to.send(None).unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
    )
    .await;
    responder.await.unwrap();
    let terminal = persistence
        .drain()
        .into_iter()
        .find_map(|record| match record {
            chat_state::PersistenceRecord::Timeline(chat_state::TimelineEvent {
                kind: chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(terminal),
                ),
                ..
            }) => Some(terminal),
            _ => None,
        })
        .expect("missing child should close the parent spawn");
    assert_eq!(terminal.outcome, chat_state::SubagentOutcome::Cancelled);
    assert!(terminal.result_ref.is_none());
}
#[cfg(unix)]
#[test]
fn subagent_output_reader_rejects_symlinked_artifact_root() {
    use std::os::unix::fs::symlink;

    let session = tempfile::tempdir().expect("session");
    let outside = tempfile::tempdir().expect("outside");
    let output = "redirected output";
    let json = serde_json::to_string(&SubagentOutputFileRef {
        schema_version: SUBAGENT_OUTPUT_SCHEMA_VERSION,
        output,
    })
    .unwrap();
    let hash = blake3::hash(json.as_bytes()).to_hex().to_string();
    let outside_dir = outside.path().join("subagent-output");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::write(outside_dir.join(format!("{hash}.json")), json).unwrap();
    symlink(outside.path(), session.path().join("artifacts")).unwrap();

    let directory = crate::session::storage::ContainedDirectory::open(
        session.path(),
        std::path::Path::new(""),
        "subagent output symlink test session",
        false,
    )
    .unwrap();
    assert!(load_subagent_output_ref_from_directory(
        &directory,
        &format!("artifact:subagent-output:blake3:{hash}")
    )
    .is_err());
}
#[test]
fn initial_context_source_new_is_default() {
    let source = InitialContextSource::New;
    assert!(matches!(source, InitialContextSource::New));
}
#[test]
fn initial_context_source_forked_distinct_from_new_and_resumed() {
    let source = InitialContextSource::Forked;
    assert!(matches!(source, InitialContextSource::Forked));
    assert_ne!(source, InitialContextSource::New);
    assert_ne!(source, InitialContextSource::Resumed);
}
#[test]
fn forked_initial_context_normalizes_parent_history() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("parent system"),
            ConversationItem::user("UNIQUE_FORK_MARKER_abc123 implement multi-repo fix"),
            ConversationItem::assistant("noted"),
        ];
    let ctx = forked_initial_context(items).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert_eq!(ctx.prefix_len, Some(2));
    assert_eq!(ctx.conversation.len(), 2);
    if let ConversationItem::User(ref u) = ctx.conversation[1] {
        let text: String = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect();
        assert!(text.contains("<background_context>"));
        assert!(
                text.contains("UNIQUE_FORK_MARKER_abc123"),
                "distinctive parent token must appear in background: {text}"
            );
    } else {
        panic!("expected User background at [1]");
    }
}
#[test]
fn forked_initial_context_inherits_parent_across_reasoning() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("parent system"),
            ConversationItem::user("remember UNIQUE_FORK_MARKER_TEST"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "deliberating",
            )),
            ConversationItem::assistant("ack"),
        ];
    let ctx = forked_initial_context(items).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert_eq!(ctx.prefix_len, Some(2));
    assert_eq!(ctx.conversation.len(), 2);
    if let ConversationItem::User(ref u) = ctx.conversation[1] {
        let text: String = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect();
        assert!(
                text.contains("<background_context>"),
                "background wrapper must be present: {text}"
            );
        assert!(
                text.contains("UNIQUE_FORK_MARKER_TEST"),
                "parent context must be inherited across the reasoning sibling: {text}"
            );
    } else {
        panic!("expected User background at [1]");
    }
}
#[test]
fn forked_initial_context_empty_is_rejected() {
    assert_eq!(forked_initial_context(vec![]).unwrap_err(), "empty parent Surface");
}
#[test]
fn resume_vs_fork_helper_shapes_differ() {
    use sampling_types::conversation::ConversationItem;
    let resume_items = vec![
            ConversationItem::system("child system"),
            ConversationItem::user("prior subagent work"),
            ConversationItem::assistant("done"),
        ];
    let resumed = resume_initial_context(resume_items.clone());
    let forked = forked_initial_context(resume_items).unwrap();
    assert_eq!(resumed.source, InitialContextSource::Resumed);
    assert_eq!(forked.source, InitialContextSource::Forked);
    assert!(resumed.conversation.len() > forked.conversation.len());
    assert!(!matches!(
            resumed.conversation.get(1),
            Some(ConversationItem::User(u))
                if u.content.iter().any(|p| matches!(
                    p,
                    sampling_types::conversation::ContentPart::Text { text }
                        if text.contains("<background_context>")
                ))
        ));
}
#[test]
fn forked_initial_context_applies_fork_filter_before_normalize() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("complete user"),
            ConversationItem::assistant("complete asst"),
            ConversationItem::user("INCOMPLETE_TRAILING"),
        ];
    let ctx = forked_initial_context(items).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    if let ConversationItem::User(ref u) = ctx.conversation[1] {
        let text: String = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect();
        assert!(text.contains("complete user"));
        assert!(
                !text.contains("INCOMPLETE_TRAILING"),
                "fork_filter must truncate incomplete trailing turn: {text}"
            );
    } else {
        panic!("expected background user");
    }
}
#[test]
fn verbatim_fork_keeps_items_byte_for_byte_when_small() {
    use sampling_types::conversation::{
        ContentPart, ConversationItem, SyntheticReason, UserItem,
    };
    let items = vec![
            ConversationItem::system("parent system"),
            ConversationItem::user("remember UNIQUE_FORK_MARKER_TEST"),
            ConversationItem::User(UserItem {
                content: vec![ContentPart::Text {
                    text: "SYNTHETIC_KEEP_ME".into(),
                }],
                synthetic_reason: Some(SyntheticReason::SystemReminder),
                permission_evidence: None,
                ..Default::default()
            }),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "thinking",
            )),
            ConversationItem::assistant("ack"),
        ];
    let ctx = verbatim_or_normalize_fork(items, 256_000).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert!(
            ctx.verbatim_fork,
            "a small, complete-tail parent must mirror verbatim"
        );
    assert_eq!(ctx.prefix_len, Some(5));
    assert_eq!(ctx.conversation.len(), 5);
    assert!(matches!(ctx.conversation[0], ConversationItem::System(_)));
    assert!(matches!(
            ctx.conversation.last(),
            Some(ConversationItem::Assistant(_))
        ));
    let text_present = |needle: &str| {
        ctx
            .conversation
            .iter()
            .any(|i| {
                matches!(i, ConversationItem::User(u)
                    if u.content.iter().any(|p| matches!(p,
                        ContentPart::Text { text } if text.contains(needle))))
            })
    };
    assert!(
            text_present("UNIQUE_FORK_MARKER_TEST"),
            "marker must survive verbatim"
        );
    assert!(
            text_present("SYNTHETIC_KEEP_ME"),
            "synthetic-reason item must be preserved verbatim, NOT stripped"
        );
    assert!(
            ctx.conversation
                .iter()
                .any(|i| matches!(i, ConversationItem::User(u) if u.synthetic_reason.is_some())),
            "the synthetic_reason marker itself must remain in the verbatim mirror"
        );
    assert!(
            !text_present("<background_context>"),
            "verbatim fork must NOT summarize into a background blob"
        );
}
#[test]
fn verbatim_fork_falls_back_to_summary_on_incomplete_tail() {
    use sampling_types::conversation::{
        AssistantItem, ContentPart, ConversationItem, ToolCall,
    };
    let items = vec![
            ConversationItem::system("parent system"),
            ConversationItem::user("q1 UNIQUE_FORK_MARKER_TEST"),
            ConversationItem::assistant("a1"),
            ConversationItem::user("q2"),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
        ];
    let ctx = verbatim_or_normalize_fork(items, 256_000).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert!(
            !ctx.verbatim_fork,
            "an incomplete (dangling tool call) tail must fall back to summary"
        );
    assert_eq!(ctx.prefix_len, Some(2));
    assert!(
            ctx.conversation.iter().any(|i| {
                matches!(i, ConversationItem::User(u)
                    if u.content.iter().any(|p| matches!(p,
                        ContentPart::Text { text } if text.contains("<background_context>"))))
            }),
            "summarized fallback must produce a background_context blob"
        );
}
#[test]
fn summarized_fork_is_not_a_verbatim_mirror() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("parent system prompt"),
            ConversationItem::user("turn one UNIQUE_FORK_MARKER_TEST"),
            ConversationItem::assistant("ack"),
        ];
    let ctx = verbatim_or_normalize_fork(items, 1).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert!(!ctx.verbatim_fork);
    let verbatim_mirror_fork = ctx.source == InitialContextSource::Forked
        && ctx.verbatim_fork;
    assert!(
            !verbatim_mirror_fork,
            "a summarized fork must NOT be treated as a verbatim mirror"
        );
}
#[test]
fn verbatim_fork_falls_back_to_summary_when_oversize() {
    use sampling_types::conversation::{ContentPart, ConversationItem};
    let items = vec![
            ConversationItem::system("parent system"),
            ConversationItem::user("turn one UNIQUE_FORK_MARKER_TEST with some text"),
            ConversationItem::assistant("ack one"),
        ];
    let ctx = verbatim_or_normalize_fork(items, 1).unwrap();
    assert_eq!(ctx.source, InitialContextSource::Forked);
    assert!(
            !ctx.verbatim_fork,
            "oversize parent must fall back to summary"
        );
    assert_eq!(ctx.prefix_len, Some(2));
    let has_blob = ctx
        .conversation
        .iter()
        .any(|i| {
            matches!(i, ConversationItem::User(u)
                if u.content.iter().any(|p| matches!(p,
                    ContentPart::Text { text } if text.contains("<background_context>"))))
        });
    assert!(
            has_blob,
            "oversize fallback must produce a background_context blob"
        );
}
#[test]
fn verbatim_fork_empty_after_filter_is_rejected() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![ConversationItem::user("/goal do the thing")];
    assert!(verbatim_or_normalize_fork(items, 256_000).is_err());
}
#[test]
fn verbatim_or_normalize_fork_system_only_is_rejected() {
    use sampling_types::conversation::ConversationItem;
    for items in [
        vec![ConversationItem::system("sys")],
        vec![ConversationItem::system("a"), ConversationItem::system("b")],
    ] {
        assert!(verbatim_or_normalize_fork(items, 256_000).is_err());
    }
}
#[test]
fn forked_initial_context_system_only_is_rejected() {
    use sampling_types::conversation::ConversationItem;
    assert!(forked_initial_context(vec![ConversationItem::system("sys")]).is_err());
}
#[test]
fn fork_context_normalized_only_for_summarized() {
    assert!(!fork_context_normalized(
            &InitialContextSource::Forked,
            true
        ));
    assert!(fork_context_normalized(
            &InitialContextSource::Forked,
            false
        ));
    assert!(!fork_context_normalized(&InitialContextSource::New, false));
    assert!(!fork_context_normalized(
            &InitialContextSource::Resumed,
            false
        ));
    use sampling_types::conversation::ConversationItem;
    let verbatim = verbatim_or_normalize_fork(
        vec![
                ConversationItem::system("sys"),
                ConversationItem::user("q"),
                ConversationItem::assistant("a"),
            ],
        256_000,
    )
    .unwrap();
    assert!(verbatim.verbatim_fork);
    assert!(!fork_context_normalized(
            &verbatim.source,
            verbatim.verbatim_fork
        ));
    let summarized = verbatim_or_normalize_fork(
        vec![
                ConversationItem::system("sys"),
                ConversationItem::user("q with text"),
                ConversationItem::assistant("a"),
            ],
        1,
    )
    .unwrap();
    assert!(!summarized.verbatim_fork);
    assert!(fork_context_normalized(
            &summarized.source,
            summarized.verbatim_fork
        ));
}
fn bootstrap_test_request(fork_context: bool) -> SubagentRequest {
    SubagentRequest {
        id: "bootstrap-test".into(),
        prompt: "plan".into(),
        description: "d".into(),
        subagent_type: "general-purpose".into(),
        parent_session_id: "parent".into(),
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: false,
        surface_completion: false,
        await_to_completion: false,
        fork_context,
        owner: SubagentOwner::Task,
        goal_context: None,
        cancel_token: CancellationToken::new(),
    }
}
#[tokio::test]
async fn bootstrap_no_fork_is_new() {
    let req = bootstrap_test_request(false);
    let ctx = ctx_with_toggle(HashMap::new());
    let out = bootstrap_initial_context(&req, None, &ctx, 128_000).await;
    match out {
        BootstrapInitialContext::Ready(ic) => {
            assert_eq!(ic.source, InitialContextSource::New);
            assert!(ic.conversation.is_empty());
        }
        BootstrapInitialContext::Abort(m) => panic!("unexpected abort: {m}"),
    }
}
#[tokio::test]
async fn bootstrap_fork_without_parent_fails_closed() {
    let req = bootstrap_test_request(true);
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.parent_chat_state = None;
    ctx.parent_session_info = None;
    ctx.delegation_chat_state = None;
    ctx.delegation_session_info = None;
    let out = bootstrap_initial_context(&req, None, &ctx, 128_000).await;
    match out {
        BootstrapInitialContext::Ready(_) => panic!("fork must not silently become a new child"),
        BootstrapInitialContext::Abort(message) => {
            assert!(message.contains("parent Surface is unavailable"));
        }
    }
}
#[tokio::test]
async fn bootstrap_fork_live_parent_chat_state_is_forked_with_marker() {
    use sampling_types::conversation::ConversationItem;
    const MARKER: &str = "UNIQUE_LIVE_FORK_MARKER_xyz789";
    let req = bootstrap_test_request(true);
    let mut ctx = ctx_with_toggle(HashMap::new());
    let chat = spawn_test_parent_chat_state("grow-4.5");
    let (_, source_surface_revision) = chat
        .get_conversation_with_revision()
        .await
        .expect("test parent chat-state actor must be live");
    chat.replace_context_durably(
        vec![
            ConversationItem::system("parent system"),
            ConversationItem::user(format!("{MARKER} implement multi-repo fix")),
            ConversationItem::assistant("noted the multi-repo work"),
        ],
        source_surface_revision,
    )
    .await
    .unwrap();
    ctx.parent_chat_state = Some(chat.clone());
    ctx.delegation_chat_state = Some(chat);
    ctx.parent_session_info = None;
    ctx.delegation_session_info = None;
    let out = bootstrap_initial_context(&req, None, &ctx, 128_000).await;
    match out {
        BootstrapInitialContext::Ready(ic) => {
            assert_eq!(ic.source, InitialContextSource::Forked);
            assert!(
                    ic.verbatim_fork,
                    "small complete-tail parent must mirror verbatim"
                );
            assert_eq!(ic.conversation.len(), 3);
            assert_eq!(ic.prefix_len, Some(3));
            assert!(matches!(ic.conversation[0], ConversationItem::System(_)));
            assert!(matches!(ic.conversation[1], ConversationItem::User(_)));
            assert!(matches!(ic.conversation[2], ConversationItem::Assistant(_)));
            let text: String = ic
                .conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::User(u) => {
                        Some(
                            u
                                .content
                                .iter()
                                .filter_map(|p| match p {
                                    sampling_types::conversation::ContentPart::Text {
                                        text,
                                    } => Some(text.as_ref()),
                                    _ => None,
                                })
                                .collect::<String>(),
                        )
                    }
                    _ => None,
                })
                .collect();
            assert!(
                    text.contains(MARKER),
                    "live parent marker must appear verbatim: {text}"
                );
            assert!(
                    !text.contains("<background_context>"),
                    "verbatim mirror must NOT wrap items in a background_context blob: {text}"
                );
        }
        BootstrapInitialContext::Abort(m) => panic!("unexpected abort: {m}"),
    }
}
#[tokio::test]
async fn copy_session_data_preserves_parent_surface() {
    use crate::sampling::ConversationItem;
    use crate::session::storage::StorageAdapter;
    use crate::session::storage::jsonl::JsonlStorageAdapter;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let adapter = JsonlStorageAdapter::with_root(root.to_path_buf());
    let parent_info = SessionInfo {
        id: acp::SessionId::new("parent-fork-test"),
        cwd: "/workspace".to_string(),
    };
    adapter.init_session(&parent_info, acp::ModelId::new("test-model")).await.unwrap();
    let timeline = chat_state::Timeline::from_seed(vec![
        ConversationItem::user("What files?"),
        ConversationItem::assistant("listed"),
    ])
    .unwrap();
    for event in timeline.events() {
        adapter
            .append_timeline_event(&parent_info, event)
            .await
            .unwrap();
    }
    let child_info = SessionInfo {
        id: acp::SessionId::new("child-fork-test"),
        cwd: "/workspace".to_string(),
    };
    let result = adapter
        .copy_session_data_sync(
            &parent_info,
            &child_info,
            crate::session::storage::CopySessionOptions {
                parent_session_id: Some("parent-fork-test".to_string()),
                new_model_id: Some("test-model".to_string()),
                session_kind: Some("subagent_fork".to_string()),
                fork_context_source: Some("forked".to_string()),
                inherit_control: false,
                fork_filter: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(result.surface_items_copied > 0, "should copy the Surface");
    let child_data = adapter.load_session(&child_info).await.unwrap();
    assert_eq!(
            child_data.summary.session_kind.as_deref(),
            Some("subagent_fork")
        );
    assert_eq!(
            child_data.summary.fork_context_source.as_deref(),
            Some("forked")
        );
    assert_eq!(
            child_data.summary.parent_session_id.as_deref(),
            Some("parent-fork-test")
        );
    let child_timeline =
        chat_state::Timeline::from_events(child_data.timeline_events).unwrap();
    assert!(
            !child_timeline.surface().is_empty(),
            "child should have inherited the parent Surface"
        );
}
fn make_validation_ctx(toggle: HashMap<String, bool>) -> SubagentValidationContext {
    SubagentValidationContext {
        parent_cwd: PathBuf::from("/tmp"),
        subagent_toggle: toggle,
        ..Default::default()
    }
}
#[test]
fn validate_subagent_type_returns_ok_for_known_enabled_agent() {
    let ctx = make_validation_ctx(HashMap::new());
    let outcome = validate_subagent_type("explore", &ctx);
    assert!(
            matches!(outcome, SubagentValidateTypeOutcome::Ok),
            "expected Ok, got {outcome:?}",
        );
}
#[test]
fn validate_subagent_type_returns_unknown_for_invented_type() {
    let ctx = make_validation_ctx(HashMap::new());
    let outcome = validate_subagent_type("totally-invented-agent-name", &ctx);
    match outcome {
        SubagentValidateTypeOutcome::Unknown { available } => {
            for expected in ["general-purpose", "explore"] {
                assert!(
                        available.iter().any(|n| n == expected),
                        "available list must include built-in {expected:?}: {available:?}",
                    );
            }
            let mut sorted = available.clone();
            sorted.sort();
            assert_eq!(available, sorted, "available must be sorted");
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
#[test]
fn validate_subagent_type_returns_disabled_when_toggled_off() {
    let toggle = HashMap::from([("explore".to_string(), false)]);
    let ctx = make_validation_ctx(toggle);
    let outcome = validate_subagent_type("explore", &ctx);
    assert!(
            matches!(outcome, SubagentValidateTypeOutcome::Disabled),
            "expected Disabled, got {outcome:?}",
        );
}
#[test]
fn validate_subagent_type_honors_parent_agent_filter() {
    let mut definition = agent::AgentDefinition::default_grow_build();
    definition.subagents.allow = vec!["explore".to_string()];
    let mut ctx = make_validation_ctx(HashMap::new());
    ctx.subagent_filter = definition.subagent_filter();

    assert!(matches!(
        validate_subagent_type("explore", &ctx),
        SubagentValidateTypeOutcome::Ok,
    ));
    assert!(matches!(
        validate_subagent_type("general-purpose", &ctx),
        SubagentValidateTypeOutcome::Disabled,
    ));
}
#[test]
fn validate_subagent_type_unknown_includes_cli_agents_in_available() {
    let mut ctx = make_validation_ctx(HashMap::new());
    ctx.cli_agent_names = vec!["user-defined-agent".to_string()];
    match validate_subagent_type("invented", &ctx) {
        SubagentValidateTypeOutcome::Unknown { available } => {
            assert!(
                    available.iter().any(|n| n == "user-defined-agent"),
                    "cli agent name missing from available list: {available:?}",
                );
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
#[test]
fn validate_subagent_type_unknown_dedupes_cli_against_builtins() {
    let mut ctx = make_validation_ctx(HashMap::new());
    ctx.cli_agent_names = vec!["explore".to_string()];
    match validate_subagent_type("invented", &ctx) {
        SubagentValidateTypeOutcome::Unknown { available } => {
            let count = available.iter().filter(|n| n.as_str() == "explore").count();
            assert_eq!(count, 1, "explore must appear once: {available:?}");
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
#[test]
fn validate_subagent_type_unknown_omits_disabled_types_from_available_list() {
    let toggle = HashMap::from([("explore".to_string(), false)]);
    let ctx = make_validation_ctx(toggle);
    match validate_subagent_type("explor", &ctx) {
        SubagentValidateTypeOutcome::Unknown { available } => {
            assert!(
                    !available.iter().any(|n| n == "explore"),
                    "disabled type must not appear in available: {available:?}",
                );
            assert!(
                    available.iter().any(|n| n == "general-purpose"),
                    "non-disabled built-ins must still appear: {available:?}",
                );
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
#[test]
fn validate_subagent_type_unknown_omits_disabled_cli_agents_from_available_list() {
    let toggle = HashMap::from([("custom".to_string(), false)]);
    let mut ctx = make_validation_ctx(toggle);
    ctx.cli_agent_names = vec!["custom".to_string(), "user-defined".to_string()];
    match validate_subagent_type("invented", &ctx) {
        SubagentValidateTypeOutcome::Unknown { available } => {
            assert!(
                    !available.iter().any(|n| n == "custom"),
                    "disabled cli agent must not appear: {available:?}",
                );
            assert!(
                    available.iter().any(|n| n == "user-defined"),
                    "enabled cli agent must appear: {available:?}",
                );
        }
        other => panic!("expected Unknown, got {other:?}"),
    }
}
#[test]
fn validate_subagent_type_recognizes_cli_agent_by_name() {
    let mut ctx = make_validation_ctx(HashMap::new());
    ctx.cli_agent_names = vec!["user-defined".to_string()];
    assert!(matches!(
            validate_subagent_type("user-defined", &ctx),
            SubagentValidateTypeOutcome::Ok,
        ));
}
#[tokio::test]
async fn committed_cancelled_completion_presents_one_finish() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (parent_cmd_tx, mut parent_cmd_rx) = mpsc::unbounded_channel();
    ctx.parent_cmd_tx = Some(parent_cmd_tx);
    let (gateway, mut gateway_rx) = test_gateway_with_receiver();
    let request = auto_wake_test_request("promote-cancel");
    let result = cancel_pending_shell_child(
        &request.id,
        &acp::SessionId::new(request.id.clone()),
        None,
        false,
        42,
    )
    .await;
    assert!(result.cancelled);
    assert!(!result.success);
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.spawned_notification_emitted = true;
    completion_data.mark_terminal_committed();
    present_child_completion(
        ChildCompletion {
            request,
            result,
            completion_data,
            disposition: CompletionDisposition {
                foreground_delivered: false,
                backgrounded: false,
                waiter_delivered: false,
                explicitly_killed: false,
                should_surface: false,
            },
        },
        &gateway,
    );
    let mut persisted = 0;
    while let Ok(command) = parent_cmd_rx.try_recv() {
        if matches!(
                command,
                SessionCommand::GrowSessionNotification {
                    notification: SessionNotification {
                        update: SessionUpdate::SubagentFinished { status, .. },
                        ..
                    }
                } if status == "cancelled"
            ) {
            persisted += 1;
        }
    }
    assert_eq!(persisted, 1);
    let mut live = 0;
    while let Ok(message) = gateway_rx.try_recv() {
        if matches!(
                message,
                acp_transport::AcpClientMessage::ExtNotification(args)
                    if args.request.params.get().contains("\"status\":\"cancelled\"")
            ) {
            live += 1;
        }
    }
    assert_eq!(live, 1);
}
async fn run_promote_cancel_with_worktree(
    worktree: &Path,
    worktree_freshly_created: bool,
) {
    let result = cancel_pending_shell_child(
        "worktree-cancel",
        &acp::SessionId::new("worktree-cancel"),
        Some(worktree),
        worktree_freshly_created,
        42,
    )
    .await;
    assert!(result.cancelled);
}
/// A pending cancel removes a freshly-created worktree but preserves a
/// resumed child worktree owned by its source.
#[tokio::test]
async fn cancel_pending_at_promote_removes_fresh_worktree_preserves_resumed() {
    test_utils::require_git!();
    use test_utils::git::{git_commit_all, init_git_repo};
    let temp = tempfile::TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_git_repo(&repo);
    std::fs::write(repo.join("tracked.txt"), "original").unwrap();
    git_commit_all(&repo, "initial");
    let fresh = temp.path().join("subagent-fresh");
    fast_worktree::WorktreeBuilder::new(&repo, &fresh)
        .standalone(true)
        .create()
        .unwrap();
    assert!(fresh.exists());
    run_promote_cancel_with_worktree(&fresh, true).await;
    assert!(
            !fresh.exists(),
            "freshly-created worktree must be removed on pending-kill"
        );
    let resumed = temp.path().join("subagent-resumed");
    fast_worktree::WorktreeBuilder::new(&repo, &resumed)
        .standalone(true)
        .create()
        .unwrap();
    std::fs::write(resumed.join("tracked.txt"), "source edit").unwrap();
    assert!(resumed.exists());
    run_promote_cancel_with_worktree(&resumed, false).await;
    assert!(
            resumed.exists(),
            "resumed subagent's reused worktree must be preserved (source owns it)"
        );
    assert_eq!(
            std::fs::read_to_string(resumed.join("tracked.txt")).unwrap(),
            "source edit",
            "the source's working state must be left untouched"
        );
}
fn test_model_entry(model_id: &str) -> crate::agent::config::ModelEntry {
    crate::agent::config::ModelEntry {
        info: crate::agent::config::ModelInfo {
            user_selectable: true,
            id: None,
            model: model_id.to_string(),
            base_url: String::new(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            auth_scheme: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(256_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            agent_type: crate::agent::config::default_agent_type(),
            inference_idle_timeout_secs: None,
            max_retries: None,
            hidden: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: crate::agent::config::LazinessDetectorPerModelConfig::default(),
        },
        api_key: None,
        env_key: None,
        auth_provider: None,
    }
}
#[test]
fn fresh_tool_model_accepts_only_visible_catalog_id() {
    let mut models = indexmap::IndexMap::new();
    models.insert("grow-3".to_string(), test_model_entry("grow-3-2025-02-15"));
    assert!(
            super::handle_request::task_model_override_error(
                Some("grow-3"),
                ModelOverrideProvenance::Tool,
                false,
                &models,
                false,
            )
            .is_none(),
            "key lookup should succeed"
        );
    assert!(
        super::handle_request::task_model_override_error(
            Some("grow-3-2025-02-15"),
            ModelOverrideProvenance::Tool,
            false,
            &models,
            false,
        )
        .is_some(),
        "routing model names are not catalog identities"
    );
}
#[test]
fn fresh_tool_model_rejects_unavailable_exact_key_over_visible_slug_collision() {
    let mut models = indexmap::IndexMap::new();
    models.insert("visible-alias".to_string(), test_model_entry("collision"));
    let mut unavailable_exact = test_model_entry("hidden-internal");
    unavailable_exact.info.hidden = true;
    models.insert("collision".to_string(), unavailable_exact);
    assert_eq!(
            super::handle_request::task_model_override_error(
                Some("collision"),
                ModelOverrideProvenance::Tool,
                false,
                &models,
                false,
            )
            .as_deref(),
            Some(
                "Unknown Task.model ID 'collision'. Valid model IDs: visible-alias. \
                 Omit `model` to inherit the parent model."
            ),
            "validation must inspect the unavailable exact-key entry selected by execution"
        );
}
#[test]
fn fresh_tool_model_rejects_routing_slug_even_when_a_visible_model_uses_it() {
    let mut models = indexmap::IndexMap::new();
    let mut unavailable_first = test_model_entry("shared-routing-slug");
    unavailable_first.info.user_selectable = false;
    models.insert("blocked-first".to_string(), unavailable_first);
    models.insert("visible-second".to_string(), test_model_entry("shared-routing-slug"));
    assert_eq!(
            super::handle_request::task_model_override_error(
                Some("shared-routing-slug"),
                ModelOverrideProvenance::Tool,
                false,
                &models,
                false,
            )
            .as_deref(),
            Some(
                "Unknown Task.model ID 'shared-routing-slug'. Valid model IDs: \
                 visible-second. Omit `model` to inherit the parent model."
            ),
            "validation must never select by routing slug"
        );
}
#[test]
fn fresh_tool_model_rejects_unknown_and_nonavailable_entries() {
    let mut models = indexmap::IndexMap::new();
    models.insert("zeta".to_string(), test_model_entry("zeta-internal"));
    let mut hidden = test_model_entry("hidden-internal");
    hidden.info.hidden = true;
    models.insert("hidden".to_string(), hidden);
    let mut not_selectable = test_model_entry("disabled-internal");
    not_selectable.info.user_selectable = false;
    models.insert("disabled".to_string(), not_selectable);
    models.insert(
        "alternate".to_string(),
        test_model_entry("alternate-internal"),
    );
    models.insert("alpha".to_string(), test_model_entry("alpha-internal"));
    for requested in [
        "stale-model",
        "hidden",
        "hidden-internal",
        "disabled",
        "disabled-internal",
    ] {
        let error = super::handle_request::task_model_override_error(
                Some(requested),
                ModelOverrideProvenance::Tool,
                false,
                &models,
                false,
            )
            .unwrap();
        assert_eq!(
                error,
                format!(
                    "Unknown Task.model ID '{requested}'. Valid model IDs: alpha, alternate, zeta. \
                     Omit `model` to inherit the parent model."
                )
            );
        assert!(!error.contains("grow models"));
    }
    assert!(
            super::handle_request::task_model_override_error(
                Some("alternate"),
                ModelOverrideProvenance::Tool,
                false,
                &models,
                false,
            )
            .is_none(),
            "every explicitly configured provider model is selectable"
        );
}
#[test]
fn fresh_tool_model_reports_empty_valid_list() {
    let empty = indexmap::IndexMap::new();
    assert_eq!(
            super::handle_request::task_model_override_error(
                Some("anything"),
                ModelOverrideProvenance::Tool,
                false,
                &empty,
                false,
            )
            .as_deref(),
            Some(
                "Unknown Task.model ID 'anything'. No valid model IDs are currently \
                 available. Omit `model` to inherit the parent model."
            )
        );
}
#[test]
fn resumed_tool_model_override_is_ignored() {
    let empty = indexmap::IndexMap::new();
    assert!(
            super::handle_request::task_model_override_error(
                Some("stale-model"),
                ModelOverrideProvenance::Tool,
                true,
                &empty,
                false,
            )
            .is_none(),
            "resume must preserve source-model pinning"
        );
}
#[test]
fn harness_model_override_defers_to_runtime_catalog_resolution() {
    let empty = indexmap::IndexMap::new();
    assert!(
            super::handle_request::task_model_override_error(
                Some("internal-model"),
                ModelOverrideProvenance::Harness,
                false,
                &empty,
                false,
            )
            .is_none(),
            "internal routes bypass the model-facing Task validator and are resolved by the strict runtime catalogue boundary"
        );
}
#[test]
fn normalize_forked_context_empty_parent() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![ConversationItem::system("sys prompt")];
    let (conv, prefix_len) = crate::agent::subagent::resolution::context::normalize_forked_context(
        items,
    );
    assert_eq!(conv.len(), 1);
    assert_eq!(prefix_len, 1);
    assert!(matches!(conv[0], ConversationItem::System(_)));
}
#[test]
fn normalize_forked_context_short_conversation() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("hello"),
            ConversationItem::assistant("hi back"),
        ];
    let (conv, prefix_len) = crate::agent::subagent::resolution::context::normalize_forked_context(
        items,
    );
    assert_eq!(prefix_len, 2);
    assert_eq!(conv.len(), 2);
    assert!(matches!(conv[0], ConversationItem::System(_)));
    if let ConversationItem::User(u) = &conv[1] {
        let text = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(
                text.contains("<background_context>"),
                "should have background tag"
            );
        assert!(
                text.contains("[User]: hello"),
                "should include parent user message"
            );
        assert!(
                text.contains("[Assistant]: hi back"),
                "should include parent assistant message"
            );
    } else {
        panic!("expected User message at position 1");
    }
}
fn test_sampling_config(model_slug: &str) -> sampling_types::SamplingConfig {
    use std::num::NonZeroU64;
    sampling_types::SamplingConfig {
        base_url: "https://api.test/v1".to_string(),
        model: model_slug.to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(256_000).expect("non-zero context window"),
        reasoning_effort: None,
        stream_tool_calls: None,
    }
}
fn spawn_test_parent_chat_state(model_slug: &str) -> chat_state::ChatStateHandle {
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    chat_state::ChatStateActor::spawn(
        vec![],
        test_sampling_config(model_slug),
        Box::new(chat_state::NullTimelinePersistence),
        event_tx,
        token,
    )
}
mod rest;
