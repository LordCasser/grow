#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn plan_approval_opens_submitted_plan_preview() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) = make_exit_plan_ext(Some("# Submitted Plan"));

        assert!(handle_plan_approval(ext, &mut app));
        let agent = app.agents.get(&AgentId(0)).unwrap();

        assert!(agent.plan_approval_view.is_some());
        assert_eq!(
            agent
                .line_viewer
                .as_ref()
                .and_then(|v| v.markdown_content_for_test()),
            Some("# Submitted Plan")
        );
    }

    #[test]
    fn exit_plan_keeps_inline_plan_preview_available() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) = make_exit_plan_ext(Some("# First Plan"));

        assert!(handle_plan_approval(ext, &mut app));
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.line_viewer = None;
            agent.show_plan_preview();
            assert_eq!(
                agent
                    .line_viewer
                    .as_ref()
                    .and_then(|v| v.markdown_content_for_test()),
                Some("# First Plan")
            );
        }
    }

    #[test]
    fn approval_uses_request_content_without_tracked_tool() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) =
            make_exit_plan_ext_with_tool_call_id("untracked-call", Some("# Request Plan"));

        assert!(handle_plan_approval(ext, &mut app));
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .line_viewer
                .as_ref()
                .and_then(|v| v.markdown_content_for_test()),
            Some("# Request Plan")
        );
    }

    #[test]
    fn plan_approval_missing_content_is_rejected() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, mut rx) = make_exit_plan_ext(None);

        assert!(!handle_plan_approval(ext, &mut app));
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(agent.plan_approval_view.is_none());
        assert!(matches!(rx.try_recv(), Ok(Err(_))));
    }

    #[test]
    fn plan_approval_dismisses_open_modal() {
        // Regression: if the user has Ctrl+P command palette open when the
        // A Plan approval must dismiss the modal so the
        // plan preview is visible and input routes correctly. Otherwise the
        // modal hides the line viewer in draw order while input gets
        // routed to the invisible line viewer, leaving the user stuck.
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.active_modal = Some(crate::views::modal::ActiveModal::CommandPalette {
                entries: crate::views::modal::default_palette_entries(
                    &agent.prompt.slash_controller,
                ),
                state: crate::views::picker::PickerState::input_active(),
                window: crate::views::modal_window::ModalWindowState::new(),
            });
        }

        let (ext, _rx) = make_exit_plan_ext(Some("# Submitted Plan"));
        assert!(handle_plan_approval(ext, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.active_modal.is_none(),
            "Plan approval must dismiss the open modal so the plan preview is visible"
        );
        assert!(agent.plan_approval_view.is_some());
        assert!(agent.line_viewer.is_some());
    }

    #[test]
    fn plan_approval_dismisses_open_block_viewer() {
        // Regression: if the user has an Edit/tool block_viewer open when
        // Plan approval opens; dismiss it so wheel scroll reaches the plan
        // line_viewer. Draw returns on line_viewer (plan visible) but
        // handle_scroll prefers block_viewer while it remains in state.
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.block_viewer = Some(crate::views::block_viewer::BlockViewerPane::for_plain_text(
                "edit",
                "diff content",
            ));
        }

        let (ext, _rx) = make_exit_plan_ext(Some("# Submitted Plan"));
        assert!(handle_plan_approval(ext, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.block_viewer.is_none(),
            "Plan approval must dismiss open block_viewer so the plan can scroll"
        );
        assert!(agent.plan_approval_view.is_some());
        assert!(agent.line_viewer.is_some());
    }

    #[test]
    fn later_invalid_exit_plan_request_preserves_current_plan() {
        let mut app = make_app_with_agent("sess-1");
        let (first, _first_rx) = make_exit_plan_ext(Some("# First Plan"));
        let (second, _second_rx) = make_exit_plan_ext(None);

        assert!(handle_plan_approval(first, &mut app));
        assert!(!handle_plan_approval(second, &mut app));
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .plan_approval_view
                .as_ref()
                .map(|view| view.plan_content.as_str()),
            Some("# First Plan")
        );
        assert_eq!(
            agent
                .line_viewer
                .as_ref()
                .and_then(|v| v.markdown_content_for_test()),
            Some("# First Plan")
        );
    }

    #[test]
    fn plan_approval_shows_overlay() {
        let mut app = make_app_with_agent("sess-A");
        assert!(!app.agents.get(&AgentId(0)).unwrap().session.is_always_approve());

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-normal".into(),
            plan_content: "# Plan\nDo stuff".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(affected, "opening the overlay should need a redraw");
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.plan_approval_view.is_some(),
            "plan_approval_view must be set for interactive approval"
        );
        assert!(
            rx.try_recv().is_err(),
            "response must NOT have been sent yet (waiting for user)"
        );
    }

    #[test]
    fn plan_approval_shows_overlay_even_in_always_approve() {
        let mut app = make_app_with_agent("sess-A");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .permission_mode = shell::util::config::PermissionMode::AlwaysApprove;

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-always-approve".into(),
            plan_content: "# Plan\nDo stuff".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(affected, "overlay should open even in always-approve mode");
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.plan_approval_view.is_some(),
            "plan_approval_view must be set even in always-approve mode"
        );
        assert!(
            rx.try_recv().is_err(),
            "response must NOT have been sent yet (waiting for user)"
        );
    }

    #[test]
    fn plan_approval_routes_to_background_session_not_active_view() {
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-B".into(),
            tool_call_id: "tc-bg-plan".into(),
            plan_content: "# Plan".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(
            !affected,
            "a background-session plan approval must not redraw the active view"
        );
        assert!(
            app.agents
                .get(&AgentId(1))
                .unwrap()
                .plan_approval_view
                .is_some(),
            "plan approval must be parked on the session that asked (background agent B)"
        );
        assert!(
            app.agents
                .get(&AgentId(0))
                .unwrap()
                .plan_approval_view
                .is_none(),
            "plan approval must NOT land on the unrelated active agent A"
        );
        assert!(rx.try_recv().is_err(), "response must NOT be sent yet");
    }

    /// Tool-call titles never select a Behavior; only CurrentModeUpdate does.
    #[test]
    fn tool_call_title_does_not_activate_plan_behavior() {
        let mut agent = make_agent(Some("s1"));
        assert!(!agent.plan_mode_active);

        let updates = [
            make_tool_call("plan_control"),
            make_tool_call_update("plan_control"),
            make_tool_call("submit a plan"),
            make_tool_call_update("Plan selected"),
        ];
        for update in &updates {
            let refresh_needed = detect_plan_mode_change(update, &mut agent);
            assert!(
                !refresh_needed,
                "tool-call title (not a CurrentModeUpdate) must not request refresh"
            );
            assert!(
                !agent.plan_mode_active,
                "tool-call title must not flip plan mode"
            );
        }
    }

    /// Tool completion cannot deactivate Plan either.
    #[test]
    fn tool_call_title_does_not_deactivate_plan_behavior() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;

        let updates = [
            make_tool_call("plan_control"),
            make_tool_call_update("plan_control"),
            make_tool_call_update("Plan completed"),
        ];
        for update in &updates {
            let refresh_needed = detect_plan_mode_change(update, &mut agent);
            assert!(!refresh_needed);
            assert!(
                agent.plan_mode_active,
                "tool-call title must not flip plan mode"
            );
        }
    }

    #[test]
    fn current_mode_update_plan_activates_plan_mode() {
        let mut agent = make_agent(Some("s1"));
        assert!(!agent.plan_mode_active);

        let refresh_needed = detect_plan_mode_change(&make_current_mode_update("plan"), &mut agent);
        assert!(refresh_needed);
        assert!(agent.plan_mode_active);
        assert!(agent.plan_mode_pending.is_none());
    }

    #[test]
    fn current_mode_update_normal_deactivates_plan_mode() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;
        agent.plan_mode_pending = Some(true);

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("normal"), &mut agent);
        assert!(refresh_needed);
        assert!(!agent.plan_mode_active);
        assert!(agent.plan_mode_pending.is_none());
    }

    /// Unknown mode ids are invalid control-plane data. They must not silently
    /// mutate the canonical Behavior into Normal.
    #[test]
    fn current_mode_update_unknown_id_is_ignored() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("browser_use"), &mut agent);
        assert!(!refresh_needed);
        assert!(agent.plan_mode_active);
    }

    /// Idempotent CurrentModeUpdate still signals refresh because
    /// `plan_mode_pending` was cleared (affects effective state).
    #[test]
    fn current_mode_update_signals_refresh_even_on_no_op_active_change() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;
        agent.plan_mode_pending = Some(true);

        let refresh_needed = detect_plan_mode_change(&make_current_mode_update("plan"), &mut agent);
        assert!(
            refresh_needed,
            "CurrentModeUpdate must always signal refresh — pending was cleared"
        );
        assert!(agent.plan_mode_active);
        assert!(agent.plan_mode_pending.is_none());
    }

    /// The `grow/behaviorChange` meta of a `CurrentModeUpdate` mirrors the
    /// shell's `BehaviorChangeOutcome` wire shape.
    fn behavior_change_update(status: &str, current: &str, target: &str) -> acp::SessionUpdate {
        acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new(current)).meta(
                serde_json::json!({
                    "grow/behaviorChange": {
                        "status": status,
                        "source": "plan",
                        "target": target,
                        "message": format!(
                            "Switching to {target} will interrupt the active plan work. Select it again to confirm."
                        ),
                        "remainingMs": 8000,
                    }
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
    }

    fn mode_update_message(session_id: &str, update: acp::SessionUpdate) -> AcpClientMessage {
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        AcpClientMessage::SessionNotification(acp_transport::AcpArgs {
            request: acp::SessionNotification::new(acp::SessionId::new(session_id), update),
            response_tx,
        })
    }

    /// A `confirmation_required` update keeps the authoritative source mode
    /// and renders the Shell-owned window. Pager input owns no confirmation
    /// latch; only selecting the same target again can confirm it.
    #[test]
    fn behavior_change_confirmation_required_is_display_only() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.plan_mode_active = true;

        let refresh = detect_plan_mode_change(
            &behavior_change_update("confirmation_required", "plan", "normal"),
            &mut agent,
        );
        assert!(refresh);
        assert_eq!(agent.behavior_mode, tools::types::BehaviorId::Plan);
        assert!(agent.plan_mode_active);
        assert!(agent.behavior_mode_pending.is_none());
        assert!(
            agent.mode_switch_banner.is_some(),
            "the warning banner must be visible"
        );
        assert!(!behavior_mode_update_applied(&behavior_change_update(
            "confirmation_required",
            "plan",
            "normal"
        )));
    }

    #[test]
    fn confirmation_required_does_not_release_held_fifo() {
        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.plan_mode_active = true;
        agent.behavior_mode_pending = Some(tools::types::BehaviorId::Normal);
        agent.session.enqueue_prompt("held until selection repeats".into());

        let changed = handle(
            mode_update_message(
                "s1",
                behavior_change_update("confirmation_required", "plan", "normal"),
            ),
            &mut app,
        );

        assert!(changed);
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.state.is_idle());
        assert_eq!(agent.session.pending_prompts.len(), 1);
        assert!(app.pending_effects.is_empty());
    }

    #[test]
    fn applied_behavior_releases_held_fifo_under_new_identity() {
        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.plan_mode_active = true;
        agent.behavior_mode_pending = Some(tools::types::BehaviorId::Normal);
        agent.session.enqueue_prompt("run after selection applies".into());

        let changed = handle(
            mode_update_message(
                "s1",
                behavior_change_update("applied", "normal", "normal"),
            ),
            &mut app,
        );

        assert!(changed);
        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(agent.session.pending_prompts.is_empty());
        assert!(matches!(
            agent.session.state,
            crate::app::agent::AgentState::TurnSubmitting
        ));
        assert!(matches!(
            app.pending_effects.as_slice(),
            [Effect::SendPrompt { text, .. }] if text == "run after selection applies"
        ));
    }

    /// Applied mode identity is authoritative and releases held FIFO work.
    #[test]
    fn behavior_change_applied_installs_target_identity() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.plan_mode_active = true;

        let update = behavior_change_update("applied", "normal", "normal");
        detect_plan_mode_change(&update, &mut agent);

        assert_eq!(agent.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(!agent.plan_mode_active);
        assert!(behavior_mode_update_applied(&update));
    }

    #[test]
    fn leaving_workflow_closes_management_workspace_immediately() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_mode = tools::types::BehaviorId::Workflow;
        agent.show_workflows = true;

        detect_plan_mode_change(&make_current_mode_update("normal"), &mut agent);

        assert_eq!(agent.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(!agent.show_workflows);
    }

    /// Rejection retains the source identity and never releases a held FIFO.
    #[test]
    fn behavior_change_rejected_retains_source_identity() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.plan_mode_active = true;

        let update = behavior_change_update("rejected", "plan", "normal");
        detect_plan_mode_change(&update, &mut agent);

        assert_eq!(agent.behavior_mode, tools::types::BehaviorId::Plan);
        assert!(agent.plan_mode_active);
        assert!(!behavior_mode_update_applied(&update));
        assert!(agent.toast.is_some());
    }
