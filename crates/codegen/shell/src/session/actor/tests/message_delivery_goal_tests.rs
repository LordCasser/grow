//! Session-owned inquiry evidence obeys actual Goal and user FIFO admission.
use super::*;

#[test]
fn paused_and_budget_limited_goals_retain_consultation_until_explicit_restart() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::goal_tracker::{GoalPauseReason, GoalStatus};
            for budget_limited in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let (actor, _) = goal_actor_with_sampler_and_max_turns(&server, sampling_types::ApiBackend::Messages, Some(1)).await;
                let revision = activate_test_goal(&actor, "consultation-goal", "finish the approved objective", budget_limited.then_some(1)).await;
                let (completion_tx, mut completions) = mpsc::unbounded_channel();
                if budget_limited {
                    server.enqueue_response("/v1/messages", messages_turn(&[text_block("Initial Goal work spends its real provider usage.")], END_TURN));
                    crate::session::actor::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx.clone()).await;
                    let (prompt, result) = tokio::time::timeout(std::time::Duration::from_secs(10), completions.recv()).await.unwrap().unwrap();
                    assert!(result.is_ok(), "{result:?}");
                    actor.handle_completion(prompt, result).await;
                } else {
                    actor.auto_pause_goal_if_active(GoalPauseReason::User).await;
                }
                let outcome = answer_live_consultation(&actor, &server, "while-goal-closed", "What did this session advise?", "Keep the investigation result as consultation evidence.").await;
                assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered, "{outcome:?}");
                let stopped = if budget_limited { GoalStatus::BudgetLimited } else { GoalStatus::Paused };
                assert_eq!(actor.goal_tracker.lock().status(), Some(stopped));
                let consultation_id = actor.chat_state_handle.pending_notifications().await.unwrap()[0].id.clone();
                actor.receive_notification(chat_state::NotificationSource::TaskCompleted {
                    task_id: "old-goal-task".into(), task_kind: chat_state::NotificationTaskKind::Task,
                    owner: chat_state::NotificationOwner::Goal { goal_id: "consultation-goal".into(), definition_revision: revision },
                }, chat_state::NotificationSourceVersion::Ordinal { value: 1 }, "OLD_GOAL_ONLY_EVIDENCE".into()).await.unwrap();
                crate::session::actor::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx.clone()).await;
                assert!(actor.state.lock().await.foreground.is_idle());
                assert!(completions.try_recv().is_err());
                let before_restart = if budget_limited { 2 } else { 1 };
                assert_eq!(server.requests().len(), before_restart, "only prior authorized work and the actual Sideband may spend while closed");
                let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].id, consultation_id, "Session evidence survives stale Goal evidence dismissal");
                assert_eq!(actor.goal_tracker.lock().status(), Some(stopped));
                if budget_limited {
                    let message = actor.update_goal_token_budget(None).await.unwrap();
                    assert!(message.contains("Restart"), "{message}");
                }
                assert!(actor.restart_goal().await.starts_with("Goal restarted."));
                server.enqueue_response("/v1/messages", messages_turn(&[text_block("Approved Goal continuation uses the consultation.")], END_TURN));
                crate::session::actor::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx).await;
                let (prompt, result) = tokio::time::timeout(std::time::Duration::from_secs(10), completions.recv()).await.unwrap().unwrap();
                assert!(result.is_ok(), "{result:?}");
                actor.handle_completion(prompt, result).await;
                assert_eq!(server.requests().len(), before_restart + 1);
                let request = server.request_bodies().last().unwrap().to_string();
                assert!(request.contains("What did this session advise?"));
                assert!(request.contains("Keep the investigation result as consultation evidence."));
                assert!(!request.contains("OLD_GOAL_ONLY_EVIDENCE"));
                assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. }) if notification_ids.contains(&consultation_id))).count(), 1);
                let restarted = events.iter().rev().find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { identity, .. }) => Some(identity), _ => None,
                }).unwrap();
                assert_eq!(restarted.origin, "goal_continuation");
                assert_eq!(restarted.goal_id.as_deref(), Some("consultation-goal"));
                assert_eq!(restarted.goal_definition_revision, Some(actor.goal_tracker.lock().snapshot().unwrap().definition_revision));

            }
        }));
    });
}

#[test]
fn held_user_fifo_blocks_passive_wake_then_saved_input_and_consultation_share_real_turn() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::session::prompt_queue::{QueueControlOperation, QueueControlRequest};
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _) =
                actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            communication_agent(&actor).await;
            let (reply, mut response) = tokio::sync::oneshot::channel();
            actor
                .queue_input(
                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                        "original queued user input",
                    ))],
                    "held-user".into(),
                    crate::session::PromptOrigin::User,
                    crate::session::TurnKind::User,
                    Some("editing-client".into()),
                    None,
                    false,
                    None,
                    reply,
                    None,
                )
                .await;
            let request = |operation, expected_version, new_text| QueueControlRequest {
                session_id: actor.session_id_string(),
                operation,
                id: "held-user".into(),
                expected_version,
                edit_id: Some("edit-passive-input".into()),
                new_text,
                leader_client_id: Some(1),
            };
            let hold = actor
                .handle_queue_control(request(QueueControlOperation::Hold, 0, None))
                .await;
            assert!(hold.applied, "{hold:?}");
            let outcome = answer_live_consultation(
                &actor,
                &server,
                "during-fifo-edit",
                "What should the saved user task know?",
                "The consultation completed while the FIFO head was held.",
            )
            .await;
            assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered);
            let (completion_tx, mut completions) = mpsc::unbounded_channel();
            crate::session::actor::idle_arbitration::arbitrate_idle_wake(
                actor.clone(),
                completion_tx.clone(),
            )
            .await;
            assert!(actor.state.lock().await.foreground.is_idle());
            assert!(response.try_recv().is_err());
            assert!(completions.try_recv().is_err());
            assert_eq!(server.requests().len(), 1);
            assert_eq!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .len(),
                1
            );
            let saved = actor
                .handle_queue_control(request(
                    QueueControlOperation::Save,
                    0,
                    Some("saved edited user input".into()),
                ))
                .await;
            assert!(saved.applied, "{saved:?}");
            server.enqueue_response(
                "/v1/messages",
                messages_turn(
                    &[text_block(
                        "The saved input and passive context were handled.",
                    )],
                    END_TURN,
                ),
            );
            crate::session::actor::idle_arbitration::arbitrate_idle_wake(
                actor.clone(),
                completion_tx,
            )
            .await;
            let (prompt, result) =
                tokio::time::timeout(std::time::Duration::from_secs(10), completions.recv())
                    .await
                    .unwrap()
                    .unwrap();
            assert!(result.is_ok(), "{result:?}");
            actor.handle_completion(prompt, result).await;
            tokio::time::timeout(std::time::Duration::from_secs(5), response)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(server.requests().len(), 2);
            let wire = server.request_bodies().last().unwrap().to_string();
            assert!(wire.contains("saved edited user input"));
            assert!(wire.contains(sampling_types::RECEIVE_AGENT_MESSAGE_TOOL_NAME));
            assert!(wire.contains("live-child-session"));
            assert!(wire.contains("What should the saved user task know?"));
            assert!(wire.contains("The consultation completed while the FIFO head was held."));
            assert!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .is_empty()
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { prompt_id, identity, prompt_text, .. })
                    if prompt_id == "held-user" && identity.origin == "user" && prompt_text.contains("saved edited user input"))));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                    ))
                    .count(),
                1
            );
        }));
    });
}

#[test]
fn actual_goal_budget_closes_final_successor_with_consultation_still_pending() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let mut main = server.expect_response_blocked("Goal final before actual budget settlement",
                test_support::InferenceRequestMatcher::foreground(test_support::InferenceEndpoint::Messages),
                messages_turn(&[text_block("The admitted Goal response spends its remaining budget.")], END_TURN));
            let (actor, _) = goal_actor_with_sampler_and_max_turns(&server, sampling_types::ApiBackend::Messages, None).await;
            // Admit the Session-owned auxiliary before Goal activation. Its
            // captured absent Goal owner must not migrate into the new budget.
            let mut consultation = server.expect_response_blocked("Session consultation admitted before Goal activation",
                test_support::InferenceRequestMatcher::auxiliary(test_support::InferenceEndpoint::Messages),
                messages_turn(&[text_block("Session evidence must wait when Goal spending closes.")], END_TURN));
            let (inquiry, response) = live_consultation(&actor, "before-budget-final", "What must the next authorized task know?");
            actor.enqueue_coordination_inquiry(inquiry).await;
            tokio::time::timeout(std::time::Duration::from_secs(10), consultation.wait_blocked()).await.unwrap();
            activate_test_goal(&actor, "final-budget-consultation", "complete the approved goal", Some(1)).await;
            let (completion_tx, mut completions) = mpsc::unbounded_channel();
            crate::session::actor::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx.clone()).await;
            tokio::time::timeout(std::time::Duration::from_secs(10), main.wait_blocked()).await.unwrap();
            consultation.release();
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), response).await.unwrap().unwrap();
            assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered, "{outcome:?}");
            let receipt = actor.chat_state_handle.pending_notifications().await.unwrap()[0].id.clone();
            assert_eq!(server.requests().len(), 2);
            main.release();
            let (prompt, result) = tokio::time::timeout(std::time::Duration::from_secs(10), completions.recv()).await.unwrap().unwrap();
            assert!(result.is_ok(), "{result:?}");
            actor.handle_completion(prompt, result).await;
            crate::session::actor::idle_arbitration::arbitrate_idle_wake(actor.clone(), completion_tx).await;
            assert_eq!(actor.goal_tracker.lock().status(), Some(crate::session::goal_tracker::GoalStatus::BudgetLimited));
            // The durable BudgetLimited transition retires the active window;
            // its transient Exhausted flag no longer represents lifecycle admission.
            assert!(actor.goal_usage_window.active_goal_id().is_none());
            assert!(actor.state.lock().await.foreground.is_idle());
            assert!(completions.try_recv().is_err());
            assert_eq!(server.requests().len(), 2, "consultation cannot justify a successor after actual Goal usage closes admission");
            assert_eq!(actor.chat_state_handle.pending_notifications().await.unwrap()[0].id, receipt);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(!events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { notification_ids, .. }) if notification_ids.contains(&receipt))));
            assert_eq!(events.iter().filter(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. }))).count(), 1);
        }));
    });
}
