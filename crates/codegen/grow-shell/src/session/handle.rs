//! `SessionHandle` — the `Clone + Send` proxy for interacting with a session actor.
//!
//! Callers hold a `SessionHandle` and send `SessionCommand` messages via the
//! internal channel. Extracted from `acp_session.rs` to keep the actor
//! implementation focused on behaviour.
use super::commands::SessionCommand;
use super::persistence::PersistenceMsg;
use agent_client_protocol as acp;
use grow_sampling_types::ReasoningEffort;
use std::collections::{HashMap, HashSet};
use tokio::sync::{mpsc, oneshot};
use xai_hunk_tracker::HunkTrackerHandle;
/// Coarse lifecycle state of a session as known to the leader/agent.
///
/// A grow session has no
/// terminal status field on its own — it is a resumable log on disk — so
/// "liveness" is *residency + turn-state*, not a pid. The agent's join-handle
/// supervisor tracks this per session so a panicked actor can be reaped
/// (demoted to `Dormant`) instead of lingering as a roster zombie. This is the
/// data source the roster/dashboard reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLiveState {
    /// Resident actor, a turn is currently running.
    Working,
    /// Resident actor, no turn in flight.
    IdleResident,
    /// On disk, not resident (idle-unloaded or never loaded this run).
    Dormant,
    /// Finished and resumable (terminal marker on disk).
    Completed,
    /// Actor panicked / load failed: the `JoinHandle` ended with no terminal
    /// marker. Harmless to reap — the conversation persists and demotes to
    /// `Dormant` on the next disk scan.
    DeadFailed,
}
/// `_meta` key carrying [`SessionHandle::scheduler_background_loops`] on the
/// `session/new` and `session/load` responses. Defined here so the shell that
/// publishes it and the clients that read it share one spelling.
pub const SCHEDULER_BACKGROUND_LOOPS_META_KEY: &str = "grow/schedulerBackgroundLoops";
/// Handle for interacting with a session actor.
/// Note: Permission event receivers are returned separately from `spawn_session_actor`
/// and should be stored/managed by the caller.
#[derive(Clone)]
pub struct SessionHandle {
    pub cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Persistence channel shared with the actor (used by extension handlers).
    pub(crate) persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    /// Current running prompt/turn id, if any.
    ///
    /// Shared with the session actor so external cancellation paths can target
    /// subagents launched by the active turn only.
    pub current_prompt_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// Open blocking reverse-requests (permission / question / plan-approval),
    /// keyed by `tool_call_id`. Mirrors `current_prompt_id`: the same `Arc` is
    /// shared with the session actor, which inserts on issue and removes on
    /// resolve. The roster reads this synchronously to surface `NeedsInput`
    /// Never persisted.
    pub pending_interactions: crate::session::pending_interaction::PendingInteractions,
    /// Session info (id, cwd) - cached for quick access without querying persistence
    pub info: crate::session::info::Info,
    /// Resolved turn limit for this session; lets a spawned subagent inherit
    /// the parent's limit. `None` = unlimited.
    pub max_turns: Option<usize>,
    /// Handle to the hunk tracker for this session
    pub hunk_tracker_handle: HunkTrackerHandle,
    /// Actor-based chat state handle — lets callers inspect final conversation state.
    pub chat_state_handle: grow_chat_state::ChatStateHandle,
    /// Handle to session signals (used for completion tracking)
    pub signals_handle: super::signals::SessionSignalsHandle,
    /// Shared gate controlling whether the session actor forwards
    /// notifications to the client via the gateway. See
    /// [`SessionActor::gateway_enabled`] for details.
    pub gateway_enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// MCP server configs for this session (merged local + client-provided).
    /// Stored on the handle so forked sessions can inherit the parent's
    /// MCP servers without requiring a round-trip through the session actor.
    ///
    /// **Note:** This is a snapshot from `spawn_session_actor` time. If the
    /// client later sends `UpdateMcpServers`, the handle's copy is NOT updated.
    /// This is fine for forks that happen immediately after spawn, but callers
    /// that need the latest MCP state should query the session actor via command.
    pub mcp_servers: Vec<acp::McpServer>,
    /// Original client-provided MCP servers (pre-merge). Used by plugin
    /// reload to re-compute the merged MCP server list.
    pub initial_client_mcp_servers: Vec<acp::McpServer>,
    /// Stable display path for forked sessions (original project path).
    ///
    /// When set, the hunk tracker extension handler rewrites worktree paths
    /// in API responses to this path so the client UI shows the original
    /// project path, not the worktree path.
    pub display_cwd: Option<String>,
    /// Session context captured at spawn time so callers can inherit shared runtime state.
    pub tool_context: crate::tools::ToolContext,
    /// The model this session was created with (or switched to via setModel).
    /// Per-session tracking prevents cross-client contamination in leader mode
    /// where `MvpAgent.current_model_id` is shared mutable state.
    pub model_id: acp::ModelId,
    /// Whether this session's scheduled fires run as detached background
    /// subagents. Copied from the value the spawn resolved for the session's
    /// [`AgentRebuildSpec`](crate::session::agent_rebuild::AgentRebuildSpec), so
    /// it is pinned for the session's whole life exactly like the fire side.
    /// Published to clients on the `session/new` / `session/load` response so
    /// they describe the fires this session will actually get rather than
    /// re-resolving a setting that may have flipped since spawn.
    pub scheduler_background_loops: bool,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// YOLO (auto-approve) mode for this session.
    /// Per-session tracking prevents cross-client contamination in leader mode
    /// where one client enabling YOLO could affect another client's sessions.
    pub yolo_mode: bool,
    /// Explicit origin client metadata captured when the session was created.
    /// Used for per-session User-Agent rendering and for scoping leader-mode
    /// client behaviors like yolo broadcasts.
    pub origin_client: Option<crate::http::OriginClientInfo>,
    /// Whether the client that created this session advertised
    /// `grow/codeNavigation.enabled`.  Stored per-session so that in leader
    /// mode a later `initialize()` from a different client cannot retroactively
    /// change code-nav eligibility for already-running sessions.
    pub code_nav_enabled: bool,
    /// Whether the `ask_user_question` tool is exposed for this session
    /// (`_meta.askUserQuestion` / `--no-ask-user` and the remote settings / config /
    /// env gate). Stored per-session so subagents inherit it at spawn.
    pub ask_user_question_enabled: bool,
    /// Plan mode tracker — shared with the session actor via Arc.
    /// Exposed so the `grow/toggle_plan_mode` handler can toggle plan mode
    /// without going through the session command channel.
    pub behavior: std::sync::Arc<parking_lot::Mutex<crate::session::behavior::BehaviorController>>,
    /// Debug flag: when set to `true`, the next turn unconditionally triggers
    /// auto-compaction regardless of context window usage. Consumed (reset to
    /// `false`) atomically on use via `compare_exchange`.
    /// Set via `grow/debug/arm_auto_compact`.
    pub force_compact: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub permission_handle: grow_workspace::permission::PermissionHandle,
    /// The agent definition name for this session.
    pub agent_name: String,
    /// Peer-Agent visibility derived from the active Agent's task-tool policy.
    /// Kept beside `agent_name` so runtime spawn validation matches the tool
    /// description even after an in-session Agent switch.
    pub subagent_filter: grow_agent::config::SubagentFilter,
    pub managed_mcp_proxy_base_url: String,
    /// Hook registry for this session (snapshot from spawn time).
    pub hook_registry: Option<std::sync::Arc<grow_hooks::discovery::HookRegistry>>,
    /// Typed workspace operations handle (agent sessions use local ops).
    pub workspace_ops: grow_workspace::WorkspaceOps,
    /// Terminal backend for this session. Subagents inherit the parent's
    /// backend so background tasks and monitors survive the subagent's exit.
    pub terminal_backend: Option<std::sync::Arc<dyn grow_tools::computer::types::TerminalBackend>>,
    /// Notification handle for this session's tool bridge. Subagents use
    /// this to reparent surviving tasks' notification handles on exit so
    /// events route to the parent's notification bridge.
    pub tools_notification_handle: Option<grow_tools::notification::types::ToolNotificationHandle>,
    /// Scheduler handle for this session. Subagents inherit the parent's
    /// handle so scheduled tasks survive the subagent's exit.
    pub scheduler_handle:
        Option<grow_tools::implementations::grow_build::scheduler::types::SchedulerHandle>,
}
impl SessionHandle {
    /// Last assistant `model_id` / `model_fingerprint` in conversation (global, not turn-scoped).
    pub(crate) async fn get_model_metadata(&self) -> grow_chat_state::ModelMetadata {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetModelMetadata { responds_to: tx })
            .is_ok()
        {
            rx.await.unwrap_or_default()
        } else {
            grow_chat_state::ModelMetadata::default()
        }
    }
    /// Move a foreground bash command to background by tool_call_id.
    /// Returns `true` if a matching foreground process was found and unblocked.
    pub async fn background_foreground_command(&self, tool_call_id: &str) -> bool {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::BackgroundForegroundCommand {
                tool_call_id: tool_call_id.to_string(),
                respond_to: tx,
            })
            .is_err()
        {
            return false;
        }
        rx.await.unwrap_or(false)
    }
    /// Kill a background task by task_id.
    /// Routes through the session actor to the ToolBridge's TerminalBackend.
    pub async fn kill_background_task(
        &self,
        task_id: &str,
    ) -> Result<grow_tools::types::KillOutcome, String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::KillBackgroundTask {
                task_id: task_id.to_string(),
                respond_to: tx,
            })
            .is_err()
        {
            return Err("session not found".to_string());
        }
        rx.await.unwrap_or(Err("session actor died".to_string()))
    }
    pub async fn delete_scheduled_task(&self, task_id: &str) -> Result<bool, String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::DeleteScheduledTask {
                task_id: task_id.to_string(),
                respond_to: tx,
            })
            .is_err()
        {
            return Err("session not found".to_string());
        }
        rx.await.unwrap_or(Err("session actor died".to_string()))
    }
    /// Returns `true` if the session has work in flight: a running turn or
    /// queued inputs (`running_task.is_some() || !pending_inputs.is_empty()`).
    ///
    /// Used by the leader's idle-unload decision on client disconnect.
    /// Falls back to `true` (conservative: keep the session resident, never
    /// unload) if the actor is unreachable.
    pub async fn is_busy(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::IsBusy { respond_to: tx })
            .is_err()
        {
            return true;
        }
        rx.await.unwrap_or(true)
    }
    /// List all background tasks.
    /// Routes through the session actor to the ToolBridge's TerminalBackend.
    pub async fn list_tasks(&self) -> Option<Vec<grow_tools::types::TaskSnapshot>> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ListTasks { respond_to: tx })
            .is_err()
        {
            return None;
        }
        rx.await.unwrap_or(None)
    }
    /// Get hooks list for the pager modal.
    pub async fn get_hooks_list(&self) -> Option<xai_hooks_plugins_types::HooksListResponse> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetHooksList { respond_to: tx })
            .is_err()
        {
            return None;
        }
        rx.await.ok()
    }
    /// Execute a hooks management action from the pager modal.
    pub async fn execute_hooks_action(
        &self,
        action: xai_hooks_plugins_types::HooksAction,
    ) -> Option<xai_hooks_plugins_types::ActionOutcome> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::HooksAction {
                action,
                respond_to: tx,
            })
            .is_err()
        {
            return None;
        }
        rx.await.ok()
    }
    /// Execute a plugins management action from the pager modal.
    pub async fn execute_plugins_action(
        &self,
        action: xai_hooks_plugins_types::PluginsAction,
    ) -> Option<xai_hooks_plugins_types::ActionOutcome> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::PluginsAction {
                action,
                respond_to: tx,
            })
            .is_err()
        {
            return None;
        }
        rx.await.ok()
    }
    /// This session's plugin registry, including plugins loaded via `_meta.pluginDirs`.
    pub async fn plugins_list(
        &self,
    ) -> Option<std::sync::Arc<grow_agent::plugins::PluginRegistry>> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::PluginsList { respond_to: tx })
            .is_err()
        {
            return None;
        }
        rx.await.ok().flatten()
    }
    /// Snapshot the session's live MCP client pool for subagent inheritance.
    pub async fn snapshot_mcp_pool(&self) -> Option<crate::session::mcp_servers::SharedMcpPool> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::SnapshotMcpPool { respond_to: tx })
            .ok()?;
        rx.await.ok().flatten()
    }
    /// Snapshot the session's client-registered hooks for subagent inheritance. A dead actor
    /// or dropped reply fails open to no hooks, warned since it drops the inherited deny gate.
    pub(crate) async fn snapshot_client_hooks(&self) -> crate::extensions::hooks::ClientHooks {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::SnapshotClientHooks { respond_to: tx })
            .is_err()
        {
            tracing::warn!(
                "snapshot_client_hooks: session actor gone; subagent inherits no client hooks"
            );
            return Default::default();
        }
        rx.await.unwrap_or_else(|_| {
            tracing::warn!(
                "snapshot_client_hooks: reply dropped; subagent inherits no client hooks"
            );
            Default::default()
        })
    }
    /// Snapshot the session's resolved tool schema for verbatim-fork inheritance.
    /// A dead actor or dropped reply fails open to an empty list (child then builds
    /// its own toolset, same as a non-fork spawn).
    pub(crate) async fn snapshot_tool_definitions(&self) -> Vec<grow_sampling_types::ToolSpec> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::SnapshotToolDefinitions { respond_to: tx })
            .is_err()
        {
            tracing::warn!(
                "snapshot_tool_definitions: session actor gone; fork child inherits no parent tools"
            );
            return Vec::new();
        }
        rx.await.unwrap_or_else(|_| {
            tracing::warn!(
                "snapshot_tool_definitions: reply dropped; fork child inherits no parent tools"
            );
            Vec::new()
        })
    }
    pub(crate) async fn workflow_catalog_state(&self) -> (bool, bool) {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetWorkflowCatalogState { respond_to: tx })
            .is_err()
        {
            return (false, false);
        }
        rx.await.unwrap_or((false, false))
    }
    pub(crate) async fn list_available_commands(
        &self,
    ) -> crate::session::slash_commands::ListCommandsResponse {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ListAvailableCommands { respond_to: tx })
            .is_err()
        {
            return crate::session::slash_commands::ListCommandsResponse::default();
        }
        rx.await
            .unwrap_or_else(|_| crate::session::slash_commands::ListCommandsResponse::default())
    }
    /// Replace the live session's client-registered hooks (see `SessionCommand::SetClientHooks`).
    pub(crate) fn set_client_hooks(&self, hooks: crate::extensions::hooks::ClientHooks) {
        let _ = self.cmd_tx.send(SessionCommand::SetClientHooks { hooks });
    }
    pub async fn get_mcp_status(&self) -> crate::extensions::mcp::McpStatusSnapshot {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetMcpStatus { respond_to: tx })
            .is_err()
        {
            return Default::default();
        }
        rx.await.unwrap_or_default()
    }
    pub async fn toggle_mcp_server(
        &self,
        server_name: String,
        enabled: bool,
        server_config: Option<agent_client_protocol::McpServer>,
    ) -> Result<(), agent_client_protocol::Error> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ToggleMcpServer {
                server_name,
                enabled,
                server_config,
                respond_to: tx,
            })
            .is_err()
        {
            return Err(agent_client_protocol::Error::internal_error().data("session closed"));
        }
        rx.await
            .map_err(|_| agent_client_protocol::Error::internal_error().data("session closed"))?
    }
    pub async fn toggle_mcp_tool(
        &self,
        server_name: String,
        tool_name: String,
        enabled: bool,
    ) -> Result<(), agent_client_protocol::Error> {
        self.toggle_mcp_tool_with_source(server_name, tool_name, enabled, false)
            .await
    }
    pub async fn toggle_managed_gateway_tool(
        &self,
        server_name: String,
        tool_name: String,
        enabled: bool,
    ) -> Result<(), agent_client_protocol::Error> {
        self.toggle_mcp_tool_with_source(server_name, tool_name, enabled, true)
            .await
    }
    async fn toggle_mcp_tool_with_source(
        &self,
        server_name: String,
        tool_name: String,
        enabled: bool,
        is_managed_gateway: bool,
    ) -> Result<(), agent_client_protocol::Error> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ToggleMcpTool {
                server_name,
                tool_name,
                enabled,
                is_managed_gateway,
                respond_to: tx,
            })
            .is_err()
        {
            return Err(agent_client_protocol::Error::internal_error().data("session closed"));
        }
        rx.await
            .map_err(|_| agent_client_protocol::Error::internal_error().data("session closed"))?
    }
    pub async fn managed_gateway_disabled_tool_names(&self) -> HashMap<String, HashSet<String>> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetManagedGatewayDisabledTools { respond_to: tx })
            .is_err()
        {
            return HashMap::new();
        }
        rx.await.unwrap_or_default()
    }
    pub async fn call_mcp_tool(
        &self,
        server_name: String,
        server_url: Option<String>,
        tool_name: String,
        arguments: serde_json::Value,
    ) -> Result<crate::extensions::mcp::McpCallResponse, String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::CallMcpTool {
                server_name,
                server_url,
                tool_name,
                arguments,
                respond_to: tx,
            })
            .is_err()
        {
            return Err("session closed".to_string());
        }
        rx.await
            .unwrap_or_else(|_| Err("session closed".to_string()))
    }
    pub async fn read_mcp_resource(
        &self,
        server_name: String,
        uri: String,
    ) -> Result<crate::extensions::mcp::McpReadResourceResponse, String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ReadMcpResource {
                server_name,
                uri,
                respond_to: tx,
            })
            .is_err()
        {
            return Err("session closed".to_string());
        }
        rx.await
            .unwrap_or_else(|_| Err("session closed".to_string()))
    }
    /// Emit a PluginUpdatesInstalled notification to the session.
    /// Fire-and-forget — no response expected.
    pub async fn notify_plugin_updates(&self, updates: Vec<(String, String, String)>) {
        let _ = self
            .cmd_tx
            .send(SessionCommand::NotifyPluginUpdates { updates });
    }
}
