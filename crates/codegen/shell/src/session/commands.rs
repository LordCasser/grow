//! Session actor command enum and associated public types.
//!
//! `SessionCommand` defines the message protocol used to drive a session
//! actor. It was extracted from the actor module to keep the actor
//! implementation focused on behaviour.
use super::acp_types::*;
use crate::extensions::notification::SessionNotification;
use crate::session::signals::TurnDeltaSnapshot;
use acp_transport::protocol as acp;
use tokio::sync::oneshot;

/// One user-visible invocation on the Shell-owned command plane.
///
/// This identity crosses the ACP extension boundary and follows an idle
/// command through the internal prompt scheduler. It is presentation
/// metadata only: the command may create durable domain events, but neither
/// the description nor the invocation id is projected into model context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCommandInvocation {
    pub command: String,
    pub description: String,
    pub invocation_id: String,
}

/// Client-authored ordering authority for a desired-state control request.
///
/// Shell revisions describe the order in which requests reach the actor. This
/// token preserves the user's intent order when two RPC tasks from one client
/// race in transit: an older `(generation, sequence)` can never replace a
/// newer target from the same client instance. Different clients remain
/// ordered by actor admission.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlIntent {
    pub client_id: String,
    pub generation: u64,
    pub sequence: u64,
}

pub const CONTROL_INTENT_META_KEY: &str = "grow/controlIntent";

/// Marks `set_session_model` as an effort-only Sampling patch. The request's
/// model id is a client display hint; the actor composes the effort with its
/// newest desired Sampling model so a stale client cannot restore an older
/// model while another client has a model change pending.
pub const EFFORT_PATCH_META_KEY: &str = "grow/effortPatch";

pub fn effort_patch_from_meta(meta: Option<&acp::Meta>) -> Result<bool, acp::Error> {
    match meta.and_then(|meta| meta.get(EFFORT_PATCH_META_KEY)) {
        None => Ok(false),
        Some(serde_json::Value::Bool(value)) => Ok(*value),
        Some(_) => {
            Err(acp::Error::invalid_params()
                .data(format!("{EFFORT_PATCH_META_KEY} must be a boolean")))
        }
    }
}

/// Immutable authority captured when an effort-only request enters Shell.
/// Resolution remains actor-owned because only the actor knows the latest
/// desired Sampling model. Ordinary sessions use one published catalog
/// generation; Workflow children remain confined to their frozen Run route.
#[derive(Clone)]
pub enum SessionEffortAuthority {
    Catalog {
        catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
        origin_client: Option<crate::http::OriginClientInfo>,
    },
    Workflow {
        route: crate::session::workflow::tracker::WorkflowRuntimeRoute,
        models_manager: crate::agent::models::ModelsManager,
    },
}

impl ControlIntent {
    pub fn from_meta(meta: Option<&acp::Meta>) -> Result<Option<Self>, acp::Error> {
        let intent: Option<Self> = meta
            .and_then(|meta| meta.get(CONTROL_INTENT_META_KEY))
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| {
                acp::Error::invalid_params()
                    .data(format!("invalid {CONTROL_INTENT_META_KEY}: {error}"))
            })?;
        if let Some(intent) = intent.as_ref() {
            intent.validate().map_err(|error| {
                acp::Error::invalid_params()
                    .data(format!("invalid {CONTROL_INTENT_META_KEY}: {error}"))
            })?;
        }
        Ok(intent)
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.client_id.trim().is_empty() {
            return Err("clientId must be a non-empty string");
        }
        Ok(())
    }

    pub fn insert_meta(&self, meta: &mut acp::Meta) {
        meta.insert(
            CONTROL_INTENT_META_KEY.to_owned(),
            serde_json::to_value(self).expect("ControlIntent is JSON-serializable"),
        );
    }
}

/// Terminal outcome of a latest-wins desired-state request.
/// Supersession is expected control flow, not a transport error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesiredStateOutcome<T> {
    Applied(T),
    /// An exact replay of an intent the resident actor is still applying.
    /// The client keeps its pending projection and waits for the authoritative
    /// terminal update; this is not supersession.
    InFlight,
    Superseded,
}

const CONTROL_TERMINAL_PUBLISHED_KEY: &str = "grow/controlTerminalPublished";

/// Mark an ACP control error whose actor-owned terminal projection has already
/// been scheduled. Clients can then avoid painting a second local error while
/// still surfacing validation/transport failures that never reached the actor.
pub fn mark_control_terminal_published(mut error: acp::Error) -> acp::Error {
    let mut data = match error.data.take() {
        Some(serde_json::Value::Object(data)) => data,
        Some(detail) => serde_json::Map::from_iter([("detail".to_string(), detail)]),
        None => serde_json::Map::new(),
    };
    data.insert(
        CONTROL_TERMINAL_PUBLISHED_KEY.to_string(),
        serde_json::Value::Bool(true),
    );
    error.data = Some(serde_json::Value::Object(data));
    error
}

pub fn control_terminal_was_published(error: &acp::Error) -> bool {
    error
        .data
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .and_then(|data| data.get(CONTROL_TERMINAL_PUBLISHED_KEY))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Structured context for a cancelled turn, replacing stringly-typed JSON.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
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

const TURN_BOUNDARY_FAILURE_KIND: &str = "turn_boundary_persistence_failed";

pub(crate) fn fatal_turn_boundary_error(phase: &str, detail: impl Into<String>) -> acp::Error {
    let detail = detail.into();
    acp::Error::internal_error().data(serde_json::json!({
        "message": format!("turn {phase} was not durably recorded: {detail}"),
        "error_kind": TURN_BOUNDARY_FAILURE_KIND,
        "phase": phase,
    }))
}

pub(crate) fn is_fatal_turn_boundary_error(error: &acp::Error) -> bool {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("error_kind"))
        .and_then(serde_json::Value::as_str)
        == Some(TURN_BOUNDARY_FAILURE_KIND)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_boundary_failure_is_a_typed_session_fatal_error() {
        let error = fatal_turn_boundary_error("terminal", "disk rejected append");
        assert!(is_fatal_turn_boundary_error(&error));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("phase"))
                .and_then(serde_json::Value::as_str),
            Some("terminal")
        );
        assert!(!is_fatal_turn_boundary_error(
            &acp::Error::internal_error().data("ordinary turn error")
        ));
    }
}
pub enum SessionCommand {
    /// Install an immutable Goal snapshot before a delegated child turn is
    /// admitted. The snapshot is read-only and never follows parent revisions.
    SetGoalContextSnapshot {
        snapshot: tools::implementations::grow_build::update_goal::GoalContextSnapshot,
    },
    /// Resume hook: after a session is restored with
    /// `approval_pending == true`, re-issue the `grow/plan_approval`
    /// reverse-request so the client re-shows approval chrome over a real live
    /// waiter. Fire-and-forget; the actor spawns the round-trip + decision.
    RestorePlanApproval {
        respond_to: oneshot::Sender<Result<(), String>>,
    },
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
        invocation: HostCommandInvocation,
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
    /// Re-publish transient Shell-authoritative control projections after a
    /// client load or renderer restart. These snapshots never enter replay.
    PublishControlState {
        respond_to: oneshot::Sender<()>,
    },
    /// Admit one source-owned signal into the durable Timeline inbox.
    /// Producers never queue model turns directly; the actor derives delivery
    /// from received-minus-consumed facts after the immutable payload lands.
    ReceiveNotification {
        source: chat_state::NotificationSource,
        source_version: chat_state::NotificationSourceVersion,
        body: String,
        /// Optional producer barrier. Terminal producers use it so their
        /// lifecycle cannot advance past notification delivery until the
        /// durable inbox owns the corresponding exactly-once receipt.
        respond_to: Option<oneshot::Sender<Result<String, String>>>,
    },
    BehaviorChange {
        session_mode: acp::SessionModeId,
        intent: Option<ControlIntent>,
        responds_to:
            oneshot::Sender<Result<crate::session::behavior::BehaviorChangeOutcome, acp::Error>>,
    },
    /// Serialize model-initiated Goal lifecycle changes with every other
    /// session control mutation. The tool-facing channel is only an ingress
    /// adapter; this mailbox remains the sole control-plane writer.
    GoalControl {
        command: tools::implementations::grow_build::update_goal::GoalCommand,
    },
    /// Charge one model call settled inside the root session's active Goal
    /// usage window. Descendant sessions and sideband runtimes capture the
    /// Goal id at settlement time and submit it to the root actor through this
    /// single accounting ingress.
    RecordGoalUsage {
        goal_id: String,
        tokens: i64,
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    /// Fail closed when a provider attempt admitted inside a Goal window did
    /// not return usage. The Goal ledger becomes a lower bound and autonomous
    /// continuation is paused durably until an eligible explicit user restart.
    RecordGoalUsageIncomplete {
        goal_id: String,
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    /// Settle one provider attempt whose immutable Goal ownership and usage
    /// outcome were already claimed in the shared usage window. The attempt id
    /// makes retries and owner-future destruction idempotent at the root.
    SettleGoalUsageAttempt {
        attempt_id: String,
        respond_to: oneshot::Sender<Result<bool, String>>,
    },
    SetSessionModel {
        /// Complete route resolved under one catalog/Workflow authority
        /// snapshot. Every model-sensitive execution knob is applied together
        /// at the next completed-step boundary, or immediately when idle.
        route: crate::agent::models::PublishedSessionRoute,
        /// Exact ordinary-session catalog generation that authorized this
        /// selection. Workflow children carry `None` because their Run route
        /// is already the complete authority.
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        intent: Option<ControlIntent>,
        responds_to:
            oneshot::Sender<Result<DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>>,
    },
    /// Patch reasoning effort onto the actor's latest desired Sampling model.
    /// This is distinct from `SetSessionModel`: composing at the actor is what
    /// prevents cross-client stale model hints from winning accidentally.
    PatchSessionEffort {
        effort: sampling_types::ReasoningEffort,
        authority: SessionEffortAuthority,
        intent: Option<ControlIntent>,
        responds_to:
            oneshot::Sender<Result<DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>>,
    },
    /// Apply a validated hot-reload snapshot to an existing session without
    /// changing its harness or treating the update as a user model switch.
    ReloadModelConfig {
        catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
        responds_to: oneshot::Sender<Result<(), acp::Error>>,
    },
    /// Select a new Agent profile. The actor admits this command immediately,
    /// applies it after the current step without interrupting its stream or
    /// tool batch, durably appends the role through Timeline Control, then
    /// swaps the harness and re-registers runtime resources.
    RebuildAgentForDefinition {
        definition: agent::AgentDefinition,
        intent: Option<ControlIntent>,
        responds_to: oneshot::Sender<Result<DesiredStateOutcome<()>, acp::Error>>,
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
        subagent_id: String,
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
    /// Dispatch a host `Notification` hook (e.g. `task_complete` from the
    /// notification bridge, which does not go through `send_grow_notification`).
    DispatchNotificationHook {
        notification_type: String,
        message: Option<String>,
        title: Option<String>,
        level: Option<String>,
    },
    /// Record background-task ids that survive a delegated Goal child. The
    /// producer carries the immutable owner captured at child admission; the
    /// parent must not re-sample its current Goal when the child exits.
    RecordGoalOwnedTaskIds {
        goal_id: String,
        definition_revision: u64,
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
    /// Admit a process-local coordination inquiry into this Session's own
    /// bounded FIFO. The target actor is the sole owner of context snapshot,
    /// cross-workspace approval, and Sideband execution.
    RunCoordinationInquiry {
        inquiry: crate::coordination::InboundInquiry,
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
    /// Workflow terminal producer notification. The state is the exact
    /// manifest snapshot whose revision ended the execution, so a queued
    /// command can never render a later retry as this completion.
    WorkflowCompleted {
        state: crate::session::workflow::tracker::WorkflowRunState,
        respond_to: oneshot::Sender<Result<(), String>>,
    },
    /// A Workflow owner failed to commit its terminal Timeline boundary.
    /// This is a session-fatal persistence error, not merely a failed Run;
    /// the actor must enter the same fail-stop teardown as a foreground turn.
    WorkflowTerminalFailure {
        run_id: String,
        error: String,
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
