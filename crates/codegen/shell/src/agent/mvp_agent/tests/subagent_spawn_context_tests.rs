//! Subagent spawn-context inheritance: a child session must inherit the parent's
//! permission handle, goal-loop gate, and configured tool-overrides cutoff so policy,
//! run-state, and a backtest bound can't be bypassed by delegating to a subagent.

use super::{build_minimal_agent_for_tests, make_test_handle};
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use agent_client_protocol as acp;

/// Subagents inherit the parent permission handle, so a managed `Read(**/.env)`
/// deny still blocks the child — direct read and the `cat .env` shell equivalent.
#[tokio::test]
async fn subagent_spawn_context_inherits_parent_permission_handle() {
    use workspace::permission::types::{
        PatternMode, PermissionConfig, PermissionRule, RuleAction, ToolFilter,
    };

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let sid = acp::SessionId::new("parent-permission");
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let gateway = GatewaySender::new(tx);
            let permission_prompt_timeout = std::time::Duration::from_secs(17);
            let cwd = paths::AbsPathBuf::new(std::path::PathBuf::from("/tmp"))
                .expect("absolute cwd");
            let (permission_handle, _events_rx) =
                workspace::permission::spawn_permission_manager(
                    sid.clone(),
                    gateway,
                    cwd,
                    workspace::permission::types::ClientType::Generic,
                    permission_prompt_timeout,
                    Some(PermissionConfig::new(vec![PermissionRule {
                        action: RuleAction::Deny,
                        tool: ToolFilter::Read,
                        pattern: Some("**/.env".to_owned()),
                        pattern_mode: PatternMode::Glob,
                    }])),
                    Vec::new(), // deny_read_globs
                    Vec::new(),
                    diagnostics::enums::PermissionMode::Ask,
                    None,
                    false,
                );

            let mut handle = make_test_handle("test-model", None);
            handle.permission_prompt_timeout = permission_prompt_timeout;
            handle.permission_handle = permission_handle;
            agent.sessions.borrow_mut().insert(sid.clone(), handle);

            let ctx = agent.build_subagent_spawn_context(sid.0.as_ref());
            assert_eq!(ctx.permission_prompt_timeout, permission_prompt_timeout);
            let inherited = ctx
                .permission_handle
                .expect("subagent context must inherit parent permission handle");

            // Direct file read and the shell equivalent both hit the parent deny.
            for access in [
                workspace::permission::AccessKind::Read(Some(".env".into())),
                workspace::permission::AccessKind::Bash("cat .env".into()),
            ] {
                let decision = inherited
                    .request(
                        access.clone(),
                        acp::ToolCallUpdate::new(acp::ToolCallId::new("tc"), Default::default()),
                        Some("child-session".to_owned()),
                        Some("general-purpose".to_owned()),
                        Some("permission inheritance regression".to_owned()),
                    )
                    .await;
                assert!(
                    matches!(
                        decision,
                        workspace::permission::Decision::PolicyDeny(_)
                    ),
                    "subagent-inherited handle must enforce parent deny for {access:?}, got {decision:?}"
                );
            }
        })
        .await;
}

/// A subagent shares the parent's `goal_loop_active_gate` Arc, so flipping the
/// parent gate is observed through the child context (same allocation).
async fn subagent_spawn_context_shares_parent_goal_loop_gate() {
    use std::sync::atomic::Ordering::Relaxed;

    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("parent-goal");
    let handle = make_test_handle("test-model", None);
    // Clone the parent's live gate before the handle moves into `sessions`.
    let parent_gate = handle.tool_context.goal_loop_active_gate.clone();
    agent.sessions.borrow_mut().insert(sid.clone(), handle);

    let ctx = agent.build_subagent_spawn_context(sid.0.as_ref());

    // Flipping the parent gate must surface through the child flag (shared Arc).
    assert!(!ctx.goal_loop_active.load(Relaxed));
    parent_gate.store(true, Relaxed);
    assert!(
        ctx.goal_loop_active.load(Relaxed),
        "subagent context must observe the parent's goal-loop gate (same Arc)"
    );
}

/// A subagent inherits the parent session's `ask_user_question` gate, so
/// `--no-ask-user` strips the tool from subagents too, while the default keeps it.
async fn subagent_spawn_context_inherits_parent_ask_user_question_gate() {
    let agent = build_minimal_agent_for_tests();

    // Parent with the tool disabled (the `--no-ask-user` case) → child off.
    let sid_off = acp::SessionId::new("parent-no-ask");
    let mut handle_off = make_test_handle("test-model", None);
    handle_off.ask_user_question_enabled = false;
    agent
        .sessions
        .borrow_mut()
        .insert(sid_off.clone(), handle_off);
    let ctx_off = agent.build_subagent_spawn_context(sid_off.0.as_ref());
    assert!(
        !ctx_off.ask_user_question_enabled,
        "subagent must inherit the parent's disabled ask_user_question gate (--no-ask-user)"
    );

    // Parent with the tool enabled (the default) → child on.
    let sid_on = acp::SessionId::new("parent-ask");
    let handle_on = make_test_handle("test-model", None);
    agent
        .sessions
        .borrow_mut()
        .insert(sid_on.clone(), handle_on);
    let ctx_on = agent.build_subagent_spawn_context(sid_on.0.as_ref());
    assert!(
        ctx_on.ask_user_question_enabled,
        "subagent must inherit the parent's enabled ask_user_question gate"
    );
}

/// A subagent inherits the parent's `process_scope`, so an owner enrolled through it stays visible via the child.
/// End-to-end reaping is covered by the spine's `process_scope_reclaim` tests.
#[tokio::test]
async fn subagent_spawn_context_inherits_parent_process_scope() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("parent-process-scope");
    let mut handle = make_test_handle("test-model", None);
    let parent_scope = tty_utils::ProcessScope::new();
    handle.tool_context.process_scope = Some(parent_scope.clone());
    agent.sessions.borrow_mut().insert(sid.clone(), handle);

    // Hold an owner Arc in the parent scope so live_count == 1.
    let owner = std::sync::Arc::new(tty_utils::ProcessGroup::new().expect("process group"));
    parent_scope.register(&owner);

    let ctx = agent.build_subagent_spawn_context(sid.0.as_ref());
    let inherited = ctx
        .process_scope
        .expect("subagent context must inherit the parent's process scope");

    assert_eq!(
        inherited.live_count(),
        1,
        "the child sees the owner enrolled through the parent scope"
    );
}

/// Nested delegation keeps lifecycle persistence on the root session while
/// inheriting every workspace/authority-bearing route from the immediate
/// spawning child.
#[tokio::test]
async fn nested_spawn_context_uses_immediate_parent_workspace_and_route() {
    let agent = build_minimal_agent_for_tests();
    let root_sid = acp::SessionId::new("root-lifecycle");
    let mut root = make_test_handle("root-model", None);
    root.info.id = root_sid.clone();
    root.info.cwd = "/tmp".to_owned();
    let root_scope = tty_utils::ProcessScope::new();
    root.tool_context.process_scope = Some(root_scope.clone());
    agent.sessions.borrow_mut().insert(root_sid.clone(), root);

    let immediate_dir = tempfile::tempdir().expect("immediate workspace");
    let immediate_cwd = immediate_dir.path().to_path_buf();
    let mut immediate = make_test_handle("immediate-model", None);
    immediate.info.id = acp::SessionId::new("immediate-child");
    immediate.info.cwd = immediate_cwd.to_string_lossy().into_owned();
    immediate.permission_mode = crate::util::config::PermissionMode::Auto;
    immediate.ask_user_question_enabled = false;
    immediate.agent_name = "immediate-agent".to_owned();
    immediate.tool_context = crate::tools::ToolContext::new_local_context(
        paths::AbsPathBuf::new(immediate_cwd.clone()).expect("absolute immediate cwd"),
        std::sync::Arc::new(workspace::file_system::LocalFs::new(immediate_cwd.clone())),
        std::sync::Arc::new(crate::terminal::LocalTerminalRunner),
    );
    immediate.tool_context.subagent_depth = 2;

    let mut ctx = agent.build_subagent_spawn_context(root_sid.0.as_ref());
    agent
        .apply_immediate_delegation_context(
            &mut ctx,
            immediate.info.id.0.to_string(),
            &immediate,
        )
        .await;

    assert_eq!(ctx.parent_session_id, root_sid.0.as_ref());
    assert_eq!(
        ctx.parent_session_info.as_ref().map(|info| info.cwd.as_str()),
        Some("/tmp"),
        "canonical lifecycle storage must remain rooted"
    );
    assert!(ctx.process_scope.is_some(), "root process scope must remain shared");
    assert_eq!(ctx.security_parent_session_id, "immediate-child");
    assert_eq!(
        ctx.delegation_session_info
            .as_ref()
            .map(|info| info.cwd.as_str()),
        Some(immediate.info.cwd.as_str())
    );
    assert_eq!(ctx.parent_cwd, immediate_cwd);
    assert_eq!(ctx.model_id.0.as_ref(), "immediate-model");
    assert_eq!(ctx.sampling_config.model, "immediate-model");
    assert_eq!(ctx.permission_mode, crate::util::config::PermissionMode::Auto);
    assert_eq!(ctx.parent_depth, 2);
    assert!(!ctx.ask_user_question_enabled);
    assert_eq!(ctx.parent_agent_name.as_deref(), Some("immediate-agent"));
    assert!(ctx.delegation_chat_state.is_some());
}
