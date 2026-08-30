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
}

impl GoalNotifySender {
    pub(crate) fn new(
        session_id: agent_client_protocol::schema::v1::SessionId,
        gateway: acp_transport::AcpAgentGatewaySender,
        persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    ) -> Self {
        Self {
            session_id,
            gateway,
            persistence_tx,
        }
    }

    pub(crate) fn emit_goal_updated(&self, tracker: &GoalTracker, tokens_used: i64) {
        let Some(goal) = tracker.snapshot() else {
            return;
        };
        self.send_update(build_goal_updated(goal, tokens_used, tracker.elapsed_ms()));
    }

    pub(crate) fn send_update(&self, update: GrowSessionUpdate) {
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        let notification = GrowSessionNotification {
            session_id: self.session_id.clone(),
            update,
            meta: meta.map(serde_json::Value::Object),
        };
        let raw = serde_json::to_value(&notification)
            .and_then(|value| serde_json::value::to_raw_value(&value))
            .ok();
        let _ = self.persistence_tx.send(PersistenceMsg::Update(
            crate::session::storage::SessionUpdate::Grow(Box::new(notification)),
        ));
        if let Some(raw) = raw {
            self.gateway.forward_fire_and_forget(
                agent_client_protocol::schema::v1::ExtNotification::new(
                    "grow/session_notification",
                    raw.into(),
                ),
            );
        }
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
