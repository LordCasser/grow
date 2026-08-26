//! Persisted thread Goal state and pure transitions.
//!
//! A Goal is a long-lived objective, not a plan executor. The durable state
//! records only what the user asked for, whether automatic continuation is
//! armed, and the usage charged while it was active. Foreground ownership,
//! idle admission, cancellation and continuation turns remain SessionActor
//! runtime state and are never persisted here.

use std::time::Instant;

pub const GOAL_ARCHITECTURE_VERSION: u8 = 9;

const REQUIRED_CONSECUTIVE_BLOCKED_TURNS: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalBlockedAudit {
    /// Normalized model-reported impasse. Exact identity is intentionally
    /// conservative: changing the claimed blocker starts a fresh audit.
    pub blocker: String,
    pub consecutive_turns: u8,
    pub last_prompt_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    BudgetLimited,
    Complete,
}

impl GoalStatus {
    pub fn continues_automatically(self) -> bool {
        self == Self::Active
    }

    pub fn can_restart(self) -> bool {
        matches!(self, Self::Paused | Self::Blocked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPauseReason {
    User,
    TurnError,
    RuntimeUnavailable,
}

impl GoalPauseReason {
    pub fn default_message(self) -> &'static str {
        match self {
            Self::User => "Paused by the user.",
            Self::TurnError => "Paused after a terminal turn error.",
            Self::RuntimeUnavailable => "Paused because Goal runtime tools are unavailable.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalState {
    pub architecture_version: u8,
    /// Stable identity of this long-lived Goal. It changes only when the Goal
    /// is explicitly cleared and a new one is created. Definition revisions
    /// invalidate stale continuations without orphaning usage from work that
    /// was admitted before an edit, pause, or restart.
    pub goal_id: String,
    /// Monotonic identity of the user-controlled Goal definition. Runtime
    /// accounting and lifecycle transitions do not change it, so request
    /// projection can shadow superseded continuation directives without
    /// coupling model context to unrelated Control checkpoints.
    pub definition_revision: u64,
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<i64>,
    /// Durable cumulative Goal charge. This is model consumption, not the
    /// current provider context length: uncached input plus output for each
    /// main-Agent call, plus acknowledged usage folds from Goal-owned
    /// subagents.
    pub tokens_used: i64,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_audit: Option<GoalBlockedAudit>,
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

    /// The current Goal architecture intentionally has no compatibility projection. Old
    /// planner/blackboard snapshots are rejected instead of reviving two
    /// lifecycle models in one session.
    pub fn from_snapshot(snapshot: GoalState) -> Option<Self> {
        Self::validate_snapshot(&snapshot).ok()?;
        let active_since = snapshot.status.continues_automatically().then(Instant::now);
        Some(Self {
            goal: Some(snapshot),
            active_since,
        })
    }

    /// Validate the durable Goal payload without constructing runtime leases.
    /// Control loading uses the same predicate as runtime restoration so a
    /// malformed Goal can never be silently dropped while the rest of its
    /// Control event is accepted.
    pub fn validate_snapshot(snapshot: &GoalState) -> Result<(), &'static str> {
        if snapshot.architecture_version != GOAL_ARCHITECTURE_VERSION
            || snapshot.goal_id.trim().is_empty()
            || snapshot.definition_revision == 0
            || snapshot.objective.trim().is_empty()
            || snapshot.token_budget.is_some_and(|budget| budget <= 0)
            || snapshot.tokens_used < 0
            || snapshot.blocked_audit.as_ref().is_some_and(|audit| {
                audit.blocker.trim().is_empty()
                    || audit.consecutive_turns == 0
                    || audit.consecutive_turns >= REQUIRED_CONSECUTIVE_BLOCKED_TURNS
            })
            || (snapshot.status != GoalStatus::Active && snapshot.blocked_audit.is_some())
        {
            return Err("invalid Goal control payload");
        }
        Ok(())
    }

    pub fn restore_runtime_snapshot(&mut self, snapshot: GoalState) {
        self.active_since = snapshot.status.continues_automatically().then(Instant::now);
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
        created_at: String,
    ) -> Result<(), String> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err("Goal objective must not be empty.".to_string());
        }
        if token_budget.is_some_and(|budget| budget <= 0) {
            return Err("Goal token budget must be positive.".to_string());
        }
        if self.goal.is_some() {
            return Err("A Goal already exists; edit or clear it before creating another.".into());
        }
        self.goal = Some(GoalState {
            architecture_version: GOAL_ARCHITECTURE_VERSION,
            goal_id,
            definition_revision: 1,
            objective: objective.to_string(),
            status: GoalStatus::Active,
            token_budget,
            tokens_used: 0,
            elapsed_ms: 0,
            created_at: created_at.clone(),
            updated_at: created_at,
            status_message: None,
            blocked_audit: None,
        });
        self.active_since = Some(Instant::now());
        Ok(())
    }

    /// Explicit user edit. Usage belongs to the same long-running Goal and is
    /// preserved. Paused/blocked state remains stopped so edit
    /// and restart are independent user controls; budget-limited or complete
    /// state is reactivated because its prior terminal definition was replaced.
    pub fn revise_goal(&mut self, objective: String, token_budget: Option<i64>) -> bool {
        let objective = objective.trim();
        if objective.is_empty() || token_budget.is_some_and(|budget| budget <= 0) {
            return false;
        }
        let Some(goal) = self.goal.as_ref() else {
            return false;
        };
        let next_status = match goal.status {
            GoalStatus::Paused | GoalStatus::Blocked => goal.status,
            GoalStatus::Active | GoalStatus::BudgetLimited | GoalStatus::Complete => {
                GoalStatus::Active
            }
        };
        let changed = goal.objective != objective
            || goal.token_budget != token_budget
            || goal.status != next_status;
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
        self.account_elapsed();
        let goal = self.goal.as_mut().expect("Goal existed before accounting");
        goal.objective = objective.to_string();
        goal.token_budget = token_budget;
        if let Some(next) = next_definition_revision {
            goal.definition_revision = next;
            goal.blocked_audit = None;
        }
        goal.status = next_status;
        if next_status == GoalStatus::Active {
            goal.status_message = None;
        }
        goal.updated_at = now();
        self.active_since = next_status.continues_automatically().then(Instant::now);
        true
    }

    pub fn pause(&mut self, reason: GoalPauseReason) -> bool {
        self.pause_with_message(reason, reason.default_message().to_string())
    }

    pub fn pause_with_message(&mut self, reason: GoalPauseReason, message: String) -> bool {
        let status = match reason {
            GoalPauseReason::User
            | GoalPauseReason::TurnError
            | GoalPauseReason::RuntimeUnavailable => GoalStatus::Paused,
        };
        self.set_stopped_status(status, message)
    }

    /// Record one model-reported impasse at a durable prompt coordinate.
    /// Returns the current consecutive count; only the third consecutive turn
    /// transitions the Goal to Blocked. Repeated calls in one turn are not a
    /// substitute for three independent continuation audits.
    pub fn report_blocked(&mut self, blocker: String, prompt_index: u64) -> Result<u8, String> {
        let blocker = blocker.split_whitespace().collect::<Vec<_>>().join(" ");
        if blocker.is_empty() {
            return Err("A concrete blocker is required when reporting a blocked Goal.".into());
        }
        let Some(goal) = self.goal.as_mut() else {
            return Err("No Goal is currently set.".into());
        };
        if goal.status != GoalStatus::Active {
            return Err("Only an active Goal can report a blocker.".into());
        }
        if goal
            .blocked_audit
            .as_ref()
            .is_some_and(|audit| audit.last_prompt_index == prompt_index)
        {
            return Err("This Goal turn has already reported its blocker.".into());
        }
        let consecutive_turns = goal
            .blocked_audit
            .as_ref()
            .filter(|audit| {
                audit.blocker == blocker
                    && audit.last_prompt_index.checked_add(1) == Some(prompt_index)
            })
            .map_or(1, |audit| audit.consecutive_turns.saturating_add(1));
        if consecutive_turns < REQUIRED_CONSECUTIVE_BLOCKED_TURNS {
            goal.blocked_audit = Some(GoalBlockedAudit {
                blocker,
                consecutive_turns,
                last_prompt_index: prompt_index,
            });
            goal.updated_at = now();
            return Ok(consecutive_turns);
        }
        self.account_elapsed();
        let goal = self.goal.as_mut().expect("Goal existed before accounting");
        goal.status = GoalStatus::Blocked;
        goal.status_message = Some(blocker);
        goal.blocked_audit = None;
        goal.updated_at = now();
        self.active_since = None;
        Ok(consecutive_turns)
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
        goal.blocked_audit = None;
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
        goal.blocked_audit = None;
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
        goal.blocked_audit = None;
        goal.updated_at = now();
        self.active_since = None;
        true
    }

    pub fn complete(&mut self) -> bool {
        self.account_elapsed();
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active {
            return false;
        }
        goal.status = GoalStatus::Complete;
        goal.status_message = None;
        goal.blocked_audit = None;
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
        goal.blocked_audit = None;
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

    /// Charge one main-Agent model call to the immutable owner captured when
    /// its turn was admitted. A lifecycle tool may stop the Goal before the
    /// provider response is settled; owner identity, not live status, decides
    /// attribution. Definition revisions invalidate stale continuation text,
    /// while the stable Goal identity keeps already-admitted usage chargeable.
    pub fn account_model_tokens(&mut self, goal_id: &str, tokens: i64) -> bool {
        if tokens <= 0 {
            return false;
        }
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.goal_id != goal_id {
            return false;
        }
        goal.tokens_used = goal.tokens_used.saturating_add(tokens);
        true
    }

    pub fn settle_subagent_tokens(&mut self, goal_id: &str, tokens: i64) -> bool {
        if tokens <= 0 {
            return false;
        }
        let Some(goal) = self.goal.as_mut() else {
            return false;
        };
        if goal.goal_id != goal_id {
            return false;
        }
        goal.tokens_used = goal.tokens_used.saturating_add(tokens);
        true
    }

    pub fn tokens_used(&self) -> i64 {
        self.goal.as_ref().map_or(0, |goal| goal.tokens_used)
    }
}

/// Codex Goal budget unit: uncached input plus output from one model call.
/// Reasoning is already included in provider output and must not be added a
/// second time.
pub fn model_usage_goal_tokens(usage: &sampling_types::TokenUsage) -> i64 {
    let uncached_input = usage
        .prompt_tokens
        .saturating_sub(usage.cached_prompt_tokens);
    i64::from(uncached_input).saturating_add(i64::from(usage.completion_tokens))
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
            .create_goal("g1".into(), "ship it".into(), Some(100), "now".into())
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
    fn editing_a_resumable_stopped_goal_does_not_restart_it() {
        for reason in [GoalPauseReason::User, GoalPauseReason::TurnError] {
            let mut tracker = tracker();
            assert!(tracker.pause(reason));
            let stopped = tracker.status().unwrap();
            assert!(tracker.revise_goal("ship safely".into(), Some(200)));
            assert_eq!(tracker.status(), Some(stopped));
            assert!(tracker.restart());
            assert_eq!(tracker.status(), Some(GoalStatus::Active));
        }
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

    #[test]
    fn model_usage_is_monotonic_and_excludes_cache_reads() {
        let mut tracker = tracker();
        let usage = sampling_types::TokenUsage {
            prompt_tokens: 1_000,
            completion_tokens: 80,
            total_tokens: 1_080,
            reasoning_tokens: 40,
            cached_prompt_tokens: 700,
            cache_creation_prompt_tokens: 0,
        };
        let charge = model_usage_goal_tokens(&usage);
        assert_eq!(charge, 380);
        assert!(tracker.account_model_tokens("g1", charge));
        assert!(tracker.account_model_tokens("g1", 20));
        assert_eq!(tracker.tokens_used(), 400);
    }

    #[test]
    fn admitted_owner_is_charged_even_if_goal_stops_before_settlement() {
        let mut tracker = tracker();
        assert!(tracker.pause(GoalPauseReason::User));
        assert!(tracker.account_model_tokens("g1", 50));
        assert_eq!(tracker.tokens_used(), 50);
    }

    #[test]
    fn restart_keeps_the_definition_owner_for_late_settlement() {
        let mut tracker = tracker();
        assert!(tracker.pause(GoalPauseReason::User));
        assert!(tracker.restart());
        assert!(tracker.account_model_tokens("g1", 50));
        assert_eq!(tracker.tokens_used(), 50);
    }

    #[test]
    fn definition_edit_keeps_goal_identity_and_accepts_admitted_usage() {
        let mut tracker = tracker();
        assert!(tracker.revise_goal("ship safely".into(), Some(200)));
        assert_eq!(tracker.snapshot().unwrap().goal_id, "g1");
        assert_eq!(tracker.snapshot().unwrap().definition_revision, 2);
        assert!(tracker.account_model_tokens("g1", 50));
        assert_eq!(tracker.tokens_used(), 50);
    }

    #[test]
    fn turn_error_pauses_instead_of_claiming_a_genuine_impasse() {
        let mut tracker = tracker();
        assert_eq!(tracker.report_blocked("waiting".into(), 1), Ok(1));
        assert!(tracker.pause(GoalPauseReason::TurnError));
        assert_eq!(tracker.status(), Some(GoalStatus::Paused));
        assert!(tracker.snapshot().unwrap().blocked_audit.is_none());
        assert!(
            !tracker.complete(),
            "a stopped Goal is controlled by the user and cannot be completed by a later model turn"
        );
    }

    #[test]
    fn blocked_requires_same_impasse_on_three_consecutive_turns() {
        let mut tracker = tracker();
        assert_eq!(tracker.report_blocked("waiting on user".into(), 10), Ok(1));
        assert_eq!(tracker.status(), Some(GoalStatus::Active));
        assert_eq!(tracker.report_blocked("waiting on user".into(), 11), Ok(2));
        assert_eq!(tracker.status(), Some(GoalStatus::Active));
        assert_eq!(
            tracker.report_blocked("different blocker".into(), 12),
            Ok(1)
        );
        assert_eq!(
            tracker.report_blocked("different blocker".into(), 13),
            Ok(2)
        );
        assert_eq!(
            tracker.report_blocked("different blocker".into(), 14),
            Ok(3)
        );
        assert_eq!(tracker.status(), Some(GoalStatus::Blocked));
        assert_eq!(
            tracker.snapshot().unwrap().status_message.as_deref(),
            Some("different blocker")
        );
    }

    #[test]
    fn one_turn_cannot_increment_the_blocked_audit_twice() {
        let mut tracker = tracker();
        assert_eq!(tracker.report_blocked("external outage".into(), 7), Ok(1));
        assert!(tracker.report_blocked("external outage".into(), 7).is_err());
        assert_eq!(
            tracker
                .snapshot()
                .unwrap()
                .blocked_audit
                .as_ref()
                .unwrap()
                .consecutive_turns,
            1
        );
    }
}
