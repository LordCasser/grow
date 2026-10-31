#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn new_approval_retires_preview_reads_and_only_adopts_its_own_comments() {
        use crate::views::plan_approval_view::PlanComment;
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        for same_submission in [true, false] {
            let mut app = make_app_with_agent("sess-1");
            let body = "# Submitted Plan";
            let (mut ext, _rx) = make_exit_plan_ext(Some(body));
            let mut params: serde_json::Value = serde_json::from_str(ext.request.params.get()).unwrap();
            let hash = blake3::hash(body.as_bytes()).to_hex().to_string();
            params["artifactHash"] = hash.clone().into();
            ext.request.params = serde_json::value::to_raw_value(&params).unwrap().into();
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.resolve_plan_preview(PlanArtifactQueryResponse {
                session_id: "sess-1".into(),
                submission_id: if same_submission { 1 } else { 9 },
                retained: true,
                artifact_revision: 2,
                artifact_hash: hash,
                plan_content: body.into(),
                completion_report: None,
            }).unwrap();
            agent.plan_comments_artifact = agent.plan_artifact_preview.clone();
            agent.plan_comments.push(PlanComment {
                id: 4,
                line_range: 1..2,
                text: "keep this version's note".into(),
            });
            agent.plan_next_comment_id = 5;
            let request_id = agent.plan_preview_request_id;

            assert!(handle_plan_approval(ext, &mut app));
            let agent = &app.agents[&AgentId(0)];
            assert_ne!(agent.plan_preview_request_id, request_id);
            assert_eq!(agent.plan_comments.len(), 1, "saved comments must not be lost");
            let approval = agent.plan_approval_view.as_ref().unwrap();
            assert_eq!(approval.comments.len(), usize::from(same_submission));
            if same_submission {
                assert_eq!(approval.comments[0].text, "keep this version's note");
                assert_eq!(approval.next_comment_id, 5);
            }
        }
    }

    fn approval_ext(
        body: &str,
        submission_id: u64,
    ) -> (
        acp_transport::AcpArgs<acp::ExtRequest>,
        tokio::sync::oneshot::Receiver<acp_transport::AcpResult<acp::ExtResponse>>,
    ) {
        let (mut ext, rx) = make_exit_plan_ext(Some(body));
        let mut params: serde_json::Value = serde_json::from_str(ext.request.params.get()).unwrap();
        params["submissionId"] = submission_id.into();
        params["artifactHash"] = blake3::hash(body.as_bytes()).to_hex().to_string().into();
        ext.request.params = serde_json::value::to_raw_value(&params).unwrap().into();
        (ext, rx)
    }

    fn open_approval(app: &mut AppView, body: &str, submission_id: u64) {
        let (ext, _rx) = approval_ext(body, submission_id);
        assert!(handle_plan_approval(ext, app));
    }

    fn install_pending_decision(
        agent: &mut AgentView,
        decision_id: &str,
        outcome: crate::views::plan_approval_view::PlanApprovalOutcome,
        feedback: Option<&str>,
    ) -> serde_json::Value {
        use crate::views::plan_approval_view::{PendingPlanApprovalDecision, PlanApprovalExtResponse};

        let pav = agent.plan_approval_view.as_ref().unwrap();
        let response = PlanApprovalExtResponse {
            outcome,
            feedback: feedback.map(str::to_owned),
            submission_id: pav.submission_id,
            decision_id: decision_id.into(),
        };
        let pending = PendingPlanApprovalDecision {
            session_id: pav.source_session_id.clone(),
            binding_epoch: agent.session_binding_epoch,
            response: response.clone(),
            artifact_revision: pav.artifact_revision,
            artifact_hash: pav.artifact_hash.clone(),
            comments: pav.comments.clone(),
        };
        agent.plan_approval_view.as_mut().unwrap().decision_response = Some(response.clone());
        agent.pending_plan_approval_decision = Some(pending);
        let action = match response.outcome {
            crate::views::plan_approval_view::PlanApprovalOutcome::Approved => "approve",
            crate::views::plan_approval_view::PlanApprovalOutcome::Cancelled => "revise",
            crate::views::plan_approval_view::PlanApprovalOutcome::Abandoned => "abandon",
            crate::views::plan_approval_view::PlanApprovalOutcome::Deferred => "defer",
        };
        serde_json::json!({
            "decision_id": response.decision_id,
            "submission_id": response.submission_id,
            "action": action,
            "feedback": response.feedback,
            "resulting_status": "revision_requested"
        })
    }

    #[test]
    fn early_unconfirmed_status_then_exact_mode_receipt_closes_plan() {
        use crate::app::actions::{Action, TaskResult};
        use crate::views::plan_approval_view::PlanApprovalOutcome;
        use acp_transport::protocol as acp;

        let mut app = make_app_with_agent("sess-1");
        open_approval(&mut app, "# First Plan", 1);
        let receipt = install_pending_decision(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "decision-1",
            PlanApprovalOutcome::Cancelled,
            Some("please revise"),
        );
        crate::app::root::dispatch::dispatch(
            Action::TaskComplete(TaskResult::PlanApprovalDecisionStatusResolved {
                agent_id: AgentId(0),
                session_id: acp::SessionId::new("sess-1"),
                binding_epoch: app.agents[&AgentId(0)].session_binding_epoch,
                submission_id: 1,
                decision_id: "decision-1".into(),
                result: Ok(serde_json::json!({"status": "unconfirmed"})),
            }),
            &mut app,
        );
        assert!(app.agents[&AgentId(0)].pending_plan_approval_decision.is_some());
        assert!(app.agents[&AgentId(0)].plan_approval_view.is_some());

        let update = acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(
                serde_json::json!({"grow/planDecisionReceipt": receipt})
                    .as_object().cloned().unwrap(),
            ),
        );
        assert!(detect_plan_mode_change(&update, app.agents.get_mut(&AgentId(0)).unwrap()));
        assert!(app.agents[&AgentId(0)].pending_plan_approval_decision.is_none());
        assert!(app.agents[&AgentId(0)].plan_approval_view.is_none());

        // A later round has a distinct authoritative submission and artifact.
        open_approval(&mut app, "# Revised Plan", 2);
        install_pending_decision(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "decision-2",
            PlanApprovalOutcome::Approved,
            None,
        );
        let old_receipt = serde_json::json!({
            "decision_id": "decision-1", "submission_id": 1,
            "action": "revise", "feedback": "please revise",
            "resulting_status": "revision_requested"
        });
        assert!(!app.agents.get_mut(&AgentId(0)).unwrap().confirm_plan_decision_receipt(&old_receipt));
        assert!(app.agents[&AgentId(0)].pending_plan_approval_decision.is_some());
        assert!(app.agents[&AgentId(0)].plan_approval_view.is_some());
    }

    #[test]
    fn mode_update_receipt_is_required_and_checked_against_current_identity() {
        use acp_transport::protocol as acp;

        let mut app = make_app_with_agent("sess-1");
        open_approval(&mut app, "# Plan", 1);
        let mut agent = app.agents.swap_remove(&AgentId(0)).unwrap();
        let receipt = install_pending_decision(
            &mut agent,
            "decision-1",
            crate::views::plan_approval_view::PlanApprovalOutcome::Cancelled,
            Some("please revise"),
        );
        let plain_update = acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")),
        );
        assert!(detect_plan_mode_change(&plain_update, &mut agent));
        assert!(agent.pending_plan_approval_decision.is_some());

        let wrong_receipt = serde_json::json!({
            "decision_id": "decision-other", "submission_id": 1,
            "action": "revise", "feedback": "please revise"
        });
        let bad = acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(serde_json::json!({
                "grow/planDecisionReceipt": wrong_receipt
            }).as_object().cloned().unwrap()),
        );
        assert!(detect_plan_mode_change(&bad, &mut agent));
        assert!(agent.pending_plan_approval_decision.is_some());

        for (action, feedback) in [("approve", "please revise"), ("revise", "different feedback")] {
            let wrong_payload = serde_json::json!({
                "decision_id": "decision-1", "submission_id": 1,
                "action": action, "feedback": feedback
            });
            let bad = acp::SessionUpdate::CurrentModeUpdate(
                acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(
                    serde_json::json!({
                        "grow/planDecisionReceipt": wrong_payload
                    }).as_object().cloned().unwrap(),
                ),
            );
            assert!(detect_plan_mode_change(&bad, &mut agent));
            assert!(agent.pending_plan_approval_decision.is_some());
        }

        agent.pending_plan_approval_decision.as_mut().unwrap().session_id = "other-session".into();
        let bad = acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(serde_json::json!({
                "grow/planDecisionReceipt": receipt
            }).as_object().cloned().unwrap()),
        );
        assert!(detect_plan_mode_change(&bad, &mut agent));
        assert!(agent.pending_plan_approval_decision.is_some());

        agent.pending_plan_approval_decision.as_mut().unwrap().session_id = "sess-1".into();
        agent.pending_plan_approval_decision.as_mut().unwrap().binding_epoch += 1;
        assert!(detect_plan_mode_change(&bad, &mut agent));
        assert!(agent.pending_plan_approval_decision.is_some());

        let binding_epoch = agent.session_binding_epoch;
        agent.pending_plan_approval_decision.as_mut().unwrap().binding_epoch = binding_epoch;
        let exact = acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new("plan")).meta(serde_json::json!({
                "grow/planDecisionReceipt": receipt
            }).as_object().cloned().unwrap()),
        );
        assert!(detect_plan_mode_change(&exact, &mut agent));
        assert!(agent.pending_plan_approval_decision.is_none());
        assert!(agent.plan_approval_view.is_none());
    }

    #[test]
    fn new_submission_retires_pending_decision_but_duplicate_keeps_it_and_comments() {
        use crate::views::plan_approval_view::{PlanComment, PlanApprovalOutcome};
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        let mut app = make_app_with_agent("sess-1");
        open_approval(&mut app, "# Plan", 1);
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.resolve_plan_preview(PlanArtifactQueryResponse {
                session_id: "sess-1".into(),
                submission_id: 1,
                retained: true,
                artifact_revision: 2,
                artifact_hash: blake3::hash(b"# Plan").to_hex().to_string(),
                plan_content: "# Plan".into(),
                completion_report: None,
            }).unwrap();
            install_pending_decision(
                agent,
                "decision-1",
                PlanApprovalOutcome::Cancelled,
                Some("please revise"),
            );
            agent.plan_comments.push(PlanComment {
                id: 1,
                line_range: 1..2,
                text: "preserve me".into(),
            });
            agent.plan_comments_artifact = agent.plan_artifact_preview.clone();
        }

        open_approval(&mut app, "# Plan", 1);
        assert!(app.agents[&AgentId(0)].pending_plan_approval_decision.is_some());
        assert_eq!(app.agents[&AgentId(0)].plan_comments[0].text, "preserve me");
        assert_eq!(app.agents[&AgentId(0)].plan_comments_artifact.as_ref().unwrap().submission_id, 1);

        open_approval(&mut app, "# Next Plan", 2);
        assert!(app.agents[&AgentId(0)].pending_plan_approval_decision.is_none());
        assert_eq!(app.agents[&AgentId(0)].plan_comments[0].text, "preserve me");
        assert_eq!(app.agents[&AgentId(0)].plan_comments_artifact.as_ref().unwrap().submission_id, 1);

        let registry = crate::actions::ActionRegistry::defaults();
        let mut effects = Vec::new();
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.handle_input(
            &crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            )),
            &registry,
            &mut effects,
        );
        agent.prompt.set_text("change the next step");
        let outcome = agent.handle_input(
            &crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
            &registry,
            &mut effects,
        );
        assert!(matches!(outcome, crate::app::root::InputOutcome::Action(crate::app::actions::Action::CheckPlanApprovalDecision)));
        assert!(agent.plan_approval_view.as_ref().unwrap().decision_pending());
        let pending = agent.pending_plan_approval_decision.as_ref().unwrap();
        assert_eq!(pending.response.outcome, PlanApprovalOutcome::Cancelled);
        assert_eq!(pending.response.feedback.as_deref(), Some("change the next step"));
    }

    #[test]
    fn duplicate_same_submission_keeps_pending_until_its_receipt_closes_replacement_view() {
        use crate::views::plan_approval_view::PlanApprovalOutcome;

        let mut app = make_app_with_agent("sess-1");
        open_approval(&mut app, "# Plan", 1);
        install_pending_decision(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "decision-1",
            PlanApprovalOutcome::Cancelled,
            Some("please revise"),
        );
        open_approval(&mut app, "# Plan", 1);
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert!(agent.pending_plan_approval_decision.is_some());
        assert!(agent.confirm_plan_decision_receipt(&serde_json::json!({
            "decision_id": "decision-1", "submission_id": 1,
            "action": "revise", "feedback": "please revise"
        })));
        assert!(agent.pending_plan_approval_decision.is_none());
        assert!(agent.plan_approval_view.is_none());
    }

    #[test]
    fn plan_approval_opens_submitted_plan_preview() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .show_behavior_switch_warning(tools::types::BehaviorId::Normal, 8_000);
        let (ext, _rx) = make_exit_plan_ext(Some("# Submitted Plan"));

        assert!(handle_plan_approval(ext, &mut app));
        let agent = app.agents.get(&AgentId(0)).unwrap();

        assert!(agent.plan_approval_view.is_some());
        assert!(agent.behavior_switch_target.is_none());
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
        assert!(!agent.session.plan_mode_active);

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
                !agent.session.plan_mode_active,
                "tool-call title must not flip plan mode"
            );
        }
    }

    /// Tool completion cannot deactivate Plan either.
    #[test]
    fn tool_call_title_does_not_deactivate_plan_behavior() {
        let mut agent = make_agent(Some("s1"));
        agent.session.plan_mode_active = true;

        let updates = [
            make_tool_call("plan_control"),
            make_tool_call_update("plan_control"),
            make_tool_call_update("Plan completed"),
        ];
        for update in &updates {
            let refresh_needed = detect_plan_mode_change(update, &mut agent);
            assert!(!refresh_needed);
            assert!(
                agent.session.plan_mode_active,
                "tool-call title must not flip plan mode"
            );
        }
    }

    #[test]
    fn current_mode_update_plan_activates_plan_mode() {
        let mut agent = make_agent(Some("s1"));
        assert!(!agent.session.plan_mode_active);

        let refresh_needed = detect_plan_mode_change(&make_current_mode_update("plan"), &mut agent);
        assert!(refresh_needed);
        assert!(agent.session.plan_mode_active);
    }

    #[test]
    fn current_mode_update_normal_deactivates_plan_mode() {
        let mut agent = make_agent(Some("s1"));
        agent.session.plan_mode_active = true;

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("normal"), &mut agent);
        assert!(refresh_needed);
        assert!(!agent.session.plan_mode_active);
    }

    /// Unknown mode ids are invalid control-plane data. They must not silently
    /// mutate the canonical Behavior into Normal.
    #[test]
    fn current_mode_update_unknown_id_is_ignored() {
        let mut agent = make_agent(Some("s1"));
        agent.session.plan_mode_active = true;

        let refresh_needed =
            detect_plan_mode_change(&make_current_mode_update("browser_use"), &mut agent);
        assert!(!refresh_needed);
        assert!(agent.session.plan_mode_active);
    }

    /// Idempotent CurrentModeUpdate still signals refresh for UI consumers.
    #[test]
    fn current_mode_update_signals_refresh_even_on_no_op_active_change() {
        let mut agent = make_agent(Some("s1"));
        agent.session.plan_mode_active = true;

        let refresh_needed = detect_plan_mode_change(&make_current_mode_update("plan"), &mut agent);
        assert!(
            refresh_needed,
            "CurrentModeUpdate must always signal refresh"
        );
        assert!(agent.session.plan_mode_active);
    }

    fn plan_completion_update(mode: &str, completed: bool, change_status: &str) -> acp::SessionUpdate {
        acp::SessionUpdate::CurrentModeUpdate(
            acp::CurrentModeUpdate::new(acp::SessionModeId::new(mode)).meta(
                serde_json::json!({
                    "grow/planCompleted": completed,
                    "grow/behaviorChange": {
                        "status": change_status,
                        "source": "normal",
                        "target": mode,
                    }
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
    }

    #[test]
    fn entering_a_new_plan_retires_pending_reads_without_dropping_cached_preview() {
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        let mut agent = make_agent(Some("s1"));
        let content = "# Previous active plan";
        agent
            .resolve_plan_preview(PlanArtifactQueryResponse {
                session_id: "s1".into(),
                submission_id: 3,
                retained: false,
                artifact_revision: 2,
                artifact_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                plan_content: content.into(),
                completion_report: None,
            })
            .unwrap();
        let old_request_id = agent.plan_preview_request_id;

        assert!(detect_plan_mode_change(
            &plan_completion_update("plan", false, "applied"),
            &mut agent,
        ));

        assert_ne!(agent.plan_preview_request_id, old_request_id);
        assert_eq!(
            agent
                .plan_artifact_preview
                .as_ref()
                .map(|preview| preview.plan_content.as_str()),
            Some(content),
            "entering a new Plan must preserve an existing active preview"
        );
    }

    #[test]
    fn completed_plan_stays_inspectable_until_a_behavior_change_applies() {
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        let mut agent = make_agent(Some("s1"));
        let completion = plan_completion_update("normal", true, "applied");
        assert!(detect_plan_mode_change(&completion, &mut agent));
        assert!(agent.session.plan_completed);

        let plan = "# Plan";
        agent
            .resolve_plan_preview(PlanArtifactQueryResponse {
                session_id: "s1".into(),
                submission_id: 5,
                retained: false,
                artifact_revision: 2,
                artifact_hash: blake3::hash(plan.as_bytes()).to_hex().to_string(),
                plan_content: plan.into(),
                completion_report: Some("# Original report\n\nAll checks passed.".into()),
            })
            .unwrap();
        assert!(agent
            .line_viewer
            .as_ref()
            .and_then(crate::views::file_search::line_viewer::LineViewerState::plan_ref)
            .is_some_and(|plan| plan.completed_read_only));

        let waiting = plan_completion_update("normal", true, "confirmation_required");
        assert!(detect_plan_mode_change(&waiting, &mut agent));
        assert!(agent.session.plan_completed);
        assert!(agent.plan_artifact_preview.is_some());
        assert!(agent.line_viewer.is_some());

        let request_id = agent.plan_preview_request_id;
        let applied_same_mode = plan_completion_update("normal", false, "applied");
        assert!(detect_plan_mode_change(&applied_same_mode, &mut agent));
        assert!(!agent.session.plan_completed);
        assert!(agent.plan_artifact_preview.is_none());
        assert!(agent.line_viewer.is_none());
        assert_ne!(agent.plan_preview_request_id, request_id);
    }

    #[test]
    fn completion_projection_upgrades_an_already_open_plan_preview() {
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        let mut agent = make_agent(Some("s1"));
        let plan = "# Plan";
        let response = |completion_report| PlanArtifactQueryResponse {
                session_id: "s1".into(),
                submission_id: 5,
                retained: false,
                artifact_revision: 2,
                artifact_hash: blake3::hash(plan.as_bytes()).to_hex().to_string(),
                plan_content: plan.into(),
                completion_report,
            };
        agent.resolve_plan_preview(response(None)).unwrap();
        assert!(agent.resolve_plan_preview(response(Some("# Original report".into()))).is_err());
        assert_eq!(
            agent
                .line_viewer
                .as_ref()
                .unwrap()
                .markdown_content_for_feedback()
                .as_deref(),
            Some(plan)
        );

        assert!(detect_plan_mode_change(
            &plan_completion_update("normal", true, "applied"),
            &mut agent,
        ));
        assert!(!agent.line_viewer.as_ref().unwrap().show_footer());
        agent.resolve_plan_preview(response(Some("# Original report".into()))).unwrap();
        let viewer = agent.line_viewer.as_ref().unwrap();
        assert!(!viewer.show_footer());
        assert_eq!(
            viewer.markdown_content_for_feedback().as_deref(),
            Some("# Original report")
        );
        assert!(viewer
            .plan_ref()
            .is_some_and(|plan| plan.has_result_report && plan.completed_read_only));
    }

    #[test]
    fn completion_projection_refreshes_an_open_plan_detail_once() {
        use tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse;

        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;
        let plan = "# Active Plan";
        agent
            .resolve_plan_preview(PlanArtifactQueryResponse {
                session_id: "s1".into(),
                submission_id: 6,
                retained: false,
                artifact_revision: 2,
                artifact_hash: blake3::hash(plan.as_bytes()).to_hex().to_string(),
                plan_content: plan.into(),
                completion_report: None,
            })
            .unwrap();
        let stale_request_id = 31;
        agent.plan_preview_request_id = stale_request_id;
        let binding_epoch = agent.session_binding_epoch;

        assert!(handle(
            mode_update_message("s1", plan_completion_update("normal", true, "applied")),
            &mut app,
        ));

        assert!(matches!(
            app.pending_effects.as_slice(),
            [Effect::QueryPlanArtifact {
                agent_id: AgentId(0),
                session_id,
                binding_epoch: actual_binding_epoch,
                request_id,
            }] if session_id.0.as_ref() == "s1"
                && *actual_binding_epoch == binding_epoch
                && *request_id != stale_request_id
        ));
        assert!(app.agents[&AgentId(0)].session.plan_completed);
        assert!(app.agents[&AgentId(0)].line_viewer.is_some());
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
    /// and projects the Shell-owned target into the keyboard confirmation.
    #[test]
    fn behavior_switch_confirmation_required_preserves_authoritative_mode() {
        let mut agent = make_agent(Some("s1"));
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;

        let refresh = detect_plan_mode_change(
            &behavior_change_update("confirmation_required", "plan", "normal"),
            &mut agent,
        );
        assert!(refresh);
        assert_eq!(agent.session.behavior_mode, tools::types::BehaviorId::Plan);
        assert!(agent.session.plan_mode_active);
        assert!(
            agent.mode_switch_banner.is_some(),
            "the warning banner must be visible"
        );
        assert_eq!(agent.behavior_switch_target, Some(tools::types::BehaviorId::Normal));
        let hint = &agent.mode_switch_banner.as_ref().unwrap().0;
        assert!(hint.contains("Enter") && hint.contains("Any other key"));
        assert!(!matches!(
            behavior_mode_update_resolution(&behavior_change_update(
                "confirmation_required",
                "plan",
                "normal"
            )),
            Some(crate::app::session::BehaviorControlResolution::Applied)
        ));
    }

    #[test]
    fn behavior_switch_confirmation_clears_on_terminal_outcome_and_reload() {
        for status in ["applied", "rejected"] {
            let mut agent = make_agent(Some("s1"));
            detect_plan_mode_change(
                &behavior_change_update("confirmation_required", "plan", "normal"),
                &mut agent,
            );
            detect_plan_mode_change(
                &behavior_change_update(status, "plan", "normal"),
                &mut agent,
            );
            assert!(agent.behavior_switch_target.is_none());
            assert!(agent.mode_switch_banner.is_none());
        }
        let mut agent = make_agent(Some("s1"));
        detect_plan_mode_change(
            &behavior_change_update("confirmation_required", "plan", "normal"),
            &mut agent,
        );
        agent.begin_replay_window();
        assert!(agent.behavior_switch_target.is_none());
    }

    #[test]
    fn confirmation_required_does_not_release_held_fifo() {
        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;
        agent.session.begin_behavior_switch(tools::types::BehaviorId::Normal);
        agent.session.deferred_session_mode = Some(tools::types::BehaviorId::Normal);
        agent.session.enqueue_prompt("held until selection repeats".into());
        agent
            .session
            .defer_authoritative_agent_change("remote-agent".into());

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
        assert_eq!(
            agent.session.deferred_session_mode,
            Some(tools::types::BehaviorId::Normal)
        );
        assert_eq!(agent.session.agent_name(), Some("remote-agent"));
        assert!(app.pending_effects.is_empty());
    }

    #[test]
    fn applied_behavior_releases_held_fifo_under_new_identity() {
        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;
        agent.session.begin_behavior_switch(tools::types::BehaviorId::Normal);
        agent.session.deferred_session_mode = Some(tools::types::BehaviorId::Normal);
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
        assert_eq!(agent.session.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(agent.session.deferred_session_mode.is_none());
        assert!(agent.session.pending_prompts.is_empty());
        assert!(matches!(
            agent.session.state,
            crate::app::session::AgentState::TurnSubmitting
        ));
        assert!(matches!(
            app.pending_effects.as_slice(),
            [Effect::SendPrompt { text, .. }] if text == "run after selection applies"
        ));
    }

    /// Session creation can report its initial Normal identity before the
    /// staged Dashboard Behavior RPC applies. That update must not race the
    /// first prompt out of the admission queue.
    #[test]
    fn initial_mode_update_does_not_release_a_different_deferred_behavior() {
        let mut app = make_app_with_agent("s1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.deferred_session_mode = Some(tools::types::BehaviorId::Plan);
        agent.session.enqueue_prompt("must run as plan".into());

        assert!(handle(
            mode_update_message("s1", make_current_mode_update("normal")),
            &mut app,
        ));
        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.session.pending_prompts.len(), 1);
        assert_eq!(
            agent.session.deferred_session_mode,
            Some(tools::types::BehaviorId::Plan)
        );
        assert!(app.pending_effects.is_empty());

        assert!(handle(
            mode_update_message("s1", make_current_mode_update("plan")),
            &mut app,
        ));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.pending_prompts.is_empty());
        assert!(agent.session.deferred_session_mode.is_none());
        assert!(matches!(
            app.pending_effects.as_slice(),
            [Effect::SendPrompt { text, .. }] if text == "must run as plan"
        ));
    }

    /// Applied mode identity is authoritative and releases held FIFO work.
    #[test]
    fn behavior_change_applied_installs_target_identity() {
        let mut agent = make_agent(Some("s1"));
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;

        let update = behavior_change_update("applied", "normal", "normal");
        detect_plan_mode_change(&update, &mut agent);

        assert_eq!(agent.session.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(!agent.session.plan_mode_active);
        assert_eq!(
            behavior_mode_update_resolution(&update),
            Some(crate::app::session::BehaviorControlResolution::Applied)
        );
    }

    #[test]
    fn leaving_workflow_closes_management_workspace_immediately() {
        let mut agent = make_agent(Some("s1"));
        agent.session.behavior_mode = tools::types::BehaviorId::Workflow;
        agent.show_workflows = true;

        detect_plan_mode_change(&make_current_mode_update("normal"), &mut agent);

        assert_eq!(agent.session.behavior_mode, tools::types::BehaviorId::Normal);
        assert!(!agent.show_workflows);
    }

    /// Rejection retains the source identity. Presentation is owned by the
    /// Shell's typed terminal Notice, not a second Pager-local toast.
    #[test]
    fn behavior_change_rejected_retains_source_identity() {
        let mut agent = make_agent(Some("s1"));
        agent.session.behavior_mode = tools::types::BehaviorId::Plan;
        agent.session.plan_mode_active = true;

        let update = behavior_change_update("rejected", "plan", "normal");
        detect_plan_mode_change(&update, &mut agent);

        assert_eq!(agent.session.behavior_mode, tools::types::BehaviorId::Plan);
        assert!(agent.session.plan_mode_active);
        assert_eq!(
            behavior_mode_update_resolution(&update),
            Some(crate::app::session::BehaviorControlResolution::Rejected)
        );
        assert!(agent.toast.is_none());
    }
