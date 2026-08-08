//! Goal mode support — notification helpers and state formatters.
//!

use crate::extensions::notification::{
    SessionNotification as GrowSessionNotification, SessionUpdate as GrowSessionUpdate,
};
use crate::session::goal_tracker::{GoalOrchestration, GoalPhase, GoalStatus, GoalTracker};
use crate::session::persistence::PersistenceMsg;

// ---------------------------------------------------------------------------
// GoalNotifySender — fire-and-forget notification sender
// ---------------------------------------------------------------------------

/// Lightweight notification sender for goal progress updates.
pub(crate) struct GoalNotifySender {
    session_id: agent_client_protocol::SessionId,
    gateway: acp_transport::AcpAgentGatewaySender,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
}

impl GoalNotifySender {
    pub(crate) fn new(
        session_id: agent_client_protocol::SessionId,
        gateway: acp_transport::AcpAgentGatewaySender,
        persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    ) -> Self {
        Self {
            session_id,
            gateway,
            persistence_tx,
        }
    }

    /// Send a `GoalUpdated` built from the snapshot; token args come
    /// from [`SessionActor::goal_tokens`].
    pub(crate) fn emit_goal_updated(
        &self,
        tracker: &mut GoalTracker,
        tokens_used: i64,
        finished_subagent_tokens: i64,
    ) {
        tracker.account_elapsed();
        let Some(o) = tracker.snapshot() else { return };
        let _ = self
            .persistence_tx
            .send(PersistenceMsg::GoalModeState(o.clone()));
        self.send_update(build_goal_updated(o, tokens_used, finished_subagent_tokens));
    }

    pub(crate) fn persist_goal_state(&self, tracker: &GoalTracker) {
        let Some(snapshot) = tracker.snapshot() else {
            return;
        };
        let _ = self
            .persistence_tx
            .send(PersistenceMsg::GoalModeState(snapshot.clone()));
    }

    /// Like [`Self::emit_goal_updated`] but fire-and-forget to the gateway
    /// ONLY — the update is not appended to the session JSONL. Used by the
    /// high-frequency `SubagentProgress` live-token path: those ticks recur
    /// (~every `PROGRESS_PUBLISH_INTERVAL`) while a subagent runs, so
    /// persisting each one would grow the updates log without bound. Finished
    /// subagent usage is settled by its terminal event; graceful shutdown also
    /// checkpoints the last live marginal before the persistence barrier. The
    /// pager self-heals the live figure on the next tick. Mirrors the
    /// gateway-only `scheduled_task_fired` convention.
    pub(crate) fn emit_goal_updated_ephemeral(
        &self,
        tracker: &mut GoalTracker,
        tokens_used: i64,
        finished_subagent_tokens: i64,
    ) {
        tracker.account_elapsed();
        let Some(o) = tracker.snapshot() else { return };
        self.dispatch_update(
            build_goal_updated(o, tokens_used, finished_subagent_tokens),
            false,
        );
    }

    /// Persist + fire-and-forget a notification to the gateway. Used for
    /// snapshot-derived payloads and for the "planning…" / "Verifying…"
    /// latch updates that must not run the `send_grow_notification`
    /// rewind-window-close side effect.
    pub(crate) fn send_update(&self, update: GrowSessionUpdate) {
        self.dispatch_update(update, true);
    }

    /// Stamp, optionally persist, and fire-and-forget a notification.
    /// `persist == false` ships the update to the gateway only (no JSONL
    /// append) for recurring/transient ticks — see
    /// [`Self::emit_goal_updated_ephemeral`].
    fn dispatch_update(&self, update: GrowSessionUpdate, persist: bool) {
        // Stamped before the persist/broadcast fork — see `ensure_event_id_meta`.
        let mut meta = None;
        crate::util::event_id::ensure_event_id_meta(&self.session_id.0, &mut meta);
        let notification = GrowSessionNotification {
            session_id: self.session_id.clone(),
            update,
            meta: meta.map(serde_json::Value::Object),
        };
        let raw = serde_json::to_value(&notification)
            .and_then(|v| serde_json::value::to_raw_value(&v))
            .ok();
        if persist {
            let _ = self.persistence_tx.send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification)),
            ));
        }
        if let Some(raw) = raw {
            let ext = agent_client_protocol::ExtNotification::new(
                "grow/session_notification",
                raw.into(),
            );
            self.gateway.forward_fire_and_forget(ext);
        }
    }
}

/// Map a `GoalEvent` to its snake_case wire name.
fn goal_event_as_str(event: &crate::session::goal_tracker::GoalEvent) -> &'static str {
    use crate::session::goal_tracker::GoalEvent;
    match event {
        GoalEvent::GoalCreated => "goal_created",
        GoalEvent::GoalRevised => "goal_revised",
        GoalEvent::PlanningStarted => "planning_started",
        GoalEvent::PlanningCompleted => "planning_completed",
        GoalEvent::PlanningFailed => "planning_failed",
        GoalEvent::WorkerStarted => "worker_started",
        GoalEvent::WorkerCompleted => "worker_completed",
        GoalEvent::WorkerFailed => "worker_failed",
        GoalEvent::GoalPaused => "goal_paused",
        GoalEvent::GoalResumed => "goal_resumed",
        GoalEvent::VerificationRejected => "verification_rejected",
        GoalEvent::VerificationAccepted => "verification_accepted",
        GoalEvent::GoalCompleted => "goal_completed",
        GoalEvent::GoalCleared => "goal_cleared",
        GoalEvent::BudgetExceeded => "budget_exceeded",
    }
}

/// Build a `GoalUpdated` from an orchestration snapshot. `tokens_used`
/// already includes every subagent's marginal (live progress ticks
/// advance the records), so `o.live_subagent_tokens` is NOT folded in —
/// it ships only as the separate Active-subagent wire field.
pub(crate) fn build_goal_updated(
    o: &GoalOrchestration,
    tokens_used: i64,
    finished_subagent_tokens: i64,
) -> GrowSessionUpdate {
    let last_entry = o.history.last();

    let status_str = match o.status {
        GoalStatus::Active => "active",
        GoalStatus::Paused => "paused",
        GoalStatus::Blocked => "blocked",
        GoalStatus::BudgetLimited => "budget_limited",
        GoalStatus::Complete => "complete",
    };
    let phase_str = match o.phase {
        GoalPhase::Planning => "planning",
        GoalPhase::Executing => "executing",
        GoalPhase::Verifying => "verifying",
        GoalPhase::Summarizing => "summarizing",
    };

    let last_event = last_entry.map(|e| goal_event_as_str(&e.event).to_owned());

    GrowSessionUpdate::GoalUpdated {
        goal_id: o.goal_id.clone(),
        objective: o.objective.clone(),
        objective_revision: o.objective_revision,
        status: status_str.to_owned(),
        phase: phase_str.to_owned(),
        plan_revision: o.plan.revision,
        plan_markdown: o.plan.markdown.clone(),
        verifier_feedback: o.verifier_feedback.clone(),
        token_budget: o.token_budget,
        tokens_used,
        elapsed_ms: o.elapsed_ms,
        current_subagent_role: o.current_subagent_role.clone(),
        total_worker_rounds: o.total_worker_rounds,
        total_verify_rounds: o.total_verify_rounds,
        token_baseline: o.token_baseline,
        finished_subagent_tokens,
        live_subagent_tokens: (o.live_subagent_tokens > 0).then_some(o.live_subagent_tokens),
        // Only transmit the breakdown when ≥2 distinct models appear; a
        // single-model (or all-inherit) goal collapses to the single
        // tokens line, so a 1-element vec is never sent. This makes the
        // wire-field doc literally true and is the single source of the
        // collapse rule (the pager re-checks ≥2 as defence in depth).
        // `o.live_tokens_by_model` is a live active-subagent-window field
        // (cleared on `SubagentFinished`, like `live_subagent_tokens`); the
        // pager renders it only under the "Active subagent" block, so this
        // producer must keep its populate gate on that same axis.
        live_tokens_by_model: if o.live_tokens_by_model.len() >= 2 {
            o.live_tokens_by_model.clone()
        } else {
            Vec::new()
        },
        live_context_pct: (o.live_context_pct > 0).then_some(o.live_context_pct),
        live_turn_count: (o.live_turn_count > 0).then_some(o.live_turn_count),
        live_tool_call_count: (o.live_tool_call_count > 0).then_some(o.live_tool_call_count),
        last_event,
        last_event_detail: last_entry.and_then(|e| e.detail.clone()),
        last_event_timestamp: last_entry.map(|e| e.timestamp.clone()),
        pause_message: o.pause_message.clone(),
    }
}

/// Build a `GoalUpdated` with `status: "cleared"` to tell the pager to
/// drop its goal state.
pub(crate) fn build_goal_cleared() -> GrowSessionUpdate {
    GrowSessionUpdate::GoalUpdated {
        goal_id: String::new(),
        objective: String::new(),
        objective_revision: 0,
        status: "cleared".to_owned(),
        phase: "planning".to_owned(),
        plan_revision: 0,
        plan_markdown: String::new(),
        verifier_feedback: None,
        token_budget: None,
        tokens_used: 0,
        elapsed_ms: 0,
        current_subagent_role: None,
        total_worker_rounds: 0,
        total_verify_rounds: 0,
        token_baseline: 0,
        finished_subagent_tokens: 0,
        live_subagent_tokens: None,
        live_tokens_by_model: Vec::new(),
        live_context_pct: None,
        live_turn_count: None,
        live_tool_call_count: None,
        last_event: None,
        last_event_detail: None,
        last_event_timestamp: None,
        pause_message: None,
    }
}

/// Format elapsed milliseconds as a compact human-readable duration.
pub(crate) fn format_elapsed(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::goal_tracker::{GoalPlanAuthor, GoalTracker};

    fn verifying_goal() -> GoalOrchestration {
        let mut tracker = GoalTracker::new();
        tracker.create_goal(
            "g1".into(),
            "ship it".into(),
            Some(42_000),
            100,
            "now".into(),
            None,
        );
        assert!(tracker.replace_plan(
            "- [ ] implement\n- [ ] verify".into(),
            GoalPlanAuthor::Planner,
            None,
        ));
        assert!(tracker.candidate_complete("candidate".into()));
        tracker.snapshot().unwrap().clone()
    }

    #[test]
    fn goal_update_carries_v2_blackboard_and_phase() {
        let goal = verifying_goal();
        let update = build_goal_updated(&goal, 123, 23);
        let GrowSessionUpdate::GoalUpdated {
            goal_id,
            objective_revision,
            status,
            phase,
            plan_revision,
            plan_markdown,
            verifier_feedback,
            ..
        } = update
        else {
            panic!("expected GoalUpdated");
        };
        assert_eq!(goal_id, "g1");
        assert_eq!(objective_revision, 0);
        assert_eq!(status, "active");
        assert_eq!(phase, "verifying");
        assert_eq!(plan_revision, 1);
        assert_eq!(plan_markdown, "- [ ] implement\n- [ ] verify");
        assert_eq!(verifier_feedback, None);
    }

    #[test]
    fn cleared_update_is_an_explicit_removal_signal() {
        let GrowSessionUpdate::GoalUpdated { status, .. } = build_goal_cleared() else {
            panic!("expected GoalUpdated");
        };
        assert_eq!(status, "cleared");
    }
}
