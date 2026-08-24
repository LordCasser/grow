//! Session actor command enum and associated public types.
//!
//! `SessionCommand` defines the message protocol used to drive a session
//! actor. It was extracted from `acp_session.rs` to keep the actor
//! implementation focused on behaviour.
use super::acp_types::*;
use crate::extensions::notification::SessionNotification;
use crate::session::signals::TurnDeltaSnapshot;
use agent_client_protocol as acp;
use tokio::sync::oneshot;
/// Structured context for a cancelled turn, replacing stringly-typed JSON.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CancellationContext {
    pub tool_name: Option<String>,
    pub reason: Option<String>,
    pub hook_name: Option<String>,
    /// What triggered the cancel (`"esc"`, `"ctrl_c"`); surfaced
    /// as `cancelTrigger` on the `PromptResponse`/`TurnCompleted` `_meta`.
    /// `None` for graceful in-turn cancels and older clients.
    pub trigger: Option<String>,
}
/// Failure surface of a `/btw` side question. Kept typed until the ACP
/// boundary so `handle_btw` can route model errors through the canonical
/// [`map_sampling_err_to_acp`](crate::sampling::error::map_sampling_err_to_acp)
/// (typed rate-limit / auth codes) instead of a flattened string.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SideQuestionError {
    #[error("side question model call failed: {0}")]
    Sampling(#[from] sampling_types::SamplingError),
    #[error("failed to prepare client: {0}")]
    PrepareClient(String),
    #[error("side question Sideband failed: {0}")]
    Sideband(String),
    #[error("No response from model")]
    EmptyResponse,
}
/// Prompt completion kind returned to the ACP layer.
#[derive(Debug, Clone)]
pub enum PromptCompletionKind {
    Completed,
    /// Silent EndTurn after stationarity/true-noop thrash. Distinct from
    /// Completed so a Goal-owned continuation can pause instead of hot-looping.
    StationarityEnded,
    Cancelled {
        category: Option<crate::session::events::CancellationCategory>,
        context: Option<CancellationContext>,
    },
    MaxTurnsReached {
        limit: usize,
    },
    Rewound,
    /// A queued prompt was removed (or cleared) from the server-authoritative
    /// queue before it ever ran. Used to resolve the still-pending
    /// `session/prompt` RPC of the client that submitted it WITHOUT triggering
    /// any turn-completion side effects: the prompt never started a turn, so the
    /// `prompt_complete` broadcast (which carries no `promptId` and would tell
    /// every attached leader-mode client the *running* turn ended) and the
    /// roster `Idle` delta (which would flip the dashboard off `Working` while
    /// the real turn is still in flight) must be skipped. See
    /// `MvpAgent::prompt`'s short-circuit and `respond_removed_prompt`.
    RemovedFromQueue,
}
/// Successful prompt/turn payload returned to the ACP layer and local persistence.
#[derive(Debug, Clone)]
pub struct PromptTurnOk {
    pub stop_reason: acp::StopReason,
    pub total_tokens: u64,
    pub turn_snapshot: Option<TurnDeltaSnapshot>,
    pub completion_kind: PromptCompletionKind,
    /// Schema-validated `--json-schema` output, delivered to the client in the
    /// prompt-response `_meta`. `None` unless a schema was requested;
    /// `Some(Err)` carries a parse/validation error message.
    pub structured_output: Option<Result<serde_json::Value, String>>,
    pub usage: Option<crate::extensions::notification::PromptUsage>,
}
/// Result of a prompt turn, containing the stop reason, accumulated token count,
/// and an optional turn-end signals snapshot (for trace metadata enrichment).
pub type PromptTurnResult = Result<PromptTurnOk, acp::Error>;
/// Convenience: successful end-of-turn result.
pub(crate) fn ok_end_turn(tokens: u64, snapshot: Option<TurnDeltaSnapshot>) -> PromptTurnResult {
    Ok(PromptTurnOk {
        stop_reason: acp::StopReason::EndTurn,
        total_tokens: tokens,
        turn_snapshot: snapshot,
        completion_kind: PromptCompletionKind::Completed,
        structured_output: None,
        usage: None,
    })
}
/// Priority levels for notification drain timing.
///
/// Ordering: `Next < Later` (derived from declaration order).
/// `Next` = more urgent, eligible for mid-turn drain (future enhancement).
/// `Later` = deferred to end-of-turn or idle drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationPriority {
    /// Drain mid-turn (between tool calls). For urgent monitor events.
    Next,
    /// Drain only at end-of-turn or when idle. Used for bash task completions.
    Later,
}
#[derive(Debug, Clone)]
pub enum NotificationSource {
    MonitorEvent { task_id: String },
    MonitorCompleted { task_id: String },
    BashTaskCompleted { task_id: String },
    SubagentCompleted { task_id: String },
}
impl NotificationSource {
    pub fn task_id(&self) -> &str {
        match self {
            Self::MonitorEvent { task_id }
            | Self::MonitorCompleted { task_id }
            | Self::BashTaskCompleted { task_id }
            | Self::SubagentCompleted { task_id } => task_id,
        }
    }
}
#[derive(Debug)]
pub struct TaskWakeFallback {
    pub prompt_id: String,
    pub prompt_blocks: Vec<acp::ContentBlock>,
    pub source: NotificationSource,
}
#[derive(Debug)]
pub struct TaskWakeAdmission {
    pub respond_to: oneshot::Sender<bool>,
    pub fallback: TaskWakeFallback,
}
pub enum SessionCommand {
    Initialize {
        system_prompt: String,
    },
    /// Non-destructive system-prompt sync on session attach: swaps only the
    /// leading `System` message, keeping user/assistant turns. Backed by the
    /// atomic `ChatStateCommand::ReplaceSystemHead` (see its doc for the
    /// serialization guarantees); no-op when the live head already matches.
    ReplaceSystemPrompt {
        system_prompt: String,
    },
    /// Install an immutable Goal snapshot before a delegated child turn is
    /// admitted. The snapshot is read-only and never follows parent revisions.
    SetGoalContextSnapshot {
        snapshot: tools::implementations::grow_build::update_goal::GoalContextSnapshot,
    },
    /// Resume hook: after a session is restored with
    /// `approval_pending == true`, re-issue the `grow/plan_approval`
    /// reverse-request so the client re-shows approval chrome over a real live
    /// waiter. Fire-and-forget; the actor spawns the round-trip + decision.
    RestorePlanApproval,
    QueuePrompt {
        prompt_id: String,
        prompt_blocks: Vec<acp::ContentBlock>,
        /// Explicit producer-assigned origin; never inferred from `prompt_id`.
        origin: crate::session::PromptOrigin,
        /// Explicit lifecycle kind for this regular turn.
        turn_kind: crate::session::TurnKind,
        /// Optional client identifier from the prompt request meta (overrides session-level one)
        client_identifier: Option<String>,
        /// Optional screen mode from the prompt request meta (`_meta.screenMode`,
        /// pager-only: `fullscreen` | `inline` | `minimal` | `headless`).
        /// Diagnostic-only; `None` for other clients and synthetic prompts.
        screen_mode: Option<String>,
        /// Skip `<user_query>` wrapping and large-prompt truncation.
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        /// Actor-authoritative admission and deferred fallback for terminal task wakes.
        admission: Option<TaskWakeAdmission>,
        respond_to: oneshot::Sender<PromptTurnResult>,
        /// Optional oneshot fired after the user-message Timeline event is
        /// durably committed, before LLM inference begins.
        persist_ack: Option<oneshot::Sender<()>>,
    },
    /// Execute a host slash command through the actor mailbox instead of the
    /// prompt queue. This keeps commands responsive while a model turn is
    /// running and guarantees that slash-prefixed text never reaches the
    /// model as user input.
    ExecuteSlashCommand {
        command: String,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    /// Append a user-authored `session/title` fact to the canonical Timeline.
    /// The actor owns this mutation so it serializes against automatic title
    /// generation and permanently consumes the one-shot generated-title route.
    SetSessionTitle {
        title: String,
        respond_to: oneshot::Sender<Result<chat_state::TimelineEvent, String>>,
    },
    QueryPromptStatus {
        prompt_id: String,
        respond_to: oneshot::Sender<crate::session::prompt_queue::PromptStatus>,
    },
    /// Snapshot the sole regular foreground owner for `session/load`.
    /// A future Goal continuation is not a foreground owner.
    QueryForeground {
        respond_to: oneshot::Sender<Option<prompt_queue::ForegroundSnapshot>>,
    },
    /// System event (NOT a user input): a background task completed after its
    /// Goal wait was displaced by user steering, or completed while the Goal
    /// turn gate was active. The actor either satisfies the explicit deferred
    /// wait or puts the completion through the ordinary idle drain.
    DeferredCompletionAvailable {
        source: NotificationSource,
        body: String,
    },
    BehaviorChange {
        session_mode: acp::SessionModeId,
        responds_to: oneshot::Sender<crate::session::behavior::BehaviorChangeOutcome>,
    },
    SetSessionModel {
        /// Stable `provider/model` catalog identity used by the UI and
        /// persistence. This is intentionally distinct from
        /// `sampling_config.model`, which is the provider-facing wire name.
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        use_concise: bool,
        /// When `false`, skip the system prompt rewrite (concise/default swap).
        /// Set to `false` for forked sessions so mid-session model switches
        /// cannot contaminate the inherited prompt configuration.
        apply_prompt_override: bool,
        /// When `true`, suppress the system prompt rewrite even though
        /// `apply_prompt_override` may be `true`. Set by the model-switch
        /// orchestrator immediately after a successful
        /// `RebuildAgentForDefinition` so the fresh harness's prompt
        /// (already installed by the rebuild handler) is not clobbered by
        /// the concise/default swap below.
        skip_prompt_rewrite: bool,
        /// Re-resolved auto-compact threshold for the new model. Computed
        /// by `MvpAgent` against the new model id so per-model remote settings
        /// and per-model user TOML overrides target the right model after a
        /// `/model` switch. The session actor stores this on
        /// `compaction.threshold_percent` (which is `Cell<u8>` so it can
        /// update without `&mut self`).
        auto_compact_threshold_percent: u8,
        responds_to: oneshot::Sender<Result<acp::ModelId, acp::Error>>,
    },
    /// Apply a validated hot-reload snapshot to an existing session without
    /// changing its harness or treating the update as a user model switch.
    ReloadModelConfig {
        model_id: acp::ModelId,
        sampling_config: sampler::SamplerConfig,
        image_description_model: Option<String>,
        inference_idle_timeout: std::time::Duration,
        max_retries: u32,
        auto_compact_threshold_percent: u8,
    },
    /// Zero-turn harness rebuild: build a brand-new `Agent` from the
    /// session's `AgentRebuildSpec` and the new `AgentDefinition`,
    /// re-register MCP tools, swap the live `Agent`, rewrite the
    /// system message in the conversation, persist the new prompt
    /// artifacts, and update `active_agent_type`.
    ///
    /// Triggered by `MvpAgent::set_session_model` when the new model's
    /// `agent_type` differs from the session's current one and no user
    /// message has been sent yet (`turn_count == 0`).
    RebuildAgentForDefinition {
        definition: agent::AgentDefinition,
        responds_to: oneshot::Sender<Result<(), acp::Error>>,
    },
    /// Override the model name and optionally inject extra HTTP headers
    /// into the session's sampling config.
    ///
    /// Unlike `SetSessionModel` (which requires a fully resolved `ModelEntry`
    /// and does NOT update `primaryModelId` in signals — the resolved model
    /// is already tracked via inference responses), this command also calls
    /// `set_primary_model()` so that signals report the override model
    /// rather than the agent-level default (e.g. `grow-4.5`).
    ///
    /// Keeps the existing base_url, api_key, and other config — only changes
    /// the request `model` field and merges any explicitly configured headers.
    ///
    /// Used to set model IDs (e.g. opaque third-party routing names) that are
    /// routing hints for the backend and don't need to exist in the
    /// agent's local model registry.
    OverrideModelName {
        model_name: String,
        extra_headers: indexmap::IndexMap<String, String>,
        /// Override the context window size for the new model. Without this,
        /// forked sessions inherit the source session's context window, causing
        /// auto-compact and context-usage signals to use the wrong threshold.
        context_window: Option<std::num::NonZeroU64>,
    },
    GetCurrentModel {
        responds_to: oneshot::Sender<String>,
    },
    GetCurrentBehavior {
        responds_to: oneshot::Sender<tool_types::BehaviorId>,
    },
    GetModelMetadata {
        responds_to: oneshot::Sender<chat_state::ModelMetadata>,
    },
    /// Snapshot for `/session-info`.
    GetSessionInfo {
        responds_to: oneshot::Sender<SessionInfoData>,
    },
    /// Compacts the current session, saving on the context window
    CompactSession {
        /// Optional user-provided context to guide the compaction
        user_context: Option<String>,
        respond_to: oneshot::Sender<acp::Result<CompactConversationStatus>>,
    },
    /// Reload plugin hooks and registry mid-session.
    ReloadPlugins {
        registry: Option<std::sync::Arc<agent::plugins::PluginRegistry>>,
    },
    /// Re-discover the session's own project hooks (`.grow/hooks`,
    /// `.grow/hooks`, plugin, or MCP config changes) mid-session, re-evaluating folder trust. Used by
    /// the interactive folder-trust grant so a granted folder's repo-local hooks
    /// start without a session restart (plugin-contributed hooks are handled by
    /// `ReloadPlugins`; this covers the non-plugin project hook registry).
    ReloadHooks,
    /// Re-discover skills from disk and update the session's skill baseline.
    RefreshSkillBaseline,
    /// Trigger an on-demand memory flush for this session.
    ///
    /// Calls `run_memory_flush("user_requested", None)` on the session actor.
    /// Returns an error if memory is not enabled for this session, or
    /// `Ok(true/false)` indicating whether a flush actually ran (false if
    /// another flush was already in progress).
    FlushMemory {
        respond_to: oneshot::Sender<acp::Result<bool>>,
    },
    /// Atomically select the session's canonical permission mode.
    SetPermissionMode {
        mode: crate::util::config::PermissionMode,
    },
    ResetPermissionState,
    Rewind {
        request: RewindRequest,
        respond_to: oneshot::Sender<anyhow::Result<RewindResponse>>,
    },
    /// Out-of-band history repair (`grow/session/repair`): fix tool-pairing
    /// violations (orphaned/displaced `ToolResult`s, duplicates, unanswered
    /// calls) that would otherwise 400 on every request. `dry_run` only
    /// reports. Refused while a turn is in flight.
    RepairHistory {
        dry_run: bool,
        respond_to:
            oneshot::Sender<anyhow::Result<chat_state::compaction_utils::HistoryRepairReport>>,
    },
    GetRewindPoints {
        respond_to: oneshot::Sender<RewindPointsResponse>,
    },
    /// Local file-snapshot counts keyed by `prompt_index`, read straight from
    /// the file-state tracker (independent of the chat-state prompt index,
    /// which is empty in bridge mode). The bridge joins these onto the
    /// server's rewind points so `num_file_snapshots`/`has_file_changes` match
    /// what local-mode rewind reports.
    GetRewindFileCounts {
        respond_to: oneshot::Sender<std::collections::HashMap<usize, usize>>,
    },
    /// Grow extension session notification - client-side events to store in persistence
    GrowSessionNotification {
        notification: SessionNotification,
    },
    /// Apply subagent usage into parent ledgers. Acks `()` once chat-state
    /// applied (prompt-attributed or session-only). Drop the oneshot on failure
    /// so the child treats the fold as not landed.
    RecordSubagentUsage {
        by_model: Vec<(String, chat_state::UsageTotals)>,
        parent_prompt_id: Option<String>,
        /// Nested subagent bill may under-count.
        incomplete: bool,
        respond_to: oneshot::Sender<()>,
    },
    /// Sticky incomplete for a parent prompt (or the live pin when `None`). Acks when marked.
    MarkSubagentUsageNotApplied {
        parent_prompt_id: Option<String>,
        respond_to: oneshot::Sender<()>,
    },
    /// Shared error-path usage attach (same policy as durable TurnCompleted).
    ErrorPathUsageFallback {
        prompt_id: Option<String>,
        respond_to: oneshot::Sender<Option<crate::extensions::notification::PromptUsage>>,
    },
    /// Flush the replay buffer and persistence, then signal completion.
    /// Used during reconnect to ensure all buffered content is persisted before replay.
    FlushComplete {
        respond_to: oneshot::Sender<()>,
    },
    /// Update MCP servers for an existing session (used during reconnect or
    /// mid-session via the `grow/session/update_mcp_servers` extension method).
    /// This replaces the current MCP server configuration and triggers re-initialization.
    ///
    /// The caller is notified via `respond_to` once MCP re-initialization
    /// completes (or immediately if configs are unchanged).
    UpdateMcpServers {
        mcp_servers: Vec<acp::McpServer>,
        respond_to: oneshot::Sender<Result<(), acp::Error>>,
    },
    /// Toggle an MCP server on/off within the session actor's event loop.
    /// Atomic read-modify-write avoids TOCTOU races with background config
    /// refreshes that can change `mcp_state.configs` between a snapshot read
    /// and an `UpdateMcpServers` command.
    ToggleMcpServer {
        server_name: String,
        enabled: bool,
        /// Fully-formed server config to add when re-enabling. Built by the
        /// caller via `merge_mcp_servers` (with explicit headers injected).
        /// `None` when disabling.
        server_config: Option<acp::McpServer>,
        respond_to: oneshot::Sender<Result<(), acp::Error>>,
    },
    /// Toggle a single MCP tool on/off within a server. The server stays connected;
    /// only the tool's registration in ToolBridge is affected.
    ToggleMcpTool {
        server_name: String,
        tool_name: String,
        enabled: bool,
        respond_to: oneshot::Sender<Result<(), acp::Error>>,
    },
    /// Read MCP status: which servers are configured, which clients are healthy, what tools.
    GetMcpStatus {
        respond_to: oneshot::Sender<crate::extensions::mcp::McpStatusSnapshot>,
    },
    /// Snapshot the session's live MCP client pool for subagent inheritance.
    SnapshotMcpPool {
        respond_to: oneshot::Sender<Option<crate::session::mcp_servers::SharedMcpPool>>,
    },
    /// Snapshot the session's client-registered hooks so a subagent inherits the same
    /// PreToolUse gate and observe hooks over the parent's connection.
    SnapshotClientHooks {
        respond_to: oneshot::Sender<crate::extensions::hooks::ClientHooks>,
    },
    /// Replace the session's client-registered hooks. Sent on `load_session` reconnect to a
    /// live actor so a client can re-register (or clear) its hooks without a fresh session.
    SetClientHooks {
        hooks: crate::extensions::hooks::ClientHooks,
    },
    /// Client-driven MCP tool call outside the LLM loop.
    CallMcpTool {
        server_name: String,
        server_url: Option<String>,
        tool_name: String,
        arguments: serde_json::Value,
        respond_to: oneshot::Sender<Result<crate::extensions::mcp::McpCallResponse, String>>,
    },
    ReadMcpResource {
        server_name: String,
        uri: String,
        respond_to:
            oneshot::Sender<Result<crate::extensions::mcp::McpReadResourceResponse, String>>,
    },
    /// Move a foreground bash command to background by tool_call_id.
    /// Unblocks the agent loop so it can continue with the next action.
    BackgroundForegroundCommand {
        tool_call_id: String,
        respond_to: oneshot::Sender<bool>,
    },
    /// Kill a background task by task_id.
    /// Routes through the ToolBridge's TerminalBackend (lock-free, Arc-shared).
    KillBackgroundTask {
        task_id: String,
        respond_to: oneshot::Sender<Result<tools::types::KillOutcome, String>>,
    },
    DeleteScheduledTask {
        task_id: String,
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    /// List all background tasks.
    /// Routes through the ToolBridge's TerminalBackend.
    ListTasks {
        respond_to: oneshot::Sender<Option<Vec<tools::types::TaskSnapshot>>>,
    },
    /// Atomically unload this actor only when it owns no live work. The actor
    /// decides from its canonical state and closes its mailbox before
    /// acknowledging `true`; callers must never follow this with a separate
    /// `Shutdown`, which would reintroduce a check-then-act race.
    UnloadIfIdle {
        respond_to: oneshot::Sender<bool>,
    },
    GetHooksList {
        respond_to: oneshot::Sender<extension_types::HooksListResponse>,
    },
    /// Execute a hooks management action from the pager modal.
    HooksAction {
        action: extension_types::HooksAction,
        respond_to: oneshot::Sender<extension_types::ActionOutcome>,
    },
    /// Broadcast a plugin updates notification to the session.
    NotifyPluginUpdates {
        updates: Vec<(String, String, String)>,
    },
    /// Execute a plugins management action from the pager modal.
    PluginsAction {
        action: extension_types::PluginsAction,
        respond_to: oneshot::Sender<extension_types::ActionOutcome>,
    },
    /// This session's plugin registry, as served by `grow/plugins/list`.
    PluginsList {
        respond_to: oneshot::Sender<Option<std::sync::Arc<agent::plugins::PluginRegistry>>>,
    },
    /// System event (NOT a user input): inject a notification (monitor
    /// event or bash task completion) into the session's notification queue.
    /// Notifications are idle-gated and batched by
    /// `maybe_drain_notifications`.
    InjectNotification {
        prompt_id: String,
        prompt_blocks: Vec<acp::ContentBlock>,
        priority: NotificationPriority,
        source: NotificationSource,
    },
    /// Drop queued / mid-turn-buffered `MonitorEvent` notifications for a
    /// task. Used when natural monitor exit already auto-woke via
    /// `TaskCompleted` so stdout + terminal pipeline events do not start a
    /// second `NotificationDrain` turn for the same completion.
    DropMonitorNotifications {
        task_id: String,
    },
    /// Dispatch a compat `Notification` hook (e.g. `task_complete`
    /// from the notification bridge, which does not go through `send_grow_notification`).
    DispatchNotificationHook {
        notification_type: String,
        message: Option<String>,
        title: Option<String>,
        level: Option<String>,
    },
    /// Record background-task ids that survive a delegated child spawned by a
    /// Goal turn. The handler keeps them in `goal_turn_task_ids` whenever the
    /// Goal runtime is available, so a late completion cannot wake the parent
    /// after the Goal has paused, blocked, completed, or been cleared.
    RecordGoalTurnTaskIds {
        task_ids: Vec<String>,
    },
    /// Remove a queued (not-yet-running) prompt from the authoritative prompt
    /// queue. Versioned + idempotent: a stale `expected_version`
    /// or an already-drained `id` is a no-op (the actor just re-broadcasts the
    /// current queue so the client reconciles). When `owner` is `Some`, the
    /// removal only applies if the item's attribution matches (edit authority:
    /// a client edits its own items).
    RemoveQueuedPrompt {
        id: String,
        expected_version: u64,
        owner: Option<String>,
    },
    /// Reorder the queued (not-yet-running) prompts to match `ordered_ids`.
    /// Ids not present in the live queue are ignored; queued items missing
    /// from `ordered_ids` keep their relative order at the back. The actor
    /// re-broadcasts the resulting queue. Idempotent.
    ReorderQueue {
        ordered_ids: Vec<String>,
    },
    /// Clear queued (not-yet-running) prompts. When `owner` is `Some`, only
    /// that client's items are cleared. The running turn is never touched.
    ClearQueue {
        owner: Option<String>,
    },
    /// Replace the text of a queued (not-yet-running) prompt in place
    /// (server-side LWW). Last write wins via the actor's
    /// serialized mailbox; the rebroadcast of `grow/queue/changed` is the
    /// truth signal for every attached client. The original `owner`
    /// attribution is preserved; `editor` is recorded as the most recent
    /// editor (for future "alice edited this" UX). A missing id, or an id
    /// that names the currently-running turn, is a benign no-op.
    EditQueuedPrompt {
        id: String,
        new_text: String,
        editor: Option<String>,
    },
    /// Hold a queued prompt out of combine-on-promote while a client edits it
    /// in the composer. Released via [`Self::ReleaseCombineEdit`].
    HoldCombineEdit {
        id: String,
    },
    /// Release a previous [`Self::HoldCombineEdit`].
    ReleaseCombineEdit {
        id: String,
    },
    /// Atomically move a queued prompt into the current regular turn. The
    /// current turn id is part of admission so a late UI action cannot steer
    /// a replacement turn. Versioned + idempotent like
    /// [`RemoveQueuedPrompt`]; `grow/queue/changed` remains authoritative.
    SteerQueuedPrompt {
        expected_turn_id: String,
        id: String,
        expected_version: u64,
        owner: Option<String>,
        /// Optional replacement text (client-edited row). When `Some`, it is
        /// interjected INSTEAD of the stored queue text, under the same single
        /// version check — edit + interject is one atomic op (a stale version
        /// no-ops the whole thing, edited text included).
        new_text: Option<String>,
    },
    /// Cancel the running turn. `kill_background_tasks` distinguishes a hard
    /// teardown (subagent shutdown — drains the whole queue) from a normal
    /// interactive cancel (Ctrl+C — preserves queued user prompts so the next
    /// one auto-runs). Ctrl+C tears down the running turn and queued terminal
    /// task-completion wakes; other cancel triggers tear down only the running
    /// turn. The follow-up `maybe_start_running_task` promotes the next item.
    Cancel {
        cancel_subagents: bool,
        kill_background_tasks: bool,
        rewind_if_pristine: bool,
        /// Whether this cancel carries an explicit user intent to PAUSE an
        /// active Goal (the Goal interrupt panel's "Pause goal" choice).
        /// When true the Cancel arm runs `auto_pause_goal_if_active`; every
        /// other cancel (plain Ctrl+C/Esc outside Goal, subagent teardown,
        /// lifecycle shutdown) leaves an active Goal untouched.
        pause_goal: bool,
        /// Free-form discriminator for *what* triggered the cancel, taken from
        /// the `session/cancel` request `_meta.cancelTrigger` (e.g. `"esc"`,
        /// `"ctrl_c"`). `None` for older clients and programmatic teardowns
        /// (subagent shutdown). Recorded in the `mid_turn_abort` turn-end's
        /// `cancellation_context` JSON; the category stays `MidTurnAbort`.
        trigger: Option<String>,
    },
    Shutdown,
    AdvertiseCommands,
    GetWorkflowCatalogState {
        respond_to: oneshot::Sender<(bool, bool)>,
    },
    ListAvailableCommands {
        respond_to: oneshot::Sender<crate::session::slash_commands::ListCommandsResponse>,
    },
    /// Re-discover skills from disk, update the SkillManager baseline,
    /// and re-advertise slash commands to the client.
    ReloadSkills,
    /// Dispatch session_start hook using the actor's loaded HookRegistry.
    DispatchSessionStartHook {
        /// "new" for brand new sessions, "load" for sessions loaded from disk.
        source: String,
    },
    /// Retrieve the session's active agent type.
    ///
    /// Returns the name of the `AgentDefinition` that was used to initialize
    /// this session (or the most recent one applied via `request_behavior_change`).
    /// Used by `mvp_agent.set_session_model` to check whether a model's
    /// `agent_type` is compatible with the current session before switching.
    GetActiveAgent {
        responds_to: oneshot::Sender<Option<String>>,
    },
    /// Ask a side question without interrupting the current turn.
    /// The session snapshots the conversation context, makes a single
    /// tool-free model call, and returns the response text.
    SideQuestion {
        question: String,
        respond_to: oneshot::Sender<Result<String, SideQuestionError>>,
    },
    /// Generate a session recap (a short "where was I" summary) and broadcast
    /// it to clients via `SessionUpdate::SessionRecap`.
    ///
    /// Fire-and-forget: the session snapshots the conversation, makes a single
    /// tool-free model call, and emits the result for display only. It never
    /// mutates the conversation, so unlike `SideQuestion` it needs no reply
    /// channel — the answer travels back as a notification.
    Recap {
        /// `true` when triggered automatically on return-from-away,
        /// `false` for an explicit `/recap`.
        auto: bool,
    },
    /// Request an AI-generated shell command suggestion.
    ///
    /// The session actor builds a minimal prompt from `prefix` + `cwd`, calls
    /// the sampler using configured or upstream sampling defaults, and returns
    /// the suggested completion via `respond_to`.
    AISuggest {
        prefix: String,
        cwd: String,
        model_override: Option<String>,
        respond_to: oneshot::Sender<Option<String>>,
    },
    /// Predict the user's likely next prompt (tab autocomplete ghost text).
    ///
    /// Fired by the client after a turn completes. The session builds a
    /// compact text-only transcript of the recent conversation, makes one
    /// tool-free model call (default `grow-build-0.1` when available via
    /// `model_override`, else the session model), sanitizes the output, and
    /// returns the predicted prompt via `respond_to`. Best-effort: any
    /// failure returns `None`.
    SuggestPrompt {
        model_override: Option<String>,
        respond_to: oneshot::Sender<Option<String>>,
    },
    /// Rewrite a raw memory note into well-structured markdown via a one-shot
    /// LLM call. The session uses `prepare_chat_completion()` with the
    /// `grow-build` model and configured or upstream sampling defaults.
    RewriteMemoryNote {
        raw_text: String,
        context_summary: String,
        respond_to: oneshot::Sender<Result<String, String>>,
    },
    /// Inject a user message into the identified active regular turn without
    /// creating another terminal or changing Goal lifecycle.
    SteerTurn {
        expected_turn_id: String,
        text: String,
        /// Client-minted id echoed back on the broadcast
        /// `grow/session/interjection` so the originating pager can dedup its
        /// optimistic local block. `None` from older clients.
        id: Option<String>,
        /// Pasted images riding along with the interjection. Empty from
        /// text-only / older clients.
        images: Vec<acp::ImageContent>,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    /// System event (NOT a user input): a workflow run completed and the
    /// actor queues a synthetic completion turn for the model.
    WorkflowCompletionTurn {
        run_id: String,
        revision: u64,
        outcome: workflow::WorkflowOutcome,
    },
    /// Take turn messages from the chat state actor (proxied from mvp_agent).
    TakeTurnMessages {
        respond_to: oneshot::Sender<Option<chat_state::TurnCapture>>,
    },
    /// Persist the current git HEAD commit and branch to summary.json.
    ///
    /// Sent at the end of each prompt turn so `--restore-code` sees the latest
    /// HEAD even when the `GitHeadChanged` filesystem watcher misses events.
    PersistGitHead {
        commit: Option<String>,
        branch: Option<String>,
    },
}
