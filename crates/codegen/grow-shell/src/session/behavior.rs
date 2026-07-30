//! Primary-Agent Behavior state machine.
//!
//! Behavior is one mutually exclusive, session-scoped collaboration protocol.
//! It does not select an Agent role, grant tools, or own Workflow/Goal runtime
//! state. The controller is synchronous and contains no SessionActor or I/O.

use std::path::{Path, PathBuf};

use xai_tool_types::BehaviorId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviorChangeOutcome {
    Applied,
    ConfirmationRequired { message: String, remaining_ms: u64 },
    Rejected { message: String },
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BehaviorState {
    Normal,
    Clarify,
    Plan(PlanPhase),
    Workflow,
    DeepResearch { run_id: Option<String> },
    Goal,
}

pub struct BehaviorController {
    state: BehaviorState,
    /// A reverse approval request is parked and must be restored after a
    /// reconnect. This is transport state for a Plan phase, not another phase.
    approval_pending: bool,
    reminder_count: u32,
    plan_file_path: PathBuf,
    approved_plan_file_path: PathBuf,
    pending_switch: Option<PendingBehaviorSwitch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingBehaviorSwitch {
    source: Option<BehaviorId>,
    target: Option<BehaviorId>,
    expires_at: std::time::Instant,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BehaviorSnapshot {
    pub state: BehaviorState,
    pub approval_pending: bool,
    pub reminder_count: u32,
}

impl BehaviorController {
    pub fn new(session_dir: PathBuf) -> Self {
        Self {
            state: BehaviorState::Normal,
            approval_pending: false,
            reminder_count: 0,
            plan_file_path: session_dir.join("plan.md"),
            approved_plan_file_path: session_dir.join("approved_plan.md"),
            pending_switch: None,
        }
    }

    pub fn from_snapshot(session_dir: PathBuf, snapshot: BehaviorSnapshot) -> Self {
        Self {
            state: snapshot.state,
            approval_pending: snapshot.approval_pending,
            reminder_count: snapshot.reminder_count,
            plan_file_path: session_dir.join("plan.md"),
            approved_plan_file_path: session_dir.join("approved_plan.md"),
            pending_switch: None,
        }
    }

    pub fn snapshot(&self) -> BehaviorSnapshot {
        BehaviorSnapshot {
            state: self.state.clone(),
            approval_pending: self.approval_pending,
            reminder_count: self.reminder_count,
        }
    }

    pub fn state(&self) -> BehaviorState {
        self.state.clone()
    }

    pub fn behavior(&self) -> Option<BehaviorId> {
        match &self.state {
            BehaviorState::Normal => None,
            BehaviorState::Clarify => Some(BehaviorId::Clarify),
            BehaviorState::Plan(_) => Some(BehaviorId::Plan),
            BehaviorState::Workflow => Some(BehaviorId::Workflow),
            BehaviorState::DeepResearch { .. } => Some(BehaviorId::DeepResearch),
            BehaviorState::Goal => Some(BehaviorId::Goal),
        }
    }

    pub fn select_behavior(&mut self, behavior: Option<BehaviorId>) -> bool {
        let next = match behavior {
            None => BehaviorState::Normal,
            Some(BehaviorId::Clarify) => BehaviorState::Clarify,
            Some(BehaviorId::Plan) => BehaviorState::Plan(PlanPhase::Drafting),
            Some(BehaviorId::Workflow) => BehaviorState::Workflow,
            Some(BehaviorId::DeepResearch) => BehaviorState::DeepResearch { run_id: None },
            Some(BehaviorId::Goal) => BehaviorState::Goal,
        };
        if self.state == next {
            return false;
        }
        self.state = next;
        self.approval_pending = false;
        self.reminder_count = 0;
        self.pending_switch = None;
        true
    }

    pub fn confirm_interrupting_switch(
        &mut self,
        target: Option<BehaviorId>,
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

    pub fn pending_switch(&self) -> Option<(Option<BehaviorId>, Option<BehaviorId>, u64)> {
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
        matches!(self.state, BehaviorState::Plan(_))
    }

    pub fn is_drafting_plan(&self) -> bool {
        self.state == BehaviorState::Plan(PlanPhase::Drafting)
    }

    pub fn plan_allows_edits(&self) -> bool {
        self.state == BehaviorState::Plan(PlanPhase::Executing)
    }

    pub fn plan_phase_label(&self) -> Option<&'static str> {
        match self.state {
            BehaviorState::Plan(PlanPhase::Drafting) => Some("drafting"),
            BehaviorState::Plan(PlanPhase::AwaitingApproval) => Some("awaiting_approval"),
            BehaviorState::Plan(PlanPhase::Executing) => Some("executing"),
            BehaviorState::Plan(PlanPhase::Amending) => Some("amending"),
            _ => None,
        }
    }

    pub fn deep_research_run_id(&self) -> Option<&str> {
        match &self.state {
            BehaviorState::DeepResearch { run_id } => run_id.as_deref(),
            _ => None,
        }
    }

    pub fn attach_deep_research_run(&mut self, run_id: String) -> bool {
        match &mut self.state {
            BehaviorState::DeepResearch { run_id: slot } if slot.is_none() => {
                *slot = Some(run_id);
                true
            }
            _ => false,
        }
    }

    pub fn clear_deep_research_run(&mut self) -> Option<String> {
        match &mut self.state {
            BehaviorState::DeepResearch { run_id } => run_id.take(),
            _ => None,
        }
    }

    pub fn submit_initial_plan(&mut self) -> bool {
        if self.state != BehaviorState::Plan(PlanPhase::Drafting) {
            return false;
        }
        self.state = BehaviorState::Plan(PlanPhase::AwaitingApproval);
        self.approval_pending = true;
        true
    }

    pub fn submit_plan_amendment(&mut self) -> bool {
        if !matches!(
            self.state,
            BehaviorState::Plan(PlanPhase::Executing | PlanPhase::Amending)
        ) {
            return false;
        }
        self.state = BehaviorState::Plan(PlanPhase::Amending);
        self.approval_pending = true;
        true
    }

    pub fn approve_submitted_plan(&mut self) -> bool {
        if !matches!(
            self.state,
            BehaviorState::Plan(PlanPhase::AwaitingApproval | PlanPhase::Amending)
        ) {
            return false;
        }
        self.state = BehaviorState::Plan(PlanPhase::Executing);
        self.approval_pending = false;
        self.reminder_count = 0;
        true
    }

    pub fn reject_submitted_plan(&mut self) -> bool {
        self.approval_pending = false;
        match self.state {
            BehaviorState::Plan(PlanPhase::AwaitingApproval) => {
                self.state = BehaviorState::Plan(PlanPhase::Drafting);
                self.reminder_count = 0;
                true
            }
            BehaviorState::Plan(PlanPhase::Amending) => true,
            _ => false,
        }
    }

    pub fn finish_plan(&mut self) -> bool {
        if !self.is_plan() {
            return false;
        }
        self.state = BehaviorState::Normal;
        self.approval_pending = false;
        self.reminder_count = 0;
        true
    }

    pub fn set_approval_pending(&mut self, pending: bool) {
        self.approval_pending = pending;
    }

    pub fn approval_pending(&self) -> bool {
        self.approval_pending
    }

    pub fn plan_file_path(&self) -> &Path {
        &self.plan_file_path
    }

    pub fn approved_plan_file_path(&self) -> &Path {
        &self.approved_plan_file_path
    }

    pub fn should_use_full_reminder(&self) -> bool {
        self.reminder_count.is_multiple_of(2)
    }

    pub fn record_reminder_injected(&mut self) {
        self.reminder_count += 1;
    }

    pub fn reset_after_compaction(&mut self) {
        if self.is_plan() {
            self.reminder_count = 0;
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
    include_str!("../../prompts/behaviors/workflow.md")
}

pub fn deep_research_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/deep-research.md")
}

pub fn goal_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/goal.md")
}

pub fn plan_execution_reminder_template() -> &'static str {
    include_str!("../../prompts/behaviors/plan/executing.md")
}

pub(crate) async fn plan_file_has_content(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, strum::Display,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PromptMode {
    #[default]
    Agent,
    Ask,
    Plan,
    Workflow,
    DeepResearch,
    Goal,
}

impl PromptMode {
    pub fn from_meta_str(value: &str) -> Self {
        match value {
            "ask" => Self::Ask,
            "plan" => Self::Plan,
            "workflow" => Self::Workflow,
            "deep_research" => Self::DeepResearch,
            "goal" => Self::Goal,
            _ => Self::Agent,
        }
    }

    pub fn behavior(self) -> Option<BehaviorId> {
        match self {
            Self::Agent => None,
            Self::Ask => Some(BehaviorId::Clarify),
            Self::Plan => Some(BehaviorId::Plan),
            Self::Workflow => Some(BehaviorId::Workflow),
            Self::DeepResearch => Some(BehaviorId::DeepResearch),
            Self::Goal => Some(BehaviorId::Goal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> BehaviorController {
        BehaviorController::new(PathBuf::from("/tmp/grow-behavior-test"))
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
            assert!(controller.select_behavior(Some(behavior)));
            assert_eq!(controller.behavior(), Some(behavior));
        }
        assert!(controller.select_behavior(None));
        assert_eq!(controller.state(), BehaviorState::Normal);
    }

    #[test]
    fn only_executing_plan_allows_edits() {
        let mut controller = controller();
        controller.select_behavior(Some(BehaviorId::Plan));
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
        controller.select_behavior(Some(BehaviorId::Plan));
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
        controller.select_behavior(Some(BehaviorId::Plan));
        controller.submit_initial_plan();
        let restored = BehaviorController::from_snapshot(
            PathBuf::from("/tmp/grow-behavior-test"),
            controller.snapshot(),
        );
        assert_eq!(
            restored.state(),
            BehaviorState::Plan(PlanPhase::AwaitingApproval)
        );
        assert!(restored.approval_pending());
    }

    #[test]
    fn interrupt_confirmation_requires_same_target() {
        let mut controller = controller();
        controller.select_behavior(Some(BehaviorId::Goal));
        let window = std::time::Duration::from_secs(3);
        assert!(!controller.confirm_interrupting_switch(Some(BehaviorId::Plan), window));
        assert!(!controller.confirm_interrupting_switch(Some(BehaviorId::Workflow), window));
        assert!(controller.confirm_interrupting_switch(Some(BehaviorId::Workflow), window));
    }

    #[test]
    fn deep_research_run_is_owned_only_by_deep_research_behavior() {
        let mut controller = controller();
        controller.select_behavior(Some(BehaviorId::DeepResearch));
        assert!(controller.attach_deep_research_run("research-run".into()));
        assert_eq!(controller.deep_research_run_id(), Some("research-run"));
        controller.select_behavior(Some(BehaviorId::Workflow));
        assert_eq!(controller.deep_research_run_id(), None);
    }
}
