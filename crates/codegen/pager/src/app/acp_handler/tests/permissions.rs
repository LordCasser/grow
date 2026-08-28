#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    /// The permission prompt must surface the payload an MCP call would
    /// send — both `UseTool` (meta-dispatch) and `MCPTool` (natively
    /// registered) raw_input shapes.
    #[test]
    fn mcp_args_lines_extracts_planned_tool_input() {
        for variant in ["UseTool", "MCPTool"] {
            let req = permission_req_with_raw_input(Some(serde_json::json!({
                "variant": variant,
                "tool_name": "jira__AddjiraComment",
                "tool_input": {"issue": "ABC-123", "body": "hello"},
            })));
            let lines = mcp_args_lines(&req);
            let joined = lines.join("\n");
            assert!(
                joined.contains("\"issue\": \"ABC-123\""),
                "{variant}: {joined}"
            );
            assert!(
                joined.contains("\"body\": \"hello\""),
                "{variant}: {joined}"
            );
        }
    }

    /// Non-MCP raw_input (bash, edit, gateway `{command}` shapes) must not
    /// grow a JSON dump — those prompts have dedicated displays.
    #[test]
    fn mcp_args_lines_empty_for_non_mcp_shapes() {
        for raw in [
            None,
            Some(serde_json::json!({"variant": "Bash", "command": "ls", "description": "d"})),
            Some(serde_json::json!({"command": "rm -rf /"})),
            Some(serde_json::json!({"file_path": "/tmp/x"})),
            Some(serde_json::json!("not-an-object")),
        ] {
            let req = permission_req_with_raw_input(raw.clone());
            assert!(
                mcp_args_lines(&req).is_empty(),
                "expected empty for {raw:?}"
            );
        }
    }

    /// A `tool_input` that is missing or JSON null renders nothing rather
    /// than a misleading `null`.
    #[test]
    fn mcp_args_lines_empty_for_missing_or_null_input() {
        for raw in [
            serde_json::json!({"variant": "UseTool", "tool_name": "t"}),
            serde_json::json!({"variant": "UseTool", "tool_name": "t", "tool_input": null}),
        ] {
            let req = permission_req_with_raw_input(Some(raw));
            assert!(mcp_args_lines(&req).is_empty());
        }
    }

    /// A pathological single-line value (e.g. an embedded base64 blob) is
    /// elided at `MCP_ARGS_MAX_LINE_CHARS` so per-frame wrap cost stays
    /// bounded. Uses a multi-byte char to pin char (not byte) slicing.
    #[test]
    fn mcp_args_lines_caps_line_length() {
        let req = permission_req_with_raw_input(Some(serde_json::json!({
            "variant": "UseTool",
            "tool_name": "t",
            "tool_input": {"blob": "é".repeat(MCP_ARGS_MAX_LINE_CHARS * 2)},
        })));
        let lines = mcp_args_lines(&req);
        let long = lines
            .iter()
            .find(|l| l.contains("é"))
            .expect("blob line present");
        assert_eq!(long.chars().count(), MCP_ARGS_MAX_LINE_CHARS + 1);
        assert!(long.ends_with('…'));
    }

    /// Pathologically large payloads are capped in storage with an explicit
    /// hidden-line count (the overlay clips further at render time).
    #[test]
    fn mcp_args_lines_caps_stored_lines() {
        let big: serde_json::Map<String, serde_json::Value> = (0..MCP_ARGS_MAX_LINES + 50)
            .map(|i| (format!("k{i:04}"), serde_json::Value::from(i)))
            .collect();
        let req = permission_req_with_raw_input(Some(serde_json::json!({
            "variant": "UseTool",
            "tool_name": "t",
            "tool_input": big,
        })));
        let lines = mcp_args_lines(&req);
        assert_eq!(lines.len(), MCP_ARGS_MAX_LINES + 1);
        let last = lines.last().unwrap();
        assert!(
            last.starts_with("… (+") && last.ends_with(" more lines)"),
            "unexpected tail: {last}"
        );
    }

    /// Manual recap clears only the ephemeral status and appends one immutable
    /// result block.
    #[test]
    fn manual_recap_clears_live_status_and_appends_terminal_block() {
        let mut agent = make_agent(Some("s1"));
        agent.session.set_live_feedback(
            "recap",
            crate::scrollback::blocks::NoticeTone::Progress,
            "Generating session recap\u{2026}",
        );

        apply_recap_block(&mut agent, false, recap_block("THE RECAP"));

        assert_eq!(agent.scrollback.len(), 1);
        assert!(agent.session.live_status(100).is_none());
        assert!(!agent.scrollback.get(0).expect("recap").is_running);
    }

    /// Minimal's print-once frontier sees only the terminal recap; transient
    /// status never becomes a committed entry.
    #[test]
    fn manual_recap_terminal_is_fresh_for_minimal_commit() {
        let mut agent = make_agent(Some("s1"));
        agent.session.set_live_feedback(
            "recap",
            crate::scrollback::blocks::NoticeTone::Progress,
            "Generating session recap\u{2026}",
        );

        apply_recap_block(&mut agent, false, recap_block("THE RECAP"));

        assert_eq!(agent.scrollback.len(), 1);
        let fresh = agent.scrollback.get(0).expect("fresh block");
        assert!(
            !agent.scrollback.is_committed(fresh.id),
            "fresh block is uncommitted so the commit pass will print it"
        );
    }

    /// An automatic recap does not consume an unrelated manual live status.
    #[test]
    fn auto_recap_appends_and_leaves_manual_live_status() {
        let mut agent = make_agent(Some("s1"));
        agent.session.set_live_feedback(
            "recap",
            crate::scrollback::blocks::NoticeTone::Progress,
            "Generating session recap\u{2026}",
        );

        apply_recap_block(&mut agent, true, recap_block("AUTO RECAP"));

        assert_eq!(agent.scrollback.len(), 1, "auto recap appended");
        assert_eq!(
            agent.session.live_status(100).as_deref(),
            Some("Generating session recap\u{2026}")
        );
    }

    #[test]
    fn late_auto_recap_dropped_when_agent_not_idle() {
        assert!(should_drop_late_auto_recap(true, false, false));
        assert!(
            !should_drop_late_auto_recap(true, false, true),
            "idle agent: show auto recap"
        );
        assert!(
            !should_drop_late_auto_recap(false, false, false),
            "manual /recap always shown"
        );
        assert!(
            !should_drop_late_auto_recap(true, true, false),
            "history replay rebuilds scrollback even mid-turn"
        );
    }

    #[test]
    fn enqueue_while_scrollback_steals_focus_to_prompt() {
        use crate::app::agent_view::AgentPane;

        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .force_active_pane(AgentPane::Scrollback);

        let (msg, _rx) = make_permission_message("sess-1");
        handle(msg, &mut app);

        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.permission_queue.len(), 1);
        assert_eq!(agent.active_pane, AgentPane::Prompt);
        assert_eq!(agent.permission_stashed_pane, Some(AgentPane::Scrollback));
    }

    #[test]
    fn enqueue_while_prompt_does_not_stash_pane() {
        use crate::app::agent_view::AgentPane;

        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .force_active_pane(AgentPane::Prompt);

        let (msg, _rx) = make_permission_message("sess-1");
        handle(msg, &mut app);

        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.permission_queue.len(), 1);
        assert_eq!(agent.active_pane, AgentPane::Prompt);
        assert!(agent.permission_stashed_pane.is_none());
    }

    #[test]
    fn enqueue_while_queue_or_tasks_does_not_steal() {
        use crate::app::agent_view::AgentPane;

        for pane in [AgentPane::Queue, AgentPane::Tasks] {
            let mut app = make_app_with_agent("sess-1");
            app.agents
                .get_mut(&AgentId(0))
                .unwrap()
                .force_active_pane(pane);

            let (msg, _rx) = make_permission_message("sess-1");
            handle(msg, &mut app);

            let agent = &app.agents[&AgentId(0)];
            assert_eq!(agent.permission_queue.len(), 1, "pane={pane:?}");
            assert_eq!(agent.active_pane, pane);
            assert!(agent.permission_stashed_pane.is_none(), "pane={pane:?}");
        }
    }

    #[test]
    fn second_enqueue_does_not_resteal_if_user_returned_to_scrollback() {
        use crate::app::agent_view::AgentPane;

        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .force_active_pane(AgentPane::Scrollback);

        let (msg1, _rx1) = make_permission_message("sess-1");
        handle(msg1, &mut app);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .force_active_pane(AgentPane::Scrollback);

        let (msg2, _rx2) = make_permission_message("sess-1");
        handle(msg2, &mut app);

        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.permission_queue.len(), 2);
        assert_eq!(agent.active_pane, AgentPane::Scrollback);
        assert_eq!(agent.permission_stashed_pane, Some(AgentPane::Scrollback));
    }

    #[test]
    fn enqueue_while_scrollback_then_select_restores_scrollback() {
        use crate::app::actions::Action;
        use crate::app::agent_view::AgentPane;
        use crate::app::root::dispatch::dispatch;
        use std::sync::Arc;

        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .force_active_pane(AgentPane::Scrollback);

        let (msg, _rx) = make_permission_message("sess-1");
        handle(msg, &mut app);
        {
            let agent = &app.agents[&AgentId(0)];
            assert_eq!(agent.active_pane, AgentPane::Prompt);
            assert_eq!(agent.permission_stashed_pane, Some(AgentPane::Scrollback));
        }

        let _ = dispatch(
            Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from("allow-once"))),
            &mut app,
        );

        let agent = &app.agents[&AgentId(0)];
        assert!(agent.permission_queue.is_empty());
        assert_eq!(agent.active_pane, AgentPane::Scrollback);
        assert!(agent.permission_stashed_pane.is_none());
    }

    #[test]
    fn replay_cleanup_cancels_permissions_and_restores_composer_state() {
        use crate::app::agent_view::AgentPane;

        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.prompt.set_text("draft before permission");
            agent.force_active_pane(AgentPane::Scrollback);
        }
        let (msg, mut response_rx) = make_permission_message("sess-1");
        handle(msg, &mut app);

        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .clear_transport_interactions_for_replay();

        let agent = &app.agents[&AgentId(0)];
        assert!(agent.permission_queue.is_empty());
        assert_eq!(agent.prompt.text(), "draft before permission");
        assert_eq!(agent.active_pane, AgentPane::Scrollback);
        assert!(agent.permission_stashed_prompt.is_none());
        assert!(agent.permission_stashed_pane.is_none());
        assert!(matches!(
            response_rx.try_recv(),
            Ok(Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Cancelled,
                ..
            }))
        ));
    }

    #[test]
    fn terminal_child_cleanup_advances_retained_permission_without_losing_stashes() {
        use crate::views::permission_view::PermissionFocus;

        let mut app = make_app_with_agent("sess-root");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .prompt
            .set_text("root draft");
        let (first, mut first_rx) = make_permission_message("sess-root");
        let (second, mut second_rx) = make_permission_message("sess-root");
        handle(first, &mut app);
        handle(second, &mut app);

        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.permission_queue.front_mut().unwrap().request.request.session_id =
                acp::SessionId::new("sess-child");
            agent.permission_queue.get_mut(1).unwrap().focus = PermissionFocus::FollowupInput;
            agent.prompt.set_text("temporary followup");
            assert!(agent.clear_transport_interactions_for_session("sess-child"));
        }

        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.permission_queue.len(), 1);
        assert!(matches!(
            agent.permission_queue.front().unwrap().focus,
            PermissionFocus::Options
        ));
        assert_eq!(agent.prompt.text(), "");
        assert!(
            agent.permission_stashed_prompt.is_some(),
            "the root draft remains stashed until the retained permission resolves"
        );
        assert!(matches!(
            first_rx.try_recv(),
            Ok(Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Cancelled,
                ..
            }))
        ));
        assert!(matches!(
            second_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
    }
