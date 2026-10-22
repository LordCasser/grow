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
    source_revision: u64,
}

impl GoalNotifySender {
    pub(crate) fn new(
        session_id: agent_client_protocol::schema::v1::SessionId,
        gateway: acp_transport::AcpAgentGatewaySender,
        persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
        budget: sampler::PreviewEventBudget,
        preview: std::sync::Arc<parking_lot::Mutex<Option<(String, u32, bool)>>>,
        source_revision: u64,
    ) -> Self {
        Self {
            session_id,
            gateway,
            persistence_tx,
            budget,
            preview,
            source_revision,
        }
    }

    pub(crate) async fn emit_goal_updated(&self, goal: Option<(GoalState, u64)>, tokens_used: i64) {
        let Some((goal, elapsed_ms)) = goal else {
            return;
        };
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        let source_id = format!(
            "goal:{}:definition:{}",
            goal.goal_id, goal.definition_revision
        );
        let mut meta = meta.unwrap_or_default();
        meta.insert(
            "grow/displayProjectionSource".to_owned(),
            serde_json::Value::String(source_id),
        );
        meta.insert(
            "grow/displayProjectionRevision".to_owned(),
            serde_json::Value::from(self.source_revision),
        );
        let notification = GrowSessionNotification {
            session_id: self.session_id.clone(),
            update: build_goal_updated(&goal, tokens_used, elapsed_ms),
            meta: Some(serde_json::Value::Object(meta)),
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
        crate::session::notifications::publish_prepared_auxiliary_grow(
            &self.gateway,
            &self.persistence_tx,
            "grow/session_notification",
            prepared,
        );
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

    #[tokio::test(flavor = "current_thread")]
    async fn goal_snapshot_is_one_typed_fact_before_live_publication() {
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

        sender.emit_goal_updated(Some((goal, 125)), 0).await;

        let message = session_persistence_rx
            .try_recv()
            .expect("goal snapshot is queued as one durable fact");
        let notification = match &message {
            PersistenceMsg::AuxiliaryGrow { notification, .. } => notification,
            other => panic!("expected AuxiliaryGrow, got {other:?}"),
        };
        assert!(matches!(
            &notification.update,
            GrowSessionUpdate::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget: None,
                tokens_used: 0,
                usage_breakdown,
                usage_incomplete: false,
                elapsed_ms: 125,
                created_at,
                updated_at,
                status_message: None,
            } if goal_id == "goal-1"
                && objective == "ship the feature"
                && status == "active"
                && usage_breakdown.cached_input_tokens == 0
                && usage_breakdown.uncached_input_tokens == 0
                && usage_breakdown.unclassified_input_tokens == 0
                && usage_breakdown.output_tokens == 0
                && created_at == "2026-10-19T00:00:00Z"
                && updated_at == "2026-10-19T00:00:00Z"
        ));
        let meta = notification.meta.as_ref().unwrap().as_object().unwrap();
        assert!(
            meta.get("eventId")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        assert_eq!(
            meta.get("grow/displayProjectionSource")
                .and_then(serde_json::Value::as_str),
            Some("goal:goal-1:definition:3")
        );
        assert_eq!(
            meta.get("grow/displayProjectionRevision")
                .and_then(serde_json::Value::as_u64),
            Some(7)
        );
        let update = crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone()));
        let fact = crate::session::history::HistoryFact::from_update(&update)
            .expect("goal snapshot converts to a history fact")
            .expect("goal snapshot is a history fact");
        let round_trip = fact
            .into_update(&notification.session_id)
            .expect("history fact converts back to an update")
            .expect("goal snapshot remains an update");
        assert_eq!(
            serde_json::to_value(round_trip).unwrap(),
            serde_json::to_value(update).unwrap(),
            "history fact round-trips the full notification"
        );
        assert!(
            gateway_rx.try_recv().is_err(),
            "live update awaits writer ACK"
        );

        drop(message);

        assert!(
            gateway_rx.try_recv().is_err(),
            "failed persistence sends no live update"
        );
        let all_credits = budget
            .try_acquire_bytes(sampler::PreviewEventBudget::MAX_BYTES)
            .expect("dropping the refused fact releases the full preview budget");
        drop(all_credits);
    }
}
