#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn interaction_resolved_dismisses_matching_permission() {
        // A peer answered a shared permission → this pane retracts its copy.
        let mut app = make_app_with_agent("sess-1");
        let (msg, _rx) = make_permission_message("sess-1");
        handle(msg, &mut app);
        assert_eq!(app.agents[&AgentId(0)].permission_queue.len(), 1);

        let changed = handle_session_notification(
            &interaction_resolved_ext("sess-1", "call-perm-1"),
            &mut app,
        );
        assert!(changed, "dismissing a visible permission must redraw");
        assert!(
            app.agents[&AgentId(0)].permission_queue.is_empty(),
            "the resolved permission must be removed from the queue"
        );
    }

    #[test]
    fn child_interaction_resolved_dismisses_primary_owned_permission() {
        let mut app = make_app_with_agent("sess-parent");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .insert("sess-child".into(), Box::new(make_agent(Some("sess-child"))));
        let (root_msg, _root_rx) = make_permission_message("sess-parent");
        let (child_msg, _child_rx) = make_permission_message("sess-child");
        handle(root_msg, &mut app);
        handle(child_msg, &mut app);
        assert_eq!(app.agents[&AgentId(0)].permission_queue.len(), 2);
        assert_eq!(
            app.agents[&AgentId(0)].subagent_views["sess-child"]
                .permission_queue
                .len(),
            0
        );

        let changed = handle_session_notification(
            &interaction_resolved_ext("sess-child", "call-perm-1"),
            &mut app,
        );

        assert!(changed, "dismissing the child permission must redraw");
        let parent = &app.agents[&AgentId(0)];
        let queue = &parent.permission_queue;
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue.front().unwrap().request.request.session_id.0.as_ref(),
            "sess-parent",
            "same tool-call id in the root session must remain queued"
        );
        assert!(parent.subagent_views["sess-child"].permission_queue.is_empty());
    }

    #[test]
    fn interaction_resolved_dismisses_matching_question() {
        use crate::views::question_view::QuestionViewState;
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let stashed = agent.prompt.stash();
            agent.question_view = Some(QuestionViewState::with_response_tx(
                Some("sess-1".into()),
                "call-q".into(),
                vec![],
                stashed,
                None,
                tools::implementations::grow_build::ask_user_question::AskUserQuestionMode::Default,
            ));
        }

        let changed =
            handle_session_notification(&interaction_resolved_ext("sess-1", "call-q"), &mut app);
        assert!(changed, "dismissing a visible question must redraw");
        assert!(
            app.agents[&AgentId(0)].question_view.is_none(),
            "the resolved question must be cleared"
        );
    }

    #[test]
    fn interaction_resolved_dismisses_matching_plan_approval() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) = make_exit_plan_ext_with_tool_call_id("call-plan", Some("# Plan"));
        assert!(handle_plan_approval(ext, &mut app));
        assert!(app.agents[&AgentId(0)].plan_approval_view.is_some());

        let changed =
            handle_session_notification(&interaction_resolved_ext("sess-1", "call-plan"), &mut app);
        assert!(changed, "dismissing a visible plan approval must redraw");
        assert!(
            app.agents[&AgentId(0)].plan_approval_view.is_none(),
            "the resolved plan approval must be cleared"
        );
    }

    #[test]
    fn interaction_resolved_is_noop_for_unknown_tool_call_id() {
        let mut app = make_app_with_agent("sess-1");
        let (msg, _rx) = make_permission_message("sess-1");
        handle(msg, &mut app);

        let changed = handle_session_notification(
            &interaction_resolved_ext("sess-1", "some-other-call"),
            &mut app,
        );
        assert!(!changed, "an unknown tool_call_id must be a silent no-op");
        assert_eq!(
            app.agents[&AgentId(0)].permission_queue.len(),
            1,
            "an unrelated pending modal must be left intact"
        );
    }

    #[test]
    fn permission_for_inactive_agent_queues_on_owning_agent() {
        // The headline behavior change in handle_permission_request:
        // permissions for an inactive owning agent now QUEUE (not cancel)
        // so the user sees them on switching back.
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        switch_active_to(&mut app, AgentId(1));

        let (msg, mut rx) = make_permission_message("sess-A");
        let affected = handle(msg, &mut app);

        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent_a.permission_queue.len(),
            1,
            "permission for inactive A must queue on A's permission_queue"
        );
        let agent_b = app.agents.get(&AgentId(1)).unwrap();
        assert_eq!(
            agent_b.permission_queue.len(),
            0,
            "active B's permission_queue must remain empty"
        );
        assert!(
            !affected,
            "permission queued on a non-active agent must not request a redraw"
        );
        // Permission is still pending; the response_tx must still be alive
        // (no auto-cancel was sent).
        assert!(
            rx.try_recv().is_err(),
            "permission must NOT have been answered yet (queued, not cancelled)"
        );
    }

    #[test]
    fn ask_user_question_routes_to_background_session_not_active_view() {
        // Repro of the dashboard bug: a session started but not entered asks a
        // question. Active view is agent A (sess-A); the question is for the
        // BACKGROUND agent B (sess-B). It must land on B, not fail or land on A.
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        assert_eq!(app.active_view, ActiveView::Agent(AgentId(0)));

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let raw = serde_json::value::to_raw_value(&serde_json::json!({
            "sessionId": "sess-B",
            "toolCallId": "tc-bg",
            "questions": [],
            "mode": "default",
        }))
        .unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/ask_user_question", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(
            !affected,
            "a background-session question must not redraw the active view"
        );
        assert!(
            app.agents.get(&AgentId(1)).unwrap().question_view.is_some(),
            "question must be parked on the session that asked (background agent B)"
        );
        assert!(
            app.agents.get(&AgentId(0)).unwrap().question_view.is_none(),
            "question must NOT land on the unrelated active agent A"
        );
        assert!(
            rx.try_recv().is_err(),
            "response must NOT be sent yet (parked, waiting for user)"
        );
    }

    #[test]
    fn sibling_child_questions_are_owned_independently() {
        let mut app = make_app_with_agent("sess-parent");
        {
            let parent = app.agents.get_mut(&AgentId(0)).unwrap();
            parent
                .subagent_views
                .insert("child-a".into(), Box::new(make_agent(Some("child-a"))));
            parent
                .subagent_views
                .insert("child-b".into(), Box::new(make_agent(Some("child-b"))));
            parent.active_subagent = Some("child-a".into());
        }

        let ask = |session_id: &str, tool_call_id: &str| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let raw = serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session_id,
                "toolCallId": tool_call_id,
                "questions": [],
                "mode": "default",
            }))
            .unwrap();
            (
                AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
                    request: acp::ExtRequest::new("grow/ask_user_question", raw.into()),
                    response_tx: tx,
                }),
                rx,
            )
        };
        let (ask_a, mut rx_a) = ask("child-a", "ask-a");
        let (ask_b, mut rx_b) = ask("child-b", "ask-b");

        assert!(
            handle(ask_a, &mut app),
            "fullscreen child A must redraw for its own question"
        );
        assert!(
            !handle(ask_b, &mut app),
            "background sibling B does not redraw child A"
        );

        let parent = &app.agents[&AgentId(0)];
        assert_eq!(
            parent.subagent_views["child-a"]
                .question_view
                .as_ref()
                .map(|question| question.tool_call_id.as_str()),
            Some("ask-a")
        );
        assert_eq!(
            parent.subagent_views["child-b"]
                .question_view
                .as_ref()
                .map(|question| question.tool_call_id.as_str()),
            Some("ask-b")
        );
        assert!(rx_a.try_recv().is_err(), "sibling B must not cancel A");
        assert!(rx_b.try_recv().is_err(), "both questions remain pending");
    }

    #[test]
    fn ask_user_question_unknown_session_parks_without_error() {
        // No local view for the session, and the active agent HAS a session_id
        // (so the race-window fallback does not fire). The reverse-request must
        // be left UNANSWERED (dropped) — NOT failed with an error, which would
        // render the tool red. Leader replay-on-attach handles it later.
        let mut app = make_app_with_agent("sess-A");

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let raw = serde_json::value::to_raw_value(&serde_json::json!({
            "sessionId": "sess-unknown",
            "toolCallId": "tc-unknown",
            "questions": [],
            "mode": "default",
        }))
        .unwrap();
        let msg = AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
            request: acp::ExtRequest::new("grow/ask_user_question", raw.into()),
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(!affected);
        assert!(
            app.agents.get(&AgentId(0)).unwrap().question_view.is_none(),
            "must not attach the question to an unrelated active agent"
        );
        // A dropped oneshot sender yields `Closed`; `Empty` would mean still
        // held open, `Ok` would mean a (failing) response was sent.
        match rx.try_recv() {
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                panic!("response_tx must be dropped (parked), not held open")
            }
            Ok(_) => panic!("must NOT send any response — that would fail/resolve the tool"),
        }
    }

    #[test]
    fn permission_for_inactive_yolo_agent_auto_approves() {
        // YOLO mode is honored on the OWNING agent, not the active one,
        // so background turns aren't blocked waiting for a switch.
        let mut app = make_app_with_agent("sess-A");
        app.agents.get_mut(&AgentId(0)).unwrap().session.yolo_mode = true;
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        switch_active_to(&mut app, AgentId(1));

        let (msg, rx) = make_permission_message("sess-A");
        let affected = handle(msg, &mut app);

        assert!(!affected, "YOLO auto-approve never needs a redraw");
        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent_a.permission_queue.len(),
            0,
            "YOLO must auto-approve in place of queueing"
        );
        let response = rx
            .blocking_recv()
            .expect("YOLO must have sent a response on response_tx");
        let resp = response.expect("YOLO response must be Ok");
        match resp.outcome {
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome {
                option_id,
                ..
            }) => {
                assert_eq!(option_id.0.as_ref(), "allow-once");
            }
            other => panic!("expected Selected, got {other:?}"),
        }
    }

    #[test]
    fn child_permission_is_not_auto_approved_by_parent_yolo() {
        let mut app = make_app_with_agent("sess-parent");
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.session.yolo_mode = true;
        parent.subagent_views.insert(
            "sess-child".into(),
            Box::new(make_agent(Some("sess-child"))),
        );

        let (msg, mut rx) = make_permission_message("sess-child");
        let _affected = handle(msg, &mut app);

        assert_eq!(
            app.agents.get(&AgentId(0)).unwrap().permission_queue.len(),
            1,
            "child Ask must penetrate to the owning primary interaction layer"
        );
        assert_eq!(
            app.agents.get(&AgentId(0)).unwrap().subagent_views["sess-child"]
                .permission_queue
                .len(),
            0,
            "the child transcript must not own a hidden permission timeout"
        );
        assert!(
            matches!(
                rx.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "child ask must remain pending instead of inheriting parent YOLO"
        );
    }

    #[test]
    fn capability_permission_displays_target_and_purpose() {
        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::new("sess-child"),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new("call-capability"),
                acp::ToolCallUpdateFields::new().kind(Some(acp::ToolKind::Other)),
            ),
            vec![],
        )
        .meta(
            serde_json::json!({
                "subagentCapabilityGrant": {
                    "target": "native:execute",
                    "purpose": "Run the focused parser regression tests"
                }
            })
            .as_object()
            .cloned(),
        );

        let (title, description, command) = build_permission_display(&request, None);
        assert_eq!(title, "Grant subagent access to `native:execute`?");
        assert_eq!(
            description,
            vec!["Purpose: Run the focused parser regression tests"]
        );
        assert!(command.is_none());
    }

    #[test]
    fn permission_for_unknown_session_id_is_cancelled() {
        // No agent owns the session and the active agent already has a
        // session_id (so the race-window fallback does not fire). The
        // permission must be cancelled rather than queued anywhere.
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        // make_app_with_agent already activated AgentId(0); no switch needed.

        let (msg, rx) = make_permission_message("sess-unknown");
        let affected = handle(msg, &mut app);

        assert!(!affected);
        for id in [AgentId(0), AgentId(1)] {
            assert_eq!(
                app.agents.get(&id).unwrap().permission_queue.len(),
                0,
                "no agent should have queued the unknown-session permission",
            );
        }
        let response = rx
            .blocking_recv()
            .expect("cancel_permission must have sent a response");
        let resp = response.expect("response must be Ok");
        assert!(
            matches!(resp.outcome, acp::RequestPermissionOutcome::Cancelled),
            "unknown session_id permissions must be cancelled, got {:?}",
            resp.outcome,
        );
    }

    // ── Plan approval persistence tests ─────────────────────────

    #[test]
    fn close_viewer_preserves_plan_approval_state() {
        let mut app = make_app_with_agent("sess-A");

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-persist".into(),
            plan_content: "# Plan\nDo stuff".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        handle(
            AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
                request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
                response_tx: tx,
            }),
            &mut app,
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(agent.plan_approval_view.is_some(), "approval should be set");

        // Close the viewer (simulates Esc / close button).
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.cancel_line_viewer();

        // Approval state must survive the close.
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.plan_approval_view.is_some(),
            "plan_approval_view must persist after viewer close"
        );
        assert!(agent.line_viewer.is_none(), "viewer should be closed");

        // Response must NOT have been sent (still waiting for user).
        assert!(
            rx.try_recv().is_err(),
            "response must not be sent on viewer close"
        );
    }

    #[test]
    fn reopen_viewer_restores_approval_buttons() {
        let mut app = make_app_with_agent("sess-A");
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-reopen".into(),
            plan_content: "# Plan\nStep 1".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        handle(
            AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
                request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
                response_tx: tx,
            }),
            &mut app,
        );

        // Close viewer.
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.cancel_line_viewer();
        assert!(agent.line_viewer.is_none());

        // Reopen plan preview from the submitted approval content.
        agent.show_plan_preview();

        assert!(agent.line_viewer.is_some(), "viewer should reopen");
        assert!(
            agent.line_viewer.as_ref().unwrap().feedback_active(),
            "feedback_active must be true after reopen"
        );
    }

    #[test]
    fn approve_after_reopen_does_not_overwrite_prompt() {
        let mut app = make_app_with_agent("sess-A");
        let (tx, rx) = tokio::sync::oneshot::channel();
        let ext_req = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "sess-A".into(),
            tool_call_id: "tc-prompt".into(),
            plan_content: "# Plan\nDo things".into(),
        };
        let raw = serde_json::value::to_raw_value(&ext_req).unwrap();
        handle(
            AcpClientMessage::ExtMethod(acp_transport::AcpArgs {
                request: acp::ExtRequest::new("grow/plan_approval", raw.into()),
                response_tx: tx,
            }),
            &mut app,
        );

        // Close viewer.
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.cancel_line_viewer();

        // User types new text in the prompt while viewer is closed.
        agent.prompt.set_text("my new prompt text");

        agent.reopen_plan_approval();
        agent.approve_plan();

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.prompt.text(),
            "my new prompt text",
            "stashed prompt should be restored after reopen + approve"
        );

        // Response should be approved.
        let response = rx.blocking_recv().expect("should have sent response");
        let raw = response.expect("should be Ok");
        let parsed: serde_json::Value = serde_json::from_str(raw.0.get()).unwrap();
        assert_eq!(parsed["outcome"], "approved");
    }
