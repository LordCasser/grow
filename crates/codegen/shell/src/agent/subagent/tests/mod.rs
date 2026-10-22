#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
use super::handle_request::{
    canonical_total_tokens, child_task_prompt_identity, record_subagent_usage,
    usage_is_incomplete,
};
use crate::test_support::lsp_runtime::{
    DummyLspDispatch, ctx_with_toggle, test_gateway_with_receiver,
};
use tools::implementations::grow_build::task::coordinator::{
    ChildCompletion, CompletionDisposition,
};

#[test]
fn delegated_task_is_a_child_user_turn() {
    let (origin, turn_kind) = child_task_prompt_identity();

    assert_eq!(origin, crate::session::PromptOrigin::User);
    assert_eq!(turn_kind, crate::session::TurnKind::User);
    assert_eq!(origin.turn_identity(turn_kind).turn_kind, "user");
}

#[test]
fn restored_allowed_input_prevents_a_duplicate_child_prompt() {
    let payload_ref = chat_state::InputPayloadRef {
        blake3: "input-hash".into(),
        bytes: 1,
    };
    assert!(!super::handle_request::has_restored_pending_input(&[]));
    assert!(super::handle_request::has_restored_pending_input(&[
        chat_state::PendingAllowedInput {
            input_id: "fifo-input".into(),
            intent: chat_state::InputIntent::Prompt,
            payload_ref: payload_ref.clone(),
            route: chat_state::InputRoute::Fifo,
        },
    ]));
    // Startup restoration durably reroutes an unresolved Steer to FIFO before
    // placing it in pending_inputs, so it also owns the next prompt.
    assert!(super::handle_request::has_restored_pending_input(&[
        chat_state::PendingAllowedInput {
            input_id: "steer-input".into(),
            intent: chat_state::InputIntent::Steer,
            payload_ref,
            route: chat_state::InputRoute::Steer {
                target_turn: chat_state::TurnId(7),
            },
        },
    ]));
}

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
        Some("child head"),
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
        Some("child head"),
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
            None,
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
                None,
            )
            .is_err()
        );
    }
}

#[test]
fn a_new_child_still_requires_a_rendered_system_head() {
    let mut conversation = Vec::new();
    let mut prefix_len = None;

    assert!(
        seed_child_system_head(
            &InitialContextSource::New,
            false,
            &mut conversation,
            &mut prefix_len,
            None,
        )
        .is_err()
    );
}

#[test]
fn missing_non_worktree_cwd_falls_back_but_unsafe_paths_do_not() {
    let parent = tempfile::tempdir().expect("parent workspace");
    let missing = parent.path().join("removed-child-dir");
    assert!(
        validate_resume_non_worktree_cwd(
            missing.to_string_lossy().as_ref(),
            parent.path(),
        )
        .is_ok()
    );

    let file = parent.path().join("not-a-directory");
    std::fs::write(&file, b"file").expect("fixture file");
    assert!(
        validate_resume_non_worktree_cwd(file.to_string_lossy().as_ref(), parent.path()).is_err()
    );

    let outside = tempfile::tempdir().expect("outside directory");
    assert!(
        validate_resume_non_worktree_cwd(
            outside.path().to_string_lossy().as_ref(),
            parent.path(),
        )
        .is_err()
    );
}

#[tokio::test]
async fn cancellation_waits_for_the_child_prompt_terminal() {
    let (prompt_tx, prompt_rx) = oneshot::channel();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut wait = Box::pin(await_subagent_turn_or_cancellation(prompt_rx, cancel));

    assert!(
        tokio::time::timeout(std::time::Duration::ZERO, &mut wait)
            .await
            .is_err(),
        "cancellation alone must not synthesize a terminal result"
    );
    prompt_tx
        .send(Ok(crate::session::commands::PromptTurnOk {
            stop_reason: acp::StopReason::Cancelled,
            total_tokens: 7,
            turn_snapshot: None,
            completion_kind: PromptCompletionKind::Cancelled {
                category: Some(CancellationCategory::MidTurnAbort),
                context: None,
            },
            structured_output: None,
            usage: None,
        }))
        .expect("settled prompt terminal");
    let terminal = wait.await.expect("prompt sender remained live").unwrap();
    assert!(matches!(
        terminal.completion_kind,
        PromptCompletionKind::Cancelled {
            category: Some(CancellationCategory::MidTurnAbort),
            ..
        }
    ));
}

#[test]
fn cancelled_canonical_link_remains_resume_eligible() {
    let spawn_fact = recovery_spawn("cancelled-source", "cancelled-child");
    let mut parent = chat_state::Timeline::default();
    let spawn = parent
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn_fact.clone())),
        ))
        .expect("parent spawn");
    let mut child = chat_state::Timeline::default();
    child
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: "parent-session".into(),
                parent_spawn_seq: spawn.seq.get(),
                subagent_id: spawn_fact.subagent_id.clone(),
                security_parent_session_id: spawn_fact.security_parent_session_id.clone(),
                context_source: spawn_fact.context_source,
                source_ref: spawn_fact.source_ref.clone(),
                normalized: spawn_fact.context_normalized,
            },
        ))
        .expect("child seed");
    let result = child
        .record(chat_state::TimelineEventKind::SubagentResult(
            chat_state::SubagentResultEvent {
                subagent_id: spawn_fact.subagent_id.clone(),
                outcome: chat_state::SubagentOutcome::Cancelled,
                duration_ms: 41,
                tool_calls: 2,
                turns: 1,
                tokens_used: 73,
                error: Some("provider quota stopped the root Goal".into()),
                output_ref: None,
                planning_summary: None,
            },
        ))
        .expect("child result");
    let terminal = chat_state::SubagentTerminalEvent {
        subagent_id: spawn_fact.subagent_id.clone(),
        child_session_id: spawn_fact.child_session_id.clone(),
        outcome: chat_state::SubagentOutcome::Cancelled,
        duration_ms: 41,
        tool_calls: 2,
        turns: 1,
        tokens_used: 73,
        error: Some("provider quota stopped the root Goal".into()),
        result_ref: Some(chat_state::TimelineRangeRef {
            timeline_id: spawn_fact.child_session_id.clone(),
            first_seq: result.seq.get(),
            last_seq: result.seq.get(),
        }),
        snapshot_ref: None,
    };

    child
        .validate_subagent_result_link(
            "parent-session",
            spawn.seq,
            &spawn_fact,
            &terminal,
        )
        .expect("cancelled result link is canonical");
    parent
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(terminal),
        ))
        .expect("parent terminal");
    let source = resume_source_from_timeline(&parent, "cancelled-source")
        .expect("cancelled lifecycle projects as a resume source");
    assert_eq!(source.child_session_id, "cancelled-child");
    assert_eq!(source.model_id, "model");
}

#[test]
fn planning_result_reference_requires_the_exact_child_terminal_and_result_event() {
    let lifecycle_root_id = "lifecycle-root";
    let direct_parent_id = "nested-direct-parent";
    assert!(validate_child_session_identity_fields(
        "planning-child",
        Some(lifecycle_root_id),
        Some("subagent"),
        "planning-child",
        lifecycle_root_id,
    )
    .is_ok());
    assert!(validate_child_session_identity_fields(
        "planning-child",
        Some(direct_parent_id),
        Some("subagent"),
        "planning-child",
        lifecycle_root_id,
    )
    .is_err());
    let mut spawn_fact = recovery_spawn("planning-result", "planning-child");
    spawn_fact.security_parent_session_id = direct_parent_id.into();
    let mut root = chat_state::Timeline::default();
    let spawn_event = root
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn_fact.clone())),
        ))
        .expect("parent spawn");

    let mut child = chat_state::Timeline::default();
    child
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: lifecycle_root_id.into(),
                parent_spawn_seq: spawn_event.seq.get(),
                subagent_id: spawn_fact.subagent_id.clone(),
                security_parent_session_id: direct_parent_id.into(),
                context_source: spawn_fact.context_source,
                source_ref: spawn_fact.source_ref.clone(),
                normalized: spawn_fact.context_normalized,
            },
        ))
        .expect("child seed");
    let result_event = child
        .record(chat_state::TimelineEventKind::SubagentResult(
            chat_state::SubagentResultEvent {
                subagent_id: spawn_fact.subagent_id.clone(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 12,
                tool_calls: 2,
                turns: 1,
                tokens_used: 34,
                error: None,
                output_ref: None,
                planning_summary: None,
            },
        ))
        .expect("canonical child result");
    let result_ref = chat_state::TimelineRangeRef {
        timeline_id: spawn_fact.child_session_id.clone(),
        first_seq: result_event.seq.get(),
        last_seq: result_event.seq.get(),
    };
    root
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: spawn_fact.subagent_id.clone(),
                child_session_id: spawn_fact.child_session_id.clone(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 12,
                tool_calls: 2,
                turns: 1,
                tokens_used: 34,
                error: None,
                result_ref: Some(result_ref.clone()),
                snapshot_ref: None,
            }),
        ))
        .expect("parent terminal");

    validate_planning_result_facts(
        direct_parent_id,
        lifecycle_root_id,
        &spawn_fact.subagent_id,
        &spawn_fact.child_session_id,
        &result_ref,
        &root,
        &child,
    )
    .expect("nested result validation uses the root seed and direct security parent");

    let mut wrong_range = result_ref.clone();
    wrong_range.first_seq += 1;
    wrong_range.last_seq += 1;
    assert!(validate_planning_result_facts(
        direct_parent_id,
        lifecycle_root_id,
        &spawn_fact.subagent_id,
        &spawn_fact.child_session_id,
        &wrong_range,
        &root,
        &child,
    )
    .is_err());

    let mut wrong_child = result_ref.clone();
    wrong_child.timeline_id = "another-child".into();
    assert!(validate_planning_result_facts(
        direct_parent_id,
        lifecycle_root_id,
        &spawn_fact.subagent_id,
        &spawn_fact.child_session_id,
        &wrong_child,
        &root,
        &child,
    )
    .is_err());
    assert!(validate_planning_result_facts(
        "sibling-parent",
        lifecycle_root_id,
        &spawn_fact.subagent_id,
        &spawn_fact.child_session_id,
        &result_ref,
        &root,
        &child,
    )
    .is_err());
    assert!(validate_planning_result_facts(
        direct_parent_id,
        "different-lifecycle-root",
        &spawn_fact.subagent_id,
        &spawn_fact.child_session_id,
        &result_ref,
        &root,
        &child,
    )
    .is_err());
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
                },
                forward_to_gateway: true,
                restored: false,
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
/// The parent actor assigns the lifecycle event ID at publication. The child
/// only queues one metadata-only command and no second gateway copy.
#[tokio::test]
async fn emit_subagent_notification_sends_one_metadata_only_parent_command() {
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
            output: Some("large child answer".into()),
        },
        Some(&cmd_tx),
        false,
    );
    match cmd_rx.try_recv().expect("parent command must fire") {
        SessionCommand::GrowSessionNotification {
            notification,
            forward_to_gateway: true,
            restored: false,
        } => {
            assert!(matches!(
                notification.update,
                SessionUpdate::SubagentFinished { output: None, .. }
            ));
            assert!(notification.meta.is_none(), "parent actor stamps after hooks");
        }
        _ => panic!("expected GrowSessionNotification"),
    }
    assert!(gateway_rx.try_recv().is_err());

    emit_subagent_notification(
        &gateway,
        "parent-sess",
        SessionUpdate::SubagentFinished {
            subagent_id: "sa-2".into(),
            child_session_id: "child-2".into(),
            status: "completed".into(),
            error: None,
            tool_calls: 0,
            turns: 0,
            duration_ms: 5,
            tokens_used: 0,
            output: Some("large child answer".into()),
        },
        None,
        false,
    );
    let acp_transport::AcpClientMessage::ExtNotification(message) =
        gateway_rx.try_recv().expect("direct fallback must forward")
    else {
        panic!("expected Grow extension notification");
    };
    let fallback: serde_json::Value = serde_json::from_str(message.request.params.get()).unwrap();
    assert!(fallback["_meta"]["eventId"]
        .as_str()
        .is_some_and(|id| id.starts_with("parent-sess-")));
    assert!(fallback["update"]["output"].is_null());
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

        origin_tool_call_id: None,
        prompt: String::new(),
        description: "explore".into(),
        subagent_type: "general-purpose".into(),
        parent_session_id: "parent".into(),
        parent_step_id: None,
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
        let expected_from_spawn = completion_data.expected_parent_step_id.clone();
        let admission = admit_completion_receipt_before_result(request, result, completion_data);
        tokio::pin!(admission);
        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("planning result command"),
            () = &mut admission => panic!("terminal result outran planning result admission"),
        };
        let SessionCommand::PlanningTaskResult {
            expected_parent_step_id,
            respond_to,
            ..
        } = command
        else {
            panic!("expected direct-parent planning result");
        };
        assert_eq!(expected_parent_step_id, expected_from_spawn);
        respond_to.send(Ok(())).unwrap();
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
    ctx.parent_cmd_tx = Some(cmd_tx.clone());
    ctx.completion_cmd_tx = Some(cmd_tx);
    let mut request = auto_wake_test_request("sa-1");
    request.parent_step_id = Some("step-1".into());
    let result = SubagentResult {
        success: true,
        subagent_id: "sa-1".into(),
        child_session_id: "sa-1".into(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.expected_parent_step_id = Some("step-1".into());
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
async fn missing_direct_completion_route_does_not_fall_back_to_lifecycle_root() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (root_tx, mut root_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(root_tx);
    ctx.completion_cmd_tx = None;
    let request = auto_wake_test_request("nested-child");
    let result = SubagentResult {
        success: true,
        subagent_id: request.id.clone(),
        child_session_id: request.id.clone(),
        ..Default::default()
    };
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    completion_data.mark_terminal_committed();

    admit_completion_receipt_before_result(&request, &result, &mut completion_data).await;

    assert!(!completion_data.completion_receipt_admitted);
    assert!(completion_data.receipt_required);
    assert!(!completion_data.terminal_committed());
    assert!(matches!(
        completion_data.finalization_error.as_ref().map(|failure| failure.stage),
        Some(FinalizationStage::Receipt)
    ));
    assert!(root_rx.try_recv().is_err());
}

#[test]
fn recovery_completion_route_requires_the_exact_direct_parent() {
    let (root_tx, _root_rx) = mpsc::unbounded_channel::<SessionCommand>();
    let (direct_tx, _direct_rx) = mpsc::unbounded_channel::<SessionCommand>();
    let root = "root-session";
    let mut spawn = recovery_spawn("nested-child", "child-session");
    spawn.security_parent_session_id = "direct-session".into();
    let mut routes = HashMap::from([("direct-session".into(), direct_tx)]);

    let route = completion_command_route(root, &spawn, Some(&root_tx), &routes).unwrap();
    assert!(std::ptr::eq(route, routes.get("direct-session").unwrap()));
    routes.clear();
    assert!(completion_command_route(root, &spawn, Some(&root_tx), &routes).is_none());

    spawn.security_parent_session_id = root.into();
    assert!(completion_command_route(root, &spawn, Some(&root_tx), &routes).is_some());
}

#[tokio::test]
async fn orphan_recovery_keeps_planning_result_pending_without_direct_parent() {
    let mut spawn = recovery_spawn("nested-child", "child-session");
    spawn.security_parent_session_id = "unloaded-parent".into();
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

    assert!(!repair_subagent_planning_result(None, &spawn, &terminal).await);
}

#[tokio::test]
async fn loop_completion_uses_the_same_acknowledged_durable_receipt() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<SessionCommand>();
    ctx.parent_cmd_tx = Some(cmd_tx.clone());
    ctx.completion_cmd_tx = Some(cmd_tx);
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
    ctx.parent_cmd_tx = Some(cmd_tx.clone());
    ctx.completion_cmd_tx = Some(cmd_tx);
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
    ctx.parent_cmd_tx = Some(cmd_tx.clone());
    ctx.completion_cmd_tx = Some(cmd_tx);
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
            command = cmd_rx.recv() => command.expect("planning result command"),
            () = &mut admission => panic!("completion returned before planning result admission"),
        };
        let SessionCommand::PlanningTaskResult { respond_to, .. } = command else {
            panic!("expected direct-parent planning result");
        };
        respond_to.send(Ok(())).unwrap();
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
    assert!(persist_subagent_output(&session, &empty).unwrap().is_none());
    let failed = SubagentResult {
        success: false,
        output: std::sync::Arc::from("partial"),
        ..Default::default()
    };
    assert!(persist_subagent_output(&session, &failed).unwrap().is_none());
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
        origin_tool_call_id: None,
        security_parent_session_id: "parent-session".into(),
            parent_step_id: None,
        subagent_type: "review".into(),
        description: "recover child".into(),
        prompt: "finish".into(),
        execution_recipe: serde_json::json!({}),
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
    let journal = crate::session::storage::JsonlStorageAdapter::encode_journal(
        child.events(),
        &[],
    )
    .unwrap();
    crate::session::storage::write_bytes_atomic(
        &dir.join(crate::session::storage::TIMELINE_FILE),
        &journal,
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
    let fallback = result_from_inspection(&spawn, Some(&inspection), 999).unwrap();
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
            planning_summary: None,
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
                planning_summary: None,
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
            chat_state::SubagentEvent::Spawned(Box::new(spawn.clone())),
        ))
        .unwrap();
    let journal = crate::session::storage::JsonlStorageAdapter::encode_journal(
        timeline.events(),
        &[],
    )
    .unwrap();
    crate::session::storage::write_bytes_atomic(
        &session_dir.join(crate::session::storage::TIMELINE_FILE),
        &journal,
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

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn nested_planning_result_and_summary_follow_direct_parent_edges() {
    const CHILD_MARKER: &str = "GROW_NESTED_PLANNING_RESULT_ISOLATED_HOME_CHILD";
    const TEST_NAME: &str = concat!(
        module_path!(),
        "::nested_planning_result_and_summary_follow_direct_parent_edges"
    );
    if std::env::var_os(CHILD_MARKER).is_none() {
        // `config::grow_home()` is process-cached. Run this test in a fresh
        // process so its production storage readers and writer share the same
        // isolated root before that cache is initialized.
        let home = tempfile::tempdir().unwrap();
        let test_name = TEST_NAME.strip_prefix("shell::").unwrap_or(TEST_NAME);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env("GROW_HOME", home.path())
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run nested planning fixture with an isolated Grow home");
        assert!(
            output.status.success(),
            "isolated nested planning test failed\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
    use tool_types::planning::{PlanLink, PlanPriority, PlanStep, PlanStepStatus};

    async fn persist_kind(
        actor: &crate::session::actor::SessionActor,
        storage: &JsonlStorageAdapter,
        info: &SessionInfo,
        kind: chat_state::TimelineEventKind,
    ) -> chat_state::TimelineEvent {
        let event = actor
            .chat_state_handle
            .record_timeline_event_durably(kind)
            .await
            .expect("actor timeline event is durable");
        storage
            .append_timeline_event(info, &event)
            .await
            .expect("same canonical event is stored for cold readers");
        event
    }

    async fn accept_and_receipt(
        parent: &crate::session::actor::SessionActor,
        parent_info: &SessionInfo,
        storage: &JsonlStorageAdapter,
        cmd_tx: &mpsc::UnboundedSender<SessionCommand>,
        cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
        request: &SubagentRequest,
        result: &SubagentResult,
        result_ref: chat_state::TimelineRangeRef,
        parent_step_id: &str,
    ) {
        let mut completion = ShellCompletionData {
            completion_cmd_tx: Some(cmd_tx.clone()),
            child_result_ref: Some(result_ref.clone()),
            expected_parent_step_id: Some(parent_step_id.to_owned()),
            ..ShellCompletionData::not_admitted()
        };
        let mut admission = Box::pin(admit_completion_receipt_before_result(
            request,
            result,
            &mut completion,
        ));

        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("planning result reaches direct parent"),
            () = &mut admission => panic!("completion passed its direct-parent result gate"),
        };
        let SessionCommand::PlanningTaskResult {
            task_id,
            expected_parent_step_id,
            result_ref: admitted_ref,
            respond_to,
        } = command else {
            panic!("expected a planning result command");
        };
        assert_eq!(task_id, request.id);
        assert_eq!(expected_parent_step_id.as_deref(), Some(parent_step_id));
        assert_eq!(admitted_ref.as_ref(), Some(&result_ref));
        parent
            .accept_planning_task_result(
                &task_id,
                expected_parent_step_id.as_deref(),
                admitted_ref.as_ref(),
            )
            .await
            .expect("direct parent accepts the canonical child result");
        let events = parent.chat_state_handle.timeline_events().await.unwrap();
        storage
            .append_timeline_event(parent_info, events.last().expect("accepted plan event"))
            .await
            .expect("acceptance is available to the next canonical reader");
        respond_to.send(Ok(())).expect("planning admission ACK");

        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("receipt reaches the same direct parent"),
            () = &mut admission => panic!("completion passed its notification receipt gate"),
        };
        let SessionCommand::ReceiveNotification {
            source,
            source_version,
            body,
            respond_to: Some(receipt_ack),
            ..
        } = command else {
            panic!("expected a completion notification command");
        };
        assert!(matches!(
            &source,
            chat_state::NotificationSource::SubagentCompleted { subagent_id, .. }
                if subagent_id == &request.id
        ));
        let payload = chat_state::NotificationPayloadRef {
            blake3: blake3::hash(body.as_bytes()).to_hex().to_string(),
            bytes: body.len() as u64,
        };
        let receipt = parent
            .chat_state_handle
            .receive_notification_durably(
                parent_info.id.0.to_string(),
                source,
                source_version,
                payload,
            )
            .await
            .expect("direct parent's notification inbox commits the receipt");
        storage
            .append_timeline_event(parent_info, &receipt)
            .await
            .expect("notification receipt remains in the direct parent's canonical ledger");
        let chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received { id, .. },
        ) = receipt.kind
        else {
            panic!("expected a notification receipt fact");
        };
        receipt_ack.send(Ok(id)).expect("receipt admission ACK");
        admission.await;
        assert!(completion.terminal_committed());
    }

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let root_info = SessionInfo {
                id: acp::SessionId::new("nested-plan-root"),
                cwd: "/nested-plan-workspace".into(),
            };
            let child_info = SessionInfo {
                id: acp::SessionId::new("nested-plan-child"),
                cwd: "/nested-plan-workspace".into(),
            };
            let grandchild_info = SessionInfo {
                id: acp::SessionId::new("nested-plan-grandchild"),
                cwd: "/nested-plan-workspace".into(),
            };
            let storage = JsonlStorageAdapter::new();
            for (info, parent) in [
                (&root_info, None),
                (&child_info, Some(root_info.id.0.to_string())),
                (&grandchild_info, Some(root_info.id.0.to_string())),
            ] {
                let mut summary = crate::session::persistence::Summary::new(
                    info,
                    crate::session::persistence::default_model_id(),
                )
                .unwrap();
                if let Some(parent) = parent {
                    summary.parent_session_id = Some(parent);
                    summary.session_kind = Some("subagent".into());
                }
                storage
                    .init_session_with_summary(info, summary, Vec::new(), Default::default(), Vec::new())
                    .await
                    .unwrap();
            }
            let (mut root, _) = crate::session::actor::build_actor_for_subagent_test().await;
            let (mut child, _) = crate::session::actor::build_actor_for_subagent_test().await;
            let (mut grandchild, _) = crate::session::actor::build_actor_for_subagent_test().await;
            for (actor, info) in [
                (&mut root, &root_info),
                (&mut child, &child_info),
                (&mut grandchild, &grandchild_info),
            ] {
                std::sync::Arc::get_mut(actor)
                    .expect("test actor is uniquely owned")
                    .session_info
                    .id = info.id.clone();
                for event in actor.chat_state_handle.timeline_events().await.unwrap() {
                    storage.append_timeline_event(info, &event).await.unwrap();
                }
            }

            let mut root_spawn_child = recovery_spawn("task-child", &child_info.id.0);
            root_spawn_child.security_parent_session_id = root_info.id.0.to_string();
            root_spawn_child.parent_step_id = Some("root-step".into());
            let child_spawned = persist_kind(
                &root,
                &storage,
                &root_info,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    Box::new(root_spawn_child.clone()),
                )),
            )
            .await;

            let mut root_spawn_grandchild = recovery_spawn("task-grandchild", &grandchild_info.id.0);
            root_spawn_grandchild.security_parent_session_id = child_info.id.0.to_string();
            root_spawn_grandchild.parent_step_id = Some("child-step".into());
            let grandchild_spawned = persist_kind(
                &root,
                &storage,
                &root_info,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    Box::new(root_spawn_grandchild.clone()),
                )),
            )
            .await;

            for (actor, info, spawn, spawn_seq) in [
                (&child, &child_info, &root_spawn_child, child_spawned.seq.get()),
                (
                    &grandchild,
                    &grandchild_info,
                    &root_spawn_grandchild,
                    grandchild_spawned.seq.get(),
                ),
            ] {
                persist_kind(
                    actor,
                    &storage,
                    info,
                    chat_state::TimelineEventKind::SubagentSeed(
                        chat_state::SubagentSeedEvent {
                            parent_timeline_id: root_info.id.0.to_string(),
                            parent_spawn_seq: spawn_seq,
                            subagent_id: spawn.subagent_id.clone(),
                            security_parent_session_id: spawn.security_parent_session_id.clone(),
                            context_source: spawn.context_source,
                            source_ref: spawn.source_ref.clone(),
                            normalized: spawn.context_normalized,
                        },
                    ),
                )
                .await;
            }

            let step = |id: &str, content: &str| PlanStep {
                id: id.into(),
                content: content.into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            };
            let mut root_plan = tool_types::LocalPlan::new(root_info.id.0.to_string(), "root-work");
            root_plan.steps.push(step("root-step", "accept child deliverable"));
            root_plan.links.push(PlanLink {
                id: "task-child".into(),
                parent_step_id: "root-step".into(),
                child_session_id: child_info.id.0.to_string(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            persist_kind(
                &root,
                &storage,
                &root_info,
                chat_state::TimelineEventKind::Planning(chat_state::timeline::PlanningEvent {
                    previous: None,
                    plan: Box::new(root_plan),
                }),
            )
            .await;

            let mut child_plan = tool_types::LocalPlan::new(child_info.id.0.to_string(), "child-work");
            child_plan.steps.push(step("child-step", "accept grandchild deliverable"));
            child_plan.links.push(PlanLink {
                id: "task-grandchild".into(),
                parent_step_id: "child-step".into(),
                child_session_id: grandchild_info.id.0.to_string(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            persist_kind(
                &child,
                &storage,
                &child_info,
                chat_state::TimelineEventKind::Planning(chat_state::timeline::PlanningEvent {
                    previous: None,
                    plan: Box::new(child_plan),
                }),
            )
            .await;
            let mut grandchild_plan =
                tool_types::LocalPlan::new(grandchild_info.id.0.to_string(), "grandchild-work");
            grandchild_plan.steps.push(step("leaf-step", "produce a bounded result"));
            persist_kind(
                &grandchild,
                &storage,
                &grandchild_info,
                chat_state::TimelineEventKind::Planning(chat_state::timeline::PlanningEvent {
                    previous: None,
                    plan: Box::new(grandchild_plan),
                }),
            )
            .await;

            let mut grandchild_result = SubagentResult {
                success: true,
                duration_ms: 1,
                turns: 1,
                subagent_id: root_spawn_grandchild.subagent_id.clone(),
                child_session_id: grandchild_info.id.0.to_string(),
                output: std::sync::Arc::from("grandchild canonical result"),
                ..Default::default()
            };
            let grandchild_result_ref = record_child_result(
                &grandchild.chat_state_handle,
                &mut grandchild_result,
                None,
            )
            .await
            .expect("canonical result contains the grandchild's own plan summary");
            let grandchild_events = grandchild.chat_state_handle.timeline_events().await.unwrap();
            storage
                .append_timeline_event(
                    &grandchild_info,
                    grandchild_events.last().expect("grandchild result event"),
                )
                .await
                .unwrap();
            persist_kind(
                &root,
                &storage,
                &root_info,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    chat_state::SubagentTerminalEvent {
                        subagent_id: root_spawn_grandchild.subagent_id.clone(),
                        child_session_id: grandchild_info.id.0.to_string(),
                        outcome: chat_state::SubagentOutcome::Completed,
                        duration_ms: 1,
                        tool_calls: 0,
                        turns: 1,
                        tokens_used: 0,
                        error: None,
                        result_ref: Some(grandchild_result_ref.clone()),
                        snapshot_ref: None,
                    },
                )),
            )
            .await;

            let (grandchild_cmd_tx, mut grandchild_cmd_rx) = mpsc::unbounded_channel();
            accept_and_receipt(
                &child,
                &child_info,
                &storage,
                &grandchild_cmd_tx,
                &mut grandchild_cmd_rx,
                &SubagentRequest {
                    id: root_spawn_grandchild.subagent_id.clone(),
                    parent_session_id: child_info.id.0.to_string(),
                    parent_step_id: Some("child-step".into()),
                    surface_completion: true,
                    owner: SubagentOwner::Task,
                    ..auto_wake_test_request("task-grandchild")
                },
                &grandchild_result,
                grandchild_result_ref,
                "child-step",
            )
            .await;
            assert!(child
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan
                .links
                .iter()
                .any(|link| link.id == "task-grandchild" && link.terminal));

            // Receiving a child result does not accept its required delivery.
            // The direct parent records that separate business decision in
            // its own plan before producing its summary for the root.
            let snapshot = child.chat_state_handle.planning_state().await.unwrap().unwrap();
            let mut accepted_plan = snapshot.plan;
            accepted_plan.reviews.push(tool_types::DeliveryReview {
                link_id: "task-grandchild".into(),
                result_id: accepted_plan.links[0].result_id.clone(),
                reviewed_source_revision: accepted_plan.revision,
                decision: tool_types::ReviewDecision::Accepted,
                reason: Some("verified grandchild canonical result".into()),
                verification_refs: Vec::new(),
            });
            persist_kind(
                &child,
                &storage,
                &child_info,
                chat_state::TimelineEventKind::Planning(chat_state::timeline::PlanningEvent {
                    previous: Some(snapshot.seq),
                    plan: Box::new(accepted_plan),
                }),
            ).await;

            let mut child_result = SubagentResult {
                success: true,
                duration_ms: 2,
                tool_calls: 1,
                turns: 1,
                subagent_id: root_spawn_child.subagent_id.clone(),
                child_session_id: child_info.id.0.to_string(),
                output: std::sync::Arc::from("child accepted the grandchild deliverable"),
                ..Default::default()
            };
            let child_result_ref = record_child_result(
                &child.chat_state_handle,
                &mut child_result,
                None,
            )
            .await
            .expect("child canonical result contains its updated bounded plan summary");
            let child_events = child.chat_state_handle.timeline_events().await.unwrap();
            storage
                .append_timeline_event(&child_info, child_events.last().expect("child result"))
                .await
                .unwrap();
            persist_kind(
                &root,
                &storage,
                &root_info,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    chat_state::SubagentTerminalEvent {
                        subagent_id: root_spawn_child.subagent_id.clone(),
                        child_session_id: child_info.id.0.to_string(),
                        outcome: chat_state::SubagentOutcome::Completed,
                        duration_ms: 2,
                        tool_calls: 1,
                        turns: 1,
                        tokens_used: 0,
                        error: None,
                        result_ref: Some(child_result_ref.clone()),
                        snapshot_ref: None,
                    },
                )),
            )
            .await;

            let summary = durable_child_planning_summary(
                &root_spawn_child,
                &chat_state::SubagentTerminalEvent {
                    subagent_id: root_spawn_child.subagent_id.clone(),
                    child_session_id: child_info.id.0.to_string(),
                    outcome: chat_state::SubagentOutcome::Completed,
                    duration_ms: 2,
                    tool_calls: 1,
                    turns: 1,
                    tokens_used: 0,
                    error: None,
                    result_ref: Some(child_result_ref.clone()),
                    snapshot_ref: None,
                },
            )
            .expect("root reads the immutable canonical child result summary")
            .expect("child result includes a local plan summary");
            assert_eq!(summary.scope_id, "child-work");
            assert!(summary.unresolved_required_deliveries.is_empty());

            let (child_cmd_tx, mut child_cmd_rx) = mpsc::unbounded_channel();
            accept_and_receipt(
                &root,
                &root_info,
                &storage,
                &child_cmd_tx,
                &mut child_cmd_rx,
                &SubagentRequest {
                    id: root_spawn_child.subagent_id.clone(),
                    parent_session_id: root_info.id.0.to_string(),
                    parent_step_id: Some("root-step".into()),
                    surface_completion: true,
                    owner: SubagentOwner::Task,
                    ..auto_wake_test_request("task-child")
                },
                &child_result,
                child_result_ref,
                "root-step",
            )
            .await;

            let notification_sources = |events: Vec<chat_state::TimelineEvent>| {
                events.into_iter().filter_map(|event| match event.kind {
                    chat_state::TimelineEventKind::Notification(
                        chat_state::NotificationEvent::Received { source, .. },
                    ) => Some(source),
                    _ => None,
                }).collect::<Vec<_>>()
            };
            let root_receipts = notification_sources(root.chat_state_handle.timeline_events().await.unwrap());
            let child_receipts = notification_sources(child.chat_state_handle.timeline_events().await.unwrap());
            assert!(root_receipts.iter().any(|source| matches!(
                source,
                chat_state::NotificationSource::SubagentCompleted { subagent_id, .. }
                    if subagent_id == "task-child"
            )));
            assert!(!root_receipts.iter().any(|source| matches!(
                source,
                chat_state::NotificationSource::SubagentCompleted { subagent_id, .. }
                    if subagent_id == "task-grandchild"
            )), "root must not receive a second grandchild completion receipt");
            assert!(child_receipts.iter().any(|source| matches!(
                source,
                chat_state::NotificationSource::SubagentCompleted { subagent_id, .. }
                    if subagent_id == "task-grandchild"
            )), "grandchild completion belongs to its direct parent");
        })
        .await;
}

#[tokio::test]
async fn recovery_repairs_receipt_after_durable_parent_terminal() {
    let parent_dir = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-receipt-{}", uuid::Uuid::now_v7());
    let mut spawn = recovery_spawn(
        "sa-receipt",
        &format!("child-receipt-{}", uuid::Uuid::now_v7()),
    );
    spawn.security_parent_session_id = parent_id.clone();
    spawn.parent_step_id = Some("step-recovery".into());
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
                true,
                &backend,
                &parent_id,
                &parent,
                None,
                &gateway,
                Some(&cmd_tx),
                &Default::default(),
            )
            .await;
        }
    });

    let SessionCommand::PlanningTaskResult {
        expected_parent_step_id,
        respond_to,
        ..
    } =
        cmd_rx.recv().await.expect("recovery planning result command")
    else {
        panic!("expected direct-parent planning result");
    };
    assert_eq!(expected_parent_step_id.as_deref(), Some("step-recovery"));
    respond_to.send(Ok(())).unwrap();
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
    let occurrence_id = format!("independent-notification-hook-{}", uuid::Uuid::now_v7());
    let hook_event = parent
        .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(
            chat_state::HookEvent::Triggered {
                occurrence_id: occurrence_id.clone(),
                event: chat_state::HookEventType::Notification,
                gate: chat_state::HookGateKind::Observe,
                cause: chat_state::HookCause::Notification {
                    notification_id: occurrence_id.clone(),
                },
                config_generation: 1,
                handlers: Vec::new(),
            },
        ))
        .await
        .expect("persist an independent notification hook before receipt ACK");
    let hook_seq = hook_event.seq;
    assert!(matches!(
        hook_event.kind,
        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
            occurrence_id: ref persisted,
            event: chat_state::HookEventType::Notification,
            gate: chat_state::HookGateKind::Observe,
            cause: chat_state::HookCause::Notification { notification_id: ref cause },
            ref handlers,
            ..
        }) if persisted == &occurrence_id && cause == &occurrence_id && handlers.is_empty()
    ));
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
    let receipt_seq = receipt.seq;
    let receipt_id = match receipt.kind {
        chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received { id, .. },
        ) => id,
        other => panic!("expected notification receipt, got {other:?}"),
    };
    respond_to.send(Ok(receipt_id.clone())).unwrap();
    task.await.unwrap();
    assert!(
        hook_seq < receipt_seq,
        "the independent hook must precede the recovery receipt in the durable Timeline"
    );
    let parent_events = parent.timeline_events().await.unwrap();
    assert!(parent_events.iter().any(|event| matches!(
        &event.kind,
        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
            occurrence_id: persisted,
            event: chat_state::HookEventType::Notification,
            gate: chat_state::HookGateKind::Observe,
            cause: chat_state::HookCause::Notification { notification_id },
            handlers,
            ..
        }) if persisted == &occurrence_id && notification_id == &occurrence_id && handlers.is_empty()
    )));
    assert!(!parent_events.iter().any(|event| matches!(
        &event.kind,
        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
            occurrence_id: completed,
            ..
        }) if completed == &occurrence_id
    )), "recovery must not complete a hook that was admitted independently before its receipt ACK");
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
            .send(Ok(Some(SubagentInspection {
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
            })))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        true,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(event)
                if matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Ended(_)
                    )
                )
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
            .send(Ok(Some(SubagentInspection {
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
            })))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        true,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(event)
                if matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Ended(_)
                    )
                )
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
            .send(Ok(Some(SubagentInspection {
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
            })))
            .unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        true,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    responder.await.unwrap();
    assert!(
        persistence.drain().iter().all(|record| !matches!(
            record,
            chat_state::PersistenceRecord::Timeline(event)
                if matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Ended(_)
                    )
                )
        )),
        "unverifiable completed output must not be laundered into a parent terminal"
    );
}

#[tokio::test]
async fn missing_unpublished_child_preserves_spawn_without_forging_terminal() {
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
        request.respond_to.send(Ok(None)).unwrap();
    });
    let (gateway, _gateway_rx) = test_gateway_with_receiver();
    reconcile_orphaned_subagents_with_backend(
        &crate::session::storage::SubagentProjectionState::default(),
        false,
        true,
        &backend,
        &parent_id,
        &parent,
        None,
        &gateway,
        None,
        &Default::default(),
    )
    .await;
    responder.await.unwrap();
    let terminal = persistence
        .drain()
        .into_iter()
        .find_map(|record| match record {
            chat_state::PersistenceRecord::Timeline(event) => match event.kind {
                chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(terminal),
                ) => Some(terminal),
                _ => None,
            },
            _ => None,
        });
    assert!(terminal.is_none(), "a missing child entity cannot prove a cancelled execution or supply its result identity");
    let events = parent.timeline_events().await.unwrap();
    assert!(events.iter().any(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(found)) if found.subagent_id == spawn.subagent_id)));
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
                response_messages: Vec::new(),
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

        origin_tool_call_id: None,
        prompt: "plan".into(),
        description: "d".into(),
        subagent_type: "general-purpose".into(),
        parent_session_id: "parent".into(),
        parent_step_id: None,
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
    adapter.init_session(&parent_info, crate::agent::models::ModelId::new("test-model")).await.unwrap();
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
                    },
                    forward_to_gateway: true,
                    restored: false,
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
    assert_eq!(live, 0);
}
async fn run_promote_cancel_with_worktree(
    worktree: &Path,
    worktree_freshly_created: bool,
) -> SubagentResult {
    cancel_pending_shell_child(
        "worktree-cancel",
        &acp::SessionId::new("worktree-cancel"),
        Some(worktree),
        worktree_freshly_created,
        42,
    )
    .await
}
/// A pending cancel removes only a fresh pristine worktree and reports retained paths.
#[tokio::test]
async fn cancel_pending_at_promote_removes_pristine_and_preserves_changed_or_resumed_worktrees() {
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
    let fresh_result = run_promote_cancel_with_worktree(&fresh, true).await;
    assert!(fresh_result.cancelled);
    assert!(fresh_result.worktree_path.is_none());
    assert!(
            !fresh.exists(),
            "freshly-created worktree must be removed on pending-kill"
        );

    let dirty = temp.path().join("subagent-dirty");
    fast_worktree::WorktreeBuilder::new(&repo, &dirty)
        .standalone(true)
        .create()
        .unwrap();
    std::fs::write(dirty.join("untracked.txt"), "user change").unwrap();
    let dirty_result = run_promote_cancel_with_worktree(&dirty, true).await;
    assert!(dirty.exists(), "a fresh worktree with an untracked change is retained");
    assert_eq!(dirty_result.worktree_path.as_deref(), Some(dirty.to_str().unwrap()));
    assert_eq!(std::fs::read_to_string(dirty.join("untracked.txt")).unwrap(), "user change");

    let resumed = temp.path().join("subagent-resumed");
    fast_worktree::WorktreeBuilder::new(&repo, &resumed)
        .standalone(true)
        .create()
        .unwrap();
    std::fs::write(resumed.join("tracked.txt"), "source edit").unwrap();
    assert!(resumed.exists());
    let resumed_result = run_promote_cancel_with_worktree(&resumed, false).await;
    assert_eq!(resumed_result.worktree_path.as_deref(), Some(resumed.to_str().unwrap()));
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

#[tokio::test(start_paused = true)]
async fn live_receipt_ack_deadline_does_not_dispatch_a_second_mutation() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let delivery = tokio::spawn(async move {
        admit_subagent_completion_receipt(&tx, "stalled-receipt", &SubagentOwner::Task, "body".into()).await
    });
    let SessionCommand::ReceiveNotification { respond_to: Some(ack), deadline: Some(deadline), .. } = rx.recv().await.unwrap() else {
        panic!("expected deadline-bound receipt");
    };
    assert!(deadline <= tokio::time::Instant::now() + std::time::Duration::from_secs(30));
    tokio::time::advance(std::time::Duration::from_secs(5)).await;
    assert!(!delivery.await.unwrap());
    assert!(rx.try_recv().is_err(), "a live ambiguous operation cannot be retried");
    assert!(ack.is_closed());
}

#[tokio::test]
async fn recovery_refuses_live_child_writer_then_preserves_interrupted_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let spawn = recovery_spawn("sa-live-lease", "child-live-lease");
    let seq = chat_state::EventSeq::new(7);
    write_recovery_child(dir.path(), "parent-recovery", seq, &spawn);
    let owner = crate::session::storage::jsonl::JsonlStorageAdapter::with_explicit_session_dir(dir.path().to_path_buf());
    let info = SessionInfo { id: acp::SessionId::new(spawn.child_session_id.clone()), cwd: spawn.child_cwd.clone() };
    owner.claim_recovery_writer(&info).unwrap();
    let before = std::fs::read(dir.path().join("timeline.jsonl")).unwrap();
    let fallback = result_from_inspection(&spawn, None, 99_000).unwrap();
    assert!(ensure_recovered_child_result_in_dir("parent-recovery", seq, &spawn, fallback.clone(), dir.path()).await.is_err());
    assert_eq!(std::fs::read(dir.path().join("timeline.jsonl")).unwrap(), before);
    drop(owner);
    let (_, result, output) = ensure_recovered_child_result_in_dir("parent-recovery", seq, &spawn, fallback, dir.path()).await.unwrap();
    assert_eq!(result.outcome, chat_state::SubagentOutcome::Failed);
    assert_eq!(result.duration_ms, 0, "offline duration is not execution time");
    assert!(result.error.as_deref().unwrap().contains("interrupted"));
    assert!(output.is_none());
}

#[tokio::test]
async fn recovery_result_survives_independent_worktree_gc_and_delivers_once() {
    test_utils::require_git!();
    use test_utils::git::{git_commit_all, init_git_repo};

    async fn admit_and_persist_receipt(
        parent_id: &str,
        parent: &chat_state::ChatStateHandle,
        cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>,
        request: &SubagentRequest,
        result: &SubagentResult,
        completion: &mut ShellCompletionData,
    ) -> (String, String) {
        let (receipt_id, body) = {
        let admission = admit_completion_receipt_before_result(request, result, completion);
        tokio::pin!(admission);
        let command = tokio::select! {
            command = cmd_rx.recv() => command.expect("planning result command"),
            () = &mut admission => panic!("completion returned before planning result admission"),
        };
        let SessionCommand::PlanningTaskResult { respond_to, .. } = command else {
            panic!("expected direct-parent planning result");
        };
        respond_to.send(Ok(())).unwrap();
        let SessionCommand::ReceiveNotification {
            source,
            source_version,
            body,
            respond_to: Some(respond_to),
            ..
        } = (tokio::select! {
            command = cmd_rx.recv() => command.expect("completion receipt command"),
            () = &mut admission => panic!("completion returned before receipt admission"),
        })
        else {
            panic!("expected a durable completion receipt command");
        };
        let receipt = parent
            .receive_notification_durably(
                parent_id.to_owned(),
                source,
                source_version,
                chat_state::NotificationPayloadRef {
                    blake3: blake3::hash(body.as_bytes()).to_hex().to_string(),
                    bytes: body.len() as u64,
                },
            )
            .await
            .expect("persist completion receipt");
        let receipt_id = match receipt.kind {
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received { id, .. },
            ) => id,
            other => panic!("expected durable receipt, got {other:?}"),
        };
        respond_to
            .send(Ok(receipt_id.clone()))
            .expect("acknowledge durable receipt");
        admission.await;
        (receipt_id, body)
        };
        assert!(completion.completion_receipt_admitted);
        (receipt_id, body)
    }

    let temp = tempfile::tempdir().unwrap();
    let parent_id = format!("parent-gc-recovery-{}", uuid::Uuid::now_v7());
    let child_id = format!("child-gc-recovery-{}", uuid::Uuid::now_v7());
    let subagent_id = format!("sa-gc-recovery-{}", uuid::Uuid::now_v7());
    let mut spawn = recovery_spawn(&subagent_id, &child_id);
    spawn.security_parent_session_id = parent_id.clone();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_git_repo(&repo);
    std::fs::write(repo.join("tracked.txt"), "base").unwrap();
    git_commit_all(&repo, "initial");
    let worktree = temp.path().join("subagent-worktree");
    fast_worktree::WorktreeBuilder::new(&repo, &worktree)
        .standalone(true)
        .create()
        .unwrap();
    std::fs::write(worktree.join("user-change.txt"), "preserve this edit").unwrap();
    spawn.worktree_path = Some(worktree.to_string_lossy().into_owned());
    spawn.child_cwd = worktree.to_string_lossy().into_owned();

    let worktree_db = fast_worktree::WorktreeDb::open_at(&temp.path().join("worktrees.db"))
        .unwrap();
    worktree_db
        .register(&fast_worktree::WorktreeRecord {
            id: subagent_id.clone(),
            path: worktree.clone(),
            source_repo: repo.clone(),
            repo_name: "recovery-fixture".into(),
            kind: fast_worktree::WorktreeKind::Subagent,
            creation_mode: "standalone".into(),
            git_ref: Some(format!("refs/grow/subagents/{subagent_id}")),
            head_commit: None,
            session_id: Some(parent_id.clone()),
            creator_pid: None,
            created_at: 1,
            last_accessed_at: Some(1),
            status: fast_worktree::WorktreeStatus::Alive,
            metadata: None,
        })
        .unwrap();
    let report = fast_worktree::gc_worktrees(
        &worktree_db,
        &fast_worktree::GcOptions {
            max_age_secs: Some(0),
            max_age_by_kind: [(
                fast_worktree::WorktreeKind::Subagent,
                Some(0),
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.expired_removed, 0);
    assert_eq!(
        std::fs::read_to_string(worktree.join("user-change.txt")).unwrap(),
        "preserve this edit",
        "independent fast-worktree GC must preserve subagent user changes"
    );

    let spawn_seq = chat_state::EventSeq::new(1);
    let child_dir = temp.path().join("child-session");
    std::fs::create_dir(&child_dir).unwrap();
    write_recovery_child(&child_dir, &parent_id, spawn_seq, &spawn);
    let child_info = SessionInfo {
        id: acp::SessionId::new(child_id.clone()),
        cwd: spawn.child_cwd.clone(),
    };
    let old_writer = crate::session::storage::jsonl::JsonlStorageAdapter::
        with_explicit_session_dir(child_dir.clone());
    old_writer.claim_recovery_writer(&child_info).unwrap();
    let before_recovery = std::fs::read(child_dir.join(crate::session::storage::TIMELINE_FILE))
        .unwrap();
    let fallback = RecoveredInspectionResult {
        event: chat_state::SubagentResultEvent {
            subagent_id: subagent_id.clone(),
            outcome: chat_state::SubagentOutcome::Completed,
            duration_ms: 38,
            tool_calls: 2,
            turns: 1,
            tokens_used: 42,
            error: None,
            output_ref: None,
            planning_summary: None,
        },
        output: Some("completed child output retained across recovery".into()),
    };
    assert!(ensure_recovered_child_result_in_dir(
        &parent_id,
        spawn_seq,
        &spawn,
        fallback.clone(),
        &child_dir,
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read(child_dir.join(crate::session::storage::TIMELINE_FILE)).unwrap(),
        before_recovery,
        "a live legal child writer prevents result publication"
    );
    drop(old_writer);

    let (result_ref, original_result, output) = ensure_recovered_child_result_in_dir(
        &parent_id,
        spawn_seq,
        &spawn,
        fallback.clone(),
        &child_dir,
    )
    .await
    .expect("replacement writer publishes the recovered canonical result");
    let original_output = output.expect("recovered output artifact");
    assert_eq!(original_output, fallback.output.clone().unwrap());
    let child_directory = crate::session::storage::ContainedDirectory::open(
        &child_dir,
        std::path::Path::new(""),
        "recovered child test session",
        false,
    )
    .unwrap();
    let terminal = chat_state::SubagentTerminalEvent {
        subagent_id: subagent_id.clone(),
        child_session_id: child_id.clone(),
        outcome: original_result.outcome,
        duration_ms: original_result.duration_ms,
        tool_calls: original_result.tool_calls,
        turns: original_result.turns,
        tokens_used: original_result.tokens_used,
        error: original_result.error.clone(),
        result_ref: Some(result_ref.clone()),
        snapshot_ref: None,
    };
    let finished = finish_from_durable_facts_in_directory(
        &parent_id,
        spawn_seq,
        &spawn,
        &terminal,
        &child_directory,
    )
    .expect("parent delivery projection validates the exact recovered child result");
    assert!(matches!(
        finished,
        SessionUpdate::SubagentFinished { output: Some(ref text), .. }
            if text == &original_output
    ));

    let parent_dir = temp.path().join("parent-session");
    std::fs::create_dir(&parent_dir).unwrap();
    let (parent, _persistence) = recovery_parent(&parent_dir, &parent_id, &spawn).await;
    parent
        .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(terminal),
        ))
        .await
        .unwrap();

    let mut request = auto_wake_test_request(&subagent_id);
    request.parent_session_id = parent_id.clone();
    let result = SubagentResult {
        success: true,
        output: std::sync::Arc::from(original_output.clone()),
        subagent_id: subagent_id.clone(),
        child_session_id: child_id,
        tool_calls: original_result.tool_calls,
        turns: original_result.turns,
        duration_ms: original_result.duration_ms,
        tokens_used: original_result.tokens_used,
        total_tokens_used: original_result.tokens_used,
        ..Default::default()
    };
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let mut completion = ShellCompletionData::not_admitted();
    completion.parent_cmd_tx = Some(cmd_tx.clone());
    completion.completion_cmd_tx = Some(cmd_tx);
    let (receipt_id, body) = admit_and_persist_receipt(
        &parent_id,
        &parent,
        &mut cmd_rx,
        &request,
        &result,
        &mut completion,
    )
    .await;
    assert!(body.contains(&original_output));
    let after_first_delivery = parent.timeline_events().await.unwrap();

    let (retried_receipt_id, retried_body) = admit_and_persist_receipt(
        &parent_id,
        &parent,
        &mut cmd_rx,
        &request,
        &result,
        &mut completion,
    )
    .await;
    assert_eq!(retried_receipt_id, receipt_id);
    assert_eq!(retried_body, body);
    assert_eq!(
        parent.timeline_events().await.unwrap().len(),
        after_first_delivery.len(),
        "exact delivery retry does not rewrite the parent receipt or terminal"
    );

    let changed_fallback = RecoveredInspectionResult {
        event: chat_state::SubagentResultEvent {
            outcome: chat_state::SubagentOutcome::Failed,
            duration_ms: 999,
            tool_calls: 99,
            turns: 99,
            tokens_used: 999,
            error: Some("must not replace canonical result".into()),
            ..fallback.event
        },
        output: Some("must not replace canonical output".into()),
    };
    let timeline_before_exact_retry = std::fs::read(
        child_dir.join(crate::session::storage::TIMELINE_FILE),
    )
    .unwrap();
    let (retried_ref, retried_result, retried_output) = ensure_recovered_child_result_in_dir(
        &parent_id,
        spawn_seq,
        &spawn,
        changed_fallback,
        &child_dir,
    )
    .await
    .unwrap();
    assert_eq!(retried_ref, result_ref);
    assert_eq!(retried_result, original_result);
    assert_eq!(retried_output.as_deref(), Some(original_output.as_str()));
    assert_eq!(
        std::fs::read(child_dir.join(crate::session::storage::TIMELINE_FILE)).unwrap(),
        timeline_before_exact_retry,
        "exact child replay cannot rewrite outcome, usage, or output"
    );
    assert_eq!(
        after_first_delivery
            .iter()
            .filter(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(_))
            ))
            .count(),
        1
    );
    assert_eq!(
        after_first_delivery
            .iter()
            .filter(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Received { .. }
                )
            ))
            .count(),
        1,
        "one immutable completion receipt represents both exact deliveries"
    );
}

#[tokio::test]
async fn canonical_completed_result_survives_missing_and_corrupt_output() {
    for corrupt in [false, true] {
        // Explicit session storage puts the writer lease in the session
        // directory's parent. Give each fixture a private parent so parallel
        // tests with the same stable session ID cannot contend in /tmp.
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("child-session");
        std::fs::create_dir(&dir).unwrap();
        let spawn = recovery_spawn("sa-output-loss", "child-output-loss");
        let seq = chat_state::EventSeq::new(7);
        write_recovery_child(&dir, "parent-recovery", seq, &spawn);
        let fallback = RecoveredInspectionResult {
            event: chat_state::SubagentResultEvent { subagent_id: spawn.subagent_id.clone(), outcome: chat_state::SubagentOutcome::Completed, duration_ms: 12, tool_calls: 2, turns: 1, tokens_used: 31, error: None, output_ref: None, planning_summary: None },
            output: Some("{\"structured\":true}".into()),
        };
        let (reference, original, _) = ensure_recovered_child_result_in_dir("parent-recovery", seq, &spawn, fallback, &dir).await.unwrap();
        let hash = original.output_ref.as_ref().unwrap().rsplit(':').next().unwrap();
        let artifact = dir.join("artifacts/subagent-output").join(format!("{hash}.json"));
        if corrupt { std::fs::write(artifact, b"corrupt").unwrap(); } else { std::fs::remove_file(artifact).unwrap(); }
        let before = std::fs::read(dir.join("timeline.jsonl")).unwrap();
        let replacement = result_from_inspection(&spawn, None, 999).unwrap();
        let (again, result, output) = ensure_recovered_child_result_in_dir("parent-recovery", seq, &spawn, replacement, &dir).await.unwrap();
        assert_eq!(again, reference);
        assert_eq!(result, original);
        assert!(output.is_none(), "missing structured output must never be guessed");
        assert_eq!(std::fs::read(dir.join("timeline.jsonl")).unwrap(), before);
    }
}
