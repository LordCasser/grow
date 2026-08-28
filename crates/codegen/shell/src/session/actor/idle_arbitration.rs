//! Idle admission and manual-compaction arbitration for the session actor.
use super::*;

pub(super) fn spawn_manual_compaction(
    session: std::sync::Arc<SessionActor>,
    user_context: Option<String>,
    respond_to: Option<
        tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
    >,
) {
    let Some(activity) = session.session_activities.try_start("manual_compaction") else {
        if let Some(respond_to) = respond_to {
            let _ = respond_to.send(Err(
                acp::Error::internal_error().data("session is shutting down")
            ));
        }
        let _ = session
            .event_tx
            .send(SessionEvent::ManualCompactionFinished { failure: None });
        return;
    };
    tokio::task::spawn_local(async move {
        let _activity = activity;
        let outcome = std::panic::AssertUnwindSafe(session.run_compact(user_context))
            .catch_unwind()
            .await;
        let failure = match outcome {
            Ok(result) => {
                if let Some(respond_to) = respond_to {
                    let response = result
                        .as_ref()
                        .map(|_| crate::session::CompactConversationStatus::Completed)
                        .map_err(Clone::clone);
                    let _ = respond_to.send(response);
                } else if let Err(error) = &result {
                    session
                        .send_host_turn_slash_command_error(
                            "Scheduled compaction failed",
                            format!(
                                "Reason: {error}\nThe session remains usable; retry /compact or continue after reducing context."
                            ),
                        )
                        .await;
                }
                None
            }
            Err(payload) => {
                let panic = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                let message = format!("manual compaction owner panicked: {panic}");
                tracing::error!(%message);
                {
                    let mut state = session.state.lock().await;
                    state.termination.request(TerminationState::Fatal);
                }
                if let Some(respond_to) = respond_to {
                    let _ =
                        respond_to.send(Err(acp::Error::internal_error().data(message.clone())));
                }
                Some(message)
            }
        };
        let _ = session
            .event_tx
            .send(SessionEvent::ManualCompactionFinished { failure });
    });
}

pub(super) async fn maybe_start_pending_manual_compaction(
    session: std::sync::Arc<SessionActor>,
) -> bool {
    let user_context = {
        let mut state = session.state.lock().await;
        if !state.termination.is_open() || !state.foreground.is_idle() {
            return false;
        }
        let Some(user_context) = state.pending_manual_compact.take() else {
            return false;
        };
        state.foreground = ForegroundState::Compaction;
        user_context
    };
    spawn_manual_compaction(session, user_context, None);
    true
}

/// Apply the complete idle-admission order for an external idle permit.
/// Restored receipts may predate the permit, so the Goal driver must never be
/// called directly from the select branch.
pub(super) async fn arbitrate_idle_wake(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
) {
    if !session.state.lock().await.termination.is_open() {
        return;
    }
    session.apply_pending_step_controls_if_idle().await;
    if !maybe_start_pending_manual_compaction(session.clone()).await {
        SessionActor::maybe_start_running_task(session.clone(), completion_tx.clone()).await;
    }
    SessionActor::maybe_drain_notifications(session.clone(), completion_tx.clone()).await;
    if session.state.lock().await.foreground.is_idle() {
        session.schedule_goal_on_idle(completion_tx);
    }
}

impl SessionActor {
    /// `CompactSession` admission on the mailbox: decide the immediate
    /// status and either queue a pending compact behind the running turn or
    /// spawn the foreground compaction. Extracted from the command arm so
    /// the mailbox-not-blocked property is directly testable (the command
    /// must be accepted while a background Goal verification stage runs).
    pub(super) async fn admit_manual_compaction(
        self: &std::sync::Arc<Self>,
        user_context: Option<String>,
        respond_to: Option<
            tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
        >,
    ) {
        let mut state = self.state.lock().await;
        if !state.termination.is_open() {
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(
                    acp::Error::internal_error().data("session is shutting down")
                ));
            }
        } else if matches!(state.foreground, ForegroundState::Compaction)
            || self.compaction.lease.is_in_flight()
        {
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(
                    crate::session::CompactConversationStatus::AlreadyRunning,
                ));
            }
        } else if !state.foreground.is_idle() || state.behavior_control_worker_active {
            let status = if state.pending_manual_compact.is_some() {
                crate::session::CompactConversationStatus::AlreadyRunning
            } else {
                state.pending_manual_compact = Some(user_context);
                crate::session::CompactConversationStatus::Scheduled
            };
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(status));
            }
        } else {
            state.foreground = ForegroundState::Compaction;
            drop(state);
            spawn_manual_compaction(std::sync::Arc::clone(self), user_context, respond_to);
        }
    }
}

#[cfg(test)]
mod idle_admission_tests {
    use super::*;

    #[tokio::test]
    async fn terminating_idle_wake_does_not_admit_any_work() {
        for termination in [TerminationState::Graceful, TerminationState::Fatal] {
            tokio::task::LocalSet::new()
                .run_until(async {
                    let (actor, _gateway_rx) =
                        crate::session::actor::tests::support::build_actor().await;

                    // Seed every idle producer that the arbiter could admit:
                    // a queued user prompt, a pending manual compaction, an
                    // active Goal continuation, and a durable notification.
                    actor
                        .goal_runtime_available
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    actor
                        .goal_tracker
                        .lock()
                        .create_goal(
                            "goal-termination-fence".into(),
                            "must not be scheduled after termination".into(),
                            None,
                            "2026-08-24T00:00:00Z".into(),
                        )
                        .unwrap();
                    actor
                        .behavior
                        .lock()
                        .select_behavior(tool_types::BehaviorId::Goal);
                    actor
                        .receive_notification(
                            chat_state::NotificationSource::TaskCompleted {
                                task_id: "termination-fence-task".into(),
                                task_kind: chat_state::NotificationTaskKind::Task,
                                owner: chat_state::NotificationOwner::Session,
                            },
                            chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                            "must remain pending during termination".into(),
                        )
                        .await
                        .unwrap();

                    let (prompt, _prompt_rx) =
                        crate::session::actor::tests::support::user_item_with_rx(
                            "termination-fence-prompt",
                            "test-client",
                        );
                    let (control_tx, _control_rx) = tokio::sync::oneshot::channel();
                    let control_route = SessionActor::selection_route_for_test(
                        acp::ModelId::new("provider/termination-fence"),
                        sampler::SamplerConfig::default(),
                        85,
                    );

                    let notification_count_before = actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("test Timeline must be available")
                        .len();
                    {
                        let mut state = actor.state.lock().await;
                        state.termination = termination;
                        state.pending_manual_compact = Some(Some("queued compact".into()));
                        state.pending_inputs.push_back(prompt);
                        state.pending_step_controls.admit_sampling(
                            control_route,
                            None,
                            None,
                            control_tx,
                        );
                    }

                    let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();
                    arbitrate_idle_wake(actor.clone(), completion_tx).await;
                    // A detached Goal preparation must not win the race after
                    // the arbiter returns either.
                    tokio::task::yield_now().await;

                    let state = actor.state.lock().await;
                    assert_eq!(state.termination, termination);
                    assert!(state.foreground.is_idle());
                    assert_eq!(state.pending_inputs.len(), 1);
                    assert_eq!(
                        state
                            .pending_inputs
                            .front()
                            .map(|item| item.prompt_id.as_str()),
                        Some("termination-fence-prompt")
                    );
                    assert_eq!(
                        state
                            .pending_manual_compact
                            .as_ref()
                            .and_then(|value| value.as_ref()),
                        Some(&"queued compact".to_string())
                    );
                    assert_eq!(state.pending_step_controls.len(), 1);
                    drop(state);
                    assert_eq!(
                        actor
                            .chat_state_handle
                            .pending_notifications()
                            .await
                            .expect("test Timeline must be available")
                            .len(),
                        notification_count_before
                    );
                    assert!(
                        !actor.goal_drive.is_running(),
                        "termination must fence Goal scheduling"
                    );
                })
                .await;
        }
    }

    #[tokio::test]
    async fn restored_notification_wins_an_idle_permit_before_goal_continuation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .goal_runtime_available
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish the architecture migration".into(),
                        None,
                        "2026-08-24T00:00:00Z".into(),
                    )
                    .unwrap();
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "outside-goal".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                            owner: chat_state::NotificationOwner::Session,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "independent task completed".into(),
                    )
                    .await
                    .unwrap();
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                arbitrate_idle_wake(actor.clone(), completion_tx).await;

                let state = actor.state.lock().await;
                assert!(matches!(
                    state.foreground.regular().map(|task| &task.origin),
                    Some(crate::session::PromptOrigin::TaskCompleted { task_id })
                        if task_id == "outside-goal"
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn active_goal_continues_after_manual_compaction_releases_the_foreground() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .goal_runtime_available
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish the architecture migration".into(),
                        None,
                        "2026-08-24T00:00:00Z".into(),
                    )
                    .unwrap();
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                actor.state.lock().await.foreground = ForegroundState::Idle;
                arbitrate_idle_wake(actor.clone(), completion_tx).await;

                tokio::time::timeout(std::time::Duration::from_secs(1), async {
                    loop {
                        let admitted = {
                            let state = actor.state.lock().await;
                            matches!(
                                state.foreground.regular().map(|task| &task.origin),
                            Some(crate::session::PromptOrigin::GoalContinuation { goal_id, .. })
                                    if goal_id == "goal-1"
                            )
                        };
                        if admitted {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("Goal continuation should be admitted after compaction");
            })
            .await;
    }

    #[tokio::test]
    async fn user_input_arriving_before_async_goal_admission_keeps_priority() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .goal_runtime_available
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish the architecture migration".into(),
                        None,
                        "2026-08-24T00:00:00Z".into(),
                    )
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                actor.schedule_goal_on_idle(completion_tx);
                actor.state.lock().await.pending_inputs.push_back(
                    crate::session::actor::tests::support::user_item("user-wins", "test-client"),
                );
                tokio::task::yield_now().await;
                tokio::task::yield_now().await;

                let state = actor.state.lock().await;
                assert!(state.foreground.is_idle());
                assert_eq!(state.pending_inputs.front().unwrap().prompt_id, "user-wins");
            })
            .await;
    }
}
