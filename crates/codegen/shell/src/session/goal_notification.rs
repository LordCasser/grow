//! Goal notification projection and presentation helpers.

use crate::extensions::notification::{
    SessionNotification as GrowSessionNotification, SessionUpdate as GrowSessionUpdate,
};
use crate::session::goal_tracker::{GoalState, GoalStatus, GoalTracker};
use crate::session::persistence::PersistenceMsg;

pub(crate) struct GoalNotifySender {
    session_id: agent_client_protocol::schema::v1::SessionId,
    gateway: acp_transport::AcpAgentGatewaySender,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    budget: sampler::PreviewEventBudget,
    preview: std::sync::Arc<parking_lot::Mutex<Option<(String, u32, bool)>>>,
    chat_state: chat_state::ChatStateHandle,
    source_revision: u64,
}

impl GoalNotifySender {
    pub(crate) fn new(
        session_id: agent_client_protocol::schema::v1::SessionId,
        gateway: acp_transport::AcpAgentGatewaySender,
        persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
        budget: sampler::PreviewEventBudget,
        preview: std::sync::Arc<parking_lot::Mutex<Option<(String, u32, bool)>>>,
        chat_state: chat_state::ChatStateHandle,
        source_revision: u64,
    ) -> Self {
        Self {
            session_id,
            gateway,
            persistence_tx,
            budget,
            preview,
            chat_state,
            source_revision,
        }
    }

    pub(crate) fn emit_goal_updated(
        &self,
        goal: Option<(GoalState, u64)>,
        tokens_used: i64,
    ) -> impl std::future::Future<Output = ()> + '_ {
        async move {
            let Some((goal, elapsed_ms)) = goal else {
                return;
            };
            let mut meta = None;
            crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
            let notification = GrowSessionNotification {
                session_id: self.session_id.clone(),
                update: build_goal_updated(&goal, tokens_used, elapsed_ms),
                meta: meta.map(serde_json::Value::Object),
            };
            let Some(prepared) = crate::session::notifications::prepare_auxiliary_grow(
                &self.persistence_tx,
                &self.budget,
                &self.preview,
                notification.clone(),
                true,
            ) else {
                return;
            };
            if let Err(error) =
                crate::session::notifications::record_display_projection_expectation(
                    &self.chat_state,
                    &notification,
                    chat_state::DisplayProjectionKind::GoalUpdated,
                    format!(
                        "goal:{}:definition:{}",
                        goal.goal_id, goal.definition_revision
                    ),
                    self.source_revision,
                )
                .await
            {
                tracing::error!(%error, "Goal display projection expectation was not committed; suppressing publication");
                return;
            }
            crate::session::notifications::publish_prepared_auxiliary_grow(
                &self.gateway,
                &self.persistence_tx,
                "grow/session_notification",
                prepared,
            );
        }
    }

    pub(crate) fn snapshot_goal(tracker: &GoalTracker) -> Option<(GoalState, u64)> {
        tracker
            .snapshot()
            .cloned()
            .map(|goal| (goal, tracker.elapsed_ms()))
    }

    pub(crate) fn send_update(&self, update: GrowSessionUpdate) {
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        let notification = GrowSessionNotification {
            session_id: self.session_id.clone(),
            update,
            meta: meta.map(serde_json::Value::Object),
        };
        crate::session::notifications::dispatch_auxiliary_grow(
            &self.gateway,
            &self.persistence_tx,
            &self.budget,
            &self.preview,
            "grow/session_notification",
            notification,
            true,
        );
    }
}

fn status_name(status: GoalStatus) -> &'static str {
    match status {
        GoalStatus::Active => "active",
        GoalStatus::Paused => "paused",
        GoalStatus::Blocked => "blocked",
        GoalStatus::BudgetLimited => "budget_limited",
        GoalStatus::Complete => "complete",
    }
}

pub(crate) fn build_goal_updated(
    goal: &GoalState,
    tokens_used: i64,
    elapsed_ms: u64,
) -> GrowSessionUpdate {
    GrowSessionUpdate::GoalUpdated {
        goal_id: goal.goal_id.clone(),
        objective: goal.objective.clone(),
        status: status_name(goal.status).to_string(),
        token_budget: goal.token_budget,
        tokens_used,
        usage_breakdown: goal.usage_breakdown.unwrap_or_default(),
        usage_incomplete: goal.usage_incomplete,
        elapsed_ms,
        created_at: goal.created_at.clone(),
        updated_at: goal.updated_at.clone(),
        status_message: goal.status_message.clone(),
    }
}

pub(crate) fn build_goal_cleared() -> GrowSessionUpdate {
    GrowSessionUpdate::GoalUpdated {
        goal_id: String::new(),
        objective: String::new(),
        status: "cleared".to_string(),
        token_budget: None,
        tokens_used: 0,
        usage_breakdown: Default::default(),
        usage_incomplete: false,
        elapsed_ms: 0,
        created_at: String::new(),
        updated_at: String::new(),
        status_message: None,
    }
}

pub(crate) fn format_elapsed(ms: u64) -> String {
    let total_seconds = ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RejectProjectionExpectation {
        observed: tokio::sync::mpsc::UnboundedSender<chat_state::TimelineEvent>,
    }

    impl chat_state::TimelinePersistence for RejectProjectionExpectation {
        fn persist_timeline_event_and_ack(
            &mut self,
            event: &chat_state::TimelineEvent,
        ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            if matches!(
                &event.kind,
                chat_state::TimelineEventKind::DisplayProjectionExpected(_)
            ) {
                let _ = self.observed.send(event.clone());
                let _ = ack_tx.send(Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected projection expectation ACK failure",
                )));
            } else {
                let _ = ack_tx.send(Ok(()));
            }
            ack_rx
        }

        fn flush(&mut self) {}
    }

    #[tokio::test(flavor = "current_thread")]
    async fn goal_update_is_not_published_when_projection_expectation_ack_fails() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (observed_tx, mut observed_rx) = tokio::sync::mpsc::unbounded_channel();
                let cancellation = tokio_util::sync::CancellationToken::new();
                let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
                let chat_state = chat_state::ChatStateActor::spawn(
                    vec![sampling_types::ConversationItem::system(
                        "test system prompt",
                    )],
                    sampling_types::SamplingConfig {
                        base_url: "http://localhost".to_string(),
                        model: "test".to_string(),
                        output_limit: None,
                        temperature: None,
                        top_p: None,
                        api_backend: Default::default(),
                        extra_headers: Default::default(),
                        query_params: Default::default(),
                        env_http_headers: Default::default(),
                        context_window: std::num::NonZeroU64::new(256_000)
                            .expect("nonzero test context window"),
                        reasoning_effort: None,
                        stream_tool_calls: None,
                    },
                    Box::new(RejectProjectionExpectation {
                        observed: observed_tx,
                    }),
                    chat_event_tx,
                    cancellation.clone(),
                );
                // A query is admitted only after the actor has durably completed
                // its bootstrap facts, so the injected failure targets the live
                // expectation generated by GoalNotifySender.
                chat_state
                    .get_sampling_config()
                    .await
                    .expect("bootstrap facts are acknowledged");

                let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let gateway = acp_transport::AcpAgentGatewaySender::new(gateway_tx);
                let (session_persistence_tx, mut session_persistence_rx) =
                    tokio::sync::mpsc::unbounded_channel();
                let budget = sampler::PreviewEventBudget::default();
                let sender = GoalNotifySender::new(
                    agent_client_protocol::schema::v1::SessionId::new("goal-projection-ack"),
                    gateway,
                    session_persistence_tx,
                    budget.clone(),
                    std::sync::Arc::new(parking_lot::Mutex::new(None)),
                    chat_state.clone(),
                    7,
                );
                let goal = GoalState {
                    architecture_version: crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION,
                    goal_id: "goal-1".into(),
                    definition_revision: 3,
                    objective: "ship the feature".into(),
                    status: GoalStatus::Active,
                    token_budget: None,
                    tokens_used: 0,
                    usage_breakdown: None,
                    usage_incomplete: false,
                    usage_incomplete_acknowledged: false,
                    elapsed_ms: 0,
                    created_at: "2026-10-19T00:00:00Z".into(),
                    updated_at: "2026-10-19T00:00:00Z".into(),
                    status_message: None,
                    blocked_audit: None,
                };

                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    sender.emit_goal_updated(Some((goal, 125)), 0),
                )
                .await
                .expect("failed expectation ACK returns without publishing");

                let attempted =
                    tokio::time::timeout(std::time::Duration::from_secs(1), observed_rx.recv())
                        .await
                        .expect("producer attempted a typed expectation")
                        .expect("persistence seam remains available");
                assert!(matches!(
                    attempted.kind,
                    chat_state::TimelineEventKind::DisplayProjectionExpected(ref expected)
                        if expected.kind == chat_state::DisplayProjectionKind::GoalUpdated
                            && expected.source_id == "goal:goal-1:definition:3"
                            && expected.source_revision == 7
                ));
                assert!(
                    chat_state.is_closed(),
                    "permanent ACK failure closes writer"
                );
                assert!(gateway_rx.try_recv().is_err(), "no live update was sent");
                assert!(
                    session_persistence_rx.try_recv().is_err(),
                    "no cache update was queued"
                );
                let all_credits = budget
                    .try_acquire_bytes(sampler::PreviewEventBudget::MAX_BYTES)
                    .expect("both prepared notification reservations were released");
                drop(all_credits);

                cancellation.cancel();
            })
            .await;
    }
}
