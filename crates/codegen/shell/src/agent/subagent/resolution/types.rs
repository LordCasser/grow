//! Public API types for subagent resolution.

use std::path::PathBuf;

/// Resolved effective runtime configuration for a child agent.
///
/// Explicit spawn-time runtime values are resolved here; Agent-definition
/// defaults and parent constraints are applied by the spawn orchestrator.
#[derive(Debug, Clone, Default)]
pub struct EffectiveRuntimeConfig {
    /// Resolved model ID override (if any).
    pub model: Option<String>,
    /// Resolved reasoning effort (e.g. "low", "medium", "high").
    // TODO(phase2): consider a typed `ReasoningEffort` enum to prevent typos.
    // Currently stringly-typed for compatibility with the shell's existing API.
    pub reasoning_effort: Option<String>,
    /// Resolved capability mode controlling tool access.
    pub capability_mode: Option<tool_types::SubagentCapabilityMode>,
    /// Isolation mode for the child execution environment.
    pub isolation: tool_types::SubagentIsolationMode,
}

/// Data about a completed source subagent, needed for resume validation
/// and downstream spawn orchestration.
#[derive(Debug, Clone)]
pub struct ResumeSourceData {
    /// Source subagent ID.
    pub subagent_id: String,
    /// Source subagent type (e.g. "general-purpose", "explore").
    /// Used by `validate_resume_identity` to check type match.
    pub subagent_type: String,
    /// Effective model ID used by the source child session.
    /// Used by the shell for resume model pinning (model overrides on
    /// resume are soft-ignored, not identity-gated).
    pub model_id: Option<String>,
    /// Effective cwd the source child used. Consumed by the shell's
    /// spawn orchestration for raw transcript continuation and worktree reuse.
    pub child_cwd: String,
    /// Worktree path if the source used `isolation=worktree`. Consumed
    /// by the shell to reuse the source's isolated workspace directory
    /// when resuming a worktree-isolated child.
    pub worktree_path: Option<PathBuf>,
    /// Durable git ref holding a snapshot of the source worktree's working
    /// state, set when the worktree was snapshotted at completion. Consumed
    /// by the shell to rehydrate a deleted worktree directory on resume.
    pub snapshot_ref: Option<String>,
    /// The child session ID of the source subagent. Consumed by the
    /// shell's entity resolver to locate and validate the source Timeline.
    pub child_session_id: String,
}

/// Errors that can occur during subagent resolution.
#[derive(Debug, thiserror::Error)]
pub enum ResolutionError {
    /// No production or session CLI definition has this name.
    #[error("unknown subagent type \"{subagent_type}\"; available: {available:?}")]
    Unknown {
        subagent_type: String,
        available: Vec<String>,
    },

    /// The definition exists but is disabled by the session toggle.
    #[error("subagent \"{subagent_type}\" is disabled")]
    Disabled { subagent_type: String },
}
#[cfg(test)]
mod tests {
    use super::*;
    use tool_types::SubagentIsolationMode;

    #[test]
    fn effective_runtime_config_default_values() {
        let config = EffectiveRuntimeConfig::default();
        assert!(config.model.is_none());
        assert!(config.reasoning_effort.is_none());
        assert!(config.capability_mode.is_none());
        assert_eq!(config.isolation, SubagentIsolationMode::None);
    }
}
