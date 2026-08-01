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
        assert!(!app.agents.get(&AgentId(0)).unwrap().session.is_yolo());

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
    fn plan_approval_shows_overlay_even_in_yolo() {
        let mut app = make_app_with_agent("sess-A");
        app.agents.get_mut(&AgentId(0)).unwrap().session.yolo_mode = true;

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-yolo".into(),
            plan_content: "# Plan\nDo stuff".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(affected, "overlay should open even in yolo mode");
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
    fn current_mode_update_default_deactivates_plan_mode() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;
        agent.plan_mode_pending = Some(true);

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("default"), &mut agent);
        assert!(refresh_needed);
        assert!(!agent.plan_mode_active);
        assert!(agent.plan_mode_pending.is_none());
    }

    /// Unknown mode ids (e.g. a custom agent definition name like
    /// `"browser_use"`) parse to `SessionMode::Default` and deactivate
    /// plan mode.
    #[test]
    fn current_mode_update_unknown_id_treated_as_default() {
        let mut agent = make_agent(Some("s1"));
        agent.plan_mode_active = true;

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("browser_use"), &mut agent);
        assert!(refresh_needed);
        assert!(!agent.plan_mode_active);
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
    fn behavior_change_update(status: &str, target: &str) -> acp::SessionUpdate {
        acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(
                serde_json::json!({
                    "grow/behaviorChange": {
                        "status": status,
                        "source": "plan",
                        "target": target,
                        "message": format!(
                            "Switching to {target} will interrupt the active plan work. Press Enter to confirm the switch, or press Esc to cancel."
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

    /// A `confirmation_required` update parks the switch target on the agent
    /// and pins the warning banner (which must stay visible until Enter/Esc).
    #[test]
    fn behavior_change_confirmation_required_parks_switch_target() {
        let mut agent = make_agent(Some("s1"));

        let refresh = detect_plan_mode_change(
            &behavior_change_update("confirmation_required", "default"),
            &mut agent,
        );
        assert!(refresh);
        let confirm = agent
            .behavior_switch_confirm
            .as_ref()
            .expect("the interrupting switch must be parked");
        assert_eq!(confirm.target, tools::types::SessionMode::Default);
        assert!(confirm.prompt.is_none());
        assert!(
            agent.behavior_switch_warning_pending,
            "the warning must stay pending until Enter/Esc"
        );
        assert!(
            agent.mode_switch_banner.is_some(),
            "the warning banner must be visible"
        );
    }

    /// The notification and the `SetModeThenPrompt` task result arrive over
    /// independent channels in unspecified order. A late notification must
    /// re-park the target WITHOUT dropping the prompt the task already
    /// stashed (and vice versa — covered by the dispatch-level replay test).
    #[test]
    fn behavior_change_confirmation_required_preserves_stashed_prompt() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_switch_confirm = Some(crate::app::agent_view::BehaviorSwitchConfirm {
            target: tools::types::SessionMode::Default,
            prompt: Some(crate::app::agent_view::BehaviorSwitchStashedPrompt {
                text: "add auth to the app".into(),
            }),
        });

        detect_plan_mode_change(
            &behavior_change_update("confirmation_required", "default"),
            &mut agent,
        );

        let confirm = agent
            .behavior_switch_confirm
            .as_ref()
            .expect("the interrupting switch must stay parked");
        assert_eq!(confirm.target, tools::types::SessionMode::Default);
        let prompt = confirm.prompt.as_ref().expect("the stashed prompt must survive");
        assert_eq!(prompt.text, "add auth to the app");
    }

    /// `applied` resolves the parked switch: banner pending and stashed
    /// prompt (and any confirm state) are cleared.
    #[test]
    fn behavior_change_applied_clears_parked_switch() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_switch_confirm = Some(crate::app::agent_view::BehaviorSwitchConfirm {
            target: tools::types::SessionMode::Default,
            prompt: Some(crate::app::agent_view::BehaviorSwitchStashedPrompt {
                text: "add auth to the app".into(),
            }),
        });
        agent.behavior_switch_warning_pending = true;
        agent.mode_switch_banner = Some(("banner".into(), 69));

        detect_plan_mode_change(&behavior_change_update("applied", "default"), &mut agent);

        assert!(agent.behavior_switch_confirm.is_none());
        assert!(!agent.behavior_switch_warning_pending);
        assert!(
            agent.mode_switch_banner.is_some(),
            "the banner text itself fades out via the regular tick once unpinned"
        );
    }

    /// `rejected` also clears the parked switch (nothing left to confirm) and
    /// surfaces the rejection toast as before.
    #[test]
    fn behavior_change_rejected_clears_parked_switch() {
        let mut agent = make_agent(Some("s1"));
        agent.behavior_switch_confirm = Some(crate::app::agent_view::BehaviorSwitchConfirm {
            target: tools::types::SessionMode::Default,
            prompt: Some(crate::app::agent_view::BehaviorSwitchStashedPrompt {
                text: "add auth to the app".into(),
            }),
        });
        agent.behavior_switch_warning_pending = true;

        detect_plan_mode_change(&behavior_change_update("rejected", "default"), &mut agent);

        assert!(agent.behavior_switch_confirm.is_none());
        assert!(!agent.behavior_switch_warning_pending);
    }
