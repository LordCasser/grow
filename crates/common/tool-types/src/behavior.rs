use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The user-facing protocol a primary Agent follows to advance a goal.
///
/// Behaviors are session state. They do not select an Agent role, grant tools,
/// or propagate to delegated child Agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorId {
    Clarify,
    Plan,
    Workflow,
    DeepResearch,
    Goal,
}
