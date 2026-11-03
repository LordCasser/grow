//! Model-facing subagent observation contracts, separate from command tasks.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentListScope {
    #[default]
    Active,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Initializing,
    Running,
    RunningProgressUnavailable,
    Suspended,
    FinalizationPending,
    Completed,
    Failed,
    Cancelled,
    OutputUnavailable,
    ObservationUnavailable,
    ObservationTimedOut,
    ObservationInvalidEvidence,
    ObservationInterrupted,
    Unauthorized,
}
impl SubagentStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::OutputUnavailable
        )
    }
}

/// Basic inventory facts. No counters, tool-call payloads or final answer.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentSummary {
    pub subagent_id: String,
    pub direct_parent_session_id: String,
    pub subagent_type: String,
    pub description: String,
    pub status: SubagentStatus,
    pub started_at_epoch_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSubagentInput {
    /// active includes initializing, running, suspended, settling and unknown children.
    /// all also includes authorized canonical history. Neither scope enumerates commands.
    #[serde(default)]
    pub scope: SubagentListScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListSubagentOutput {
    pub scope: SubagentListScope,
    pub subagents: Vec<SubagentSummary>,
}

/// Counters are optional: no sample or canonical count must remain unknown.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SubagentStatistics {
    pub tool_call_count: Option<u32>,
    pub turn_count: Option<u32>,
    pub total_tokens_used: Option<u64>,
    pub context_tokens: Option<u64>,
    pub context_window_tokens: Option<u64>,
    pub context_usage_pct: Option<u8>,
    pub error_count: Option<u32>,
    pub tools_used: Option<Vec<String>>,
}
impl SubagentStatistics {
    pub fn terminal(tool_calls: u32, turns: u32, tokens: u64) -> Self {
        Self {
            tool_call_count: Some(tool_calls),
            turn_count: Some(turns),
            total_tokens_used: Some(tokens),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetSubagentStatusInput {
    /// Non-empty subagent IDs, at most 20. This is a non-blocking status query.
    #[schemars(length(min = 1, max = 20))]
    pub subagent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentDetails {
    pub subagent_id: String,
    pub direct_parent_session_id: Option<String>,
    pub subagent_type: Option<String>,
    pub description: Option<String>,
    pub status: SubagentStatus,
    pub started_at_epoch_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub statistics: SubagentStatistics,
    pub error: Option<String>,
}
impl SubagentDetails {
    pub fn from_summary(summary: SubagentSummary, statistics: SubagentStatistics) -> Self {
        Self {
            subagent_id: summary.subagent_id,
            direct_parent_session_id: Some(summary.direct_parent_session_id),
            subagent_type: Some(summary.subagent_type),
            description: Some(summary.description),
            status: summary.status,
            started_at_epoch_ms: summary.started_at_epoch_ms,
            duration_ms: summary.duration_ms,
            statistics,
            error: summary.error,
        }
    }
    pub fn unknown(id: String, status: SubagentStatus, error: String) -> Self {
        Self {
            subagent_id: id,
            direct_parent_session_id: None,
            subagent_type: None,
            description: None,
            status,
            started_at_epoch_ms: None,
            duration_ms: None,
            statistics: Default::default(),
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetSubagentStatusOutput {
    pub subagents: Vec<SubagentDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitSubagentInput {
    /// Non-empty subagent IDs, at most 20. Already finished children return immediately.
    #[schemars(length(min = 1, max = 20))]
    pub subagent_ids: Vec<String>,
    /// Positive total call budget in milliseconds, default 30000; capped at {max_wait_ms}.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentWaitReason {
    AllFinished,
    DeadlineElapsed,
    ObservationFailed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubagentWaitResult {
    pub subagent_id: String,
    pub status: SubagentStatus,
    pub started_at_epoch_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Verified final answer when available. No answer is inferred from a status sample.
    pub output: Option<String>,
    pub error: Option<String>,
}
impl SubagentWaitResult {
    pub fn unknown(id: String, status: SubagentStatus, error: String) -> Self {
        Self {
            subagent_id: id,
            status,
            started_at_epoch_ms: None,
            duration_ms: None,
            output: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitSubagentOutput {
    pub reason: SubagentWaitReason,
    /// Actual elapsed time; independent of child execution duration and configured timeout.
    pub waited_ms: u64,
    pub subagents: Vec<SubagentWaitResult>,
}
