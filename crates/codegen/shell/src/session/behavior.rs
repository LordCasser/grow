//! Primary-Agent Behavior state machine.
//!
//! Behavior is one mutually exclusive, session-scoped collaboration protocol.
//! It does not select an Agent role, grant tools, or own Workflow/Goal runtime
//! state. The controller is synchronous and contains no SessionActor or I/O.

use tool_types::BehaviorId;
use tool_types::{BehaviorAvailabilityDisposition, BehaviorAvailabilityEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorChangeOutcome {
    Applied,
    InFlight,
    Superseded,
    ConfirmationRequired { message: String, remaining_ms: u64 },
    Rejected { message: String },
}

/// Runtime facts captured by `SessionActor` before asking the coordinator for
/// a transition. The coordinator is deliberately I/O-free: it never queries a
/// workflow, runs a model, persists a file, or touches the pager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorRequestAuthority {
    Picker,
    HostCommand,
    GoalLifecycle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BehaviorForeground {
    #[default]
    Idle,
    BehaviorControl,
    HostCommand,
    Regular,
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorSwitchFacts {
    pub unavailable_reason: Option<String>,
    pub active_goal: bool,
    pub public_workflow_active: bool,
    pub source_owned_work_active: bool,
    pub termination_open: bool,
    pub pending_step_control: bool,
    pub foreground: BehaviorForeground,
}

impl Default for BehaviorSwitchFacts {
    fn default() -> Self {
        Self {
            unavailable_reason: None,
            active_goal: false,
            public_workflow_active: false,
            source_owned_work_active: false,
            termination_open: true,
            pending_step_control: false,
            foreground: BehaviorForeground::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorEffect {
    CancelSourceForeground(BehaviorId),
    Select(BehaviorId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorDecision {
    pub outcome: BehaviorChangeOutcome,
    pub effects: Vec<BehaviorEffect>,
}

impl BehaviorChangeOutcome {
    pub fn response_meta(&self) -> serde_json::Map<String, serde_json::Value> {
        let value = match self {
            Self::Applied => serde_json::json!({ "status": "applied" }),
            Self::InFlight => serde_json::json!({ "status": "in_flight" }),
            Self::Superseded => serde_json::json!({ "status": "superseded" }),
            Self::ConfirmationRequired {
                message,
                remaining_ms,
            } => serde_json::json!({
                "status": "confirmation_required",
                "message": message,
                "remainingMs": remaining_ms,
            }),
            Self::Rejected { message } => serde_json::json!({
                "status": "rejected",
                "message": message,
            }),
        };
        serde_json::json!({ "grow/behaviorChange": value })
            .as_object()
            .cloned()
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PlanPhase {
    Drafting,
    AwaitingApproval,
    Executing,
    Amending,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanDecisionHandoff {
    pub kind: chat_state::PlanHandoffKind,
    pub artifact_hash: String,
    pub artifact_revision: u64,
    pub submission_id: Option<PlanSubmissionId>,
    pub decision_id: Option<String>,
    pub feedback: Option<String>,
}

/// A submitted Plan identity is allocated monotonically by its session, so
/// resetting Plan runtime cannot make an old approval look current again.
pub type PlanSubmissionId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetainedPlanStatus {
    Deferred,
    Reviewing,
    Abandoned,
    Approved,
    RevisionRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetainedPlanAction {
    Defer,
    Review,
    Abandon,
    Approve,
    Revise,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedPlanDecision {
    pub decision_id: String,
    pub submission_id: PlanSubmissionId,
    pub action: RetainedPlanAction,
    pub feedback: Option<String>,
    pub resulting_status: RetainedPlanStatus,
}

/// An immutable artifact reference and its decision receipts, independent of
/// whichever Behavior currently owns the live Plan runtime.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedPlanCandidate {
    pub submission_id: PlanSubmissionId,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub amendment: bool,
    pub status: RetainedPlanStatus,
    pub decisions: Vec<RetainedPlanDecision>,
}

/// Read-only result of a completed execution, independent of live Behavior.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedPlan {
    pub submission_id: PlanSubmissionId,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub report_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainedPlanDecisionOutcome {
    Applied(RetainedPlanStatus),
    Duplicate(RetainedPlanStatus),
    Conflict,
    Stale,
    Invalid,
}

/// Serialized control projection. The coordinator does not use this enum as
/// its mutable state machine: Plan keeps independent runtime state and is
/// combined here only for one atomic persistence payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BehaviorState {
    Normal,
    Clarify,
    Plan(PlanPhase),
    Workflow,
    Goal,
}

/// Pure Plan runtime owned by the Plan Behavior.
///
/// Keeping this separate from selection is important: selecting a Behavior is
/// a coordination decision, while advancing a Plan phase is a Plan-runtime
/// transition. Both live under the same mutex only so snapshots stay atomic.
#[derive(Debug, Clone)]
struct PlanRuntime {
    phase: PlanPhase,
    /// A reverse approval request is parked and must be restored after a
    /// reconnect. This is transport state for a Plan phase, not another phase.
    approval_pending: bool,
    artifact_revision: u64,
    artifact_hash: Option<String>,
    last_handoff: Option<PlanDecisionHandoff>,
    submission_id: Option<PlanSubmissionId>,
}

impl Default for PlanRuntime {
    fn default() -> Self {
        Self {
            phase: PlanPhase::Drafting,
            approval_pending: false,
            artifact_revision: 0,
            artifact_hash: None,
            last_handoff: None,
            submission_id: None,
        }
    }
}

pub struct BehaviorCoordinator {
    selected: BehaviorId,
    plan: PlanRuntime,
    last_plan_submission_id: PlanSubmissionId,
    retained_plan: Option<RetainedPlanCandidate>,
    completed_plan: Option<CompletedPlan>,
    pending_switch: Option<PendingBehaviorSwitch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingBehaviorSwitch {
    source: BehaviorId,
    target: BehaviorId,
    owner: Option<String>,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BehaviorSnapshot {
    pub state: BehaviorState,
    pub approval_pending: bool,
    pub plan_artifact_revision: u64,
    pub plan_artifact_hash: Option<String>,
    #[serde(default)]
    pub last_plan_handoff: Option<PlanDecisionHandoff>,
    pub plan_submission_id: Option<PlanSubmissionId>,
    pub last_plan_submission_id: PlanSubmissionId,
    pub retained_plan: Option<RetainedPlanCandidate>,
    #[serde(default)]
    pub completed_plan: Option<CompletedPlan>,
}

impl BehaviorSnapshot {
    pub fn normal() -> Self {
        Self::selected(BehaviorId::Normal)
    }

    pub fn selected(behavior: BehaviorId) -> Self {
        let state = match behavior {
            BehaviorId::Normal => BehaviorState::Normal,
            BehaviorId::Clarify => BehaviorState::Clarify,
            BehaviorId::Plan => BehaviorState::Plan(PlanPhase::Drafting),
            BehaviorId::Workflow => BehaviorState::Workflow,
            BehaviorId::Goal => BehaviorState::Goal,
        };
        Self {
            state,
            approval_pending: false,
            plan_artifact_revision: 0,
            plan_artifact_hash: None,
            last_plan_handoff: None,
            plan_submission_id: None,
            last_plan_submission_id: 0,
            retained_plan: None,
            completed_plan: None,
        }
    }

    pub fn behavior(&self) -> BehaviorId {
        match &self.state {
            BehaviorState::Normal => BehaviorId::Normal,
            BehaviorState::Clarify => BehaviorId::Clarify,
            BehaviorState::Plan(_) => BehaviorId::Plan,
            BehaviorState::Workflow => BehaviorId::Workflow,
            BehaviorState::Goal => BehaviorId::Goal,
        }
    }

    /// Reject cross-runtime residue instead of reviving state that could not
    /// have been emitted by the current coordinator. Plan transport/artifact
    /// fields are meaningful only while Plan is selected.
    pub fn runtime_fields_match_selection(&self) -> bool {
        matches!(self.state, BehaviorState::Plan(_))
            || (!self.approval_pending
                && self.plan_artifact_revision == 0
                && self.plan_artifact_hash.is_none()
                && self.last_plan_handoff.is_none()
                && self.plan_submission_id.is_none())
    }

    pub fn completed_plan_is_valid(&self) -> bool {
        self.completed_plan.as_ref().is_none_or(|completed| {
            completed.submission_id > 0
                && completed.submission_id <= self.last_plan_submission_id
                && completed.artifact_revision > 0
                && [&completed.artifact_hash, &completed.report_hash]
                    .into_iter()
                    .all(|hash| is_blake3_hash(hash))
        })
    }

    pub fn retained_plan_is_valid(&self) -> bool {
        if self
            .plan_submission_id
            .is_some_and(|id| id == 0 || id > self.last_plan_submission_id)
        {
            return false;
        }
        let Some(candidate) = self.retained_plan.as_ref() else {
            return true;
        };
        let decisions_well_formed = candidate.decisions.iter().all(|decision| {
            !decision.decision_id.trim().is_empty()
                && decision.submission_id == candidate.submission_id
                && decision.resulting_status == status_after(decision.action)
                && decision
                    .feedback
                    .as_deref()
                    .is_none_or(|text| !text.trim().is_empty() && text == text.trim())
        });
        let decisions_unique = candidate
            .decisions
            .iter()
            .enumerate()
            .all(|(index, decision)| {
                candidate.decisions[..index]
                    .iter()
                    .all(|prior| prior.decision_id != decision.decision_id)
            });
        let mut replayed_status = RetainedPlanStatus::Reviewing;
        let transitions_valid = candidate.decisions.iter().all(|decision| {
            let next = match (replayed_status, decision.action) {
                (RetainedPlanStatus::Reviewing, RetainedPlanAction::Defer) => {
                    RetainedPlanStatus::Deferred
                }
                (RetainedPlanStatus::Deferred, RetainedPlanAction::Review) => {
                    RetainedPlanStatus::Reviewing
                }
                (RetainedPlanStatus::Reviewing, RetainedPlanAction::Review) => {
                    RetainedPlanStatus::Reviewing
                }
                (RetainedPlanStatus::Reviewing, RetainedPlanAction::Approve) => {
                    RetainedPlanStatus::Approved
                }
                (RetainedPlanStatus::Reviewing, RetainedPlanAction::Revise) => {
                    RetainedPlanStatus::RevisionRequested
                }
                (
                    RetainedPlanStatus::Deferred | RetainedPlanStatus::Reviewing,
                    RetainedPlanAction::Abandon,
                ) => RetainedPlanStatus::Abandoned,
                _ => return false,
            };
            replayed_status = next;
            decision.resulting_status == next
        });
        candidate.submission_id > 0
            && candidate.submission_id <= self.last_plan_submission_id
            && candidate.artifact_revision > 0
            && is_blake3_hash(&candidate.artifact_hash)
            && !candidate.decisions.is_empty()
            && (self.plan_submission_id != Some(candidate.submission_id)
                || (self.plan_artifact_revision == candidate.artifact_revision
                    && self.plan_artifact_hash.as_deref()
                        == Some(candidate.artifact_hash.as_str())))
            && decisions_well_formed
            && decisions_unique
            && transitions_valid
            && replayed_status == candidate.status
            && match (
                candidate.status,
                self.plan_submission_id == Some(candidate.submission_id),
                self.last_plan_handoff.as_ref(),
            ) {
                (RetainedPlanStatus::Approved, true, Some(handoff)) => {
                    handoff.kind == chat_state::PlanHandoffKind::Execute
                        && handoff.submission_id == Some(candidate.submission_id)
                        && candidate.decisions.last().is_some_and(|decision| {
                            decision.action == RetainedPlanAction::Approve
                                && handoff.decision_id.as_deref()
                                    == Some(decision.decision_id.as_str())
                        })
                }
                (RetainedPlanStatus::RevisionRequested, true, Some(handoff)) => {
                    handoff.kind == chat_state::PlanHandoffKind::Revise
                        && handoff.submission_id == Some(candidate.submission_id)
                        && candidate.decisions.last().is_some_and(|decision| {
                            decision.action == RetainedPlanAction::Revise
                                && handoff.decision_id.as_deref()
                                    == Some(decision.decision_id.as_str())
                        })
                }
                (
                    RetainedPlanStatus::Approved | RetainedPlanStatus::RevisionRequested,
                    true,
                    None,
                ) => false,
                _ => true,
            }
    }

    pub fn plan_runtime_is_valid(&self) -> bool {
        if !matches!(self.state, BehaviorState::Plan(_)) {
            return self.runtime_fields_match_selection();
        }
        match self.last_plan_handoff.as_ref() {
            None => true,
            Some(handoff) => {
                handoff.artifact_revision > 0
                    && self.plan_artifact_revision == handoff.artifact_revision
                    && self.plan_artifact_hash.as_deref() == Some(handoff.artifact_hash.as_str())
                    && handoff
                        .submission_id
                        .is_none_or(|id| self.plan_submission_id == Some(id))
                    && handoff
                        .decision_id
                        .as_deref()
                        .is_none_or(|id| !id.trim().is_empty())
                    && (handoff.decision_id.is_none() || handoff.submission_id.is_some())
                    && matches!(
                        (&self.state, handoff.kind),
                        (
                            BehaviorState::Plan(PlanPhase::Executing),
                            chat_state::PlanHandoffKind::Execute
                        ) | (
                            BehaviorState::Plan(PlanPhase::Drafting | PlanPhase::Amending),
                            chat_state::PlanHandoffKind::Revise
                        )
                    )
            }
        }
    }
}

impl Default for BehaviorCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl BehaviorCoordinator {
    pub fn new() -> Self {
        Self {
            selected: BehaviorId::Normal,
            plan: PlanRuntime::default(),
            last_plan_submission_id: 0,
            retained_plan: None,
            completed_plan: None,
            pending_switch: None,
        }
    }

    pub fn from_snapshot(snapshot: BehaviorSnapshot) -> Self {
        let (selected, plan_phase) = match snapshot.state {
            BehaviorState::Normal => (BehaviorId::Normal, PlanPhase::Drafting),
            BehaviorState::Clarify => (BehaviorId::Clarify, PlanPhase::Drafting),
            BehaviorState::Plan(phase) => (BehaviorId::Plan, phase),
            BehaviorState::Workflow => (BehaviorId::Workflow, PlanPhase::Drafting),
            BehaviorState::Goal => (BehaviorId::Goal, PlanPhase::Drafting),
        };
        Self {
            selected,
            plan: PlanRuntime {
                phase: plan_phase,
                approval_pending: snapshot.approval_pending,
                artifact_revision: snapshot.plan_artifact_revision,
                artifact_hash: snapshot.plan_artifact_hash,
                last_handoff: snapshot.last_plan_handoff,
                submission_id: snapshot.plan_submission_id,
            },
            last_plan_submission_id: snapshot.last_plan_submission_id,
            retained_plan: snapshot.retained_plan,
            completed_plan: snapshot.completed_plan,
            pending_switch: None,
        }
    }

    pub fn snapshot(&self) -> BehaviorSnapshot {
        BehaviorSnapshot {
            state: self.state(),
            approval_pending: self.plan.approval_pending,
            plan_artifact_revision: self.plan.artifact_revision,
            plan_artifact_hash: self.plan.artifact_hash.clone(),
            last_plan_handoff: self.plan.last_handoff.clone(),
            plan_submission_id: self.plan.submission_id,
            last_plan_submission_id: self.last_plan_submission_id,
            retained_plan: self.retained_plan.clone(),
            completed_plan: self.completed_plan.clone(),
        }
    }

    pub fn state(&self) -> BehaviorState {
        match self.selected {
            BehaviorId::Normal => BehaviorState::Normal,
            BehaviorId::Clarify => BehaviorState::Clarify,
            BehaviorId::Plan => BehaviorState::Plan(self.plan.phase),
            BehaviorId::Workflow => BehaviorState::Workflow,
            BehaviorId::Goal => BehaviorState::Goal,
        }
    }

    pub fn behavior(&self) -> BehaviorId {
        self.selected
    }

    pub fn select_behavior(&mut self, behavior: BehaviorId) -> bool {
        if self.selected == behavior {
            return false;
        }
        self.selected = behavior;
        self.plan = PlanRuntime::default();
        self.pending_switch = None;
        true
    }

    /// Decide a Behavior transition from an atomic control-plane snapshot.
    /// Effects are declarative and must be executed serially by SessionActor;
    /// this method never performs work on its own.
    pub fn decide_switch(
        &mut self,
        target: BehaviorId,
        facts: BehaviorSwitchFacts,
        confirmation_window: std::time::Duration,
    ) -> BehaviorDecision {
        self.decide_switch_owned(target, facts, confirmation_window, None)
    }

    /// Decide a transition while binding any destructive confirmation latch
    /// to the client generation that created it. A different client may ask
    /// for the same target, but its first request must never confirm another
    /// client's interruption.
    pub fn decide_switch_owned(
        &mut self,
        target: BehaviorId,
        facts: BehaviorSwitchFacts,
        confirmation_window: std::time::Duration,
        confirmation_owner: Option<&str>,
    ) -> BehaviorDecision {
        let assessment = self.assess_switch(target, &facts, BehaviorRequestAuthority::Picker);
        self.decide_assessed_switch_owned(
            target,
            assessment,
            confirmation_window,
            confirmation_owner,
        )
    }

    /// Resolve a previously assessed transition. The Shell uses this entry
    /// point so the exact projection decision, including its reason, is also
    /// the admission decision. Only confirmation ownership mutates here.
    pub fn decide_assessed_switch_owned(
        &mut self,
        target: BehaviorId,
        assessment: BehaviorAvailabilityEntry,
        confirmation_window: std::time::Duration,
        confirmation_owner: Option<&str>,
    ) -> BehaviorDecision {
        debug_assert_eq!(assessment.behavior, target);
        let source = self.behavior();
        if assessment.disposition == BehaviorAvailabilityDisposition::Unavailable {
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::Rejected {
                    message: assessment.reason.clone().unwrap_or_else(|| {
                        format!("{} behavior is unavailable.", target.display_label())
                    }),
                },
                effects: Vec::new(),
            };
        }

        if assessment.disposition == BehaviorAvailabilityDisposition::ConfirmationRequired
            && !self.confirm_interrupting_switch_owned(
                target,
                confirmation_window,
                confirmation_owner,
            )
        {
            let remaining_ms = self
                .pending_switch()
                .map(|(_, _, remaining)| remaining)
                .unwrap_or(confirmation_window.as_millis() as u64);
            let message = assessment.reason.clone().unwrap_or_else(|| {
                format!(
                    "Switching to {} requires confirmation. Select it again to confirm.",
                    target.display_label()
                )
            });
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                },
                effects: Vec::new(),
            };
        }

        if source == target {
            self.clear_pending_switch();
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::Applied,
                effects: Vec::new(),
            };
        }

        let mut effects = Vec::new();
        if assessment.disposition == BehaviorAvailabilityDisposition::ConfirmationRequired {
            effects.push(BehaviorEffect::CancelSourceForeground(source));
        }
        effects.push(BehaviorEffect::Select(target));
        BehaviorDecision {
            outcome: BehaviorChangeOutcome::Applied,
            effects,
        }
    }

    /// Project one transition target from the same facts consumed by
    /// [`Self::decide_switch`]. It never mutates the confirmation latch.
    pub fn switch_availability(
        &self,
        target: BehaviorId,
        facts: &BehaviorSwitchFacts,
    ) -> BehaviorAvailabilityEntry {
        self.assess_switch(target, facts, BehaviorRequestAuthority::Picker)
    }

    /// Pure transition assessment shared by user-visible projection and the
    /// authoritative admission path. It never mutates the confirmation latch.
    pub fn assess_switch(
        &self,
        target: BehaviorId,
        facts: &BehaviorSwitchFacts,
        authority: BehaviorRequestAuthority,
    ) -> BehaviorAvailabilityEntry {
        let supported = facts.unavailable_reason.is_none();
        let source = self.behavior();
        let admission_reason = if !facts.termination_open {
            Some("The session is shutting down; Behavior selection is unavailable.".to_string())
        } else if facts.pending_step_control
            && facts.foreground != BehaviorForeground::BehaviorControl
        {
            Some(format!(
                "{} Behavior cannot overtake an earlier model or Agent control. Retry after the current step reaches its boundary.",
                target.display_label()
            ))
        } else {
            let foreground_allowed = match facts.foreground {
                BehaviorForeground::Idle | BehaviorForeground::BehaviorControl => true,
                BehaviorForeground::HostCommand => {
                    authority == BehaviorRequestAuthority::HostCommand
                        || (authority == BehaviorRequestAuthority::GoalLifecycle
                            && target == BehaviorId::Goal
                            && matches!(source, BehaviorId::Normal | BehaviorId::Clarify))
                }
                BehaviorForeground::Regular => {
                    matches!(source, BehaviorId::Normal | BehaviorId::Clarify)
                        && ((authority == BehaviorRequestAuthority::Picker
                            && matches!(target, BehaviorId::Plan | BehaviorId::Workflow))
                            || (authority == BehaviorRequestAuthority::GoalLifecycle
                                && target == BehaviorId::Goal))
                }
                BehaviorForeground::Busy => false,
            };
            (!foreground_allowed).then(|| {
                format!(
                    "Stop the active foreground work before selecting {} Behavior.",
                    target.display_label()
                )
            })
        };
        if let Some(reason) = admission_reason {
            return BehaviorAvailabilityEntry {
                behavior: target,
                supported,
                disposition: BehaviorAvailabilityDisposition::Unavailable,
                reason: Some(reason),
            };
        }
        if source == target {
            return BehaviorAvailabilityEntry {
                behavior: target,
                supported: true,
                disposition: BehaviorAvailabilityDisposition::Available,
                reason: None,
            };
        }
        let reason = if facts.active_goal && target != BehaviorId::Goal {
            Some("An active Goal owns automatic continuation; pause or stop it before selecting another Behavior.".to_string())
        } else if let Some(reason) = facts.unavailable_reason.clone() {
            Some(reason)
        } else if target != BehaviorId::Workflow
            && target.owns_special_runtime()
            && facts.public_workflow_active
        {
            Some(format!(
                "{} behavior is unavailable while a public Workflow run is active; wait for it or stop it explicitly.",
                target.display_label()
            ))
        } else {
            None
        };
        if let Some(reason) = reason {
            return BehaviorAvailabilityEntry {
                behavior: target,
                supported,
                disposition: BehaviorAvailabilityDisposition::Unavailable,
                reason: Some(reason),
            };
        }
        if facts.source_owned_work_active {
            let reason = if source == BehaviorId::Workflow {
                format!(
                    "An active public Workflow Run will continue in the background, but it can only be managed in Workflow behavior. Select {} again to leave and confirm.",
                    target.display_label()
                )
            } else {
                format!(
                    "Switching to {} will interrupt active {} work. Select it again to confirm.",
                    target.display_label(),
                    source.display_label()
                )
            };
            return BehaviorAvailabilityEntry {
                behavior: target,
                supported: true,
                disposition: BehaviorAvailabilityDisposition::ConfirmationRequired,
                reason: Some(reason),
            };
        }
        BehaviorAvailabilityEntry {
            behavior: target,
            supported: true,
            disposition: BehaviorAvailabilityDisposition::Available,
            reason: None,
        }
    }

    pub fn confirm_interrupting_switch(
        &mut self,
        target: BehaviorId,
        window: std::time::Duration,
    ) -> bool {
        self.confirm_interrupting_switch_owned(target, window, None)
    }

    pub fn confirm_interrupting_switch_owned(
        &mut self,
        target: BehaviorId,
        window: std::time::Duration,
        owner: Option<&str>,
    ) -> bool {
        let now = std::time::Instant::now();
        let source = self.behavior();
        let confirmed = self.pending_switch.as_ref().is_some_and(|pending| {
            pending.source == source
                && pending.target == target
                && pending.owner.as_deref() == owner
                && pending.expires_at > now
        });
        if confirmed {
            self.pending_switch = None;
            true
        } else {
            self.pending_switch = Some(PendingBehaviorSwitch {
                source,
                target,
                owner: owner.map(str::to_owned),
                expires_at: now + window,
            });
            false
        }
    }

    pub fn pending_switch(&self) -> Option<(BehaviorId, BehaviorId, u64)> {
        let pending = self.pending_switch.as_ref()?;
        let remaining = pending
            .expires_at
            .checked_duration_since(std::time::Instant::now())?;
        Some((pending.source, pending.target, remaining.as_millis() as u64))
    }

    pub fn clear_pending_switch(&mut self) -> bool {
        self.pending_switch.take().is_some()
    }

    pub fn is_plan(&self) -> bool {
        self.selected == BehaviorId::Plan
    }

    pub fn is_drafting_plan(&self) -> bool {
        self.is_plan() && self.plan.phase == PlanPhase::Drafting
    }

    pub fn plan_allows_edits(&self) -> bool {
        self.is_plan() && self.plan.phase == PlanPhase::Executing
    }

    pub fn plan_phase_label(&self) -> Option<&'static str> {
        self.is_plan().then_some(match self.plan.phase {
            PlanPhase::Drafting => "drafting",
            PlanPhase::AwaitingApproval => "awaiting_approval",
            PlanPhase::Executing => "executing",
            PlanPhase::Amending => "amending",
        })
    }

    pub fn submit_initial_plan(&mut self) -> bool {
        if !self.is_plan() || self.plan.phase != PlanPhase::Drafting {
            return false;
        }
        let Some(submission_id) = self.allocate_plan_submission_id() else {
            return false;
        };
        self.plan.phase = PlanPhase::AwaitingApproval;
        self.plan.approval_pending = true;
        self.plan.submission_id = Some(submission_id);
        true
    }

    pub fn submit_plan_amendment(&mut self) -> bool {
        if !self.is_plan() || !matches!(self.plan.phase, PlanPhase::Executing | PlanPhase::Amending)
        {
            return false;
        }
        let Some(submission_id) = self.allocate_plan_submission_id() else {
            return false;
        };
        self.plan.phase = PlanPhase::Amending;
        self.plan.approval_pending = true;
        self.plan.submission_id = Some(submission_id);
        true
    }

    fn allocate_plan_submission_id(&mut self) -> Option<PlanSubmissionId> {
        let next = self.last_plan_submission_id.checked_add(1)?;
        self.last_plan_submission_id = next;
        self.plan.submission_id = Some(next);
        Some(next)
    }

    pub fn plan_submission_id(&self) -> Option<PlanSubmissionId> {
        self.plan.submission_id
    }

    pub fn retained_plan(&self) -> Option<&RetainedPlanCandidate> {
        self.retained_plan.as_ref()
    }

    /// Resolve a submitted Plan with a stable decision identity. The complete
    /// BehaviorSnapshot is the CAS key; retries are checked before CAS so an
    /// already accepted decision remains idempotent after its own mutation.
    pub fn decide_submitted_plan_if(
        &mut self,
        expected: &BehaviorSnapshot,
        submission_id: PlanSubmissionId,
        decision_id: &str,
        action: RetainedPlanAction,
        feedback: Option<String>,
    ) -> RetainedPlanDecisionOutcome {
        let feedback = normalize_plan_feedback(feedback);
        if decision_id.trim().is_empty() || action == RetainedPlanAction::Review {
            return RetainedPlanDecisionOutcome::Invalid;
        }
        if let Some(prior) = self.retained_plan.as_ref().and_then(|candidate| {
            candidate
                .decisions
                .iter()
                .find(|d| d.decision_id == decision_id)
        }) {
            return if prior.submission_id == submission_id
                && prior.action == action
                && prior.feedback == feedback
            {
                RetainedPlanDecisionOutcome::Duplicate(prior.resulting_status)
            } else {
                RetainedPlanDecisionOutcome::Conflict
            };
        }
        let abandon_expected_matches = action == RetainedPlanAction::Abandon
            && self.snapshot() == *expected
            && !(self.is_plan()
                && self.plan.approval_pending
                && self.plan.submission_id == Some(submission_id));
        if abandon_expected_matches
            && let Some(candidate) = self.retained_plan.as_mut()
            && candidate.submission_id == submission_id
            && matches!(
                candidate.status,
                RetainedPlanStatus::Deferred | RetainedPlanStatus::Reviewing
            )
        {
            candidate.status = RetainedPlanStatus::Abandoned;
            candidate.decisions.push(RetainedPlanDecision {
                decision_id: decision_id.to_owned(),
                submission_id,
                action,
                feedback,
                resulting_status: RetainedPlanStatus::Abandoned,
            });
            return RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Abandoned);
        }
        if self.snapshot() != *expected
            || !self.is_plan()
            || self.plan.submission_id != Some(submission_id)
            || !self.plan.approval_pending
            || !matches!(
                self.plan.phase,
                PlanPhase::AwaitingApproval | PlanPhase::Amending
            )
            || self.plan.artifact_revision == 0
            || !self
                .plan
                .artifact_hash
                .as_deref()
                .is_some_and(is_blake3_hash)
        {
            return RetainedPlanDecisionOutcome::Stale;
        }
        let Some(artifact_hash) = expected.plan_artifact_hash.clone() else {
            return RetainedPlanDecisionOutcome::Invalid;
        };
        let existing = self.retained_plan.as_ref();
        if existing.is_some_and(|candidate| {
            candidate.submission_id != submission_id
                && !matches!(
                    candidate.status,
                    RetainedPlanStatus::Abandoned
                        | RetainedPlanStatus::Approved
                        | RetainedPlanStatus::RevisionRequested
                )
        }) {
            return RetainedPlanDecisionOutcome::Conflict;
        }
        if existing.is_some_and(|candidate| {
            candidate.submission_id == submission_id
                && (candidate.artifact_revision != self.plan.artifact_revision
                    || Some(candidate.artifact_hash.as_str()) != self.plan.artifact_hash.as_deref())
        }) {
            return RetainedPlanDecisionOutcome::Stale;
        }
        let starting_status = existing
            .filter(|candidate| candidate.submission_id == submission_id)
            .map(|candidate| candidate.status)
            .unwrap_or(RetainedPlanStatus::Reviewing);
        if starting_status != RetainedPlanStatus::Reviewing {
            return RetainedPlanDecisionOutcome::Invalid;
        }
        let next = match action {
            RetainedPlanAction::Defer => RetainedPlanStatus::Deferred,
            RetainedPlanAction::Abandon => RetainedPlanStatus::Abandoned,
            RetainedPlanAction::Approve => RetainedPlanStatus::Approved,
            RetainedPlanAction::Revise => RetainedPlanStatus::RevisionRequested,
            RetainedPlanAction::Review => unreachable!(),
        };
        match action {
            RetainedPlanAction::Approve => {
                if !self.approve_submitted_plan_if_with_feedback(expected, feedback.clone()) {
                    return RetainedPlanDecisionOutcome::Stale;
                }
            }
            RetainedPlanAction::Revise => {
                if !self.reject_submitted_plan_if_with_feedback(expected, feedback.clone()) {
                    return RetainedPlanDecisionOutcome::Stale;
                }
            }
            RetainedPlanAction::Defer | RetainedPlanAction::Abandon => {
                self.plan.approval_pending = false;
                self.plan.last_handoff = None;
                if self.plan.phase == PlanPhase::AwaitingApproval {
                    self.plan.phase = PlanPhase::Drafting;
                }
            }
            RetainedPlanAction::Review => unreachable!(),
        }
        if self
            .retained_plan
            .as_ref()
            .is_none_or(|candidate| candidate.submission_id != submission_id)
        {
            self.retained_plan = Some(RetainedPlanCandidate {
                submission_id,
                artifact_revision: expected.plan_artifact_revision,
                artifact_hash,
                amendment: expected.state == BehaviorState::Plan(PlanPhase::Amending),
                status: starting_status,
                decisions: Vec::new(),
            });
        }
        let candidate = self
            .retained_plan
            .as_mut()
            .expect("candidate was initialized");
        candidate.status = next;
        candidate.decisions.push(RetainedPlanDecision {
            decision_id: decision_id.to_owned(),
            submission_id,
            action,
            feedback,
            resulting_status: next,
        });
        if let Some(handoff) = self.plan.last_handoff.as_mut()
            && matches!(
                action,
                RetainedPlanAction::Approve | RetainedPlanAction::Revise
            )
        {
            handoff.submission_id = Some(submission_id);
            handoff.decision_id = Some(decision_id.to_owned());
        }
        RetainedPlanDecisionOutcome::Applied(next)
    }

    /// Open the actual Plan approval gate for a retained candidate. Review is
    /// CAS-bound to the full snapshot and never creates an execution handoff.
    pub fn review_retained_plan_if(
        &mut self,
        expected: &BehaviorSnapshot,
        submission_id: PlanSubmissionId,
        decision_id: &str,
        feedback: Option<String>,
    ) -> RetainedPlanDecisionOutcome {
        let feedback = normalize_plan_feedback(feedback);
        if decision_id.trim().is_empty() {
            return RetainedPlanDecisionOutcome::Invalid;
        }
        let Some(current) = self.retained_plan.as_ref() else {
            return RetainedPlanDecisionOutcome::Stale;
        };
        if let Some(prior) = current
            .decisions
            .iter()
            .find(|prior| prior.decision_id == decision_id)
        {
            return if prior.submission_id == submission_id
                && prior.action == RetainedPlanAction::Review
                && prior.feedback == feedback
            {
                RetainedPlanDecisionOutcome::Duplicate(prior.resulting_status)
            } else {
                RetainedPlanDecisionOutcome::Conflict
            };
        }
        if current.submission_id != submission_id {
            return RetainedPlanDecisionOutcome::Stale;
        }
        if self.snapshot() != *expected
            || !matches!(
                current.status,
                RetainedPlanStatus::Deferred | RetainedPlanStatus::Reviewing
            )
            || !matches!(self.behavior(), BehaviorId::Normal | BehaviorId::Plan)
            || self.plan.approval_pending
            || (self.is_plan()
                && self.plan.phase != PlanPhase::Drafting
                && !(self.plan.phase == PlanPhase::Amending && current.amendment))
        {
            return RetainedPlanDecisionOutcome::Stale;
        }
        let candidate = current.clone();
        self.selected = BehaviorId::Plan;
        self.plan = PlanRuntime {
            phase: if candidate.amendment {
                PlanPhase::Amending
            } else {
                PlanPhase::AwaitingApproval
            },
            approval_pending: true,
            artifact_revision: candidate.artifact_revision,
            artifact_hash: Some(candidate.artifact_hash.clone()),
            last_handoff: None,
            submission_id: Some(candidate.submission_id),
        };
        let current = self.retained_plan.as_mut().expect("candidate was checked");
        current.status = RetainedPlanStatus::Reviewing;
        current.decisions.push(RetainedPlanDecision {
            decision_id: decision_id.to_owned(),
            submission_id,
            action: RetainedPlanAction::Review,
            feedback,
            resulting_status: RetainedPlanStatus::Reviewing,
        });
        self.pending_switch = None;
        RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Reviewing)
    }

    pub fn approve_submitted_plan(&mut self) -> bool {
        self.approve_submitted_plan_with_feedback(None)
    }

    pub fn approve_submitted_plan_with_feedback(&mut self, feedback: Option<String>) -> bool {
        if !self.is_plan()
            || !matches!(
                self.plan.phase,
                PlanPhase::AwaitingApproval | PlanPhase::Amending
            )
        {
            return false;
        }
        let Some(artifact_hash) = self.plan.artifact_hash.clone() else {
            return false;
        };
        self.plan.phase = PlanPhase::Executing;
        self.plan.approval_pending = false;
        self.plan.last_handoff = Some(PlanDecisionHandoff {
            kind: chat_state::PlanHandoffKind::Execute,
            artifact_hash,
            artifact_revision: self.plan.artifact_revision,
            submission_id: self.plan.submission_id,
            decision_id: None,
            feedback: normalize_plan_feedback(feedback),
        });
        true
    }

    pub fn reject_submitted_plan(&mut self) -> bool {
        self.reject_submitted_plan_with_feedback(None)
    }

    pub fn reject_submitted_plan_with_feedback(&mut self, feedback: Option<String>) -> bool {
        if !self.is_plan()
            || !matches!(
                self.plan.phase,
                PlanPhase::AwaitingApproval | PlanPhase::Amending
            )
        {
            return false;
        }
        let Some(artifact_hash) = self.plan.artifact_hash.clone() else {
            return false;
        };
        self.plan.approval_pending = false;
        match self.plan.phase {
            PlanPhase::AwaitingApproval => {
                self.plan.phase = PlanPhase::Drafting;
            }
            PlanPhase::Amending => {}
            _ => return false,
        }
        self.plan.last_handoff = Some(PlanDecisionHandoff {
            kind: chat_state::PlanHandoffKind::Revise,
            artifact_hash,
            artifact_revision: self.plan.artifact_revision,
            submission_id: self.plan.submission_id,
            decision_id: None,
            feedback: normalize_plan_feedback(feedback),
        });
        true
    }

    pub fn finish_plan(&mut self) -> bool {
        if !self.is_plan() {
            return false;
        }
        self.selected = BehaviorId::Normal;
        self.plan = PlanRuntime::default();
        true
    }

    pub fn complete_plan(&mut self, report_hash: String) -> bool {
        if self.state() != BehaviorState::Plan(PlanPhase::Executing) {
            return false;
        }
        let (Some(submission_id), Some(artifact_hash)) =
            (self.plan.submission_id, self.plan.artifact_hash.clone())
        else {
            return false;
        };
        let completed = CompletedPlan {
            submission_id,
            artifact_revision: self.plan.artifact_revision,
            artifact_hash,
            report_hash,
        };
        self.finish_plan();
        self.completed_plan = Some(completed);
        true
    }

    pub fn clear_completed_plan(&mut self) {
        self.completed_plan = None;
    }

    pub fn set_approval_pending(&mut self, pending: bool) {
        self.plan.approval_pending = pending;
    }

    pub fn approval_pending(&self) -> bool {
        self.plan.approval_pending
    }

    /// Capture the exact submitted Plan state that a pending approval is
    /// allowed to resolve.  The artifact revision/hash are part of the
    /// snapshot so a decision cannot be applied to a later submission that
    /// happens to reuse the same phase.
    pub fn pending_plan_approval_snapshot(&self) -> Option<BehaviorSnapshot> {
        (self.plan.approval_pending
            && self.is_plan()
            && matches!(
                self.plan.phase,
                PlanPhase::AwaitingApproval | PlanPhase::Amending
            ))
        .then(|| self.snapshot())
    }

    fn matches_pending_plan_approval(&self, expected: &BehaviorSnapshot) -> bool {
        self.snapshot() == *expected
            && expected.approval_pending
            && matches!(
                &expected.state,
                BehaviorState::Plan(PlanPhase::AwaitingApproval | PlanPhase::Amending)
            )
    }

    /// Approve only the exact pending submission captured by `expected`.
    /// This is intentionally a compare-and-swap boundary for restored
    /// approvals, whose response arrives outside the actor mailbox.
    pub fn approve_submitted_plan_if(&mut self, expected: &BehaviorSnapshot) -> bool {
        self.approve_submitted_plan_if_with_feedback(expected, None)
    }

    pub fn approve_submitted_plan_if_with_feedback(
        &mut self,
        expected: &BehaviorSnapshot,
        feedback: Option<String>,
    ) -> bool {
        if !self.matches_pending_plan_approval(expected) {
            return false;
        }
        let Some(artifact_hash) = self.plan.artifact_hash.clone() else {
            return false;
        };
        self.plan.phase = PlanPhase::Executing;
        self.plan.approval_pending = false;
        self.plan.last_handoff = Some(PlanDecisionHandoff {
            kind: chat_state::PlanHandoffKind::Execute,
            artifact_hash,
            artifact_revision: self.plan.artifact_revision,
            submission_id: self.plan.submission_id,
            decision_id: None,
            feedback: normalize_plan_feedback(feedback),
        });
        true
    }

    /// Reject only the exact pending submission captured by `expected`.
    pub fn reject_submitted_plan_if(&mut self, expected: &BehaviorSnapshot) -> bool {
        self.reject_submitted_plan_if_with_feedback(expected, None)
    }

    /// Close a failed approval transport without fabricating a human
    /// Request-changes decision or successor handoff.
    pub fn fail_pending_plan_approval_if(&mut self, expected: &BehaviorSnapshot) -> bool {
        if !self.matches_pending_plan_approval(expected) {
            return false;
        }
        self.plan.approval_pending = false;
        match self.plan.phase {
            PlanPhase::AwaitingApproval => self.plan.phase = PlanPhase::Drafting,
            PlanPhase::Amending => {}
            _ => unreachable!("pending approval phase was checked above"),
        }
        self.plan.last_handoff = None;
        true
    }

    pub fn reject_submitted_plan_if_with_feedback(
        &mut self,
        expected: &BehaviorSnapshot,
        feedback: Option<String>,
    ) -> bool {
        if !self.matches_pending_plan_approval(expected) {
            return false;
        }
        let Some(artifact_hash) = self.plan.artifact_hash.clone() else {
            return false;
        };
        self.plan.approval_pending = false;
        match self.plan.phase {
            PlanPhase::AwaitingApproval => {
                self.plan.phase = PlanPhase::Drafting;
            }
            PlanPhase::Amending => {}
            _ => unreachable!("pending approval phase was checked above"),
        }
        self.plan.last_handoff = Some(PlanDecisionHandoff {
            kind: chat_state::PlanHandoffKind::Revise,
            artifact_hash,
            artifact_revision: self.plan.artifact_revision,
            submission_id: self.plan.submission_id,
            decision_id: None,
            feedback: normalize_plan_feedback(feedback),
        });
        true
    }

    /// Clear malformed persisted approval transport state only when the
    /// snapshot being normalized is still current.
    pub fn clear_approval_pending_if(&mut self, expected: &BehaviorSnapshot) -> bool {
        if self.snapshot() != *expected || !expected.approval_pending {
            return false;
        }
        self.plan.approval_pending = false;
        true
    }

    pub fn record_plan_artifact(&mut self, markdown: &str) {
        self.plan.artifact_revision = self.plan.artifact_revision.saturating_add(1);
        self.plan.artifact_hash = Some(blake3::hash(markdown.as_bytes()).to_hex().to_string());
        self.plan.last_handoff = None;
        self.plan.submission_id = None;
    }

    pub fn plan_artifact_hash(&self) -> Option<&str> {
        self.plan.artifact_hash.as_deref()
    }

    pub fn plan_artifact_revision(&self) -> u64 {
        self.plan.artifact_revision
    }

    pub fn last_plan_handoff(&self) -> Option<&PlanDecisionHandoff> {
        self.plan.last_handoff.as_ref()
    }

    pub fn plan_artifact_is_valid(&self, markdown: &str) -> bool {
        self.plan.artifact_revision > 0
            && self.plan.artifact_hash.as_deref()
                == Some(blake3::hash(markdown.as_bytes()).to_hex().as_str())
    }
}

fn normalize_plan_feedback(feedback: Option<String>) -> Option<String> {
    feedback.and_then(|feedback| {
        let feedback = feedback.trim();
        (!feedback.is_empty()).then(|| feedback.to_owned())
    })
}

fn status_after(action: RetainedPlanAction) -> RetainedPlanStatus {
    match action {
        RetainedPlanAction::Defer => RetainedPlanStatus::Deferred,
        RetainedPlanAction::Review => RetainedPlanStatus::Reviewing,
        RetainedPlanAction::Abandon => RetainedPlanStatus::Abandoned,
        RetainedPlanAction::Approve => RetainedPlanStatus::Approved,
        RetainedPlanAction::Revise => RetainedPlanStatus::RevisionRequested,
    }
}

fn is_blake3_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn plan_behavior_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/base.md")
}

pub fn plan_phase_model_context(snapshot: &BehaviorSnapshot, plan_content: &str) -> Option<String> {
    let BehaviorState::Plan(phase) = snapshot.state else {
        return None;
    };
    let plan = plan_content.trim();
    let feedback = snapshot
        .last_plan_handoff
        .as_ref()
        .and_then(|handoff| handoff.feedback.as_deref())
        .map(str::trim)
        .filter(|feedback| !feedback.is_empty());
    let context = match phase {
        PlanPhase::Drafting => format!(
            "{}\n\nPlan phase: Drafting. Workspace mutation and execution are forbidden. Investigate, resolve material uncertainties, and submit one complete plan for approval with `plan_control` action `submit`.{}{}{}",
            plan_behavior_template().trim(),
            if plan.is_empty() {
                ""
            } else {
                "\n\nPlan requiring revision:\n\n"
            },
            if plan.is_empty() { "" } else { plan },
            feedback.map_or_else(String::new, |feedback| format!(
                "\n\nApproval feedback:\n\n{feedback}"
            )),
        ),
        PlanPhase::AwaitingApproval => format!(
            "{}\n\nPlan phase: AwaitingApproval. The plan below is frozen and awaiting a human decision. Do not execute it, modify the workspace, or replace it unless the approval result moves the phase.\n\nFrozen plan:\n\n{}",
            plan_behavior_template().trim(),
            plan,
        ),
        PlanPhase::Amending if snapshot.approval_pending => format!(
            "{}\n\nPlan phase: Amending. The replacement plan below is frozen and awaiting a human decision. Workspace mutation and execution are forbidden; do not revise or resubmit it until the approval result moves the phase.\n\nFrozen replacement plan:\n\n{}",
            plan_behavior_template().trim(),
            plan,
        ),
        PlanPhase::Amending => format!(
            "{}\n\nPlan phase: Amending. Workspace mutation and execution are forbidden. Revise the complete plan in response to the review feedback, then submit the complete replacement with `plan_control` action `amend`.\n\nPlan requiring amendment:\n\n{}{}",
            plan_behavior_template().trim(),
            plan,
            feedback.map_or_else(String::new, |feedback| format!(
                "\n\nApproval feedback:\n\n{feedback}"
            )),
        ),
        PlanPhase::Executing => format!(
            "The following plan is a frozen, human-approved contract and is now in the Executing phase. Implement and verify only work within its scope. If its objective, architecture, major steps, risk boundary, or deletion behavior must change, stop workspace mutation and submit a complete replacement with `plan_control` action `amend`. When all approved work is verified, call `plan_control` with action `complete` and a non-empty final user-facing `report`; that tool result is the only final report.\n\nApproved plan:\n\n{}{}",
            plan,
            feedback.map_or_else(String::new, |feedback| format!(
                "\n\nApproval review:\n\n{feedback}"
            )),
        ),
    };
    Some(context)
}

pub fn plan_mode_edit_rejected_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/edit-rejected.md")
}

pub fn clarify_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/clarify.md")
}

pub fn workflow_reminder_template() -> &'static str {
    concat!(
        include_str!("../../prompts/behaviors/workflow.md"),
        "\n\n",
        include_str!("../../../../../docs/workflow-rhai.md"),
    )
}

pub fn goal_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/goal.md")
}

/// Render one append-only model context item for a Behavior transition.
///
/// The item is committed in the same Timeline Control event as the selection,
/// so later assistant output stays after the exact protocol that conditioned
/// it. Switching to Normal explicitly retires earlier special instructions.
pub fn behavior_transition_context(admitted: BehaviorId) -> String {
    let instructions = match admitted {
        BehaviorId::Normal => {
            "Normal Behavior is now active. Earlier special Behavior protocols are historical and no longer apply. Follow the active Agent role and the current user request without Clarify, Plan, Workflow, or Goal-specific constraints."
        }
        BehaviorId::Clarify => clarify_reminder_template(),
        BehaviorId::Plan => plan_behavior_template(),
        BehaviorId::Workflow => workflow_reminder_template(),
        BehaviorId::Goal => goal_reminder_template(),
    };
    let escaped = instructions.replace("</behavior-context>", "<\\/behavior-context>");
    format!("<behavior-context>\n{escaped}\n</behavior-context>")
}

pub(crate) const MAX_PLAN_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn write_plan_artifact(
    session: &crate::session::storage::ContainedDirectory,
    markdown: &str,
) -> std::io::Result<String> {
    if markdown.is_empty() || markdown.len() as u64 > MAX_PLAN_ARTIFACT_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Plan artifact is empty or exceeds its byte limit",
        ));
    }
    let hash = blake3::hash(markdown.as_bytes()).to_hex().to_string();
    crate::session::persistence::write_immutable_blob_to_directory(
        session,
        &std::path::Path::new("artifacts/plan").join(format!("{hash}.md")),
        markdown.as_bytes(),
    )?;
    Ok(hash)
}

pub(crate) fn read_plan_artifact(
    session: &crate::session::storage::ContainedDirectory,
    hash: &str,
) -> std::io::Result<String> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Plan artifact hash is invalid",
        ));
    }
    let directory = session.open_relative(
        std::path::Path::new("artifacts/plan"),
        "Plan artifact directory",
        false,
    )?;
    let bytes = directory.read_bounded(
        std::ffi::OsStr::new(&format!("{hash}.md")),
        "Plan artifact",
        MAX_PLAN_ARTIFACT_BYTES,
    )?;
    if blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Plan artifact hash mismatch",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_behavior_transition_has_one_canonical_wrapper() {
        for behavior in [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::Goal,
        ] {
            let context = behavior_transition_context(behavior);
            assert!(context.starts_with("<behavior-context>\n"), "{behavior:?}");
            assert!(context.ends_with("\n</behavior-context>"), "{behavior:?}");
            assert_eq!(context.matches("<behavior-context>").count(), 1);
        }
        assert!(behavior_transition_context(BehaviorId::Normal).contains("no longer apply"));
    }

    fn controller() -> BehaviorCoordinator {
        BehaviorCoordinator::new()
    }

    #[test]
    fn behaviors_are_mutually_exclusive() {
        let mut controller = controller();
        for behavior in [
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::Goal,
        ] {
            assert!(controller.select_behavior(behavior));
            assert_eq!(controller.behavior(), behavior);
        }
        assert!(controller.select_behavior(BehaviorId::Normal));
        assert_eq!(controller.state(), BehaviorState::Normal);
    }

    #[test]
    fn only_executing_plan_allows_edits() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# plan");
        assert!(!controller.plan_allows_edits());
        assert!(controller.submit_initial_plan());
        assert!(!controller.plan_allows_edits());
        assert!(controller.approve_submitted_plan());
        assert!(controller.plan_allows_edits());
        assert!(controller.submit_plan_amendment());
        assert!(!controller.plan_allows_edits());
    }

    #[test]
    fn amendment_rejection_stays_read_only_for_revision() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# initial");
        controller.submit_initial_plan();
        controller.approve_submitted_plan();
        controller.record_plan_artifact("# amendment");
        controller.submit_plan_amendment();
        controller.set_approval_pending(false);
        assert!(controller.reject_submitted_plan());
        assert_eq!(controller.state(), BehaviorState::Plan(PlanPhase::Amending));
        assert!(!controller.plan_allows_edits());
        assert!(controller.submit_plan_amendment());
    }

    #[test]
    fn approval_transport_state_persists_without_hidden_behavior() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.submit_initial_plan();
        let restored = BehaviorCoordinator::from_snapshot(controller.snapshot());
        assert_eq!(
            restored.state(),
            BehaviorState::Plan(PlanPhase::AwaitingApproval)
        );
        assert!(restored.approval_pending());
    }

    #[test]
    fn approval_cas_rejects_stale_phase_or_artifact() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# first");
        assert!(controller.submit_initial_plan());
        let expected = controller.snapshot();

        // A later Plan state must not be resolved by the old decision.
        assert!(controller.approve_submitted_plan_if(&expected));
        assert!(controller.submit_plan_amendment());
        assert!(!controller.approve_submitted_plan_if(&expected));
        assert_eq!(controller.state(), BehaviorState::Plan(PlanPhase::Amending));

        // Even the same phase is stale when the submitted artifact changed.
        controller.set_approval_pending(false);
        controller.record_plan_artifact("# second");
        assert!(controller.submit_plan_amendment());
        assert!(!controller.reject_submitted_plan_if(&expected));
        assert!(controller.approval_pending());
    }

    #[test]
    fn approval_handoff_is_bound_to_the_frozen_artifact_and_feedback_is_projected_once() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# approved");
        assert!(controller.submit_initial_plan());
        let expected = controller.snapshot();
        assert!(controller.approve_submitted_plan_if_with_feedback(
            &expected,
            Some("  keep the boundary explicit  ".into()),
        ));

        let snapshot = controller.snapshot();
        let handoff = snapshot.last_plan_handoff.as_ref().unwrap();
        assert_eq!(handoff.kind, chat_state::PlanHandoffKind::Execute);
        assert_eq!(handoff.artifact_revision, snapshot.plan_artifact_revision);
        assert_eq!(
            Some(handoff.artifact_hash.as_str()),
            snapshot.plan_artifact_hash.as_deref()
        );
        assert!(snapshot.plan_runtime_is_valid());

        let context = plan_phase_model_context(&snapshot, "# approved").unwrap();
        assert_eq!(context.matches("keep the boundary explicit").count(), 1);

        controller.record_plan_artifact("# replacement");
        assert!(controller.snapshot().last_plan_handoff.is_none());
    }

    #[test]
    fn revision_context_preserves_current_feedback_across_restore_and_resubmission() {
        for amendment in [false, true] {
            let mut controller = controller();
            controller.select_behavior(BehaviorId::Plan);
            if amendment {
                controller.record_plan_artifact("# approved");
                assert!(controller.submit_initial_plan());
                assert!(controller.approve_submitted_plan());
            }
            let action = if amendment { "amend" } else { "submit" };
            let phase = if amendment {
                PlanPhase::Amending
            } else {
                PlanPhase::Drafting
            };
            for (plan, feedback) in [
                ("# candidate", Some("补充恢复流程\n保留原有接口")),
                ("# replacement", Some("覆盖失败路径，不增加重试")),
                ("# final candidate", Some("   ")),
                ("# no notes", None),
            ] {
                controller.record_plan_artifact(plan);
                assert!(if amendment {
                    controller.submit_plan_amendment()
                } else {
                    controller.submit_initial_plan()
                });
                let submitted = controller.snapshot();
                let pending_context = plan_phase_model_context(&submitted, plan).unwrap();
                assert!(!pending_context.contains("Approval feedback:"));
                assert!(!pending_context.contains("补充恢复流程"));
                assert!(!pending_context.contains("覆盖失败路径"));
                assert!(controller.reject_submitted_plan_if_with_feedback(
                    &submitted,
                    feedback.map(str::to_owned),
                ));
                controller = BehaviorCoordinator::from_snapshot(controller.snapshot());
                assert_eq!(controller.state(), BehaviorState::Plan(phase));
                assert!(!controller.plan_allows_edits());
                let context = plan_phase_model_context(&controller.snapshot(), plan).unwrap();
                assert!(context.contains(plan));
                assert!(context.contains(&format!("action `{action}`")));
                assert!(context.contains("Workspace mutation and execution are forbidden"));
                if let Some(feedback) = feedback.filter(|text| !text.trim().is_empty()) {
                    assert_eq!(context.matches(feedback).count(), 1, "{phase:?}: {context}");
                } else {
                    assert!(!context.contains("Approval feedback:"));
                }
                if plan != "# candidate" {
                    assert!(!context.contains("补充恢复流程"));
                }
            }
        }
    }

    #[test]
    fn request_changes_persists_a_revision_handoff_and_plan_exit_clears_it() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let expected = controller.snapshot();
        assert!(
            controller
                .reject_submitted_plan_if_with_feedback(&expected, Some("cover recovery".into()),)
        );
        let revised = controller.snapshot();
        assert_eq!(
            revised
                .last_plan_handoff
                .as_ref()
                .map(|handoff| handoff.kind),
            Some(chat_state::PlanHandoffKind::Revise)
        );
        assert!(revised.plan_runtime_is_valid());
        assert!(controller.finish_plan());
        assert!(controller.snapshot().last_plan_handoff.is_none());
    }

    #[test]
    fn approval_transport_failure_does_not_fabricate_a_revision_handoff() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let expected = controller.snapshot();

        assert!(controller.fail_pending_plan_approval_if(&expected));
        let failed = controller.snapshot();
        assert_eq!(failed.state, BehaviorState::Plan(PlanPhase::Drafting));
        assert!(!failed.approval_pending);
        assert!(failed.last_plan_handoff.is_none());
    }

    #[test]
    fn behavior_selection_resets_plan_runtime_state() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# plan");
        controller.submit_initial_plan();
        assert!(controller.select_behavior(BehaviorId::Workflow));

        let snapshot = controller.snapshot();
        assert_eq!(snapshot.state, BehaviorState::Workflow);
        assert!(snapshot.runtime_fields_match_selection());
        assert_eq!(snapshot.plan_artifact_revision, 0);
        assert!(snapshot.plan_artifact_hash.is_none());
        assert!(!snapshot.approval_pending);
    }

    #[test]
    fn snapshot_rejects_plan_runtime_residue_under_another_behavior() {
        let snapshot = BehaviorSnapshot {
            state: BehaviorState::Normal,
            approval_pending: true,
            plan_artifact_revision: 1,
            plan_artifact_hash: Some("stale".into()),
            last_plan_handoff: None,
            plan_submission_id: Some(1),
            last_plan_submission_id: 1,
            retained_plan: None,
            completed_plan: None,
        };
        assert!(!snapshot.runtime_fields_match_selection());
    }

    #[test]
    fn retained_candidate_survives_plan_reset_and_submission_identity_is_monotonic() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# first");
        assert!(controller.submit_initial_plan());
        let first = controller.plan_submission_id().unwrap();
        assert_eq!(first, 1);
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                first,
                "defer-1",
                RetainedPlanAction::Defer,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );

        assert!(controller.finish_plan());
        assert_eq!(controller.behavior(), BehaviorId::Normal);
        let retained = controller.snapshot();
        assert!(retained.retained_plan_is_valid());
        assert_eq!(
            retained.retained_plan.as_ref().unwrap().submission_id,
            first
        );

        let restored = BehaviorCoordinator::from_snapshot(retained);
        assert_eq!(restored.retained_plan(), controller.retained_plan());
        let mut restored = restored;
        restored.select_behavior(BehaviorId::Plan);
        restored.record_plan_artifact("# second");
        assert!(restored.submit_initial_plan());
        assert_eq!(restored.plan_submission_id(), Some(first + 1));
        assert_eq!(restored.retained_plan().unwrap().submission_id, first);
    }

    #[test]
    fn retained_decisions_are_idempotent_and_reject_identity_conflicts() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let submission_id = controller.plan_submission_id().unwrap();
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "decision-1",
                RetainedPlanAction::Defer,
                Some("  later  ".into()),
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "decision-1",
                RetainedPlanAction::Defer,
                Some("later".into()),
            ),
            RetainedPlanDecisionOutcome::Duplicate(RetainedPlanStatus::Deferred)
        );
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "decision-1",
                RetainedPlanAction::Approve,
                Some("different".into()),
            ),
            RetainedPlanDecisionOutcome::Conflict
        );
        let deferred = controller.snapshot();
        assert_eq!(
            controller.review_retained_plan_if(&deferred, submission_id, "decision-2", None),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Reviewing)
        );
        let reviewing = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &reviewing,
                submission_id,
                "decision-3",
                RetainedPlanAction::Abandon,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Abandoned)
        );
        assert!(controller.snapshot().retained_plan_is_valid());
        assert_eq!(
            controller.review_retained_plan_if(&reviewing, submission_id, "decision-4", None),
            RetainedPlanDecisionOutcome::Stale
        );
    }

    #[test]
    fn retained_plan_validation_rejects_invalid_references_and_decision_history() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let id = controller.plan_submission_id().unwrap();
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                id,
                "d1",
                RetainedPlanAction::Defer,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );
        controller.finish_plan();

        let mut snapshot = controller.snapshot();
        snapshot.retained_plan.as_mut().unwrap().artifact_hash = "broken".into();
        assert!(!snapshot.retained_plan_is_valid());

        let mut snapshot = controller.snapshot();
        snapshot.retained_plan.as_mut().unwrap().decisions[0].resulting_status =
            RetainedPlanStatus::Abandoned;
        assert!(!snapshot.retained_plan_is_valid());
    }

    #[test]
    fn retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let submission_id = controller.plan_submission_id().unwrap();
        let submitted = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &submitted,
                submission_id,
                "defer-1",
                RetainedPlanAction::Defer,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );
        controller.finish_plan();

        let deferred = controller.snapshot();
        assert_eq!(
            controller.review_retained_plan_if(&deferred, submission_id, "review-1", None),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Reviewing)
        );
        let pending = controller.snapshot();
        assert_eq!(
            pending.state,
            BehaviorState::Plan(PlanPhase::AwaitingApproval)
        );
        assert!(pending.approval_pending);
        assert_eq!(pending.plan_submission_id, Some(submission_id));
        assert_eq!(
            pending.plan_artifact_revision,
            deferred.retained_plan.as_ref().unwrap().artifact_revision
        );
        assert_eq!(
            pending.plan_artifact_hash,
            deferred
                .retained_plan
                .as_ref()
                .map(|candidate| candidate.artifact_hash.clone())
        );

        assert_eq!(
            controller.decide_submitted_plan_if(
                &pending,
                submission_id,
                "approve-1",
                RetainedPlanAction::Approve,
                Some("  ship  ".into()),
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Approved)
        );
        let approved = controller.snapshot();
        assert_eq!(approved.state, BehaviorState::Plan(PlanPhase::Executing));
        assert_eq!(
            approved.last_plan_handoff.as_ref().unwrap().submission_id,
            Some(submission_id)
        );
        assert_eq!(
            approved
                .last_plan_handoff
                .as_ref()
                .unwrap()
                .decision_id
                .as_deref(),
            Some("approve-1")
        );
        assert!(approved.retained_plan_is_valid());
        assert_eq!(
            controller.decide_submitted_plan_if(
                &pending,
                submission_id,
                "approve-1",
                RetainedPlanAction::Approve,
                Some("ship".into()),
            ),
            RetainedPlanDecisionOutcome::Duplicate(RetainedPlanStatus::Approved)
        );
    }

    #[test]
    fn revise_decision_keeps_the_existing_handoff_and_rejects_stale_snapshots() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let submission_id = controller.plan_submission_id().unwrap();
        let expected = controller.snapshot();
        let mut stale = expected.clone();
        stale.last_plan_submission_id += 1;
        assert_eq!(
            controller.decide_submitted_plan_if(
                &stale,
                submission_id,
                "revise-1",
                RetainedPlanAction::Revise,
                Some("clarify scope".into()),
            ),
            RetainedPlanDecisionOutcome::Stale
        );
        assert_eq!(controller.snapshot(), expected);

        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "revise-1",
                RetainedPlanAction::Revise,
                Some("clarify scope".into()),
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::RevisionRequested)
        );
        let revised = controller.snapshot();
        assert_eq!(revised.state, BehaviorState::Plan(PlanPhase::Drafting));
        let handoff = revised.last_plan_handoff.as_ref().unwrap();
        assert_eq!(handoff.kind, chat_state::PlanHandoffKind::Revise);
        assert_eq!(handoff.submission_id, Some(submission_id));
        assert_eq!(handoff.decision_id.as_deref(), Some("revise-1"));
        assert!(revised.retained_plan_is_valid());
    }

    #[test]
    fn abandoned_candidate_can_be_closed_after_leaving_plan() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# candidate");
        assert!(controller.submit_initial_plan());
        let submission_id = controller.plan_submission_id().unwrap();
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "defer-1",
                RetainedPlanAction::Defer,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );
        controller.finish_plan();
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                submission_id,
                "abandon-1",
                RetainedPlanAction::Abandon,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Abandoned)
        );
        assert!(controller.snapshot().retained_plan_is_valid());
    }

    #[test]
    fn deferring_an_amendment_clears_the_previous_execute_handoff() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# original");
        assert!(controller.submit_initial_plan());
        assert!(controller.approve_submitted_plan());
        assert_eq!(
            controller.last_plan_handoff().unwrap().kind,
            chat_state::PlanHandoffKind::Execute
        );

        assert!(controller.submit_plan_amendment());
        let amendment_id = controller.plan_submission_id().unwrap();
        let expected = controller.snapshot();
        assert_eq!(
            controller.decide_submitted_plan_if(
                &expected,
                amendment_id,
                "defer-amendment",
                RetainedPlanAction::Defer,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
        );
        let deferred = controller.snapshot();
        assert_eq!(deferred.state, BehaviorState::Plan(PlanPhase::Amending));
        assert!(!deferred.approval_pending);
        assert!(deferred.last_plan_handoff.is_none());
        assert_eq!(deferred.plan_submission_id, Some(amendment_id));
        assert!(deferred.retained_plan_is_valid());

        assert_eq!(
            controller.review_retained_plan_if(&deferred, amendment_id, "review-amendment", None,),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Reviewing)
        );
        let reviewing = controller.snapshot();
        assert_eq!(reviewing.state, BehaviorState::Plan(PlanPhase::Amending));
        assert!(reviewing.approval_pending);
        assert_eq!(
            controller.decide_submitted_plan_if(
                &reviewing,
                amendment_id,
                "approve-amendment",
                RetainedPlanAction::Approve,
                None,
            ),
            RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Approved)
        );
        assert_eq!(
            controller.snapshot().state,
            BehaviorState::Plan(PlanPhase::Executing)
        );
    }

    #[test]
    fn interrupt_confirmation_requires_same_target() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Goal);
        let window = std::time::Duration::from_secs(3);
        assert!(!controller.confirm_interrupting_switch(BehaviorId::Plan, window));
        assert!(!controller.confirm_interrupting_switch(BehaviorId::Workflow, window));
        assert!(controller.confirm_interrupting_switch(BehaviorId::Workflow, window));
    }

    #[test]
    fn active_goal_rejects_every_other_behavior_and_reselect_is_idempotent() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Goal);
        for target in [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
        ] {
            let decision = controller.decide_switch(
                target,
                BehaviorSwitchFacts {
                    active_goal: true,
                    ..BehaviorSwitchFacts::default()
                },
                std::time::Duration::from_secs(8),
            );
            assert!(matches!(
                decision.outcome,
                BehaviorChangeOutcome::Rejected { .. }
            ));
            assert!(decision.effects.is_empty());
        }

        assert!(
            !controller
                .confirm_interrupting_switch(BehaviorId::Normal, std::time::Duration::from_secs(8))
        );
        let same = controller.decide_switch(
            BehaviorId::Goal,
            BehaviorSwitchFacts {
                active_goal: true,
                ..BehaviorSwitchFacts::default()
            },
            std::time::Duration::from_secs(8),
        );
        assert!(matches!(same.outcome, BehaviorChangeOutcome::Applied));
        assert!(same.effects.is_empty());
        assert!(controller.pending_switch().is_none());
    }

    #[test]
    fn availability_projection_uses_the_same_transition_rules_without_mutating_confirmation() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        let facts = BehaviorSwitchFacts {
            source_owned_work_active: true,
            ..BehaviorSwitchFacts::default()
        };
        let normal = controller.switch_availability(BehaviorId::Normal, &facts);
        assert_eq!(
            normal.disposition,
            BehaviorAvailabilityDisposition::ConfirmationRequired
        );
        assert!(controller.pending_switch().is_none());

        let unavailable = controller.switch_availability(
            BehaviorId::Goal,
            &BehaviorSwitchFacts {
                public_workflow_active: true,
                ..facts
            },
        );
        assert_eq!(
            unavailable.disposition,
            BehaviorAvailabilityDisposition::Unavailable
        );
        assert!(unavailable.supported);

        let unsupported = controller.switch_availability(
            BehaviorId::Goal,
            &BehaviorSwitchFacts {
                unavailable_reason: Some("Goal tools are absent".into()),
                ..BehaviorSwitchFacts::default()
            },
        );
        assert!(!unsupported.supported);
        assert_eq!(
            unsupported.disposition,
            BehaviorAvailabilityDisposition::Unavailable
        );
    }

    #[test]
    fn admission_assessment_reason_is_the_exact_rejection_reason() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        assert!(matches!(
            controller
                .decide_switch_owned(
                    BehaviorId::Clarify,
                    BehaviorSwitchFacts {
                        source_owned_work_active: true,
                        ..BehaviorSwitchFacts::default()
                    },
                    std::time::Duration::from_secs(8),
                    Some("client-a:1"),
                )
                .outcome,
            BehaviorChangeOutcome::ConfirmationRequired { .. }
        ));
        let (pending_source, pending_target, _) =
            controller.pending_switch().expect("parked confirmation");
        let assessment = controller.assess_switch(
            BehaviorId::Normal,
            &BehaviorSwitchFacts {
                foreground: BehaviorForeground::Regular,
                source_owned_work_active: true,
                ..BehaviorSwitchFacts::default()
            },
            BehaviorRequestAuthority::Picker,
        );
        let projected_reason = assessment.reason.clone().unwrap();
        let decision = controller.decide_assessed_switch_owned(
            BehaviorId::Normal,
            assessment,
            std::time::Duration::from_secs(8),
            Some("client-a:1"),
        );
        assert_eq!(
            decision.outcome,
            BehaviorChangeOutcome::Rejected {
                message: projected_reason
            }
        );
        let (source_after, target_after, _) = controller
            .pending_switch()
            .expect("unavailable assessment preserves confirmation");
        assert_eq!(
            (source_after, target_after),
            (pending_source, pending_target),
            "an unavailable assessment must neither arm nor clear confirmation"
        );
    }

    #[test]
    fn foreground_exceptions_require_the_request_authority_that_owns_them() {
        let controller = controller();
        let host_foreground = BehaviorSwitchFacts {
            foreground: BehaviorForeground::HostCommand,
            ..BehaviorSwitchFacts::default()
        };
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Plan,
                    &host_foreground,
                    BehaviorRequestAuthority::Picker,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Unavailable,
            "a Picker request cannot borrow the current HostCommand foreground"
        );
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Plan,
                    &host_foreground,
                    BehaviorRequestAuthority::HostCommand,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Available
        );

        let regular_foreground = BehaviorSwitchFacts {
            foreground: BehaviorForeground::Regular,
            ..BehaviorSwitchFacts::default()
        };
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Plan,
                    &regular_foreground,
                    BehaviorRequestAuthority::Picker,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Available,
            "the picker may select Plan for the next boundary of a Normal turn"
        );
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Workflow,
                    &regular_foreground,
                    BehaviorRequestAuthority::Picker,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Available,
            "the picker may select Workflow for the next boundary of a Normal turn"
        );
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Goal,
                    &regular_foreground,
                    BehaviorRequestAuthority::Picker,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Unavailable
        );
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Goal,
                    &regular_foreground,
                    BehaviorRequestAuthority::GoalLifecycle,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Available,
            "only Goal lifecycle admission may enter Goal beside a regular foreground"
        );
    }

    #[test]
    fn terminal_and_step_control_fences_are_part_of_the_shared_assessment() {
        let controller = controller();
        for facts in [
            BehaviorSwitchFacts {
                termination_open: false,
                ..BehaviorSwitchFacts::default()
            },
            BehaviorSwitchFacts {
                pending_step_control: true,
                ..BehaviorSwitchFacts::default()
            },
        ] {
            assert_eq!(
                controller
                    .assess_switch(BehaviorId::Plan, &facts, BehaviorRequestAuthority::Picker,)
                    .disposition,
                BehaviorAvailabilityDisposition::Unavailable
            );
        }
        assert_eq!(
            controller
                .assess_switch(
                    BehaviorId::Plan,
                    &BehaviorSwitchFacts {
                        pending_step_control: true,
                        foreground: BehaviorForeground::BehaviorControl,
                        ..BehaviorSwitchFacts::default()
                    },
                    BehaviorRequestAuthority::Picker,
                )
                .disposition,
            BehaviorAvailabilityDisposition::Available,
            "the Behavior worker that owns foreground may drain its admitted control"
        );
    }

    #[test]
    fn public_workflow_blocks_only_special_runtime_behaviors() {
        for target in [BehaviorId::Plan, BehaviorId::Goal] {
            let mut controller = controller();
            let decision = controller.decide_switch(
                target,
                BehaviorSwitchFacts {
                    public_workflow_active: true,
                    ..BehaviorSwitchFacts::default()
                },
                std::time::Duration::from_secs(8),
            );
            assert!(matches!(
                decision.outcome,
                BehaviorChangeOutcome::Rejected { .. }
            ));
        }
        for target in [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Workflow,
        ] {
            let mut controller = controller();
            let decision = controller.decide_switch(
                target,
                BehaviorSwitchFacts {
                    public_workflow_active: true,
                    ..BehaviorSwitchFacts::default()
                },
                std::time::Duration::from_secs(8),
            );
            assert!(matches!(decision.outcome, BehaviorChangeOutcome::Applied));
        }
    }

    #[test]
    fn active_public_workflow_cannot_enter_another_special_runtime() {
        for target in [BehaviorId::Plan, BehaviorId::Goal] {
            let mut controller = controller();
            controller.select_behavior(BehaviorId::Workflow);
            let decision = controller.decide_switch(
                target,
                BehaviorSwitchFacts {
                    public_workflow_active: true,
                    source_owned_work_active: true,
                    ..BehaviorSwitchFacts::default()
                },
                std::time::Duration::from_secs(8),
            );

            assert!(matches!(
                decision.outcome,
                BehaviorChangeOutcome::Rejected { .. }
            ));
            assert_eq!(controller.behavior(), BehaviorId::Workflow);
            assert!(controller.pending_switch().is_none());
        }
    }

    #[test]
    fn base_transition_matrix_has_one_identity_and_one_select_effect() {
        let behaviors = [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::Goal,
        ];
        for source in behaviors {
            for target in behaviors {
                let mut controller = controller();
                controller.select_behavior(source);
                let decision = controller.decide_switch(
                    target,
                    BehaviorSwitchFacts::default(),
                    std::time::Duration::from_secs(8),
                );
                assert!(
                    matches!(decision.outcome, BehaviorChangeOutcome::Applied),
                    "{source:?} -> {target:?}"
                );
                if source == target {
                    assert!(decision.effects.is_empty(), "{source:?} -> {target:?}");
                } else {
                    assert_eq!(
                        decision.effects,
                        [BehaviorEffect::Select(target)],
                        "{source:?} -> {target:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn interrupt_confirmation_is_bound_to_source_and_target() {
        let window = std::time::Duration::from_secs(8);
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        let facts = BehaviorSwitchFacts {
            source_owned_work_active: true,
            ..BehaviorSwitchFacts::default()
        };
        let first = controller.decide_switch(BehaviorId::Normal, facts.clone(), window);
        assert!(matches!(
            first.outcome,
            BehaviorChangeOutcome::ConfirmationRequired { .. }
        ));
        let wrong_target = controller.decide_switch(BehaviorId::Clarify, facts.clone(), window);
        assert!(matches!(
            wrong_target.outcome,
            BehaviorChangeOutcome::ConfirmationRequired { .. }
        ));
        let confirmed = controller.decide_switch(BehaviorId::Clarify, facts, window);
        assert!(matches!(confirmed.outcome, BehaviorChangeOutcome::Applied));
        assert_eq!(
            confirmed.effects,
            [
                BehaviorEffect::CancelSourceForeground(BehaviorId::Plan),
                BehaviorEffect::Select(BehaviorId::Clarify),
            ]
        );
    }

    #[test]
    fn leaving_workflow_with_active_public_run_confirms_without_cancelling_run() {
        let window = std::time::Duration::from_secs(8);
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Workflow);
        let facts = BehaviorSwitchFacts {
            public_workflow_active: true,
            source_owned_work_active: true,
            ..BehaviorSwitchFacts::default()
        };
        let first = controller.decide_switch(BehaviorId::Normal, facts.clone(), window);
        assert!(matches!(
            first.outcome,
            BehaviorChangeOutcome::ConfirmationRequired { ref message, .. }
                if message.contains("continue in the background")
        ));
        let confirmed = controller.decide_switch(BehaviorId::Normal, facts, window);
        assert!(matches!(confirmed.outcome, BehaviorChangeOutcome::Applied));
        assert_eq!(
            confirmed.effects,
            [
                BehaviorEffect::CancelSourceForeground(BehaviorId::Workflow),
                BehaviorEffect::Select(BehaviorId::Normal),
            ]
        );
    }

    #[test]
    fn interrupting_confirmation_is_owned_by_one_client_generation() {
        let window = std::time::Duration::from_secs(8);
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        let facts = BehaviorSwitchFacts {
            source_owned_work_active: true,
            ..BehaviorSwitchFacts::default()
        };
        let projected_reason = controller
            .assess_switch(BehaviorId::Normal, &facts, BehaviorRequestAuthority::Picker)
            .reason
            .expect("confirmation reason");

        let first = controller.decide_switch_owned(
            BehaviorId::Normal,
            facts.clone(),
            window,
            Some("client-a:1"),
        );
        assert!(matches!(
            first.outcome,
            BehaviorChangeOutcome::ConfirmationRequired { ref message, .. }
                if message == &projected_reason
        ));

        let other_client = controller.decide_switch_owned(
            BehaviorId::Normal,
            facts.clone(),
            window,
            Some("client-b:1"),
        );
        assert!(matches!(
            other_client.outcome,
            BehaviorChangeOutcome::ConfirmationRequired { .. }
        ));

        let owner_confirmation =
            controller.decide_switch_owned(BehaviorId::Normal, facts, window, Some("client-b:1"));
        assert!(matches!(
            owner_confirmation.outcome,
            BehaviorChangeOutcome::Applied
        ));
    }

    #[test]
    fn behavior_prompts_preserve_primary_orchestration_ownership() {
        assert!(clarify_reminder_template().contains("primary Agent must integrate"));
        assert!(plan_behavior_template().contains("must not replace the primary Agent's"));
        assert!(goal_reminder_template().contains("do not hand the objective itself"));
    }

    #[test]
    fn workflow_requires_bounded_jobs_and_definition_discovery() {
        let prompt = workflow_reminder_template();
        assert!(prompt.contains("Workflow behavior"));
        assert!(!prompt.contains("Dynamic Workflow"));
        assert!(prompt.contains("Do not wrap the whole request"));
        assert!(prompt.contains("Personally inspect central evidence"));
    }

    #[test]
    fn workflow_prompt_embeds_the_rhai_authoring_reference() {
        let prompt = workflow_reminder_template();
        assert!(prompt.contains("Workflow Rhai 写作参考"));
        assert!(prompt.contains("let meta = #{"));
        assert!(prompt.contains("parallel([opts1, opts2, ...])"));
        assert!(prompt.contains("output_schema"));
        assert!(prompt.contains("capability_mode"));
    }
}
