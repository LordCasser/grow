//! Completion blockers exercise the production sampler and turn loop.
use super::*;

async fn install_unresolved_child(actor: &SessionActor) {
    let mut plan = tool_types::LocalPlan::new(actor.session_id_string(), "work:blocked");
    plan.steps.push(tool_types::PlanStep {
        id: "parent-step".into(),
        content: "review".into(),
        priority: tool_types::PlanPriority::Normal,
        status: tool_types::PlanStepStatus::InProgress,
        requirements: None,
        removed: false,
    });
    plan.links.push(tool_types::PlanLink {
        id: "failed-child".into(),
        parent_step_id: "parent-step".into(),
        child_session_id: "failed-child".into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: 0,
        result_id: None,
        dispatch_failure: None,
    });
    plan.update_steps(
        tool_types::PlanUpdateMode::Merge,
        vec![tool_types::PlanStep {
            content: "updated requirements".into(),
            ..plan.steps[0].clone()
        }],
    )
    .unwrap();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan),
            },
        ))
        .await
        .unwrap();
}

#[test]
fn unchanged_planning_blockers_stop_sampling_and_goal_recovery() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            for structured in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                for index in 0..2 {
                    let response = if structured {
                        messages_turn(&[tool_use_block(
                            &format!("answer-{index}"), "StructuredOutput", r#"{"answer":"blocked"}"#,
                        )], "tool_use")
                    } else {
                        messages_turn(&[text_block("blocked; waiting for host repair")], END_TURN)
                    };
                    server.enqueue_response("/v1/messages", response);
                }
                // A broken gate fails promptly instead of looping in the fixture.
                server.enqueue_response("/v1/messages", ScriptedResponse::json(400,
                    json!({"type":"error","error":{"type":"invalid_request_error","message":"unexpected extra request"}})));
                let (actor, _) = goal_actor_with_sampler_and_max_turns(
                    &server, sampling_types::ApiBackend::Messages, None,
                ).await;
                // Verify that Agent completion recovery cannot restart this error.
                let (mut definition, bridge) = {
                    let agent = actor.agent.borrow();
                    (agent.definition().clone(), agent.tool_bridge().clone())
                };
                definition.completion_requirement = Some(agent::config::CompletionRequirement {
                    tool: "never_called".into(), reminder: "must not recover planning deadlock".into(),
                    recovery: Some(agent::config::RecoveryPolicy {
                        max_retries: 1, base_delay_ms: 0, max_delay_ms: 0,
                    }),
                });
                *actor.agent.borrow_mut() = agent::Agent::new(definition,
                    agent::PromptContext::default(), String::new(), None,
                    bridge);
                install_unresolved_child(&actor).await;
                let revision = activate_test_goal(&actor, "blocked-goal", "resolve the obligation", None).await;
                install_test_foreground(&actor, "blocked-turn").await;
                *actor.current_prompt_id.lock().unwrap() = Some("blocked-turn".into());
                let origin = crate::session::PromptOrigin::GoalContinuation {
                    goal_id: "blocked-goal".into(), definition_revision: revision,
                };
                let schema = structured.then(|| json!({"type":"object",
                    "properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}));
                let result = tokio::time::timeout(std::time::Duration::from_secs(10), actor.handle_prompt(
                    "blocked-turn", Vec::new(), origin.clone(), Vec::new(),
                    crate::session::TurnKind::Internal,
                    vec![acp::ContentBlock::Text(acp::TextContent::new("continue"))],
                    tool_types::BehaviorId::Goal, None, None, false, schema, None,
                )).await.expect("unchanged obligations must stop promptly");
                assert_eq!(server.messages_request_count(), 2, "structured={structured}");
                let error = result.as_ref().expect_err("unresolved work cannot succeed");
                let data = error.data.as_ref().unwrap();
                assert_eq!(data["error_kind"], "planning_completion_blocked");
                assert!(data["message"].as_str().unwrap().contains("failed-child"));
                assert!(!actor.planning_completion_blockers().await.unwrap().is_empty());
                assert!(!actor.events.has_active_step(), "planning errors must close the Runtime Step");

                // Use the same completion/Goal settlement sequence as run_session.
                let (_, _, stop) = SessionActor::post_turn_goal_degradation_plan(&result, Some(&origin));
                actor.handle_completion("blocked-turn".into(), result).await;
                assert!(actor.apply_goal_stop_after_turn("blocked-turn", stop.unwrap()).await);
                assert_eq!(actor.goal_tracker.lock().snapshot().unwrap().status,
                    crate::session::goal_tracker::GoalStatus::Paused);
                let (completion_tx, _completion_rx) = mpsc::unbounded_channel();
                super::super::super::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx).await;
                assert!(!actor.goal_drive.is_running());
                assert_eq!(server.messages_request_count(), 2);
            }
        }));
    });
}

#[test]
fn progress_steer_keeps_work_running_and_the_real_final_still_checks_deliveries() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let foreground = || {
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Messages,
                )
            };
            let _initial = server.expect_response(
                "first completion attempt",
                foreground(),
                messages_turn(&[text_block("Checking the required review.")], END_TURN),
            );
            let mut before_steer = server.expect_response_blocked(
                "step after the planning reminder",
                foreground(),
                messages_turn(&[text_block("Review still pending.")], END_TURN),
            );
            let _progress = server.expect_response(
                "progress reply",
                foreground(),
                messages_turn(
                    &[
                        text_block("The implementation is ready; the required review is pending."),
                        tool_use_block("progress", "GrowContinueAfterProgress", "{}"),
                    ],
                    "tool_use",
                ),
            );
            let mut final_step = server.expect_response_blocked(
                "actual final",
                foreground(),
                messages_turn(&[text_block("This is the final handoff.")], END_TURN),
            );
            let (actor, _) = goal_actor_with_sampler_and_max_turns(
                &server,
                sampling_types::ApiBackend::Messages,
                None,
            )
            .await;
            install_unresolved_child(&actor).await;
            let (result, ()) = tokio::join!(run_user_turn(&actor, "progress-turn"), async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    before_steer.wait_blocked(),
                )
                .await
                .unwrap();
                let turn = actor.events.current_turn().unwrap();
                actor
                    .admit_human_steer(
                        "progress-turn",
                        "Is the main work complete?".into(),
                        Vec::new(),
                        Some("progress-question".into()),
                    )
                    .await
                    .unwrap();
                before_steer.release();

                tokio::time::timeout(std::time::Duration::from_secs(5), final_step.wait_blocked())
                    .await
                    .expect("progress reply should lead to another step in the same turn");
                assert_eq!(actor.events.current_turn(), Some(turn));
                let requests = server.request_bodies();
                assert!(
                    !requests[0]["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| { tool["name"] == "GrowContinueAfterProgress" })
                );
                assert!(
                    requests[2]["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|tool| { tool["name"] == "GrowContinueAfterProgress" })
                );
                assert!(
                    requests[2]
                        .to_string()
                        .contains("Is the main work complete?")
                );
                let next_request = requests[3].to_string();
                assert!(
                    next_request
                        .contains("The implementation is ready; the required review is pending.")
                );
                assert!(
                    next_request.contains("Progress reply delivered. Continue the original work.")
                );
                assert_eq!(
                    next_request
                        .matches("This work cannot be declared complete yet")
                        .count(),
                    1
                );
                final_step.release();
            });
            let error = result.expect_err("the final must still check unresolved delivery");
            assert_eq!(
                error.data.unwrap()["error_kind"],
                "planning_completion_blocked"
            );
            assert_eq!(server.messages_request_count(), 4);
        }));
    });
}

#[test]
fn resolved_planning_blockers_allow_the_next_final() {
    run_with_session_stack(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let _first = server.expect_response(
                "initial planning attempt",
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(&[text_block("inspect the failure")], END_TURN),
            );
            let mut second = server.expect_response_blocked(
                "after planning reminder",
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Messages,
                ),
                messages_turn(&[text_block("finished after release")], END_TURN),
            );
            let (actor, _) = goal_actor_with_sampler_and_max_turns(
                &server,
                sampling_types::ApiBackend::Messages,
                None,
            )
            .await;
            install_unresolved_child(&actor).await;
            let (result, ()) = tokio::join!(run_user_turn(&actor, "resolved-turn"), async {
                tokio::time::timeout(std::time::Duration::from_secs(5), second.wait_blocked())
                    .await
                    .unwrap();
                let snapshot = actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let mut plan = snapshot.plan;
                plan.mark_link_undispatched("failed-child", "confirmed startup failure")
                    .unwrap();
                plan.reviews.push(tool_types::DeliveryReview {
                    link_id: "failed-child".into(),
                    result_id: None,
                    reviewed_source_revision: plan.revision,
                    decision: tool_types::ReviewDecision::Released,
                    reason: Some("replacement completed the work".into()),
                    verification_refs: Vec::new(),
                });
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                        chat_state::timeline::PlanningEvent {
                            previous: Some(snapshot.seq),
                            plan: Box::new(plan),
                        },
                    ))
                    .await
                    .unwrap();
                second.release();
            });
            result.expect("resolved obligations must allow normal completion");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(server.messages_request_count(), 2);
        }));
    });
}
