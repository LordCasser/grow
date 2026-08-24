//! Persisted thread Goal state and pure transitions.
//!
//! A Goal is a long-lived objective, not a plan executor. The durable state
//! records only what the user asked for, whether automatic continuation is
//! armed, and the usage charged while it was active. Foreground ownership,
//! idle admission, cancellation and continuation turns remain SessionActor
//! runtime state and are never persisted here.

use std::time::Instant;

pub const GOAL_ARCHITECTURE_VERSION: u8 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

impl GoalStatus {
    pub fn continues_automatically(self) -> bool {
        self == Self::Active
    }

    pub fn can_restart(self) -> bool {
        matches!(self, Self::Paused | Self::Blocked | Self::UsageLimited)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPauseReason {
    User,
    TurnError,
    UsageLimit,
    RuntimeUnavailable,
}

impl GoalPauseReason {
    pub fn default_message(self) -> &'static str {
        match self {
            Self::User => "Paused by the user.",
            Self::TurnError => "Paused after a terminal turn error.",
            Self::UsageLimit => "Paused because the model usage limit was reached.",
            Self::RuntimeUnavailable => "Paused because Goal runtime tools are unavailable.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalState {
    pub architecture_version: u8,
    pub goal_id: String,
    /// Monotonic identity of the user-controlled Goal definition. Runtime
    /// accounting and lifecycle transitions do not change it, so request
    /// projection can shadow superseded continuation directives without
    /// coupling model context to unrelated Control checkpoints.
    pub definition_revision: u64,
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<i64>,
    pub token_baseline: i64,
    #[serde(default)]
    pub parent_tokens_spent: i64,
    #[serde(default)]
    pub subagent_tokens_spent: i64,
    #[serde(default)]
    pub last_session_tokens_seen: Option<i64>,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

#[derive(Debug)]
pub struct GoalTracker {
    goal: Option<GoalState>,
    active_since: Option<Instant>,
}

impl Default for GoalTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalTracker {
    pub fn new() -> Self {
        Self {
            goal: None,
            active_since: None,
        }
    }

    /// Goal v7 intentionally has no compatibility projection. Old
    /// planner/blackboard snapshots are rejected instead of reviving two
    /// lifecycle models in one session.
    pub fn from_snapshot(snapshot: GoalState) -> Option<Self> {
        if snapshot.architecture_version != GOAL_ARCHITECTURE_VERSION
            || snapshot.goal_id.trim().is_empty()
            || snapshot.definition_revision == 0
            || snapshot.objective.trim().is_empty()
            || snapshot.token_budget.is_some_and(|budget| budget <= 0)
            || snapshot.token_baseline < 0
            || snapshot.parent_tokens_spent < 0
            || snapshot.subagent_tokens_spent < 0
        {
            return None;
        }
        let active_since = snapshot.status.continues_automatically().then(Instant::now);
        Some(Self {
            goal: Some(snapshot),
            active_since,
        })
    }

    pub fn restore_runtime_snapshot(&mut self, snapshot: GoalState) {
        self.active_since = snapshot
            .status
            .continues_automatically()
            .then(Instant::now);
        self.goal = Some(snapshot);
    }

    pub fn snapshot(&self) -> Option<&GoalState> {
        self.goal.as_ref()
    }

    pub fn snapshot_mut(&mut self) -> Option<&mut GoalState> {
        self.goal.as_mut()
    }

    pub fn status(&self) -> Option<GoalStatus> {
        self.goal.as_ref().map(|goal| goal.status)
    }

    pub fn objective(&self) -> Option<&str> {
        self.goal.as_ref().map(|goal| goal.objective.as_str())
    }

    pub fn token_budget(&self) -> Option<i64> {
        self.goal.as_ref().and_then(|goal| goal.token_budget)
    }

    pub fn create_goal(
        &mut self,
        goal_id: String,
        objective: String,
        token_budget: Option<i64>,
        token_baseline: i64,
        created_at: String,
    ) -> Result<(), String> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err("Goal objective must not be empty.".to_string());
        }
        if token_budget.is_some_and(|budget| budget <= 0) {
            return Err("Goal token budget must be positive.".to_string());
        }
        if self
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status != GoalStatus::Complete)
        {
            return Err(
                "An unfinished Goal already exists; edit, pause, complete, or clear it first."
                    .to_string(),
            );
        }
        self.goal = Some(GoalState {
            architecture_version: GOAL_ARCHITECTURE_VERSION,
            goal_id,
            definition_revision: 1,
            objective: objective.to_string(),
            status: GoalStatus::Active,
            token_budget,
            token_baseline: token_baseline.max(0),
            parent_tokens_spent: 0,
            subagent_tokens_spent: 0,
            last_session_tokens_seen: Some(token_baseline.max(0)),
            elapsed_ms: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            status_message: None,
        });
        self.active_since = Some(Instant::now());
        Ok(())
    }

    /// Explicit user edit. Usage belongs to the same long-running Goal and is
    /// preserved; editing a stopped Goal re-arms it, matching Codex's Goal UI.
    pub fn revise_goal(&mut self, objective: String, token_budget: Option<i64>) -> bool {
        let objective = objective.trim();
        if objective.is_empty() || token_budget.is_some_and(|budget| budget <= 0) {
            return false;
        }
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        let changed = goal.objective != objective
            || goal.token_budget != token_budget
            || goal.status != GoalStatus::Active;
        if !changed {
            return false;
        }
        let definition_changed = goal.objective != objective || goal.token_budget != token_budget;
        let next_definition_revision = if definition_changed {
            let Some(next) = goal.definition_revision.checked_add(1) else {
                return false;
            };
            Some(next)
        } else {
            None
        };
        goal.objective = objective.to_string();
        goal.token_budget = token_budget;
        if let Some(next) = next_definition_revision {
            goal.definition_revision = next;
        }
        goal.status = GoalStatus::Active;
        goal.status_message = None;
        goal.updated_at = now();
        self.active_since = Some(Instant::now());
        true
    }

    pub fn pause(&mut self, reason: GoalPauseReason) -> bool {
        self.pause_with_message(reason, reason.default_message().to_string())
    }

    pub fn pause_with_message(&mut self, reason: GoalPauseReason, message: String) -> bool {
        let status = match reason {
            GoalPauseReason::User | GoalPauseReason::RuntimeUnavailable => GoalStatus::Paused,
            GoalPauseReason::TurnError => GoalStatus::Blocked,
            GoalPauseReason::UsageLimit => GoalStatus::UsageLimited,
        };
        self.set_stopped_status(status, message)
    }

    pub fn report_blocked(&mut self, message: String) -> bool {
        self.set_stopped_status(GoalStatus::Blocked, message)
    }

    fn set_stopped_status(&mut self, status: GoalStatus, message: String) -> bool {
        self.account_elapsed();
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active {
            return false;
        }
        goal.status = status;
        goal.status_message = (!message.trim().is_empty()).then(|| message.trim().to_string());
        goal.updated_at = now();
        self.active_since = None;
        true
    }

    pub fn restart(&mut self) -> bool {
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if !goal.status.can_restart() {
            return false;
        }
        goal.status = GoalStatus::Active;
        goal.status_message = None;
        goal.updated_at = now();
        self.active_since = Some(Instant::now());
        true
    }

    pub fn budget_limit(&mut self) -> bool {
        self.account_elapsed();
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active {
            return false;
        }
        goal.status = GoalStatus::BudgetLimited;
        goal.status_message = Some("Goal token budget reached.".to_string());
        goal.updated_at = now();
        self.active_since = None;
        true
    }

    pub fn complete(&mut self) -> bool {
        self.account_elapsed();
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete {
            return false;
        }
        goal.status = GoalStatus::Complete;
        goal.status_message = None;
        goal.updated_at = now();
        self.active_since = None;
        true
    }

    pub fn set_token_budget(&mut self, budget: Option<i64>) -> bool {
        if budget.is_some_and(|budget| budget <= 0) {
            return false;
        }
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete || goal.token_budget == budget {
            return false;
        }
        let Some(next_definition_revision) = goal.definition_revision.checked_add(1) else {
            return false;
        };
        goal.token_budget = budget;
        goal.definition_revision = next_definition_revision;
        if goal.status == GoalStatus::BudgetLimited {
            goal.status = GoalStatus::Paused;
            goal.status_message = Some(
                "Budget updated. Restart the Goal when you are ready to continue.".to_string(),
            );
        }
        goal.updated_at = now();
        true
    }

    pub fn clear(&mut self) {
        self.goal = None;
        self.active_since = None;
    }

    pub fn account_elapsed(&mut self) {
        let Some(started) = self.active_since.replace(Instant::now()) else {
            return;
        };
        let Some(goal) = self.goal.as_mut() else {
            self.active_since = None;
            return;
        };
        goal.elapsed_ms = goal
            .elapsed_ms
            .saturating_add(started.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        if !goal.status.continues_automatically() {
            self.active_since = None;
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        let persisted = self.goal.as_ref().map_or(0, |goal| goal.elapsed_ms);
        persisted.saturating_add(
            self.active_since
                .map(|started| started.elapsed().as_millis().try_into().unwrap_or(u64::MAX))
                .unwrap_or(0),
        )
    }

    pub fn account_parent_tokens(&mut self, current_session_tokens: i64) -> i64 {
        let Some(goal) = self.goal.as_mut() else {
            return 0;
        };
        let current = current_session_tokens.max(0);
        let previous = goal
            .last_session_tokens_seen
            .unwrap_or(goal.token_baseline)
            .max(0);
        if goal.status == GoalStatus::Active {
            goal.parent_tokens_spent = goal
                .parent_tokens_spent
                .saturating_add(current.saturating_sub(previous));
        }
        goal.last_session_tokens_seen = Some(current);
        goal.parent_tokens_spent
    }

    pub fn settle_subagent_tokens(&mut self, tokens: i64) -> bool {
        if tokens <= 0 {
            return false;
        }
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        goal.subagent_tokens_spent = goal.subagent_tokens_spent.saturating_add(tokens);
        true
    }

    pub fn subagent_tokens_spent(&self) -> i64 {
        self.goal
            .as_ref()
            .map_or(0, |goal| goal.subagent_tokens_spent)
    }


    pub fn tokens_used(&self) -> i64 {
        self.goal.as_ref().map_or(0, |goal| {
            goal.parent_tokens_spent
                .saturating_add(goal.subagent_tokens_spent)
        })
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> GoalTracker {
        let mut tracker = GoalTracker::new();
        tracker
            .create_goal("g1".into(), "ship it".into(), Some(100), 10, "now".into())
            .unwrap();
        tracker
    }

    #[test]
    fn goal_is_long_lived_without_plan_phase() {
        let tracker = tracker();
        assert_eq!(tracker.status(), Some(GoalStatus::Active));
        assert_eq!(tracker.objective(), Some("ship it"));
        assert_eq!(tracker.snapshot().unwrap().token_budget, Some(100));
    }

    #[test]
    fn pause_restart_edit_and_complete_are_explicit() {
        let mut tracker = tracker();
        let initial_revision = tracker.snapshot().unwrap().definition_revision;
        assert!(tracker.pause(GoalPauseReason::User));
        assert_eq!(tracker.status(), Some(GoalStatus::Paused));
        assert!(tracker.restart());
        assert_eq!(
            tracker.snapshot().unwrap().definition_revision,
            initial_revision,
            "lifecycle transitions must not invalidate an unchanged definition"
        );
        assert!(tracker.revise_goal("ship safely".into(), Some(200)));
        assert_eq!(tracker.objective(), Some("ship safely"));
        assert_eq!(
            tracker.snapshot().unwrap().definition_revision,
            initial_revision + 1
        );
        assert!(tracker.complete());
        assert_eq!(
            tracker.snapshot().unwrap().definition_revision,
            initial_revision + 1
        );
        assert_eq!(tracker.status(), Some(GoalStatus::Complete));
    }

    #[test]
    fn budget_changes_advance_the_goal_definition_once() {
        let mut tracker = tracker();
        assert_eq!(tracker.snapshot().unwrap().definition_revision, 1);
        assert!(tracker.set_token_budget(Some(200)));
        assert_eq!(tracker.snapshot().unwrap().definition_revision, 2);
        assert!(!tracker.set_token_budget(Some(200)));
        assert_eq!(tracker.snapshot().unwrap().definition_revision, 2);
    }

    #[test]
    fn definition_revision_exhaustion_rejects_definition_mutation() {
        let mut tracker = tracker();
        tracker.snapshot_mut().unwrap().definition_revision = u64::MAX;
        assert!(!tracker.revise_goal("different".into(), Some(100)));
        assert!(!tracker.set_token_budget(Some(200)));
        assert_eq!(tracker.objective(), Some("ship it"));
        assert_eq!(tracker.token_budget(), Some(100));
    }

    #[test]
    fn old_goal_architecture_is_rejected() {
        let mut state = tracker().snapshot().unwrap().clone();
        state.architecture_version = GOAL_ARCHITECTURE_VERSION - 1;
        assert!(GoalTracker::from_snapshot(state).is_none());
    }
}
