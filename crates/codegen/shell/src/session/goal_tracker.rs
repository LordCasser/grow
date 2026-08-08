//! Persisted Goal state and its pure transition rules.
//!
//! The tracker owns no tasks and performs no I/O. `SessionActor` is the sole
//! scheduler; background stages commit only through a matching [`StageLease`].

use std::time::Instant;

pub const GOAL_ARCHITECTURE_VERSION: u8 = 3;
pub const IDENTICAL_GAP_BLOCK_THRESHOLD: u32 = 3;
pub const INFRA_FAILURE_PAUSE_THRESHOLD: u8 = 3;

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
    pub fn is_paused(self) -> bool {
        matches!(self, Self::Paused | Self::Blocked | Self::BudgetLimited)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPhase {
    Planning,
    Executing,
    Verifying,
    Summarizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPauseReason {
    User,
    BackOff,
    NoProgress,
    Verification,
    Infra,
}

impl GoalPauseReason {
    pub fn history_detail(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::BackOff => "back_off",
            Self::NoProgress => "no_progress",
            Self::Verification => "verification",
            Self::Infra => "infra",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPlanAuthor {
    Planner,
    Agent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalPlan {
    pub revision: u64,
    pub markdown: String,
    pub updated_at: String,
    pub updated_by: GoalPlanAuthor,
}

impl GoalPlan {
    fn empty(created_at: String) -> Self {
        Self {
            revision: 0,
            markdown: String::new(),
            updated_at: created_at,
            updated_by: GoalPlanAuthor::Planner,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StageLease {
    pub goal_id: String,
    pub objective_revision: u64,
    pub plan_revision: u64,
    pub stage_id: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEvent {
    GoalCreated,
    GoalRevised,
    PlanningStarted,
    PlanningCompleted,
    PlanningFailed,
    WorkerStarted,
    WorkerCompleted,
    WorkerFailed,
    GoalPaused,
    GoalResumed,
    VerificationRejected,
    VerificationAccepted,
    GoalCompleted,
    GoalCleared,
    BudgetExceeded,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalHistoryEntry {
    pub event: GoalEvent,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl GoalHistoryEntry {
    pub fn new(event: GoalEvent, detail: Option<String>) -> Self {
        Self {
            event,
            timestamp: chrono::Utc::now().to_rfc3339(),
            detail,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoalOrchestration {
    pub architecture_version: u8,
    pub goal_id: String,
    pub objective: String,
    pub objective_revision: u64,
    pub status: GoalStatus,
    pub phase: GoalPhase,
    pub plan: GoalPlan,
    pub token_budget: Option<i64>,
    pub token_baseline: i64,
    #[serde(default)]
    pub parent_tokens_spent: i64,
    /// Tokens durably charged by Goal-owned subagents that have finished.
    /// Live subagent deltas remain transient until their terminal event (or a
    /// graceful session shutdown) settles them into this counter.
    #[serde(default)]
    pub subagent_tokens_spent: i64,
    #[serde(default)]
    pub last_session_tokens_seen: Option<i64>,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub created_at: String,
    #[serde(default)]
    pub history: Vec<GoalHistoryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_feedback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_gap_fingerprint: Option<String>,
    #[serde(default)]
    pub repeated_gap_count: u32,
    #[serde(default)]
    pub planner_failures: u8,
    #[serde(default)]
    pub total_worker_rounds: u32,
    #[serde(default)]
    pub total_verify_rounds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes_baseline_commit: Option<String>,

    #[serde(skip)]
    pub in_flight_stage: Option<StageLease>,
    #[serde(skip)]
    pub current_subagent_role: Option<String>,
    #[serde(skip)]
    pub live_subagent_tokens: u64,
    #[serde(skip)]
    pub live_tokens_by_model: Vec<(String, u64)>,
    #[serde(skip)]
    pub live_context_pct: u8,
    #[serde(skip)]
    pub live_turn_count: u32,
    #[serde(skip)]
    pub live_tool_call_count: u32,
}

#[derive(Debug)]
pub struct GoalTracker {
    orchestration: Option<GoalOrchestration>,
    active_since: Option<Instant>,
    next_stage_id: u64,
}

impl GoalTracker {
    pub fn new() -> Self {
        Self {
            orchestration: None,
            active_since: None,
            next_stage_id: 0,
        }
    }

    /// Old or internally inconsistent Goal snapshots deliberately do not
    /// migrate. A valid active snapshot keeps its phase and is re-driven by
    /// the idle hook with a fresh lease.
    pub fn from_snapshot(mut snapshot: GoalOrchestration) -> Option<Self> {
        if snapshot.architecture_version != GOAL_ARCHITECTURE_VERSION {
            tracing::warn!(
                found = snapshot.architecture_version,
                expected = GOAL_ARCHITECTURE_VERSION,
                "discarding incompatible Goal state"
            );
            return None;
        }
        if !Self::snapshot_invariants_hold(&snapshot) {
            tracing::warn!(
                goal_id = snapshot.goal_id,
                status = ?snapshot.status,
                phase = ?snapshot.phase,
                "discarding internally inconsistent Goal state"
            );
            return None;
        }
        snapshot.in_flight_stage = None;
        snapshot.current_subagent_role = None;
        snapshot.live_subagent_tokens = 0;
        snapshot.live_tokens_by_model.clear();
        snapshot.live_context_pct = 0;
        snapshot.live_turn_count = 0;
        snapshot.live_tool_call_count = 0;
        let active_since = (snapshot.status == GoalStatus::Active).then(Instant::now);
        Some(Self {
            orchestration: Some(snapshot),
            active_since,
            next_stage_id: 0,
        })
    }

    fn snapshot_invariants_hold(snapshot: &GoalOrchestration) -> bool {
        if snapshot.goal_id.trim().is_empty()
            || snapshot.objective.trim().is_empty()
            || snapshot.parent_tokens_spent < 0
            || snapshot.subagent_tokens_spent < 0
        {
            return false;
        }
        if snapshot.phase != GoalPhase::Planning && snapshot.plan.markdown.trim().is_empty() {
            return false;
        }
        if matches!(
            snapshot.phase,
            GoalPhase::Verifying | GoalPhase::Summarizing
        ) && snapshot
            .candidate_summary
            .as_deref()
            .is_none_or(|summary| summary.trim().is_empty())
        {
            return false;
        }
        snapshot.status != GoalStatus::Complete || snapshot.phase == GoalPhase::Summarizing
    }

    pub fn snapshot(&self) -> Option<&GoalOrchestration> {
        self.orchestration.as_ref()
    }

    pub fn snapshot_mut(&mut self) -> Option<&mut GoalOrchestration> {
        self.orchestration.as_mut()
    }

    pub fn status(&self) -> Option<GoalStatus> {
        self.orchestration.as_ref().map(|goal| goal.status)
    }

    pub fn phase(&self) -> Option<GoalPhase> {
        self.orchestration.as_ref().map(|goal| goal.phase)
    }

    pub fn objective(&self) -> Option<&str> {
        self.orchestration
            .as_ref()
            .map(|goal| goal.objective.as_str())
    }

    pub fn token_budget(&self) -> Option<i64> {
        self.orchestration
            .as_ref()
            .and_then(|goal| goal.token_budget)
    }

    pub fn create_goal(
        &mut self,
        goal_id: String,
        objective: String,
        token_budget: Option<i64>,
        token_baseline: i64,
        created_at: String,
        baseline_commit: Option<String>,
    ) {
        let mut goal = GoalOrchestration {
            architecture_version: GOAL_ARCHITECTURE_VERSION,
            goal_id,
            objective,
            objective_revision: 0,
            status: GoalStatus::Active,
            phase: GoalPhase::Planning,
            plan: GoalPlan::empty(created_at.clone()),
            token_budget,
            token_baseline,
            parent_tokens_spent: 0,
            subagent_tokens_spent: 0,
            last_session_tokens_seen: Some(token_baseline),
            elapsed_ms: 0,
            created_at,
            history: Vec::new(),
            pause_message: None,
            verifier_feedback: None,
            candidate_summary: None,
            last_gap_fingerprint: None,
            repeated_gap_count: 0,
            planner_failures: 0,
            total_worker_rounds: 0,
            total_verify_rounds: 0,
            changes_baseline_commit: baseline_commit,
            in_flight_stage: None,
            current_subagent_role: None,
            live_subagent_tokens: 0,
            live_tokens_by_model: Vec::new(),
            live_context_pct: 0,
            live_turn_count: 0,
            live_tool_call_count: 0,
        };
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::GoalCreated, None));
        self.orchestration = Some(goal);
        self.active_since = Some(Instant::now());
        self.next_stage_id = 0;
    }

    /// Explicit `/goal edit`; ordinary user messages never call this.
    pub fn revise_goal(&mut self, objective: String, token_budget: Option<i64>) -> bool {
        // An edit starts a new objective revision, not a new Goal accounting
        // lifetime. Settle the current active interval before resetting the
        // stage clock below.
        self.account_elapsed();
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete {
            return false;
        }
        goal.objective = objective;
        goal.objective_revision = goal.objective_revision.saturating_add(1);
        if token_budget.is_some() {
            goal.token_budget = token_budget;
        }
        goal.status = GoalStatus::Active;
        goal.phase = GoalPhase::Planning;
        goal.plan = GoalPlan::empty(chrono::Utc::now().to_rfc3339());
        goal.pause_message = None;
        goal.verifier_feedback = None;
        goal.candidate_summary = None;
        goal.last_gap_fingerprint = None;
        goal.repeated_gap_count = 0;
        goal.planner_failures = 0;
        goal.in_flight_stage = None;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::GoalRevised, None));
        self.active_since = Some(Instant::now());
        true
    }

    pub fn replace_plan(
        &mut self,
        markdown: String,
        updated_by: GoalPlanAuthor,
        reason: Option<String>,
    ) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active || markdown.trim().is_empty() {
            return false;
        }
        goal.plan.revision = goal.plan.revision.saturating_add(1);
        goal.plan.markdown = markdown;
        goal.plan.updated_at = chrono::Utc::now().to_rfc3339();
        goal.plan.updated_by = updated_by;
        goal.phase = GoalPhase::Executing;
        goal.in_flight_stage = None;
        // A plan revision starts a new execution attempt. A candidate belongs
        // to the plan revision that produced it and must not survive into the
        // revised plan, even though useful verifier feedback remains on the
        // blackboard for the implementer.
        goal.candidate_summary = None;
        goal.planner_failures = 0;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::PlanningCompleted, reason));
        true
    }

    pub fn apply_planner_result(&mut self, lease: &StageLease, markdown: String) -> bool {
        if !self.lease_is_current(lease, GoalPhase::Planning) {
            return false;
        }
        self.release_stage(lease);
        self.replace_plan(
            markdown,
            GoalPlanAuthor::Planner,
            Some("background planner".into()),
        )
    }

    pub fn claim_stage(&mut self, phase: GoalPhase) -> Option<StageLease> {
        let goal = self.orchestration.as_mut()?;
        if goal.status != GoalStatus::Active
            || goal.phase != phase
            || goal.in_flight_stage.is_some()
        {
            return None;
        }
        self.next_stage_id = self.next_stage_id.saturating_add(1);
        let lease = StageLease {
            goal_id: goal.goal_id.clone(),
            objective_revision: goal.objective_revision,
            plan_revision: goal.plan.revision,
            stage_id: self.next_stage_id,
        };
        goal.in_flight_stage = Some(lease.clone());
        Some(lease)
    }

    pub fn lease_is_current(&self, lease: &StageLease, phase: GoalPhase) -> bool {
        self.orchestration.as_ref().is_some_and(|goal| {
            goal.status == GoalStatus::Active
                && goal.phase == phase
                && goal.in_flight_stage.as_ref() == Some(lease)
        })
    }

    pub fn release_stage(&mut self, lease: &StageLease) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.in_flight_stage.as_ref() != Some(lease) {
            return false;
        }
        goal.in_flight_stage = None;
        true
    }

    pub fn planner_failed(&mut self, lease: &StageLease, message: String) -> bool {
        if !self.release_stage(lease) {
            return false;
        }
        self.account_elapsed();
        let goal = self.orchestration.as_mut().expect("lease had a goal");
        goal.planner_failures = goal.planner_failures.saturating_add(1);
        goal.history.push(GoalHistoryEntry::new(
            GoalEvent::PlanningFailed,
            Some(message.clone()),
        ));
        if goal.planner_failures >= INFRA_FAILURE_PAUSE_THRESHOLD {
            goal.status = GoalStatus::Paused;
            goal.pause_message = Some(message);
            self.active_since = None;
        }
        true
    }

    pub fn candidate_complete(&mut self, message: String) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active || goal.phase != GoalPhase::Executing {
            return false;
        }
        goal.candidate_summary = Some(message);
        goal.phase = GoalPhase::Verifying;
        goal.in_flight_stage = None;
        true
    }

    pub fn verification_not_achieved(
        &mut self,
        lease: &StageLease,
        feedback: String,
        fingerprint: String,
    ) -> bool {
        if !self.release_stage(lease) {
            return false;
        }
        self.account_elapsed();
        let goal = self.orchestration.as_mut().expect("lease had a goal");
        goal.total_verify_rounds = goal.total_verify_rounds.saturating_add(1);
        if goal.last_gap_fingerprint.as_deref() == Some(fingerprint.as_str()) {
            goal.repeated_gap_count = goal.repeated_gap_count.saturating_add(1);
        } else {
            goal.last_gap_fingerprint = Some(fingerprint);
            goal.repeated_gap_count = 1;
        }
        goal.verifier_feedback = Some(feedback.clone());
        goal.candidate_summary = None;
        goal.history.push(GoalHistoryEntry::new(
            GoalEvent::VerificationRejected,
            Some(feedback.clone()),
        ));
        if goal.repeated_gap_count >= IDENTICAL_GAP_BLOCK_THRESHOLD {
            goal.status = GoalStatus::Blocked;
            goal.pause_message = Some(feedback);
            self.active_since = None;
        } else {
            goal.phase = GoalPhase::Executing;
        }
        true
    }

    pub fn verification_blocked(&mut self, lease: &StageLease, message: String) -> bool {
        if !self.release_stage(lease) {
            return false;
        }
        self.account_elapsed();
        let goal = self.orchestration.as_mut().expect("lease had a goal");
        goal.status = GoalStatus::Blocked;
        goal.pause_message = Some(message);
        goal.in_flight_stage = None;
        self.active_since = None;
        true
    }

    pub fn verification_achieved(&mut self, lease: &StageLease) -> bool {
        if !self.release_stage(lease) {
            return false;
        }
        let goal = self.orchestration.as_mut().expect("lease had a goal");
        goal.total_verify_rounds = goal.total_verify_rounds.saturating_add(1);
        goal.phase = GoalPhase::Summarizing;
        goal.verifier_feedback = None;
        goal.last_gap_fingerprint = None;
        goal.repeated_gap_count = 0;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::VerificationAccepted, None));
        true
    }

    pub fn worker_started(&mut self) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active || goal.phase != GoalPhase::Executing {
            return false;
        }
        goal.total_worker_rounds = goal.total_worker_rounds.saturating_add(1);
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::WorkerStarted, None));
        true
    }

    pub fn complete_verified(&mut self) -> bool {
        self.account_elapsed();
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active || goal.phase != GoalPhase::Summarizing {
            return false;
        }
        goal.status = GoalStatus::Complete;
        goal.in_flight_stage = None;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::GoalCompleted, None));
        self.active_since = None;
        true
    }

    pub fn pause(&mut self, reason: GoalPauseReason) -> bool {
        self.pause_with_message(reason, reason.history_detail().to_string())
    }

    pub fn pause_with_message(&mut self, reason: GoalPauseReason, message: String) -> bool {
        self.account_elapsed();
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status != GoalStatus::Active {
            return false;
        }
        goal.status = if reason == GoalPauseReason::Verification {
            GoalStatus::Blocked
        } else {
            GoalStatus::Paused
        };
        goal.pause_message = Some(message.clone());
        goal.in_flight_stage = None;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::GoalPaused, Some(message)));
        self.active_since = None;
        true
    }

    pub fn resume(&mut self) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if !matches!(goal.status, GoalStatus::Paused | GoalStatus::Blocked) {
            return false;
        }
        goal.status = GoalStatus::Active;
        goal.pause_message = None;
        goal.in_flight_stage = None;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::GoalResumed, None));
        self.active_since = Some(Instant::now());
        true
    }

    pub fn budget_limit(&mut self) -> bool {
        self.account_elapsed();
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete || goal.status == GoalStatus::BudgetLimited {
            return false;
        }
        goal.status = GoalStatus::BudgetLimited;
        goal.in_flight_stage = None;
        goal.history
            .push(GoalHistoryEntry::new(GoalEvent::BudgetExceeded, None));
        self.active_since = None;
        true
    }

    pub fn set_token_budget(&mut self, budget: Option<i64>) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete {
            return false;
        }
        goal.token_budget = budget;
        if goal.status == GoalStatus::BudgetLimited {
            goal.status = GoalStatus::Paused;
        }
        true
    }

    pub fn clear(&mut self) {
        self.orchestration = None;
        self.active_since = None;
    }

    pub fn account_elapsed(&mut self) {
        if let (Some(goal), Some(since)) = (self.orchestration.as_mut(), self.active_since) {
            goal.elapsed_ms = goal
                .elapsed_ms
                .saturating_add(since.elapsed().as_millis() as u64);
            self.active_since = Some(Instant::now());
        }
    }

    /// Charge parent-session tokens monotonically while the Goal is unfinished.
    /// A completed Goal is a frozen receipt: later Normal turns must not change
    /// its accounting when the session is reloaded.
    pub fn account_parent_tokens(&mut self, current_session_tokens: i64) -> i64 {
        let Some(goal) = self.orchestration.as_mut() else {
            return 0;
        };
        if goal.status != GoalStatus::Complete {
            let last = goal.last_session_tokens_seen.unwrap_or(goal.token_baseline);
            if current_session_tokens > last {
                goal.parent_tokens_spent = goal
                    .parent_tokens_spent
                    .saturating_add(current_session_tokens - last);
            }
            goal.last_session_tokens_seen = Some(current_session_tokens);
        }
        goal.parent_tokens_spent
    }

    pub fn settle_subagent_tokens(&mut self, tokens: i64) -> bool {
        let Some(goal) = self.orchestration.as_mut() else {
            return false;
        };
        if goal.status == GoalStatus::Complete || tokens <= 0 {
            return false;
        }
        goal.subagent_tokens_spent = goal.subagent_tokens_spent.saturating_add(tokens);
        true
    }

    pub fn subagent_tokens_spent(&self) -> i64 {
        self.orchestration
            .as_ref()
            .map(|goal| goal.subagent_tokens_spent)
            .unwrap_or(0)
    }

    pub fn update_live_progress(
        &mut self,
        subagent_tokens: u64,
        tokens_by_model: Vec<(String, u64)>,
        _context_window: u64,
        context_pct: u8,
        turn_count: u32,
        tool_call_count: u32,
    ) {
        if let Some(goal) = self.orchestration.as_mut() {
            goal.live_subagent_tokens = subagent_tokens;
            goal.live_tokens_by_model = tokens_by_model;
            goal.live_context_pct = context_pct;
            goal.live_turn_count = turn_count;
            goal.live_tool_call_count = tool_call_count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> GoalTracker {
        let mut tracker = GoalTracker::new();
        tracker.create_goal("g1".into(), "ship it".into(), None, 0, "now".into(), None);
        tracker
    }

    #[test]
    fn plan_revision_invalidates_old_lease() {
        let mut tracker = tracker();
        let lease = tracker.claim_stage(GoalPhase::Planning).unwrap();
        assert!(tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Agent, None));
        assert!(!tracker.lease_is_current(&lease, GoalPhase::Planning));
    }

    #[test]
    fn plan_update_invalidates_in_flight_verifier() {
        let mut tracker = tracker();
        tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Planner, None);
        tracker.candidate_complete("candidate".into());
        let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
        assert!(tracker.replace_plan(
            "- [x] implement\n- [ ] integration test".into(),
            GoalPlanAuthor::Agent,
            Some("new evidence".into()),
        ));
        assert!(!tracker.lease_is_current(&lease, GoalPhase::Verifying));
        assert_eq!(tracker.phase(), Some(GoalPhase::Executing));
        assert!(tracker.snapshot().unwrap().candidate_summary.is_none());
    }

    #[test]
    fn explicit_edit_replans_and_clears_verification_evidence() {
        let mut tracker = tracker();
        tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Planner, None);
        tracker.candidate_complete("candidate".into());
        let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
        tracker.verification_not_achieved(&lease, "missing test".into(), "test".into());

        assert!(tracker.revise_goal("ship it safely".into(), Some(42)));
        let goal = tracker.snapshot().unwrap();
        assert_eq!(goal.objective_revision, 1);
        assert_eq!(goal.phase, GoalPhase::Planning);
        assert_eq!(goal.plan.revision, 0);
        assert!(goal.plan.markdown.is_empty());
        assert!(goal.verifier_feedback.is_none());
        assert!(goal.candidate_summary.is_none());
        assert_eq!(goal.token_budget, Some(42));
    }

    #[test]
    fn execution_round_does_not_change_objective_or_plan_revision() {
        let mut tracker = tracker();
        tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Planner, None);
        let before = tracker.snapshot().unwrap();
        let revisions = (before.objective_revision, before.plan.revision);
        assert!(tracker.worker_started());
        let after = tracker.snapshot().unwrap();
        assert_eq!((after.objective_revision, after.plan.revision), revisions);
    }

    #[test]
    fn planner_retries_three_times_then_pauses_and_resume_rearms_it() {
        let mut tracker = tracker();
        for attempt in 1..=INFRA_FAILURE_PAUSE_THRESHOLD {
            let lease = tracker.claim_stage(GoalPhase::Planning).unwrap();
            assert!(tracker.planner_failed(&lease, format!("infra {attempt}")));
        }
        assert_eq!(tracker.status(), Some(GoalStatus::Paused));
        assert!(tracker.claim_stage(GoalPhase::Planning).is_none());
        assert!(tracker.resume());
        assert!(tracker.claim_stage(GoalPhase::Planning).is_some());
    }

    #[test]
    fn explicit_pause_invalidates_planner_lease() {
        let mut tracker = tracker();
        let lease = tracker.claim_stage(GoalPhase::Planning).unwrap();
        assert!(tracker.pause(GoalPauseReason::User));
        assert!(!tracker.lease_is_current(&lease, GoalPhase::Planning));
        assert!(!tracker.apply_planner_result(&lease, "stale".into()));
    }

    #[test]
    fn third_identical_gap_blocks() {
        let mut tracker = tracker();
        tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Planner, None);
        for attempt in 1..=3 {
            tracker.candidate_complete("done".into());
            let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
            tracker.verification_not_achieved(&lease, "still broken".into(), "same".into());
            assert_eq!(
                tracker.status(),
                Some(if attempt == 3 {
                    GoalStatus::Blocked
                } else {
                    GoalStatus::Active
                })
            );
        }
    }

    #[test]
    fn a_distinct_gap_resets_the_block_counter() {
        let mut tracker = tracker();
        tracker.replace_plan("- [ ] implement".into(), GoalPlanAuthor::Planner, None);
        for fingerprint in ["same", "same", "different"] {
            tracker.candidate_complete("done".into());
            let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
            tracker.verification_not_achieved(
                &lease,
                format!("gap {fingerprint}"),
                fingerprint.into(),
            );
        }
        let goal = tracker.snapshot().unwrap();
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.repeated_gap_count, 1);
        assert_eq!(goal.phase, GoalPhase::Executing);
    }

    #[test]
    fn achieved_candidate_requires_finalization_before_complete() {
        let mut tracker = tracker();
        tracker.replace_plan("- [x] implement".into(), GoalPlanAuthor::Planner, None);
        tracker.candidate_complete("verified candidate".into());
        let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
        assert!(tracker.verification_achieved(&lease));
        assert_eq!(tracker.phase(), Some(GoalPhase::Summarizing));
        assert_eq!(tracker.status(), Some(GoalStatus::Active));
        assert!(tracker.complete_verified());
        assert_eq!(tracker.status(), Some(GoalStatus::Complete));
    }

    #[test]
    fn verifier_can_block_immediately_with_evidence() {
        let mut tracker = tracker();
        tracker.replace_plan("- [x] inspect".into(), GoalPlanAuthor::Planner, None);
        tracker.candidate_complete("candidate".into());
        let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
        assert!(tracker.verification_blocked(&lease, "missing credentials".into()));
        assert_eq!(tracker.status(), Some(GoalStatus::Blocked));
        assert_eq!(
            tracker.snapshot().unwrap().pause_message.as_deref(),
            Some("missing credentials")
        );
    }

    #[test]
    fn restart_drops_transient_lease_and_preserves_active_phase() {
        let mut tracker = tracker();
        let old_lease = tracker.claim_stage(GoalPhase::Planning).unwrap();
        let snapshot = tracker.snapshot().unwrap().clone();
        let mut restored =
            GoalTracker::from_snapshot(snapshot).expect("current snapshot should restore");
        assert!(!restored.lease_is_current(&old_lease, GoalPhase::Planning));
        assert_eq!(restored.status(), Some(GoalStatus::Active));
        assert_eq!(restored.phase(), Some(GoalPhase::Planning));
        assert!(restored.claim_stage(GoalPhase::Planning).is_some());
    }

    #[test]
    fn restart_preserves_durable_subagent_tokens() {
        let mut tracker = tracker();
        assert!(tracker.settle_subagent_tokens(250));
        let encoded = serde_json::to_vec(tracker.snapshot().unwrap()).unwrap();
        let snapshot: GoalOrchestration = serde_json::from_slice(&encoded).unwrap();
        let restored =
            GoalTracker::from_snapshot(snapshot).expect("current snapshot should restore");
        assert_eq!(restored.subagent_tokens_spent(), 250);
    }

    #[test]
    fn revising_an_active_goal_preserves_elapsed_time() {
        let mut tracker = tracker();
        tracker.active_since = Some(Instant::now() - std::time::Duration::from_millis(50));

        assert!(tracker.revise_goal("revised objective".into(), None));

        assert!(
            tracker.snapshot().unwrap().elapsed_ms >= 40,
            "the active interval before /goal edit must be retained"
        );
    }

    #[test]
    fn completed_receipt_freezes_parent_token_accounting() {
        let mut tracker = tracker();
        assert_eq!(tracker.account_parent_tokens(100), 100);
        assert!(tracker.replace_plan("- [x] done".into(), GoalPlanAuthor::Planner, None));
        assert!(tracker.candidate_complete("candidate".into()));
        let lease = tracker.claim_stage(GoalPhase::Verifying).unwrap();
        assert!(tracker.verification_achieved(&lease));
        assert_eq!(tracker.account_parent_tokens(150), 150);
        assert!(tracker.complete_verified());
        assert_eq!(tracker.account_parent_tokens(10_000), 150);
    }

    #[test]
    fn inconsistent_verifying_snapshot_is_dropped() {
        let mut tracker = tracker();
        assert!(tracker.replace_plan("- [ ] verify".into(), GoalPlanAuthor::Planner, None));
        assert!(tracker.candidate_complete("candidate".into()));
        let mut snapshot = tracker.snapshot().unwrap().clone();
        snapshot.candidate_summary = None;
        assert!(GoalTracker::from_snapshot(snapshot).is_none());
    }

    #[test]
    fn incompatible_snapshot_is_dropped() {
        let tracker = tracker();
        let mut snapshot = tracker.snapshot().unwrap().clone();
        snapshot.architecture_version = 1;
        assert!(GoalTracker::from_snapshot(snapshot).is_none());
    }
}
