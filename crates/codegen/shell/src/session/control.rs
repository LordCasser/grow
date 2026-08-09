//! Atomic session control-plane snapshot.
//!
//! This is the only persisted source for Behavior selection and the Goal
//! runtime. Runtime leases, cancellation handles, activity projections and UI
//! clocks are intentionally absent and are reconstructed after reload.

pub const SESSION_CONTROL_ARCHITECTURE_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionControlSnapshot {
    pub architecture_version: u32,
    pub control_revision: u64,
    pub behavior: crate::session::behavior::BehaviorSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<crate::session::goal_tracker::GoalOrchestration>,
}

impl SessionControlSnapshot {
    pub fn new(
        control_revision: u64,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalOrchestration>,
    ) -> Self {
        Self {
            architecture_version: SESSION_CONTROL_ARCHITECTURE_VERSION,
            control_revision,
            behavior,
            goal,
        }
    }

    pub fn architecture_is_current(&self) -> bool {
        self.architecture_version == SESSION_CONTROL_ARCHITECTURE_VERSION
    }
}
