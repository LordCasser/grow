//! The actual Submit action and durable observer share the ordinary FIFO.
use super::*;
// Mechanical imports for the doctor-fix replacement test (TaskA card
// lifecycle): the helpers live in the dispatch prompt module.
use super::super::prompt::{doctor_fix_target, open_doctor_fix_question};
use crate::app::actions::{InputAdmissionStatusWire as Wire, TaskResult};
use crate::scrollback::blocks::tool::{
    AdvisoryCard, AdvisoryCardStatus, AdvisoryReplyStatus, OtherToolCallBlock,
};

const SOURCE: &str = "test-session";
const CALL: &str = "call:with:colons";

fn identity() -> String {
    format!("question-reply:{SOURCE}:{CALL}")
}

fn fixture() -> AppView {
    let mut app = crate::app::root::tests::test_app_with_agent();
    let card = AdvisoryCard {
        source_session_id: SOURCE.into(),
        tool_call_id: CALL.into(),
        questions: vec![],
        status: AdvisoryCardStatus::Confirmed,
        reply_status: AdvisoryReplyStatus::Unobserved,
        reply_open_intent: false,
    };
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .scrollback
        .push_block(RenderBlock::ToolCall(
            crate::scrollback::ToolCallBlock::Other(
                OtherToolCallBlock::new("ask_user_question", "suggestion").with_advisory_card(card),
            ),
        ));
    app
}

fn submit(app: &mut AppView, text: &str) -> Vec<Effect> {
    dispatch(
        Action::SubmitAdvisoryReply {
            source_session_id: SOURCE.into(),
            tool_call_id: CALL.into(),
            text: text.into(),
        },
        app,
    )
}

fn observe(app: &mut AppView, status: Wire) -> Vec<Effect> {
    let agent = &app.agents[&AgentId(0)];
    let session_id = agent.session.session_id.clone().unwrap();
    let result = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: session_id.clone(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: Some(session_id.0.to_string()),
        response_prompt_id: Some(identity()),
        response_source: Some("durable".into()),
        status: Ok(status),
    };
    dispatch(Action::TaskComplete(result), app)
}

fn projection(app: &AppView) -> AdvisoryReplyStatus {
    let agent = &app.agents[&AgentId(0)];
    for idx in 0..agent.scrollback.len() {
        if let Some(RenderBlock::ToolCall(crate::scrollback::ToolCallBlock::Other(other))) =
            agent.scrollback.get(idx).map(|entry| &entry.block)
            && let Some(card) = &other.advisory
        {
            return card.reply_status;
        }
    }
    panic!("card missing")
}

fn assert_query(effects: &[Effect]) {
    assert!(
        matches!(effects, [Effect::QueryInputAdmissionStatus { prompt_id, session_id, agent_id: AgentId(0), .. }]
        if prompt_id == &identity() && session_id.0.as_ref() == SOURCE)
    );
}

fn query_reply(app: &mut AppView) -> Vec<Effect> {
    dispatch(
        Action::QueryAdvisoryReplyAdmission {
            source_session_id: SOURCE.into(),
            tool_call_id: CALL.into(),
        },
        app,
    )
}

#[test]
fn u3_first_unknown_opens_editor_and_real_question_submit_sends_literal_reply() {
    use tools::implementations::grow_build::ask_user_question::{Question, QuestionOption};
    let mut app = fixture();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.prompt.set_text("keep composer draft");
    for idx in 0..agent.scrollback.len() {
        let id = agent.scrollback.get(idx).unwrap().id;
        if let Some(entry) = agent.scrollback.get_by_id_mut(id)
            && let RenderBlock::ToolCall(crate::scrollback::ToolCallBlock::Other(other)) =
                &mut entry.block
            && let Some(card) = &mut other.advisory
        {
            card.questions = vec![Question {
                question: "Choose a route".into(),
                options: vec![QuestionOption {
                    label: "/exit".into(),
                    description: "literal answer".into(),
                    preview: None,
                    id: None,
                }],
                multi_select: Some(false),
                recommended_option: Some("/exit".into()),
                id: None,
            }];
        }
    }
    assert_query(&query_reply(&mut app));
    assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    assert!(
        agent
            .question_view
            .as_ref()
            .is_some_and(|view| view.is_advisory_reply_edit()),
        "a legal durable Unknown with no local payload must allow first editing"
    );
    assert!(!agent.session.question_pending);
    let outcome = agent.submit_question_answers_for_test(false);
    let crate::app::root::InputOutcome::Action(action @ Action::SubmitAdvisoryReply { .. }) =
        outcome
    else {
        panic!("real question submission must produce the reply action")
    };
    let Action::SubmitAdvisoryReply { ref text, .. } = action else {
        unreachable!()
    };
    let original = text.clone();
    assert!(original.contains("Choose a route"));
    assert!(original.contains("/exit"));
    assert!(original.contains(SOURCE));
    assert_send(&dispatch(action, &mut app), &original);
    assert_eq!(app.agents[&AgentId(0)].prompt.text(), "keep composer draft");
    assert_ne!(projection(&app), AdvisoryReplyStatus::Accepted);
}

/// U6: the user explicitly opens the reply editor from the scrollback card
/// (`r` on the selected card). After the admission callback opens the editor,
/// REAL keys (Down/Enter) must navigate THIS question — not the history area.
/// Reproduces the PTY failure where Down/Enter opened the history viewer.
#[test]
fn u6_explicitly_opened_reply_editor_owns_keyboard_from_scrollback() {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    use tools::implementations::grow_build::ask_user_question::{Question, QuestionOption};

    let key = |code: KeyCode| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
    let mut app = fixture();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.prompt.set_text("keep composer draft");
    let two_questions = || {
        vec![
            Question {
                question: "Choose a route".into(),
                options: vec![
                    QuestionOption {
                        label: "A".into(),
                        description: "first".into(),
                        preview: None,
                        id: None,
                    },
                    QuestionOption {
                        label: "B".into(),
                        description: "second".into(),
                        preview: None,
                        id: None,
                    },
                ],
                multi_select: Some(false),
                recommended_option: Some("A".into()),
                id: None,
            },
            Question {
                question: "Add notes?".into(),
                options: vec![
                    QuestionOption {
                        label: "Yes".into(),
                        description: String::new(),
                        preview: None,
                        id: None,
                    },
                    QuestionOption {
                        label: "No".into(),
                        description: String::new(),
                        preview: None,
                        id: None,
                    },
                ],
                multi_select: Some(false),
                recommended_option: Some("No".into()),
                id: None,
            },
        ]
    };
    let card_entry = agent.scrollback.get(0).unwrap().id;
    if let Some(entry) = agent.scrollback.get_by_id_mut(card_entry)
        && let RenderBlock::ToolCall(crate::scrollback::ToolCallBlock::Other(other)) =
            &mut entry.block
        && let Some(card) = &mut other.advisory
    {
        card.questions = two_questions();
    }
    // Real entry state: the card row is selected and the keyboard is in the
    // scrollback pane (the `r` origin; also the fixture default).
    agent.scrollback.set_selected(Some(0));
    agent.force_active_pane(crate::views::agent::ActivePane::Scrollback);

    // Real 'r' from the scrollback key path → the reply query action.
    let mut effects = Vec::new();
    let registry = crate::actions::ActionRegistry::defaults();
    let outcome = app.agents.get_mut(&AgentId(0)).unwrap().handle_input(
        &key(KeyCode::Char('r')),
        &registry,
        &mut effects,
    );
    let action = match outcome {
        crate::app::root::InputOutcome::Action(action) => action,
        other => panic!("r on a selected advisory card must query admission, got {other:?}"),
    };
    let Action::QueryAdvisoryReplyAdmission {
        source_session_id,
        tool_call_id,
        ..
    } = action
    else {
        panic!("r must query advisory admission, got {action:?}")
    };
    assert_eq!(source_session_id, SOURCE);
    assert_eq!(tool_call_id, CALL);

    // Qualified durable Unknown with no frozen payload → observer opens the
    // editor (same wire path as production reconcile).
    assert_query(&dispatch(
        Action::QueryAdvisoryReplyAdmission {
            source_session_id,
            tool_call_id,
        },
        &mut app,
    ));
    assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
    {
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert!(
            agent
                .question_view
                .as_ref()
                .is_some_and(|v| v.is_advisory_reply_edit())
        );
        assert!(!agent.session.question_pending);
    }

    // The old bug: the editor was visible but the pane stayed Scrollback, so
    // Down/Enter drove the history area (the PTY run opened the history
    // viewer). With the fix these keys must navigate this question.
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let outcome = agent.handle_input(&key(KeyCode::Down), &registry, &mut effects);
    assert!(
        matches!(outcome, crate::app::root::InputOutcome::Changed),
        "Down must be consumed by the editor"
    );
    {
        let agent = &app.agents[&AgentId(0)];
        let view = agent
            .question_view
            .as_ref()
            .expect("editor still open after Down");
        assert_eq!(view.cursor(), 1, "Down must move the option cursor");
        assert!(
            agent.block_viewer.is_none(),
            "Down must not open the history viewer"
        );
        assert!(agent.line_viewer.is_none());
    }

    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let outcome = agent.handle_input(&key(KeyCode::Enter), &registry, &mut effects);
    assert!(matches!(outcome, crate::app::root::InputOutcome::Changed));
    {
        let agent = &app.agents[&AgentId(0)];
        let view = agent
            .question_view
            .as_ref()
            .expect("editor still open after Enter");
        assert_eq!(
            view.active_tab, 1,
            "Enter on the last option of question 1 must advance to question 2"
        );
        assert!(
            agent.block_viewer.is_none(),
            "Enter must not open the history viewer"
        );
    }

    // Esc dismisses only the local edit: composer draft restored, no
    // waiting projection, no turn pause.
    let paused_before = app.agents[&AgentId(0)].session.turn_paused_duration;
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let outcome = agent.handle_input(&key(KeyCode::Esc), &registry, &mut effects);
    assert!(matches!(outcome, crate::app::root::InputOutcome::Changed));
    let agent = &app.agents[&AgentId(0)];
    assert!(agent.question_view.is_none());
    assert_eq!(agent.prompt.text(), "keep composer draft");
    assert!(!agent.session.question_pending);
    assert_eq!(agent.session.turn_paused_duration, paused_before);
}

#[test]
fn u3_unknown_with_frozen_queue_or_inflight_never_opens_editor() {
    for in_flight in [false, true] {
        let mut app = fixture();
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        if in_flight {
            agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
                prompt_id: Some(identity()),
                admission_resolved: None,
                frozen_skill_selection: None,
                expected_turn_id: None,
                text: "fixed original".into(),
                images: vec![],
                scrollback_entry: crate::scrollback::EntryId::new(0),
                combined_scrollback_entries: vec![],
                chip_elements: vec![],
            });
        } else {
            agent
                .session
                .pending_prompts
                .push_back(crate::app::session::QueuedPrompt {
                    prompt_id: Some(identity()),
                    requires_review: true,
                    ..crate::app::session::QueuedPrompt::plain(
                        1,
                        "fixed original",
                        crate::app::session::QueueEntryKind::Prompt,
                    )
                });
        }
        assert_query(&query_reply(&mut app));
        // U2 makes this explicit Check retry the same immutable payload;
        // U3's guarantee remains that it cannot reopen selection editing.
        assert_send(
            &observe(&mut app, Wire::UnknownUnconfirmed),
            "fixed original",
        );
        assert!(app.agents[&AgentId(0)].question_view.is_none());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
    }
}

#[test]
fn u3_busy_first_submit_keeps_ready_fifo_after_unknown_observation() {
    let mut app = fixture();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.state = AgentState::TurnRunning;
    agent.session.current_prompt_id = Some("original-turn".into());
    assert!(submit(&mut app, "first queued answer").is_empty());
    let queued = app.agents[&AgentId(0)]
        .session
        .pending_prompts
        .front()
        .unwrap();
    assert_eq!(queued.prompt_id.as_deref(), Some(identity().as_str()));
    assert!(
        !queued.requires_review,
        "first explicit input must be ready for normal FIFO drain"
    );
    assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
    assert!(
        !app.agents[&AgentId(0)]
            .session
            .pending_prompts
            .front()
            .unwrap()
            .requires_review,
        "unknown observation cannot revoke an unsent ready input"
    );
    for status in [
        Wire::TimedOut,
        Wire::Unavailable {
            reason: "offline".into(),
        },
        Wire::InvalidEvidence {
            reason: "bad response".into(),
        },
    ] {
        assert!(observe(&mut app, status).is_empty());
        assert!(
            !app.agents[&AgentId(0)]
                .session
                .pending_prompts
                .front()
                .unwrap()
                .requires_review
        );
    }
    let agent = &app.agents[&AgentId(0)];
    let failure = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: agent.session.session_id.clone().unwrap(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: None,
        response_prompt_id: None,
        response_source: None,
        status: Err("transport observation failed before this input was sent".into()),
    };
    assert!(dispatch(Action::TaskComplete(failure), &mut app).is_empty());
    assert!(
        !app.agents[&AgentId(0)]
            .session
            .pending_prompts
            .front()
            .unwrap()
            .requires_review
    );
    assert!(app.agents[&AgentId(0)].question_view.is_none());
    let mut response = acp::PromptResponse::new(acp::StopReason::EndTurn);
    response.meta =
        Some(serde_json::from_value(serde_json::json!({ "promptId": "original-turn" })).unwrap());
    let effects = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Ok(response),
            http_status: None,
            prompt_id: Some("original-turn".into()),
        }),
        &mut app,
    );
    let sends: Vec<_> = effects
        .into_iter()
        .filter(|effect| matches!(effect, Effect::SendPrompt { .. }))
        .collect();
    assert_send(&sends, "first queued answer");
}

#[test]
fn u3_noneditable_observations_without_payload_do_not_open_editor() {
    for status in [
        Wire::Submitted,
        Wire::TimedOut,
        Wire::Unavailable {
            reason: "offline".into(),
        },
        Wire::InvalidEvidence {
            reason: "invalid".into(),
        },
    ] {
        let mut app = fixture();
        assert_query(&query_reply(&mut app));
        assert!(observe(&mut app, status).is_empty());
        assert!(app.agents[&AgentId(0)].question_view.is_none());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
    }
    let mut app = fixture();
    assert_query(&query_reply(&mut app));
    let agent = &app.agents[&AgentId(0)];
    let failure = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: agent.session.session_id.clone().unwrap(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: None,
        response_prompt_id: None,
        response_source: None,
        status: Err("decode or transport failure".into()),
    };
    assert!(dispatch(Action::TaskComplete(failure), &mut app).is_empty());
    assert!(app.agents[&AgentId(0)].question_view.is_none());
    assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
}

fn reply_completion_pr(app: &mut AppView, prompt_id: &str) -> Vec<Effect> {
    let mut response = acp::PromptResponse::new(acp::StopReason::EndTurn);
    response.meta =
        Some(serde_json::from_value(serde_json::json!({ "promptId": prompt_id })).unwrap());
    dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Ok(response),
            http_status: None,
            prompt_id: Some(prompt_id.into()),
        }),
        app,
    )
}

fn reply_completion_durable(app: &mut AppView, prompt_id: &str) {
    let payload = serde_json::json!({
        "sessionId": SOURCE,
        "update": { "sessionUpdate": "turn_completed", "prompt_id": prompt_id,
            "stop_reason": "end_turn", "agent_result": null, "usage": null, "identity": null }
    });
    let raw = serde_json::value::to_raw_value(&payload).unwrap();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    crate::app::acp_handler::handle(
        acp_transport::AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request: acp::ExtNotification::new("grow/session_notification", raw.into()),
            response_tx: tx,
        }),
        app,
    );
}

fn only_admission_queries(effects: Vec<Effect>) -> Vec<Effect> {
    effects
        .into_iter()
        .filter(|effect| matches!(effect, Effect::QueryInputAdmissionStatus { .. }))
        .collect()
}

#[test]
fn u1_normal_reply_pr_completion_observes_original_identity_once_before_next_drain() {
    let mut app = fixture();
    assert_send(&submit(&mut app, "answer"), "answer");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .enqueue_prompt("next ordinary input".into());
    let effects = reply_completion_pr(&mut app, &identity());
    assert_query(&only_admission_queries(effects));
    assert_ne!(
        app.agents[&AgentId(0)].session.current_prompt_id.as_deref(),
        Some(identity().as_str())
    );
    let history_len = app.agents[&AgentId(0)].scrollback.len();
    assert!(only_admission_queries(reply_completion_pr(&mut app, &identity())).is_empty());
    reply_completion_durable(&mut app, &identity());
    assert!(only_admission_queries(std::mem::take(&mut app.pending_effects)).is_empty());
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), history_len);
    assert!(
        observe(
            &mut app,
            Wire::Consumed {
                route: chat_state::InputRoute::Fifo
            }
        )
        .is_empty()
    );
    assert_eq!(projection(&app), AdvisoryReplyStatus::Accepted);
}

#[test]
fn u1_normal_reply_durable_completion_observes_once_and_late_pr_only_merges() {
    let mut app = fixture();
    assert_send(&submit(&mut app, "answer"), "answer");
    // Durable terminal requires a confirmed running foreground, as in the
    // existing first-wins tests. Submit's unconfirmed state is insufficient.
    app.agents.get_mut(&AgentId(0)).unwrap().session.state = AgentState::TurnRunning;
    reply_completion_durable(&mut app, &identity());
    assert_query(&only_admission_queries(std::mem::take(
        &mut app.pending_effects,
    )));
    let history_len = app.agents[&AgentId(0)].scrollback.len();
    assert!(only_admission_queries(reply_completion_pr(&mut app, &identity())).is_empty());
    reply_completion_durable(&mut app, &identity());
    assert!(only_admission_queries(std::mem::take(&mut app.pending_effects)).is_empty());
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), history_len);
    assert!(observe(&mut app, Wire::Handled).is_empty());
    assert_eq!(projection(&app), AdvisoryReplyStatus::Accepted);
}

#[test]
fn u1_completion_observation_requires_canonical_acceptance_and_never_resends() {
    for status in [
        Wire::Allowed {
            route: chat_state::InputRoute::Fifo,
        },
        Wire::Handled,
        Wire::Blocked {
            reason: chat_state::InputBlockReason::ProcessInterrupted,
        },
        Wire::Dismissed {
            reason: chat_state::InputDismissReason::SessionClosing,
        },
        Wire::UnknownUnconfirmed,
    ] {
        let expected = match &status {
            Wire::Allowed { .. } | Wire::Handled => AdvisoryReplyStatus::Accepted,
            Wire::Blocked { .. } | Wire::Dismissed { .. } => AdvisoryReplyStatus::Rejected,
            _ => AdvisoryReplyStatus::Unconfirmed,
        };
        let mut app = fixture();
        submit(&mut app, "answer");
        assert_query(&only_admission_queries(reply_completion_pr(
            &mut app,
            &identity(),
        )));
        assert_ne!(projection(&app), AdvisoryReplyStatus::Accepted);
        assert!(observe(&mut app, status).is_empty());
        assert_eq!(projection(&app), expected);
        assert!(app.agents[&AgentId(0)].question_view.is_none());
    }
    let mut app = fixture();
    submit(&mut app, "answer");
    assert_query(&only_admission_queries(reply_completion_pr(
        &mut app,
        &identity(),
    )));
    let agent = &app.agents[&AgentId(0)];
    let error = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: agent.session.session_id.clone().unwrap(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: None,
        response_prompt_id: None,
        response_source: None,
        status: Err("completion observation offline".into()),
    };
    assert!(dispatch(Action::TaskComplete(error), &mut app).is_empty());
    assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
    assert!(app.agents[&AgentId(0)].question_view.is_none());
}

#[test]
fn u1_ordinary_turn_completion_has_no_advisory_query() {
    let mut app = fixture();
    let effects = dispatch(Action::SendPrompt("ordinary input".into()), &mut app);
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::SendPrompt { prompt_id, .. } => Some(prompt_id.clone()),
            _ => None,
        })
        .unwrap();
    assert!(only_admission_queries(reply_completion_pr(&mut app, &id)).is_empty());
}

fn failed_reply_fixture() -> AppView {
    let mut app = fixture();
    assert_send(&submit(&mut app, "frozen answer"), "frozen answer");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .prompt
        .set_text("unrelated composer draft");
    let recovery = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Err("lost ACK".into()),
            http_status: None,
            prompt_id: Some(identity()),
        }),
        &mut app,
    );
    assert_query(&recovery);
    app
}

#[test]
fn u2_actual_check_retries_frozen_original_once_on_unknown_or_absent() {
    for missing in [Wire::UnknownUnconfirmed, Wire::Absent] {
        let mut app = failed_reply_fixture();
        assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
        assert_query(&query_reply(&mut app));
        assert!(
            query_reply(&mut app).is_empty(),
            "rapid second click coalesces"
        );
        assert_send(&observe(&mut app, missing), "frozen answer");
        assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
        assert_eq!(
            app.agents[&AgentId(0)].prompt.text(),
            "unrelated composer draft"
        );
        assert_query(&submit(&mut app, "changed choice cannot replace original"));
        assert_eq!(
            app.agents[&AgentId(0)]
                .session
                .in_flight_prompt
                .as_ref()
                .unwrap()
                .text,
            "frozen answer"
        );
    }
}

fn admission_replay(app: &mut AppView, status: serde_json::Value) -> Vec<Effect> {
    let mut response = acp::PromptResponse::new(acp::StopReason::EndTurn);
    response.meta = Some(
        serde_json::from_value(serde_json::json!({
            "sessionId": SOURCE, "promptId": identity(),
            "grow/inputAdmission": { "inputId": "original-input", "status": status }
        }))
        .unwrap(),
    );
    dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Ok(response),
            http_status: None,
            prompt_id: Some(identity()),
        }),
        app,
    )
}

#[test]
fn u2_admission_replay_is_observation_not_a_completed_business_turn() {
    for status in [
        serde_json::json!({"status":"submitted"}),
        serde_json::json!({"status":"allowed", "route":{"route":"fifo"}}),
        serde_json::json!({"status":"consumed", "route":{"route":"fifo"}}),
        serde_json::json!({"status":"handled"}),
        serde_json::json!({"status":"blocked", "reason":{"kind":"process_interrupted"}}),
        serde_json::json!({"status":"dismissed", "reason":"user_removed"}),
    ] {
        let mut app = failed_reply_fixture();
        observe(&mut app, Wire::UnknownUnconfirmed);
        assert_query(&query_reply(&mut app));
        assert_send(
            &observe(&mut app, Wire::UnknownUnconfirmed),
            "frozen answer",
        );
        let canonical: Wire = serde_json::from_value(status.clone()).unwrap();
        let expected = match &canonical {
            Wire::Allowed { .. } | Wire::Consumed { .. } | Wire::Handled => {
                AdvisoryReplyStatus::Accepted
            }
            Wire::Blocked { .. } | Wire::Dismissed { .. } => AdvisoryReplyStatus::Rejected,
            _ => AdvisoryReplyStatus::Unconfirmed,
        };
        let history_len = app.agents[&AgentId(0)].scrollback.len();
        let effects = admission_replay(&mut app, status);
        assert_query(&effects);
        assert_eq!(
            app.agents[&AgentId(0)].scrollback.len(),
            history_len,
            "no new terminal marker"
        );
        assert!(app.agents[&AgentId(0)].session.state.is_idle());
        assert!(app.agents[&AgentId(0)].session.current_prompt_id.is_none());
        assert_eq!(
            app.agents[&AgentId(0)]
                .session
                .in_flight_prompt
                .as_ref()
                .unwrap()
                .text,
            "frozen answer"
        );
        assert!(app.deferred_notification.is_none());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
        assert!(admission_replay(&mut app, serde_json::json!({"status":"handled"})).is_empty());
        assert!(observe(&mut app, canonical).is_empty());
        assert_eq!(projection(&app), expected);
    }
}

#[test]
fn u2_check_submitted_errors_and_terminal_never_authorize_retry() {
    for status in [
        Wire::Submitted,
        Wire::TimedOut,
        Wire::Unavailable {
            reason: "offline".into(),
        },
        Wire::InvalidEvidence {
            reason: "invalid".into(),
        },
        Wire::Allowed {
            route: chat_state::InputRoute::Fifo,
        },
        Wire::Blocked {
            reason: chat_state::InputBlockReason::ProcessInterrupted,
        },
    ] {
        let mut app = failed_reply_fixture();
        observe(&mut app, Wire::UnknownUnconfirmed);
        assert_query(&query_reply(&mut app));
        assert!(observe(&mut app, status).is_empty());
        assert_eq!(
            app.agents[&AgentId(0)].prompt.text(),
            "unrelated composer draft"
        );
    }
}

#[test]
fn u2_replay_bad_metadata_and_other_foreground_never_fake_a_terminal() {
    for other_foreground in [false, true] {
        let mut app = fixture();
        submit(&mut app, "frozen answer");
        if other_foreground {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.current_prompt_id = Some("another-real-turn".into());
            agent.session.state = AgentState::TurnRunning;
        }
        let history_len = app.agents[&AgentId(0)].scrollback.len();
        let mut response = acp::PromptResponse::new(acp::StopReason::EndTurn);
        response.meta = Some(
            serde_json::from_value(serde_json::json!({
                "sessionId": if other_foreground { SOURCE } else { "wrong-session" },
                "promptId": identity(), "grow/inputAdmission": {
                    "inputId": "original-input", "status": {"status":"handled"}
                }
            }))
            .unwrap(),
        );
        assert!(
            dispatch(
                Action::TaskComplete(TaskResult::PromptResponse {
                    agent_id: AgentId(0),
                    result: Ok(response),
                    http_status: None,
                    prompt_id: Some(identity()),
                }),
                &mut app
            )
            .is_empty()
        );
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), history_len);
        assert_ne!(projection(&app), AdvisoryReplyStatus::Accepted);
        assert!(app.deferred_notification.is_none());
        if other_foreground {
            assert!(app.agents[&AgentId(0)].session.state.is_turn_running());
            assert_eq!(
                app.agents[&AgentId(0)].session.current_prompt_id.as_deref(),
                Some("another-real-turn")
            );
        }
    }
}

#[test]
fn u2_check_transport_failure_preserves_frozen_payload_and_never_sends() {
    let mut app = failed_reply_fixture();
    observe(&mut app, Wire::UnknownUnconfirmed);
    assert_query(&query_reply(&mut app));
    let agent = &app.agents[&AgentId(0)];
    let failure = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: agent.session.session_id.clone().unwrap(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: None,
        response_prompt_id: None,
        response_source: None,
        status: Err("decode/transport failure".into()),
    };
    assert!(dispatch(Action::TaskComplete(failure), &mut app).is_empty());
    let queued = app.agents[&AgentId(0)]
        .session
        .pending_prompts
        .front()
        .unwrap();
    assert!(queued.requires_review);
    assert_eq!(queued.text, "frozen answer");
    assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
}

#[test]
fn u2_terminal_observation_unblocks_following_ordinary_fifo_once() {
    for terminal in [
        Wire::Handled,
        Wire::Blocked {
            reason: chat_state::InputBlockReason::ProcessInterrupted,
        },
    ] {
        let mut app = failed_reply_fixture();
        observe(&mut app, Wire::UnknownUnconfirmed);
        assert!(
            dispatch(
                Action::SendPrompt("following ordinary input".into()),
                &mut app
            )
            .is_empty()
        );
        assert_query(&query_reply(&mut app));
        assert_send(
            &observe(&mut app, Wire::UnknownUnconfirmed),
            "frozen answer",
        );
        assert_query(&admission_replay(
            &mut app,
            serde_json::json!({"status":"handled"}),
        ));
        let effects = observe(&mut app, terminal);
        assert!(
            matches!(effects.as_slice(), [Effect::SendPrompt { text, prompt_id, .. }]
            if text == "following ordinary input" && prompt_id != &identity())
        );
        assert!(observe(&mut app, Wire::Handled).is_empty());
    }
}

#[test]
fn u5_automatic_absent_keeps_real_queue_only_failure_frozen_until_explicit_check() {
    for explicit_status in [Wire::UnknownUnconfirmed, Wire::Absent] {
        let mut app = failed_reply_fixture();
        // The real failed RPC cannot restore over an ordinary composer draft:
        // its immutable answer must be queue-only, not an in-flight shortcut.
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.in_flight_prompt.is_none());
        let queued = agent.session.pending_prompts.front().unwrap();
        assert!(queued.requires_review);
        assert_eq!(queued.prompt_id.as_deref(), Some(identity().as_str()));
        assert_eq!(queued.text, "frozen answer");
        assert_eq!(agent.prompt.text(), "unrelated composer draft");

        assert!(
            observe(&mut app, Wire::Absent).is_empty(),
            "automatic durable Absent must not release a failed review row"
        );
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.in_flight_prompt.is_none());
        let queued = agent.session.pending_prompts.front().unwrap();
        assert!(queued.requires_review);
        assert_eq!(queued.prompt_id.as_deref(), Some(identity().as_str()));
        assert_eq!(queued.text, "frozen answer");
        assert_eq!(agent.prompt.text(), "unrelated composer draft");

        assert_query(&query_reply(&mut app));
        assert!(query_reply(&mut app).is_empty(), "rapid Check coalesces");
        assert_send(&observe(&mut app, explicit_status), "frozen answer");
        assert!(
            observe(&mut app, Wire::Absent).is_empty(),
            "late automatic Absent cannot dispatch twice"
        );
        assert!(observe(&mut app, Wire::UnknownUnconfirmed).is_empty());
        assert_eq!(
            app.agents[&AgentId(0)].prompt.text(),
            "unrelated composer draft"
        );
        assert_eq!(
            app.agents[&AgentId(0)]
                .session
                .in_flight_prompt
                .as_ref()
                .unwrap()
                .text,
            "frozen answer"
        );
    }
}

fn assert_send(effects: &[Effect], text: &str) {
    assert!(
        matches!(effects, [Effect::SendPrompt { prompt_id, text: sent, session_id, agent_id: AgentId(0), .. }]
        if prompt_id == &identity() && sent == text && session_id.0.as_ref() == SOURCE),
        "{effects:?}"
    );
}

#[test]
fn advisory_first_submit_sends_original_literal_payload_and_duplicate_only_observes() {
    let mut app = fixture();
    assert_send(&submit(&mut app, "/exit"), "/exit");
    assert_query(&submit(&mut app, "changed choice"));
    let queued = app.agents[&AgentId(0)]
        .session
        .pending_prompts
        .front()
        .unwrap();
    assert_eq!(queued.text, "/exit");
    assert!(queued.requires_review);
    assert!(observe(&mut app, Wire::Absent).is_empty());
    assert_ne!(projection(&app), AdvisoryReplyStatus::Accepted);
    assert_eq!(
        app.agents[&AgentId(0)]
            .session
            .in_flight_prompt
            .as_ref()
            .unwrap()
            .text,
        "/exit"
    );
}

#[test]
fn advisory_unknown_keeps_fixed_payload_and_never_claims_answered() {
    let mut app = fixture();
    submit(&mut app, "original");
    for status in [
        Wire::Submitted,
        Wire::UnknownUnconfirmed,
        Wire::TimedOut,
        Wire::Unavailable {
            reason: "offline".into(),
        },
        Wire::InvalidEvidence {
            reason: "bad".into(),
        },
    ] {
        assert!(observe(&mut app, status).is_empty());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
        assert!(
            app.agents[&AgentId(0)]
                .session
                .pending_prompts
                .front()
                .unwrap()
                .requires_review
        );
    }
    let agent = &app.agents[&AgentId(0)];
    let failure = TaskResult::InputAdmissionStatusResolved {
        agent_id: AgentId(0),
        session_id: agent.session.session_id.clone().unwrap(),
        binding_epoch: agent.session_binding_epoch,
        prompt_id: identity(),
        response_session_id: None,
        response_prompt_id: None,
        response_source: None,
        status: Err("transport unavailable".into()),
    };
    assert!(dispatch(Action::TaskComplete(failure), &mut app).is_empty());
    assert_eq!(projection(&app), AdvisoryReplyStatus::Unconfirmed);
    assert_query(&submit(&mut app, "replacement"));
    assert!(observe(&mut app, Wire::Absent).is_empty());
    assert_eq!(
        app.agents[&AgentId(0)]
            .session
            .in_flight_prompt
            .as_ref()
            .unwrap()
            .text,
        "original"
    );
}

#[test]
fn advisory_canonical_acceptance_is_read_only_even_after_inflight_disappears() {
    for status in [
        Wire::Allowed {
            route: chat_state::InputRoute::Fifo,
        },
        Wire::Consumed {
            route: chat_state::InputRoute::Fifo,
        },
        Wire::Handled,
    ] {
        let mut app = fixture();
        submit(&mut app, "original");
        assert!(observe(&mut app, status).is_empty());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Accepted);
        assert!(app.agents[&AgentId(0)].session.pending_prompts.is_empty());
        assert!(observe(&mut app, Wire::Absent).is_empty());
        assert!(observe(&mut app, Wire::Submitted).is_empty());
        assert!(submit(&mut app, "changed").is_empty());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Accepted);
        assert!(app.agents[&AgentId(0)].session.pending_prompts.is_empty());
    }
}

#[test]
fn advisory_terminal_rejection_never_reenters_and_preserves_copy_text() {
    for status in [
        Wire::Blocked {
            reason: chat_state::InputBlockReason::Hook {
                reason: "denied".into(),
            },
        },
        Wire::Dismissed {
            reason: chat_state::InputDismissReason::UserRemoved,
        },
    ] {
        let mut app = fixture();
        assert_send(
            &submit(&mut app, "copy this original reply"),
            "copy this original reply",
        );
        assert!(observe(&mut app, status).is_empty());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Rejected);
        assert!(submit(&mut app, "replacement").is_empty());
        assert!(observe(&mut app, Wire::Absent).is_empty());
        assert!(app.agents[&AgentId(0)].session.pending_prompts.is_empty());
        let scrollback = &app.agents[&AgentId(0)].scrollback;
        assert!((0..scrollback.len()).any(|idx| matches!(
            scrollback.get(idx).map(|entry| &entry.block),
            Some(RenderBlock::Notice(notice)) if notice.text.contains("copy this original reply")
        )));
    }
}

#[test]
fn advisory_cold_card_queries_durable_status_without_trusting_optimistic_echo() {
    let mut app = fixture();
    // No local queue/inflight is necessary for a replay card observation.
    assert!(observe(&mut app, Wire::Handled).is_empty());
    assert_eq!(projection(&app), AdvisoryReplyStatus::Accepted);
    assert!(submit(&mut app, "second answer").is_empty());
}

#[test]
fn advisory_observer_rejects_other_session_epoch_prompt_and_source() {
    for mismatch in 0..5 {
        let mut app = fixture();
        submit(&mut app, "frozen");
        observe(&mut app, Wire::UnknownUnconfirmed);
        assert_query(&query_reply(&mut app));
        let agent = &app.agents[&AgentId(0)];
        let session_id = agent.session.session_id.clone().unwrap();
        let mut result = TaskResult::InputAdmissionStatusResolved {
            agent_id: AgentId(0),
            session_id: session_id.clone(),
            binding_epoch: agent.session_binding_epoch,
            prompt_id: identity(),
            response_session_id: Some(session_id.0.to_string()),
            response_prompt_id: Some(identity()),
            response_source: Some("durable".into()),
            status: Ok(Wire::Absent),
        };
        if let TaskResult::InputAdmissionStatusResolved {
            agent_id,
            binding_epoch,
            response_session_id,
            response_prompt_id,
            response_source,
            ..
        } = &mut result
        {
            match mismatch {
                0 => *agent_id = AgentId(99),
                1 => *binding_epoch = binding_epoch.wrapping_add(1),
                2 => *response_session_id = Some("wrong".into()),
                3 => *response_prompt_id = Some("wrong".into()),
                _ => *response_source = Some("optimistic".into()),
            }
        }
        assert!(dispatch(Action::TaskComplete(result), &mut app).is_empty());
        assert_eq!(projection(&app), AdvisoryReplyStatus::Checking);
        assert!(
            app.agents[&AgentId(0)]
                .session
                .pending_prompts
                .front()
                .unwrap()
                .requires_review
        );
    }
}

#[test]
fn advisory_unknown_ack_explicit_retry_reuses_frozen_id_after_absent() {
    let mut app = fixture();
    assert_send(
        &submit(&mut app, "original wire text"),
        "original wire text",
    );
    // Exercise the existing failed Prompt RPC/F3 recovery, including its
    // automatic observation. Absent alone does not auto-retry that failure.
    let recovery = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Err("transport lost acknowledgement".into()),
            http_status: None,
            prompt_id: Some(identity()),
        }),
        &mut app,
    );
    assert_query(&recovery);
    assert!(observe(&mut app, Wire::Absent).is_empty());
    assert_query(&submit(&mut app, "different selected answer"));
    assert!(observe(&mut app, Wire::Absent).is_empty());
    let frozen = app.agents[&AgentId(0)]
        .session
        .pending_prompts
        .front()
        .unwrap();
    assert_eq!(frozen.text, "original wire text");
    assert_eq!(frozen.prompt_id.as_deref(), Some(identity().as_str()));
    assert!(frozen.requires_review);
    assert_query(&query_reply(&mut app));
    assert_send(&observe(&mut app, Wire::Absent), "original wire text");
}

#[test]
fn advisory_reply_from_ended_child_targets_primary_even_on_another_tab() {
    let mut app = fixture();
    let child_source = "ended:child";
    let make_view = |app: &AppView, id, sid: &str| {
        AgentView::new(
            crate::app::session::AgentSession::new(
                AgentId(id),
                app.acp_tx.clone(),
                Some(acp::SessionId::new(sid.to_owned())),
                crate::acp::model_state::ModelState::default(),
                std::path::PathBuf::from("/tmp"),
                shell::util::config::PermissionMode::Ask,
            ),
            crate::scrollback::state::ScrollbackState::new(),
        )
    };
    let mut child = Box::new(make_view(&app, 7, child_source));
    child.mark_as_subagent_view();
    child.scrollback.push_block(RenderBlock::ToolCall(
        crate::scrollback::ToolCallBlock::Other(
            OtherToolCallBlock::new("ask_user_question", "historical child question")
                .with_advisory_card(AdvisoryCard {
                    source_session_id: child_source.into(),
                    tool_call_id: CALL.into(),
                    questions: vec![],
                    status: AdvisoryCardStatus::Confirmed,
                    reply_status: AdvisoryReplyStatus::Unobserved,
                    reply_open_intent: false,
                }),
        ),
    ));
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .subagent_views
        .insert(child_source.into(), child);
    let other = make_view(&app, 1, "unrelated-tab");
    app.agents.insert(AgentId(1), other);
    app.active_view = ActiveView::Agent(AgentId(1));
    let effects = dispatch(
        Action::SubmitAdvisoryReply {
            source_session_id: child_source.into(),
            tool_call_id: CALL.into(),
            text: "child reply".into(),
        },
        &mut app,
    );
    let stable = format!("question-reply:{child_source}:{CALL}");
    assert!(
        matches!(effects.as_slice(), [Effect::SendPrompt { agent_id: AgentId(0), session_id, prompt_id, text, .. }]
        if session_id.0.as_ref() == SOURCE && prompt_id == &stable && text == "child reply")
    );
    assert!(app.agents[&AgentId(1)].session.pending_prompts.is_empty());
    assert!(
        app.agents[&AgentId(0)].subagent_views[child_source]
            .session
            .in_flight_prompt
            .is_none()
    );
    let recovery = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Err("child answer ACK lost".into()),
            http_status: None,
            prompt_id: Some(stable.clone()),
        }),
        &mut app,
    );
    assert_eq!(only_admission_queries(recovery).len(), 1);
    let query = dispatch(
        Action::QueryAdvisoryReplyAdmission {
            source_session_id: child_source.into(),
            tool_call_id: CALL.into(),
        },
        &mut app,
    );
    assert!(
        matches!(query.as_slice(), [Effect::QueryInputAdmissionStatus { agent_id: AgentId(0), prompt_id, .. }] if prompt_id == &stable)
    );
    let primary = &app.agents[&AgentId(0)];
    let retry = dispatch(
        Action::TaskComplete(TaskResult::InputAdmissionStatusResolved {
            agent_id: AgentId(0),
            session_id: primary.session.session_id.clone().unwrap(),
            binding_epoch: primary.session_binding_epoch,
            prompt_id: stable.clone(),
            response_session_id: Some(SOURCE.into()),
            response_prompt_id: Some(stable.clone()),
            response_source: Some("durable".into()),
            status: Ok(Wire::UnknownUnconfirmed),
        }),
        &mut app,
    );
    assert!(
        matches!(retry.as_slice(), [Effect::SendPrompt { agent_id: AgentId(0), session_id, prompt_id, text, .. }]
        if session_id.0.as_ref() == SOURCE && prompt_id == &stable && text == "child reply")
    );
    // Complete the primary input while an unrelated tab remains selected;
    // the ended child card is found by full identity, not active view.
    let mut response = acp::PromptResponse::new(acp::StopReason::EndTurn);
    response.meta =
        Some(serde_json::from_value(serde_json::json!({ "promptId": stable })).unwrap());
    let effects = dispatch(
        Action::TaskComplete(TaskResult::PromptResponse {
            agent_id: AgentId(0),
            result: Ok(response),
            http_status: None,
            prompt_id: Some(stable.clone()),
        }),
        &mut app,
    );
    let queries = only_admission_queries(effects);
    assert!(
        matches!(queries.as_slice(), [Effect::QueryInputAdmissionStatus { agent_id: AgentId(0), session_id, prompt_id, .. }]
        if session_id.0.as_ref() == SOURCE && prompt_id == &stable)
    );
}

#[test]
fn advisory_reply_does_not_restart_a_stopped_goal() {
    let mut app = fixture();
    let payload = serde_json::json!({
        "sessionId": SOURCE,
        "update": {
            "sessionUpdate": "goal_updated", "goal_id": "goal-stopped", "objective": "historical objective",
            "status": "paused", "token_budget": null, "tokens_used": 0, "elapsed_ms": 0,
            "created_at": "2026-08-24T00:00:00Z", "updated_at": "2026-08-24T00:01:00Z"
        }
    });
    let raw = serde_json::value::to_raw_value(&payload).unwrap();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    assert!(crate::app::acp_handler::handle(
        acp_transport::AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request: acp::ExtNotification::new("grow/session_notification", raw.into()),
            response_tx: tx,
        }),
        &mut app
    ));
    let before = app.agents[&AgentId(0)]
        .session
        .goal_state
        .as_ref()
        .unwrap()
        .status
        .clone();
    assert_send(&submit(&mut app, "late answer"), "late answer");
    assert_eq!(
        app.agents[&AgentId(0)]
            .session
            .goal_state
            .as_ref()
            .unwrap()
            .status,
        before
    );
}

#[test]
fn doctor_fix_replaces_only_local_advisory_edit_and_preserves_original_composer() {
    use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
    let mut app = fixture();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.prompt.set_text("original composer draft");
    let stashed = agent.prompt.stash();
    agent.replace_question_view(Some(
        QuestionViewState::new(CALL.into(), vec![], stashed).with_local_kind(
            LocalQuestionKind::AdvisoryReply {
                source_session_id: SOURCE.into(),
                tool_call_id: CALL.into(),
            },
        ),
    ));
    agent.prompt.set_text("temporary local answer");
    let paused_before = agent.session.turn_paused_duration;
    let target = doctor_fix_target(agent);
    let home = tempfile::tempdir().unwrap();
    open_doctor_fix_question(
        &mut app,
        target,
        Box::new(crate::diagnostics::test_fix_plan(home.path())),
    );
    let agent = &app.agents[&AgentId(0)];
    let doctor = agent.question_view.as_ref().unwrap();
    assert!(matches!(
        doctor.local_kind,
        Some(LocalQuestionKind::DoctorFix { .. })
    ));
    assert_eq!(doctor.stashed_prompt.text, "original composer draft");
    assert_eq!(agent.session.turn_paused_duration, paused_before);
}
