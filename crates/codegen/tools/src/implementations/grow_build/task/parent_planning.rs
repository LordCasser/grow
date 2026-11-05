//! Host-only queries for the plan owned by a caller's direct parent.

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tool_types::planning::PlanningReadSnapshot;

/// Read failures remain distinct from a parent that has no local plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParentPlanningReadError {
    #[error("This session has no direct parent")]
    NoParent,
    #[error("The direct parent's planning owner is unavailable")]
    Unavailable,
    #[error("The direct-parent planning query timed out")]
    TimedOut,
    #[error("The direct-parent planning evidence is invalid")]
    InvalidEvidence,
}

impl ParentPlanningReadError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoParent => "no_parent",
            Self::Unavailable => "unavailable",
            Self::TimedOut => "timed_out",
            Self::InvalidEvidence => "invalid_evidence",
        }
    }
}

/// Caller identity comes exclusively from the session-bound backend. The
/// coordinator resolves the immediate parent and this execution's task/step.
pub struct ParentPlanningRequest {
    pub source_session_id: String,
    pub cancellation: CancellationToken,
    pub respond_to: oneshot::Sender<Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>>,
}
