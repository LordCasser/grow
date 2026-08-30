//! `SessionHandle` — the `Clone + Send` proxy for interacting with a session actor.
//!
//! Callers hold a `SessionHandle` and send `SessionCommand` messages via the
//! internal channel. Extracted from the actor module to keep the actor
//! implementation focused on behaviour.
use super::commands::SessionCommand;
use super::persistence::PersistenceMsg;
use acp_transport::protocol as acp;
use hunk_tracker::HunkTrackerHandle;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone)]
pub(crate) struct SessionModelRouteSnapshot {
    pub(crate) revision: u64,
    /// Canonical catalog identity (`provider/model`).
    pub(crate) model_id: crate::agent::models::ModelId,
    /// Provider transport configuration; `model` is the wire model only.
    pub(crate) sampling_config: sampler::SamplerConfig,
}

/// Actor-committed model route shared with read-only session consumers. The
/// catalog identity and provider-facing sampler configuration always move as
/// one revision, so UI, reload, and subagent inheritance cannot splice axes
/// from different model selections.
#[derive(Clone)]
pub(crate) struct SessionModelRoute(std::sync::Arc<parking_lot::RwLock<SessionModelRouteSnapshot>>);

impl SessionModelRoute {
    pub(crate) fn new(
        model_id: crate::agent::models::ModelId,
        sampling_config: sampler::SamplerConfig,
    ) -> Self {
        Self(std::sync::Arc::new(parking_lot::RwLock::new(
            SessionModelRouteSnapshot {
                revision: 0,
                model_id,
                sampling_config,
            },
        )))
    }

    pub(crate) fn snapshot(&self) -> SessionModelRouteSnapshot {
        self.0.read().clone()
    }

    pub(crate) fn replace(
        &self,
        model_id: crate::agent::models::ModelId,
        sampling_config: sampler::SamplerConfig,
    ) -> SessionModelRouteSnapshot {
        let mut route = self.0.write();
        route.revision = route.revision.saturating_add(1);
        route.model_id = model_id;
        route.sampling_config = sampling_config;
        route.clone()
    }
}

#[derive(Clone)]
struct SessionAgentProfileSnapshot {
    name: String,
    subagent_filter: agent::config::SubagentFilter,
}

/// Actor-committed Agent identity shared by every clone of a session handle.
/// Nested delegation reads this object, so an Agent switch cannot leave a
/// child runtime enforcing the profile that existed when its handle was first
/// cloned.
#[derive(Clone)]
pub(crate) struct SessionAgentProfile(
    std::sync::Arc<parking_lot::RwLock<SessionAgentProfileSnapshot>>,
);

impl SessionAgentProfile {
    pub(crate) fn new(name: String, subagent_filter: agent::config::SubagentFilter) -> Self {
        Self(std::sync::Arc::new(parking_lot::RwLock::new(
            SessionAgentProfileSnapshot {
                name,
                subagent_filter,
            },
        )))
    }

    pub(crate) fn name(&self) -> String {
        self.0.read().name.clone()
    }

    pub(crate) fn subagent_filter(&self) -> agent::config::SubagentFilter {
        self.0.read().subagent_filter.clone()
    }

    pub(crate) fn replace(&self, name: String, subagent_filter: agent::config::SubagentFilter) {
        *self.0.write() = SessionAgentProfileSnapshot {
            name,
            subagent_filter,
        };
    }
}
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
/// Handle for interacting with a session actor.
/// Note: Permission event receivers are returned separately from `spawn_session_actor`
/// and should be stored/managed by the caller.
#[derive(Clone)]
pub struct SessionHandle {
    /// External ownership lease. Internal actor bridges may retain command
    /// senders, but they do not retain this lease; dropping the last real
    /// handle therefore requests deterministic teardown instead of waiting
    /// for a sender cycle to disappear.
    pub(crate) lifecycle_owner: std::sync::Arc<SessionLifecycleOwner>,
    pub cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Root Goal activity/accounting route. Nested children are re-parented to
    /// the lifecycle root and inherit this exact handle.
    pub(crate) goal_usage_window: crate::session::actor::goal_support::GoalUsageWindow,
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
    /// Permission response deadline selected when this session was created.
    /// Child sessions inherit it together with the shared permission manager.
    pub permission_prompt_timeout: std::time::Duration,
    /// Handle to the hunk tracker for this session
    pub hunk_tracker_handle: HunkTrackerHandle,
    /// Actor-based chat state handle — lets callers inspect final conversation state.
    pub chat_state_handle: chat_state::ChatStateHandle,
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
    pub(crate) model_route: SessionModelRoute,
    /// Canonical permission mode for this session.
    pub permission_mode: crate::util::config::PermissionMode,
    /// Explicit origin client metadata captured when the session was created.
    /// Used for per-session User-Agent rendering and for scoping leader-mode
    /// client behaviors like always-approve broadcasts.
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
    pub behavior: std::sync::Arc<parking_lot::Mutex<crate::session::behavior::BehaviorCoordinator>>,
    /// Canonical Workflow Run state, shared with child admission so a
    /// Workflow-owned Task resolves the Run's frozen sampler route rather
    /// than the mutable process catalog.
    pub(crate) workflow_tracker:
        std::sync::Arc<parking_lot::Mutex<crate::session::workflow::tracker::WorkflowTracker>>,
    /// Debug flag: when set to `true`, the next turn unconditionally triggers
    /// auto-compaction regardless of context window usage. Consumed (reset to
    /// `false`) atomically on use via `compare_exchange`.
    /// Set via `grow/debug/arm_auto_compact`.
    pub force_compact: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub permission_handle: workspace::permission::PermissionHandle,
    /// Immutable initial authority this session may pass to a nested child.
    /// `None` identifies a primary/root session; runtime grants are held in the
    /// child-local capability state and never update this ceiling.
    pub(crate) delegable_capability_ceiling:
        Option<super::subagent_capability::DelegableCapabilityCeiling>,
    /// Canonical Agent identity and nested-delegation filter. This is shared
    /// with the actor and every runtime handle clone.
    pub(crate) agent_profile: SessionAgentProfile,
    /// Present when this child is pinned to an immutable Workflow Run route.
    /// Explicit controls remain legal; automatic catalog fanout excludes it.
    pub(crate) workflow_run_id: Option<String>,
    /// Session-authoritative plugin snapshot shared with reload and Agent
    /// rebuild. Per-session plugin directories are intentionally absent from
    /// the process-global registry.
    pub(crate) plugin_registry: crate::session::workflow::tracker::SharedWorkflowPluginRegistry,
    /// Hook registry for this session (snapshot from spawn time).
    pub hook_registry: Option<std::sync::Arc<::hooks::discovery::HookRegistry>>,
    /// Typed workspace operations handle (agent sessions use local ops).
    pub workspace_ops: workspace::WorkspaceOps,
    /// Terminal backend for this session. Subagents inherit the parent's
    /// backend so background tasks and monitors survive the subagent's exit.
    pub terminal_backend: Option<std::sync::Arc<dyn tools::computer::types::TerminalBackend>>,
    /// Notification handle for this session's tool bridge. Subagents use
    /// this to reparent surviving tasks' notification handles on exit so
    /// events route to the parent's notification bridge.
    pub tools_notification_handle: Option<tools::notification::types::ToolNotificationHandle>,
    /// Scheduler handle for this session. Subagents inherit the parent's
    /// handle so scheduled tasks survive the subagent's exit.
    pub scheduler_handle:
        Option<tools::implementations::grow_build::scheduler::types::SchedulerHandle>,
}

pub(crate) struct SessionLifecycleOwner {
    cmd_tx: mpsc::WeakUnboundedSender<SessionCommand>,
}

impl SessionLifecycleOwner {
    pub(crate) fn new(cmd_tx: &mpsc::UnboundedSender<SessionCommand>) -> Self {
        Self {
            cmd_tx: cmd_tx.downgrade(),
        }
    }
}

impl Drop for SessionLifecycleOwner {
    fn drop(&mut self) {
        if let Some(cmd_tx) = self.cmd_tx.upgrade() {
            let _ = cmd_tx.send(SessionCommand::Shutdown);
        }
    }
}
impl SessionHandle {
    /// Last assistant `model_id` / `model_fingerprint` in conversation (global, not turn-scoped).
    pub(crate) async fn get_model_metadata(&self) -> chat_state::ModelMetadata {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::GetModelMetadata { responds_to: tx })
            .is_ok()
        {
            rx.await.unwrap_or_default()
        } else {
            chat_state::ModelMetadata::default()
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
    ) -> Result<tools::types::KillOutcome, String> {
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
    /// Ask the actor to atomically unload itself if it owns no live work.
    /// Returns `true` once the actor has latched termination and accepted the
    /// unload. Physical teardown may continue after the acknowledgement;
    /// actor failure before acceptance is conservative and keeps the handle.
    pub async fn unload_if_idle(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::UnloadIfIdle { respond_to: tx })
            .is_err()
        {
            return false;
        }
        rx.await.unwrap_or(false)
    }
    /// List all background tasks.
    /// Routes through the session actor to the ToolBridge's TerminalBackend.
    pub async fn list_tasks(&self) -> Option<Vec<tools::types::TaskSnapshot>> {
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
    pub async fn get_hooks_list(&self) -> Option<extension_types::HooksListResponse> {
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
        action: extension_types::HooksAction,
    ) -> Option<extension_types::ActionOutcome> {
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
        action: extension_types::PluginsAction,
    ) -> Option<extension_types::ActionOutcome> {
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
    pub async fn plugins_list(&self) -> Option<std::sync::Arc<agent::plugins::PluginRegistry>> {
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
    pub(crate) async fn execute_slash_command(
        &self,
        invocation: crate::session::HostCommandInvocation,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCommand::ExecuteSlashCommand {
                invocation,
                respond_to: tx,
            })
            .map_err(|_| "session actor is not available".to_string())?;
        rx.await
            .map_err(|_| "session actor stopped before executing the command".to_string())?
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
        server_config: Option<agent_client_protocol::schema::v1::McpServer>,
    ) -> Result<(), agent_client_protocol::schema::v1::Error> {
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
            return Err(
                agent_client_protocol::schema::v1::Error::internal_error().data("session closed")
            );
        }
        rx.await.map_err(|_| {
            agent_client_protocol::schema::v1::Error::internal_error().data("session closed")
        })?
    }
    pub async fn toggle_mcp_tool(
        &self,
        server_name: String,
        tool_name: String,
        enabled: bool,
    ) -> Result<(), agent_client_protocol::schema::v1::Error> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SessionCommand::ToggleMcpTool {
                server_name,
                tool_name,
                enabled,
                respond_to: tx,
            })
            .is_err()
        {
            return Err(
                agent_client_protocol::schema::v1::Error::internal_error().data("session closed")
            );
        }
        rx.await.map_err(|_| {
            agent_client_protocol::schema::v1::Error::internal_error().data("session closed")
        })?
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn last_external_owner_requests_shutdown_despite_internal_senders() {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let _internal_sender = cmd_tx.clone();
        let owner = std::sync::Arc::new(SessionLifecycleOwner::new(&cmd_tx));
        let second_handle_owner = owner.clone();
        drop(owner);
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        drop(second_handle_owner);
        assert!(matches!(cmd_rx.try_recv(), Ok(SessionCommand::Shutdown)));
    }
}
