//! SessionActor command boundary for local planning.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tool_types::{DeliveryReview, LocalPlan, PlanWait};

use super::TodoWriteInput;

/// Typed operations sent to the owning session actor.
pub enum PlanningCommand {
    Write {
        prompt_id: String,
        input: TodoWriteInput,
        respond_to: oneshot::Sender<Result<LocalPlan, String>>,
    },
    Link {
        prompt_id: String,
        step_id: String,
        task_id: String,
        required: bool,
        respond_to: oneshot::Sender<Result<LocalPlan, String>>,
    },
    Undispatched {
        task_id: String,
        reason: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
}

impl std::fmt::Debug for PlanningCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Write {
                prompt_id, input, ..
            } => f
                .debug_struct("Write")
                .field("prompt_id", prompt_id)
                .field("input", input)
                .finish_non_exhaustive(),
            Self::Link {
                prompt_id,
                step_id,
                task_id,
                required,
                ..
            } => f
                .debug_struct("Link")
                .field("prompt_id", prompt_id)
                .field("step_id", step_id)
                .field("task_id", task_id)
                .field("required", required)
                .finish_non_exhaustive(),
            Self::Undispatched {
                task_id, reason, ..
            } => f
                .debug_struct("Undispatched")
                .field("task_id", task_id)
                .field("reason", reason)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone)]
pub struct PlanningRuntimeHandle(pub mpsc::UnboundedSender<PlanningCommand>);

impl std::fmt::Debug for PlanningRuntimeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PlanningRuntimeHandle").finish()
    }
}

/// Read-only, ephemeral projection for surfaces that still consume TodoState.
#[derive(Debug, Clone, Default)]
pub struct LocalPlanProjection(pub Option<LocalPlan>);

/// Structured side effects applied atomically with a plan write by the owner.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanningAction {
    Wait {
        wait: PlanWait,
    },
    ClearWait {
        id: String,
    },
    HandleReplans {
        ids: Vec<String>,
        no_change_reason: Option<String>,
    },
    Review {
        review: DeliveryReview,
    },
    SelectScope {
        scope_id: String,
    },
}
