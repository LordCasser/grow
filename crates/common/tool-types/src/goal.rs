use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical status encoded by both the checkbox and status token in a Goal task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskStatus {
    Pending,
    InProgress,
    Blocked,
    Done,
}

impl GoalTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }
}

/// Read-only task projection derived from the durable Markdown blackboard.
/// It is never persisted independently and therefore cannot diverge from the
/// document that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalTaskProjection {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: u8,
    pub status: GoalTaskStatus,
    pub summary: String,
    pub completed_descendants: u32,
    pub total_descendants: u32,
}

/// Fields the primary Agent may change without changing task structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalProgressUpdate {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<GoalTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
}
