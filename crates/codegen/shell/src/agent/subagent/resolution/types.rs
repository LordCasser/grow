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
    /// Resolved reasoning policy. The outer value records an explicit
    /// spawn-time/definition policy; its inner `None` explicitly disables
    /// reasoning instead of inheriting the selected model default.
    // TODO(phase2): consider a typed `ReasoningEffort` enum to prevent typos.
    // The sampling/model boundary currently exposes this value as a string.
    pub reasoning_effort: Option<Option<String>>,
    /// Fully resolved initial capability request. Agent-definition and global
    /// defaults have already been applied; parent confinement is the only
    /// remaining transformation.
    pub capability_mode: tool_types::SubagentCapabilityMode,
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
    /// Effective model ID used by the source child session. Resume ignores a
    /// caller override and pins this exact catalogue entry.
    pub model_id: String,
    /// Exact secret-free provider/model/endpoint identity used by the source.
    /// Resume fails closed if the catalogue ID now resolves elsewhere.
    pub model_transport_key: sampling_types::ModelImageInputKey,
    /// Effective reasoning policy used by the source child. Resume preserves
    /// this together with the model instead of inheriting a later parent or
    /// Workflow route.
    pub reasoning_effort: Option<sampling_types::ReasoningEffort>,
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
        assert_eq!(
            config.capability_mode,
            tool_types::SubagentCapabilityMode::ReadWrite
        );
        assert_eq!(config.isolation, SubagentIsolationMode::None);
    }
}
