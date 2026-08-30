//! Shell child runtime adapter and presentation.
//!
//! Lifecycle state and command scheduling live in the shared
//! `tools` coordinator actor. This module keeps shell-specific
//! child-session construction, ACP presentation, and persistence.
//!
//! ## Design
//!
//! - `run_shell_child()` runs one shell child behind `ChildRunner`.
//! - Pending/active/completed, waiters, deadlines, and cancellation are actor-owned.
//! - Child sessions share the parent's hunk tracker, filesystem, terminal, and env
//!   so that edits, bash commands, and file reads go through the same backends.
use crate::agent::config::{resolve_credentials, sampling_config_for_model};
use crate::agent::subagent::resolution::ResumeSourceData;
use crate::extensions::notification::{SessionNotification, SessionUpdate};
use crate::session::events::CancellationCategory;
use crate::session::{
    self, SessionCommand, SessionHandle, SessionThread,
    commands::{PromptCompletionKind, PromptTurnResult as SubagentPromptTurnResult},
    fs_watch::FsWatchCapabilities,
    info::Info as SessionInfo,
};
use crate::terminal::AsyncTerminalRunner;
use crate::tools::ToolContext;
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use acp_transport::protocol as acp;
use agent::config::McpInheritance;
use hunk_tracker::HunkTrackerHandle;
use sampling_types::conversation::ConversationItem;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tools::implementations::grow_build::task::coordinator::{
    ChildCompletion, ChildControl, ChildReporter, ChildRunOutput, LocalBoxFuture, StartedChild,
    SubagentProgress,
};
use tools::implementations::grow_build::task::types::*;
use tools::types::tool::ToolKind;
use workspace::file_system::AsyncFileSystem;
mod handle_request;
pub(crate) mod resolution;
pub(crate) use handle_request::run_shell_child;
/// How the child session's initial context was bootstrapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InitialContextSource {
    /// Fresh session — no inherited history.
    New,
    /// Parent history as `<background_context>` (harness-only chat-prefix fork).
    Forked,
    /// Resumed from a previously completed peer subagent. The child inherits
    /// the source's raw transcript and model. System prompt, tool runtime, and
    /// prompt context are freshly rendered from the current agent definition.
    Resumed,
}
/// Captured parent-side tier inputs for resolving
/// `auto_compact_threshold_percent` once the subagent's actual model id is
/// known. Stored on [`SubagentSpawnContext`] so the resolver can run at
/// spawn time and the per-model lookup honors the SUBAGENT's model rather
/// than the parent's.
#[derive(Debug, Clone, Default)]
pub(crate) struct AutoCompactThresholdTiers {
    /// `cfg.session.auto_compact_threshold_percent` (user global TOML).
    pub user_session: Option<u8>,
    /// Subset of `cfg.config_models` whose `auto_compact_threshold_percent`
    /// is set, keyed by the model entry's id (the table key in
    /// `[provider.<id>.models.<model>]`). Looked up by the subagent's resolved model id at
    /// spawn time so user per-model overrides for the subagent's model are
    /// honored (not just the parent's).
    pub user_per_model: std::collections::HashMap<String, u8>,
    /// `cfg.remote_settings.auto_compact_threshold_percent` (GB global).
    pub remote_global: Option<u8>,
}
impl AutoCompactThresholdTiers {
    /// Slice the parent's `Config` into the four tier inputs we'll resolve
    /// against later. Only fields relevant to the auto-compact threshold
    /// are captured; the parent's `Config` is not held by reference.
    pub fn capture(cfg: &crate::agent::config::Config) -> Self {
        let user_per_model = cfg
            .config_models
            .iter()
            .filter_map(|(k, v)| v.auto_compact_threshold_percent.map(|t| (k.clone(), t)))
            .collect();
        Self {
            user_session: cfg.session.auto_compact_threshold_percent,
            user_per_model,
            remote_global: cfg
                .remote_settings
                .as_ref()
                .and_then(|r| r.auto_compact_threshold_percent),
        }
    }
}
/// Everything the coordinator needs from MvpAgent to spawn a child session.
/// Avoids passing `&MvpAgent` (which would require the coordinator to know
/// about the full agent struct). Built by `MvpAgent::build_subagent_spawn_context()`.
pub(crate) struct SubagentSpawnContext {
    /// Live child-session control routes. Child sessions intentionally stay
    /// out of the primary resident-session roster, but ACP model/Agent
    /// selection still needs to address their actor by exact session id.
    pub active_child_sessions: ActiveChildSessions,
    /// Parent's LSP runtime — inherited via ToolContext, same as fs/terminal.
    pub lsp: Option<std::sync::Arc<dyn tools::implementations::lsp::LspBackend>>,
    /// Root session's process scope, inherited so the subagent's own child
    /// processes are reaped when the parent session closes. It is the root's
    /// (not an intermediate parent's) because tools task/coordinator.rs
    /// `handle_command`'s Spawn arm re-parents nested Spawn requests to the root
    /// parent, so every subagent resolves back to the root session.
    pub process_scope: Option<tty_utils::ProcessScope>,
    /// Parent's client-registered hooks, inherited so the subagent's tool calls hit the
    /// same PreToolUse gate and its events fire the same observe hooks over the parent's
    /// connection. Empty when the parent has none. Filled by the coordinator after the
    /// context is built (an async snapshot from the parent session actor).
    pub client_hooks: crate::extensions::hooks::ClientHooks,
    pub sampling_config: sampler::SamplerConfig,
    /// The staging auth header value propagated from the parent. Used
    /// when materialising subagent `SamplerConfig`s for auth-flow tracking
    /// and for `inject_url_derived_headers` in the construction helpers.
    pub alpha_test_key: Option<String>,
    pub auth_method_id: acp::AuthMethodId,
    pub model_id: crate::agent::models::ModelId,
    pub parent_cwd: PathBuf,
    pub parent_session_id: String,
    /// Session that directly owns this delegation's security boundary. It is
    /// distinct from the lifecycle-root `parent_session_id` for nested Tasks.
    pub security_parent_session_id: String,
    /// Parent permission mode inherited at child spawn.
    pub permission_mode: crate::util::config::PermissionMode,
    pub subagent_event_tx: mpsc::UnboundedSender<SubagentEvent>,
    pub parent_depth: u32,
    pub subagents_max_depth: u32,
    /// Inference idle timeout (secs), resolved from the parent's model config at spawn-context creation time.
    pub inference_idle_timeout_secs: u64,
    /// Permission response deadline inherited from the root session.
    pub permission_prompt_timeout: std::time::Duration,
    /// Global child permission route resolved from `[subagents]`.
    pub subagent_permission_mode: workspace::permission::types::RequestPermissionMode,
    /// Immutable delegation ceiling copied from the immediate security
    /// parent. Root sessions have no ceiling; nested children must stay within
    /// this value before any worktree/session side effect is created.
    pub parent_capability_ceiling:
        Option<crate::session::subagent_capability::DelegableCapabilityCeiling>,
    /// Tier inputs for resolving `auto_compact_threshold_percent` at
    /// spawn time — once the subagent's actual model id is known.
    /// Lazy because the subagent may be assigned a different model from
    /// the parent (via `[subagents.models]`);
    /// we want the resolver's per-model
    /// tiers to be looked up against the SUBAGENT's model, not the
    /// parent's. Call [`Self::resolve_auto_compact_threshold_percent`]
    /// once the subagent's `effective_sampling_config.model` is known.
    pub auto_compact_threshold_tiers: AutoCompactThresholdTiers,
    /// Parent's hunk tracker handle — cheap Clone, backed by an mpsc channel
    /// to the parent's HunkTrackerActor. Subagent edits are attributed to
    /// the same hunk tracker so the parent sees all file changes.
    pub hunk_tracker_handle: HunkTrackerHandle,
    /// Parent's hunk-tracking gate, inherited so a disabled parent's subagent
    /// also skips the per-event forward instead of paying it into a noop handle.
    pub hunk_tracking_enabled: bool,
    /// Parent's filesystem implementation (LocalFs or AcpSessionFs).
    /// Shared so the child reads/writes the same working tree.
    pub fs: Arc<dyn AsyncFileSystem>,
    /// Parent's terminal runner — shared so bash commands run in the
    /// same terminal environment (env vars, cwd, color settings).
    pub terminal: Arc<dyn AsyncTerminalRunner>,
    /// Parent's terminal backend — shared so background tasks, monitors, and
    /// scheduled tasks survive subagent exit. When `Some`, the subagent session
    /// reuses this backend instead of creating a new `LocalTerminalBackend`.
    pub parent_terminal_backend: Option<Arc<dyn tools::computer::types::TerminalBackend>>,
    /// Parent's notification handle for reparenting on subagent exit.
    /// When a subagent exits, its surviving tasks (monitors, bg commands)
    /// need their notification handles swapped to this so events route
    /// to the parent's notification bridge.
    pub parent_notification_handle: Option<tools::notification::types::ToolNotificationHandle>,
    /// Parent's scheduler handle. When `Some`, the subagent reuses the
    /// parent's scheduler actor so scheduled tasks survive subagent exit.
    pub parent_scheduler_handle:
        Option<tools::implementations::grow_build::scheduler::types::SchedulerHandle>,
    /// Parent's session environment variables (.envrc + color settings).
    /// Shared so the child inherits the same env without re-loading.
    pub session_env: Arc<HashMap<String, String>>,
    /// Parent's memory config — shared so the child can access the same
    /// cross-session memory store.
    pub memory_config: Option<crate::config::MemoryConfig>,
    /// Resolved config for web fetch.
    pub web_fetch_config: tools::implementations::grow_build::web_fetch::WebFetchConfig,
    /// Resolved config for the deploy service.
    pub app_builder_deployer_config:
        tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig,
    /// Whether the write_file tool is enabled.
    pub write_file_enabled: bool,
    /// Whether goal mode (`/goal`) is enabled.
    pub goal_enabled: bool,
    pub background_workflows_enabled: bool,
    /// Whether the `ask_user_question` tool is exposed to this subagent,
    /// inherited from the parent session (see `build_subagent_spawn_context`).
    pub ask_user_question_enabled: bool,
    /// Parent session command channel. Carries lifecycle notifications the
    /// parent persists (`SubagentSpawned` / `SubagentFinished`) and terminal
    /// usage-ledger folds. Goal accounting uses `goal_usage_window` instead.
    pub parent_cmd_tx: Option<mpsc::UnboundedSender<SessionCommand>>,
    /// Root Goal activity/accounting route shared by the complete descendant
    /// tree. Unlike delegation ownership, this charges every model call
    /// settled during an Active Goal window.
    pub goal_usage_window: crate::session::actor::goal_support::GoalUsageWindow,
    /// Parent session info — used to locate parent session directory.
    pub parent_session_info: Option<SessionInfo>,
    /// Session info for the immediate delegation parent. Fork context and
    /// prompt-blob freezing use this source; lifecycle persistence continues
    /// to use `parent_session_info`.
    pub delegation_session_info: Option<SessionInfo>,
    /// Parent session's ChatStateHandle — used to read the actual live
    /// lifecycle Timeline and fork source. Nested Tasks keep this bound to the
    /// lifecycle root even though their delegation authority comes from the
    /// immediate parent.
    pub parent_chat_state: Option<chat_state::ChatStateHandle>,
    /// Root session Workflow state. Only Workflow-owned child requests read
    /// this handle, selecting the immutable sampler snapshot by `run_id`.
    pub workflow_tracker: Option<
        std::sync::Arc<parking_lot::Mutex<crate::session::workflow::tracker::WorkflowTracker>>,
    >,
    /// Immediate delegation parent's ChatStateHandle. This is deliberately
    /// separate from `parent_chat_state`: only credentials follow the
    /// security parent, while lifecycle facts remain rooted in the canonical
    /// parent Timeline.
    pub delegation_chat_state: Option<chat_state::ChatStateHandle>,
    /// Parent session's resolved turn limit, for subagent inheritance.
    pub parent_max_turns: Option<usize>,
    /// All available models for resolving model IDs from overrides.
    pub available_models: indexmap::IndexMap<String, crate::agent::config::ModelEntry>,
    /// Generation captured atomically with `available_models`.
    pub catalog_revision: u64,
    /// Per-subagent model ID overrides from config.toml `[subagents.models]`.
    pub subagent_model_overrides: std::collections::HashMap<String, String>,
    /// Per-subagent enable/disable toggles from config.toml `[subagents.toggle]`.
    /// Omitted agents default to enabled (`true`).
    pub subagent_toggle: std::collections::HashMap<String, bool>,
    /// Active parent Agent's task-tool restriction, composed with the global
    /// toggle at validation and spawn time.
    pub subagent_filter: agent::config::SubagentFilter,
    /// Remote settings snapshot from the parent session.
    pub remote_settings: Option<crate::util::config::RemoteSettings>,
    /// Inherited `--laziness-debug-log <path>` from the parent session.
    /// Subagent classifier fires append to the same log file. `None`
    /// when the parent did not enable debug mode.
    pub laziness_debug_log: Option<std::path::PathBuf>,
    /// Whether tools should respect `.gitignore` patterns.
    /// Inherited from the parent session.
    pub respect_gitignore: bool,
    /// Whether to enrich path-not-found errors with hints.
    /// Inherited from the parent session.
    pub path_not_found_hints: bool,
    /// Plugin registry for plugin-aware agent lookup.
    pub plugin_registry: Option<std::sync::Arc<agent::plugins::PluginRegistry>>,
    /// Shared models manager for etag-triggered refresh.
    pub models_manager: crate::agent::models::ModelsManager,
    /// Pre-resolved file tool overrides (hashline vs standard) from the parent.
    /// `None` means use the standard (default) file tools.
    pub file_tool_overrides: Option<Vec<tools::registry::types::ToolConfig>>,
    /// Parent session's agent config snapshot.
    pub agent_config: Option<crate::agent::config::Config>,
    pub hook_registry: Option<std::sync::Arc<::hooks::discovery::HookRegistry>>,
    pub permission_handle: Option<workspace::permission::PermissionHandle>,
    pub worktree_type: crate::util::config::WorktreeType,
    pub image_description_model: Option<String>,
    /// Dual-mode workspace operations handle.
    pub workspace_ops: workspace::WorkspaceOps,
    /// Parent session's agent name (e.g. "grow-build").
    pub parent_agent_name: Option<String>,
    /// Snapshot of the parent session's MCP client pool at spawn time.
    pub parent_mcp_pool: Option<crate::session::mcp_servers::SharedMcpPool>,
    /// Pre-discovered skills from the parent session, captured at spawn time.
    pub parent_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
    /// Parent's skills config for the child's SkillManager.
    pub parent_skills_config: agent::prompt::skills::SkillsConfig,
    /// Resolved name of the `BackgroundTaskAction` tool in the parent's toolset.
    pub task_output_tool_name: Option<String>,
    /// Parent's live Goal wait-race marker. The completion producer uses it
    /// only to preserve results displaced by steering; Timeline owns delivery.
    pub goal_loop_active: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) type ActiveChildSessions = std::rc::Rc<
    std::cell::RefCell<
        std::collections::HashMap<agent_client_protocol::schema::v1::SessionId, SessionHandle>,
    >,
>;
impl SubagentSpawnContext {
    pub fn resolve_inference_idle_timeout_secs(&self, subagent_model_id: &str) -> u64 {
        let per_model = crate::agent::config::find_model_by_catalog_id(
            &self.available_models,
            subagent_model_id,
        )
        .and_then(|entry| entry.info.inference_idle_timeout_secs);
        let remote = self
            .remote_settings
            .as_ref()
            .and_then(|settings| settings.inference_idle_timeout_secs);
        per_model.or(remote).unwrap_or(600).max(10)
    }

    /// Resolve `auto_compact_threshold_percent` for the subagent's actual
    /// model id (the one selected by `resolve_subagent_sampling_config`,
    /// not the parent's). Walks the same precedence as the main session's
    /// resolver: env > provider model > user [session] > GB per-model > GB
    /// global > 85.
    ///
    /// The GB per-model tier is read from `available_models` (the same
    /// catalog used to pick the subagent's `SamplerConfig`); user TOML and
    /// GB global tiers are sourced from the parent's snapshot captured at
    /// spawn-context build time.
    pub fn resolve_auto_compact_threshold_percent(&self, subagent_model_id: &str) -> u8 {
        let gb_per_model = crate::agent::config::find_model_by_catalog_id(
            &self.available_models,
            subagent_model_id,
        )
        .and_then(|e| e.info.auto_compact_threshold_percent);
        crate::util::config::resolve_auto_compact_threshold_percent_from_tiers(
            self.auto_compact_threshold_tiers
                .user_per_model
                .get(subagent_model_id)
                .copied(),
            self.auto_compact_threshold_tiers.user_session,
            gb_per_model,
            self.auto_compact_threshold_tiers.remote_global,
        )
    }
    /// Bind a spawned subagent by the parent session's `--tools` and
    /// `--disallowed-tools` restrictions. Permission ownership remains in the
    /// shared session PermissionManager.
    fn apply_session_cli_overrides(&self, def: &mut agent::config::AgentDefinition) {
        if let Some(ref cfg) = self.agent_config {
            cfg.cli_agent_overrides.apply_to_subagent_definition(def);
        }
    }
    /// Subagent verbatim-input flag, mirroring `Config::resolve_compaction_verbatim_input` (env > config > remote settings > default `true`).
    pub fn resolve_compaction_verbatim_input(&self) -> bool {
        crate::agent::config::BoolFlag::env("GROW_COMPACTION_VERBATIM_INPUT")
            .config(
                self.agent_config
                    .as_ref()
                    .and_then(|c| c.features.compaction_verbatim_input),
            )
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|r| r.compaction_verbatim_input),
            )
            .default(true)
            .resolve()
            .value
    }
    /// Subagent pre-prune flag, mirroring `Config::resolve_compaction_pre_prune`
    /// (env > config `[compaction] pre_prune` > remote settings > default `true`).
    pub fn resolve_compaction_pre_prune(&self) -> bool {
        crate::util::config::resolve_compaction_pre_prune_from(
            crate::agent::config::env_string(crate::util::config::ENV_COMPACTION_PRE_PRUNE)
                .as_deref(),
            self.agent_config
                .as_ref()
                .and_then(|c| c.compaction.pre_prune),
            self.remote_settings
                .as_ref()
                .and_then(|r| r.compaction_pre_prune),
        )
    }
    /// Subagent pre-prune per-item token budget, mirroring
    /// `Config::resolve_compaction_pre_prune_token_budget`; `None` derives the
    /// budget from the context window.
    pub fn resolve_compaction_pre_prune_token_budget(&self) -> Option<u64> {
        crate::util::config::resolve_compaction_pre_prune_token_budget_from(
            crate::agent::config::env_string(
                crate::util::config::ENV_COMPACTION_PRE_PRUNE_TOKEN_BUDGET,
            )
            .as_deref(),
            self.agent_config
                .as_ref()
                .and_then(|c| c.compaction.pre_prune_token_budget),
            self.remote_settings
                .as_ref()
                .and_then(|r| r.compaction_pre_prune_token_budget),
        )
    }
    /// Whether a completed subagent's worktree is snapshotted into a durable ref
    /// and its directory deleted. Resolution mirrors the other subagent gates
    /// (env > config > remote settings > default). Default `false` so it ships dark.
    pub fn resolve_subagent_worktree_snapshot_enabled(&self) -> bool {
        crate::agent::config::BoolFlag::env("GROW_SUBAGENT_WORKTREE_SNAPSHOT")
            .config(
                self.agent_config
                    .as_ref()
                    .and_then(|c| c.features.subagent_worktree_snapshot),
            )
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|r| r.subagent_worktree_snapshot_enabled),
            )
            .default(false)
            .resolve()
            .value
    }
    /// Per-tool params for the child's spawn. The ask_user_question timeout is
    /// session-level config, so it is resolved from the same tiers as the
    /// parent (env/user from disk; remote from the parent's snapshot) and follows
    /// the session into subagents. Bash stays
    /// on tool defaults, as before that knob existed.
    pub fn resolve_tool_params_json(
        &self,
    ) -> crate::session::agent_rebuild::ResolvedToolParamsJson {
        let params = crate::util::config::resolve_ask_user_question_params_from_disk();
        crate::session::agent_rebuild::ResolvedToolParamsJson {
            bash: None,
            ask_user_question: match serde_json::to_value(params) {
                Ok(serde_json::Value::Object(map)) => Some(map),
                _ => None,
            },
        }
    }
}
/// Shell runtime handle retained while a child is active.
pub(crate) struct ShellChildRuntime {
    pub child_handle: SessionHandle,
    pub _child_thread: SessionThread,
}
impl ChildControl for ShellChildRuntime {
    type ProgressFuture = LocalBoxFuture<SubagentProgress>;
    type SecurityContext = SessionHandle;

    fn security_context(&self) -> Self::SecurityContext {
        self.child_handle.clone()
    }

    fn progress(&self) -> Self::ProgressFuture {
        let signals = self.child_handle.signals_handle.clone();
        Box::pin(async move {
            let snapshot = signals.snapshot().await.unwrap_or_default();
            SubagentProgress {
                turn_count: snapshot.turn_count,
                tool_call_count: snapshot.tool_call_count,
                tokens_used: snapshot.context_tokens_used,
                context_window_tokens: snapshot.context_window_tokens,
                context_usage_pct: snapshot.context_window_usage,
                tools_used: snapshot.tools_used,
                error_count: snapshot.error_count,
            }
        })
    }
    fn cancel(&self) {
        let _ = self.child_handle.cmd_tx.send(SessionCommand::Cancel {
            cancel_subagents: true,
            kill_background_tasks: true,
            rewind_if_pristine: false,
            pause_goal: false,
            trigger: None,
        });
        let _ = self.child_handle.cmd_tx.send(SessionCommand::Shutdown);
    }
}
#[derive(Default)]
pub(crate) struct ShellCompletionData {
    parent_cmd_tx: Option<mpsc::UnboundedSender<SessionCommand>>,
    task_output_tool_name: Option<String>,
    goal_loop_active: Arc<std::sync::atomic::AtomicBool>,
    diagnostics_tokens: u64,
    spawned_notification_emitted: bool,
    persisted_output_ref: Option<String>,
    terminal_committed: bool,
    completion_receipt_admitted: bool,
}
impl ShellCompletionData {
    fn from_context(ctx: &SubagentSpawnContext) -> Self {
        Self {
            parent_cmd_tx: ctx.parent_cmd_tx.clone(),
            task_output_tool_name: ctx.task_output_tool_name.clone(),
            goal_loop_active: Arc::clone(&ctx.goal_loop_active),
            diagnostics_tokens: 0,
            spawned_notification_emitted: false,
            persisted_output_ref: None,
            terminal_committed: false,
            completion_receipt_admitted: false,
        }
    }
    pub(crate) fn persisted_output_ref(&self) -> Option<&str> {
        self.persisted_output_ref.as_deref()
    }
    fn set_persisted_output_ref(&mut self, output_ref: Option<String>) {
        self.persisted_output_ref = output_ref;
    }

    fn mark_terminal_committed(&mut self) {
        self.terminal_committed = true;
    }

    pub(crate) fn terminal_committed(&self) -> bool {
        self.terminal_committed
    }
}
pub(crate) struct SubagentPresentation {
    is_turn_active: Arc<std::sync::atomic::AtomicBool>,
}
impl SubagentPresentation {
    pub(crate) fn new() -> Self {
        Self {
            is_turn_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    pub(crate) fn turn_active_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.is_turn_active)
    }
}
pub(crate) fn present_child_completion(
    completion: ChildCompletion<ShellCompletionData>,
    gateway: &GatewaySender,
) {
    let ChildCompletion {
        request,
        result,
        completion_data,
        disposition,
    } = completion;
    if !completion_data.terminal_committed {
        tracing::error!(
            subagent_id = %request.id,
            "suppressing subagent completion projection without a canonical parent terminal"
        );
        return;
    }
    let parent_channel_open = completion_data
        .parent_cmd_tx
        .as_ref()
        .is_some_and(|tx| !tx.is_closed());
    let goal_loop_active = completion_data
        .goal_loop_active
        .load(std::sync::atomic::Ordering::Relaxed);
    let steering_wait_may_own_completion = goal_loop_active
        && disposition.backgrounded
        && disposition.waiter_delivered
        && request.surface_completion
        && !result.cancelled
        && !disposition.explicitly_killed;
    if parent_channel_open
        && !completion_data.completion_receipt_admitted
        && (disposition.should_surface || steering_wait_may_own_completion)
    {
        tracing::error!(
            subagent_id = %request.id,
            "subagent terminal reached presentation without its durable completion receipt"
        );
    }
    if completion_data.spawned_notification_emitted || request.run_in_background {
        emit_subagent_notification(
            gateway,
            &request.parent_session_id,
            SessionUpdate::SubagentFinished {
                subagent_id: request.id.clone(),
                child_session_id: result.child_session_id.clone(),
                status: result.status().to_owned(),
                error: result.error.clone(),
                tool_calls: result.tool_calls,
                turns: result.turns,
                duration_ms: result.duration_ms,
                tokens_used: result.total_tokens_used,
                output: result.success.then(|| result.output.to_string()),
            },
            completion_data.parent_cmd_tx.as_ref(),
        );
    }
}

/// The single terminal receipt producer used by Goal, ordinary background,
/// loop, and recovery paths. A send only transfers ownership to the actor
/// mailbox; the producer retires after the Timeline write is acknowledged.
/// Retrying the immutable subagent id/version is idempotent in the Timeline
/// fold. The durable parent terminal remains the recovery source when the
/// actor cannot admit a receipt within this bounded handoff.
async fn admit_notification_receipt(
    cmd_tx: &mpsc::UnboundedSender<SessionCommand>,
    subagent_id: &str,
    owner: chat_state::NotificationOwner,
    body: String,
) -> bool {
    const MAX_ATTEMPTS: usize = 3;

    for attempt in 1..=MAX_ATTEMPTS {
        let (respond_to, admitted) = oneshot::channel();
        if cmd_tx
            .send(SessionCommand::ReceiveNotification {
                source: chat_state::NotificationSource::SubagentCompleted {
                    subagent_id: subagent_id.to_owned(),
                    owner: owner.clone(),
                },
                source_version: chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                body: body.clone(),
                respond_to: Some(respond_to),
            })
            .is_err()
        {
            tracing::warn!(%subagent_id, "subagent completion delivery stopped because the parent mailbox closed");
            return false;
        }
        let error = match admitted.await {
            Ok(Ok(_)) => return true,
            Ok(Err(error)) => error,
            Err(error) => format!("acknowledgement dropped: {error}"),
        };
        if attempt == MAX_ATTEMPTS {
            tracing::error!(
                %subagent_id,
                %error,
                attempts = MAX_ATTEMPTS,
                "subagent completion receipt admission exhausted; durable terminal remains recoverable"
            );
            return false;
        }
        tracing::warn!(
            %subagent_id,
            %error,
            attempt,
            max_attempts = MAX_ATTEMPTS,
            "subagent completion receipt admission failed; retrying"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    unreachable!("the bounded receipt admission loop always returns")
}

async fn admit_subagent_completion_receipt(
    cmd_tx: &mpsc::UnboundedSender<SessionCommand>,
    subagent_id: &str,
    owner: &SubagentOwner,
    body: String,
) -> bool {
    let owner = match owner {
        SubagentOwner::Goal {
            goal_id,
            definition_revision,
        } => chat_state::NotificationOwner::Goal {
            goal_id: goal_id.clone(),
            definition_revision: *definition_revision,
        },
        SubagentOwner::Task | SubagentOwner::Workflow { .. } => {
            chat_state::NotificationOwner::Session
        }
    };
    admit_notification_receipt(cmd_tx, subagent_id, owner, body).await
}

/// Commit a child receipt before returning its terminal result to the
/// coordinator. Foreground/wait tool results consume this already-durable
/// receipt; background and displaced waits leave it for notification delivery.
/// This single barrier applies equally to Goal, ordinary, and loop children.
pub(super) async fn admit_completion_receipt_before_result(
    request: &SubagentRequest,
    result: &SubagentResult,
    completion_data: &mut ShellCompletionData,
) {
    if !request.surface_completion || result.cancelled || !completion_data.terminal_committed {
        return;
    }
    let Some(cmd_tx) = completion_data.parent_cmd_tx.as_ref() else {
        return;
    };
    let summary = tools::implementations::grow_build::task::completion_summary(request, result);
    let body = tools::reminders::task_completion::format_subagent_completion(
        &summary,
        completion_data.task_output_tool_name.as_deref(),
    );
    completion_data.completion_receipt_admitted =
        admit_subagent_completion_receipt(cmd_tx, &request.id, &request.owner, body).await;
}
/// Resolve the sampling config and model ID for a subagent.
///
/// Subagents inherit the parent session's model by default. An explicit
/// `[subagents.models]` entry can override that inheritance; Agent prompt
/// profiles never participate in model selection. Precedence:
///
///   1. `config.toml [subagents.models].{agent_name}` override. It must
///      resolve to a known model and applies unconditionally.
///
///   2. Inherit the parent session's actual live sampling config (from
///      `ChatStateHandle`).
///
/// Explicit pins apply regardless of which model the parent is on. An unknown
/// pin fails closed instead of silently changing the child to its parent's
/// current route.
///
/// NOTE: the host/runtime override (`effective_runtime.model`) is
/// applied by the caller (`run_shell_child`) BEFORE this function
/// runs, so it is not handled here.
///
/// NOTE: `agent_type` on the resolved model is intentionally ignored.
/// Subagent prompt/toolset is always determined by the `AgentDefinition`, not
/// the model. See design spec "Behavioral Rules section 3".
async fn resolve_subagent_sampling_config(
    agent_name: &str,
    ctx: &SubagentSpawnContext,
) -> Result<(sampler::SamplerConfig, crate::agent::models::ModelId), String> {
    let (parent_config, parent_mid) = read_parent_sampling_config(ctx).await;
    if let Some(model_id) = ctx.subagent_model_overrides.get(agent_name) {
        let Some((config, canonical_id)) = resolve_model_override_to_config(model_id, ctx) else {
            return Err(format!(
                "Configured model override '{model_id}' for subagent '{agent_name}' is not present in the model catalogue"
            ));
        };
        log_subagent_model_resolution(
            agent_name,
            "config_override",
            &config,
            &canonical_id,
            &parent_config,
        );
        return Ok((config, canonical_id));
    }
    log_subagent_model_resolution(
        agent_name,
        "inherit_parent",
        &parent_config,
        &parent_mid,
        &parent_config,
    );
    Ok((parent_config, parent_mid))
}
/// Resolve a subagent's effective sampling config + model id, honoring the
/// model-resolution precedence (Key Decision #16).
///
/// An explicit `runtime_override_model` — a host-owned Goal-stage model or an
/// explicit Task runtime override — is resolved HERE, BEFORE
/// [`resolve_subagent_sampling_config`] (where the user `[subagents.models]`
/// pin applies). So an explicit host/runtime override WINS
/// over a user per-agent pin. An override that does not resolve to a known
/// model fails closed; silently inheriting a different model would violate a
/// Goal runtime route. Workflow-owned requests bypass this function and are
/// resolved from their immutable Run sampler catalog. `None` (inherit) hands
/// precedence back to the pin path entirely (pin > inherit).
///
/// Extracted from `run_shell_child` so the precedence is unit-testable
/// without spawning a child session.
async fn resolve_effective_model_config(
    runtime_override_model: Option<&str>,
    subagent_type: &str,
    ctx: &SubagentSpawnContext,
) -> Result<(sampler::SamplerConfig, crate::agent::models::ModelId), String> {
    if let Some(model_id) = runtime_override_model {
        if let Some(resolved) = resolve_model_override_to_config(model_id, ctx) {
            return Ok(resolved);
        }
        return Err(format!(
            "Runtime model override '{model_id}' is not present in the model catalogue"
        ));
    }
    resolve_subagent_sampling_config(subagent_type, ctx).await
}
/// Truncate an API key to a safe prefix for logging.
fn key_prefix(key: &Option<String>) -> String {
    match key {
        Some(k) => {
            let len = k.len().min(8);
            k[..len].to_string()
        }
        None => "<none>".to_string(),
    }
}
/// Emit a unified log entry recording which model and credentials a subagent
/// resolved to, and how they compare to the parent's.
fn log_subagent_model_resolution(
    agent_name: &str,
    priority: &str,
    resolved: &sampler::SamplerConfig,
    resolved_id: &crate::agent::models::ModelId,
    parent: &sampler::SamplerConfig,
) {
    let child_key = key_prefix(&resolved.api_key);
    let parent_key = key_prefix(&parent.api_key);
    let keys_match = resolved.api_key == parent.api_key;
    ::diagnostics::unified_log::debug(
        "subagent model resolved",
        None,
        Some(serde_json::json!({
            "agent": agent_name,
            "priority": priority,
            "child_model": resolved_id.0.as_ref(),
            "child_base_url": &resolved.base_url,
            "child_key_prefix": child_key,
            "parent_model": &parent.model,
            "parent_base_url": &parent.base_url,
            "parent_key_prefix": parent_key,
            "keys_match": keys_match,
        })),
    );
}
/// Read the actor-committed parent route captured atomically for this spawn.
/// ChatState contributes only live credentials; its conversation sampler
/// projection is never combined with a separately-read catalog identity.
async fn read_parent_sampling_config(
    ctx: &SubagentSpawnContext,
) -> (sampler::SamplerConfig, crate::agent::models::ModelId) {
    let mut inherited = ctx.sampling_config.clone();
    if let Some(ref chat_state) = ctx.delegation_chat_state {
        let creds = chat_state.get_credentials().await;
        inherited.api_key = creds.api_key;
        crate::agent::config::inject_url_derived_headers(
            &mut inherited.extra_headers,
            creds.alpha_test_key.as_deref(),
            &inherited.base_url,
        );
    }
    ::diagnostics::unified_log::debug(
        "subagent read actor-committed parent route",
        None,
        Some(serde_json::json!({
            "parent_model": &inherited.model,
            "parent_base_url": &inherited.base_url,
            "parent_key_prefix": key_prefix(&inherited.api_key),
            "session_model_id": ctx.model_id.0.as_ref(),
            "source": "session_model_route",
            "has_chat_state": ctx.delegation_chat_state.is_some(),
        })),
    );
    (inherited, ctx.model_id.clone())
}
/// Resolve an exact `provider/model` override to a
/// `(SamplerConfig, ModelId)` pair.
fn resolve_model_override_to_config(
    model_id: &str,
    ctx: &SubagentSpawnContext,
) -> Option<(sampler::SamplerConfig, crate::agent::models::ModelId)> {
    let entry =
        crate::agent::config::find_model_by_catalog_id(&ctx.available_models, model_id).cloned()?;
    let canonical_model_id = crate::agent::models::ModelId::new(model_id);
    let credentials = resolve_credentials(&entry);
    let config = sampling_config_for_model(&entry, credentials, ctx.alpha_test_key.clone());
    ::diagnostics::unified_log::debug(
        "subagent resolve_model_override_to_config",
        None,
        Some(serde_json::json!({
            "model_id": model_id,
            "canonical_model": canonical_model_id.0.as_ref(),
            "resolved_model_raw": &config.model,
            "base_url": &config.base_url,
            "key_prefix": key_prefix(&config.api_key),
            "has_own_credentials": entry.has_own_credentials(),
            "auth_method_id": ctx.auth_method_id.0.as_ref(),
        })),
    );
    Some((config, canonical_model_id))
}
/// Preserve the single stable System head on resume so the child's own work
/// remains compactable. A missing head is reported later by seed validation.
pub(crate) fn resume_inherited_prefix_len(
    conversation: &[sampling_types::conversation::ConversationItem],
) -> usize {
    usize::from(matches!(
        conversation.first(),
        Some(ConversationItem::System(_))
    ))
}
/// How a subagent's initial conversation was bootstrapped.
#[derive(Debug)]
struct InitialContext {
    source: InitialContextSource,
    source_ref: Option<chat_state::TimelineRangeRef>,
    prefix_len: Option<usize>,
    conversation: Vec<sampling_types::conversation::ConversationItem>,
    prompt_blobs: crate::session::persistence::ImmutablePromptBlobs,
    /// True only for a verbatim mirror-fork (parent conversation copied
    /// byte-for-byte before child-only runtime context is applied).
    verbatim_fork: bool,
}

/// Establish the child Timeline's stable System head before its seed is
/// persisted. Resumes and verbatim mirror-forks keep their original head;
/// new and normalized children receive the current child-audience head.
fn seed_child_system_head(
    source: &InitialContextSource,
    verbatim_fork: bool,
    conversation: &mut Vec<ConversationItem>,
    prefix_len: &mut Option<usize>,
    child_system_head: &str,
) -> Result<(), String> {
    if verbatim_fork && !matches!(source, InitialContextSource::Forked) {
        return Err("verbatim child context is not a fork".to_string());
    }
    let preserves_source_head = matches!(source, InitialContextSource::Resumed) || verbatim_fork;
    if preserves_source_head {
        return matches!(conversation.first(), Some(ConversationItem::System(_)))
            .then_some(())
            .ok_or_else(|| "inherited child Surface has no stable System head".to_string());
    }

    match conversation.first_mut() {
        Some(ConversationItem::System(system)) => {
            system.content = std::sync::Arc::<str>::from(child_system_head);
        }
        _ => {
            conversation.insert(0, ConversationItem::system(child_system_head.to_owned()));
            *prefix_len = Some(prefix_len.unwrap_or_default() + 1);
        }
    }
    Ok(())
}
/// Resume bootstrap: preserve only the System head (see `resume_inherited_prefix_len`).
fn resume_initial_context_with_ref(
    conversation: Vec<sampling_types::conversation::ConversationItem>,
    source_ref: chat_state::TimelineRangeRef,
) -> InitialContext {
    InitialContext {
        source: InitialContextSource::Resumed,
        source_ref: Some(source_ref),
        prefix_len: Some(resume_inherited_prefix_len(&conversation)),
        conversation,
        prompt_blobs: Default::default(),
        verbatim_fork: false,
    }
}
/// Apply `fork_filter_surface` then normalize. Empty or System-only input is
/// rejected because it cannot satisfy an explicit fork request.
fn forked_initial_context_with_ref(
    mut items: Vec<sampling_types::conversation::ConversationItem>,
    source_ref: chat_state::TimelineRangeRef,
) -> Result<InitialContext, String> {
    crate::session::storage::jsonl::fork_filter_surface(&mut items);
    if items.is_empty() {
        return Err("empty parent Surface".to_string());
    }
    let (conversation, prefix_len) =
        crate::agent::subagent::resolution::context::normalize_forked_context(items);
    if prefix_len < 2 {
        return Err("parent Surface has no inheritable content".to_string());
    }
    Ok(InitialContext {
        source: InitialContextSource::Forked,
        source_ref: Some(source_ref),
        prefix_len: Some(prefix_len),
        conversation,
        prompt_blobs: Default::default(),
        verbatim_fork: false,
    })
}
/// A verbatim mirror requires a coherent tail: the conversation must end on a
/// plain assistant text response (a clean turn boundary). A dangling assistant
/// (unanswered tool calls), a trailing ToolResult (mid-turn), or a trailing
/// user/reasoning means the prefix would be incoherent, so the caller falls back
/// to the summarized path instead of partial-trimming.
fn conversation_tail_is_complete(items: &[sampling_types::conversation::ConversationItem]) -> bool {
    matches!(
        items.last(),
        Some(ConversationItem::Assistant(a)) if a.tool_calls.is_empty()
    )
}
/// Decide the live-fork context.
///
/// Verbatim mirror (the cache-preserving path): when the parent fits the child
/// window (same 80% guard as resume) AND ends at a clean turn boundary, keep the
/// items BYTE-FOR-BYTE. We deliberately do NOT run `fork_filter_surface` here — its
/// step 1 strips synthetic-reason user items (`<system-reminder>`s, drained
/// monitor events, doom-loop warnings) that the parent actually sent and cached;
/// stripping them would diverge the child prefix at the first removed item and
/// cap radix reuse there. At planner spawn the conversation is between turns
/// (the `/goal` user message is not yet pushed), so the tail is already complete
/// and no trimming is needed; an incomplete tail falls back to summarized.
///
/// Summarized fallback (oversize OR incomplete tail): the reasoning-aware
/// `fork_filter_surface` drops synthetics + trims the incomplete tail, then
/// `normalize_forked_context` summarizes. (This is the ONLY path that filters;
/// the verbatim path never does.)
///
/// Input that is empty or only `System` item(s), before or after filtering, is
/// rejected rather than producing a hollow fork.
fn verbatim_or_normalize_fork_with_ref(
    items: Vec<sampling_types::conversation::ConversationItem>,
    child_context_window: u64,
    source_ref: chat_state::TimelineRangeRef,
) -> Result<InitialContext, String> {
    if !items
        .iter()
        .any(|i| !matches!(i, ConversationItem::System(_)))
    {
        return Err("parent Surface has no inheritable content".to_string());
    }
    let estimated_tokens = chat_state::estimate_conversation_tokens(&items);
    const SAFE_FORK_PERCENT: u64 = 80;
    let threshold = child_context_window * SAFE_FORK_PERCENT / 100;
    if estimated_tokens <= threshold && conversation_tail_is_complete(&items) {
        let prefix_len = items.len();
        return Ok(InitialContext {
            source: InitialContextSource::Forked,
            source_ref: Some(source_ref),
            prefix_len: Some(prefix_len),
            conversation: items,
            prompt_blobs: Default::default(),
            verbatim_fork: true,
        });
    }
    let mut filtered = items;
    crate::session::storage::jsonl::fork_filter_surface(&mut filtered);
    if !filtered
        .iter()
        .any(|i| !matches!(i, ConversationItem::System(_)))
    {
        return Err("parent Surface has no inheritable content after filtering".to_string());
    }
    let (conversation, prefix_len) =
        crate::agent::subagent::resolution::context::normalize_forked_context(filtered);
    Ok(InitialContext {
        source: InitialContextSource::Forked,
        source_ref: Some(source_ref),
        prefix_len: Some(prefix_len),
        conversation,
        prompt_blobs: Default::default(),
        verbatim_fork: false,
    })
}

fn freeze_initial_prompt_blobs(
    mut context: InitialContext,
    source_session: Option<&crate::session::storage::ContainedDirectory>,
) -> Result<InitialContext, String> {
    let references =
        crate::session::persistence::referenced_prompt_blob_hashes(&context.conversation)
            .map_err(|error| format!("invalid inherited prompt artifact reference: {error}"))?;
    if references.is_empty() {
        return Ok(context);
    }
    let source_session = source_session.ok_or_else(|| {
        "inherited Surface references prompt artifacts but its source session directory is unavailable"
            .to_string()
    })?;
    context.prompt_blobs = crate::session::persistence::freeze_prompt_blobs_from_directory(
        &context.conversation,
        source_session,
    )
    .map_err(|error| format!("cannot freeze inherited prompt artifacts: {error}"))?;
    Ok(context)
}
#[cfg(test)]
fn test_source_ref() -> chat_state::TimelineRangeRef {
    chat_state::TimelineRangeRef {
        timeline_id: "test-source".into(),
        first_seq: 0,
        last_seq: 0,
    }
}
#[cfg(test)]
fn resume_initial_context(
    conversation: Vec<sampling_types::conversation::ConversationItem>,
) -> InitialContext {
    resume_initial_context_with_ref(conversation, test_source_ref())
}
#[cfg(test)]
fn forked_initial_context(
    items: Vec<sampling_types::conversation::ConversationItem>,
) -> Result<InitialContext, String> {
    forked_initial_context_with_ref(items, test_source_ref())
}
#[cfg(test)]
fn verbatim_or_normalize_fork(
    items: Vec<sampling_types::conversation::ConversationItem>,
    child_context_window: u64,
) -> Result<InitialContext, String> {
    verbatim_or_normalize_fork_with_ref(items, child_context_window, test_source_ref())
}
/// `true` only when the fork actually summarized (ran `normalize_forked_context`).
/// A verbatim mirror-fork inherits items as-is and never normalizes, so it reports
/// `false` even though its source is `Forked`.
fn fork_context_normalized(source: &InitialContextSource, verbatim_fork: bool) -> bool {
    matches!(source, InitialContextSource::Forked) && !verbatim_fork
}
enum BootstrapInitialContext {
    Ready(InitialContext),
    /// The requested lineage could not be materialized — abort spawn.
    Abort(String),
}
/// Phase 3: resume > fork (live then disk) > new. Explicit lineage requests
/// fail closed; a child is never silently started with an empty Surface.
/// Unresolved non-empty resume is aborted by the caller before this runs.
async fn bootstrap_initial_context(
    request: &SubagentRequest,
    resume_source: Option<&DurableResumeSource>,
    ctx: &SubagentSpawnContext,
    child_context_window: u64,
) -> BootstrapInitialContext {
    if request.fork_context && request.resume_from.is_some() {
        tracing::info!(
            subagent_id = %request.id,
            resume_from = ?request.resume_from,
            resume_resolved = resume_source.is_some(),
            "resume_from and fork_context both set; resolved resume wins (fail-closed on copy error, never forks)"
        );
    }
    if let Some(resolved) = resume_source {
        let source = &resolved.data;
        let materialized = match resolved
            .session
            .materialize_timeline(&source.child_session_id)
        {
            Ok(materialized) if !materialized.surface.is_empty() => materialized,
            Ok(_) => {
                return BootstrapInitialContext::Abort(format!(
                    "Cannot resume from subagent '{}': source Surface is empty",
                    source.subagent_id,
                ));
            }
            Err(error) => {
                return BootstrapInitialContext::Abort(format!(
                    "Cannot resume from subagent '{}': failed to materialize source Timeline: \
                     {error}",
                    source.subagent_id,
                ));
            }
        };
        let conversation = materialized.surface;
        let estimated_tokens = chat_state::estimate_conversation_tokens(&conversation);
        const SAFE_RESUME_PERCENT: u64 = 80;
        let threshold = child_context_window * SAFE_RESUME_PERCENT / 100;
        if estimated_tokens > threshold {
            return BootstrapInitialContext::Abort(format!(
                "Cannot resume from subagent '{}': source transcript (~{estimated_tokens} \
                 tokens) exceeds {SAFE_RESUME_PERCENT}% of the model's context window \
                 ({child_context_window} tokens). The source conversation is too large for \
                 the current model.",
                source.subagent_id,
            ));
        }
        tracing::info!(
            subagent_id = %request.id,
            source_subagent = %source.subagent_id,
            surface_items = conversation.len(),
            estimated_tokens,
            "Materialized frozen resume source without publishing the child session"
        );
        return match freeze_initial_prompt_blobs(
            resume_initial_context_with_ref(conversation, materialized.input_ref),
            Some(resolved.session.directory()),
        ) {
            Ok(context) => BootstrapInitialContext::Ready(context),
            Err(error) => BootstrapInitialContext::Abort(error),
        };
    }
    if !request.fork_context {
        return BootstrapInitialContext::Ready(InitialContext {
            source: InitialContextSource::New,
            source_ref: None,
            prefix_len: None,
            conversation: vec![],
            prompt_blobs: Default::default(),
            verbatim_fork: false,
        });
    }
    let live_materialized = match ctx.delegation_chat_state.as_ref() {
        Some(chat_state) => {
            chat_state
                .materialize_timeline(ctx.security_parent_session_id.clone())
                .await
        }
        None => None,
    };
    if let Some(materialized) = live_materialized {
        let ctx_out = match verbatim_or_normalize_fork_with_ref(
            materialized.surface,
            child_context_window,
            materialized.input_ref,
        ) {
            Ok(context) => context,
            Err(error) => return BootstrapInitialContext::Abort(error),
        };
        let source_session = ctx.delegation_session_info.as_ref().and_then(|info| {
            crate::session::storage::jsonl::JsonlStorageAdapter::new()
                .open_session(info)
                .ok()
        });
        let ctx_out = match freeze_initial_prompt_blobs(
            ctx_out,
            source_session.as_ref().map(|session| session.directory()),
        ) {
            Ok(context) => context,
            Err(error) => return BootstrapInitialContext::Abort(error),
        };
        tracing::info!(
            subagent_id = %request.id,
            subagent_type = %request.subagent_type,
            loaded_items = ctx_out.conversation.len(),
            source = ?ctx_out.source,
            verbatim = ctx_out.verbatim_fork,
            "Forked context from live parent_chat_state"
        );
        return BootstrapInitialContext::Ready(ctx_out);
    }
    if let Some(ref parent_info) = ctx.delegation_session_info {
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
            crate::util::grow_home::grow_home(),
        );
        let parent_session = match storage.open_session(parent_info) {
            Ok(session) => session,
            Err(error) => {
                return BootstrapInitialContext::Abort(format!(
                    "Cannot fork parent session: source session could not be opened: {error}"
                ));
            }
        };
        let materialized =
            match parent_session.materialize_timeline(&ctx.security_parent_session_id) {
                Ok(materialized) => materialized,
                Err(error) => {
                    return BootstrapInitialContext::Abort(format!(
                        "Cannot fork parent session: source Timeline could not be materialized: \
                     {error}"
                    ));
                }
            };
        tracing::info!(
            subagent_id = %request.id,
            subagent_type = %request.subagent_type,
            surface_items = materialized.surface.len(),
            "Materialized frozen disk fork source without publishing the child session"
        );
        return match forked_initial_context_with_ref(materialized.surface, materialized.input_ref)
            .and_then(|context| {
                freeze_initial_prompt_blobs(context, Some(parent_session.directory()))
            }) {
            Ok(context) => BootstrapInitialContext::Ready(context),
            Err(error) => BootstrapInitialContext::Abort(error),
        };
    }
    BootstrapInitialContext::Abort(
        "Cannot fork parent session: parent Surface is unavailable".to_string(),
    )
}
/// Resolve the effective working directory for a child session.
///
/// Precedence: worktree path > `override_cwd` (non-empty) > parent cwd. The
/// caller selects `override_cwd`: a resumed child inherits the source's
/// effective cwd, a fresh spawn honors its `request.cwd`.
fn resolve_child_cwd(
    worktree_path: Option<&Path>,
    override_cwd: Option<&str>,
    parent_cwd: &Path,
) -> PathBuf {
    worktree_path
        .map(Path::to_path_buf)
        .or_else(|| override_cwd.filter(|s| !s.is_empty()).map(PathBuf::from))
        .unwrap_or_else(|| parent_cwd.to_path_buf())
}
/// The cwd a resumed child inherits from its source subagent, or `None` when
/// there is nothing to inherit (the caller then falls back to the parent cwd).
///
/// Only non-worktree sources inherit here — worktree-backed sources are reused
/// by the worktree path. The cwd is existence-checked because a source can be
/// pinned into a sibling's worktree that the snapshot stack later disposes;
/// resume otherwise skips cwd validation.
fn resume_inherited_cwd(source: Option<&ResumeSourceData>) -> Option<&str> {
    let source = source?;
    if source.worktree_path.is_some() || source.child_cwd.is_empty() {
        return None;
    }
    if !Path::new(&source.child_cwd).is_dir() {
        tracing::warn!(
            source_subagent_id = %source.subagent_id,
            child_cwd = %source.child_cwd,
            "Resume source cwd no longer exists; using parent workspace"
        );
        return None;
    }
    Some(source.child_cwd.as_str())
}
/// Select the cwd override for a child: a resume inherits the source's cwd
/// (never its own `request.cwd`); a fresh spawn uses `request.cwd`.
fn select_override_cwd<'a>(
    resume_source: Option<&'a ResumeSourceData>,
    request_cwd: Option<&'a str>,
) -> Option<&'a str> {
    if resume_source.is_some() {
        resume_inherited_cwd(resume_source)
    } else {
        request_cwd
    }
}
fn durable_resume_source_for(
    id: &str,
    parent_session_id: &str,
    requester_security_parent_session_id: &str,
) -> Option<DurableResumeSource> {
    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
        crate::util::grow_home::grow_home(),
    );
    // Lifecycle ownership is keyed by the root session id. The immediate
    // delegation cwd is a workspace confinement input, never part of that
    // durable identity; nested children may legitimately run in a subdir or
    // worktree while their spawn fact remains in the root Timeline.
    let parent = storage.open_session_by_id(parent_session_id).ok()??;
    let timeline = parent.validated_timeline(parent_session_id).ok()?.timeline;
    let (spawn_seq, spawn, terminal) = resume_source_facts_from_timeline(&timeline, id)?;
    if !resume_security_parent_allows(
        parent_session_id,
        requester_security_parent_session_id,
        &spawn.security_parent_session_id,
    ) {
        return None;
    }
    let child_session = storage.open_session_by_id(&spawn.child_session_id).ok()??;
    let summary = child_session.summary();
    if summary.parent_session_id.as_deref() != Some(parent_session_id)
        || !summary
            .session_kind
            .as_deref()
            .is_some_and(|kind| kind.starts_with("subagent"))
    {
        return None;
    }
    let child = child_session
        .validated_timeline(&spawn.child_session_id)
        .ok()?
        .timeline;
    child
        .validate_subagent_result_link(parent_session_id, spawn_seq, spawn, terminal)
        .ok()?;
    let mut data = resume_source_from_facts(spawn, terminal);
    data.child_cwd = summary.info.cwd.clone();
    Some(DurableResumeSource {
        data,
        session: child_session,
    })
}

fn resume_security_parent_allows(
    lifecycle_root_session_id: &str,
    requester_security_parent_session_id: &str,
    source_security_parent_session_id: &str,
) -> bool {
    requester_security_parent_session_id == lifecycle_root_session_id
        || requester_security_parent_session_id == source_security_parent_session_id
}

struct DurableResumeSource {
    data: ResumeSourceData,
    session: crate::session::storage::jsonl::OpenedSession,
}

impl std::ops::Deref for DurableResumeSource {
    type Target = ResumeSourceData;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

fn resume_source_facts_from_timeline<'a>(
    timeline: &'a chat_state::Timeline,
    id: &str,
) -> Option<(
    chat_state::EventSeq,
    &'a chat_state::SubagentSpawnEvent,
    &'a chat_state::SubagentTerminalEvent,
)> {
    let (spawn_seq, spawn) =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == id => Some((event.seq, spawn)),
                _ => None,
            })?;
    let terminal =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    terminal,
                )) if terminal.subagent_id == id => Some(terminal),
                _ => None,
            })?;
    Some((spawn_seq, spawn, terminal))
}

fn resume_source_from_facts(
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
) -> ResumeSourceData {
    ResumeSourceData {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        child_cwd: spawn.child_cwd.clone(),
        worktree_path: spawn.worktree_path.as_deref().map(PathBuf::from),
        snapshot_ref: terminal.snapshot_ref.clone(),
        subagent_type: spawn.subagent_type.clone(),
        model_id: spawn.effective_model_id.clone(),
        model_transport_key: spawn.model_transport_key.clone(),
        reasoning_effort: spawn.reasoning_effort,
    }
}

#[cfg(test)]
fn resume_source_from_timeline(
    timeline: &chat_state::Timeline,
    id: &str,
) -> Option<ResumeSourceData> {
    let (_, spawn, terminal) = resume_source_facts_from_timeline(timeline, id)?;
    Some(resume_source_from_facts(spawn, terminal))
}
/// Resolve the MCP pool a child subagent should import from its parent.
///
/// Inheritance applies to **every** agent source (built-in, user, project,
/// and plugin). Plugin agents are not excluded: the parent already connected
/// these servers for the session. Agent-owned `mcpServers` never enter this
/// path; child sessions may only reuse the parent's live client pool.
///
/// Returns `None` when there is no parent pool or `inheritance` is
/// [`McpInheritance::None`] (avoids an empty import call downstream).
fn resolve_inherited_mcp_pool(
    parent_pool: Option<crate::session::mcp_servers::SharedMcpPool>,
    inheritance: &agent::config::McpInheritance,
) -> Option<crate::session::mcp_servers::SharedMcpPool> {
    parent_pool.and_then(|pool| filter_pool_by_inheritance(pool, inheritance))
}
/// Apply `McpInheritance` filtering to a parent MCP pool snapshot.
///
/// Returns `None` for `McpInheritance::None` (no pool at all — avoids
/// an empty import call downstream). For `Named`/`Except`, retains or
/// removes the matching server names in-place.
fn filter_pool_by_inheritance(
    mut pool: crate::session::mcp_servers::SharedMcpPool,
    inheritance: &agent::config::McpInheritance,
) -> Option<crate::session::mcp_servers::SharedMcpPool> {
    if matches!(inheritance, McpInheritance::None) {
        return None;
    }
    let before = pool.server_names().count();
    pool.retain_clients(|server| inheritance.permits(server));
    tracing::debug!(
        before,
        after = pool.server_names().count(),
        ?inheritance,
        "MCP inheritance filter applied"
    );
    Some(pool)
}
/// Resolve a subagent type name to its `AgentDefinition`, with the parent
/// session's CLI tool/permission overrides already applied (so the spawn path
/// can never obtain a definition that skips them).
fn resolve_agent_definition(
    subagent_type: &str,
    ctx: &SubagentSpawnContext,
) -> Option<agent::config::AgentDefinition> {
    let cli_agents = ctx
        .agent_config
        .as_ref()
        .map(|config| config.cli_agents.as_slice())
        .unwrap_or_default();
    let resolution_context = crate::agent::subagent::resolution::DefinitionResolutionContext {
        cwd: &ctx.parent_cwd,
        plugins: ctx.plugin_registry.as_deref(),
        cli_agents,
        toggles: &ctx.subagent_toggle,
    };
    let mut def = crate::agent::subagent::resolution::discover_agent_definition(
        subagent_type,
        &resolution_context,
    )?;
    ctx.apply_session_cli_overrides(&mut def);
    Some(def)
}
fn available_agent_names(ctx: &SubagentSpawnContext) -> Vec<String> {
    let cli_agents = ctx
        .agent_config
        .as_ref()
        .map(|config| config.cli_agents.as_slice())
        .unwrap_or_default();
    crate::agent::subagent::resolution::available_agent_names(
        &crate::agent::subagent::resolution::DefinitionResolutionContext {
            cwd: &ctx.parent_cwd,
            plugins: ctx.plugin_registry.as_deref(),
            cli_agents,
            toggles: &ctx.subagent_toggle,
        },
    )
    .into_iter()
    .filter(|name| ctx.subagent_filter.allows(name))
    .collect()
}
/// Minimal per-session context for `validate_subagent_type`.
/// Avoids the heavy `SubagentSpawnContext` clone on the validation hot path.
#[derive(Default)]
pub(crate) struct SubagentValidationContext {
    pub parent_cwd: PathBuf,
    pub plugin_registry: Option<Arc<agent::plugins::PluginRegistry>>,
    pub subagent_toggle: HashMap<String, bool>,
    pub subagent_filter: agent::config::SubagentFilter,
    pub cli_agent_names: Vec<String>,
}
/// Synchronously validate a subagent type against discovery and the global toggle.
/// `Unknown { available }` is sorted by `str::cmp` for stable rendering.
pub(crate) fn validate_subagent_type(
    subagent_type: &str,
    ctx: &SubagentValidationContext,
) -> SubagentValidateTypeOutcome {
    if !ctx.subagent_filter.allows(subagent_type) {
        return SubagentValidateTypeOutcome::Disabled;
    }
    let context = crate::agent::subagent::resolution::DefinitionValidationContext {
        cwd: &ctx.parent_cwd,
        plugins: ctx.plugin_registry.as_deref(),
        cli_agent_names: &ctx.cli_agent_names,
        toggles: &ctx.subagent_toggle,
    };
    match crate::agent::subagent::resolution::validate_agent_name(subagent_type, &context) {
        Ok(()) => SubagentValidateTypeOutcome::Ok,
        Err(crate::agent::subagent::resolution::ResolutionError::Unknown { available, .. }) => {
            SubagentValidateTypeOutcome::Unknown {
                available: available
                    .into_iter()
                    .filter(|name| ctx.subagent_filter.allows(name))
                    .collect(),
            }
        }
        Err(crate::agent::subagent::resolution::ResolutionError::Disabled { .. }) => {
            SubagentValidateTypeOutcome::Disabled
        }
    }
}
/// Gate an already-resolved subagent type against the `[subagents.toggle]`
/// disable map.
///
/// The caller must have already confirmed the type resolves to an
/// `AgentDefinition`; this checks only the global toggle, returning `Ok` when
/// the type may run and `Disabled` otherwise. Shared by
/// [`run_shell_child`] and [`describe_subagent_type`] so both apply
/// identical gates.
fn gate_subagent_type(
    subagent_type: &str,
    ctx: &SubagentSpawnContext,
) -> SubagentValidateTypeOutcome {
    if !ctx.subagent_filter.allows(subagent_type) {
        return SubagentValidateTypeOutcome::Disabled;
    }
    let cli_agents = ctx
        .agent_config
        .as_ref()
        .map(|config| config.cli_agents.as_slice())
        .unwrap_or_default();
    let resolution_context = crate::agent::subagent::resolution::DefinitionResolutionContext {
        cwd: &ctx.parent_cwd,
        plugins: ctx.plugin_registry.as_deref(),
        cli_agents,
        toggles: &ctx.subagent_toggle,
    };
    match crate::agent::subagent::resolution::gate_agent_definition(
        subagent_type,
        &resolution_context,
    ) {
        Ok(()) => SubagentValidateTypeOutcome::Ok,
        Err(crate::agent::subagent::resolution::ResolutionError::Disabled { .. }) => {
            SubagentValidateTypeOutcome::Disabled
        }
        Err(crate::agent::subagent::resolution::ResolutionError::Unknown { .. }) => {
            SubagentValidateTypeOutcome::ValidationUnavailable
        }
    }
}
pub(crate) fn subagent_harness_flavor_is_representable(agent_type: &str) -> bool {
    crate::agent::subagent::resolution::subagent_harness_flavor_is_representable(agent_type)
}
/// Apply the harness-dependent toolset/prompt re-selection to a resolved
/// agent definition.
///
/// The child keeps the selected agent definition and inherits any configured
/// file-tool override.
///
/// Extracted so both [`run_shell_child`] (real spawn) and
/// [`describe_subagent_type`] (read-only probe) build the SAME `tool_config`
/// for a given `(subagent_type, parent_name)` — no
/// duplication.
fn resolve_subagent_toolset(
    subagent_type: &str,
    ctx: &SubagentSpawnContext,
    definition: &mut agent::config::AgentDefinition,
) {
    let resolution_context = crate::agent::subagent::resolution::HarnessToolsetContext {
        harness_override: None,
        parent_agent_name: ctx.parent_agent_name.as_deref(),
        file_tool_overrides: ctx.file_tool_overrides.as_deref(),
    };
    crate::agent::subagent::resolution::apply_harness_toolset(
        subagent_type,
        &resolution_context,
        definition,
    );
}
/// Resolve a subagent's turn limit: its own `maxTurns` wins, else inherit the parent's.
fn resolve_subagent_max_turns(
    definition_max_turns: Option<u32>,
    parent_max_turns: Option<usize>,
) -> Option<usize> {
    definition_max_turns
        .map(|v| v as usize)
        .or(parent_max_turns)
}
/// What to do with a resumed subagent's isolated worktree directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResumeWorktreeAction {
    /// Directory on disk and no snapshot ref — reuse it as-is.
    Reuse,
    /// Directory gone but a snapshot ref exists — rehydrate from it.
    Rehydrate,
    /// Directory gone and no snapshot — lineage cannot be resumed.
    Missing,
}
/// Decide how to recover a resumed subagent's worktree from its on-disk state
/// and whether a durable snapshot is available. Pure so the three outcomes are
/// unit-testable without git/async.
fn resume_worktree_action(dir_exists: bool, snapshot_ref: Option<&str>) -> ResumeWorktreeAction {
    if snapshot_ref.is_some() {
        ResumeWorktreeAction::Rehydrate
    } else if dir_exists {
        ResumeWorktreeAction::Reuse
    } else {
        ResumeWorktreeAction::Missing
    }
}
/// The immediate delegation parent's working directory is the only valid
/// source path for a nested subagent worktree. Lifecycle-root metadata must
/// never widen this workspace boundary.
fn parent_source_cwd(ctx: &SubagentSpawnContext) -> std::path::PathBuf {
    ctx.parent_cwd.clone()
}
/// Main repo root for a subagent's source: the durable repo a completion snapshot is transferred into and the repo a resume rehydrates from — both arms MUST resolve this identically.
fn resolve_subagent_source_repo(ctx: &SubagentSpawnContext) -> std::path::PathBuf {
    let source_cwd = parent_source_cwd(ctx);
    workspace::session::git::find_main_repo_root_from_path(&source_cwd).unwrap_or(source_cwd)
}
enum SubagentWaitOutcome {
    Cancelled,
    TurnResult(Box<Result<SubagentPromptTurnResult, oneshot::error::RecvError>>),
}
async fn await_subagent_turn_or_cancellation(
    prompt_rx: oneshot::Receiver<SubagentPromptTurnResult>,
    cancel_token: CancellationToken,
) -> SubagentWaitOutcome {
    tokio::select! {
        _ = cancel_token.cancelled() => SubagentWaitOutcome::Cancelled,
        turn_result = prompt_rx => SubagentWaitOutcome::TurnResult(Box::new(turn_result)),
    }
}
/// Fallback for cancelled/errored paths where TurnDeltaSnapshot is unavailable.
async fn signals_snapshot_counts(child_handle: &SessionHandle) -> (u32, u32) {
    child_handle
        .signals_handle
        .snapshot()
        .await
        .map(|s| (s.tool_call_count, s.turn_count))
        .unwrap_or((0, 0))
}
fn cancellation_error_message(
    category: Option<CancellationCategory>,
    context: Option<&crate::session::commands::CancellationContext>,
) -> String {
    let detail = context.and_then(|ctx| {
        let tool = ctx.tool_name.as_deref();
        let reason = ctx.reason.as_deref();
        let hook = ctx.hook_name.as_deref();
        match (tool, reason, hook) {
            (Some(t), Some(r), Some(h)) => Some(format!("{r} for tool `{t}` (hook: {h})")),
            (Some(t), Some(r), None) => Some(format!("{r} for tool `{t}`")),
            (Some(t), None, _) => Some(format!("tool `{t}`")),
            _ => None,
        }
    });
    match (category, &detail) {
        (Some(CancellationCategory::PermissionRejected), Some(d)) => {
            format!("Subagent turn was cancelled: user rejected permission — {d}")
        }
        (Some(CancellationCategory::PermissionRejected), None) => {
            "Subagent turn was cancelled: user rejected a permission prompt".to_string()
        }
        (Some(CancellationCategory::PermissionCancelled), _) => {
            "Subagent turn was cancelled: user cancelled a permission prompt".to_string()
        }
        (Some(CancellationCategory::PermissionTimedOut), Some(d)) => {
            format!("Subagent turn was cancelled: permission request timed out — {d}")
        }
        (Some(CancellationCategory::PermissionTimedOut), None) => {
            "Subagent turn was cancelled: permission request timed out".to_string()
        }
        (Some(CancellationCategory::HookDenied), Some(d)) => {
            format!("Subagent turn was cancelled: hook denied — {d}")
        }
        (Some(CancellationCategory::HookDenied), None) => {
            "Subagent turn was cancelled: blocked by a hook".to_string()
        }
        (Some(CancellationCategory::MidTurnAbort), _) => {
            "Subagent turn was cancelled: aborted mid-turn".to_string()
        }
        _ => "Subagent turn was cancelled".to_string(),
    }
}
fn failure_result(request: &SubagentRequest, error: &str) -> SubagentResult {
    SubagentResult {
        success: false,
        error: Some(error.to_string()),
        subagent_id: request.id.clone(),
        child_session_id: request.id.clone(),
        ..Default::default()
    }
}
fn cancelled_result(request: &SubagentRequest, error: &str) -> SubagentResult {
    SubagentResult {
        success: false,
        cancelled: true,
        error: Some(error.to_string()),
        subagent_id: request.id.clone(),
        child_session_id: request.id.clone(),
        ..Default::default()
    }
}
fn child_run_output(
    result: SubagentResult,
    completion_data: ShellCompletionData,
) -> ChildRunOutput<ShellCompletionData> {
    ChildRunOutput {
        result,
        completion_data,
    }
}
fn fail_subagent(
    error: &str,
    subagent_id: &str,
    child_session_id: &acp::SessionId,
    duration_ms: u64,
) -> SubagentResult {
    SubagentResult {
        success: false,
        error: Some(error.to_string()),
        subagent_id: subagent_id.to_string(),
        child_session_id: child_session_id.0.to_string(),
        duration_ms,
        ..Default::default()
    }
}

fn subagent_outcome(result: &SubagentResult) -> chat_state::SubagentOutcome {
    if result.success {
        chat_state::SubagentOutcome::Completed
    } else if result.cancelled {
        chat_state::SubagentOutcome::Cancelled
    } else {
        chat_state::SubagentOutcome::Failed
    }
}

fn subagent_result_fact(
    result: &SubagentResult,
    output_ref: Option<String>,
) -> chat_state::SubagentResultEvent {
    chat_state::SubagentResultEvent {
        subagent_id: result.subagent_id.clone(),
        outcome: subagent_outcome(result),
        duration_ms: result.duration_ms,
        tool_calls: result.tool_calls,
        turns: result.turns,
        tokens_used: result.total_tokens_used,
        error: result.error.clone(),
        output_ref,
    }
}

async fn record_child_result(
    child_chat_state: &chat_state::ChatStateHandle,
    result: &SubagentResult,
    output_ref: Option<String>,
) -> Result<chat_state::TimelineRangeRef, String> {
    let event = child_chat_state
        .record_timeline_event_durably(chat_state::TimelineEventKind::SubagentResult(
            subagent_result_fact(result, output_ref),
        ))
        .await
        .map_err(|error| format!("failed to commit child result: {error}"))?;
    Ok(chat_state::TimelineRangeRef {
        timeline_id: result.child_session_id.clone(),
        first_seq: event.seq.get(),
        last_seq: event.seq.get(),
    })
}

async fn record_child_result_with_persistence(
    persistence: &session::persistence::PersistenceHandle,
    timeline_events: Vec<chat_state::TimelineEvent>,
    result: &SubagentResult,
) -> Result<chat_state::TimelineRangeRef, String> {
    let mut timeline = chat_state::Timeline::from_events(timeline_events)
        .map_err(|error| format!("invalid child Timeline: {error}"))?;
    let event = timeline
        .record(chat_state::TimelineEventKind::SubagentResult(
            subagent_result_fact(result, None),
        ))
        .map_err(|error| format!("invalid child result: {error}"))?;
    persistence
        .append_timeline_event_durably(event.clone())
        .await
        .map_err(|error| format!("failed to persist child result: {error}"))?;
    Ok(chat_state::TimelineRangeRef {
        timeline_id: result.child_session_id.clone(),
        first_seq: event.seq.get(),
        last_seq: event.seq.get(),
    })
}

async fn record_parent_subagent_end(
    parent_chat_state: &chat_state::ChatStateHandle,
    result: &SubagentResult,
    result_ref: Option<chat_state::TimelineRangeRef>,
    snapshot_ref: Option<String>,
) -> Result<(), String> {
    parent_chat_state
        .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: result.subagent_id.clone(),
                child_session_id: result.child_session_id.clone(),
                outcome: subagent_outcome(result),
                duration_ms: result.duration_ms,
                tool_calls: result.tool_calls,
                turns: result.turns,
                tokens_used: result.total_tokens_used,
                error: result.error.clone(),
                result_ref,
                snapshot_ref,
            }),
        ))
        .await
        .map(|_| ())
        .map_err(|error| format!("failed to commit parent subagent terminal: {error}"))
}
/// Tear down a child whose pending-to-active promotion lost to cancellation.
async fn cancel_pending_shell_child(
    subagent_id: &str,
    child_session_id: &acp::SessionId,
    worktree_path: Option<&Path>,
    worktree_freshly_created: bool,
    duration_ms: u64,
) -> SubagentResult {
    if worktree_freshly_created
        && let Some(wt_path) = worktree_path
        && let Err(e) = crate::session::worktree::remove_subagent_worktree(wt_path).await
    {
        tracing::warn!(
            subagent_id,
            worktree_path = %wt_path.display(),
            error = %e,
            "failed to remove pristine worktree for killed-while-pending subagent"
        );
    }
    SubagentResult {
        success: false,
        cancelled: true,
        error: Some("Subagent was cancelled".to_string()),
        subagent_id: subagent_id.to_string(),
        child_session_id: child_session_id.0.to_string(),
        duration_ms,
        ..Default::default()
    }
}
fn emit_subagent_notification(
    gateway: &GatewaySender,
    parent_session_id: &str,
    update: SessionUpdate,
    parent_cmd_tx: Option<&mpsc::UnboundedSender<SessionCommand>>,
) {
    let mut meta = None;
    crate::util::event_id::ensure_event_id_meta(parent_session_id, &mut meta);
    let notification = SessionNotification {
        session_id: acp::SessionId::new(parent_session_id),
        update,
        meta: meta.map(serde_json::Value::Object),
    };
    if let Some(cmd_tx) = parent_cmd_tx {
        let _ = cmd_tx.send(SessionCommand::GrowSessionNotification {
            notification: notification.clone(),
        });
    }
    let params = serde_json::to_value(&notification)
        .and_then(|v| serde_json::value::to_raw_value(&v))
        .ok();
    if let Some(params) = params {
        let ext_notification =
            acp::ExtNotification::new("grow/session_notification", params.into());
        gateway.forward_fire_and_forget(ext_notification);
    }
}
/// Progress notification emission interval.
const PROGRESS_PUBLISH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
/// Change signature for the progress-publisher dedupe:
/// `(turn_count, tool_call_count, context_usage_pct, error_count, tokens_used)`.
///
/// `tokens_used` is part of the signature so rising child token spend always
/// publishes a tick: goal token accounting (subagent records, live totals,
/// and the turn-end budget check) keys off prompt token movement, which can
/// climb while turn/tool counts and the coarse context-usage *percent* bucket
/// stay flat. Omitting it would stall those updates until the heartbeat or an
/// unrelated field moved.
type ProgressSignature = (u32, u32, u8, u32, u64);
/// Whether a progress tick should be emitted given the previous and current
/// [`ProgressSignature`]s. Emits on any change, or when `heartbeat_due`
/// forces a keep-alive after an idle gap.
fn progress_tick_should_emit(
    prev: ProgressSignature,
    cur: ProgressSignature,
    heartbeat_due: bool,
) -> bool {
    cur != prev || heartbeat_due
}
/// Spawn a background task that periodically emits `SubagentProgress`
/// notifications on the parent session's notification channel.
///
/// The publisher samples the child's `SessionSignalsHandle` every
/// [`PROGRESS_PUBLISH_INTERVAL`] and emits a `SubagentProgress`
/// notification if the subagent is still running. It stops automatically
/// when `cancel_token` is cancelled (subagent completion/cancellation).
///
/// Notifications are **not** persisted to JSONL — they are transient UI
/// hints, not authoritative lifecycle events. The TUI can resync via
/// `grow/subagent/list_running` on reconnect.
fn spawn_progress_publisher(
    signals_handle: crate::session::signals::SessionSignalsHandle,
    gateway: GatewaySender,
    parent_session_id: String,
    subagent_id: String,
    child_session_id: String,
    started_at: std::time::Instant,
    cancel_token: tokio_util::sync::CancellationToken,
) {
    tokio::task::spawn_local(async move {
        let mut interval = tokio::time::interval(PROGRESS_PUBLISH_INTERVAL);
        interval.tick().await;
        let mut last_signature: ProgressSignature = (0, 0, 0, 0, 0);
        let mut last_emit_at = tokio::time::Instant::now();
        let heartbeat_max = tokio::time::Duration::from_secs(8);
        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => break,
                _ = interval.tick() => {}
            }
            let signals = match signals_handle.snapshot().await {
                Some(s) => s,
                None => break,
            };
            let sig: ProgressSignature = (
                signals.turn_count,
                signals.tool_call_count,
                signals.context_window_usage,
                signals.error_count,
                signals.context_tokens_used,
            );
            let heartbeat_due = last_emit_at.elapsed() >= heartbeat_max;
            if !progress_tick_should_emit(last_signature, sig, heartbeat_due) {
                continue;
            }
            last_signature = sig;
            last_emit_at = tokio::time::Instant::now();
            let duration_ms = started_at.elapsed().as_millis() as u64;
            let update = SessionUpdate::SubagentProgress {
                subagent_id: subagent_id.clone(),
                parent_session_id: parent_session_id.clone(),
                child_session_id: child_session_id.clone(),
                duration_ms,
                turn_count: signals.turn_count,
                tool_call_count: signals.tool_call_count,
                tokens_used: signals.context_tokens_used,
                context_window_tokens: signals.context_window_tokens,
                context_usage_pct: signals.context_window_usage,
                tools_used: signals.tools_used,
                error_count: signals.error_count,
            };
            let notification = SessionNotification {
                session_id: acp::SessionId::new(parent_session_id.clone()),
                update,
                meta: None,
            };
            let params = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
                .ok();
            if let Some(params) = params {
                let ext_notification =
                    acp::ExtNotification::new("grow/session_notification", params.into());
                gateway.forward_fire_and_forget(ext_notification);
            }
        }
    });
}
#[cfg(test)]
mod progress_publisher_tests {
    use super::{ProgressSignature, progress_tick_should_emit};
    const BASE: ProgressSignature = (3, 7, 12, 0, 30_000);
    #[test]
    fn token_only_change_emits() {
        let cur: ProgressSignature = (3, 7, 12, 0, 45_000);
        assert!(progress_tick_should_emit(BASE, cur, false));
    }
    #[test]
    fn unchanged_without_heartbeat_skips() {
        assert!(!progress_tick_should_emit(BASE, BASE, false));
    }
    #[test]
    fn heartbeat_forces_emit_when_unchanged() {
        assert!(progress_tick_should_emit(BASE, BASE, true));
    }
}
/// Borrowed output schema so persistence does not copy the text.
#[derive(serde::Serialize)]
struct SubagentOutputFileRef<'a> {
    schema_version: u32,
    output: &'a str,
}
const SUBAGENT_OUTPUT_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubagentOutputArtifact {
    timeline_ref: String,
}

fn write_subagent_output(
    session: &crate::session::storage::ContainedDirectory,
    output: &str,
) -> Result<SubagentOutputArtifact, String> {
    let file = SubagentOutputFileRef {
        schema_version: SUBAGENT_OUTPUT_SCHEMA_VERSION,
        output,
    };
    let json = match serde_json::to_string(&file) {
        Ok(json) => json,
        Err(e) => {
            tracing::warn!(error = %e, "failed to serialize subagent output");
            return Err(format!("failed to serialize subagent output: {e}"));
        }
    };
    let hash = blake3::hash(json.as_bytes()).to_hex().to_string();
    let relative = Path::new("artifacts")
        .join("subagent-output")
        .join(format!("{hash}.json"));
    if let Err(e) = crate::session::persistence::write_immutable_blob_to_directory(
        session,
        &relative,
        json.as_bytes(),
    ) {
        tracing::warn!(error = %e, "failed to write subagent output");
        return Err(format!("failed to write subagent output: {e}"));
    }
    Ok(SubagentOutputArtifact {
        timeline_ref: format!("artifact:subagent-output:blake3:{hash}"),
    })
}
fn read_subagent_output_from_directory(
    session: &crate::session::storage::ContainedDirectory,
    hash: &str,
) -> Option<String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OutputFile {
        schema_version: u32,
        output: String,
    }
    let directory = session
        .open_relative(
            Path::new("artifacts/subagent-output"),
            "subagent output artifact directory",
            false,
        )
        .ok()?;
    let file_name = format!("{hash}.json");
    let data = directory
        .read_bounded(
            std::ffi::OsStr::new(&file_name),
            "subagent output artifact",
            crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES,
        )
        .ok()?;
    if blake3::hash(&data).to_hex().as_str() != hash {
        return None;
    }
    let file: OutputFile = serde_json::from_slice(&data).ok()?;
    (file.schema_version == SUBAGENT_OUTPUT_SCHEMA_VERSION).then_some(file.output)
}
#[must_use]
fn persist_subagent_output(
    session: &crate::session::storage::ContainedDirectory,
    result: &SubagentResult,
) -> Result<Option<SubagentOutputArtifact>, String> {
    if !result.success || result.output.is_empty() {
        return Ok(None);
    }
    write_subagent_output(session, &result.output).map(Some)
}
const ORPHAN_RECONCILE_REASON: &str = "interrupted by process restart";

fn finish_from_terminal(
    terminal: &chat_state::SubagentTerminalEvent,
    output: Option<String>,
) -> SessionUpdate {
    let status = match terminal.outcome {
        chat_state::SubagentOutcome::Completed => "completed",
        chat_state::SubagentOutcome::Failed => "failed",
        chat_state::SubagentOutcome::Cancelled => "cancelled",
    };
    SessionUpdate::SubagentFinished {
        subagent_id: terminal.subagent_id.clone(),
        child_session_id: terminal.child_session_id.clone(),
        status: status.to_string(),
        error: terminal.error.clone(),
        tool_calls: terminal.tool_calls,
        turns: terminal.turns,
        duration_ms: terminal.duration_ms,
        tokens_used: terminal.tokens_used,
        output,
    }
}

pub(crate) fn load_subagent_output_ref_from_directory(
    child: &crate::session::storage::ContainedDirectory,
    output_ref: &str,
) -> Result<String, String> {
    const PREFIX: &str = "artifact:subagent-output:blake3:";
    let hash = output_ref
        .strip_prefix(PREFIX)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| format!("invalid subagent output reference: {output_ref}"))?;
    read_subagent_output_from_directory(child, hash)
        .ok_or_else(|| format!("subagent output artifact is missing or corrupt: {output_ref}"))
}

fn validate_child_session_identity(
    opened: &crate::session::storage::jsonl::OpenedSession,
    parent_session_id: &str,
    spawn: &chat_state::SubagentSpawnEvent,
) -> Result<(), String> {
    let summary = opened.summary();
    if summary.info.id.0.as_ref() != spawn.child_session_id
        || summary.parent_session_id.as_deref() != Some(parent_session_id)
        || !summary
            .session_kind
            .as_deref()
            .is_some_and(|kind| kind.starts_with("subagent"))
    {
        return Err(format!(
            "child session '{}' identity does not match parent spawn",
            spawn.child_session_id
        ));
    }
    Ok(())
}

fn finish_from_durable_facts(
    parent_timeline_id: &str,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
) -> Result<SessionUpdate, String> {
    if terminal.result_ref.is_none() {
        return Ok(finish_from_terminal(terminal, None));
    }
    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
    let opened = storage
        .open_session_by_id(&spawn.child_session_id)
        .map_err(|error| format!("cannot resolve child session: {error}"))?
        .ok_or_else(|| "cannot resolve child session: session is missing".to_string())
        .map_err(|error| format!("cannot resolve child session: {error}"))?;
    validate_child_session_identity(&opened, parent_timeline_id, spawn)?;
    let child = opened
        .validated_timeline(&spawn.child_session_id)
        .map_err(|error| format!("cannot validate child session entity: {error}"))?
        .timeline;
    let result = child
        .validate_subagent_result_link(parent_timeline_id, spawn_seq, spawn, terminal)
        .map_err(|error| format!("invalid child result link: {error}"))?;
    let output = result
        .output_ref
        .as_deref()
        .map(|output_ref| load_subagent_output_ref_from_directory(opened.directory(), output_ref))
        .transpose()?;
    Ok(finish_from_terminal(terminal, output))
}

pub(crate) enum DurableChildOperation {
    Missing,
    Open,
    Completed(SubagentResult),
}

pub(crate) async fn durable_child_operation(
    parent_timeline_id: &str,
    subagent_id: &str,
    parent: &chat_state::ChatStateHandle,
) -> Result<DurableChildOperation, String> {
    let events = parent
        .timeline_events()
        .await
        .ok_or_else(|| "cannot query parent Timeline".to_string())?;
    let timeline = chat_state::Timeline::from_events(events)
        .map_err(|error| format!("invalid parent Timeline: {error}"))?;
    let Some((spawn_seq, spawn)) =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == subagent_id => Some((event.seq, spawn)),
                _ => None,
            })
    else {
        return Ok(DurableChildOperation::Missing);
    };
    let Some(terminal) =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    terminal,
                )) if terminal.subagent_id == subagent_id => Some(terminal),
                _ => None,
            })
    else {
        return Ok(DurableChildOperation::Open);
    };

    let (output, outcome, error) = if terminal.result_ref.is_some() {
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
        let child = storage
            .open_session_by_id(&spawn.child_session_id)
            .map_err(|error| format!("cannot resolve child session: {error}"))?
            .ok_or_else(|| "canonical child session is missing".to_string())?;
        validate_child_session_identity(&child, parent_timeline_id, spawn)?;
        let child_timeline = child
            .validated_timeline(&spawn.child_session_id)
            .map_err(|error| format!("invalid child session entity: {error}"))?
            .timeline;
        let result = child_timeline
            .validate_subagent_result_link(parent_timeline_id, spawn_seq, spawn, terminal)
            .map_err(|error| format!("invalid child result link: {error}"))?;
        let output = result
            .output_ref
            .as_deref()
            .map(|output_ref| {
                load_subagent_output_ref_from_directory(child.directory(), output_ref)
            })
            .transpose()?;
        (
            output.unwrap_or_default(),
            result.outcome,
            result.error.clone(),
        )
    } else {
        if terminal.outcome == chat_state::SubagentOutcome::Completed {
            return Err("completed child terminal has no canonical result reference".into());
        }
        (String::new(), terminal.outcome, terminal.error.clone())
    };
    Ok(DurableChildOperation::Completed(SubagentResult {
        success: outcome == chat_state::SubagentOutcome::Completed,
        output: std::sync::Arc::from(output),
        error,
        cancelled: outcome == chat_state::SubagentOutcome::Cancelled,
        subagent_id: terminal.subagent_id.clone(),
        child_session_id: terminal.child_session_id.clone(),
        tool_calls: terminal.tool_calls,
        turns: terminal.turns,
        duration_ms: terminal.duration_ms,
        tokens_used: terminal.tokens_used,
        total_tokens_used: terminal.tokens_used,
        ..Default::default()
    }))
}

#[cfg(test)]
fn finish_from_durable_facts_in_directory(
    parent_timeline_id: &str,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
    child_directory: &crate::session::storage::ContainedDirectory,
) -> Result<SessionUpdate, String> {
    let events = crate::session::storage::read_committed_jsonl_from_directory(
        child_directory,
        std::ffi::OsStr::new(crate::session::storage::TIMELINE_FILE),
        "child Timeline ledger",
        crate::session::storage::MAX_JSONL_ENTRY_BYTES,
    )
    .map_err(|error| format!("cannot validate child Timeline: {error}"))?;
    let child = chat_state::Timeline::from_events(events)
        .map_err(|error| format!("cannot validate child Timeline: {error}"))?;
    let result = child
        .validate_subagent_result_link(parent_timeline_id, spawn_seq, spawn, terminal)
        .map_err(|error| format!("invalid child result link: {error}"))?;
    let output = result
        .output_ref
        .as_deref()
        .map(|output_ref| load_subagent_output_ref_from_directory(child_directory, output_ref))
        .transpose()?;
    Ok(finish_from_terminal(terminal, output))
}

fn spawn_from_fact(
    parent_session_id: &str,
    spawn: &chat_state::SubagentSpawnEvent,
    workflow_tracker: Option<
        &std::sync::Arc<parking_lot::Mutex<crate::session::workflow::tracker::WorkflowTracker>>,
    >,
) -> SessionUpdate {
    let context_source = match spawn.context_source {
        chat_state::SubagentContextSource::New => "new",
        chat_state::SubagentContextSource::Forked => "forked",
        chat_state::SubagentContextSource::Resumed => "resumed",
    };
    let workflow_projection = spawn.workflow_run_id.as_deref().and_then(|run_id| {
        let tracker = workflow_tracker?.lock();
        let route = &tracker.get(run_id)?.runtime_route;
        let model_state = route
            .model_state_for(&spawn.effective_model_id, spawn.reasoning_effort)
            .ok()?;
        Some((model_state, route.frozen_agent_names()))
    });
    let (model_state, workflow_agent_names) = workflow_projection.unzip();
    SessionUpdate::SubagentSpawned {
        subagent_id: spawn.subagent_id.clone(),
        child_session_id: spawn.child_session_id.clone(),
        parent_session_id: parent_session_id.to_string(),
        parent_prompt_id: spawn.parent_prompt_id.clone(),
        subagent_type: spawn.subagent_type.clone(),
        description: spawn.description.clone(),
        effective_context_source: Some(context_source.to_string()),
        context_normalized: spawn.context_normalized,
        capability_mode: spawn.capability_mode.clone(),
        permission_mode: spawn.permission_mode.clone(),
        effective_permission_mode: spawn.effective_permission_mode.clone(),
        model: Some(spawn.effective_model_id.clone()),
        model_state,
        workflow_agent_names,
        resumed_from: spawn.resumed_from.clone(),
        workflow_run_id: spawn.workflow_run_id.clone(),
        goal_id: spawn.goal_id.clone(),
    }
}

#[derive(Debug, Clone)]
struct RecoveredInspectionResult {
    event: chat_state::SubagentResultEvent,
    output: Option<String>,
}

fn result_from_inspection(
    spawn: &chat_state::SubagentSpawnEvent,
    inspection: Option<&SubagentInspection>,
    duration_ms: u64,
) -> RecoveredInspectionResult {
    let observed_duration_ms = inspection
        .map(|inspection| inspection.snapshot.duration_ms)
        .unwrap_or(duration_ms);
    let (outcome, error, tool_calls, turns, tokens_used, output) =
        match inspection.map(|value| &value.snapshot.status) {
            Some(SubagentSnapshotStatus::Completed {
                output,
                tool_calls,
                turns,
                tokens_used,
                ..
            }) => (
                chat_state::SubagentOutcome::Completed,
                None,
                *tool_calls,
                *turns,
                *tokens_used,
                (!output.is_empty()).then(|| output.clone()),
            ),
            Some(SubagentSnapshotStatus::CompletedOutputUnavailable { .. }) => {
                unreachable!("unverifiable completed outputs are filtered before recovery")
            }
            Some(SubagentSnapshotStatus::Failed { error }) => (
                chat_state::SubagentOutcome::Failed,
                Some(error.clone()),
                0,
                0,
                0,
                None,
            ),
            Some(SubagentSnapshotStatus::Cancelled { reason }) => (
                chat_state::SubagentOutcome::Cancelled,
                Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| ORPHAN_RECONCILE_REASON.to_string()),
                ),
                0,
                0,
                0,
                None,
            ),
            Some(SubagentSnapshotStatus::Initializing | SubagentSnapshotStatus::Running { .. }) => {
                unreachable!("running inspections are filtered before recovery")
            }
            None => (
                chat_state::SubagentOutcome::Cancelled,
                Some(ORPHAN_RECONCILE_REASON.to_string()),
                0,
                0,
                0,
                None,
            ),
        };
    RecoveredInspectionResult {
        event: chat_state::SubagentResultEvent {
            subagent_id: spawn.subagent_id.clone(),
            outcome,
            duration_ms: observed_duration_ms,
            tool_calls,
            turns,
            tokens_used,
            error,
            output_ref: None,
        },
        output,
    }
}

#[derive(Debug, thiserror::Error)]
enum ChildResultRecoveryError {
    #[error("child result was never published: {0}")]
    Unpublished(String),
    #[error("child result cannot be trusted: {0}")]
    Invalid(String),
}

async fn ensure_recovered_child_result(
    parent_timeline_id: &str,
    parent_spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    fallback: RecoveredInspectionResult,
) -> Result<
    (
        chat_state::TimelineRangeRef,
        chat_state::SubagentResultEvent,
        Option<String>,
    ),
    ChildResultRecoveryError,
> {
    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
    let opened = storage
        .open_session_by_id(&spawn.child_session_id)
        .map_err(|error| {
            ChildResultRecoveryError::Invalid(format!("cannot open child session: {error}"))
        })?
        .ok_or_else(|| {
            ChildResultRecoveryError::Unpublished(format!(
                "child session {} does not exist",
                spawn.child_session_id
            ))
        })?;
    ensure_recovered_child_result_with_opened(
        parent_timeline_id,
        parent_spawn_seq,
        spawn,
        fallback,
        &opened,
        &storage,
    )
    .await
}

async fn ensure_recovered_child_result_in_dir(
    parent_timeline_id: &str,
    parent_spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    fallback: RecoveredInspectionResult,
    child_dir: &Path,
) -> Result<
    (
        chat_state::TimelineRangeRef,
        chat_state::SubagentResultEvent,
        Option<String>,
    ),
    ChildResultRecoveryError,
> {
    let child_info = SessionInfo {
        id: acp::SessionId::new(spawn.child_session_id.clone()),
        cwd: spawn.child_cwd.clone(),
    };
    let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_explicit_session_dir(
        child_dir.to_path_buf(),
    );
    let opened = storage.open_session(&child_info).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ChildResultRecoveryError::Unpublished(format!(
                "child session {} does not exist",
                spawn.child_session_id
            ))
        } else {
            ChildResultRecoveryError::Invalid(format!("cannot open child session: {error}"))
        }
    })?;
    ensure_recovered_child_result_with_opened(
        parent_timeline_id,
        parent_spawn_seq,
        spawn,
        fallback,
        &opened,
        &storage,
    )
    .await
}

async fn ensure_recovered_child_result_with_opened(
    parent_timeline_id: &str,
    parent_spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    fallback: RecoveredInspectionResult,
    opened: &crate::session::storage::jsonl::OpenedSession,
    storage: &crate::session::storage::jsonl::JsonlStorageAdapter,
) -> Result<
    (
        chat_state::TimelineRangeRef,
        chat_state::SubagentResultEvent,
        Option<String>,
    ),
    ChildResultRecoveryError,
> {
    validate_child_session_identity(opened, parent_timeline_id, spawn)
        .map_err(ChildResultRecoveryError::Invalid)?;
    let mut timeline = opened
        .validated_timeline(&spawn.child_session_id)
        .map_err(|error| {
            ChildResultRecoveryError::Invalid(format!("invalid child session entity: {error}"))
        })?
        .timeline;
    timeline
        .validate_subagent_seed_link(parent_timeline_id, parent_spawn_seq, spawn)
        .map_err(|error| {
            ChildResultRecoveryError::Invalid(format!("invalid child seed link: {error}"))
        })?;
    if let Some((event, result)) = timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            chat_state::TimelineEventKind::SubagentResult(result) => Some((event, result)),
            _ => None,
        })
    {
        let output = result
            .output_ref
            .as_deref()
            .map(|output_ref| {
                load_subagent_output_ref_from_directory(opened.directory(), output_ref)
            })
            .transpose()
            .map_err(ChildResultRecoveryError::Invalid)?;
        return Ok((
            chat_state::TimelineRangeRef {
                timeline_id: spawn.child_session_id.clone(),
                first_seq: event.seq.get(),
                last_seq: event.seq.get(),
            },
            result.clone(),
            output,
        ));
    }
    let mut fallback_event = fallback.event;
    if fallback_event.outcome == chat_state::SubagentOutcome::Completed
        && let Some(output) = fallback.output.as_deref()
    {
        let artifact = write_subagent_output(opened.directory(), output)
            .map_err(ChildResultRecoveryError::Invalid)?;
        fallback_event.output_ref = Some(artifact.timeline_ref);
    }
    let event = timeline
        .record(chat_state::TimelineEventKind::SubagentResult(
            fallback_event.clone(),
        ))
        .map_err(|error| {
            ChildResultRecoveryError::Invalid(format!("invalid recovered child result: {error}"))
        })?;
    crate::session::storage::StorageAdapter::append_timeline_event_durable(
        storage,
        &opened.summary().info,
        &event,
    )
    .await
    .map_err(|error| {
        ChildResultRecoveryError::Invalid(format!("cannot persist recovered child result: {error}"))
    })?;
    Ok((
        chat_state::TimelineRangeRef {
            timeline_id: spawn.child_session_id.clone(),
            first_seq: event.seq.get(),
            last_seq: event.seq.get(),
        },
        fallback_event,
        fallback.output,
    ))
}

fn subagent_notification_owner(
    spawn: &chat_state::SubagentSpawnEvent,
) -> chat_state::NotificationOwner {
    match (spawn.goal_id.as_ref(), spawn.goal_definition_revision) {
        (None, None) => chat_state::NotificationOwner::Session,
        (Some(goal_id), Some(definition_revision)) if definition_revision > 0 => {
            chat_state::NotificationOwner::Goal {
                goal_id: goal_id.clone(),
                definition_revision,
            }
        }
        _ => unreachable!("persisted Subagent spawn has an incomplete Goal owner"),
    }
}

fn completion_receipt_body_from_durable_facts(
    parent_timeline_id: &str,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
) -> Result<String, String> {
    let update = finish_from_durable_facts(parent_timeline_id, spawn_seq, spawn, terminal)?;
    let SessionUpdate::SubagentFinished { output, .. } = update else {
        unreachable!("durable subagent facts must produce a finished update");
    };
    let summary = tools::implementations::grow_build::task::types::SubagentCompletionSummary {
        subagent_id: terminal.subagent_id.clone(),
        subagent_type: spawn.subagent_type.clone(),
        description: spawn.description.clone(),
        success: terminal.outcome == chat_state::SubagentOutcome::Completed,
        duration_ms: terminal.duration_ms,
        tool_calls: terminal.tool_calls,
        turns: terminal.turns,
        output: std::sync::Arc::from(output.unwrap_or_default()),
    };
    Ok(tools::reminders::task_completion::format_subagent_completion(&summary, None))
}

/// Repair the crash window between the durable parent terminal and the
/// durable completion receipt. This deliberately goes through the same
/// SessionCommand admission used by live child completion; the Timeline's
/// source/version fold makes the repair idempotent.
async fn repair_subagent_completion_receipt(
    parent_session_id: &str,
    parent_chat_state: &chat_state::ChatStateHandle,
    parent_cmd_tx: Option<&mpsc::UnboundedSender<SessionCommand>>,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
) {
    if !spawn.surface_completion || terminal.outcome == chat_state::SubagentOutcome::Cancelled {
        return;
    }
    let Some(cmd_tx) = parent_cmd_tx else {
        return;
    };
    let owner = subagent_notification_owner(spawn);
    let source = chat_state::NotificationSource::SubagentCompleted {
        subagent_id: terminal.subagent_id.clone(),
        owner: owner.clone(),
    };
    let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
    match parent_chat_state
        .received_notification_id(source, source_version)
        .await
    {
        Some(Some(_)) => return,
        None => return,
        Some(None) => {}
    }
    let body = match completion_receipt_body_from_durable_facts(
        parent_session_id,
        spawn_seq,
        spawn,
        terminal,
    ) {
        Ok(body) => body,
        Err(error) => {
            tracing::error!(
                subagent_id = %terminal.subagent_id,
                %error,
                "refusing to repair an unverified subagent completion receipt"
            );
            return;
        }
    };
    if !admit_notification_receipt(cmd_tx, &terminal.subagent_id, owner, body).await {
        tracing::warn!(
            subagent_id = %terminal.subagent_id,
            "parent mailbox closed before subagent completion receipt repair"
        );
    }
}

/// Reconcile parent spawn facts that have no terminal. The backend is merely
/// an observation source: recovery first commits a child result (when the child
/// entity exists), then closes the parent spawn, and only then emits UI state.
pub(crate) async fn reconcile_orphaned_subagents_with_backend(
    projections: &crate::session::storage::SubagentProjectionState,
    emit_replay_projections: bool,
    backend: &tools::implementations::grow_build::task::backend::ChannelBackend,
    parent_session_id: &str,
    parent_chat_state: &chat_state::ChatStateHandle,
    workflow_tracker: Option<
        &std::sync::Arc<parking_lot::Mutex<crate::session::workflow::tracker::WorkflowTracker>>,
    >,
    gateway: &GatewaySender,
    parent_cmd_tx: Option<&mpsc::UnboundedSender<SessionCommand>>,
) {
    let Some(events) = parent_chat_state.timeline_events().await else {
        tracing::error!(%parent_session_id, "cannot query parent Timeline during subagent recovery");
        return;
    };
    let Ok(timeline) = chat_state::Timeline::from_events(events) else {
        return;
    };
    let mut spawns = std::collections::BTreeMap::<
        String,
        (chat_state::EventSeq, i64, chat_state::SubagentSpawnEvent),
    >::new();
    let mut terminals =
        std::collections::BTreeMap::<String, chat_state::SubagentTerminalEvent>::new();
    for event in timeline.events() {
        match &event.kind {
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) => {
                spawns.insert(
                    spawn.subagent_id.clone(),
                    (event.seq, event.at_ms, spawn.clone()),
                );
            }
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(terminal)) => {
                terminals.insert(terminal.subagent_id.clone(), terminal.clone());
            }
            _ => {}
        }
    }

    if emit_replay_projections {
        for (_, _, spawn) in spawns.values() {
            if !projections.spawned.contains(&spawn.subagent_id) {
                emit_subagent_notification(
                    gateway,
                    parent_session_id,
                    spawn_from_fact(parent_session_id, spawn, workflow_tracker),
                    parent_cmd_tx,
                );
            }
        }
        for (subagent_id, terminal) in &terminals {
            if !projections.finished.contains(&terminal.subagent_id) {
                let Some((spawn_seq, _, spawn)) = spawns.get(subagent_id) else {
                    tracing::error!(%subagent_id, "parent terminal has no spawn fact");
                    continue;
                };
                match finish_from_durable_facts(parent_session_id, *spawn_seq, spawn, terminal) {
                    Ok(update) => emit_subagent_notification(
                        gateway,
                        parent_session_id,
                        update,
                        parent_cmd_tx,
                    ),
                    Err(error) => tracing::error!(
                        %subagent_id,
                        %error,
                        "refusing to replay an unverified subagent result"
                    ),
                }
            }
        }
    }

    // A process may have committed the parent terminal and died before the
    // live completion producer received its receipt acknowledgement. Replay
    // the durable model reminder independently of the UI projection ledger.
    for (subagent_id, terminal) in &terminals {
        let Some((spawn_seq, _, spawn)) = spawns.get(subagent_id) else {
            tracing::error!(%subagent_id, "parent terminal has no spawn fact");
            continue;
        };
        repair_subagent_completion_receipt(
            parent_session_id,
            parent_chat_state,
            parent_cmd_tx,
            *spawn_seq,
            spawn,
            terminal,
        )
        .await;
    }

    for (subagent_id, (spawn_seq, spawned_at_ms, spawn)) in spawns {
        if terminals.contains_key(&subagent_id) {
            continue;
        }
        let inspection = backend.inspect(&subagent_id).await;
        if let Some(inspection) = &inspection
            && (inspection.parent_session_id != parent_session_id
                || inspection.child_session_id != spawn.child_session_id
                || inspection.snapshot.subagent_id != subagent_id)
        {
            tracing::error!(
                %subagent_id,
                expected_parent_session_id = %parent_session_id,
                actual_parent_session_id = %inspection.parent_session_id,
                expected_child_session_id = %spawn.child_session_id,
                actual_child_session_id = %inspection.child_session_id,
                actual_subagent_id = %inspection.snapshot.subagent_id,
                "backend inspection identity mismatch; leaving parent spawn open"
            );
            continue;
        }
        if inspection.as_ref().is_some_and(|inspection| {
            matches!(
                inspection.snapshot.status,
                SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
            )
        }) {
            tracing::error!(
                %subagent_id,
                "completed backend output is unavailable or invalid; leaving parent spawn open"
            );
            continue;
        }
        if inspection
            .as_ref()
            .is_some_and(|inspection| inspection.snapshot.is_running())
        {
            continue;
        }
        let duration_ms = chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub(spawned_at_ms)
            .max(0) as u64;
        let fallback = result_from_inspection(&spawn, inspection.as_ref(), duration_ms);
        let recovered =
            ensure_recovered_child_result(parent_session_id, spawn_seq, &spawn, fallback.clone())
                .await;
        let (result_ref, result, output) = match recovered {
            Ok((result_ref, result, output)) => (Some(result_ref), result, output),
            Err(ChildResultRecoveryError::Unpublished(error))
                if fallback.event.outcome != chat_state::SubagentOutcome::Completed =>
            {
                tracing::warn!(%subagent_id, %error, "closing an unpublished child without a result reference");
                (None, fallback.event, None)
            }
            Err(error) => {
                tracing::error!(
                    %subagent_id,
                    %error,
                    "child result cannot be proven; leaving parent spawn open"
                );
                continue;
            }
        };
        let terminal = chat_state::SubagentTerminalEvent {
            subagent_id: subagent_id.clone(),
            child_session_id: spawn.child_session_id.clone(),
            outcome: result.outcome,
            duration_ms: result.duration_ms,
            tool_calls: result.tool_calls,
            turns: result.turns,
            tokens_used: result.tokens_used,
            error: result.error,
            result_ref,
            snapshot_ref: None,
        };
        match parent_chat_state
            .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(terminal.clone()),
            ))
            .await
        {
            Ok(_) => {
                repair_subagent_completion_receipt(
                    parent_session_id,
                    parent_chat_state,
                    parent_cmd_tx,
                    spawn_seq,
                    &spawn,
                    &terminal,
                )
                .await;
                if emit_replay_projections {
                    emit_subagent_notification(
                        gateway,
                        parent_session_id,
                        finish_from_terminal(&terminal, output),
                        parent_cmd_tx,
                    );
                }
            }
            Err(error) => tracing::error!(
                %subagent_id,
                %error,
                "failed to commit recovered parent subagent terminal"
            ),
        }
    }
    if let Err(error) = parent_chat_state.recover_interrupted_durably().await {
        tracing::error!(
            %error,
            "failed to close local recovery scopes after subagent reconciliation"
        );
    }
}
#[cfg(test)]
mod tests;
