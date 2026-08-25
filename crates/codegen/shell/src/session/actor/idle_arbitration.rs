//! Idle admission and manual-compaction arbitration for the session actor.
use super::*;

pub(super) fn spawn_manual_compaction(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
    user_context: Option<String>,
    respond_to: Option<
        tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
    >,
) {
    tokio::task::spawn_local(async move {
        let result = session.run_compact(user_context).await;
        if let Some(respond_to) = respond_to {
            let response = result
                .as_ref()
                .map(|_| crate::session::CompactConversationStatus::Completed)
                .map_err(Clone::clone);
            let _ = respond_to.send(response);
        } else if let Err(error) = &result {
            session
                .send_host_turn_slash_command_output(&format!(
                    "Scheduled compaction failed: {error}"
                ))
                .await;
        }
        session.state.lock().await.foreground = ForegroundState::Idle;
        SessionActor::maybe_start_running_task(session.clone(), completion_tx.clone()).await;
        SessionActor::maybe_drain_notifications(session.clone(), completion_tx).await;
        session.emit_session_idle_if_idle().await;
    });
}

pub(super) async fn maybe_start_pending_manual_compaction(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
) -> bool {
    let user_context = {
        let mut state = session.state.lock().await;
        if !state.foreground.is_idle() {
            return false;
        }
        let Some(user_context) = state.pending_manual_compact.take() else {
            return false;
        };
        state.foreground = ForegroundState::Compaction;
        user_context
    };
    spawn_manual_compaction(session, completion_tx, user_context, None);
    true
}

/// Apply the complete idle-admission order for an external idle permit.
/// Restored receipts may predate the permit, so the Goal driver must never be
/// called directly from the select branch.
pub(super) async fn arbitrate_idle_wake(
    session: std::sync::Arc<SessionActor>,
    completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
) {
    if !maybe_start_pending_manual_compaction(session.clone(), completion_tx.clone()).await {
        SessionActor::maybe_start_running_task(session.clone(), completion_tx.clone()).await;
    }
    SessionActor::maybe_drain_notifications(session.clone(), completion_tx.clone()).await;
    if session.state.lock().await.foreground.is_idle() {
        session.drive_goal_on_idle(completion_tx).await;
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
        completion_tx: tokio::sync::mpsc::UnboundedSender<(String, PromptTurnResult)>,
        respond_to: Option<
            tokio::sync::oneshot::Sender<acp::Result<crate::session::CompactConversationStatus>>,
        >,
    ) {
        let mut state = self.state.lock().await;
        if state.foreground.regular().is_some() {
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
        } else if matches!(state.foreground, ForegroundState::Compaction)
            || self.compaction.lease.is_in_flight()
        {
            drop(state);
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(
                    crate::session::CompactConversationStatus::AlreadyRunning,
                ));
            }
        } else {
            state.foreground = ForegroundState::Compaction;
            drop(state);
            spawn_manual_compaction(
                std::sync::Arc::clone(self),
                completion_tx,
                user_context,
                respond_to,
            );
        }
    }
}

#[cfg(test)]
mod idle_admission_tests {
    use super::*;

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
}
