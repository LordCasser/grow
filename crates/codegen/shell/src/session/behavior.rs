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
    ConfirmationRequired { message: String, remaining_ms: u64 },
    Rejected { message: String },
}

/// Runtime facts captured by `SessionActor` before asking the coordinator for
/// a transition. The coordinator is deliberately I/O-free: it never queries a
/// workflow, runs a model, persists a file, or touches the pager.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BehaviorSwitchFacts {
    pub unavailable_reason: Option<String>,
    pub unfinished_goal: bool,
    pub public_workflow_active: bool,
    pub source_owned_work_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorEffect {
    CancelSourceForeground(BehaviorId),
    CancelDeepResearchRun,
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

/// Serialized control projection. The coordinator does not use this enum as
/// its mutable state machine: Plan and Deep Research keep independent runtime
/// state and are combined here only for one atomic persistence payload.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BehaviorState {
    Normal,
    Clarify,
    Plan(PlanPhase),
    Workflow,
    DeepResearch { run_id: Option<String> },
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
    reminder_count: u32,
    artifact_revision: u64,
    artifact_hash: Option<String>,
}

impl Default for PlanRuntime {
    fn default() -> Self {
        Self {
            phase: PlanPhase::Drafting,
            approval_pending: false,
            reminder_count: 0,
            artifact_revision: 0,
            artifact_hash: None,
        }
    }
}

/// Pure Deep Research runtime owned by the Deep Research Behavior.
#[derive(Debug, Clone, Default)]
struct DeepResearchRuntime {
    owned_run_id: Option<String>,
}

pub struct BehaviorCoordinator {
    selected: BehaviorId,
    plan: PlanRuntime,
    deep_research: DeepResearchRuntime,
    pending_switch: Option<PendingBehaviorSwitch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingBehaviorSwitch {
    source: BehaviorId,
    target: BehaviorId,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BehaviorSnapshot {
    pub state: BehaviorState,
    pub approval_pending: bool,
    pub reminder_count: u32,
    #[serde(default)]
    pub plan_artifact_revision: u64,
    #[serde(default)]
    pub plan_artifact_hash: Option<String>,
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
            BehaviorId::DeepResearch => BehaviorState::DeepResearch { run_id: None },
            BehaviorId::Goal => BehaviorState::Goal,
        };
        Self {
            state,
            approval_pending: false,
            reminder_count: 0,
            plan_artifact_revision: 0,
            plan_artifact_hash: None,
        }
    }

    pub fn behavior(&self) -> BehaviorId {
        match &self.state {
            BehaviorState::Normal => BehaviorId::Normal,
            BehaviorState::Clarify => BehaviorId::Clarify,
            BehaviorState::Plan(_) => BehaviorId::Plan,
            BehaviorState::Workflow => BehaviorId::Workflow,
            BehaviorState::DeepResearch { .. } => BehaviorId::DeepResearch,
            BehaviorState::Goal => BehaviorId::Goal,
        }
    }

    /// Reject cross-runtime residue instead of reviving state that could not
    /// have been emitted by the current coordinator. Plan transport/artifact
    /// fields are meaningful only while Plan is selected.
    pub fn runtime_fields_match_selection(&self) -> bool {
        matches!(self.state, BehaviorState::Plan(_))
            || (!self.approval_pending
                && self.reminder_count == 0
                && self.plan_artifact_revision == 0
                && self.plan_artifact_hash.is_none())
    }
}

impl BehaviorCoordinator {
    pub fn new() -> Self {
        Self {
            selected: BehaviorId::Normal,
            plan: PlanRuntime::default(),
            deep_research: DeepResearchRuntime::default(),
            pending_switch: None,
        }
    }

    pub fn from_snapshot(snapshot: BehaviorSnapshot) -> Self {
        let (selected, plan_phase, owned_run_id) = match snapshot.state {
            BehaviorState::Normal => (BehaviorId::Normal, PlanPhase::Drafting, None),
            BehaviorState::Clarify => (BehaviorId::Clarify, PlanPhase::Drafting, None),
            BehaviorState::Plan(phase) => (BehaviorId::Plan, phase, None),
            BehaviorState::Workflow => (BehaviorId::Workflow, PlanPhase::Drafting, None),
            BehaviorState::DeepResearch { run_id } => {
                (BehaviorId::DeepResearch, PlanPhase::Drafting, run_id)
            }
            BehaviorState::Goal => (BehaviorId::Goal, PlanPhase::Drafting, None),
        };
        Self {
            selected,
            plan: PlanRuntime {
                phase: plan_phase,
                approval_pending: snapshot.approval_pending,
                reminder_count: snapshot.reminder_count,
                artifact_revision: snapshot.plan_artifact_revision,
                artifact_hash: snapshot.plan_artifact_hash,
            },
            deep_research: DeepResearchRuntime { owned_run_id },
            pending_switch: None,
        }
    }

    pub fn snapshot(&self) -> BehaviorSnapshot {
        BehaviorSnapshot {
            state: self.state(),
            approval_pending: self.plan.approval_pending,
            reminder_count: self.plan.reminder_count,
            plan_artifact_revision: self.plan.artifact_revision,
            plan_artifact_hash: self.plan.artifact_hash.clone(),
        }
    }

    pub fn state(&self) -> BehaviorState {
        match self.selected {
            BehaviorId::Normal => BehaviorState::Normal,
            BehaviorId::Clarify => BehaviorState::Clarify,
            BehaviorId::Plan => BehaviorState::Plan(self.plan.phase),
            BehaviorId::Workflow => BehaviorState::Workflow,
            BehaviorId::DeepResearch => BehaviorState::DeepResearch {
                run_id: self.deep_research.owned_run_id.clone(),
            },
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
        self.deep_research = DeepResearchRuntime::default();
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
        let source = self.behavior();
        if source == target {
            self.clear_pending_switch();
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::Applied,
                effects: Vec::new(),
            };
        }
        let availability = self.switch_availability(target, &facts);
        if availability.disposition == BehaviorAvailabilityDisposition::Unavailable {
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::Rejected {
                    message: availability.reason.unwrap_or_else(|| {
                        format!("{} behavior is unavailable.", target.display_label())
                    }),
                },
                effects: Vec::new(),
            };
        }

        if facts.source_owned_work_active
            && !self.confirm_interrupting_switch(target, confirmation_window)
        {
            let remaining_ms = self
                .pending_switch()
                .map(|(_, _, remaining)| remaining)
                .unwrap_or(confirmation_window.as_millis() as u64);
            let message = if source == BehaviorId::Workflow {
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
            return BehaviorDecision {
                outcome: BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                },
                effects: Vec::new(),
            };
        }

        let mut effects = Vec::new();
        if facts.source_owned_work_active {
            effects.push(BehaviorEffect::CancelSourceForeground(source));
            if source == BehaviorId::DeepResearch {
                effects.push(BehaviorEffect::CancelDeepResearchRun);
            }
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
        let supported = facts.unavailable_reason.is_none();
        let source = self.behavior();
        if source == target {
            return BehaviorAvailabilityEntry {
                behavior: target,
                supported: true,
                disposition: BehaviorAvailabilityDisposition::Available,
                reason: None,
            };
        }
        let reason = if facts.unfinished_goal && target != BehaviorId::Goal {
            Some("Goal behavior is exclusive until the Goal completes or is cleared.".to_string())
        } else if let Some(reason) = facts.unavailable_reason.clone() {
            Some(reason)
        } else if target.owns_special_runtime() && facts.public_workflow_active {
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
                    "Leaving Workflow while an active public Run continues requires confirmation; re-enter Workflow to manage it."
                )
            } else {
                format!(
                    "Switching to {} will interrupt active {} work and requires confirmation.",
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
        let now = std::time::Instant::now();
        let source = self.behavior();
        let confirmed = self.pending_switch.is_some_and(|pending| {
            pending.source == source && pending.target == target && pending.expires_at > now
        });
        if confirmed {
            self.pending_switch = None;
            true
        } else {
            self.pending_switch = Some(PendingBehaviorSwitch {
                source,
                target,
                expires_at: now + window,
            });
            false
        }
    }

    pub fn pending_switch(&self) -> Option<(BehaviorId, BehaviorId, u64)> {
        let pending = self.pending_switch?;
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

    pub fn deep_research_run_id(&self) -> Option<&str> {
        if self.selected == BehaviorId::DeepResearch {
            self.deep_research.owned_run_id.as_deref()
        } else {
            None
        }
    }

    pub fn attach_deep_research_run(&mut self, run_id: String) -> bool {
        if self.selected != BehaviorId::DeepResearch || self.deep_research.owned_run_id.is_some() {
            return false;
        }
        self.deep_research.owned_run_id = Some(run_id);
        true
    }

    pub fn clear_deep_research_run(&mut self) -> Option<String> {
        if self.selected != BehaviorId::DeepResearch {
            return None;
        }
        self.deep_research.owned_run_id.take()
    }

    pub fn submit_initial_plan(&mut self) -> bool {
        if !self.is_plan() || self.plan.phase != PlanPhase::Drafting {
            return false;
        }
        self.plan.phase = PlanPhase::AwaitingApproval;
        self.plan.approval_pending = true;
        true
    }

    pub fn submit_plan_amendment(&mut self) -> bool {
        if !self.is_plan() || !matches!(self.plan.phase, PlanPhase::Executing | PlanPhase::Amending)
        {
            return false;
        }
        self.plan.phase = PlanPhase::Amending;
        self.plan.approval_pending = true;
        true
    }

    pub fn approve_submitted_plan(&mut self) -> bool {
        if !self.is_plan()
            || !matches!(
                self.plan.phase,
                PlanPhase::AwaitingApproval | PlanPhase::Amending
            )
        {
            return false;
        }
        self.plan.phase = PlanPhase::Executing;
        self.plan.approval_pending = false;
        self.plan.reminder_count = 0;
        true
    }

    pub fn reject_submitted_plan(&mut self) -> bool {
        self.plan.approval_pending = false;
        if !self.is_plan() {
            return false;
        }
        match self.plan.phase {
            PlanPhase::AwaitingApproval => {
                self.plan.phase = PlanPhase::Drafting;
                self.plan.reminder_count = 0;
                true
            }
            PlanPhase::Amending => true,
            _ => false,
        }
    }

    pub fn finish_plan(&mut self) -> bool {
        if !self.is_plan() {
            return false;
        }
        self.selected = BehaviorId::Normal;
        self.plan = PlanRuntime::default();
        true
    }

    pub fn set_approval_pending(&mut self, pending: bool) {
        self.plan.approval_pending = pending;
    }

    pub fn approval_pending(&self) -> bool {
        self.plan.approval_pending
    }

    pub fn record_plan_artifact(&mut self, markdown: &str) {
        self.plan.artifact_revision = self.plan.artifact_revision.saturating_add(1);
        self.plan.artifact_hash = Some(blake3::hash(markdown.as_bytes()).to_hex().to_string());
    }

    pub fn plan_artifact_hash(&self) -> Option<&str> {
        self.plan.artifact_hash.as_deref()
    }

    pub fn plan_artifact_ref(&self) -> Option<String> {
        self.plan
            .artifact_hash
            .as_deref()
            .map(|hash| format!("artifact:plan:blake3:{hash}"))
    }

    pub fn plan_artifact_is_valid(&self, markdown: &str) -> bool {
        self.plan.artifact_revision > 0
            && self.plan.artifact_hash.as_deref()
                == Some(blake3::hash(markdown.as_bytes()).to_hex().as_str())
    }

    pub fn should_use_full_reminder(&self) -> bool {
        self.plan.reminder_count.is_multiple_of(2)
    }

    pub fn record_reminder_injected(&mut self) {
        self.plan.reminder_count += 1;
    }

    pub fn reset_after_compaction(&mut self) {
        if self.is_plan() {
            self.plan.reminder_count = 0;
        }
    }
}

pub fn plan_mode_reminder_full_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/full.md")
}

pub fn plan_behavior_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/base.md")
}

pub fn plan_mode_reminder_sparse_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/sparse.md")
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

pub fn deep_research_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/deep-research.md")
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
            "Normal Behavior is now active. Earlier special Behavior protocols are historical and no longer apply. Follow the active Agent role and the current user request without Clarify, Plan, Workflow, Deep Research, or Goal-specific constraints."
        }
        BehaviorId::Clarify => clarify_reminder_template(),
        BehaviorId::Plan => plan_behavior_template(),
        BehaviorId::Workflow => workflow_reminder_template(),
        BehaviorId::DeepResearch => deep_research_reminder_template(),
        BehaviorId::Goal => goal_reminder_template(),
    };
    let escaped = instructions.replace("</behavior-context>", "<\\/behavior-context>");
    format!("<behavior-context>\n{escaped}\n</behavior-context>")
}

pub fn plan_execution_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/executing.md")
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
            BehaviorId::DeepResearch,
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
            BehaviorId::DeepResearch,
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
        controller.submit_initial_plan();
        controller.approve_submitted_plan();
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
    fn behavior_selection_and_owned_runtimes_do_not_share_state() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Plan);
        controller.record_plan_artifact("# plan");
        controller.submit_initial_plan();
        assert!(controller.select_behavior(BehaviorId::DeepResearch));
        assert!(controller.attach_deep_research_run("research-run".into()));

        let snapshot = controller.snapshot();
        assert_eq!(
            snapshot.state,
            BehaviorState::DeepResearch {
                run_id: Some("research-run".into()),
            }
        );
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
            reminder_count: 1,
            plan_artifact_revision: 1,
            plan_artifact_hash: Some("stale".into()),
        };
        assert!(!snapshot.runtime_fields_match_selection());
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
    fn deep_research_run_is_owned_only_by_deep_research_behavior() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::DeepResearch);
        assert!(controller.attach_deep_research_run("research-run".into()));
        assert_eq!(controller.deep_research_run_id(), Some("research-run"));
        controller.select_behavior(BehaviorId::Workflow);
        assert_eq!(controller.deep_research_run_id(), None);
    }

    #[test]
    fn unfinished_goal_rejects_every_other_behavior_and_reselect_is_idempotent() {
        let mut controller = controller();
        controller.select_behavior(BehaviorId::Goal);
        for target in [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::DeepResearch,
        ] {
            let decision = controller.decide_switch(
                target,
                BehaviorSwitchFacts {
                    unfinished_goal: true,
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
                unfinished_goal: true,
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
    fn public_workflow_blocks_only_special_runtime_behaviors() {
        for target in [BehaviorId::Plan, BehaviorId::Goal, BehaviorId::DeepResearch] {
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
        for target in [BehaviorId::Plan, BehaviorId::Goal, BehaviorId::DeepResearch] {
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
            BehaviorId::DeepResearch,
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
    fn behavior_prompts_preserve_primary_orchestration_ownership() {
        assert!(clarify_reminder_template().contains("primary Agent must integrate"));
        assert!(plan_behavior_template().contains("must not replace the primary Agent's"));
        assert!(goal_reminder_template().contains("do not hand the objective itself"));
        assert!(deep_research_reminder_template().contains("no worker's output is a final"));
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
