//! Data and channel types for subagent coordination.
//!
//! Request data is deliberately separate from command reply envelopes. The
//! shared coordinator actor owns every reply sender and every lifecycle
//! transition; child runners receive only plain request data.
//!
//! ## Resource types
//!
//! The primary resource injected into every session's `Resources`:
//!
//! - `SubagentBackendResource` — wraps an `Arc<dyn SubagentBackend>` that
//!   abstracts spawn/query/cancel (see [`super::backend`])
//! - `SubagentDepthCounter` — current nesting depth
//! - `MaxSubagentDepth` — configured max nesting depth
//! - `SessionIdResource` — carries the current session ID for parent scoping
//! - `TaskModelValidator` — validates explicit model slugs before background spawn
//!
//! All coordinator messages are funnelled through a single
//! `SubagentEventSender` / `SubagentEvent` enum channel.

use std::sync::Arc;

use educe::Educe;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tool_types::{SubagentCapabilityMode, SubagentIsolationMode};

use crate::register_resource;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SubagentOwner {
    #[default]
    Task,
    Goal {
        goal_id: String,
    },
    Workflow {
        run_id: String,
    },
}

impl SubagentOwner {
    pub fn goal(goal_id: impl Into<String>) -> Self {
        Self::Goal {
            goal_id: goal_id.into(),
        }
    }

    pub fn workflow(run_id: impl Into<String>) -> Self {
        Self::Workflow {
            run_id: run_id.into(),
        }
    }

    pub fn workflow_run_id(&self) -> Option<&str> {
        match self {
            Self::Task | Self::Goal { .. } => None,
            Self::Workflow { run_id } => Some(run_id),
        }
    }

    pub fn goal_id(&self) -> Option<&str> {
        match self {
            Self::Goal { goal_id, .. } => Some(goal_id),
            Self::Task | Self::Workflow { .. } => None,
        }
    }

    pub fn is_workflow(&self) -> bool {
        matches!(self, Self::Workflow { .. })
    }
}

// Request / Response

/// Plain spawn request emitted by `TaskTool`.
#[derive(Debug, Clone)]
pub struct SubagentRequest {
    /// Subagent ID (UUID v7). Same as `TaskToolInput.task_id`; becomes the child session ID.
    pub id: String,
    pub prompt: String,
    pub description: String,
    pub subagent_type: String,
    pub parent_session_id: String,
    /// Parent turn/prompt ID that launched this subagent.
    ///
    /// Used to cancel only the subagents spawned by the currently-cancelled turn,
    /// without affecting background subagents from earlier turns.
    pub parent_prompt_id: Option<String>,
    /// Resume from a previously completed subagent's conversation.
    /// Inherits the canonical raw transcript and model. System prompt, live
    /// grants, and tool runtime are freshly constructed.
    pub resume_from: Option<String>,
    /// Explicit working directory for the child session.
    /// Validated at spawn time by the injected child runner.
    pub cwd: Option<String>,
    /// Runtime overrides for the child agent.
    pub runtime_overrides: SubagentRuntimeOverrides,
    /// Whether this subagent was launched with `run_in_background: true`.
    ///
    /// Controls immediate handle delivery and completion surfacing. A
    /// background child still auto-surfaces its completion to the model
    /// (durable completion notification) when `surface_completion` is set —
    /// background does not mean fire-and-forget. Prompt cancellation still
    /// cancels every child owned by that prompt.
    pub run_in_background: bool,
    /// When false, the subagent's completion is NOT buffered for the
    /// between-turn "idle completion" reminder — used by harness-internal
    /// subagents like the goal planner/classifier that the model must never see.
    pub surface_completion: bool,
    pub await_to_completion: bool,
    /// Harness-only: seed child with normalized parent conversation, then append
    /// `prompt`. Not on TaskToolInput. Successful `resume_from` takes precedence.
    pub fork_context: bool,
    pub owner: SubagentOwner,
    /// Immutable Goal snapshot available to a Goal-owned child through
    /// `get_goal`. It is captured at spawn and never follows later edits.
    pub goal_context: Option<crate::implementations::grow_build::update_goal::GoalContextSnapshot>,
    pub cancel_token: CancellationToken,
}

/// Spawn command envelope owned by the coordinator mailbox.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentSpawnRequest {
    pub request: Box<SubagentRequest>,
    #[educe(Debug(ignore))]
    pub result_tx: oneshot::Sender<SubagentResult>,
}

impl std::ops::Deref for SubagentSpawnRequest {
    type Target = SubagentRequest;

    fn deref(&self) -> &Self::Target {
        &self.request
    }
}

impl SubagentSpawnRequest {
    /// Build and send a reply while the plain request remains borrowable.
    ///
    /// Primarily useful for channel adapters and deterministic test harnesses;
    /// production lifecycle replies are owned by `SubagentCoordinator`.
    pub fn respond_with(
        self,
        build: impl FnOnce(&SubagentRequest) -> SubagentResult,
    ) -> Result<(), SubagentResult> {
        let result = build(&self.request);
        self.result_tx.send(result)
    }
}

/// Per-spawn dynamic runtime overrides for a subagent.
///
/// Optional values inherit from the parent or role default. Explicit values take
/// precedence over role defaults.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModelOverrideProvenance {
    /// Internal harness, role, or config resolution.
    #[default]
    Harness,
    /// A model-facing `Task.model` argument.
    Tool,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentRuntimeOverrides {
    /// Override the model (e.g. "test-model").
    pub model: Option<String>,
    /// Expected secret-free identity of the resolved model transport.
    ///
    /// Harness-owned long-lived executions use this to reject a catalogue
    /// entry that kept the same public model ID while changing provider,
    /// endpoint, or underlying model. Ordinary Task calls leave it unset.
    pub model_transport_key: Option<crate::types::resources::ModelImageInputKey>,
    /// Whether `model` came from a model-facing Task call or internal harness logic.
    pub model_override_provenance: ModelOverrideProvenance,
    /// Override the reasoning policy.
    ///
    /// The outer `Option` records whether the caller supplied a policy. The
    /// inner value is the policy itself: `Some(None)` deliberately disables
    /// reasoning instead of inheriting a later Agent/model default.
    pub reasoning_effort: Option<Option<String>>,
    /// Capability mode controlling tool access.
    pub capability_mode: Option<SubagentCapabilityMode>,
    /// Isolation mode for child execution environment.
    /// `None` means use the agent-definition default.
    pub isolation: Option<SubagentIsolationMode>,
    pub completion_output_cap: Option<usize>,
    pub spawn_depth: Option<u32>,
    pub output_token_budget: Option<u64>,
    pub output_schema: Option<serde_json::Value>,
    pub loop_task_id: Option<String>,
}

/// Re-export of [`tool_types::is_not_sentinel`] for existing call sites.
pub use tool_types::is_not_sentinel;

/// Sanitize a model-emitted `cwd` argument for the `task` tool.
///
/// Strips stray surrounding quote/backtick characters (matched or unmatched),
/// trims whitespace, expands a leading `~` to the user's home directory, and
/// rejects sentinel placeholders (`""`, `"null"`, `"none"`, `"undefined"`).
///
/// Returns `Some(cleaned)` for a usable path, `None` if the value should be
/// treated as absent. Shared by the tool layer (`task::mod`) and the
/// defense-in-depth check in `shell`'s subagent coordinator.
pub fn sanitize_cwd_value(s: &str) -> Option<String> {
    let unquoted = s.trim().trim_matches(['"', '\'', '`']);
    // Re-trim after stripping quotes: this trim flows into the returned
    // value; the trim inside `is_not_sentinel` does not.
    let cleaned = unquoted.trim();
    if !is_not_sentinel(cleaned) {
        return None;
    }
    Some(shellexpand::tilde(cleaned).into_owned())
}

/// Returns `true` if the string looks like a real subagent ID rather than a
/// model-emitted placeholder (`""`, `"null"`, `"none"`, `"undefined"`, whitespace).
pub fn is_valid_resume_id(s: &str) -> bool {
    is_not_sentinel(s)
}

/// Result returned by a completed subagent.
#[derive(Debug, Clone)]
pub struct SubagentResult {
    pub success: bool,
    /// The subagent's final output text.
    ///
    /// Stored as `Arc<str>` so cloning into completion payloads, snapshot
    /// status, and other projections is a refcount
    /// bump rather than a full copy. Subagent outputs can be arbitrarily
    /// large (entire transcript), so this matters at scale.
    pub output: Arc<str>,
    /// Error message if the subagent failed.
    pub error: Option<String>,
    /// True if the subagent was cancelled (by user or model).
    /// Distinct from failure — cancellation is intentional.
    pub cancelled: bool,
    pub subagent_id: String,
    /// The child session ID (same as subagent_id for MVP).
    pub child_session_id: String,
    pub tool_calls: u32,
    pub turns: u32,
    pub duration_ms: u64,
    pub tokens_used: u64,
    pub output_tokens_used: u64,
    pub total_tokens_used: u64,
    pub output_usage_incomplete: bool,
    /// Path to the isolated worktree if one was created.
    pub worktree_path: Option<String>,
    /// Set when a blocking subagent exceeded its await budget and was
    /// auto-backgrounded: the child is still running (result via durable notification /
    /// `get_command_or_subagent_output`), so the tool returns a `task_id` notice
    /// instead of a completion. Never set for natively backgrounded subagents.
    pub backgrounded: bool,
}

impl Default for SubagentResult {
    fn default() -> Self {
        Self {
            success: false,
            output: Arc::from(""),
            error: None,
            cancelled: false,
            subagent_id: String::new(),
            child_session_id: String::new(),
            tool_calls: 0,
            turns: 0,
            duration_ms: 0,
            tokens_used: 0,
            output_tokens_used: 0,
            total_tokens_used: 0,
            output_usage_incomplete: false,
            worktree_path: None,
            backgrounded: false,
        }
    }
}

impl SubagentResult {
    /// Terminal status string: `"cancelled"`, `"completed"`, or `"failed"`.
    pub fn status(&self) -> &'static str {
        if self.cancelled {
            "cancelled"
        } else if self.success {
            "completed"
        } else {
            "failed"
        }
    }
}

// Query protocol

/// Query sent by `TaskOutputTool` to the shared coordinator actor.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentQueryRequest {
    /// The subagent ID to look up.
    pub subagent_id: String,
    /// Restrict the lookup to children owned by this parent session.
    pub parent_session_id: Option<String>,
    /// If true, coordinator waits for completion (up to timeout) before responding.
    pub block: bool,
    /// Max wait time in ms when blocking. Default 30s.
    pub timeout_ms: Option<u64>,
    /// Oneshot for the coordinator to send back the snapshot.
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<Option<SubagentSnapshot>>,
}

#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentLoopUnitActiveRequest {
    pub task_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<bool>,
}

/// Point-in-time snapshot of a subagent's state.
/// Returned by the coordinator in response to a `SubagentQueryRequest`.
#[derive(Debug, Clone)]
pub struct SubagentSnapshot {
    pub subagent_id: String,
    pub description: String,
    pub subagent_type: String,
    pub status: SubagentSnapshotStatus,
    /// Wall-clock start time (epoch ms).
    pub started_at_epoch_ms: u64,
    /// Elapsed wall-clock time in milliseconds.
    pub duration_ms: u64,
}

/// Lifecycle metadata returned to shell presentation and extension callers.
#[derive(Debug, Clone)]
pub struct SubagentInspection {
    pub snapshot: SubagentSnapshot,
    pub parent_session_id: String,
    pub child_session_id: String,
    pub fork_parent_prompt_id: Option<String>,
    pub resumed_from: Option<String>,
}

impl SubagentSnapshot {
    /// Whether the child is still in flight (initializing or running) — the
    /// shared liveness rule every driver's blocking query loops on.
    pub fn is_running(&self) -> bool {
        matches!(
            self.status,
            SubagentSnapshotStatus::Running { .. } | SubagentSnapshotStatus::Initializing
        )
    }
}

/// Status of a subagent snapshot.
#[derive(Debug, Clone)]
pub enum SubagentSnapshotStatus {
    /// Subagent is being set up (creating worktree, resolving config, spawning
    /// session). Queries during this phase should report the subagent as
    /// initializing rather than "not found".
    Initializing,
    /// Child session is still running. Fields are populated from the child
    /// session's `SessionSignals` snapshot at query time (pull-based).
    Running {
        /// Number of completed turns so far.
        turn_count: u32,
        /// Total tool calls executed so far.
        tool_call_count: u32,
        /// Current tokens used in the context window.
        tokens_used: u64,
        /// Total context window capacity (tokens).
        context_window_tokens: u64,
        /// Context window usage as a percentage (0–100).
        context_usage_pct: u8,
        /// Distinct tool names called so far (e.g. `["bash", "read_file"]`).
        tools_used: Vec<String>,
        /// Number of errors encountered so far.
        error_count: u32,
    },
    /// Child session completed successfully.
    Completed {
        output: String,
        tool_calls: u32,
        turns: u32,
        /// Canonical total usage accumulated by the completed child.
        tokens_used: u64,
        worktree_path: Option<String>,
    },
    /// The child completed, but its content-addressed output can no longer be
    /// verified. Recovery must not convert this integrity failure into a new
    /// canonical output artifact.
    CompletedOutputUnavailable {
        error: String,
        tool_calls: u32,
        turns: u32,
        tokens_used: u64,
        worktree_path: Option<String>,
    },
    /// Child session failed or crashed.
    Failed { error: String },
    /// Child session was cancelled (by user or model).
    Cancelled { reason: Option<String> },
}

impl SubagentSnapshotStatus {
    /// Returns `true` for terminal states: `Completed`, `Failed`, `Cancelled`.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::CompletedOutputUnavailable { .. }
                | Self::Failed { .. }
                | Self::Cancelled { .. }
        )
    }
}

// Cancel protocol

#[derive(Debug, Clone)]
pub enum SubagentCancelTarget {
    SubagentId(String),
    /// Turn-scoped cancel (soft cancel / max-turns).
    ParentPromptId(String),
    /// User Stop / Esc with cancel_subagents — prior-turn background too.
    ParentSession,
    /// Revoke every descendant carrying the immutable Goal owner. The parent
    /// session scope prevents one session from cancelling another session's
    /// coincidentally equal identifier.
    GoalId(String),
    WorkflowRunId(String),
}

/// Cancel request sent by `KillTaskTool` or session cancellation paths.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentCancelRequest {
    pub parent_session_id: Option<String>,
    pub target: SubagentCancelTarget,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<SubagentCancelOutcome>,
}

#[derive(Debug, Clone)]
pub enum SubagentCancelOutcome {
    Cancelled,
    AlreadyFinished { status: String },
    NotFound,
}

/// Model-facing projection of one completed subagent. The shell serializes it
/// into a content-addressed durable notification payload.
#[derive(Debug, Clone)]
pub struct SubagentCompletionSummary {
    pub subagent_id: String,
    pub subagent_type: String,
    pub description: String,
    pub success: bool,
    pub duration_ms: u64,
    pub tool_calls: u32,
    pub turns: u32,
    /// The subagent's final output text. Refcount-shared with
    /// `SubagentResult.output` (no allocation on the path from coordinator to
    /// notification receipt).
    ///
    /// Surfaced inline in completion notifications when the parent agent's
    /// toolset has no `BackgroundTaskAction` tool. Toolsets
    /// that DO have a polling tool keep the existing metadata-only line +
    /// "Use get_task_output(...)" pointer.
    pub output: Arc<str>,
}

/// Live subagents and whether finished-subagent usage is still missing from the parent bill.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubagentOutstandingReply {
    /// Turn-blocking (foreground) children still pending or active.
    pub live_ids: Vec<String>,
    /// A background child is still running: its spend is missing from the
    /// prompt report but reaches the session ledger when it finishes.
    pub background_live: bool,
    pub subagent_usage_not_applied: bool,
}

#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentOutstandingRequest {
    pub parent_session_id: String,
    pub prompt_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<SubagentOutstandingReply>,
}

/// Clear sticky incomplete after freeze/cancel has snapshotted the bill.
#[derive(Debug)]
pub struct SubagentClearUsageNotAppliedRequest {
    pub parent_session_id: String,
    pub prompt_id: String,
}

/// Mark sticky incomplete for a parent prompt (usage apply failed).
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentMarkUsageNotAppliedRequest {
    pub parent_session_id: String,
    pub prompt_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<()>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubagentRegistryCounts {
    pub pending: usize,
    pub active: usize,
    pub completed: usize,
}

#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentRegistryCountsRequest {
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<SubagentRegistryCounts>,
}

/// Request for full metadata plus a resolved progress snapshot.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentInspectRequest {
    pub subagent_id: String,
    pub parent_session_id: Option<String>,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<Option<SubagentInspection>>,
}

/// Request for all running children owned by one parent session.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentListRunningRequest {
    pub parent_session_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<Vec<SubagentInspection>>,
}

/// Fork/resume provenance retained by the shared coordinator.
#[derive(Debug, Clone, Default)]
pub struct SubagentProvenance {
    pub fork_parent_prompt_id: Option<String>,
    pub resumed_from: Option<String>,
}

/// Reference to a child spawned during one parent prompt.
#[derive(Debug, Clone)]
pub struct SpawnedSubagentRef {
    pub subagent_id: String,
    pub child_session_id: String,
    pub subagent_type: String,
    pub description: String,
    pub resumed_from: Option<String>,
}

/// Request for prompt-scoped spawned-child references.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentSpawnedRefsRequest {
    pub parent_session_id: String,
    pub prompt_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<Vec<SpawnedSubagentRef>>,
}

// Validate-type protocol

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SubagentValidateTypeOutcome {
    Ok,
    /// `available` is sorted by `str::cmp` and filtered by `[subagents.toggle]`.
    Unknown {
        available: Vec<String>,
    },
    Disabled,
    /// Coordinator unreachable; distinct from `Unknown` (the type may be valid).
    ValidationUnavailable,
}

#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentValidateTypeRequest {
    pub subagent_type: String,
    pub parent_session_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<SubagentValidateTypeOutcome>,
}

/// Coordinator message enum. Kept exhaustive so every actor command is handled.
pub enum SubagentEvent {
    Spawn(SubagentSpawnRequest),
    Query(SubagentQueryRequest),
    Cancel(SubagentCancelRequest),
    ListActive(SubagentListActiveRequest),
    ListRunning(SubagentListRunningRequest),
    /// Cancel a closed session's children and discard its runtime admission state.
    TeardownSession {
        parent_session_id: String,
    },
    /// Re-open Task spawns for a parent session after a prior ParentSession stop.
    /// Emitted at the start of each user turn so Stop's late-spawn gate does not
    /// permanently block the next prompt.
    OpenSpawnAdmission {
        parent_session_id: String,
    },
    Outstanding(SubagentOutstandingRequest),
    ClearUsageNotApplied(SubagentClearUsageNotAppliedRequest),
    MarkUsageNotApplied(SubagentMarkUsageNotAppliedRequest),
    RegistryCounts(SubagentRegistryCountsRequest),
    Inspect(SubagentInspectRequest),
    SpawnedRefs(SubagentSpawnedRefsRequest),
    ValidateType(SubagentValidateTypeRequest),
    LoopUnitActive(SubagentLoopUnitActiveRequest),
}

// Resource types

/// One shared channel to the subagent coordinator, cloned into each session.
#[derive(Clone, Educe)]
#[educe(Debug)]
pub struct SubagentEventSender(#[educe(Debug(ignore))] pub mpsc::UnboundedSender<SubagentEvent>);

register_resource!("grow_build", "SubagentEventSender", SubagentEventSender);

// Active subagent listing (compaction)

/// Lightweight summary of a running subagent.
///
/// The shared coordinator produces this through the channel protocol, and the
/// compaction pipeline consumes it through `RunningSubagentSummary`.
#[derive(Debug, Clone)]
pub struct ActiveSubagentSummary {
    /// The subagent's unique ID (same ID used by `get_task_output` / `kill_task`).
    pub subagent_id: String,
    /// The agent type name (e.g. "Explore", "general-purpose", "Plan").
    pub subagent_type: String,
    /// Human-readable description of what the subagent is doing.
    pub description: String,
    /// Wall-clock elapsed time since the subagent was spawned, in milliseconds.
    pub elapsed_ms: u64,
}

/// Request to list currently-running subagents for a specific parent session.
///
/// Sent by the compaction pipeline in `SessionActor::run_compact_inner()`.
/// Handled by the shared coordinator actor.
#[derive(Educe)]
#[educe(Debug)]
pub struct SubagentListActiveRequest {
    pub parent_session_id: String,
    #[educe(Debug(ignore))]
    pub respond_to: oneshot::Sender<Vec<ActiveSubagentSummary>>,
}

/// Current nesting depth (top-level = 0; child = parent + 1).
#[derive(Debug, Clone)]
pub struct SubagentDepthCounter(pub u32);

register_resource!("grow_build", "SubagentDepthCounter", SubagentDepthCounter);

/// Host-injected max nesting depth; absent → [`super::MAX_SUBAGENT_DEPTH`].
#[derive(Debug, Clone, Copy)]
pub struct MaxSubagentDepth(pub u32);

register_resource!("grow_build", "MaxSubagentDepth", MaxSubagentDepth);

/// Session-scoped validator for model-facing `Task.model` arguments.
///
/// Returns an error message for an invalid slug and `None` for a valid slug.
/// The closure reads the live model catalog so refreshes apply without rebuilding
/// the tool bridge.
type TaskModelValidationFn = dyn Fn(&str) -> Option<String> + Send + Sync;

#[derive(Clone)]
pub struct TaskModelValidator(Arc<TaskModelValidationFn>);

impl TaskModelValidator {
    pub fn new(validate: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(validate))
    }

    pub fn error_for(&self, model: &str) -> Option<String> {
        (self.0)(model)
    }
}

impl std::fmt::Debug for TaskModelValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskModelValidator").finish()
    }
}

register_resource!("grow_build", "TaskModelValidator", TaskModelValidator);

/// Carries the current session ID so TaskTool can set `parent_session_id`
/// on the `SubagentRequest`.
#[derive(Debug, Clone)]
pub struct SessionIdResource(pub String);

register_resource!("grow_build", "SessionIdResource", SessionIdResource);

/// Host-owned RAII token for an interruptible foreground wait.
pub trait ForegroundWaitGuard: Send {}

impl<T: Send> ForegroundWaitGuard for T {}

type ForegroundWaitFactory = dyn Fn() -> Box<dyn ForegroundWaitGuard> + Send + Sync;

/// Factory injected by hosts that expose a send-now wait window.
#[derive(Clone)]
pub struct SubagentForegroundWait(Arc<ForegroundWaitFactory>);

impl SubagentForegroundWait {
    pub fn new(factory: impl Fn() -> Box<dyn ForegroundWaitGuard> + Send + Sync + 'static) -> Self {
        Self(Arc::new(factory))
    }

    pub fn enter(&self) -> Box<dyn ForegroundWaitGuard> {
        (self.0)()
    }
}

impl std::fmt::Debug for SubagentForegroundWait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentForegroundWait").finish()
    }
}

register_resource!(
    "grow_build",
    "SubagentForegroundWait",
    SubagentForegroundWait
);

/// Carries the current parent prompt/turn ID for TaskTool subagent scoping.
///
/// Set by shell immediately before a prompt turn begins executing so
/// subagents launched during that turn can be cancelled together if the user
/// aborts the turn.
#[derive(Debug, Clone)]
pub struct CurrentPromptIdResource(pub String);

register_resource!(
    "grow_build",
    "CurrentPromptIdResource",
    CurrentPromptIdResource
);

/// Producer-stamped ownership for TaskTool children launched by the current
/// turn. Delayed lifecycle events carry this value instead of consulting the
/// Goal that happens to be current when they arrive.
#[derive(Debug, Clone, Default)]
pub struct CurrentSubagentOwnerResource(pub SubagentOwner);

register_resource!(
    "grow_build",
    "CurrentSubagentOwnerResource",
    CurrentSubagentOwnerResource
);

/// Thread-local tracing capture for behavioral log-emission tests.
#[cfg(test)]
pub(crate) mod test_capture {
    use tokio::sync::mpsc;
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;

    pub(crate) struct CapturedEvent {
        pub level: tracing::Level,
        pub fields: String,
    }

    pub(crate) struct CapturedTracing {
        pub events_rx: mpsc::UnboundedReceiver<CapturedEvent>,
        _guard: tracing::subscriber::DefaultGuard,
    }

    pub(crate) fn capture() -> CapturedTracing {
        let (tx, rx) = mpsc::unbounded_channel();
        let layer = CaptureLayer { tx };
        let subscriber = tracing_subscriber::registry().with(layer);
        let guard = tracing::subscriber::set_default(subscriber);
        CapturedTracing {
            events_rx: rx,
            _guard: guard,
        }
    }

    struct CaptureLayer {
        tx: mpsc::UnboundedSender<CapturedEvent>,
    }

    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            let _ = self.tx.send(CapturedEvent {
                level: *event.metadata().level(),
                fields: visitor.out,
            });
        }
    }

    #[derive(Default)]
    struct FieldVisitor {
        out: String,
    }

    impl tracing::field::Visit for FieldVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(field.name());
            self.out.push('=');
            self.out.push_str(&format!("{value:?}"));
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(field.name());
            self.out.push('=');
            self.out.push_str(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_resume_id;

    #[test]
    fn is_valid_resume_id_rejects_sentinels() {
        for bad in [
            "",
            "  ",
            "null",
            "Null",
            "NULL",
            "none",
            "None",
            "NONE",
            "undefined",
            "  null  ",
        ] {
            assert!(!is_valid_resume_id(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn is_valid_resume_id_accepts_real_ids() {
        for good in ["019e0000-0000-7000-8000-0000000000bb", "abc-123", "prev-id"] {
            assert!(is_valid_resume_id(good), "{good:?} should be valid");
        }
    }

    #[test]
    fn sanitize_cwd_value_strips_unmatched_leading_quote() {
        // Regression: stray leading double-quote from a model-emitted path.
        assert_eq!(
            super::sanitize_cwd_value("\"/Users/dev/work/project"),
            Some("/Users/dev/work/project".to_string()),
        );
    }

    #[test]
    fn sanitize_cwd_value_strips_matched_quotes() {
        assert_eq!(
            super::sanitize_cwd_value("\"/tmp\""),
            Some("/tmp".to_string())
        );
        assert_eq!(
            super::sanitize_cwd_value("'/tmp'"),
            Some("/tmp".to_string())
        );
        assert_eq!(
            super::sanitize_cwd_value("`/tmp`"),
            Some("/tmp".to_string())
        );
    }

    #[test]
    fn sanitize_cwd_value_strips_unmatched_trailing_quote() {
        assert_eq!(
            super::sanitize_cwd_value("/tmp\""),
            Some("/tmp".to_string())
        );
    }

    #[test]
    fn sanitize_cwd_value_rejects_sentinels() {
        for sentinel in ["", "  ", "null", "Null", "NONE", "undefined", "  null  "] {
            assert_eq!(
                super::sanitize_cwd_value(sentinel),
                None,
                "sentinel {sentinel:?} should be None",
            );
        }
    }

    #[test]
    fn sanitize_cwd_value_rejects_quoted_sentinels() {
        for input in ["\"null\"", "'none'", "`undefined`", "\"\"", "''", "``"] {
            assert_eq!(
                super::sanitize_cwd_value(input),
                None,
                "quoted sentinel {input:?} should be None",
            );
        }
    }

    #[test]
    fn sanitize_cwd_value_trims_whitespace_inside_quotes() {
        assert_eq!(
            super::sanitize_cwd_value("\"  /tmp  \""),
            Some("/tmp".to_string()),
        );
    }

    #[test]
    fn sanitize_cwd_value_preserves_clean_paths() {
        assert_eq!(super::sanitize_cwd_value("/tmp"), Some("/tmp".to_string()));
        assert_eq!(
            super::sanitize_cwd_value("/Users/me/project"),
            Some("/Users/me/project".to_string()),
        );
    }

    #[test]
    fn sanitize_cwd_value_expands_tilde() {
        let expected = shellexpand::tilde("~/foo").into_owned();
        let got = super::sanitize_cwd_value("~/foo").expect("should sanitize");
        assert_eq!(got, expected);
        // If we have a real home dir, it should no longer start with `~`.
        if expected != "~/foo" {
            assert!(!got.starts_with('~'), "tilde should be expanded: {got:?}");
        }
    }

    #[test]
    fn sanitize_cwd_value_keeps_inner_quotes() {
        let input = "/path with \"quote\" inside/";
        assert_eq!(super::sanitize_cwd_value(input), Some(input.to_string()));
    }

    #[test]
    fn sanitize_cwd_value_is_idempotent() {
        for input in [
            "\"/tmp",
            "'/tmp'",
            "  /tmp  ",
            "/tmp",
            "~/foo",
            "/path with \"quote\" inside/",
        ] {
            let once = super::sanitize_cwd_value(input);
            let twice = once.as_deref().and_then(super::sanitize_cwd_value);
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }

    #[test]
    fn is_terminal_returns_true_for_completed() {
        let status = super::SubagentSnapshotStatus::Completed {
            output: "done".into(),
            tool_calls: 1,
            turns: 1,
            tokens_used: 1,
            worktree_path: None,
        };
        assert!(status.is_terminal());
    }

    #[test]
    fn is_terminal_returns_true_for_failed() {
        let status = super::SubagentSnapshotStatus::Failed {
            error: "boom".into(),
        };
        assert!(status.is_terminal());
    }

    #[test]
    fn is_terminal_returns_true_for_cancelled() {
        let status = super::SubagentSnapshotStatus::Cancelled {
            reason: Some("user".into()),
        };
        assert!(status.is_terminal());
    }

    #[test]
    fn is_terminal_returns_false_for_running() {
        let status = super::SubagentSnapshotStatus::Running {
            turn_count: 0,
            tool_call_count: 0,
            tokens_used: 0,
            context_window_tokens: 0,
            context_usage_pct: 0,
            tools_used: vec![],
            error_count: 0,
        };
        assert!(!status.is_terminal());
    }

    #[test]
    fn is_terminal_returns_false_for_initializing() {
        let status = super::SubagentSnapshotStatus::Initializing;
        assert!(!status.is_terminal());
    }

    #[test]
    fn event_sender_is_clone() {
        use tokio::sync::mpsc;

        let (tx, _rx) = mpsc::unbounded_channel::<super::SubagentEvent>();
        let sender = super::SubagentEventSender(tx);
        let _cloned = sender.clone();
    }
}
