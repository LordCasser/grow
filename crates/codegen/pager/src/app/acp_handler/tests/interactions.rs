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

    // ── Advisory suggestion card (Task A) ─────────────────────────────────
    use crate::scrollback::blocks::tool::{
        AdvisoryCard, AdvisoryCardStatus, AdvisoryReplyStatus,
    };

    fn make_advisory_ask_ext(
        tool_call_id: &str,
    ) -> (
        acp_transport::AcpArgs<acp::ExtRequest>,
        tokio::sync::oneshot::Receiver<acp_transport::AcpResult<acp::ExtResponse>>,
    ) {
        let raw = serde_json::value::to_raw_value(&serde_json::json!({
            "sessionId": "sess-1",
            "toolCallId": tool_call_id,
            "mode": "default",
            "blocking": false,
            "questions": [{
                "question": "Which cache?",
                "options": [
                    {"label": "Redis", "description": "in-memory"},
                    {"label": "Postgres", "description": "relational"}
                ],
                // Nested `Question` is `rename_all = "snake_case"` with
                // `deny_unknown_fields` — the ext request's camelCase only
                // covers its own fields. This is the real Shell wire shape.
                "multi_select": false,
                "recommended_option": "Redis"
            }]
        }))
        .unwrap();
        let request = acp::ExtRequest::new("grow/ask_user_question", raw.into());
        let (tx, rx) = tokio::sync::oneshot::channel();
        (acp_transport::AcpArgs { request, response_tx: tx }, rx)
    }

    fn advisory_card_of(agent: &crate::app::agent_view::AgentView, call: &str) -> AdvisoryCard {
        for idx in 0..agent.scrollback.len() {
            let Some(entry) = agent.scrollback.get(idx) else { continue };
            if let crate::scrollback::block::RenderBlock::ToolCall(
                crate::scrollback::ToolCallBlock::Other(other),
            ) = &entry.block
                && let Some(card) = &other.advisory
                && card.tool_call_id == call
            {
                return card.clone();
            }
        }
        panic!("advisory card {call} not found");
    }

    #[test]
    fn advisory_ext_presents_renderable_card_before_presented_ack() {
        use tools::implementations::grow_build::ask_user_question::AskUserQuestionExtResponse;
        let mut app = make_app_with_agent("sess-1");
        app.agents.get_mut(&AgentId(0)).unwrap().prompt.set_text("draft");
        let paused_before = app.agents[&AgentId(0)].session.turn_paused_duration;
        let (ext, mut rx) = make_advisory_ask_ext("call-a");
        let changed = handle_ask_user_question(ext, &mut app);
        assert!(changed);
        let response = rx
            .try_recv()
            .expect("Presented ack sent")
            .expect("ack is a valid response");
        let typed: AskUserQuestionExtResponse =
            serde_json::from_str(response.0.get()).expect("typed ack");
        assert!(matches!(typed, AskUserQuestionExtResponse::Presented));
        let agent = &app.agents[&AgentId(0)];
        let card = advisory_card_of(agent, "call-a");
        assert_eq!(card.questions.len(), 1);
        assert_eq!(card.questions[0].recommended_option.as_deref(), Some("Redis"));
        assert_eq!(card.status, AdvisoryCardStatus::Presented);
        assert_eq!(card.reply_status, AdvisoryReplyStatus::Unobserved);
        // No composer takeover, no waiting projection, no turn pause.
        assert_eq!(agent.prompt.text(), "draft");
        assert!(!agent.session.question_pending);
        assert!(agent.question_view.is_none());
        assert_eq!(agent.session.turn_paused_duration, paused_before);
    }

    #[test]
    fn advisory_ext_keeps_blocking_overlay_and_merges_duplicate_ext() {
        use crate::views::question_view::QuestionViewState;
        // A real blocking question stays; the advisory card never replaces it.
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let stashed = agent.prompt.stash();
            agent.replace_question_view(Some(QuestionViewState::with_response_tx(
                Some("sess-1".into()),
                "call-block".into(),
                vec![],
                stashed,
                None,
                tools::implementations::grow_build::ask_user_question::AskUserQuestionMode::Default,
            )));
            agent.session.question_pending = true;
        }
        let (ext, mut rx) = make_advisory_ask_ext("call-b");
        assert!(handle_ask_user_question(ext, &mut app));
        assert!(rx.try_recv().is_ok(), "Presented sent under an open overlay");
        let (ext2, _rx2) = make_advisory_ask_ext("call-b");
        assert!(handle_ask_user_question(ext2, &mut app));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.question_view.is_some(), "blocking overlay untouched");
        assert!(agent.session.question_pending);
        let cards = (0..agent.scrollback.len())
            .filter_map(|idx| agent.scrollback.get(idx))
            .filter_map(|entry| match &entry.block {
                crate::scrollback::block::RenderBlock::ToolCall(
                    crate::scrollback::ToolCallBlock::Other(other),
                ) => other.advisory.as_ref(),
                _ => None,
            })
            .filter(|card| card.tool_call_id == "call-b")
            .count();
        assert_eq!(cards, 1, "duplicate ext delivery → one card");
    }

    #[test]
    fn advisory_reply_projection_helpers_gates_and_identity() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) = make_advisory_ask_ext("call-c");
        assert!(handle_ask_user_question(ext, &mut app));

        // Fresh card is not openable: only Available opens.
        assert!(!app.open_advisory_reply_editor("sess-1", "call-c"));
        assert!(app.set_advisory_reply_status("sess-1", "call-c", AdvisoryReplyStatus::Available));
        assert!(app.open_advisory_reply_editor("sess-1", "call-c"));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.question_view.as_ref().is_some_and(|qv| qv.is_advisory_reply_edit()));
        assert!(!agent.session.question_pending, "advisory edit is not NeedsInput");

        // Esc-style dismiss restores the composer draft and adds no pause.
        let paused = agent.session.turn_paused_duration;
        let outcome = app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .submit_question_answers_for_test(true);
        assert!(matches!(outcome, crate::app::root::InputOutcome::Changed));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.question_view.is_none());
        assert_eq!(agent.session.turn_paused_duration, paused);

        // Terminal projections are absorbing with precise bool semantics.
        assert!(app.set_advisory_reply_status("sess-1", "call-c", AdvisoryReplyStatus::Accepted));
        assert!(!app.set_advisory_reply_status("sess-1", "call-c", AdvisoryReplyStatus::Available));
        assert!(!app.set_advisory_reply_status("sess-1", "call-c", AdvisoryReplyStatus::Checking));
        assert!(app.set_advisory_reply_status("sess-1", "call-c", AdvisoryReplyStatus::Accepted));
        assert!(!app.open_advisory_reply_editor("sess-1", "call-c"), "terminal card read-only");
        assert_eq!(
            advisory_card_of(&app.agents[&AgentId(0)], "call-c").reply_status,
            AdvisoryReplyStatus::Accepted
        );

        // Stable identity: full-format match on the owning primary only.
        assert_eq!(
            app.advisory_reply_card_identity(AgentId(0), "question-reply:sess-1:call-c"),
            Some(("sess-1".to_string(), "call-c".to_string()))
        );
        assert_eq!(
            app.advisory_reply_card_identity(AgentId(0), "question-reply:sess-1:other"),
            None
        );

        // A terminal card refuses a fresh check too (terminal absorption).
        assert!(app.start_advisory_reply_check("sess-1", "call-c").is_none());

        // A FRESH card drives the one-shot open-intent / Checking lifecycle:
        // Unobserved is queryable exactly once, the intent is consumed once,
        // and the follow-on Checking state refuses a second query.
        let (ext_e, _rx_e) = make_advisory_ask_ext("call-e");
        assert!(handle_ask_user_question(ext_e, &mut app));
        let (owner, resolved) = app
            .start_advisory_reply_check("sess-1", "call-e")
            .expect("fresh Unobserved card is queryable");
        assert_eq!(owner, AgentId(0));
        assert_eq!(resolved, "sess-1");
        assert_eq!(
            advisory_card_of(&app.agents[&AgentId(0)], "call-e").reply_status,
            AdvisoryReplyStatus::Checking
        );
        assert!(app.take_advisory_reply_open_intent("sess-1", "call-e"));
        assert!(!app.take_advisory_reply_open_intent("sess-1", "call-e"));
        // Already Checking: no second query.
        assert!(app.start_advisory_reply_check("sess-1", "call-e").is_none());
    }

    #[test]
    fn advisory_reply_submit_action_retained_with_identity() {
        let mut app = make_app_with_agent("sess-1");
        let (ext, _rx) = make_advisory_ask_ext("call-d");
        assert!(handle_ask_user_question(ext, &mut app));
        assert!(app.set_advisory_reply_status("sess-1", "call-d", AdvisoryReplyStatus::Available));
        assert!(app.open_advisory_reply_editor("sess-1", "call-d"));
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        if let Some(qv) = agent.question_view.as_mut() {
            qv.select_option(0, 0);
        }
        let outcome = agent.submit_question_answers_for_test(false);
        match outcome {
            crate::app::root::InputOutcome::Action(crate::app::actions::Action::SubmitAdvisoryReply {
                source_session_id,
                tool_call_id,
                text,
            }) => {
                assert_eq!(source_session_id, "sess-1");
                assert_eq!(tool_call_id, "call-d");
                assert!(text.contains("You continued with your recommendation: Redis"));
                assert!(text.contains("My answer: Redis"));
            }
            other => panic!("expected SubmitAdvisoryReply, got {other:?}"),
        }
        assert!(app.agents[&AgentId(0)].question_view.is_none());
        assert!(!app.agents[&AgentId(0)].session.question_pending);
    }

    #[test]
    fn turn_status_projection_excludes_advisory_reply_edit() {
        use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
        use tools::implementations::grow_build::ask_user_question::AskUserQuestionMode;

        // Own view: a local advisory-reply edit borrows input but never
        // lights the NeedsInput/waiting diamond.
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let stashed = agent.prompt.stash();
            agent.replace_question_view(Some(
                QuestionViewState::new("call-edit".into(), vec![], stashed).with_local_kind(
                    LocalQuestionKind::AdvisoryReply {
                        source_session_id: "sess-1".into(),
                        tool_call_id: "call-edit".into(),
                    },
                ),
            ));
        }
        assert!(
            !app.agents[&AgentId(0)].pending_user_input_for_turn_status(),
            "advisory edit must not project waiting-on-user"
        );
        assert!(!app.agents[&AgentId(0)].session.question_pending);

        // A real blocking question still lights it.
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let stashed = agent.prompt.stash();
            agent.replace_question_view(Some(QuestionViewState::with_response_tx(
                Some("sess-1".into()),
                "call-block".into(),
                vec![],
                stashed,
                None,
                AskUserQuestionMode::Default,
            )));
        }
        assert!(app.agents[&AgentId(0)].pending_user_input_for_turn_status());
    }

    #[test]
    fn turn_status_child_projection_excludes_advisory_edit_keeps_guards() {
        use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
        use tools::implementations::grow_build::ask_user_question::AskUserQuestionMode;

        let mut app = make_app_with_agent("sess-parent");
        let install_child_question = |app: &mut AppView,
                                      advisory: bool,
                                      finished: bool| {
            let mut child = make_agent(Some("sess-child"));
            let stashed = child.prompt.stash();
            if advisory {
                child.replace_question_view(Some(
                    QuestionViewState::new("call-ce".into(), vec![], stashed).with_local_kind(
                        LocalQuestionKind::AdvisoryReply {
                            source_session_id: "sess-child".into(),
                            tool_call_id: "call-ce".into(),
                        },
                    ),
                ));
            } else {
                child.replace_question_view(Some(QuestionViewState::with_response_tx(
                    Some("sess-child".into()),
                    "call-cb".into(),
                    vec![],
                    stashed,
                    None,
                    AskUserQuestionMode::Default,
                )));
            }
            let mut info = make_subagent_info("sess-child");
            info.finished = finished;
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.subagent_views.insert("sess-child".into(), Box::new(child));
            agent.session.subagent_sessions.insert("sess-child".into(), info);
        };

        // Child advisory edit: not waiting, even for a live child.
        install_child_question(&mut app, true, false);
        assert!(!app.agents[&AgentId(0)].pending_user_input_for_turn_status());

        // Child blocking question on a live child: waiting (original guard).
        install_child_question(&mut app, false, false);
        assert!(app.agents[&AgentId(0)].pending_user_input_for_turn_status());

        // Finished child keeps the original guard: never waiting.
        install_child_question(&mut app, false, true);
        assert!(!app.agents[&AgentId(0)].pending_user_input_for_turn_status());
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
            agent.replace_question_view(Some(QuestionViewState::with_response_tx(
                Some("sess-1".into()),
                "call-q".into(),
                vec![],
                stashed,
                None,
                tools::implementations::grow_build::ask_user_question::AskUserQuestionMode::Default,
            )));
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
            affected,
            "a background-session question must still trigger a redraw so the \
             dashboard NeedsInput row reflects it immediately"
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
            handle(ask_b, &mut app),
            "background sibling B must still trigger a redraw so the dashboard \
             and parent turn-status reflect B's parked question"
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
    fn background_child_question_flags_parent_needs_input() {
        // A child that is NOT fullscreen (no active_subagent) asks a question:
        // the question must stay parked on the child's own view while the
        // parent's activity projection reports needs_input so the dashboard
        // top-level row, the subagent row, and the parent turn-status ◆ all
        // light up immediately.
        let mut app = make_app_with_agent("sess-parent");
        {
            let parent = app.agents.get_mut(&AgentId(0)).unwrap();
            parent
                .subagent_views
                .insert("child-1".into(), Box::new(make_agent(Some("child-1"))));
            parent
                .session.subagent_sessions
                .insert("child-1".into(), make_subagent_info("child-1"));
        }

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let raw = serde_json::value::to_raw_value(&serde_json::json!({
            "sessionId": "child-1",
            "toolCallId": "tc-child-1",
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
            affected,
            "a background child question must trigger a redraw so the \
             dashboard/turn-status chrome reflects it"
        );
        let parent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            crate::app::session::activity::AgentActivityProjection::from_sessions(
                &parent.session,
                parent.subagent_views.values().map(|child| &child.session),
            )
            .needs_input,
            "a non-finished child with a pending question must set needs_input"
        );
        assert!(
            parent.question_view.is_none(),
            "the question must stay on the child view, not hoist to the parent"
        );
        assert!(
            matches!(
                rx.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ),
            "response must still be parked on the child view, waiting for the user"
        );
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
    fn permission_for_inactive_always_approve_agent_auto_approves() {
        // always-approve mode is honored on the OWNING agent, not the active one,
        // so background turns aren't blocked waiting for a switch.
        let mut app = make_app_with_agent("sess-A");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .permission_mode = shell::util::config::PermissionMode::AlwaysApprove;
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        switch_active_to(&mut app, AgentId(1));

        let (msg, rx) = make_permission_message("sess-A");
        let affected = handle(msg, &mut app);

        assert!(!affected, "always-approve auto-approve never needs a redraw");
        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent_a.permission_queue.len(),
            0,
            "always-approve must auto-approve in place of queueing"
        );
        let response = rx
            .blocking_recv()
            .expect("always-approve must have sent a response on response_tx");
        let resp = response.expect("always-approve response must be Ok");
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
    fn child_permission_is_not_auto_approved_by_parent_always_approve() {
        let mut app = make_app_with_agent("sess-parent");
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.session.permission_mode = shell::util::config::PermissionMode::AlwaysApprove;
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
            "child ask must remain pending instead of inheriting parent always-approve"
        );
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

    #[test]
    fn unmatched_permission_during_startup_is_cancelled_without_root_fallback() {
        // Ordinary session updates may use the startup fallback while the
        // root ID is being assigned. A permission request with a stranger's
        // raw session ID must not be attributed to that root.
        let mut app = make_app_with_agent("sess-A");
        app.agents.get_mut(&AgentId(0)).unwrap().session.session_id = None;

        let (msg, mut rx) = make_permission_message("not-yet-assigned/stranger");
        let affected = handle(msg, &mut app);

        assert!(!affected);
        let root = app.agents.get(&AgentId(0)).unwrap();
        assert!(root.permission_queue.is_empty());
        assert_eq!(
            root.session.permission_mode,
            shell::util::config::PermissionMode::Ask,
            "the unmatched request must not change root permission mode"
        );
        let response = rx
            .try_recv()
            .expect("unmatched permission must be answered immediately")
            .expect("response must be Ok");
        assert!(
            matches!(response.outcome, acp::RequestPermissionOutcome::Cancelled),
            "unmatched startup permission must be cancelled, got {:?}",
            response.outcome,
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
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
